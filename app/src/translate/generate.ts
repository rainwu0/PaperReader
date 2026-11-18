// 產生譯本 PDF 的接線：讀譯文疊加檔與原文的位元組、系統字型，交給 `buildPdf.ts` 組出 PDF，
// 再交給核心存成這篇的譯本材料。組裝在畫面這一側（PDF.js、pdf-lib 都在這裡），資料規則在核心。

import * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs";
import { api } from "../api";
// 讀整份原文；PDF.js 的背景工作者也在這裡設定（閱讀畫面與分析版面共用）
import { openBytes, readWhole } from "../pdf/pdfSource";
import { buildTranslatedPdf, coverRects, graphicsFromOps, type PageGraphics } from "./buildPdf";

export type GenerateStage = "reading" | "building" | "saving";

export interface GenerateHooks {
  onStage?: (stage: GenerateStage) => void;
  /** 排完幾頁（只算有譯文的頁）。 */
  onPage?: (done: number, total: number) => void;
  cancelled?: () => boolean;
}

export interface GenerateOutcome {
  materialId: string;
  /** `created`：第一次；`updated`：原地更新；`unchanged`：內容與現有的相同。 */
  outcome: "created" | "updated" | "unchanged";
  /** 放進 PDF 的段落、縮了字級的、往下多出的、截斷的段落數。 */
  placed: number;
  shrunk: number;
  overflowed: number;
  truncated: number;
  /** 含行內公式、保留原文的段落。 */
  keptMath: number;
  /** 碰到圖片、表格線、分數線，保留原文的段落。 */
  keptGraphics: number;
  /** 從原文刪掉的文字指令數（0 可能是沒有蓋到字，也可能是這份 PDF 讀不了、沒做）。 */
  stripped: number;
  /** 翻好的段落占全部的幾段。 */
  done: number;
  total: number;
  /** 沒翻好、保留原文的段落。 */
  kept: number;
}

/** 用 `overlayId` 這份譯文疊加檔產生（或更新）譯本 PDF；被取消時交回 `null`。 */
export async function generateTranslationPdf(
  paperId: string,
  overlayId: string,
  hooks: GenerateHooks = {},
): Promise<GenerateOutcome | null> {
  hooks.onStage?.("reading");
  const overlay = await api.readOverlay(paperId, overlayId);
  const done = overlay.segments.filter((s) => s.status === "done" && s.target);
  const [source, font] = await Promise.all([readWhole(paperId, overlay.source_material), api.systemCjkFont()]);
  if (hooks.cancelled?.()) return null;
  hooks.onStage?.("building");
  const segments = done.map((s) => ({
    page: s.page,
    bbox: s.bbox,
    kind: s.kind,
    size: s.size,
    target: s.target ?? "",
    source: s.source,
  }));
  // 原文每一頁畫了哪些圖形：碰到圖形的段落（圖裡的字、表格、帶分數線的公式）保留原文、不蓋（延伸公式那一處的判斷）
  const graphics = await pageGraphics(source, new Set(segments.map((s) => s.page)));
  if (hooks.cancelled?.()) return null;
  // 被蓋住的原文連文字指令一起刪掉（外殼的 lopdf）；讀不了（加密等）就保留原檔，靠閱讀畫面的文字層過濾
  let base = source;
  let stripped = 0;
  try {
    const r = await api.stripCoveredText(source, coverRects(segments, graphics));
    base = r.bytes;
    stripped = r.removed;
  } catch {
    // 保留原檔
  }
  const built = await buildTranslatedPdf({
    sourceBytes: base,
    segments,
    fontBytes: font,
    lang: overlay.target_lang,
    graphics,
    ...(hooks.onPage ? { onPage: hooks.onPage } : {}),
    ...(hooks.cancelled ? { cancelled: hooks.cancelled } : {}),
  });
  if (!built) return null;
  hooks.onStage?.("saving");
  const put = await api.putGeneratedTranslation(paperId, overlayId, built.bytes, built.pages);
  return {
    materialId: put.materialId,
    outcome: put.outcome,
    placed: built.placed,
    shrunk: built.shrunk,
    overflowed: built.overflowed,
    truncated: built.truncated,
    keptMath: built.keptMath,
    keptGraphics: built.keptGraphics,
    stripped,
    done: done.length,
    total: overlay.segments.length,
    kept: overlay.segments.length - done.length,
  };
}

/** 原文 `pages`（頁序從 0 起）這幾頁畫出來的圖形。讀不了就交回空的，照舊只看公式。 */
async function pageGraphics(bytes: Uint8Array, pages: Set<number>): Promise<PageGraphics[]> {
  const out: PageGraphics[] = [];
  try {
    // PDF.js 會把交給它的位元組搬到背景工作者，給一份複本
    const task = openBytes(bytes.slice());
    const doc = await task.promise;
    try {
      for (const p of [...pages].sort((a, b) => a - b)) {
        if (p < 0 || p >= doc.numPages) continue;
        const page = await doc.getPage(p + 1);
        const ops = await page.getOperatorList();
        out[p] = graphicsFromOps(ops.fnArray, ops.argsArray, pdfjs.OPS);
        page.cleanup();
      }
    } finally {
      await task.destroy();
    }
  } catch {
    // 讀不了：不判斷圖形
  }
  return out;
}
