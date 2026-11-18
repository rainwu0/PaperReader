// Firefox 翻譯（Bergamot）的背景工作者：載入 WASM 引擎與一個語言組合的模型，把一批段落翻成譯文。
// 引擎的 JS 接口 `bergamot-translator.js` 是 Firefox 原樣的檔（MPL-2.0，授權見「關於」頁）；這個檔是本專案自己寫的。
// 呼叫方式參考 Firefox 的 translations-engine.worker.js 的做法（設定值照它），程式自己寫。
//
// 訊息：
//   { type: "load", pair, wasm, files: { model, lex, srcvocab, trgvocab }, source, target } → { type: "loaded" }
//   { type: "translate", texts } → { type: "result", texts }
//   出錯 → { type: "error", message }

/* global loadBergamot */
importScripts("bergamot-translator.js");

let bergamot = null;
let service = null;
let model = null;
let loadedPair = null;

// 模型檔在 WASM 記憶體裡要對齊的位元組數（照 Firefox）
const ALIGN = { model: 256, lex: 64, vocab: 64, srcvocab: 64, trgvocab: 64 };

const CONFIG = {
  "beam-size": "1",
  normalize: "1.0",
  "word-penalty": "0",
  "max-length-break": "128",
  "mini-batch-words": "1024",
  workspace: "128",
  "max-length-factor": "2.0",
  "skip-cost": "true",
  "cpu-threads": "0",
  quiet: "true",
  "quiet-translation": "true",
  "gemm-precision": "int8shiftAlphaAll",
  alignment: "soft",
};

function startEngine(wasmBinary) {
  return new Promise((resolve, reject) => {
    const m = loadBergamot({
      INITIAL_MEMORY: 40 * 1024 * 1024,
      print: () => {},
      printErr: () => {},
      onAbort: () => reject(new Error("engine aborted")),
      onRuntimeInitialized: () => Promise.resolve().then(() => resolve(m)),
      wasmBinary,
    });
  });
}

function aligned(kind, buf) {
  const mem = new bergamot.AlignedMemory(buf.byteLength, ALIGN[kind]);
  mem.getByteArrayView().set(new Uint8Array(buf));
  return mem;
}

function load(msg) {
  if (!bergamot) throw new Error("engine not started");
  if (model) {
    model.delete();
    model = null;
    loadedPair = null;
  }
  const f = msg.files;
  const vocabs = new bergamot.AlignedMemoryList();
  vocabs.push_back(aligned("srcvocab", f.srcvocab));
  vocabs.push_back(aligned("trgvocab", f.trgvocab));
  const yaml = Object.entries(CONFIG)
    .map(([k, v]) => `${k}: ${v}`)
    .join("\n");
  model = new bergamot.TranslationModel(
    msg.source,
    msg.target,
    yaml,
    aligned("model", f.model),
    aligned("lex", f.lex),
    vocabs,
    null,
  );
  if (!service) service = new bergamot.BlockingService({ cacheSize: 0 });
  loadedPair = msg.pair;
}

function translate(texts) {
  if (!model) throw new Error("no model");
  const out = texts.map(() => "");
  const idx = [];
  const messages = new bergamot.VectorString();
  const options = new bergamot.VectorResponseOptions();
  try {
    texts.forEach((t, i) => {
      // 空字串會讓引擎出錯，跳過
      if (t.trim() === "") return;
      idx.push(i);
      messages.push_back(t);
      options.push_back({ qualityScores: false, alignment: false, html: false });
    });
    if (idx.length === 0) return out;
    const responses = service.translate(model, messages, options);
    try {
      for (let k = 0; k < idx.length; k++) out[idx[k]] = responses.get(k).getTranslatedText();
    } finally {
      responses.delete();
    }
    return out;
  } finally {
    messages.delete();
    options.delete();
  }
}

self.onmessage = async (e) => {
  const msg = e.data;
  try {
    if (msg.type === "load") {
      if (!bergamot) bergamot = await startEngine(msg.wasm);
      if (loadedPair !== msg.pair) load(msg);
      self.postMessage({ type: "loaded" });
    } else if (msg.type === "translate") {
      self.postMessage({ type: "result", texts: translate(msg.texts) });
    }
  } catch (err) {
    self.postMessage({ type: "error", message: String(err && err.message ? err.message : err) });
  }
};
