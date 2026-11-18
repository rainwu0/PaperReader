// 重點筆記送出前找摘要（`src/screens/generate/digestInput.ts`）的測試。
// 執行：node --test scripts/digest-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { findAbstract } from "../src/screens/generate/digestInput.ts";

const long = (n) => Array.from({ length: n }, (_, i) => `word${i}`).join(" ");

test("Abstract 開頭的段落：取標籤後面的文字", () => {
  const segs = [
    { page: 0, kind: "heading", source: "A Great Title" },
    { page: 0, kind: "body", source: `Abstract—${long(40)}` },
    { page: 0, kind: "body", source: long(80) },
  ];
  assert.equal(findAbstract(segs), long(40));
});

test("只有「Abstract」標題：取後面第一段內文", () => {
  const segs = [
    { page: 0, kind: "heading", source: "Abstract" },
    { page: 0, kind: "body", source: "short" },
    { page: 0, kind: "body", source: long(30) },
  ];
  assert.equal(findAbstract(segs), long(30));
});

test("沒有標籤（物理期刊）：前兩頁第一段夠長的內文", () => {
  const segs = [
    { page: 0, kind: "heading", source: "Title" },
    { page: 0, kind: "body", source: long(10) },
    { page: 1, kind: "body", source: long(70) },
  ];
  assert.equal(findAbstract(segs), long(70));
});

test("第三頁以後不算；找不到是 null", () => {
  const segs = [
    { page: 0, kind: "body", source: "tiny" },
    { page: 2, kind: "body", source: `Abstract ${long(200)}` },
  ];
  assert.equal(findAbstract(segs), null);
});

test("中文的「摘要」也認得，最長 3000 字", () => {
  const body = "這".repeat(5000);
  assert.equal(findAbstract([{ page: 0, kind: "body", source: `摘要：${body}` }])?.length, 3000);
});
