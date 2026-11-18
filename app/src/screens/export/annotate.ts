// 帶標註的 PDF 副本：原文不動，另外組一份，把劃線、短評與筆跡寫成 PDF 標準的標註物件，
// 別的 PDF 閱讀器也看得到。這一份只做 PDF 的寫入、不碰指令，所以 `scripts/export-pdf-test.mjs` 可以直接測。用 pdf-lib（MIT）寫入。不寫回原文，要給別的工具看就匯出副本。
//
//   劃線   → /Highlight 標註：QuadPoints 是每一行的矩形，顏色用劃線的顏色（沒記就是螢光黃），
//            短評放在標註的 /Contents（別的閱讀器點標註會看到）；外觀串流用「色彩相乘」，字不會被蓋住。
//   筆跡   → /Ink 標註：InkList 是各點的頁面座標，線寬、顏色、不透明度照 `ink.json`，外觀串流畫成折線。
//
// 座標都是 PDF 的頁面座標（原點在左下，單位是點），與核心存的一致，不用換算。

import {
  BlendMode,
  LineCapStyle,
  LineJoinStyle,
  PDFDocument,
  PDFHexString,
  type PDFContext,
  type PDFObject,
  type PDFPage,
  type PDFRef,
  lineTo,
  moveTo,
  popGraphicsState,
  pushGraphicsState,
  rgb as pdfRgb,
  setGraphicsState,
  setLineCap,
  setLineJoin,
  setLineWidth,
  setStrokingColor,
  stroke,
} from "pdf-lib";
// 副檔名寫明：這個檔要能單獨在 Node 測（`scripts/export-pdf-test.mjs`）
import { HIGHLIGHT_COLOR, type Highlight, type Stroke } from "../../api/types.ts";

// pdf-lib 沒有匯出它的「字面物件」型別，照它的形狀自己寫一份
interface LiteralObject {
  [name: string]: LiteralValue;
}
type LiteralValue = string | number | boolean | null | undefined | PDFObject | LiteralObject | LiteralValue[];

const AUTHOR = "PaperReader";
/** 筆跡沒記顏色時用的紅：和閱讀畫面筆刷預設的紅相同（`reader/pdfInk.ts` 的 `DEFAULT_PEN_COLOR`）。
 *  這個檔要能單獨在 Node 測，不引入閱讀畫面的模組，所以在這裡另記一份。 */
const DEFAULT_INK = "#d93025";

/** `#rrggbb` → 0 到 1 的三個數；讀不懂就用 `fallback`。 */
function rgb(color: string | null | undefined, fallback: string): [number, number, number] {
  const m = /^#?([0-9a-f]{6})$/i.exec((color ?? "").trim()) ?? /^#?([0-9a-f]{6})$/i.exec(fallback);
  const n = parseInt(m?.[1] ?? "0", 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

const num = (n: number) => String(Math.round(n * 1000) / 1000);

/** 一個外觀串流（Form XObject）：`bbox` 與標註的 /Rect 相同，所以不需要矩陣。 */
function appearance(
  ctx: PDFContext,
  bbox: [number, number, number, number],
  content: string,
  gs: { blend?: string; alpha?: number },
): PDFRef {
  const stream = ctx.stream(content, {
    Type: "XObject",
    Subtype: "Form",
    BBox: bbox,
    Resources: {
      ExtGState: {
        G0: { Type: "ExtGState", BM: gs.blend ?? "Normal", CA: gs.alpha ?? 1, ca: gs.alpha ?? 1 },
      },
    },
  });
  return ctx.register(stream);
}

function highlightAnnot(ctx: PDFContext, h: Highlight): PDFRef | null {
  if (h.rects.length === 0) return null;
  const [r, g, b] = rgb(h.color, HIGHLIGHT_COLOR);
  const quads: number[] = [];
  let content = `/G0 gs ${num(r)} ${num(g)} ${num(b)} rg\n`;
  let [minX, minY, maxX, maxY] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const [l, bt, rt, tp] of h.rects) {
    // 四個角的順序：左上、右上、左下、右下（Acrobat 與 PDF.js 寫出來的順序，各家閱讀器都照這個讀）
    quads.push(l, tp, rt, tp, l, bt, rt, bt);
    content += `${num(l)} ${num(bt)} ${num(rt - l)} ${num(tp - bt)} re f\n`;
    minX = Math.min(minX, l);
    minY = Math.min(minY, bt);
    maxX = Math.max(maxX, rt);
    maxY = Math.max(maxY, tp);
  }
  const rect: [number, number, number, number] = [minX, minY, maxX, maxY];
  const ap = appearance(ctx, rect, content, { blend: "Multiply" });
  const annot: LiteralObject = {
    Type: "Annot",
    Subtype: "Highlight",
    Rect: rect,
    QuadPoints: quads,
    C: [r, g, b],
    F: 4,
    T: PDFHexString.fromText(AUTHOR),
    AP: { N: ap },
  };
  const comment = h.comment.trim();
  if (comment !== "") annot["Contents"] = PDFHexString.fromText(comment);
  return ctx.register(ctx.obj(annot));
}

function inkAnnot(ctx: PDFContext, s: Stroke): PDFRef | null {
  if (s.points.length === 0) return null;
  const [r, g, b] = rgb(s.color, DEFAULT_INK);
  const w = Math.max(0.1, s.width);
  const flat = s.points.flatMap(([x, y]) => [x, y]);
  let [minX, minY, maxX, maxY] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const [x, y] of s.points) {
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  }
  const pad = w / 2 + 1;
  const rect: [number, number, number, number] = [minX - pad, minY - pad, maxX + pad, maxY + pad];
  const [first, ...rest] = s.points;
  // 一個點也畫（圓頭線帽把一點畫成圓點）
  let content = `/G0 gs ${num(r)} ${num(g)} ${num(b)} RG ${num(w)} w 1 J 1 j\n${num(first![0])} ${num(first![1])} m\n`;
  if (rest.length === 0) content += `${num(first![0])} ${num(first![1])} l\n`;
  for (const [x, y] of rest) content += `${num(x)} ${num(y)} l\n`;
  content += "S\n";
  const ap = appearance(ctx, rect, content, { alpha: s.opacity });
  return ctx.register(
    ctx.obj({
      Type: "Annot",
      Subtype: "Ink",
      Rect: rect,
      InkList: [flat],
      C: [r, g, b],
      CA: s.opacity,
      BS: { W: w },
      F: 4,
      T: PDFHexString.fromText(AUTHOR),
      AP: { N: ap },
    }),
  );
}

export interface AnnotatedPdf {
  bytes: Uint8Array;
  highlights: number;
  strokes: number;
}

/** 一條劃線落在哪幾頁、各頁的矩形：跨頁的劃線把後半（`next_page_rects`）放在下一頁。 */
function highlightParts(h: Highlight): Array<{ page: number; rects: Highlight["rects"] }> {
  const parts = [{ page: h.page.index, rects: h.rects }];
  if (h.next_page_rects && h.next_page_rects.length > 0) parts.push({ page: h.page.index + 1, rects: h.next_page_rects });
  return parts;
}

/** 列印用：標註物件不是每個列印服務都會畫，所以把劃線與筆跡直接畫進頁面內容（短評不印）。 */
function flattenHighlight(page: PDFPage, h: Highlight, rects: Highlight["rects"]) {
  const [r, g, b] = rgb(h.color, HIGHLIGHT_COLOR);
  for (const [l, bt, rt, tp] of rects) {
    page.drawRectangle({ x: l, y: bt, width: rt - l, height: tp - bt, color: pdfRgb(r, g, b), blendMode: BlendMode.Multiply });
  }
}

function flattenInk(page: PDFPage, ctx: PDFContext, s: Stroke) {
  const [first, ...rest] = s.points;
  if (!first) return;
  const [r, g, b] = rgb(s.color, DEFAULT_INK);
  const gs = page.node.newExtGState("PRInk", ctx.obj({ Type: "ExtGState", CA: s.opacity, ca: s.opacity }));
  page.pushOperators(
    pushGraphicsState(),
    setGraphicsState(gs),
    setStrokingColor(pdfRgb(r, g, b)),
    setLineWidth(Math.max(0.1, s.width)),
    setLineCap(LineCapStyle.Round),
    setLineJoin(LineJoinStyle.Round),
    moveTo(first[0], first[1]),
    ...(rest.length === 0 ? [lineTo(first[0], first[1])] : rest.map(([x, y]) => lineTo(x, y))),
    stroke(),
    popGraphicsState(),
  );
}

/**
 * 在一份 PDF 的位元組上加標註，交回新的位元組。頁序超出範圍的標註略過。
 * `flatten` 為真時不寫標註物件，改把劃線與筆跡畫進頁面內容（給列印用）。
 */
export async function annotatePdf(
  source: Uint8Array,
  highlights: Highlight[],
  strokes: Stroke[],
  opts: { flatten?: boolean } = {},
): Promise<AnnotatedPdf> {
  const doc = await PDFDocument.load(source, { ignoreEncryption: true, updateMetadata: false });
  const pages = doc.getPages();
  const ctx = doc.context;
  let nHighlights = 0;
  let nStrokes = 0;
  for (const h of highlights) {
    let placed = false;
    for (const part of highlightParts(h)) {
      const page = pages[part.page];
      if (!page || part.rects.length === 0) continue;
      if (opts.flatten) flattenHighlight(page, h, part.rects);
      else {
        const ref = highlightAnnot(ctx, { ...h, rects: part.rects });
        if (ref) page.node.addAnnot(ref);
      }
      placed = true;
    }
    if (placed) nHighlights++;
  }
  for (const s of strokes) {
    const page = pages[s.page];
    if (!page || s.points.length === 0) continue;
    if (opts.flatten) flattenInk(page, ctx, s);
    else {
      const ref = inkAnnot(ctx, s);
      if (ref) page.node.addAnnot(ref);
    }
    nStrokes++;
  }
  return { bytes: await doc.save(), highlights: nHighlights, strokes: nStrokes };
}
