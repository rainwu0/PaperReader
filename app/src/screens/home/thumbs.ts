// 封面縮圖：原文第一頁畫成寬約 240 像素的小圖，存在應用的快取（不進文獻庫）。
// 先向外殼要現成的；沒有就排進共用的 PDF 佇列（`pdfQueue.ts`，優先），和逐篇處理一起一次只開一份 PDF。
// 畫不出來就放棄（封面維持篇名首字的色塊），同一篇最多畫兩次（一次加一次重試），不報錯。

import { api } from "../../api";
import { withPdf } from "../../pdf/pdfSource";
import { enqueuePdf, noteUrgentRequest } from "../../pdfQueue";

const WIDTH = 240;
const TIMEOUT_MS = 20_000;
const MAX_ATTEMPTS = 2;

/** 畫好的圖（這次執行期間留著，列表來回捲動不必再問）。 */
const ready = new Map<string, Blob>();
/** 每一篇已經試著畫過幾次。 */
const attempts = new Map<string, number>();

interface Entry {
  promise: Promise<Blob | null>;
  /** 還有幾個畫面在等這一張；輪到它時沒人等了就跳過。 */
  users: number;
}
const pending = new Map<string, Entry>();

function withTimeout<T>(p: Promise<T>): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const id = window.setTimeout(() => reject(new Error("timeout")), TIMEOUT_MS);
    p.then(
      (v) => {
        window.clearTimeout(id);
        resolve(v);
      },
      (e: unknown) => {
        window.clearTimeout(id);
        reject(e instanceof Error ? e : new Error("failed"));
      },
    );
  });
}

/** 用 PDF.js 畫第一頁。 */
async function draw(paperId: string, materialId: string): Promise<Blob | null> {
  return withPdf(paperId, materialId, async (pdf) => {
    const page = await pdf.getPage(1);
    const base = page.getViewport({ scale: 1 });
    const viewport = page.getViewport({ scale: WIDTH / base.width });
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.ceil(viewport.width));
    canvas.height = Math.max(1, Math.ceil(viewport.height));
    await page.render({ canvas, viewport, background: "#ffffff" }).promise;
    page.cleanup();
    return await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.8));
  });
}

async function lookup(paperId: string, materialId: string, entry: Entry): Promise<Blob | null> {
  // 先看快取資料夾裡有沒有（上次已經畫好的）
  try {
    const buf = await api.thumbGet(paperId);
    if (buf.byteLength > 0) {
      const blob = new Blob([buf], { type: "image/jpeg" });
      ready.set(paperId, blob);
      return blob;
    }
  } catch {
    // 讀不到就當作還沒有
  }
  return enqueuePdf("urgent", async () => {
    if (entry.users === 0) return null;
    const tried = attempts.get(paperId) ?? 0;
    if (tried >= MAX_ATTEMPTS) return null;
    attempts.set(paperId, tried + 1);
    try {
      const blob = await withTimeout(draw(paperId, materialId));
      if (!blob) return null;
      ready.set(paperId, blob);
      try {
        await api.thumbPut(paperId, await blob.arrayBuffer());
      } catch {
        // 存不進快取也沒關係，這次照樣顯示
      }
      return blob;
    } catch {
      return null;
    }
  });
}

export interface CoverRequest {
  promise: Promise<Blob | null>;
  /** 畫面不再需要這一張（捲走、離開）。 */
  cancel: () => void;
}

/** 要一篇的封面圖：同一篇同時只做一次。交回 `null` 表示沒有圖（畫不出來或不要了）。 */
export function requestCover(paperId: string, materialId: string): CoverRequest {
  const hit = ready.get(paperId);
  if (hit) return { promise: Promise.resolve(hit), cancel: () => {} };
  noteUrgentRequest();
  let entry = pending.get(paperId);
  if (!entry) {
    const fresh: Entry = { users: 0, promise: Promise.resolve(null) };
    fresh.promise = lookup(paperId, materialId, fresh).finally(() => {
      pending.delete(paperId);
    });
    pending.set(paperId, fresh);
    entry = fresh;
  }
  const mine = entry;
  mine.users++;
  let cancelled = false;
  return {
    promise: mine.promise,
    cancel: () => {
      if (cancelled) return;
      cancelled = true;
      mine.users--;
    },
  };
}
