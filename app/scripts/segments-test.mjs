// 譯文檢視與 PDF 上點一段共用的段落計算（`src/screens/reader/segments.ts`）的測試。
// 執行：node --test scripts/segments-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  editMark,
  joinedText,
  nearestSegment,
  newestFirst,
  pdfBehind,
  preferredTranslation,
  readingUnits,
  continuationsFor,
  segmentAt,
  shownText,
  tableOfContents,
} from "../src/screens/reader/segments.ts";

const seg = (id, page, bbox, extra = {}) => ({
  id,
  page,
  bbox,
  kind: "body",
  source: `source ${id}`,
  target: `譯文 ${id}`,
  status: "done",
  ...extra,
});

// 一頁雙欄（612×792）：左欄兩段、右欄一段、跨欄的標題
const page0 = [
  seg("h1", 0, [72, 700, 540, 720], { kind: "heading", target: "1  緒論" }),
  seg("a", 0, [72, 500, 300, 690]),
  seg("b", 0, [72, 300, 300, 490]),
  seg("c", 0, [312, 300, 540, 690]),
];
const page2 = [seg("d", 2, [72, 600, 540, 700]), seg("e", 2, [72, 400, 540, 590])];
const all = [...page0, ...page2];

test("顯示：翻好的用譯文，沒翻與翻失敗的用原文並標沒翻", () => {
  assert.deepEqual(shownText(seg("x", 0, [0, 0, 1, 1])), { text: "譯文 x", untranslated: false });
  assert.deepEqual(shownText(seg("x", 0, [0, 0, 1, 1], { status: "pending", target: null })), {
    text: "source x",
    untranslated: true,
  });
  assert.deepEqual(shownText(seg("x", 0, [0, 0, 1, 1], { status: "failed", target: null })).untranslated, true);
  // 已翻但譯文是空白：當成沒翻，不留空白
  assert.equal(shownText(seg("x", 0, [0, 0, 1, 1], { target: "  " })).untranslated, true);
});

test("改過的標記：就地改（沒有服務）是已改，並排比較挑的（有服務）是你挑的", () => {
  assert.equal(editMark(seg("x", 0, [0, 0, 1, 1])), null);
  assert.equal(editMark(seg("x", 0, [0, 0, 1, 1], { by_user: true })), "edited");
  assert.equal(editMark(seg("x", 0, [0, 0, 1, 1], { by_user: true, service: { id: "s" } })), "picked");
});

test("目錄：只取標題段落、依閱讀順序，標題用譯文", () => {
  const toc = tableOfContents([...all, seg("h2", 2, [72, 710, 540, 730], { kind: "heading", status: "pending", target: null })]);
  assert.deepEqual(
    toc.map((e) => [e.id, e.text, e.page]),
    [
      ["h1", "1 緒論", 0],
      ["h2", "source h2", 2],
    ],
  );
});

test("點到哪一段：方框包住的那段；不在任何一段上是 null；別頁的不算", () => {
  assert.equal(segmentAt(all, 0, 100, 600)?.id, "a");
  assert.equal(segmentAt(all, 0, 400, 400)?.id, "c");
  assert.equal(segmentAt(all, 0, 306, 400), null); // 中縫
  assert.equal(segmentAt(all, 1, 100, 600), null);
  // 容差：方框外一點點也算
  assert.equal(segmentAt(all, 0, 71, 600)?.id, "a");
});

test("點到哪一段：兩段重疊時取小的那段", () => {
  const nested = [seg("big", 0, [0, 0, 600, 800]), seg("small", 0, [100, 100, 200, 200])];
  assert.equal(segmentAt(nested, 0, 150, 150)?.id, "small");
  assert.equal(segmentAt(nested, 0, 400, 400)?.id, "big");
});

test("讀到哪一段：同一頁最近的，雙欄時取同一欄", () => {
  assert.equal(nearestSegment(all, 0, 150, 495)?.id, "a"); // 兩段之間，離 a 近
  assert.equal(nearestSegment(all, 0, 150, 480)?.id, "b");
  assert.equal(nearestSegment(all, 0, 500, 200)?.id, "c"); // 右欄下方空白
  assert.equal(nearestSegment(all, 0, 300, 750)?.id, "h1");
});

test("讀到哪一段：這一頁沒有段落時，取前面最近一頁的最後一段；最前面就取後面第一段", () => {
  assert.equal(nearestSegment(all, 1, 300, 400)?.id, "c");
  assert.equal(nearestSegment(page2, 0, 300, 400)?.id, "d");
  assert.equal(nearestSegment([], 0, 0, 0), null);
});

test("譯本 PDF 落後：使用者改過的段落在產生之後才改", () => {
  const pdf = "2026-11-17T08:00:00.000Z";
  const edited = seg("x", 0, [0, 0, 1, 1], { by_user: true, modified: "2026-11-17T09:00:00.000Z" });
  const older = seg("y", 0, [0, 0, 1, 1], { by_user: true, modified: "2026-11-17T07:00:00.000Z" });
  const job = seg("z", 0, [0, 0, 1, 1], { modified: "2026-11-17T09:00:00.000Z" });
  assert.equal(pdfBehind([edited], pdf), true);
  assert.equal(pdfBehind([older], pdf), false);
  // 接著翻寫進來的不算「改過」（產生畫面另有「先產生目前的譯本」）
  assert.equal(pdfBehind([job], pdf), false);
  assert.equal(pdfBehind([edited], null), false);
});

test("工具箱「譯本」：上次那份還在就是它", () => {
  const have = { generated: ["g1", "g2"], added: ["a1"], overlay: true };
  assert.deepEqual(preferredTranslation(have, "a1"), { kind: "pdf", id: "a1" });
  assert.deepEqual(preferredTranslation(have, "g1"), { kind: "pdf", id: "g1" });
  assert.deepEqual(preferredTranslation(have, "overlay"), { kind: "overlay" });
});

test("工具箱「譯本」：第一次（或上次那份不在了）依序：產生的（最新的，排在前面）、加入的、疊譯文", () => {
  assert.deepEqual(preferredTranslation({ generated: ["g2", "g1"], added: ["a1"], overlay: true }, null), {
    kind: "pdf",
    id: "g2",
  });
  assert.deepEqual(preferredTranslation({ generated: [], added: ["a1"], overlay: true }, "gone"), { kind: "pdf", id: "a1" });
  assert.deepEqual(preferredTranslation({ generated: [], added: [], overlay: true }, null), { kind: "overlay" });
  assert.deepEqual(preferredTranslation({ generated: ["g1"], added: [], overlay: false }, "overlay"), {
    kind: "pdf",
    id: "g1",
  });
  assert.equal(preferredTranslation({ generated: [], added: [], overlay: false }, null), null);
});

test("材料依做出來的時間排：最新的在前，沒有時間的排最後", () => {
  const m = (id, time) => ({ id, provenance: time === undefined ? undefined : { time } });
  const list = [
    m("old", "2026-11-15T08:00:00.000Z"),
    m("none"),
    m("new", "2026-11-17T08:00:00.000Z"),
    m("mid", "2026-11-16T08:00:00.000Z"),
  ];
  assert.deepEqual(
    newestFirst(list).map((x) => x.id),
    ["new", "mid", "old", "none"],
  );
  assert.deepEqual(
    newestFirst([m("a"), m("b")]).map((x) => x.id),
    ["a", "b"],
  );
});

test("接續：前後兩段接成一段，夾在中間的圖說排到後面；三段接成一段；對不上的不理", () => {
  const segs = ["a", "b", "cap", "c", "d", "e"].map((id) => seg(id, 0, [0, 0, 1, 1], id === "cap" ? { kind: "caption" } : {}));
  const ids = (units) => units.map((u) => u.map((s) => s.id).join("+"));
  assert.deepEqual(ids(readingUnits(segs, [])), ["a", "b", "cap", "c", "d", "e"]);
  assert.deepEqual(ids(readingUnits(segs, [["b", "c"]])), ["a", "b+c", "cap", "d", "e"]);
  assert.deepEqual(ids(readingUnits(segs, [["b", "c"], ["c", "d"]])), ["a", "b+c+d", "cap", "e"]);
  // 不認得的代號、往回接的都不理
  assert.deepEqual(ids(readingUnits(segs, [["x", "a"], ["d", "a"]])), ["a", "b", "cap", "c", "d", "e"]);
});

test("接續的譯文：中文直接接上，英文（沒翻的）中間空一格", () => {
  const a = seg("a", 0, [0, 0, 1, 1], { target: "我們的直覺是，至少在局部鄰域內，機器人" });
  const b = seg("b", 1, [0, 0, 1, 1], { target: "應以一致的方式行動。" });
  assert.deepEqual(joinedText([a, b]), { text: "我們的直覺是，至少在局部鄰域內，機器人應以一致的方式行動。", untranslated: false });
  const c = seg("c", 1, [0, 0, 1, 1], { status: "pending", target: null, source: "should act consistently." });
  const d = seg("d", 0, [0, 0, 1, 1], { status: "pending", target: null, source: "the robot" });
  assert.deepEqual(joinedText([d, c]), { text: "the robot should act consistently.", untranslated: true });
  assert.equal(joinedText([a, c]).text, "我們的直覺是，至少在局部鄰域內，機器人should act consistently.");
  assert.equal(joinedText([a, c]).untranslated, true);
});

test("接續清單：換了譯本先清掉；讀回來的是別份的不用；讀不到就是空的", () => {
  let st = continuationsFor(null, { type: "open", overlayId: "a" });
  st = continuationsFor(st, { type: "loaded", overlayId: "a", pairs: [["g0001", "g0002"]] });
  assert.deepEqual(st.pairs, [["g0001", "g0002"]]);
  // 換到另一份：還沒讀回來之前沒有接續
  st = continuationsFor(st, { type: "open", overlayId: "b" });
  assert.deepEqual(st.pairs, []);
  // 上一份晚到的結果不用
  st = continuationsFor(st, { type: "loaded", overlayId: "a", pairs: [["g0001", "g0002"]] });
  assert.deepEqual(st.pairs, []);
  st = continuationsFor(st, { type: "loaded", overlayId: "b", pairs: [["g0003", "g0004"]] });
  assert.deepEqual(st.pairs, [["g0003", "g0004"]]);
  st = continuationsFor(st, { type: "failed", overlayId: "b" });
  assert.deepEqual(st.pairs, []);
});
