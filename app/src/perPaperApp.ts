// 逐篇處理接上應用（規則在 `perPaper.ts`）：目錄、應用空間的紀錄、經共用的 PDF 佇列開原文、「閒著」的判斷、
// 內建的補書目與登記處（`src/ext/index.ts`）裡外掛的抽取。整個應用一個排程，`App.tsx` 開文獻庫之後 `startPerPaper()`。

import { api } from "./api";
import { sourcePdf } from "./api/types";
import { plugins } from "./ext";
import { enqueuePdf, msSinceUrgent, urgentWaiting } from "./pdfQueue";
import {
  bibIdsTask,
  PerPaperScheduler,
  SourceUnusable,
  textPagesTask,
  type BuiltinTask,
  type OpenedSource,
  type PaperRef,
} from "./perPaper";
import { withPdf } from "./pdf/pdfSource";
import { getBuild, isExpecting } from "./translate/autoBuild";
import { readPages } from "./translate/extract";

/** 封面縮圖的要求之後多久才開始新的一篇。 */
const THUMB_QUIET_MS = 5000;
/** 不能做時隔多久再看一次；沒有要做的時隔多久再看一次（加入、換文獻庫、回到前景會提早叫醒）。 */
const BUSY_RETRY_MS = 4000;
const IDLE_RETRY_MS = 60_000;

/** 逐篇處理補了書目（年份、DOI、網址）：首頁聽這個事件重新讀目錄。 */
export const BIB_FILLED = "pr-bib-filled";

const builtin: BuiltinTask[] = [
  bibIdsTask(async (paperId, fill) => {
    const changed = await api.fillBibBlanks(paperId, fill);
    if (changed) window.dispatchEvent(new CustomEvent(BIB_FILLED, { detail: { paperId } }));
    return changed;
  }),
  // 抽文字：原文逐頁的文字存成 `text.json`，全文搜尋與之後的功能用
  textPagesTask({ write: api.textWrite, states: api.textStates }),
];
const tasks = (): BuiltinTask[] => [...builtin, ...plugins.flatMap((p) => p.perPaper ?? [])];

async function listPapers(): Promise<PaperRef[]> {
  const list = await api.listPapers();
  const out: PaperRef[] = [];
  for (const p of list) {
    const m = sourcePdf(p.materials);
    if (!m || m.status !== "Ok" || !m.sha256 || p.needs_newer_app) continue;
    out.push({
      id: p.id,
      added: p.added ?? null,
      source: { materialId: m.id, sha256: m.sha256, pages: m.pages },
      bib: { year: p.bib.year, doi: p.bib.doi, url: p.bib.url },
    });
  }
  return out;
}

/** PDF.js 打不開的原文（壞檔、加密）：不是指令的錯誤（指令的錯誤有 `kind`）。 */
function unusable(e: unknown): unknown {
  if (typeof e === "object" && e !== null && "kind" in e) return e;
  const name = e instanceof Error ? e.name : "";
  return new SourceUnusable(name === "PasswordException" ? "Encrypted" : "Unreadable");
}

function withSource<T>(paper: PaperRef, work: (src: OpenedSource) => Promise<T>): Promise<T> {
  return enqueuePdf("background", async () => {
    // 開檔（`withPdf`）的錯誤是原文的問題；`work` 裡的錯誤（抽取丟出的）原樣交回
    let opened = false;
    try {
      const out = await withPdf(paper.id, paper.source.materialId, async (pdf, src) => {
        opened = true;
        const meta = await pdf.getMetadata().catch(() => null);
        const raw = meta?.metadata as { getRaw?: () => unknown } | null | undefined;
        const xmp = typeof raw?.getRaw === "function" ? raw.getRaw() : null;
        return work({
          numPages: pdf.numPages,
          info: (meta?.info ?? {}) as Record<string, unknown>,
          xmp: typeof xmp === "string" ? xmp : null,
          readPages: (count, stop) => readPages(src, pdf, () => {}, stop, count),
        });
      });
      // 沒有給 `cancelled`，`withPdf` 不會交回 null
      return out as T;
    } catch (e) {
      throw opened ? e : unusable(e);
    }
  });
}

/** 有產生的工作在跑（翻譯、重點筆記、譯本 PDF）。查既有的狀態，查不到當成沒有。 */
async function generating(): Promise<boolean> {
  if (getBuild().phase === "working" || isExpecting()) return true;
  const [tr, dg] = await Promise.all([api.translateStatus().catch(() => null), api.digestStatus().catch(() => null)]);
  return tr?.state === "running" || dg?.state === "running";
}

export const perPaper = new PerPaperScheduler(
  {
    listPapers,
    loadRecord: () => api.perPaperStateGet(),
    saveRecord: (rec) => api.perPaperStatePut(rec),
    withSource,
    setPages: async (paperId, materialId, pages) => {
      await api.setMaterialPages(paperId, materialId, pages);
    },
    busyReason: async () => {
      if (document.visibilityState !== "visible") return "background";
      if (msSinceUrgent() < THUMB_QUIET_MS) return "thumbs";
      if (await generating()) return "generating";
      return null;
    },
    urgentWaiting,
  },
  tasks,
);

let started = false;
let wake: (() => void) | null = null;

/** 叫醒排程（加入了一篇、繼續、回到前景、離開閱讀畫面）。 */
export function pokePerPaper(): void {
  wake?.();
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    const id = window.setTimeout(done, ms);
    function done() {
      window.clearTimeout(id);
      wake = null;
      resolve();
    }
    wake = done;
  });
}

/** 開始在背景跑（整個應用只跑一個迴圈；再呼叫無作用）。 */
export function startPerPaper(): void {
  if (started) return;
  started = true;
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") pokePerPaper();
    else perPaper.stopCurrent();
  });
  void (async () => {
    for (;;) {
      let r: Awaited<ReturnType<typeof perPaper.step>>;
      try {
        r = await perPaper.step();
      } catch {
        r = "busy"; // 例如文獻庫還沒開（NoLibrary）
      }
      if (r === "worked" || r === "stopped") {
        // 讓出主執行緒一下再做下一篇
        await sleep(r === "stopped" ? BUSY_RETRY_MS : 200);
        continue;
      }
      await sleep(r === "nothing" || r === "paused" ? IDLE_RETRY_MS : BUSY_RETRY_MS);
    }
  })();
}

/** 閱讀畫面掛上時持有一個暫停；交回放掉的函式（放掉時叫醒排程）。 */
export function holdPerPaper(): () => void {
  const release = perPaper.hold();
  return () => {
    release();
    pokePerPaper();
  };
}

/** 剛加入的一篇：`until`（補篇名與作者）結束之前不排進來，結束後叫醒排程。 */
export function deferPerPaper(paperId: string, until: Promise<unknown>): void {
  perPaper.defer(paperId, until);
  void until.finally(pokePerPaper).catch(() => {});
}
