// 閱讀畫面的短評顯示層排法（`src/screens/reader/commentLayout.ts`）與筆刷復原的動作清單（`inkHistory.ts`）的測試。
// 執行：node --test scripts/reader-undo-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { cardWidth, markerRoom, placeCards, placeMarkers, quoteHead, sideOf } from "../src/screens/reader/commentLayout.ts";
import { lastOnPage, remapIds } from "../src/screens/reader/inkHistory.ts";

const OPTS = { width: 260, inset: 6, gap: 6 };

test("標記放在靠近那一欄的頁緣：左欄放左邊，右欄與跨中線的放右邊", () => {
  assert.equal(sideOf({ left: 40, right: 160 }, 344), "left");
  assert.equal(sideOf({ left: 190, right: 320 }, 344), "right");
  assert.equal(sideOf({ left: 40, right: 300 }, 344), "right");
});

test("同一側靠得太近的標記往下讓，不疊在一起", () => {
  const spots = placeMarkers(
    [
      { id: "a", lineTop: 100, lineBottom: 110, left: 40, right: 120 },
      { id: "b", lineTop: 102, lineBottom: 112, left: 50, right: 130 },
      { id: "c", lineTop: 100, lineBottom: 110, left: 200, right: 300 },
    ],
    344,
    20,
    4,
  );
  const a = spots.find((s) => s.id === "a");
  const b = spots.find((s) => s.id === "b");
  const c = spots.find((s) => s.id === "c");
  assert.ok(a && b && c);
  assert.equal(a.x, 4);
  assert.equal(c.x, 344 - 4 - 20);
  assert.equal(a.y, 95);
  assert.ok(b.y >= a.y + 20, "b 在 a 下面");
  assert.equal(c.y, 95, "另一側不受影響");
});

test("知道欄的範圍時，標記貼著那一欄的外側；不超出頁面", () => {
  const spots = placeMarkers(
    [
      { id: "l", lineTop: 100, lineBottom: 110, left: 40, right: 120, colLeft: 32, colRight: 190 },
      { id: "r", lineTop: 100, lineBottom: 110, left: 210, right: 300, colLeft: 205, colRight: 364 },
      { id: "edge", lineTop: 200, lineBottom: 210, left: 210, right: 300, colLeft: 205, colRight: 390 },
    ],
    396,
    22,
    3,
    3,
  );
  const at = (id) => spots.find((s) => s.id === id);
  assert.equal(at("l").x, 32 - 3 - 22);
  assert.equal(at("r").x, 364 + 3);
  assert.equal(at("edge").x, 396 - 3 - 22, "欄緊貼頁緣時夾在頁面裡");
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

test("窄頁面：卡片同一欄排，互相讓位，不重疊", () => {
  const spots = placeCards(
    [
      { id: "a", side: "left", top: 100, height: 80 },
      { id: "b", side: "right", top: 120, height: 60 },
      { id: "c", side: "left", top: 130, height: 40 },
    ],
    344,
    480,
    OPTS,
  );
  const sorted = [...spots].sort((x, y) => x.top - y.top);
  const h = { a: 80, b: 60, c: 40 };
  for (let i = 1; i < sorted.length; i++) {
    const prev = sorted[i - 1];
    assert.ok(sorted[i].top >= prev.top + h[prev.id] + OPTS.gap, `${sorted[i].id} 在 ${prev.id} 下面`);
  }
  assert.equal(spots[0].width, cardWidth(344, OPTS));
  assert.equal(spots[0].width, 260);
});

test("排到頁尾放不下時整串往上推，不超出頁尾", () => {
  const spots = placeCards(
    [
      { id: "a", side: "left", top: 400, height: 50 },
      { id: "b", side: "left", top: 420, height: 50 },
    ],
    344,
    480,
    OPTS,
  );
  const b = spots.find((s) => s.id === "b");
  const a = spots.find((s) => s.id === "a");
  assert.ok(a && b);
  assert.equal(b.top + 50, 480 - OPTS.inset);
  assert.ok(a.top + 50 + OPTS.gap <= b.top);
});

test("寬頁面（放大後）：左右兩側各自一欄，右側靠右", () => {
  const spots = placeCards(
    [
      { id: "a", side: "left", top: 100, height: 80 },
      { id: "b", side: "right", top: 100, height: 80 },
    ],
    900,
    1200,
    OPTS,
  );
  const a = spots.find((s) => s.id === "a");
  const b = spots.find((s) => s.id === "b");
  assert.ok(a && b);
  assert.equal(a.top, 100);
  assert.equal(b.top, 100, "兩欄不互相讓");
  assert.equal(a.left, 6);
  assert.equal(b.left, 900 - 6 - 260);
});

test("放大讀一欄：卡片靠齊標記的外緣，不靠在畫面外的頁緣", () => {
  const [r] = placeCards([{ id: "r", side: "right", top: 100, height: 80, edge: 600 }], 1000, 1300, OPTS);
  assert.equal(r.left, 600 - 260);
  const [l] = placeCards([{ id: "l", side: "left", top: 100, height: 80, edge: 70 }], 1000, 1300, OPTS);
  assert.equal(l.left, 70);
});

test("頁面很窄時卡片縮到頁寬扣掉邊距", () => {
  assert.equal(cardWidth(200, OPTS), 188);
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
