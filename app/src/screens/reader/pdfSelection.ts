// PDF 上的選取 → 劃線（`pdfView.ts` 分出來的一塊）：把瀏覽器原生的選取換算成筆記檔要存的樣子。
// 只讀頁面的文字層與座標換算，不改任何狀態。

import type * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs";
import type { Rect } from "../../api/types";

/** 選取的文字，換算成可以存進筆記檔的樣子。 */
export interface SelectionCapture {
  /** 頁序，從 0 起算。 */
  pageIndex: number;
  rects: Rect[];
  exact: string;
  /** 選取的字數（空白收過、還沒截到 2000 字之前的；快速翻譯看它判斷是不是太長）。 */
  length: number;
  prefix: string;
  suffix: string;
  start: number;
  end: number;
  /** 第一行在畫面上的位置（放「劃線」鈕用）。 */
  anchor: { left: number; top: number; bottom: number; right: number };
}

/** 選取要用到的一頁：第幾頁（從 1 起算）、頁面的框、文字層、座標換算。 */
export interface SelectableSlot {
  n: number;
  box: HTMLElement;
  textLayer: HTMLElement | null;
  viewport: pdfjs.PageViewport | null;
}

/**
 * 目前選取的文字（瀏覽器原生的選取）換算成劃線要存的樣子。只取起點那一頁的部分；沒有選取、
 * 選取不在文字層裡就交回 `null`。`slotOf(n)` 交回第 `n` 頁（從 1 起算）。
 */
export function captureSelection(slotOf: (n: number) => SelectableSlot | undefined): SelectionCapture | null {
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0 || sel.isCollapsed) return null;
  const range = sel.getRangeAt(0);
  const startEl = elementOf(range.startContainer)?.closest<HTMLElement>(".pg");
  if (!startEl) return null;
  const slot = slotOf(Number(startEl.dataset["n"]));
  const tl = slot?.textLayer;
  const vp = slot?.viewport;
  if (!slot || !tl || !vp || !tl.contains(range.startContainer)) return null;
  const r = range.cloneRange();
  if (!tl.contains(range.endContainer)) r.setEnd(tl, tl.childNodes.length);
  const raw = r.toString();
  const whole = (selectedText(r, tl) ?? raw).replace(/\s+/g, " ").trim();
  const exact = whole.slice(0, 2000);
  if (exact === "") return null;
  const box = slot.box.getBoundingClientRect();
  const rects = Array.from(r.getClientRects()).filter(
    (c) =>
      c.width > 0.5 &&
      c.height > 0.5 &&
      c.left + c.width / 2 >= box.left &&
      c.left + c.width / 2 <= box.right &&
      c.top + c.height / 2 >= box.top &&
      c.top + c.height / 2 <= box.bottom,
  );
  if (rects.length === 0) return null;
  const lines = mergeLines(rects);
  const pdfRects: Rect[] = lines.map((l) => {
    const [x1, y1] = vp.convertToPdfPoint(l.left - box.left, l.top - box.top);
    const [x2, y2] = vp.convertToPdfPoint(l.right - box.left, l.bottom - box.top);
    const a = x1 ?? 0;
    const b = y1 ?? 0;
    const c = x2 ?? 0;
    const d = y2 ?? 0;
    return [Math.min(a, c), Math.min(b, d), Math.max(a, c), Math.max(b, d)];
  });
  // 這一頁文字裡的位移（文字層裡全部文字接起來的第幾個字）
  const pre = document.createRange();
  pre.selectNodeContents(tl);
  pre.setEnd(r.startContainer, r.startOffset);
  const start = pre.toString().length;
  const end = start + raw.length;
  const full = tl.textContent ?? "";
  const first = lines[0];
  if (!first) return null;
  return {
    pageIndex: slot.n - 1,
    rects: pdfRects,
    exact,
    length: whole.length,
    prefix: full.slice(Math.max(0, start - 32), start),
    suffix: full.slice(end, end + 32),
    start,
    end,
    anchor: { left: first.left, top: first.top, bottom: first.bottom, right: first.right },
  };
}

/**
 * 畫面裡第一行看得到的文字，當成「這一頁的短評」的依據（沒有選字時用）：選起那一行、換算、再取消選取。
 * `slot` 是目前那一頁；`capture` 換算目前的選取。
 */
export function firstVisibleLine(
  scroll: HTMLElement,
  slot: SelectableSlot | undefined,
  capture: () => SelectionCapture | null,
): SelectionCapture | null {
  const sc = scroll.getBoundingClientRect();
  const tl = slot?.textLayer;
  if (!slot || !tl) return null;
  const spans = Array.from(tl.querySelectorAll<HTMLElement>("span")).filter(
    (s) => (s.textContent ?? "").trim() !== "" && s.getBoundingClientRect().bottom > sc.top + 56,
  );
  const target = spans[0];
  if (!target || !target.firstChild) return null;
  const range = document.createRange();
  range.selectNodeContents(target);
  const sel = window.getSelection();
  sel?.removeAllRanges();
  sel?.addRange(range);
  const cap = capture();
  sel?.removeAllRanges();
  return cap;
}

/**
 * 一頁文字層的文字，換行處（文字層的 `<br>`）補一個空格，連同選取的起訖（`start`、`end` 是文字層全部文字接起來的位移，
 * 同 `captureSelection`）換算到這份文字裡的位置。快速翻譯取前後各一句用。只走一次節點，不量版面。
 */
export function layerText(layer: HTMLElement, start: number, end: number): { text: string; start: number; end: number } {
  const walker = document.createTreeWalker(layer, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
  let text = "";
  let raw = 0;
  let s = -1;
  let e = -1;
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (n instanceof Text) {
      const len = n.data.length;
      if (s < 0 && start <= raw + len) s = text.length + (start - raw);
      if (e < 0 && end <= raw + len) e = text.length + (end - raw);
      text += n.data;
      raw += len;
    } else if (n instanceof HTMLBRElement && !/\s$/.test(text)) {
      text += " ";
    }
  }
  return { text, start: s < 0 ? text.length : s, end: e < 0 ? text.length : e };
}

/**
 * 選取的字，行與行之間補一個空格（`Range.toString()` 不含文字層裡換行的標記，兩行會黏在一起）。
 * 每一段文字取被選到的那一部分；讀不出來交回 `null`。
 */
function selectedText(range: Range, layer: HTMLElement): string | null {
  let out = "";
  let lastTop: number | null = null;
  for (const span of layer.querySelectorAll("span")) {
    if (!range.intersectsNode(span)) continue;
    const node = span.firstChild;
    if (!(node instanceof Text)) continue;
    let from = 0;
    let to = node.length;
    if (node === range.startContainer) from = range.startOffset;
    if (node === range.endContainer) to = range.endOffset;
    if (to <= from) continue;
    const top = span.getBoundingClientRect().top;
    if (lastTop !== null && Math.abs(top - lastTop) > 2) out += " ";
    lastTop = top;
    out += node.data.slice(from, to);
  }
  return out === "" ? null : out;
}

function elementOf(node: Node): Element | null {
  return node instanceof Element ? node : node.parentElement;
}

interface LineBox {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** 選取的一堆小矩形，合成每一行一個矩形（同一行的上下有一半以上重疊就算同一行）。 */
function mergeLines(rects: DOMRect[]): LineBox[] {
  const sorted = [...rects].sort((a, b) => a.top - b.top || a.left - b.left);
  const lines: LineBox[] = [];
  for (const r of sorted) {
    const last = lines[lines.length - 1];
    if (last) {
      const overlap = Math.min(last.bottom, r.bottom) - Math.max(last.top, r.top);
      if (overlap > 0.5 * Math.min(last.bottom - last.top, r.height)) {
        last.left = Math.min(last.left, r.left);
        last.right = Math.max(last.right, r.right);
        last.top = Math.min(last.top, r.top);
        last.bottom = Math.max(last.bottom, r.bottom);
        continue;
      }
    }
    lines.push({ left: r.left, top: r.top, right: r.right, bottom: r.bottom });
  }
  return lines;
}
