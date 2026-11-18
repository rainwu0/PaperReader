// 從 PDF.js 讀出整份文件每一頁的文字內容，交給 `layout.ts` 組成段落（版面紀錄）。
// 逐頁讀的迴圈（`readPages`）與文字片段的轉換（`toRawItems`）產生翻譯、逐篇處理、閱讀畫面共用。

import type * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs";
import type { SegmentInput } from "../api";
import { unlessStopped, type PdfSource } from "../pdf/pdfSource";
import { layoutPages, type PageInput, type RawItem } from "./layout";
import { detectRegions } from "./pageRegions";
import { layoutWithRegions, type PageRegions } from "./regions";

/** PDF.js 一頁的文字內容 → `RawItem`（只留有文字的片段；標記內容的開頭結尾略過）。 */
export function toRawItems(content: { items: readonly unknown[] }): RawItem[] {
  const items: RawItem[] = [];
  for (const it of content.items) {
    if (typeof it === "object" && it !== null && "str" in it) {
      const t = it as { str: string; transform: number[]; width: number; height: number; fontName?: string; hasEOL?: boolean };
      items.push({
        str: t.str,
        transform: t.transform,
        width: t.width,
        height: t.height,
        fontName: t.fontName,
        hasEOL: t.hasEOL,
      });
    }
  }
  return items;
}

/**
 * 逐頁取文字。`onPage(第幾頁, 共幾頁)` 每頁做完呼叫一次；`cancelled()` 為真就停下來交回 `null`，
 * 一頁還在讀的時候也是（最慢 0.2 秒）。原文讀不出來時丟出那個錯誤，不會一直等。`count` 只讀前幾頁（預設全部）。
 */
export async function readPages(
  src: Pick<PdfSource, "transport">,
  pdf: pdfjs.PDFDocumentProxy,
  onPage: (done: number, total: number) => void,
  cancelled: () => boolean,
  count: number = pdf.numPages,
): Promise<PageInput[] | null> {
  const total = Math.min(count, pdf.numPages);
  const pages: PageInput[] = [];
  for (let n = 1; n <= total; n++) {
    if (cancelled()) return null;
    const page = await unlessStopped(pdf.getPage(n), src, cancelled);
    if (!page) return null;
    const content = await unlessStopped(page.getTextContent(), src, cancelled);
    if (!content) return null;
    const view = page.view as [number, number, number, number];
    pages.push({ view, items: toRawItems(content) });
    page.cleanup();
    onPage(n, total);
    // 讓出主執行緒，畫面的進度才畫得出來
    await new Promise((r) => setTimeout(r, 0));
  }
  return pages;
}

/** 用版面模型分段（只有翻譯用）：模型那一步每頁做完呼叫 `onPage`。 */
export interface ModelStep {
  onPage: (done: number, total: number) => void;
}

/**
 * 逐頁取文字、組成段落（`readPages` 加 `layoutPages`）。整份文件沒有任何文字（掃描版）交回空陣列。
 * 給了 `model`：讀完文字再逐頁跑版面模型，依它的區塊分段（`regions.ts`）；模型或外殼出錯時照規則分段，
 * 照常交回段落。中途取消交回 `null`。
 */
export async function extractSegments(
  src: Pick<PdfSource, "transport">,
  pdf: pdfjs.PDFDocumentProxy,
  onPage: (done: number, total: number) => void,
  cancelled: () => boolean,
  model?: ModelStep,
): Promise<SegmentInput[] | null> {
  const pages = await readPages(src, pdf, onPage, cancelled);
  if (!pages) return null;
  let regions: PageRegions[] | null = null;
  if (model && pages.some((p) => p.items.length > 0)) {
    try {
      regions = await detectRegions(src, pdf, model.onPage, cancelled);
    } catch {
      // 模型執行失敗：照規則分段
      regions = null;
    }
    if (cancelled()) return null;
  }
  return (regions ? layoutWithRegions(pages, regions) : layoutPages(pages)).map((s) => ({
    page: s.page,
    bbox: s.bbox,
    kind: s.kind,
    size: s.size,
    source: s.source,
  }));
}
