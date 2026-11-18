// 譯本 PDF 的產生工作：組裝在畫面這一側進行，所以放在整個應用共用的一處，離開「產生」畫面也會繼續。
//   - `startBuild`：開始（或重新）產生；一次只做一份，進行中再呼叫會被略過；
//   - `expectTranslation`：翻譯開始時呼叫，之後翻完（全部成功）就自動產生；取消、中途停下就不產生；
//   - 狀態用 `subscribeBuild`／`getBuild` 讀（畫面用 `useSyncExternalStore`）。

import { api, AppError, type ErrorKind } from "../api";
import { generateTranslationPdf, type GenerateOutcome, type GenerateStage } from "./generate";

export type BuildState =
  | { phase: "idle" }
  | { phase: "working"; paperId: string; overlayId: string; stage: GenerateStage; n: number; total: number }
  | { phase: "done"; paperId: string; overlayId: string; out: GenerateOutcome }
  | { phase: "failed"; paperId: string; overlayId: string; kind: ErrorKind };

let state: BuildState = { phase: "idle" };
const listeners = new Set<() => void>();
let stopRequested = false;

/** 材料清單變了（產生好、更新了譯本 PDF）：首頁與閱讀畫面聽這個事件重新讀。 */
export const MATERIALS_CHANGED = "pr-materials-changed";

function set(next: BuildState) {
  state = next;
  for (const l of listeners) l();
}

export const getBuild = (): BuildState => state;

export function subscribeBuild(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function cancelBuild() {
  stopRequested = true;
}

export async function startBuild(paperId: string, overlayId: string): Promise<void> {
  if (state.phase === "working") return;
  stopRequested = false;
  set({ phase: "working", paperId, overlayId, stage: "reading", n: 0, total: 0 });
  try {
    const out = await generateTranslationPdf(paperId, overlayId, {
      onStage: (stage) => {
        if (state.phase === "working") set({ ...state, stage });
      },
      onPage: (n, total) => {
        if (state.phase === "working") set({ ...state, n, total });
      },
      cancelled: () => stopRequested,
    });
    if (out === null) {
      set({ phase: "idle" });
      return;
    }
    window.dispatchEvent(new CustomEvent(MATERIALS_CHANGED, { detail: { paperId } }));
    set({ phase: "done", paperId, overlayId, out });
  } catch (e) {
    set({ phase: "failed", paperId, overlayId, kind: e instanceof AppError ? e.kind : "Unknown" });
  }
}

// ---------- 翻完自動產生 ----------

let expecting = false;
let timer = 0;

async function tick() {
  let s;
  try {
    s = await api.translateStatus();
  } catch {
    return;
  }
  if (!s || s.state === "running") return;
  expecting = false;
  window.clearInterval(timer);
  timer = 0;
  if (s.state === "finished" && s.failed === 0 && s.total > 0 && s.done >= s.total) {
    void startBuild(s.paperId, s.materialId);
  }
}

/** 一份翻譯開始了：之後翻完就自動產生譯本 PDF。 */
export function expectTranslation() {
  expecting = true;
  if (timer === 0) timer = window.setInterval(() => void tick(), 1500);
}

export const isExpecting = () => expecting;
