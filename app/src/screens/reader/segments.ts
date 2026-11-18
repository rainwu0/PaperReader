// 譯文疊加檔裡段落的純計算（不碰畫面、不碰外殼，Node 的測試直接引入：`scripts/segments-test.mjs`）。
// 譯文檢視與 PDF 上點一段共用：目錄、點到哪一段、讀到哪一段、譯本 PDF 是否落後、工具箱「譯本」該切到哪一份。

/** 這裡用得到的段落欄位（`OverlaySegment` 的一部分）。`page` 是頁序（從 0 起），`bbox` 是 PDF 頁面座標。 */
export interface Seg {
  id: string;
  page: number;
  bbox: [number, number, number, number];
  kind: string;
  source: string;
  target: string | null;
  status: string;
  modified?: string | null;
  by_user?: boolean;
  service?: unknown;
}

/** 這一段讀的時候顯示什麼：翻好的顯示譯文，沒翻或翻失敗的顯示原文並標「沒翻」（A9）。 */
export function shownText(s: Seg): { text: string; untranslated: boolean } {
  const done = s.status === "done" && !!s.target && s.target.trim() !== "";
  return done ? { text: s.target ?? "", untranslated: false } : { text: s.source, untranslated: true };
}

/** 使用者改過的（就地修改；`by_user` 而且沒有記服務）與在並排比較挑的（`by_user` 加上服務）。 */
export function editMark(s: Seg): "edited" | "picked" | null {
  if (!s.by_user) return null;
  return s.service ? "picked" : "edited";
}

export interface TocEntry {
  id: string;
  text: string;
  page: number;
}

/** 目錄：標題段落依閱讀順序（A3）。 */
export function tableOfContents(segs: readonly Seg[]): TocEntry[] {
  return segs
    .filter((s) => s.kind === "heading")
    .map((s) => ({ id: s.id, text: shownText(s).text.replace(/\s+/g, " ").trim(), page: s.page }))
    .filter((e) => e.text !== "");
}

/** 點到 `(x, y)`（第 `page` 頁，PDF 座標）落在哪一段：方框（加上容差）包住這一點的；幾段都包住時取面積最小的。 */
export function segmentAt<T extends Seg>(segs: readonly T[], page: number, x: number, y: number, tol = 4): T | null {
  let best: T | null = null;
  let bestArea = Infinity;
  for (const s of segs) {
    if (s.page !== page) continue;
    const [x0, y0, x1, y1] = box(s);
    if (x < x0 - tol || x > x1 + tol || y < y0 - tol || y > y1 + tol) continue;
    const area = (x1 - x0) * (y1 - y0);
    if (area < bestArea) {
      best = s;
      bestArea = area;
    }
  }
  return best;
}

/**
 * 讀到哪一段：第 `page` 頁裡離 `(x, y)` 最近的一段（點到方框的距離；雙欄時自然取同一欄的）。
 * 這一頁沒有段落時，取前面最近一頁的最後一段，再沒有就取後面第一段。
 */
export function nearestSegment<T extends Seg>(segs: readonly T[], page: number, x: number, y: number): T | null {
  let best: T | null = null;
  let bestD = Infinity;
  for (const s of segs) {
    if (s.page !== page) continue;
    const [x0, y0, x1, y1] = box(s);
    const dx = x < x0 ? x0 - x : x > x1 ? x - x1 : 0;
    const dy = y < y0 ? y0 - y : y > y1 ? y - y1 : 0;
    const d = Math.hypot(dx, dy);
    if (d < bestD) {
      best = s;
      bestD = d;
    }
  }
  if (best) return best;
  let before: T | null = null;
  for (const s of segs) {
    if (s.page < page && (!before || s.page >= before.page)) before = s;
  }
  return before ?? segs.find((s) => s.page > page) ?? null;
}

/** 產生的譯本 PDF 落後於譯文：有使用者改過或挑過的段落是在它產生之後才改的（A5）。 */
export function pdfBehind(segs: readonly Seg[], pdfTime: string | null | undefined): boolean {
  const made = pdfTime ? Date.parse(pdfTime) : NaN;
  if (Number.isNaN(made)) return false;
  return segs.some((s) => s.by_user && !!s.modified && Date.parse(s.modified) > made);
}

/** 工具箱「譯本」切到哪一份：一份 PDF（材料代號）或原頁疊譯文。 */
export type TranslationChoice = { kind: "pdf"; id: string } | { kind: "overlay" };

/**
 * 按一下「譯本」切到哪一份（A14）：上次選的那一份還在就是它；第一次（或那份不在了）依序：
 * 產生的譯本 PDF（最新的）→ 使用者加入的譯本 PDF → 原頁疊譯文。都沒有交回 `null`。
 * `generated`、`added` 是材料代號，最新的在前（`newestFirst`）；`last` 是記下的材料代號，或 `"overlay"`。
 */
export function preferredTranslation(
  have: { generated: readonly string[]; added: readonly string[]; overlay: boolean },
  last: string | null,
): TranslationChoice | null {
  if (last === "overlay" && have.overlay) return { kind: "overlay" };
  if (last && (have.generated.includes(last) || have.added.includes(last))) return { kind: "pdf", id: last };
  const gen = have.generated[0];
  if (gen) return { kind: "pdf", id: gen };
  const added = have.added[0];
  if (added) return { kind: "pdf", id: added };
  return have.overlay ? { kind: "overlay" } : null;
}

/** 材料依做出來的時間排，最新的在前；沒有時間的排最後（同一份清單裡原本的先後不變）。 */
export function newestFirst<T extends { provenance?: { time?: string | null } }>(list: readonly T[]): T[] {
  const time = (m: T) => {
    const v = m.provenance?.time ? Date.parse(m.provenance.time) : NaN;
    return Number.isNaN(v) ? -Infinity : v;
  };
  // 兩份都沒有時間時相減是 NaN：當成一樣（`|| 0`）
  return [...list].sort((a, b) => time(b) - time(a) || 0);
}

function box(s: Seg): [number, number, number, number] {
  const [a, b, c, d] = s.bbox;
  return [Math.min(a, c), Math.min(b, d), Math.max(a, c), Math.max(b, d)];
}
