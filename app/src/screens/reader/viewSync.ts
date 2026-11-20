// 兩份頁面幾何相同的 PDF 之間「同一處」的換算：原文與譯本切換時帶著的位置，以及中英對照兩格的同步。
// 只做算術，不碰畫面元件：`pdfView.ts` 量好頁框與捲動區交進來，算出的捲動位置由它去設。
//
// 位置用「畫面中線落在哪一頁的幾分之幾」記，不用像素：兩格的高度不同、譯本 PDF 的頁面大小和原文不同時，
// 照頁與比例對齊仍是同一處。縮放是相對於「頁寬剛好填滿」的倍率，所以兩格寬度不同時，放大到欄寬仍是同一欄。

/**
 * 原文與譯本之間切換、兩格同步時帶著的位置（兩份版面相同，所以這四個值就對得上）：
 * 頁序、畫面中線落在該頁高度的比例（0 到 1）、水平捲動占可捲範圍的比例（0 到 1）、縮放倍率。
 */
export interface SwitchPosition {
  page: number;
  mid: number;
  hx: number;
  zoom: number;
}

/** 一頁的頁框在捲動內容裡的上緣與高度（CSS 像素）。依頁序排好。 */
export interface PageBox {
  top: number;
  height: number;
}

/** 捲動區：捲到哪、看得到的大小、內容的寬度。 */
export interface ScrollBox {
  top: number;
  left: number;
  height: number;
  width: number;
  scrollWidth: number;
}

export function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

/** 最後一頁頂端不低於 `y` 的那一頁（頁序從 0 起）；沒有頁時是 0。 */
export function pageIndexAt(pages: readonly PageBox[], y: number): number {
  let lo = 0;
  let hi = pages.length - 1;
  while (lo < hi) {
    const m = (lo + hi + 1) >> 1;
    const p = pages[m];
    if (p && p.top <= y) lo = m;
    else hi = m - 1;
  }
  return Math.max(0, lo);
}

/** 量到的頁框與捲動區 → 位置（畫面中線在哪一頁的哪個高度、水平捲到哪、縮放多少）。 */
export function switchAt(pages: readonly PageBox[], sc: ScrollBox, zoom: number): SwitchPosition {
  const midY = sc.top + sc.height / 2;
  const i = pageIndexAt(pages, midY);
  const p = pages[i];
  const range = sc.scrollWidth - sc.width;
  return {
    page: p ? i + 1 : 1,
    mid: p ? clamp((midY - p.top) / (p.height || 1), 0, 1) : 0,
    hx: range > 1 ? clamp(sc.left / range, 0, 1) : 0,
    zoom,
  };
}

/**
 * 位置 → 這一格要捲到哪（縮放已經套用、頁框已經是新的大小之後才算）。
 * 頁序超出這份 PDF 的頁數時停在最後一頁的中間，`clamped` 是 `true`。
 */
export function scrollFor(
  pages: readonly PageBox[],
  p: SwitchPosition,
  sc: Pick<ScrollBox, "height" | "width" | "scrollWidth">,
): { top: number; left: number; clamped: boolean } {
  const n = pages.length;
  const clamped = p.page > n;
  const box = pages[clamp(p.page, 1, Math.max(1, n)) - 1];
  const mid = clamped ? 0.5 : clamp(p.mid, 0, 1);
  const range = sc.scrollWidth - sc.width;
  return {
    top: box ? box.top + mid * box.height - sc.height / 2 : 0,
    left: range > 1 ? clamp(p.hx, 0, 1) * range : 0,
    clamped,
  };
}

/** 一格被另一格帶著捲到的位置（設下去之後讀回來的、瀏覽器夾過的值）與縮放。 */
export interface FollowMark {
  top: number;
  left: number;
  zoom: number;
}

/**
 * 跟著的那一格回報「動了」：是自己被帶過去的回聲（位置還是剛才被設的那裡，不往回帶），
 * 還是它自己真的動了（例如在它上面跳頁）。容許 `tol` 像素的取整誤差。
 */
export function isEcho(mark: FollowMark | null, now: FollowMark, tol = 2): boolean {
  if (!mark) return false;
  return Math.abs(mark.zoom - now.zoom) < 1e-3 && Math.abs(mark.top - now.top) <= tol && Math.abs(mark.left - now.left) <= tol;
}

/** 譯本那一格占的比例（下面或右邊）：夾在兩格都還讀得到的範圍裡。 */
export const SPLIT_MIN = 0.25;
export const SPLIT_MAX = 0.75;
export const SPLIT_DEFAULT = 0.5;
export function clampSplit(f: number): number {
  return Number.isFinite(f) ? clamp(f, SPLIT_MIN, SPLIT_MAX) : SPLIT_DEFAULT;
}

/**
 * 分隔線拖到 `pointer`（畫面座標）時，譯本那一格占的比例。`start` 與 `size` 是兩格合起來那一塊的起點與長度
 * （上下分時是 y 與高，左右分時是 x 與寬）；譯本那一格在後面（下面或右邊）。
 */
export function splitAt(pointer: number, start: number, size: number): number {
  if (size <= 0) return SPLIT_DEFAULT;
  return clampSplit((start + size - pointer) / size);
}
