// 版面模型的偵測（只給翻譯用）：把每一頁畫好、縮成模型的輸入大小，交給外殼跑版面模型，收回區塊。
// 怎麼用這些區塊分段在 `regions.ts`（純函式）。
//
// 畫法照 `scripts/layout-model.md` 的做法：長邊 1600 像素畫一頁，再整張縮成 800×800（不保持長寬比），
// 取 RGB 編成 Base64 字串交給外殼（Android 上原始位元組會被改成數字陣列，慢 5 倍以上）。
// 外殼交回的方框是 PDF 點、頁面左上為原點，和 `regions.ts` 用的座標相同。

import type * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs";
import { api } from "../api";
import { unlessStopped, type PdfSource } from "../pdf/pdfSource";
import type { PageRegions } from "./regions";

const SIDE = 800;
const RENDER_LONG = 1600;

/** RGB 位元組 → Base64（分段轉，避免一次把幾百萬個參數交給 `fromCharCode`）。 */
function toBase64(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin);
}

/**
 * 畫一頁、縮成 800×800，交回 RGB 的 Base64；取消了交回 `null`。兩張畫布由呼叫的人給，每頁重用。
 * 畫的時候要讀原文的圖片與字型：讀不出來或按了取消時不再等（`unlessStopped`），不會停在這一步。
 */
async function pageImage(
  page: pdfjs.PDFPageProxy,
  big: HTMLCanvasElement,
  small: HTMLCanvasElement,
  src: Pick<PdfSource, "transport">,
  cancelled: () => boolean,
): Promise<string | null> {
  const base = page.getViewport({ scale: 1 });
  const viewport = page.getViewport({ scale: RENDER_LONG / Math.max(base.width, base.height) });
  big.width = Math.max(1, Math.ceil(viewport.width));
  big.height = Math.max(1, Math.ceil(viewport.height));
  const task = page.render({ canvas: big, viewport, background: "#ffffff" });
  const done = await unlessStopped(task.promise.then(() => true), src, cancelled).catch((e: unknown) => {
    task.cancel();
    throw e;
  });
  if (!done) {
    task.cancel();
    return null;
  }
  const ctx = small.getContext("2d", { willReadFrequently: true });
  if (!ctx) throw new Error("no 2d context");
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(big, 0, 0, SIDE, SIDE);
  const rgba = ctx.getImageData(0, 0, SIDE, SIDE).data;
  const rgb = new Uint8Array(SIDE * SIDE * 3);
  for (let i = 0, j = 0; i < rgba.length; i += 4, j += 3) {
    rgb[j] = rgba[i] ?? 255;
    rgb[j + 1] = rgba[i + 1] ?? 255;
    rgb[j + 2] = rgba[i + 2] ?? 255;
  }
  return toBase64(rgb);
}

/**
 * 每一頁跑一次版面模型。`onPage(第幾頁, 共幾頁)` 每頁做完呼叫一次；`cancelled()` 為真就停下來交回 `null`。
 * 頁面有旋轉的不跑（那一頁是 `null`，照規則分段）。模型或外殼出錯時丟出那個錯誤，由呼叫的人改照規則分段。
 */
export async function detectRegions(
  src: Pick<PdfSource, "transport">,
  pdf: pdfjs.PDFDocumentProxy,
  onPage: (done: number, total: number) => void,
  cancelled: () => boolean,
): Promise<PageRegions[] | null> {
  const big = document.createElement("canvas");
  const small = document.createElement("canvas");
  small.width = SIDE;
  small.height = SIDE;
  const out: PageRegions[] = [];
  try {
    for (let n = 1; n <= pdf.numPages; n++) {
      if (cancelled()) return null;
      const page = await unlessStopped(pdf.getPage(n), src, cancelled);
      if (!page) return null;
      try {
        if (page.rotate % 360 !== 0) {
          out.push(null);
        } else {
          const base = page.getViewport({ scale: 1 });
          const image = await pageImage(page, big, small, src, cancelled);
          if (image === null || cancelled()) return null;
          // 外殼那一頁跑完才會交回；按了取消就不等它
          const boxes = await unlessStopped(api.layoutDetect(image, base.width, base.height), src, cancelled);
          if (boxes === null) return null;
          out.push(boxes);
        }
      } finally {
        page.cleanup();
      }
      onPage(n, pdf.numPages);
      // 讓出主執行緒，畫面的進度才畫得出來
      await new Promise((r) => setTimeout(r, 0));
    }
    return out;
  } finally {
    // 放掉畫布的記憶體（長邊 1600 的一張約 13 MB）
    big.width = 0;
    big.height = 0;
    small.width = 0;
    small.height = 0;
  }
}
