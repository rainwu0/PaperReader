// 閱讀畫面的復原：筆刷復原的動作清單（`src/screens/reader/inkHistory.ts`）的測試。
// 執行：node --test scripts/reader-undo-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { lastOnPage, remapIds } from "../src/screens/reader/inkHistory.ts";

test("筆刷復原：只找畫面上看得到的頁上最後一個動作", () => {
  const list = [
    { kind: "draw", material: "m", page: 0, id: "s1" },
    { kind: "draw", material: "m", page: 1, id: "s2" },
    { kind: "draw", material: "x", page: 0, id: "s3" },
  ];
  assert.equal(lastOnPage(list, "m", [0]), 0);
  assert.equal(lastOnPage(list, "m", [1]), 1);
  assert.equal(lastOnPage(list, "m", [0, 1]), 1, "兩頁都看得到：較晚的那一個");
  assert.equal(lastOnPage(list, "m", [2]), -1);
});

test("畫回去之後，指著舊代號的動作改指新的", () => {
  const stroke = { id: "a", material: "m", page: 0 };
  const list = [
    { kind: "draw", material: "m", page: 0, id: "a" },
    { kind: "clear", material: "m", page: 0, strokes: [stroke, { id: "b", material: "m", page: 0 }] },
  ];
  const out = remapIds(list, new Map([["a", "a2"]]));
  assert.equal(out[0].id, "a2");
  assert.deepEqual(
    out[1].strokes.map((s) => s.id),
    ["a2", "b"],
  );
  assert.equal(list[0].id, "a", "原本的清單不動");
  const same = remapIds(list, new Map([["z", "z2"]]));
  assert.equal(same[1], list[1], "沒改到的動作是同一個物件");
});
