// 「內文」一節的查詢與再問、每一行的內容（`poll.ts`、`view.ts`）。
// 執行：node --test src/ext/fulltext/poll-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { BUILDING_AGAIN_MS, BodySearch, PENDING_AGAIN_MS, againAfter, longEnough } from "./poll.ts";
import { hitLabel, hitTarget, pieces } from "./view.ts";

/** 假的計時器：記下要在多久之後跑什麼，由測試決定什麼時候跑。 */
function fakeTimers() {
  const q = [];
  return {
    q,
    set: (fn, ms) => {
      const t = { fn, ms, live: true };
      q.push(t);
      return t;
    },
    clear: (t) => {
      t.live = false;
    },
    fire() {
      const t = q.shift();
      if (t?.live) t.fn();
      return t;
    },
  };
}

const result = (over = {}) => ({ building: false, pending: false, indexed: 1, more: false, papers: [], ...over });
const paper = (id) => ({ paper: id, total: 1, hits: [] });
const tick = () => new Promise((r) => setImmediate(r));

test("至少 2 個字（Unicode 字元，不是 UTF-16 單位）", () => {
  assert.equal(longEnough("a"), false);
  assert.equal(longEnough(" a "), false);
  assert.equal(longEnough("ab"), true);
  assert.equal(longEnough("圖"), false);
  assert.equal(longEnough("\u{1D465}"), false, "一個基本多文種平面以外的字是一個字");
  assert.equal(longEnough("\u{1D465}y"), true);
});

test("building 3 秒後再問、pending 1 秒後再問，兩者都是假就停", () => {
  assert.equal(againAfter({ building: true, pending: true }), BUILDING_AGAIN_MS);
  assert.equal(againAfter({ building: false, pending: true }), PENDING_AGAIN_MS);
  assert.equal(againAfter({ building: false, pending: false }), null);
});

test("帶 pending 時用同一個查詢再問，結果更新；都是假就不再問", async () => {
  const timers = fakeTimers();
  const calls = [];
  const answers = [result({ pending: true }), result({ papers: [paper("A")] })];
  const b = new BodySearch(
    (q, scope, offset) => {
      calls.push([q, scope, offset]);
      return Promise.resolve(answers.shift());
    },
    () => {},
    timers,
  );
  b.start("detector", ["A", "B"]);
  await tick();
  assert.equal(b.state.pending, true);
  assert.equal(timers.q.length, 1);
  assert.equal(timers.q[0].ms, PENDING_AGAIN_MS);
  timers.fire();
  await tick();
  assert.deepEqual(calls, [
    ["detector", ["A", "B"], 0],
    ["detector", ["A", "B"], 0],
  ]);
  assert.deepEqual(b.state.papers.map((p) => p.paper), ["A"]);
  assert.equal(timers.q.length, 0, "都是假就停");
});

test("查詢換了就不再問舊的；舊的晚回來也不理", async () => {
  const timers = fakeTimers();
  let release;
  const calls = [];
  const b = new BodySearch(
    (q) => {
      calls.push(q);
      if (q === "old") return new Promise((r) => (release = r));
      return Promise.resolve(result({ building: true, papers: [paper("NEW")] }));
    },
    () => {},
    timers,
  );
  b.start("old", null);
  b.start("new", null);
  await tick();
  assert.equal(timers.q.at(-1).ms, BUILDING_AGAIN_MS);
  // 舊的回來了：不理
  release(result({ papers: [paper("OLD")] }));
  await tick();
  assert.deepEqual(b.state.papers.map((p) => p.paper), ["NEW"]);
  assert.equal(b.state.query, "new");
  // 再換：先前排的再問作廢
  b.start("third", null);
  await tick();
  const stale = timers.q.find((t) => !t.live);
  assert.ok(stale, "換查詢時清掉排著的再問");
  b.stop();
  assert.deepEqual(calls, ["old", "new", "third"]);
});

test("一次 10 篇；捲到最後再要下一批，接在後面", async () => {
  const calls = [];
  const b = new BodySearch(
    (q, scope, offset) => {
      calls.push(offset);
      const ids = Array.from({ length: 10 }, (_, i) => paper(`P${offset + i}`));
      return Promise.resolve(result({ papers: ids, more: offset === 0 }));
    },
    () => {},
    fakeTimers(),
  );
  b.start("q", null);
  await tick();
  assert.equal(b.state.more, true);
  b.next();
  b.next(); // 第二批還沒回來：不重複要
  await tick();
  assert.deepEqual(calls, [0, 10]);
  assert.equal(b.state.papers.length, 20);
  assert.equal(b.state.more, false);
  b.next();
  assert.deepEqual(calls, [0, 10]);
});

test("前後文切成標與不標的幾段；截掉的一頭加「…」；換行變空白", () => {
  const p = pieces({ context: "ab\ncd ef", marks: [[3, 5], [0, 1]], cut_start: true, cut_end: false });
  assert.deepEqual(p, [
    { text: "…", mark: false },
    { text: "a", mark: true },
    { text: "b ", mark: false },
    { text: "cd", mark: true },
    { text: " ef", mark: false },
  ]);
  // 重疊與超出範圍的標記
  assert.deepEqual(
    pieces({ context: "abcdef", marks: [[1, 4], [2, 3], [5, 99]], cut_start: false, cut_end: true }).map((x) => [x.text, x.mark]),
    [
      ["a", false],
      ["bcd", true],
      ["e", false],
      ["f", true],
      ["…", false],
    ],
  );
});

const say = (key, params) => `${key}${params ? JSON.stringify(params) : ""}`;
const hit = (over) => ({
  kind: "source",
  material: "M",
  page: 4,
  target: null,
  box: null,
  context: "",
  cut_start: false,
  cut_end: false,
  marks: [],
  range: [3, 9],
  quote: "detect",
  ...over,
});

test("每一處的說明：種類與頁（頁從 1 起）；自由書寫沒有頁", () => {
  assert.equal(hitLabel(hit({}), say), 'kind.source page{"n":5}');
  assert.equal(hitLabel(hit({ kind: "note", page: null }), say), "kind.note");
});

test("點了打開哪裡", () => {
  assert.deepEqual(hitTarget("P", hit({})), { paperId: "P", material: "M", page: 4, range: [3, 9], quote: "detect" });
  assert.deepEqual(hitTarget("P", hit({ kind: "translation", material: "OV", target: "g1", box: [1, 2, 3, 4], range: null, quote: null })), {
    paperId: "P",
    material: "OV",
    page: 4,
    view: "overlay",
    box: [1, 2, 3, 4],
  });
  assert.deepEqual(hitTarget("P", hit({ kind: "digest", material: "DG", target: "d1", range: null, quote: null })), {
    paperId: "P",
    material: "DG",
    view: "digest",
    point: "d1",
  });
  assert.deepEqual(hitTarget("P", hit({ kind: "note", material: "M", page: 2, target: "h1", range: null, quote: null })), {
    paperId: "P",
    material: "M",
    page: 2,
    highlight: "h1",
  });
  assert.deepEqual(hitTarget("P", hit({ kind: "note", material: null, page: null, target: "t0", range: null, quote: null })), {
    paperId: "P",
    view: "notes",
  });
});
