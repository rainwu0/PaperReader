// 閱讀畫面的短評標記排法（`src/screens/reader/commentLayout.ts`）與筆刷復原的動作清單（`inkHistory.ts`）的測試。
// 執行：node --test scripts/reader-undo-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { markerRoom, markerSize, placeMarkers, quoteHead } from "../src/screens/reader/commentLayout.ts";
import { lastOnPage, remapIds } from "../src/screens/reader/inkHistory.ts";

const SIZE = { min: 4, max: 9 };

test("標記邊長是行高的 0.42 倍，限制在範圍內", () => {
  assert.equal(markerSize(16, 5, 9), 7);
  assert.equal(markerSize(8, 5, 9), 5);
  assert.equal(markerSize(40, 5, 9), 9);
});

test("標記接在第一行行尾、上緣往上一點（行與行之間），不蓋這一行", () => {
  const [a] = placeMarkers([{ id: "a", lineTop: 100, lineBottom: 116, lineRight: 150 }], 344, SIZE, 3, 1, 0.5);
  assert.ok(a);
  assert.equal(a.size, 7);
  assert.equal(a.x, 151);
  assert.equal(a.y, 100 - 3.5);
  assert.ok(a.y + a.size <= 116, "整個標記在這一行的上半部以內");
});

test("行尾靠近頁緣時夾在頁面裡，不超出", () => {
  const [a] = placeMarkers([{ id: "a", lineTop: 100, lineBottom: 116, lineRight: 340 }], 344, SIZE, 3, 1, 0.5);
  assert.ok(a);
  assert.equal(a.x, 344 - 3 - 7);
});

test("兩個標記會疊在一起時，後面的往旁邊讓一個標記的寬度", () => {
  const spots = placeMarkers(
    [
      { id: "a", lineTop: 100, lineBottom: 116, lineRight: 150 },
      { id: "b", lineTop: 101, lineBottom: 117, lineRight: 151 },
      { id: "c", lineTop: 300, lineBottom: 316, lineRight: 150 },
    ],
    344,
    SIZE,
    3,
    1,
    0.5,
  );
  const at = (id) => spots.find((s) => s.id === id);
  assert.equal(at("a").x, 151);
  assert.equal(at("b").x, 151 + 7 + 1);
  assert.equal(at("c").x, 151, "不同行的不受影響");
});

test("放大到欄寬時欄外留的位置：放大後剛好放得下標記", () => {
  const room = 28;
  const width = 250;
  const screen = 396;
  const m = markerRoom(room, width, screen);
  const pxPerPt = screen / (width + 2 * m);
  assert.ok(Math.abs(m * pxPerPt - room) < 1e-9);
  assert.equal(markerRoom(300, 250, 396), 0, "畫面太窄就不留");
});


test("引句的開頭幾個字", () => {
  assert.equal(quoteHead("short  quote"), "short quote");
  assert.equal(quoteHead("a".repeat(30), 10), `${"a".repeat(10)}…`);
});

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
