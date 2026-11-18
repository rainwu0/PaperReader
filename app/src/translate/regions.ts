// 版面模型的區塊 + 文字層的分段（只給翻譯用）：規則先跑，再依模型的區塊決定段落與翻不翻。
// 做法照 `scripts/layout-model.md`，用它量過的設定：一個區塊一段（`LM_SEG=region`）、
// 區塊要翻而且規則也沒丟掉的字才翻（`LM_TRANS=both`）、類別無關的 NMS（IoU 0.7）、PP-DocLayoutV3 的類別。
//
//   1. 先跑規則（`layout.ts`）得到規則段落；它也決定「哪些字規則會丟掉」（作者、參考文獻、表格、公式…）。
//   2. 每個文字片段依中心點歸到面積最小、包住它的區塊。
//   3. 只交出「要翻的角色」（篇名、標題、正文、圖說、註腳）的區塊裡、規則也留下的字；其餘角色整塊不翻。
//   4. 落在區塊外的字（方框差了幾點）歸給它所屬規則段落的字最多落在的那個區塊；還是沒有區塊的照規則的段落。
//   5. 閱讀順序：區塊依遞迴的 XY 切割排（跨欄的區塊自成一層，層與層之間先左欄、再右欄）。
//   6. 沒有區塊的頁（沒下載模型、模型失敗、頁面有旋轉）整頁照規則。
//
// 區塊的座標：頁面可見範圍的左上為原點、y 向下、單位為 PDF 點（外殼的 `layout_detect` 照這個單位交回）。
// 這個檔案只依賴 `layout.ts`，Node 的測試可以直接引入。

import {
  layoutPages,
  segmentOf,
  type LayoutSegment,
  type PageInput,
  type RawItem,
  type SegmentKind,
} from "./layout.ts";

/** 模型交回的一個區塊。 */
export interface RegionBox {
  /** PP-DocLayoutV3 的類別名稱（`app/translate/src/layout_model.rs` 的 `LABELS`）。 */
  label: string;
  score: number;
  /** `[x0, y0, x1, y1]`，左上為原點、y 向下、PDF 點。 */
  bbox: [number, number, number, number];
}

/** 一頁的區塊；`null` 是這一頁沒有跑模型（照規則）。 */
export type PageRegions = RegionBox[] | null;

/** 角色決定翻不翻。`skip` 不當成區塊（行內公式落在正文區塊裡，字要歸給外面的正文）。 */
type Role =
  | "title"
  | "heading"
  | "body"
  | "caption"
  | "footnote"
  | "formula"
  | "table"
  | "figure"
  | "header"
  | "footer"
  | "reference"
  | "other"
  | "skip";

/** PP-DocLayoutV3 的類別對到角色（`scripts/layout-model.md`）。不認得的類別當成 `other`。 */
const ROLES: Readonly<Record<string, Role>> = {
  abstract: "body",
  algorithm: "other",
  aside_text: "other",
  chart: "figure",
  content: "body",
  display_formula: "formula",
  doc_title: "title",
  figure_title: "caption",
  footer: "footer",
  footer_image: "other",
  footnote: "footnote",
  formula_number: "formula",
  header: "header",
  header_image: "other",
  image: "figure",
  inline_formula: "skip",
  number: "footer",
  paragraph_title: "heading",
  reference: "reference",
  reference_content: "reference",
  seal: "other",
  table: "table",
  text: "body",
  vertical_text: "other",
  vision_footnote: "caption",
};

const TRANSLATE_ROLES: ReadonlySet<Role> = new Set<Role>(["title", "heading", "body", "caption", "footnote"]);

/** 類別無關的 NMS：重疊（IoU）超過這個值的兩個區塊留分數高的。 */
const NMS_IOU = 0.7;

type Box = [number, number, number, number];

const area = (b: Box) => Math.max(0, b[2] - b[0]) * Math.max(0, b[3] - b[1]);
const inside = (b: Box, x: number, y: number, tol = 0) =>
  x >= b[0] - tol && x <= b[2] + tol && y >= b[1] - tol && y <= b[3] + tol;

function overlap(a: Box, b: Box): number {
  const ix = Math.max(0, Math.min(a[2], b[2]) - Math.max(a[0], b[0]));
  const iy = Math.max(0, Math.min(a[3], b[3]) - Math.max(a[1], b[1]));
  return ix * iy;
}

function iou(a: Box, b: Box): number {
  const inter = overlap(a, b);
  return inter / (area(a) + area(b) - inter || 1);
}

interface Region {
  role: Role;
  score: number;
  bbox: Box;
  items: Item[];
}

/** 一頁的一個文字片段：中心點（左上座標）。 */
interface Item {
  raw: RawItem;
  x: number;
  y: number;
  /** 字數（不計空白），找「字最多落在哪個區塊」用。 */
  len: number;
  region: Region | null;
  /** 包住它的規則段落（這一頁的第幾段）；`null` 是規則丟掉的字。 */
  rseg: number | null;
}

function prepareItems(page: PageInput): Item[] {
  const [vx0, , , vy1] = page.view;
  const out: Item[] = [];
  for (const it of page.items) {
    const len = it.str.replace(/\s+/g, "").length;
    if (len === 0 || it.transform.length < 6) continue;
    const [a = 1, b = 0, c = 0, d = 1, e = 0, f = 0] = it.transform;
    const size = Math.max(1, Math.hypot(c, d) || it.height || 1);
    const rotated = Math.abs(b) > 0.2 * Math.abs(a) && Math.abs(c) > 0.2 * Math.abs(d);
    const cx = rotated ? e : e + it.width / 2;
    const cy = rotated ? f : f + size * 0.3;
    out.push({ raw: it, x: cx - vx0, y: vy1 - cy, len, region: null, rseg: null });
  }
  return out;
}

/** PDF 座標（左下為原點）換成左上為原點。 */
function toTopLeft(view: PageInput["view"], b: Box): Box {
  return [b[0] - view[0], view[3] - b[3], b[2] - view[0], view[3] - b[1]];
}

function nms(boxes: Region[]): Region[] {
  const sorted = [...boxes].sort((a, b) => b.score - a.score);
  const keep: Region[] = [];
  for (const b of sorted) if (!keep.some((k) => iou(k.bbox, b.bbox) > NMS_IOU)) keep.push(b);
  return keep;
}

// ---------- 閱讀順序（遞迴的 XY 切割，自訂的變形，照 `layout-with-model.mjs`） ----------

interface Block {
  bbox: Box;
  region?: Region;
  orphan?: { seg: number; items: Item[] };
}

function merged(ranges: Array<[number, number]>): Array<[number, number]> {
  const r = ranges.filter(([a, b]) => b > a).sort((p, q) => p[0] - q[0]);
  const out: Array<[number, number]> = [];
  for (const [a, b] of r) {
    const last = out[out.length - 1];
    if (last && a <= last[1]) last[1] = Math.max(last[1], b);
    else out.push([a, b]);
  }
  return out;
}

/**
 * 1. 有直向的縫（沒有任何區塊蓋到的 x 範圍）就切成幾欄，由左到右；
 * 2. 否則找橫向的縫：有寬區塊（佔這一塊寬度六成以上，例如跨欄的圖、摘要）時只在寬區塊的上下切，
 *    沒有寬區塊時只切最上面那一條縫（避免把左右兩欄剛好對齊的段落間距當成整頁的分界）；
 * 3. 都沒有縫就找中縫（`gutterOrder`）。
 */
function orderBlocks(blocks: Block[]): Block[] {
  if (blocks.length <= 1) return blocks;
  const pad = (b: Block, i: 0 | 1, j: 2 | 3): [number, number] => [b.bbox[i] + 1, Math.max(b.bbox[i] + 1.01, b.bbox[j] - 1)];
  const xs = merged(blocks.map((b) => pad(b, 0, 2)));
  if (xs.length >= 2) {
    const groups: Block[][] = xs.map(() => []);
    for (const b of blocks) {
      const k = xs.findIndex(([a, c]) => b.bbox[0] + 1 >= a - 0.01 && b.bbox[0] + 1 <= c);
      groups[Math.max(0, k)]?.push(b);
    }
    return groups.flatMap(orderBlocks);
  }
  const ys = merged(blocks.map((b) => pad(b, 1, 3)));
  if (ys.length >= 2) {
    const inBand = (b: Block) => Math.max(0, ys.findIndex(([a, c]) => b.bbox[1] + 1 >= a - 0.01 && b.bbox[1] + 1 <= c));
    const left = Math.min(...blocks.map((b) => b.bbox[0]));
    const right = Math.max(...blocks.map((b) => b.bbox[2]));
    const wideBand = ys.map(() => false);
    for (const b of blocks) if (b.bbox[2] - b.bbox[0] >= 0.6 * (right - left)) wideBand[inBand(b)] = true;
    // 每一層：寬區塊所在的橫條自成一層，其餘相鄰的橫條併成一層；沒有寬區塊時第一條自成一層
    const anyWide = wideBand.some(Boolean);
    const layer: number[] = [];
    let k = -1;
    ys.forEach((_, i) => {
      if (i === 0 || wideBand[i] || wideBand[i - 1] || (!anyWide && i === 1)) k++;
      layer.push(k);
    });
    const parts: Block[][] = Array.from({ length: k + 1 }, () => []);
    for (const b of blocks) parts[layer[inBand(b)] ?? 0]?.push(b);
    if (parts.length >= 2) return parts.flatMap(orderBlocks);
  }
  return gutterOrder(blocks);
}

/**
 * 切不開的時候（區塊在 x、y 兩個方向都互相重疊）：找一條中縫（依高度加權、很少窄區塊跨過的 x），
 * 跨過中縫的區塊自成一層，層與層之間先左欄、再右欄；找不到中縫就由上到下。
 */
function gutterOrder(blocks: Block[]): Block[] {
  const left = Math.min(...blocks.map((b) => b.bbox[0]));
  const right = Math.max(...blocks.map((b) => b.bbox[2]));
  const width = right - left;
  const h = (b: Block) => b.bbox[3] - b.bbox[1];
  const narrow = blocks.filter((b) => b.bbox[2] - b.bbox[0] <= 0.6 * width);
  const total = narrow.reduce((s, b) => s + h(b), 0);
  let gutter: number | null = null;
  let best = Infinity;
  for (let x = left + 0.3 * width; x <= left + 0.7 * width; x += 2) {
    let cross = 0;
    let l = 0;
    let r = 0;
    for (const b of narrow) {
      if (b.bbox[0] < x - 2 && b.bbox[2] > x + 2) cross += h(b);
      else if (b.bbox[2] <= x + 2) l++;
      else r++;
    }
    if (l >= 2 && r >= 2 && cross < best) {
      best = cross;
      gutter = x;
    }
  }
  const sorted = [...blocks].sort((a, b) => a.bbox[1] - b.bbox[1] || a.bbox[0] - b.bbox[0]);
  if (gutter === null || best > 0.1 * total) return sorted;
  const out: Block[] = [];
  let L: Block[] = [];
  let R: Block[] = [];
  for (const b of sorted) {
    if (b.bbox[2] <= gutter + 4) L.push(b);
    else if (b.bbox[0] >= gutter - 4) R.push(b);
    else {
      out.push(...L, ...R, b);
      L = [];
      R = [];
    }
  }
  out.push(...L, ...R);
  return out;
}

const kindOf = (role: Role): SegmentKind =>
  role === "title" || role === "heading" ? "heading" : role === "caption" ? "caption" : "body";

// ---------- 主程式 ----------

/** 一頁：規則段落加模型的區塊 → 段落。 */
function pageSegments(pi: number, page: PageInput, rules: LayoutSegment[], boxes: RegionBox[]): LayoutSegment[] {
  const items = prepareItems(page);
  let kept = nms(
    boxes
      .map((b) => ({ role: ROLES[b.label] ?? "other", score: b.score, bbox: b.bbox, items: [] as Item[] }))
      .filter((r) => r.role !== "skip" && r.bbox.every(Number.isFinite) && area(r.bbox) > 0),
  );
  // 包在要翻的區塊裡的小公式框（行內公式）：不當成區塊，字留給外面的正文
  kept = kept.filter(
    (b) =>
      !(
        b.role === "formula" &&
        kept.some((o) => o !== b && TRANSLATE_ROLES.has(o.role) && overlap(b.bbox, o.bbox) / (area(b.bbox) || 1) >= 0.8)
      ),
  );
  const rsegs = rules.map((s) => toTopLeft(page.view, s.bbox));
  for (const it of items) {
    let best: Region | null = null;
    for (const r of kept) if (inside(r.bbox, it.x, it.y, 1) && (best === null || area(r.bbox) < area(best.bbox))) best = r;
    it.region = best;
    let bs: number | null = null;
    rsegs.forEach((b, si) => {
      if (inside(b, it.x, it.y) && (bs === null || area(b) < area(rsegs[bs] ?? b))) bs = si;
    });
    it.rseg = bs;
  }
  // 落在區塊外的字（模型的方框差了幾點）：歸給它所屬規則段落的字最多落在的那個區塊
  const adopted = new Map<Item, Region>();
  for (const it of items) {
    if (it.region || it.rseg === null) continue;
    const count = new Map<Region, number>();
    for (const o of items) if (o.rseg === it.rseg && o.region) count.set(o.region, (count.get(o.region) ?? 0) + o.len);
    let bestR: Region | null = null;
    let n = 0;
    for (const [r, c] of count) {
      if (c > n) {
        bestR = r;
        n = c;
      }
    }
    if (bestR) adopted.set(it, bestR);
  }
  for (const [it, r] of adopted) it.region = r;
  for (const it of items) it.region?.items.push(it);
  // 還是沒有區塊、規則留下的字：照規則的段落，當成一個個小區塊
  const orphans = new Map<number, Item[]>();
  for (const it of items) {
    if (it.region || it.rseg === null) continue;
    const g = orphans.get(it.rseg) ?? [];
    g.push(it);
    orphans.set(it.rseg, g);
  }
  const blocks: Block[] = [
    ...kept.map((r) => ({ bbox: r.bbox, region: r })),
    ...[...orphans.entries()].map(([seg, its]): Block => ({ bbox: rsegs[seg] ?? [0, 0, 0, 0], orphan: { seg, items: its } })),
  ];
  const out: LayoutSegment[] = [];
  for (const b of orderBlocks(blocks)) {
    if (b.orphan) {
      const s = segmentOf(pi, b.orphan.items.map((it) => it.raw), rules[b.orphan.seg]?.kind ?? "body");
      if (s) out.push(s);
      continue;
    }
    const r = b.region;
    if (!r || !TRANSLATE_ROLES.has(r.role)) continue;
    // 區塊要翻、而且規則也沒丟掉的字才翻
    const its = r.items.filter((it) => it.rseg !== null);
    if (its.length === 0) continue;
    const s = segmentOf(pi, its.map((it) => it.raw), kindOf(r.role));
    if (s) out.push(s);
  }
  return out;
}

/**
 * 整份文件的段落：`regions[頁序]` 有區塊的頁依模型分段，沒有的頁（`null` 或沒給）照規則。
 * 全部沒有區塊時和 `layoutPages(pages)` 逐項相同。
 */
export function layoutWithRegions(pages: PageInput[], regions: readonly (PageRegions | undefined)[]): LayoutSegment[] {
  const rules = layoutPages(pages);
  const out: LayoutSegment[] = [];
  pages.forEach((page, pi) => {
    const segsP = rules.filter((s) => s.page === pi);
    const boxes = regions[pi];
    if (!boxes) {
      out.push(...segsP);
      return;
    }
    out.push(...pageSegments(pi, page, segsP, boxes));
  });
  return out;
}
