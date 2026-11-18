// 譯文排字（`src/translate/typeset.ts`）的測試：換行、縮字級、標點規則、截斷、左右對齊。
// 執行：node --test scripts/typeset-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { typeset } from "../src/translate/typeset.ts";

/** 中日韓字寬一個字級、英數半個字級、空白四分之一；控制字元沒有字形。 */
const advance = (cp) => {
  if (cp < 32) return null;
  if (cp === 32) return 250;
  if (cp < 0x2e80) return 500;
  return 1000;
};

const base = { align: "left", advance };
const lineWidth = (r, text) => [...text].reduce((w, ch) => w + (advance(ch.codePointAt(0)) ?? 0), 0) * r.size / 1000;

test("放得下就用原字級、一行", () => {
  const r = typeset({ ...base, text: "這是一句短的譯文", width: 200, height: 20, size: 10 });
  assert.equal(r.size, 10);
  assert.equal(r.lines.length, 1);
  assert.equal(r.lines[0].text, "這是一句短的譯文");
  assert.equal(r.overflow, 0);
  assert.equal(r.truncated, false);
});

test("中文隨處可斷，每行不超過範圍的寬度", () => {
  const text = "字".repeat(25);
  const r = typeset({ ...base, text, width: 100, height: 100, size: 10 });
  assert.equal(r.size, 10);
  assert.deepEqual(
    r.lines.map((l) => [...l.text].length),
    [10, 10, 5],
  );
  for (const l of r.lines) assert.ok(lineWidth(r, l.text) <= 100.01);
});

test("英文單字整個移到下一行，太長的單字才在中間斷", () => {
  const r = typeset({ ...base, text: "alpha beta gamma delta", width: 40, height: 100, size: 10 });
  // 每個字母 5 點、空白 2.5：alpha=25、「alpha beta」=47.5 放不下
  assert.deepEqual(
    r.lines.map((l) => l.text),
    ["alpha", "beta", "gamma", "delta"],
  );
  const url = "https://example.org/a/very/long/path";
  const u = typeset({ ...base, text: url, width: 50, height: 100, size: 10 });
  assert.ok(u.lines.length > 1);
  assert.equal(u.lines.map((l) => l.text).join(""), url);
  for (const l of u.lines) assert.ok(lineWidth(u, l.text) <= 50.01);
});

test("收尾標點不在行首：吊在行尾；開頭標點不在行尾", () => {
  // 10 個字剛好一行，第 11 個是「。」
  const t1 = "字".repeat(10) + "。後面還有字";
  const r1 = typeset({ ...base, text: t1, width: 100, height: 100, size: 10 });
  assert.equal(r1.lines[0].text, "字".repeat(10) + "。");
  assert.ok(!r1.lines.some((l) => l.text.startsWith("。")));
  // 第 10 個位置是「（」，它要跟後面的字一起去下一行
  const t2 = "字".repeat(9) + "（括號內的字）";
  const r2 = typeset({ ...base, text: t2, width: 100, height: 100, size: 10 });
  assert.ok(!r2.lines.some((l) => l.text.endsWith("（")));
  assert.ok(r2.lines[1].text.startsWith("（"));
});

test("放不下就縮字級，不小於下限，縮到剛好放得下", () => {
  const text = "字".repeat(100);
  const r = typeset({ ...base, text, width: 100, height: 65, size: 10 });
  assert.ok(r.size < 10, `size ${r.size}`);
  assert.ok(r.size >= 6 - 1e-9, `size ${r.size}`);
  assert.ok(r.lines.length * r.lineHeight <= 65 + 0.01);
  assert.equal(r.overflow, 0);
  assert.equal(r.truncated, false);
});

test("縮到下限還放不下：多出一點可以，再多就截斷並補「…」", () => {
  // 100 個字、下限 6 點：每行 16 個字、7 行、行距 7.8 → 54.6；範圍高 50 → 多出 4.6，在 25% 之內
  const some = typeset({ ...base, text: "字".repeat(100), width: 100, height: 50, size: 10 });
  assert.equal(some.size, 6);
  assert.equal(some.truncated, false);
  assert.ok(some.overflow > 0 && some.overflow <= 50 * 0.25 + 0.01);
  // 範圍太矮：截斷
  const cut = typeset({ ...base, text: "字".repeat(300), width: 100, height: 20, size: 10 });
  assert.equal(cut.truncated, true);
  assert.ok(cut.lines[cut.lines.length - 1].text.endsWith("…"));
  assert.ok(cut.lines.length * cut.lineHeight <= 20 * 1.25 + 0.01);
  for (const l of cut.lines) assert.ok(lineWidth(cut, l.text) <= 100.01);
});

test("可以指定字級下限", () => {
  const r = typeset({ ...base, text: "字".repeat(400), width: 100, height: 30, size: 10, minSize: 9 });
  assert.equal(r.size, 9);
});

test("左右對齊：非最後一行把空隙平均分給每個字，最後一行靠左", () => {
  // 10 個字寬 100；每行放 9 個中文字再放不下第 10 個：寬 95 → 每行 9 個字，剩 5
  const r = typeset({ ...base, align: "justify", text: "字".repeat(20), width: 95, height: 100, size: 10 });
  const [a, b, c] = r.lines;
  assert.equal([...a.text].length, 9);
  // 9 個字、8 個間隙：5 / 8
  assert.ok(Math.abs(a.charSpace * 8 + lineWidth(r, a.text) - 95) < 1e-6);
  assert.ok(Math.abs(b.charSpace * 8 + lineWidth(r, b.text) - 95) < 1e-6);
  assert.equal(c.charSpace, 0);
});

test("左右對齊：空隙太大就靠左，不把字拉得稀稀疏疏", () => {
  const r = typeset({ ...base, align: "justify", text: "字字字字字字字字字字字字", width: 55, height: 100, size: 10 });
  // 每行 5 個字（50），剩 5 要分給 4 個間隙 = 1.25，超過字級的 20%（2）？沒超過，會對齊
  assert.ok(r.lines[0].charSpace > 0);
  const wide = typeset({ ...base, align: "justify", text: "字字字字字字字字字字字字", width: 59, height: 100, size: 10 });
  // 每行 5 個字，剩 9 要分給 4 個間隙 = 2.25 > 2 → 靠左
  assert.equal(wide.lines[0].charSpace, 0);
});

test("置中", () => {
  const r = typeset({ ...base, align: "center", text: "標題", width: 100, height: 20, size: 10 });
  assert.equal(r.lines[0].dx, 40);
});

test("字型沒有的字不畫、寬度不算", () => {
  const adv = (cp) => (cp === 0x41 ? null : 1000);
  const r = typeset({ align: "left", advance: adv, text: "字A字A字", width: 30, height: 20, size: 10 });
  assert.equal(r.lines.length, 1);
  assert.equal(r.size, 10);
});

test("空的譯文沒有行", () => {
  const r = typeset({ ...base, text: "   ", width: 100, height: 20, size: 10 });
  assert.equal(r.lines.length, 0);
  assert.equal(r.overflow, 0);
});
