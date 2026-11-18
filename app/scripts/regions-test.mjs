// 版面模型的分段（`src/translate/regions.ts`）的測試：用合成的文字層與區塊驗「一個區塊一段」、翻不翻、
// 閱讀順序、沒有區塊的頁照規則。
// 執行：node --test scripts/regions-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { layoutPages, segmentOf } from "../src/translate/layout.ts";
import { layoutWithRegions } from "../src/translate/regions.ts";

const VIEW = [0, 0, 612, 792];

function it(x, y, str, size = 10) {
  return { str, transform: [size, 0, 0, size, x, y], width: str.length * size * 0.5, height: size };
}

/** 一個段落：每行 `perLine` 個詞，行距 12。回傳 [items, 下一個 y]。 */
function para(x, y, words, { size = 10, perLine = 8, indent = 0 } = {}) {
  const items = [];
  for (let i = 0; i < words.length; i += perLine) {
    items.push(it(x + (i === 0 ? indent : 0), y, words.slice(i, i + perLine).join(" "), size));
    y -= 12;
  }
  return [items, y];
}

const id2 = (i) => String.fromCharCode(97 + Math.floor(i / 26)) + String.fromCharCode(97 + (i % 26));
const w = (prefix, n) => Array.from({ length: n }, (_, i) => `${prefix}${id2(i)}`);

/** PDF 座標的一組字 → 左上座標的方框（模型交回的單位），四周多留 `pad` 點。 */
function boxOf(items, pad = 2) {
  const x0 = Math.min(...items.map((i) => i.transform[4]));
  const x1 = Math.max(...items.map((i) => i.transform[4] + i.width));
  const y0 = Math.min(...items.map((i) => i.transform[5] - 0.25 * i.height));
  const y1 = Math.max(...items.map((i) => i.transform[5] + i.height));
  return [x0 - pad, VIEW[3] - y1 - pad, x1 + pad, VIEW[3] - y0 + pad];
}

test("沒有區塊時和規則逐項相同", () => {
  const [a, y1] = para(72, 700, w("alpha", 24), { perLine: 12 });
  const [b] = para(72, y1 - 14, w("theta", 24), { perLine: 12 });
  const pages = [{ view: VIEW, items: [...a, ...b] }, { view: VIEW, items: para(72, 700, w("gamma", 30))[0] }];
  const rules = layoutPages(pages);
  assert.deepEqual(layoutWithRegions(pages, [null, null]), rules);
  assert.deepEqual(layoutWithRegions(pages, []), rules);
});

test("規則切成兩段、模型說是一個正文區塊：交出一段", () => {
  // 第二段首行縮排，規則會切開
  const [a, y1] = para(72, 700, w("alpha", 24), { perLine: 12 });
  const [b] = para(72, y1, w("theta", 24), { perLine: 12, indent: 12 });
  const pages = [{ view: VIEW, items: [...a, ...b] }];
  assert.equal(layoutPages(pages).length, 2);
  const out = layoutWithRegions(pages, [[{ label: "text", score: 0.9, bbox: boxOf([...a, ...b]) }]]);
  assert.equal(out.length, 1, out.map((s) => s.source).join(" | "));
  assert.ok(out[0].source.startsWith("alphaaa") && out[0].source.endsWith("thetaax"), out[0].source);
  assert.equal(out[0].kind, "body");
});

test("不翻的類別整塊略過；標題的類別交出 heading", () => {
  // 第 1 頁篇名之後的字規則當成作者區，所以放在第 2 頁
  const first = { view: VIEW, items: para(72, 700, w("first", 24), { perLine: 12 })[0] };
  const title = it(72, 740, "2 Results", 12);
  const [body, y1] = para(72, 700, w("word", 24), { perLine: 12 });
  const [tab] = para(72, y1 - 30, w("cell", 24), { perLine: 12 });
  const pages = [first, { view: VIEW, items: [title, ...body, ...tab] }];
  assert.ok(layoutPages(pages).some((s) => s.source.startsWith("cellaa")), "規則會翻那張表");
  const out = layoutWithRegions(pages, [
    null,
    [
      { label: "paragraph_title", score: 0.9, bbox: boxOf([title]) },
      { label: "text", score: 0.9, bbox: boxOf(body) },
      { label: "table", score: 0.9, bbox: boxOf(tab) },
    ],
  ]).filter((s) => s.page === 1);
  assert.deepEqual(
    out.map((s) => s.kind),
    ["heading", "body"],
  );
  assert.ok(out.every((s) => !s.source.includes("cellaa")));
});

test("規則丟掉的字（頁首）即使落在正文區塊裡也不翻", () => {
  const pages = [];
  for (let p = 0; p < 3; p++) {
    const header = it(72, 770, "Journal of Examples 2024", 8);
    const [body] = para(72, 700, w("pw", 24), { perLine: 12 });
    pages.push({ view: VIEW, items: [header, ...body], header, body });
  }
  const out = layoutWithRegions(
    pages.map(({ view, items }) => ({ view, items })),
    pages.map((p) => [{ label: "text", score: 0.9, bbox: boxOf([p.header, ...p.body]) }]),
  );
  assert.equal(out.length, 3);
  assert.ok(out.every((s) => !s.source.includes("Journal")), out.map((s) => s.source).join(" | "));
});

test("雙欄：跨欄的區塊先、左欄、再右欄；重疊的區塊留分數高的", () => {
  const head = it(72, 740, "A Study of Column Reading Order Across Two Columns", 16);
  head.width = 468;
  const [left] = para(72, 700, w("L", 48), { perLine: 8 });
  const [right] = para(322, 700, w("R", 48), { perLine: 8 });
  for (const x of [...left, ...right]) x.width = 218;
  const pages = [{ view: VIEW, items: [head, ...right, ...left] }];
  const out = layoutWithRegions(pages, [
    [
      { label: "text", score: 0.95, bbox: boxOf(right) },
      { label: "text", score: 0.6, bbox: boxOf(right, 3) },
      { label: "text", score: 0.9, bbox: boxOf(left) },
      { label: "doc_title", score: 0.9, bbox: boxOf([head]) },
    ],
  ]);
  assert.deepEqual(
    out.map((s) => s.source.slice(0, 3)),
    ["A S", "Laa", "Raa"],
  );
  assert.equal(out[0].kind, "heading");
});

test("落在區塊外幾點的字歸回同一段；沒有區塊的頁照規則", () => {
  const [a] = para(72, 700, w("alpha", 36), { perLine: 12 });
  // 方框少蓋了最後一行
  const box = boxOf(a.slice(0, 2), 0);
  const pages = [
    { view: VIEW, items: a },
    { view: VIEW, items: para(72, 700, w("gamma", 30))[0] },
  ];
  const out = layoutWithRegions(pages, [[{ label: "text", score: 0.9, bbox: box }], null]);
  assert.equal(out.filter((s) => s.page === 0).length, 1);
  assert.ok(out[0].source.endsWith("alphabj"), out[0].source);
  assert.deepEqual(
    out.filter((s) => s.page === 1),
    layoutPages(pages).filter((s) => s.page === 1),
  );
});

test("一個區塊的字組成一段：行尾連字號接回，方框包住全部的字", () => {
  const items = [it(72, 700, "the quick brown fox jumps over the lazy dog transform-", 10), it(72, 688, "ation of the input", 10)];
  const s = segmentOf(0, items, "body");
  assert.ok(s);
  assert.equal(s.source, "the quick brown fox jumps over the lazy dog transformation of the input");
  assert.ok(s.bbox[1] < 688 && s.bbox[3] > 700 && s.bbox[0] < 72);
  assert.equal(segmentOf(0, [], "body"), null);
});

test("上標、下標貼回主行：行尾的連字號照樣接回（公開組 arxiv_2611.16305 第 7 頁的寫法）", () => {
  const items = [
    it(72, 700, "is the safety factor and ϵ", 10),
    it(200, 698.5, "s", 7), // 下標，比主行低 1.5 點
    it(210, 700, "= 10", 10),
    it(232, 703.6, "−3", 7), // 上標，比主行高 3.6 點：buildLines 以它為一行的開頭，下標就另成一行
    it(245, 700, "pro-", 10),
    it(72, 688, "vides a small numerical buffer", 10),
  ];
  const s = segmentOf(0, items, "body");
  assert.ok(s.source.includes("provides a small"), s.source);
  assert.ok(!/pros|s vides/.test(s.source), s.source);
  // 方框照樣包住上標與下標的中心（量測與閱讀畫面都依中心點歸段）
  assert.ok(s.bbox[3] >= 703.6 + 0.3 * 7 && s.bbox[1] <= 698.5 + 0.3 * 7, String(s.bbox));
});
