// 原文一頁的文字位置 → PDF 頁面上的方框。`text.json` 的一頁是 `pageText(items)` 經 `wellFormed`
// （逐篇處理寫的），首頁搜尋交來的 `range` 是那個字串裡的 UTF-16 位置；這裡用同一頁的文字片段接出同一個
// 字串，那一段等於 `quote` 就框出涵蓋它的片段。不等（PDF.js 改版等）就在那一頁找 `quote`；再找不到交回 `null`，
// 閱讀畫面只到那一頁。畫面不另做正規化。

import type { Box } from "../../ext/types";
import { wellFormed } from "../../perPaper.ts";
import type { RawItem } from "../../translate/layout";

/** 一頁的片段接成的字串（同 `pageText` 加 `wellFormed`），與每個片段在字串裡的 UTF-16 起訖。 */
export function itemSpans(items: readonly RawItem[]): { text: string; spans: [number, number][] } {
  const spans: [number, number][] = [];
  let raw = "";
  for (const it of items) {
    spans.push([raw.length, raw.length + it.str.length]);
    raw += it.str + (it.hasEOL ? "\n" : "");
  }
  // 換掉單獨的代理碼不改 UTF-16 的長度，所以位置照樣對得上
  return { text: wellFormed(raw), spans };
}

/** 一個片段裡第 `a` 到 `b` 個 UTF-16 單位的方框：片段的寬度依字數平均分配（字形的寬度拿不到）。 */
function partBox(it: RawItem, a: number, b: number): Box {
  const [sa = 1, sb = 0, sc = 0, sd = 1, x = 0, y = 0] = it.transform;
  const h = Math.abs(sd) || it.height || 10;
  const len = Math.max(1, it.str.length);
  if (Math.abs(sb) > 0.01 || Math.abs(sc) > 0.01 || sa < 0) {
    // 旋轉的字：框整個片段
    return [x - h, y - h, x + it.width + h, y + it.width + h];
  }
  // 基線在 y：字身往上約 0.9 個字高、往下約 0.25 個字高
  return [x + (it.width * a) / len, y - h * 0.25, x + (it.width * b) / len, y + h * 0.9];
}

/**
 * `range`（UTF-16 的 `[起, 訖)`）涵蓋的那幾個片段的方框（聯集）。那一段不等於 `quote` 時改在這一頁找 `quote`；
 * 都找不到交回 `null`。
 */
export function rangeBox(items: readonly RawItem[], range: [number, number], quote: string): Box | null {
  const { text, spans } = itemSpans(items);
  let [s, e] = range;
  if (!(s >= 0 && s < e && e <= text.length && text.slice(s, e) === quote)) {
    if (quote === "") return null;
    const at = text.indexOf(quote);
    if (at < 0) return null;
    [s, e] = [at, at + quote.length];
  }
  let box: Box | null = null;
  items.forEach((it, i) => {
    const [a, b] = spans[i]!;
    const from = Math.max(s, a);
    const to = Math.min(e, b);
    if (from >= to) return;
    const p = partBox(it, from - a, to - a);
    box = box ? [Math.min(box[0], p[0]), Math.min(box[1], p[1]), Math.max(box[2], p[2]), Math.max(box[3], p[3])] : p;
  });
  return box;
}
