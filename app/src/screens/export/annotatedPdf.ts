// 帶標註的 PDF 副本的接線：讀原文的位元組、這篇的劃線與筆跡，交給 `annotate.ts` 寫成新檔。
// 原文不動；只放這一份材料上的劃線與筆跡。

import { api, type Stroke } from "../../api";
import { readWhole } from "../../pdf/pdfSource";
import { annotatePdf, type AnnotatedPdf } from "./annotate";

/** 組出帶標註的副本。`materialId` 是要匯出的那份 PDF；`flatten` 為真是列印用（標註畫進頁面）。 */
export async function buildAnnotatedPdf(
  paperId: string,
  materialId: string,
  opts: { flatten?: boolean } = {},
): Promise<AnnotatedPdf> {
  const [bytes, notes, ink] = await Promise.all([
    readWhole(paperId, materialId),
    api.readNotes(paperId),
    api.readInk(paperId).catch(() => ({ strokes: [] as Stroke[] })),
  ]);
  return annotatePdf(
    bytes,
    notes.doc.highlights.filter((h) => h.material === materialId),
    ink.strokes.filter((s) => s.material === materialId),
    opts,
  );
}
