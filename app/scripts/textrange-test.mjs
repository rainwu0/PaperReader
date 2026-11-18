// 原文一頁的 UTF-16 位置 → 片段的方框（`src/screens/reader/textRange.ts`）。
// 執行：node --test scripts/textrange-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { itemSpans, rangeBox } from "../src/screens/reader/textRange.ts";
import { pageText } from "../src/translate/layout.ts";

/** 一個片段：起點 (x, y)，每個 UTF-16 單位寬 5 點，字高 10。 */
const item = (str, x, y, hasEOL = false) => ({ str, transform: [10, 0, 0, 10, x, y], width: str.length * 5, height: 10, hasEOL });

const items = [
  item("We propose a real-", 100, 700, true),
  item("time detector ", 100, 688),
  item("named RT-DETR.", 170, 688, true),
  item("Let \u{1D465} denote", 100, 676, true),
];

test("接出的字串和 pageText 相同，每個片段記下起訖", () => {
  const { text, spans } = itemSpans(items);
  assert.equal(text, pageText(items));
  assert.deepEqual(spans[0], [0, 18]);
  assert.deepEqual(spans[1], [19, 33]);
  assert.equal(text.slice(...spans[2]), "named RT-DETR.");
});

test("一個片段裡的一段：寬度依字數分配", () => {
  const text = pageText(items);
  const at = text.indexOf("propose");
  const box = rangeBox(items, [at, at + 7], "propose");
  assert.deepEqual(box, [100 + 3 * 5, 700 - 2.5, 100 + 10 * 5, 700 + 9]);
});

test("跨兩個片段（同一行）與跨行", () => {
  const text = pageText(items);
  const at = text.indexOf("detector named");
  const box = rangeBox(items, [at, at + 14], "detector named");
  assert.equal(box[0], 100 + 5 * 5);
  assert.equal(box[2], 170 + 5 * 5);
  assert.equal(box[1], 688 - 2.5);
  // 行尾斷開的詞：涵蓋兩行
  const s = text.indexOf("real-");
  const q = text.slice(s, s + 10);
  assert.equal(q, "real-\ntime");
  const two = rangeBox(items, [s, s + 10], q);
  assert.equal(two[1], 688 - 2.5);
  assert.equal(two[3], 700 + 9);
});

test("基本多文種平面以外的字（U+1D465）佔兩個 UTF-16 單位", () => {
  const text = pageText(items);
  const at = text.indexOf("\u{1D465}");
  const box = rangeBox(items, [at, at + 2], "\u{1D465}");
  assert.deepEqual(box, [100 + 4 * 5, 676 - 2.5, 100 + 6 * 5, 676 + 9]);
});

test("單獨的代理碼：先換成 U+FFFD 再比 quote（長度不變）", () => {
  const lone = [item("ab\uD835cd", 0, 0, true)];
  const box = rangeBox(lone, [1, 4], "b�c");
  assert.deepEqual(box, [5, -2.5, 20, 9]);
});

test("起訖對不上時改在那一頁找 quote；都找不到交回 null", () => {
  const text = pageText(items);
  const at = text.indexOf("detector");
  // 位置錯了，但 quote 在這一頁
  assert.deepEqual(rangeBox(items, [0, 8], "detector"), rangeBox(items, [at, at + 8], "detector"));
  // 超出範圍
  assert.deepEqual(rangeBox(items, [500, 508], "detector"), rangeBox(items, [at, at + 8], "detector"));
  assert.equal(rangeBox(items, [0, 5], "nowhere"), null);
  assert.equal(rangeBox(items, [0, 0], ""), null);
  assert.equal(rangeBox([], [0, 3], "abc"), null);
});
