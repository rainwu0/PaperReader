// 畫面一側的離線引擎（Firefox 翻譯，Bergamot）：外殼的翻譯工作把一批字交過來，這裡用背景工作者翻好交回。
// 外殼不能直接呼叫網頁，所以這裡一直留著一個「等工作」的請求（`ondevice_next`，最長等 20 秒，交回後立刻再問）。
// 工作的推進、分批、自動檢查、寫檔都在外殼（`src-tauri/src/ondevice.rs`），這裡只負責翻。

import { api, AppError } from "../api";

// 引擎（WASM）是程式碼，跟著安裝檔走；下載的只有模型
const ENGINE_URL = "/bergamot/bergamot-translator.wasm";
const MODEL_FILES = { model: "model.bin", lex: "lex.bin", srcvocab: "srcvocab.spm", trgvocab: "trgvocab.spm" };
/** 閒置多久就關掉背景工作者，把模型占的記憶體（約 60 MB）還給手機。 */
const IDLE_MS = 120_000;

type WorkerReply = { type: "loaded" } | { type: "result"; texts: string[] } | { type: "error"; message: string };

let worker: Worker | null = null;
let loadedPair: string | null = null;
let idleTimer: number | undefined;
let started = false;

function ask(w: Worker, msg: unknown, transfer: Transferable[] = []): Promise<WorkerReply> {
  return new Promise((resolve) => {
    w.onmessage = (e: MessageEvent<WorkerReply>) => resolve(e.data);
    w.onerror = (e) => resolve({ type: "error", message: e.message || "worker error" });
    w.postMessage(msg, transfer);
  });
}

function stopWorker() {
  worker?.terminate();
  worker = null;
  loadedPair = null;
}

/** 確認背景工作者已載入這個語言組合的模型。 */
async function ensure(pair: string): Promise<Worker> {
  if (worker && loadedPair === pair) return worker;
  if (!worker) worker = new Worker("/bergamot/worker.js");
  const [wasm, model, lex, srcvocab, trgvocab] = await Promise.all([
    fetch(ENGINE_URL).then((r) => {
      if (!r.ok) throw new Error("engine missing");
      return r.arrayBuffer();
    }),
    api.bergamotFile(pair, MODEL_FILES.model),
    api.bergamotFile(pair, MODEL_FILES.lex),
    api.bergamotFile(pair, MODEL_FILES.srcvocab),
    api.bergamotFile(pair, MODEL_FILES.trgvocab),
  ]);
  const [source, target] = [pair.slice(0, pair.indexOf("-")), pair.slice(pair.indexOf("-") + 1)];
  const r = await ask(
    worker,
    { type: "load", pair, wasm, source, target, files: { model, lex, srcvocab, trgvocab } },
    [wasm, model, lex, srcvocab, trgvocab],
  );
  if (r.type !== "loaded") {
    stopWorker();
    throw new Error(r.type === "error" ? r.message : "load failed");
  }
  loadedPair = pair;
  return worker;
}

async function serve(req: { id: number; pair: string; texts: string[] }) {
  window.clearTimeout(idleTimer);
  try {
    const w = await ensure(req.pair);
    const r = await ask(w, { type: "translate", texts: req.texts });
    if (r.type !== "result") throw new Error(r.type === "error" ? r.message : "translate failed");
    await api.onDeviceReply(req.id, r.texts, null);
  } catch (e) {
    const noModel = e instanceof AppError && e.kind === "TranslateNoModel";
    await api.onDeviceReply(req.id, null, noModel ? "no_model" : "failed").catch(() => {});
  } finally {
    idleTimer = window.setTimeout(stopWorker, IDLE_MS);
  }
}

/** 開始等外殼交來的工作（應用啟動時呼叫一次）。不在 Android 上時外殼回 `null`，迴圈照樣跑、不做事。 */
export function startBergamotBridge() {
  if (started) return;
  started = true;
  void (async () => {
    for (;;) {
      try {
        const req = await api.onDeviceNext();
        if (req) await serve(req);
      } catch {
        await new Promise((r) => window.setTimeout(r, 3000));
      }
    }
  })();
}
