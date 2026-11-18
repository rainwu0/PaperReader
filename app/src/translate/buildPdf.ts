// 組出譯本 PDF：每一頁從原文複製（圖、公式、向量圖、頁首頁尾都保留），
// 翻好的段落在它的範圍畫一個白色矩形蓋掉原文，再把譯文排進同一範圍。譯文是真的文字（嵌入字型子集），
// 所以在閱讀畫面選得起來、劃得了線。沒翻的段落保留原文。
//
// 蓋掉的原文還在頁面內容裡（PDF 的內容串流沒有改），所以文字層也會有它。為了讓閱讀畫面選字時只選到譯文：
//   - 畫上去的譯文包在標記內容 `PRText` 裡；
//   - 蓋住的範圍記在文件資訊的 `PaperReaderCovers`（JSON：頁序 → 矩形們），閱讀畫面的文字層把落在這些範圍裡
//     而不在 `PRText` 裡的原文字丟掉（`pdfSource.ts` 的 `makeTextLayer`）。
// 這個檔案只做 PDF 的組裝、不碰指令，所以可以在 node 裡測（`scripts/buildpdf-test.mjs`）。

import {
  PDFDict,
  PDFDocument,
  PDFName,
  PDFString,
  beginMarkedContent,
  endMarkedContent,
  rgb,
} from "pdf-lib";
import { mathText } from "./layout.ts";
import { COVERS_KEY, TEXT_TAG } from "./textFilter.ts";
import { openTypeface, type Typeface } from "./typeface.ts";
import { typeset, type Align } from "./typeset.ts";

export { COVERS_KEY, TEXT_TAG };

export interface BuildSegment {
  /** 頁序，從 0 起算。 */
  page: number;
  /** PDF 頁面座標 `[x0, y0, x1, y1]`（原點在左下）。 */
  bbox: [number, number, number, number];
  kind: string;
  /** 原文的字級；不知道是 `null`。 */
  size: number | null;
  target: string;
  /** 原文（用來判斷有沒有行內公式）。 */
  source?: string;
}

/**
 * 公式為主的段落才保留原文、不蓋。一般句子裡夾一兩個希臘字母、×、±、箭頭，照樣換成譯文：
 * 譯文會原樣保留這些符號（提示詞要求），而真實論文裡這種段落很多，全部留原文會讓譯本一段中文一段英文。
 * 判斷在版面紀錄那邊（`layout.ts` 的 `mathText`：TeX、數學符號一成五以上、公式的碎片），兩邊只有一份。
 */
export const hasInlineMath = mathText;

type Rect = [number, number, number, number];

/** 一頁上畫出來的圖形（PDF 頁面座標，原點在左下）：圖片、填色或描邊的路徑（表格線、分數線、向量圖）。 */
export interface PageGraphics {
  images: Rect[];
  paths: Rect[];
  /** 其中不是白色的填色（圖裡的色塊、表格的底色）：字放在上面時，白框會把顏色蓋掉。 */
  fills?: Rect[];
}

/** PDF.js 運算子清單裡要用到的運算子編號（`pdfjs.OPS`）；傳進來，這個檔案就不必依賴 PDF.js。 */
export type OpsTable = Record<string, number>;

type Matrix = [number, number, number, number, number, number];
const mul = (m: Matrix, n: Matrix): Matrix => [
  m[0] * n[0] + m[2] * n[1],
  m[1] * n[0] + m[3] * n[1],
  m[0] * n[2] + m[2] * n[3],
  m[1] * n[2] + m[3] * n[3],
  m[0] * n[4] + m[2] * n[5] + m[4],
  m[1] * n[4] + m[3] * n[5] + m[5],
];
function boxOf(m: Matrix, x0: number, y0: number, x1: number, y1: number): Rect {
  const pts = [
    [x0, y0],
    [x1, y0],
    [x0, y1],
    [x1, y1],
  ].map(([x = 0, y = 0]) => [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]] as const);
  const xs = pts.map((p) => p[0]);
  const ys = pts.map((p) => p[1]);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
}

const intersect = (a: Rect, b: Rect): Rect | null => {
  const r: Rect = [Math.max(a[0], b[0]), Math.max(a[1], b[1]), Math.min(a[2], b[2]), Math.min(a[3], b[3])];
  return r[0] <= r[2] && r[1] <= r[3] ? r : null;
};

interface GState {
  ctm: Matrix;
  /** 目前的裁切範圍（頁面座標）。 */
  clip: Rect;
  fill: string;
  stroke: string;
}

/**
 * 從一頁的運算子清單（`page.getOperatorList()` 的 `fnArray`、`argsArray`）找出畫出來的圖片與路徑的範圍。
 * 追蹤座標變換（存、還原、`cm`、表單物件的矩陣）與裁切範圍（插圖的畫布常比看得到的範圍大，超出的部分被裁掉）；
 * 只當裁切用的路徑不算，白色的填色不算（蓋白框不會改變它），文字不算。
 */
export function graphicsFromOps(fnArray: ArrayLike<number>, argsArray: ArrayLike<unknown>, OPS: OpsTable): PageGraphics {
  const out: PageGraphics = { images: [], paths: [] };
  const strokes = new Set(["stroke", "closeStroke"].map((k) => OPS[k]));
  const fills = new Set(["fill", "eoFill"].map((k) => OPS[k]));
  const both = new Set(["fillStroke", "eoFillStroke", "closeFillStroke", "closeEOFillStroke"].map((k) => OPS[k]));
  const white = (c: string) => /^#f[a-f0-9]f[a-f0-9]f[a-f0-9]$/i.test(c);
  const imageOps = new Set(
    [
      "paintImageXObject",
      "paintInlineImageXObject",
      "paintImageMaskXObject",
      "paintImageXObjectRepeat",
      "paintInlineImageXObjectGroup",
      "paintImageMaskXObjectGroup",
      "paintImageMaskXObjectRepeat",
      "paintSolidColorImageMask",
    ].map((k) => OPS[k]),
  );
  const INF = 1e9;
  let g: GState = { ctm: [1, 0, 0, 1, 0, 0], clip: [-INF, -INF, INF, INF], fill: "#000000", stroke: "#000000" };
  const stack: GState[] = [];
  let clipNext = false;
  const add = (list: Rect[], r: Rect) => {
    const v = intersect(r, g.clip);
    if (v) list.push(v);
  };
  for (let i = 0; i < fnArray.length; i++) {
    const fn = fnArray[i];
    const args = (argsArray[i] ?? []) as unknown[];
    if (fn === OPS.save) stack.push({ ...g });
    else if (fn === OPS.restore) g = stack.pop() ?? g;
    else if (fn === OPS.transform) g = { ...g, ctm: mul(g.ctm, args as unknown as Matrix) };
    else if (fn === OPS.setFillRGBColor && typeof args[0] === "string") g = { ...g, fill: args[0] };
    else if (fn === OPS.setStrokeRGBColor && typeof args[0] === "string") g = { ...g, stroke: args[0] };
    else if (fn === OPS.clip || fn === OPS.eoClip) clipNext = true;
    else if (fn === OPS.paintFormXObjectBegin) {
      stack.push({ ...g });
      const m = args[0] as number[] | null | undefined;
      if (m && m.length === 6) g = { ...g, ctm: mul(g.ctm, m as unknown as Matrix) };
      const bb = args[1] as ArrayLike<number> | null | undefined;
      if (bb && bb.length >= 4) {
        const [x0 = 0, y0 = 0, x1 = 0, y1 = 0] = Array.from(bb);
        g = { ...g, clip: intersect(g.clip, boxOf(g.ctm, x0, y0, x1, y1)) ?? [0, 0, 0, 0] };
      }
    } else if (fn === OPS.paintFormXObjectEnd) g = stack.pop() ?? g;
    else if (fn === OPS.constructPath) {
      const op = args[0] as number;
      const mm = args[2] as ArrayLike<number> | null | undefined;
      if (!mm || mm.length < 4) {
        clipNext = false;
        continue;
      }
      const [x0 = 0, y0 = 0, x1 = 0, y1 = 0] = Array.from(mm);
      if (![x0, y0, x1, y1].every(Number.isFinite)) {
        clipNext = false;
        continue;
      }
      const box = boxOf(g.ctm, x0, y0, x1, y1);
      // 畫出來的路徑先算，裁切從下一個運算開始生效
      const colored = (fills.has(op) || both.has(op)) && !white(g.fill);
      const seen = (strokes.has(op) && !white(g.stroke)) || colored || (both.has(op) && !white(g.stroke));
      if (seen) add(out.paths, box);
      if (colored) add((out.fills ??= []), box);
      if (clipNext) {
        g = { ...g, clip: intersect(g.clip, box) ?? [0, 0, 0, 0] };
        clipNext = false;
      }
    } else if (fn !== undefined && imageOps.has(fn)) add(out.images, boxOf(g.ctm, 0, 0, 1, 1));
  }
  return out;
}

/**
 * 段落的範圍碰到頁面上畫出來的東西：圖片蓋到段落一成以上、放在有顏色的色塊上（白框會把顏色蓋掉），
 * 或有路徑（線條、填色）伸進段落的範圍裡（整個包住段落的外框不算，只碰到邊緣不算）。
 * 這種段落是圖裡的字、表格、帶分數線的公式，保留原文、不蓋。
 */
export function touchesGraphics(bbox: Rect, g: PageGraphics | undefined): boolean {
  if (!g) return false;
  const [x0, y0, x1, y1] = bbox;
  const area = Math.max(1e-6, (x1 - x0) * (y1 - y0));
  const overlap = (r: Rect, a: Rect) =>
    Math.max(0, Math.min(r[2], a[2]) - Math.max(r[0], a[0])) * Math.max(0, Math.min(r[3], a[3]) - Math.max(r[1], a[1]));
  if (g.images.some((r) => overlap(r, bbox) >= 0.1 * area)) return true;
  if ((g.fills ?? []).some((r) => overlap(r, bbox) >= 0.5 * area)) return true;
  // 往內縮一點：段落的方框比字多留了半點，貼著的線（表格的橫線、欄間的線）不算伸進來
  const inner: Rect = [x0 + 1, y0 + 1.5, x1 - 1, y1 - 1.5];
  if (inner[2] <= inner[0] || inner[3] <= inner[1]) return false;
  return g.paths.some((r) => {
    const contains = r[0] <= x0 && r[1] <= y0 && r[2] >= x1 && r[3] >= y1;
    if (contains) return false;
    // 行內公式的分數線、上橫線、字詞底下的強調色塊（比段落小很多）不算：蓋掉它比整段留英文好
    const rw = r[2] - r[0];
    const rh = r[3] - r[1];
    if (rw < 18 && rh < 18) return false;
    if (rw < 0.4 * (x1 - x0) && rh < 0.5 * (y1 - y0)) return false;
    return r[0] < inner[2] && r[2] > inner[0] && r[1] < inner[3] && r[3] > inner[1];
  });
}

/** 保留原文、不蓋的段落：公式為主，或碰到圖形。 */
function keepsOriginal(s: BuildSegment, graphics: PageGraphics[] | undefined): "math" | "graphics" | null {
  if (s.source && hasInlineMath(s.source)) return "math";
  if (touchesGraphics(s.bbox, graphics?.[s.page])) return "graphics";
  return null;
}

export interface BuildOptions {
  sourceBytes: Uint8Array;
  /** 只放已翻好的段落。 */
  segments: BuildSegment[];
  fontBytes: Uint8Array;
  /** 目標語言（決定用 Noto CJK 的哪一個字面）。 */
  lang: string;
  /** 每做完一頁呼叫一次。 */
  onPage?: (done: number, total: number) => void;
  /** 回傳真就停下來（交回 `null`）。 */
  cancelled?: () => boolean;
  /** 測試用：換掉字型。 */
  makeTypeface?: (doc: PDFDocument) => Promise<Typeface>;
  /** 每一頁畫出來的圖形（`graphicsFromOps`，頁序從 0 起）；碰到圖形的段落保留原文。沒給就只看公式。 */
  graphics?: PageGraphics[];
}

export interface BuildResult {
  bytes: Uint8Array;
  pages: number;
  /** 實際放進去的段落數。 */
  placed: number;
  /** 放不下而截斷的段落數。 */
  truncated: number;
  /** 放不下而往下多出一點的段落數。 */
  overflowed: number;
  /** 縮了字級的段落數。 */
  shrunk: number;
  /** 含行內公式、保留原文沒有蓋的段落數。 */
  keptMath: number;
  /** 碰到圖片、表格線、分數線，保留原文沒有蓋的段落數。 */
  keptGraphics: number;
  fontName: string;
}

/** 哪些段落會被蓋住、蓋住的範圍（頁序 → 矩形們）：有譯文、不是公式為主、不碰到圖形的段落。 */
export function coverRects(
  segments: BuildSegment[],
  graphics?: PageGraphics[],
): Record<string, Array<[number, number, number, number]>> {
  const out: Record<string, Array<[number, number, number, number]>> = {};
  for (const s of segments) {
    if (s.target.trim() === "" || keepsOriginal(s, graphics)) continue;
    (out[String(s.page)] ??= []).push(s.bbox);
  }
  return out;
}

const round2 = (n: number) => Math.round(n * 100) / 100;

function alignOf(seg: BuildSegment, pageLeft: number, pageWidth: number): Align {
  const [x0, , x1] = seg.bbox;
  if (seg.kind === "heading") {
    const centered = Math.abs((x0 + x1) / 2 - (pageLeft + pageWidth / 2)) < pageWidth * 0.03;
    return centered && x1 - x0 < pageWidth * 0.8 ? "center" : "left";
  }
  return seg.kind === "caption" ? "left" : "justify";
}

const tick = () => new Promise<void>((r) => setTimeout(r, 0));

/** 組出譯本 PDF；被取消時交回 `null`。 */
export async function buildTranslatedPdf(o: BuildOptions): Promise<BuildResult | null> {
  const pdf = await PDFDocument.load(o.sourceBytes, { ignoreEncryption: true, updateMetadata: false });
  const face = await (o.makeTypeface ? o.makeTypeface(pdf) : openTypeface(pdf, o.fontBytes, o.lang));
  let keptMath = 0;
  let keptGraphics = 0;
  const byPage = new Map<number, BuildSegment[]>();
  for (const s of o.segments) {
    if (s.target.trim() === "") continue;
    const keep = keepsOriginal(s, o.graphics);
    if (keep === "math") keptMath++;
    if (keep === "graphics") keptGraphics++;
    if (keep) continue;
    const arr = byPage.get(s.page) ?? [];
    arr.push(s);
    byPage.set(s.page, arr);
  }
  const pageCount = pdf.getPageCount();
  const covers: Record<string, Rect[]> = {};
  let placed = 0;
  let truncated = 0;
  let overflowed = 0;
  let shrunk = 0;
  let done = 0;

  for (const [pageIndex, segs] of [...byPage].sort((a, b) => a[0] - b[0])) {
    if (o.cancelled?.()) return null;
    if (pageIndex >= pageCount) continue;
    const page = pdf.getPage(pageIndex);
    const media = page.getMediaBox();
    // 先把全部要蓋的範圍蓋掉，再畫譯文：相鄰的段落不會互相蓋到
    for (const s of segs) {
      const [x0, y0, x1, y1] = s.bbox;
      page.drawRectangle({ x: x0, y: y0, width: x1 - x0, height: y1 - y0, color: rgb(1, 1, 1), borderWidth: 0 });
      (covers[String(pageIndex)] ??= []).push([round2(x0), round2(y0), round2(x1), round2(y1)]);
    }
    page.pushOperators(beginMarkedContent(TEXT_TAG));
    for (const s of segs) {
      const [x0, y0, x1, y1] = s.bbox;
      const left = x0 + 0.5;
      const top = y1 - 0.5;
      const w = x1 - x0 - 1;
      const h = y1 - y0 - 1;
      const size0 = s.size ?? 10;
      const r = typeset({
        text: s.target,
        width: w,
        height: h,
        size: size0,
        align: alignOf(s, media.x, media.width),
        advance: (cp) => face.advance(cp),
      });
      if (r.lines.length === 0) continue;
      placed++;
      if (r.truncated) truncated++;
      else if (r.overflow > 0) overflowed++;
      if (r.size < size0 - 0.01) shrunk++;
      r.lines.forEach((ln, i) => {
        // 行框的正中放字：字高 0.88 em 在基線之上
        const baseline = top - (r.lineHeight - r.size) / 2 - 0.88 * r.size - i * r.lineHeight;
        face.drawLine(page, ln.text, left + ln.dx, baseline, {
          size: r.size,
          charSpace: ln.charSpace,
          color: [0, 0, 0],
        });
      });
    }
    page.pushOperators(endMarkedContent());
    done++;
    o.onPage?.(done, byPage.size);
    await tick();
  }

  await face.finish();

  // 蓋住的範圍記在文件資訊裡
  const ctx = pdf.context;
  let info = ctx.trailerInfo.Info ? ctx.lookup(ctx.trailerInfo.Info, PDFDict) : undefined;
  if (!info) {
    info = ctx.obj({});
    ctx.trailerInfo.Info = ctx.register(info);
  }
  info.set(PDFName.of(COVERS_KEY), PDFString.of(JSON.stringify({ v: 1, pages: covers })));

  const bytes = await pdf.save({ useObjectStreams: true });
  return { bytes, pages: pageCount, placed, truncated, overflowed, shrunk, keptMath, keptGraphics, fontName: face.name };
}
