// 首頁清單整理（`src/catalog.ts`）的測試。用 Node 內建的測試器與型別剝除，不需要額外的套件。
// 執行：node --test scripts/catalog-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  applyCatalog,
  ALL_VIEW,
  firstAuthor,
  hasContent,
  inView,
  isNarrowed,
  matchesQuery,
  recentlyAdded,
  sameView,
  searchScope,
  sortPapers,
  UNGROUPED_VIEW,
  viewKey,
} from "../src/catalog.ts";

let n = 0;
function paper(over = {}) {
  n++;
  const id = `01HZ${String(n).padStart(22, "0")}`;
  return {
    id,
    folder: over.folder ?? `folder-${n}`,
    bib: { authors: [], ...over.bib },
    materials: [],
    needs_newer_app: false,
    added: over.added ?? null,
    groups: over.groups ?? [],
    has_notes: over.hasNotes ?? false,
    progress: {
      status: over.status ?? "Unread",
      read_at: null,
      position: null,
      device_positions: [],
      furthest_pages: {},
      last_read_at: over.lastRead ?? null,
    },
  };
}

const zhao = paper({ bib: { title: "DETRs Beat YOLOs", authors: [{ family: "Zhao", given: "Yian" }], year: 2024 }, status: "Reading", lastRead: "2026-11-10T08:00:00.000Z" });
const lema = paper({ bib: { title: "PCB defect survey", authors: [{ family: "Lema" }], year: 2025 }, lastRead: "2026-11-12T08:00:00.000Z" });
const bare = paper({ folder: "Alpha-notes", status: "Read" });
const old = paper({ bib: { title: "Attention", authors: [{ literal: "Vaswani et al." }], year: 2017 }, status: "Read", lastRead: "2026-11-01T08:00:00.000Z" });
const all = [zhao, lema, bare, old];

test("第一作者排序用的姓：分開寫的用姓；一整串的取逗號前或最後一個詞", () => {
  const name = (authors) => firstAuthor({ authors });
  assert.equal(name([{ family: "Zhao", given: "Yian" }]), "zhao");
  assert.equal(name([{ literal: "Zhao, Yian" }]), "zhao");
  assert.equal(name([{ literal: "Ashish Vaswani" }]), "vaswani");
  assert.equal(name([{ literal: "Vaswani et al." }]), "vaswani");
  assert.equal(name([{ literal: "王小明" }]), "王小明");
  assert.equal(name([]), null);
  assert.equal(name([{ literal: "  " }]), null);
});
const ids = (l) => l.map((p) => p.id);

test("搜尋：篇名、作者、年份，不分大小寫", () => {
  assert.equal(matchesQuery(zhao, "detrs"), true);
  assert.equal(matchesQuery(zhao, "ZHAO"), true);
  assert.equal(matchesQuery(zhao, "yian"), true);
  assert.equal(matchesQuery(zhao, "2024"), true);
  assert.equal(matchesQuery(zhao, "2025"), false);
  assert.equal(matchesQuery(old, "vaswani"), true, "作者只有 literal 也找得到");
  assert.equal(matchesQuery(bare, "alpha"), true, "沒有篇名時用資料夾名");
});

test("搜尋：多個詞要全部符合；空白不篩", () => {
  assert.equal(matchesQuery(zhao, "detrs 2024"), true);
  assert.equal(matchesQuery(zhao, "detrs 2025"), false);
  assert.equal(matchesQuery(zhao, "   "), true);
  assert.equal(matchesQuery(zhao, ""), true);
});

test("排序：最近閱讀，沒讀過的排最後（正反都是）", () => {
  assert.deepEqual(ids(sortPapers(all, "recent", false)), ids([lema, zhao, old, bare]));
  assert.deepEqual(ids(sortPapers(all, "recent", true)), ids([old, zhao, lema, bare]));
});

test("排序：都沒有加入時間時，退回用代號（新的在前），可反向", () => {
  assert.deepEqual(ids(sortPapers(all, "added", false)), ids([old, bare, lema, zhao]));
  // 沒有加入時間的正反都排最後，所以反向時它們之間仍是新的在前
  assert.deepEqual(ids(sortPapers(all, "added", true)), ids([old, bare, lema, zhao]));
});

// 加入時間：故意讓時間順序和代號順序不同
const a1 = paper({ bib: { title: "A1" }, added: "2026-03-01T00:00:00.000Z" });
const a2 = paper({ bib: { title: "A2" }, added: "2026-11-01T00:00:00.000Z" });
const a3 = paper({ bib: { title: "A3" }, added: "2026-06-01T00:00:00.000Z" });
const legacy = paper({ bib: { title: "Legacy" } }); // 舊的文獻：沒有加入時間
const withAdded = [a1, a2, a3, legacy];

test("排序：加入時間用 added，新的在前；沒有 added 的排最後（正反都是）", () => {
  assert.deepEqual(ids(sortPapers(withAdded, "added", false)), ids([a2, a3, a1, legacy]));
  assert.deepEqual(ids(sortPapers(withAdded, "added", true)), ids([a1, a3, a2, legacy]));
});

test("最近加入：由新到舊取前幾篇，沒有加入時間的排最後", () => {
  assert.deepEqual(ids(recentlyAdded(withAdded, 2)), ids([a2, a3]));
  assert.deepEqual(ids(recentlyAdded(withAdded, 10)), ids([a2, a3, a1, legacy]));
});

test("排序：同一種排序的次要鍵是加入時間（沒有就用代號），新的在前", () => {
  const x = paper({ bib: { title: "Same", year: 2020 }, added: "2026-01-01T00:00:00.000Z" });
  const y = paper({ bib: { title: "Same", year: 2020 }, added: "2026-09-01T00:00:00.000Z" });
  assert.deepEqual(ids(sortPapers([x, y], "title", false)), ids([y, x]));
  assert.deepEqual(ids(sortPapers([x, y], "year", false)), ids([y, x]));
});

// ---------- 群組檢視 ----------

const g1 = "01G1";
const g2 = "01G2";
const gone = "01GONE"; // 文獻上還記著，但群組已經不存在
const inG1 = paper({ bib: { title: "In G1" }, groups: [g1] });
const inBoth = paper({ bib: { title: "In both" }, groups: [g1, g2] });
const onlyGone = paper({ bib: { title: "Only gone" }, groups: [gone] });
const loose = paper({ bib: { title: "Loose" } });
const grouped = [inG1, inBoth, onlyGone, loose];
const live = new Set([g1, g2]);

test("群組檢視：只列屬於那個群組的；一篇可以在好幾個群組", () => {
  const view1 = { kind: "group", id: g1 };
  const view2 = { kind: "group", id: g2 };
  assert.deepEqual(ids(grouped.filter((p) => inView(p, view1, live))), ids([inG1, inBoth]));
  assert.deepEqual(ids(grouped.filter((p) => inView(p, view2, live))), ids([inBoth]));
});

test("未分組：存在的群組一個都沒有（只記著已不存在的群組也算）", () => {
  assert.deepEqual(ids(grouped.filter((p) => inView(p, UNGROUPED_VIEW, live))), ids([onlyGone, loose]));
  // 沒給存在的群組清單時，文獻上記的全部照算
  assert.deepEqual(ids(grouped.filter((p) => inView(p, UNGROUPED_VIEW))), ids([loose]));
});

test("群組檢視：已不存在的群組不列任何文獻", () => {
  assert.deepEqual(ids(grouped.filter((p) => inView(p, { kind: "group", id: gone }, live))), []);
});

test("群組檢視配合搜尋、篩選、排序", () => {
  const q = { view: { kind: "group", id: g1 }, query: "both", status: null, sortKey: "title", sortReverse: false, groupIds: live };
  assert.deepEqual(ids(applyCatalog(grouped, q)), ids([inBoth]));
  assert.deepEqual(ids(applyCatalog(grouped, { ...q, query: "" })), ids([inBoth, inG1]));
  assert.deepEqual(ids(applyCatalog(grouped, { ...q, view: UNGROUPED_VIEW, query: "" })), ids([loose, onlyGone]));
});

test("首頁搜尋的一節的範圍：只套群組與篩選，不套搜尋字串；全部文獻又沒篩選是 null", () => {
  assert.equal(searchScope(grouped, { view: ALL_VIEW, status: null, groupIds: live }), null);
  assert.equal(searchScope(grouped, { view: ALL_VIEW, status: null, has: [], groupIds: live }), null);
  // 群組：那個群組的全部，不看篇名有沒有搜尋字串（範圍的參數裡根本沒有 query）
  const inGroup = searchScope(grouped, { view: { kind: "group", id: g1 }, status: null, groupIds: live });
  assert.deepEqual([...inGroup].sort(), ids([inBoth, inG1]).sort());
  // 篩選
  const reading = paper({ status: "Reading" });
  const withReading = [...grouped, reading];
  assert.deepEqual(searchScope(withReading, { view: ALL_VIEW, status: "Reading", groupIds: live }), [reading.id]);
  // 書目的清單套了搜尋字串、範圍沒有：同一組條件下範圍比清單多
  const q = { view: { kind: "group", id: g1 }, query: "both", status: null, sortKey: "title", sortReverse: false, groupIds: live };
  assert.ok(applyCatalog(grouped, q).length < searchScope(grouped, q).length);
});

test("檢視的比較與 key：群組要比代號", () => {
  assert.equal(sameView(ALL_VIEW, ALL_VIEW), true);
  assert.equal(sameView(ALL_VIEW, UNGROUPED_VIEW), false);
  assert.equal(sameView({ kind: "group", id: g1 }, { kind: "group", id: g1 }), true);
  assert.equal(sameView({ kind: "group", id: g1 }, { kind: "group", id: g2 }), false);
  assert.equal(sameView({ kind: "group", id: g1 }, ALL_VIEW), false);
  assert.notEqual(viewKey({ kind: "group", id: g1 }), viewKey({ kind: "group", id: g2 }));
  assert.notEqual(viewKey(ALL_VIEW), viewKey(UNGROUPED_VIEW));
});

test("排序：篇名 A 到 Z，可反向", () => {
  // 篇名：Alpha-notes（資料夾名）、Attention、DETRs…、PCB…
  assert.deepEqual(ids(sortPapers(all, "title", false)), ids([bare, old, zhao, lema]));
  assert.deepEqual(ids(sortPapers(all, "title", true)), ids([lema, zhao, old, bare]));
});

test("排序：第一作者，沒有作者的正反都排最後", () => {
  assert.deepEqual(ids(sortPapers(all, "author", false)), ids([lema, old, zhao, bare]));
  assert.deepEqual(ids(sortPapers(all, "author", true)), ids([zhao, old, lema, bare]));
});

test("排序：年份新的在前，沒有年份的正反都排最後", () => {
  assert.deepEqual(ids(sortPapers(all, "year", false)), ids([lema, zhao, old, bare]));
  assert.deepEqual(ids(sortPapers(all, "year", true)), ids([old, zhao, lema, bare]));
});

test("合在一起：搜尋、篩選、排序", () => {
  const q = { view: ALL_VIEW, query: "", status: "Read", sortKey: "title", sortReverse: false };
  assert.deepEqual(ids(applyCatalog(all, q)), ids([bare, old]));
  assert.deepEqual(ids(applyCatalog(all, { ...q, status: null, query: "survey" })), ids([lema]));
  assert.deepEqual(ids(applyCatalog(all, { ...q, status: "Unread", query: "survey" })), ids([lema]));
  assert.deepEqual(ids(applyCatalog(all, { ...q, status: "Reading", query: "survey" })), []);
});

test("有搜尋或篩選時才算「縮小了範圍」", () => {
  assert.equal(isNarrowed({ query: "", status: null }), false);
  assert.equal(isNarrowed({ query: "  ", status: null }), false);
  assert.equal(isNarrowed({ query: "a", status: null }), true);
  assert.equal(isNarrowed({ query: "", status: "Read" }), true);
  assert.equal(isNarrowed({ query: "", status: null, has: [] }), false);
  assert.equal(isNarrowed({ query: "", status: null, has: ["notes"] }), true);
});

test("依內容篩選：有譯本、有重點筆記、有筆記；選幾項就要每一項都有，和閱讀狀態一起篩", () => {
  const tr = { ...paper({ bib: { title: "with translation" } }), materials: [{ id: "m1", role: "translation", form: "pdf", pages: 3, status: "Ok" }] };
  const dg = {
    ...paper({ bib: { title: "with digest" }, status: "Read" }),
    materials: [{ id: "m2", role: "digest", form: "markdown", pages: null, status: "Ok" }],
  };
  const both = {
    ...paper({ bib: { title: "with both" } }),
    materials: [
      { id: "m3", role: "translation", form: "overlay", pages: null, status: "Ok" },
      { id: "m4", role: "digest", form: "markdown", pages: null, status: "Ok" },
    ],
  };
  const plain = paper({ bib: { title: "plain" } });
  const list = [tr, dg, both, plain];
  const q = { view: ALL_VIEW, query: "", status: null, sortKey: "title", sortReverse: false };
  assert.deepEqual(ids(applyCatalog(list, { ...q, has: ["translation"] })), ids([both, tr]));
  assert.deepEqual(ids(applyCatalog(list, { ...q, has: ["digest"] })), ids([both, dg]));
  assert.deepEqual(ids(applyCatalog(list, { ...q, has: ["translation", "digest"] })), ids([both]));
  assert.deepEqual(ids(applyCatalog(list, { ...q, has: ["digest"], status: "Read" })), ids([dg]));
  assert.deepEqual(ids(applyCatalog(list, { ...q, has: [] })), ids([plain, both, dg, tr]));
  // 有筆記：看目錄摘要裡核心算好的 `has_notes`
  const withNotes = list.map((p) => (p === plain || p === dg ? { ...p, has_notes: true } : p));
  assert.deepEqual(ids(applyCatalog(withNotes, { ...q, has: ["notes"] })), ids([plain, dg]));
  assert.deepEqual(ids(applyCatalog(list, { ...q, has: ["notes"] })), []);
  assert.equal(hasContent(plain, "translation"), false);
  assert.equal(hasContent({ ...plain, materials: [{ id: "x", role: "digest", form: "pdf", pages: 1, status: "Ok" }] }, "digest"), false);
  // 匯入的 Markdown 譯本打不開，不算「有譯本」
  const md = { id: "y", role: "translation", form: "markdown", pages: null, status: "Ok" };
  assert.equal(hasContent({ ...plain, materials: [md] }, "translation"), false);
});

// ---------- 檢查問題的說法 ----------

// 文字表（`zh-TW.ts`）把各區的文字拆成幾個檔案，彼此用不帶副檔名的方式引入（打包工具看得懂，Node 看不懂）。
// 這裡補一個解析規則：找不到時再試試加上 `.ts`；之後才用動態引入載入文字表。
import { registerHooks } from "node:module";

registerHooks({
  resolve(specifier, context, nextResolve) {
    try {
      return nextResolve(specifier, context);
    } catch (e) {
      if (e?.code === "ERR_MODULE_NOT_FOUND" && /^\.\.?\//.test(specifier) && !/\.[a-z]+$/i.test(specifier)) {
        return nextResolve(`${specifier}.ts`, context);
      }
      throw e;
    }
  },
});

const { problemText } = await import("../src/strings/problem.ts");
const { zhTW } = await import("../src/strings/zh-TW.ts");

// 這個版本做的時候核心有的十種問題：每一種都要有白話說法。
const KNOWN_KINDS = [
  "UnreadablePaperFile",
  "DuplicateId",
  "MaterialMissing",
  "MaterialSizeMismatch",
  "MaterialInvalidPath",
  "MaterialHashMismatch",
  "UnknownEntry",
  "UnlistedFile",
  "LeftoverTemp",
  "UnreadableLogFile",
];

test("檢查問題：已知的每一種都有白話說法；不認得的種類用通用的一句，不是空白也不是代號", () => {
  for (const k of KNOWN_KINDS) {
    assert.ok(Object.hasOwn(zhTW, `problem.${k}`), `缺 ${k} 的說法`);
    assert.notEqual(problemText(zhTW, k), zhTW["problem.unknown"]);
  }
  const unknown = problemText(zhTW, "SomethingNewFromTheCore");
  assert.equal(unknown, zhTW["problem.unknown"]);
  assert.ok(unknown.length > 20 && !/SomethingNew|problem\./.test(unknown));
  // 不是屬性名稱的怪字串也一樣
  assert.equal(problemText(zhTW, "constructor"), zhTW["problem.unknown"]);
});

test("核心新增的問題種類：提醒補白話說法（沒補也不會壞，會用通用的一句）", () => {
  const rs = readFileSync(new URL("../core/src/store.rs", import.meta.url), "utf8");
  const body = /pub enum ProblemKind \{([\s\S]*?)\n\}/.exec(rs)?.[1] ?? "";
  const kinds = [...body.matchAll(/^\s*([A-Z][A-Za-z]+)/gm)].map((m) => m[1]);
  assert.ok(kinds.length >= KNOWN_KINDS.length, "要讀得到核心的問題種類");
  const missing = kinds.filter((k) => !Object.hasOwn(zhTW, `problem.${k}`));
  if (missing.length > 0) console.warn(`提醒：核心新增的檢查問題還沒有白話說法：${missing.join("、")}`);
});
