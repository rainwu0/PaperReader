// 逐篇處理（`src/perPaper.ts`）與共用的 PDF 佇列（`src/pdfQueue.ts`）的測試：用假的目錄、紀錄與原文驗排程的規則。真的開檔與指令在模擬器上驗。
// 執行：node --test scripts/perpaper-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { enqueuePdf, urgentWaiting } from "../src/pdfQueue.ts";
import {
  MAX_PAGE_CHARS,
  MAX_TRIES,
  PerPaperScheduler,
  SourceUnusable,
  bibFromInput,
  bibIdsTask,
  classifyError,
  limitPages,
  parseRecord,
  textPagesTask,
  wellFormed,
} from "../src/perPaper.ts";
import { pageText } from "../src/translate/layout.ts";

const SHA = "a".repeat(64);
const STAMP = "arXiv:2304.08069v3  [cs.CV]  3 Apr 2024\nDETRs Beat YOLOs on Real-time Object Detection";

function paper(id, over = {}) {
  return {
    id,
    added: over.added ?? "2026-11-17T08:00:00.000Z",
    source: { materialId: `M-${id}`, sha256: over.sha ?? SHA, pages: over.pages === undefined ? 3 : over.pages },
    bib: over.bib ?? {},
    // 測試用：這篇原文的第一頁文字、自帶資訊
    text: over.text ?? STAMP,
    info: over.info ?? {},
    pageTexts: over.pageTexts,
  };
}

function page(text) {
  return {
    view: [0, 0, 612, 792],
    // 沒有字的頁（掃描版）沒有任何片段
    items: (text === "" ? [] : text.split("\n")).map((line, i) => ({
      str: line,
      transform: [10, 0, 0, 10, 72, 700 - 14 * i],
      width: line.length * 5,
      height: 10,
      hasEOL: true,
    })),
  };
}

/** 假的外殼與原文。`opts.onPage(n)` 每讀一頁呼叫（模擬讀到一半發生的事）。 */
function setup(papers, opts = {}) {
  let record = opts.record ?? null;
  const calls = { opened: [], filled: [], pages: [], saved: 0, read: [] };
  const fill = async (paperId, f) => {
    calls.filled.push([paperId, f]);
    if (opts.fillError) throw opts.fillError;
    return true;
  };
  const deps = {
    listPapers: async () => papers,
    loadRecord: async () => record,
    saveRecord: async (r) => {
      record = JSON.parse(JSON.stringify(r));
      calls.saved++;
    },
    withSource: async (p, work) => {
      calls.opened.push(p.id);
      if (opts.openError) throw opts.openError;
      if (opts.hang) return new Promise(() => {});
      const texts = p.pageTexts ?? [p.text, "page two", "page three"];
      return work({
        numPages: texts.length,
        info: p.info,
        xmp: null,
        readPages: async (count, stop) => {
          calls.read.push(count);
          const out = [];
          for (let n = 0; n < count; n++) {
            out.push(page(texts[n]));
            opts.onPage?.(n);
            if (stop()) return null;
          }
          return out;
        },
      });
    },
    setPages: async (...a) => calls.pages.push(a),
    busyReason: async () => (opts.busy ? opts.busy() : null),
    urgentWaiting: () => (opts.urgent ? opts.urgent() : false),
    timeoutMs: opts.timeoutMs,
  };
  const extra = opts.tasks ?? [];
  const s = new PerPaperScheduler(deps, () => [bibIdsTask(fill), ...extra]);
  return { s, calls, record: () => record, deps };
}

test("補書目：arXiv 戳記寫成網址與年份；做過了就不再做", async () => {
  const { s, calls, record } = setup([paper("P1")]);
  assert.equal(await s.step(), "worked");
  assert.deepEqual(calls.filled, [["P1", { year: 2023, doi: null, url: "https://arxiv.org/abs/2304.08069" }]]);
  assert.equal(record().papers.P1["bib.ids"].result, "done");
  assert.equal(await s.step(), "nothing");
  assert.equal(calls.opened.length, 1);
  assert.deepEqual(s.getProgress(), { done: 1, total: 1, paused: false, pending: { "bib.ids": 0 } });
});

test("補書目：什麼都沒抽到也算做過，不寫", async () => {
  const { s, calls, record } = setup([paper("P1", { text: "Just a title\nno identifiers here" })]);
  assert.equal(await s.step(), "worked");
  assert.deepEqual(calls.filled, []);
  assert.equal(record().papers.P1["bib.ids"].result, "done");
});

test("補書目：三欄都已有值算做過，不開檔；紀錄不見時只對還有空欄的篇重做一次", async () => {
  const full = { year: 2020, doi: "10.1000/x", url: "https://x" };
  const { s, calls } = setup([paper("P1", { bib: full }), paper("P2", { bib: { year: 2020 } })]);
  assert.equal(await s.step(), "worked");
  assert.deepEqual(calls.opened, ["P2"]);
  assert.equal(await s.step(), "nothing");
  assert.deepEqual(calls.opened, ["P2"]);
  assert.equal(s.getProgress().done, 2);
});

test("剛加入的一篇等補篇名作者結束才排進來", async () => {
  const { s, calls } = setup([paper("P1")]);
  let finish;
  s.defer("P1", new Promise((r) => (finish = r)));
  assert.equal(await s.step(), "nothing");
  assert.deepEqual(calls.opened, []);
  finish(true);
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(await s.step(), "worked");
  assert.deepEqual(calls.opened, ["P1"]);
});

test("暫停與繼續；暫停時照樣算已處理幾篇", async () => {
  const { s, calls } = setup([paper("P1"), paper("P2", { bib: { year: 1, doi: "10.1000/a", url: "u" } })]);
  s.setPaused(true);
  assert.equal(await s.step(), "paused");
  assert.deepEqual(calls.opened, []);
  assert.deepEqual(s.getProgress(), { done: 1, total: 2, paused: true, pending: { "bib.ids": 1 } });
  s.setPaused(false);
  assert.equal(await s.step(), "worked");
  assert.deepEqual(s.getProgress(), { done: 2, total: 2, paused: false, pending: { "bib.ids": 0 } });
});

test("閱讀時讓路：持有暫停時不做；做到一半打開閱讀畫面就停下，不算失敗，下次從頭做", async () => {
  let release = null;
  const { s, calls, record } = setup([paper("P1")], {
    onPage: () => {
      if (!release) release = s.hold();
    },
  });
  assert.equal(await s.step(), "stopped");
  assert.equal(record()?.papers?.P1, undefined);
  assert.equal(await s.step(), "busy");
  release();
  assert.equal(await s.step(), "worked");
  assert.equal(calls.opened.length, 2);
  assert.equal(record().papers.P1["bib.ids"].result, "done");
});

test("縮圖插隊：讀到一半有優先的工作在等就停下這一篇", async () => {
  let urgent = false;
  const twoPages = { tasks: [layoutTask(() => {})], onPage: (n) => (urgent = n === 0) };
  const { s, record } = setup([paper("P1")], { ...twoPages, urgent: () => urgent });
  assert.equal(await s.step(), "stopped");
  assert.equal(record()?.papers?.P1?.["bib.ids"], undefined);
});

test("有產生的工作、在背景、剛要了縮圖：不開始", async () => {
  const { s, calls } = setup([paper("P1")], { busy: () => "generating" });
  assert.equal(await s.step(), "busy");
  assert.deepEqual(calls.opened, []);
});

test("暫時的失敗：這次不再試，下次開應用再試，最多 3 次", async () => {
  let rec = null;
  for (let launch = 1; launch <= MAX_TRIES + 1; launch++) {
    const { s, calls, record } = setup([paper("P1", { text: STAMP })], { record: rec, fillError: { kind: "Io" } });
    const r = await s.step();
    if (launch <= MAX_TRIES) {
      assert.equal(r, "worked", `launch ${launch}`);
      assert.equal(record().papers.P1["bib.ids"].tries, launch);
      assert.equal(record().papers.P1["bib.ids"].result, "failed");
      // 同一次開應用不再試
      assert.equal(await s.step(), "nothing");
      assert.equal(calls.opened.length, 1);
    } else {
      assert.equal(r, "nothing", "試滿之後當成永久略過");
      assert.deepEqual(calls.opened, []);
      assert.equal(s.getProgress().done, 1);
    }
    rec = record();
  }
});

test("永久略過：原文打不開、核心交回 InvalidInput；下次開應用也不再試", async () => {
  for (const opts of [{ openError: new SourceUnusable("Unreadable") }, { fillError: { kind: "InvalidInput" } }]) {
    const a = setup([paper("P1")], opts);
    assert.equal(await a.s.step(), "worked");
    assert.equal(a.record().papers.P1["bib.ids"].result, "skipped");
    const b = setup([paper("P1")], { record: a.record() });
    assert.equal(await b.s.step(), "nothing");
    assert.deepEqual(b.calls.opened, []);
  }
});

test("逾時算暫時失敗", async () => {
  const { s, record } = setup([paper("P1")], { hang: true, timeoutMs: 30 });
  assert.equal(await s.step(), "worked");
  assert.deepEqual(
    [record().papers.P1["bib.ids"].result, record().papers.P1["bib.ids"].reason],
    ["failed", "Timeout"],
  );
});

test("版本升了、原文換了就重做", async () => {
  const done = { format: 1, papers: { P1: { "bib.ids": { sha256: SHA, version: 1, result: "done", reason: null, tries: 1 } } } };
  const same = setup([paper("P1")], { record: done });
  assert.equal(await same.s.step(), "nothing");
  const newSource = setup([paper("P1", { sha: "b".repeat(64) })], { record: done });
  assert.equal(await newSource.s.step(), "worked");
  const t2 = { ...layoutTask(() => {}), id: "x.y", version: 2 };
  const old = { format: 1, papers: { P1: { ...done.papers.P1, "x.y": { sha256: SHA, version: 1, result: "done", reason: null, tries: 1 } } } };
  const bumped = setup([paper("P1")], { record: old, tasks: [t2] });
  assert.equal(await bumped.s.step(), "worked");
  assert.equal(bumped.record().papers.P1["x.y"].version, 2);
});

/** 一件要版面分析的外掛抽取（假的）。 */
function layoutTask(onRun, over = {}) {
  return { id: "links.references", version: 1, needsLayout: true, run: async (input) => onRun(input), ...over };
}

test("外掛的抽取：共用一次開檔與版面分析；頁數不知道時先記頁數", async () => {
  let got = null;
  const refsPage = "Body text of the paper continues here with enough words to be a paragraph\nReferences\n[1] A. Smith, B. Jones, Title of a paper. Journal, 2020.";
  const { s, calls, record } = setup([paper("P1", { pages: null, pageTexts: [STAMP, "page two", refsPage] })], {
    tasks: [layoutTask((input) => (got = input))],
  });
  assert.equal(await s.step(), "worked");
  assert.deepEqual(calls.opened, ["P1"]);
  assert.deepEqual(calls.pages, [["P1", "M-P1", 3]]);
  assert.equal(got.source.pages, 3);
  assert.ok(got.firstPageText.startsWith("arXiv:2304.08069v3"));
  assert.ok(Array.isArray(got.layout.segments) && Array.isArray(got.layout.references));
  assert.equal(record().papers.P1["links.references"].result, "done");
});

// 外掛說做過的記成 done，下一輪不再問（原本每一輪都再問）
test("外掛的抽取：紀錄不見時看外掛自己的判斷，不重抽、記下來；索引整理中先不判斷", async () => {
  let ran = 0;
  let asked = 0;
  let answer = new Set(["P1"]);
  const doneFor = async () => {
    asked++;
    return answer;
  };
  const full = { year: 1, doi: "10.1000/a", url: "u" };
  const a = setup([paper("P1", { bib: full })], { tasks: [layoutTask(() => ran++, { doneFor })] });
  assert.equal(await a.s.step(), "nothing");
  assert.equal(ran, 0);
  assert.equal(a.s.getProgress().done, 1);
  assert.equal(a.record().papers.P1["links.references"].result, "done", "外掛說做過的記下來");
  answer = null; // 整理中：記下來的不再問，照樣算做完
  assert.equal(await a.s.step(), "nothing");
  assert.equal(asked, 1);
  assert.equal(a.s.getProgress().done, 1);
  // 沒記下的一篇遇到整理中：這一輪不做、不算做完；整理完再做
  const b = setup([paper("P1", { bib: full })], { tasks: [layoutTask(() => ran++, { doneFor })] });
  assert.equal(await b.s.step(), "nothing");
  assert.equal(ran, 0);
  assert.equal(b.s.getProgress().done, 0, "判斷不了的不算做完");
  answer = new Set();
  assert.equal(await b.s.step(), "worked");
  assert.equal(ran, 1);
});

test("整份沒有文字（掃描版）：要版面分析的抽取永久略過；補書目照樣看自帶資訊", async () => {
  let ran = 0;
  const p = paper("P1", { pageTexts: ["", "", ""], info: { Custom: { doi: "10.1000/scan" } } });
  const { s, calls, record } = setup([p], { tasks: [layoutTask(() => ran++)] });
  assert.equal(await s.step(), "worked");
  assert.equal(ran, 0);
  assert.equal(record().papers.P1["links.references"].result, "skipped");
  assert.equal(record().papers.P1["links.references"].reason, "NoText");
  assert.deepEqual(calls.filled, [["P1", { year: null, doi: "10.1000/scan", url: null }]]);
});

test("新加入的先做", async () => {
  const { s, calls } = setup([
    paper("OLD", { added: "2026-01-01T00:00:00.000Z" }),
    paper("NEW", { added: "2026-11-17T00:00:00.000Z" }),
  ]);
  await s.step();
  assert.deepEqual(calls.opened, ["NEW"]);
});

test("成本和篇數成正比：一輪只讀一次目錄、問一次外掛做過沒有，紀錄分批寫", async () => {
  for (const n of [40, 160]) {
    const papers = Array.from({ length: n }, (_, i) => paper(`P${i}`, { added: `2026-11-17T08:${String(i % 60).padStart(2, "0")}:00.000Z` }));
    let asked = 0;
    let askedPapers = 0;
    const t = layoutTask(() => {}, {
      doneFor: async (batch) => {
        asked++;
        askedPapers += batch.length;
        return new Set();
      },
    });
    const { s, deps, calls, record } = setup(papers, { tasks: [t] });
    let lists = 0;
    const list = deps.listPapers;
    deps.listPapers = async () => {
      lists++;
      return list();
    };
    let steps = 0;
    while ((await s.step()) === "worked") steps++;
    assert.equal(steps, n);
    assert.equal(calls.opened.length, n);
    assert.equal(Object.keys(record().papers).length, n, "每一篇都記下了");
    // 一輪做完 n 篇，再一輪發現沒事：目錄讀兩次、外掛問兩次，不隨 n 增加
    assert.ok(lists <= 2, `目錄讀了 ${lists} 次（${n} 篇）`);
    assert.ok(asked <= 2, `外掛問了 ${asked} 次（${n} 篇）`);
    assert.ok(askedPapers <= 2 * n, `外掛被問了 ${askedPapers} 篇次（${n} 篇）`);
    // 紀錄每 20 篇（或 5 秒）寫一次，加上一輪做完時一次
    assert.ok(calls.saved <= Math.ceil(n / 20) + 2, `紀錄寫了 ${calls.saved} 次（${n} 篇）`);
  }
});

test("一輪之中加入的一篇：下一步就重讀目錄，先做它", async () => {
  const papers = [paper("OLD1", { added: "2026-01-01T00:00:00.000Z" }), paper("OLD2", { added: "2026-01-02T00:00:00.000Z" })];
  const { s, calls } = setup(papers);
  assert.equal(await s.step(), "worked");
  assert.deepEqual(calls.opened, ["OLD2"]);
  papers.push(paper("NEW", { added: "2026-11-17T00:00:00.000Z" }));
  let finish;
  s.defer("NEW", new Promise((r) => (finish = r)));
  finish(true);
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(await s.step(), "worked");
  assert.deepEqual(calls.opened, ["OLD2", "NEW"]);
});

// ---------- 抽文字 ----------

const FULL_BIB = { year: 2020, doi: "10.1000/a", url: "u" };

/** 內建的抽文字，寫入與檔頭都是假的。 */
function textTask(opts = {}) {
  const writes = [];
  const asked = [];
  const t = textPagesTask({
    write: async (paperId, draft) => {
      writes.push([paperId, draft]);
      if (opts.writeError) throw opts.writeError;
      return "token";
    },
    states: async (ids) => {
      asked.push(ids);
      return opts.states ?? {};
    },
  });
  return { t, writes, asked };
}

/** 一件只要每頁文字的假抽取。 */
function pagesTask(onRun, over = {}) {
  return { id: "text.pages", version: 1, needsLayout: false, needsPages: true, run: async (input) => onRun(input), ...over };
}

test("抽文字：每頁用 pageText；和文獻連結共用一次開檔、一次讀頁、讀全部的頁", async () => {
  let layoutInput = null;
  const { t, writes } = textTask();
  const texts = [STAMP, "page two", "page three"];
  const { s, calls } = setup([paper("P1", { pageTexts: texts })], { tasks: [layoutTask((i) => (layoutInput = i)), t] });
  assert.equal(await s.step(), "worked");
  assert.deepEqual(calls.opened, ["P1"]);
  assert.deepEqual(calls.read, [3], "一篇只讀一次、讀全部的頁");
  assert.equal(writes.length, 1);
  const [pid, draft] = writes[0];
  assert.equal(pid, "P1");
  assert.deepEqual(
    draft.pages,
    texts.map((x) => pageText(page(x).items)),
  );
  assert.equal(draft.pages[1], "page two\n");
  assert.deepEqual(
    { extractor: draft.extractor, version: draft.version, source_material: draft.source_material, source_sha256: draft.source_sha256, truncated: draft.truncated },
    { extractor: "pdfjs-text", version: 1, source_material: "M-P1", source_sha256: SHA, truncated: false },
  );
  assert.ok(layoutInput.layout, "文獻連結照樣拿到版面分析");
});

test("只有要每頁文字的抽取、原文頁數不知道：先記頁數，讀全部的頁", async () => {
  let got = null;
  const { s, calls } = setup([paper("P1", { pages: null, bib: FULL_BIB })], { tasks: [pagesTask((i) => (got = i))] });
  assert.equal(await s.step(), "worked");
  assert.deepEqual(calls.pages, [["P1", "M-P1", 3]]);
  assert.deepEqual(calls.read, [3]);
  assert.equal(got.pages.length, 3);
  assert.equal(got.source.pages, 3);
  assert.equal(got.layout, null);
});

test("不要每頁文字的抽取拿到 pages: null；只讀第一頁", async () => {
  let got;
  const t = { id: "x.first", version: 1, needsLayout: false, run: async (input) => (got = input) };
  const { s, calls } = setup([paper("P1", { bib: FULL_BIB })], { tasks: [t] });
  assert.equal(await s.step(), "worked");
  assert.deepEqual(calls.read, [1]);
  assert.equal(got.pages, null);
});

test("整份沒有文字（掃描版）：抽文字照樣做，每頁寫空字串", async () => {
  const { t, writes } = textTask();
  const { s, record } = setup([paper("P1", { pageTexts: ["", "", ""], bib: FULL_BIB })], { tasks: [layoutTask(() => {}), t] });
  assert.equal(await s.step(), "worked");
  assert.equal(record().papers.P1["links.references"].reason, "NoText");
  assert.equal(record().papers.P1["text.pages"].result, "done");
  assert.deepEqual(writes[0][1].pages, ["", "", ""]);
  assert.equal(writes[0][1].truncated, false);
});

test("抽文字：單獨的代理碼換成 U+FFFD，UTF-16 長度不變", () => {
  const s = "a\uD800b\uDC00c😀d\uDBFF";
  const w = wellFormed(s);
  assert.equal(w, "a�b�c😀d�");
  assert.equal(w.length, s.length);
  assert.equal(wellFormed("plain"), "plain");
  assert.deepEqual(limitPages(["x\uD800y"]).pages, ["x�y"]);
});

test("抽文字：一頁的上限逐碼點數，截斷不切開代理對", () => {
  const x = "\u{1D465}"; // 基本多文種平面以外的字，兩個 UTF-16 單位
  const over = limitPages([x.repeat(MAX_PAGE_CHARS + 1), "next"]);
  assert.equal(over.truncated, true);
  assert.equal([...over.pages[0]].length, MAX_PAGE_CHARS);
  assert.equal(over.pages[0].length, MAX_PAGE_CHARS * 2, "沒有切開代理對");
  assert.equal(over.pages[1], "next", "截斷那一頁，後面的頁照樣抽");
  const exact = limitPages([x.repeat(MAX_PAGE_CHARS)]);
  assert.equal(exact.truncated, false);
  assert.equal(exact.pages[0].length, MAX_PAGE_CHARS * 2);
});

test("抽文字：一份最多 500 萬字，超過就停在那一頁之前", () => {
  const full = "a".repeat(MAX_PAGE_CHARS);
  const fifty = limitPages(Array(50).fill(full));
  assert.equal(fifty.truncated, false);
  assert.equal(fifty.pages.length, 50);
  const more = limitPages([...Array(50).fill(full), "b", "c"]);
  assert.equal(more.truncated, true);
  assert.equal(more.pages.length, 50);
});

test("抽文字：到了上限照樣寫，設 truncated", async () => {
  const { t, writes } = textTask();
  const big = "w".repeat(MAX_PAGE_CHARS + 5);
  const { s } = setup([paper("P1", { pageTexts: [big, "two", "three"], bib: FULL_BIB })], { tasks: [t] });
  assert.equal(await s.step(), "worked");
  const draft = writes[0][1];
  assert.equal(draft.truncated, true);
  assert.equal(draft.pages.length, 3);
  assert.equal(draft.pages[0].length, MAX_PAGE_CHARS);
});

test("抽文字：做過沒有一次問一批；同一份原文、同一個抽法、版本不小於自己的算做過", async () => {
  const states = {
    P1: { extractor: "pdfjs-text", version: 1, source_sha256: SHA },
    P2: { extractor: "pdfjs-text", version: 1, source_sha256: "b".repeat(64) },
    P3: { extractor: "ocr", version: 5, source_sha256: SHA },
    P4: null,
    P5: { extractor: "pdfjs-text", version: 2, source_sha256: SHA },
  };
  const { t, asked } = textTask({ states });
  const ids = ["P1", "P2", "P3", "P4", "P5", "P6"];
  const done = await t.doneFor(ids.map((id) => ({ paperId: id, sourceSha256: SHA })));
  assert.deepEqual([...done].sort(), ["P1", "P5"]);
  assert.deepEqual(asked, [ids]);
});

test("排程：只把紀錄裡沒做完的篇交給 doneFor，一篇都沒有就不呼叫；說做過的記成 done，下一輪不再問", async () => {
  const asked = [];
  const ran = [];
  const t = pagesTask((i) => ran.push(i.paperId), {
    doneFor: async (batch) => {
      asked.push(batch.map((b) => b.paperId));
      return new Set(["P2"]);
    },
  });
  const rec = { format: 1, papers: { P1: { "text.pages": { sha256: SHA, version: 1, result: "done", reason: null, tries: 1 } } } };
  const papers = ["P1", "P2", "P3"].map((id) => paper(id, { bib: FULL_BIB }));
  const { s, record } = setup(papers, { record: rec, tasks: [t] });
  assert.equal(await s.step(), "worked");
  assert.deepEqual(asked, [["P2", "P3"]]);
  assert.deepEqual(ran, ["P3"]);
  assert.equal(record().papers.P2["text.pages"].result, "done");
  assert.equal(record().papers.P2["text.pages"].sha256, SHA);
  assert.equal(await s.step(), "nothing");
  assert.equal(await s.step(), "nothing");
  assert.deepEqual(asked, [["P2", "P3"]], "全部做完之後不再問");
  assert.equal(s.getProgress().done, 3);
});

test("Progress.pending：每件抽取各自還沒做完的篇數；永久略過、試滿次數的不算", async () => {
  const t = pagesTask((input) => {
    if (input.paperId === "P2") throw { kind: "InvalidInput" };
    if (input.paperId === "P3") throw { kind: "Io" };
  });
  const papers = ["P1", "P2", "P3", "P4"].map((id) => paper(id, { bib: FULL_BIB }));
  const a = setup(papers, { tasks: [t] });
  a.s.setPaused(true);
  assert.equal(await a.s.step(), "paused");
  assert.deepEqual(a.s.getProgress().pending, { "bib.ids": 0, "text.pages": 4 });
  a.s.setPaused(false);
  while ((await a.s.step()) === "worked");
  // P1、P4 做過；P2 永久略過；P3 暫時失敗（還會再試）
  assert.deepEqual(a.s.getProgress().pending, { "bib.ids": 0, "text.pages": 1 });
  assert.equal(a.s.getProgress().done, 3);
  // P3 試滿次數：算做完
  const rec = a.record();
  rec.papers.P3["text.pages"].tries = MAX_TRIES;
  const b = setup(papers, { record: rec, tasks: [t] });
  assert.equal(await b.s.step(), "nothing");
  assert.deepEqual(b.s.getProgress().pending, { "bib.ids": 0, "text.pages": 0 });
  // 訂閱的一方收得到只有 pending 變了：補書目做完、抽文字暫時失敗，做完的篇數不變
  let heard = 0;
  const c = setup([paper("P3")], { tasks: [t] });
  c.s.subscribe(() => heard++);
  assert.equal(await c.s.step(), "worked");
  assert.deepEqual(c.s.getProgress(), { done: 0, total: 1, paused: false, pending: { "bib.ids": 0, "text.pages": 1 } });
  assert.equal(heard, 2, "開始一輪一次、pending 變了一次");
});

test("錯誤的分類與讀不懂的紀錄", () => {
  assert.equal(classifyError({ kind: "Conflict" }).result, "failed");
  assert.equal(classifyError({ kind: "NoLibrary" }).result, "failed");
  assert.equal(classifyError({ kind: "Unreadable" }).result, "skipped");
  assert.equal(classifyError(new SourceUnusable("Encrypted")).reason, "Encrypted");
  assert.equal(classifyError(new Error("x")).result, "failed");
  assert.deepEqual(parseRecord(null), { format: 1, papers: {} });
  assert.deepEqual(parseRecord({ papers: { P: { t: { sha256: 1 } } } }), { format: 1, papers: {} });
});

test("補書目的三欄：DOI 照順序、年份照 arXiv 戳記", () => {
  const f = bibFromInput({ info: { CreationDate: "D:20250101" }, xmp: null, firstPageText: STAMP + "\nDOI: 10.1109/CVPR.2024.01605" });
  assert.deepEqual(f, { year: 2023, doi: "10.1109/cvpr.2024.01605", url: "https://arxiv.org/abs/2304.08069" });
});

test("PDF 佇列：一次一件，優先的排在背景的前面", async () => {
  const order = [];
  let releaseFirst;
  const first = enqueuePdf("background", () => new Promise((r) => (releaseFirst = () => r(order.push("bg1")))));
  const bg2 = enqueuePdf("background", async () => order.push("bg2"));
  const urgent = enqueuePdf("urgent", async () => order.push("urgent"));
  assert.equal(urgentWaiting(), true);
  releaseFirst();
  await Promise.all([first, bg2, urgent]);
  assert.deepEqual(order, ["bg1", "urgent", "bg2"]);
  assert.equal(urgentWaiting(), false);
  // 一件失敗不擋後面的
  await assert.rejects(enqueuePdf("urgent", async () => Promise.reject(new Error("x"))));
  assert.equal(await enqueuePdf("background", async () => 7), 7);
});
