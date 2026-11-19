// 版面紀錄（`src/translate/layout.ts`）的測試：用合成的文字層資料驗段落、雙欄順序、頁首頁尾、圖與公式、參考文獻。
// 執行：node --test scripts/layout-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  columnAt,
  formulaFragment,
  layoutDocument,
  layoutPages as rawLayoutPages,
  mathText,
} from "../src/translate/layout.ts";

/**
 * 每個案例都經過這裡：`layoutPages` 等於 `layoutDocument(...).segments`；`segments` 裡沒有
 * `reference`；`references` 都是 `reference`、依頁序。
 */
function layoutPages(pages) {
  const doc = layoutDocument(pages);
  const segs = rawLayoutPages(pages);
  assert.deepEqual(segs, doc.segments);
  assert.ok(segs.every((s) => s.kind !== "reference"));
  assert.ok(doc.references.every((s) => s.kind === "reference"));
  assert.ok(doc.references.every((s, i, a) => i === 0 || s.page >= a[i - 1].page));
  return segs;
}

const VIEW = [0, 0, 612, 792];

/** 一行文字（一個文字區塊）。字寬用字級的一半。 */
function it(x, y, str, size = 10) {
  return { str, transform: [size, 0, 0, size, x, y], width: str.length * size * 0.5, height: size };
}

/** 一個段落：每行約 `chars` 個字，行距 12。回傳 [items, 下一個 y]。 */
function para(x, y, words, { size = 10, perLine = 8, indent = 0 } = {}) {
  const items = [];
  for (let i = 0; i < words.length; i += perLine) {
    const text = words.slice(i, i + perLine).join(" ");
    items.push(it(x + (i === 0 ? indent : 0), y, text, size));
    y -= 12;
  }
  return [items, y];
}

// 只用字母（含數字太多的文字會被當成公式）
const id2 = (i) => String.fromCharCode(97 + Math.floor(i / 26)) + String.fromCharCode(97 + (i % 26));
const w = (prefix, n) => Array.from({ length: n }, (_, i) => `${prefix}${id2(i)}`);

test("兩個段落：行距變大與首行縮排都分段", () => {
  const [a, y1] = para(72, 700, w("alpha", 24), { perLine: 12 });
  const [b, y2] = para(72, y1 - 14, w("theta", 24), { perLine: 12 });
  const [c] = para(72, y2, w("gamma", 24), { perLine: 12, indent: 12 });
  const out = layoutPages([{ view: VIEW, items: [...a, ...b, ...c] }]);
  assert.equal(out.length, 3);
  assert.ok(out[0].source.startsWith("alphaaa alphaab"));
  assert.ok(out[0].source.endsWith("alphaax"));
  assert.ok(out[1].source.startsWith("thetaaa"));
  assert.ok(out[2].source.startsWith("gammaaa"));
  assert.equal(out[0].page, 0);
  assert.ok(out[0].bbox[0] <= 72 && out[0].bbox[2] > 72 + 100 && out[0].bbox[1] < 700 && out[0].bbox[3] > 700);
  assert.equal(out[0].size, 10);
});

test("行尾的連字號接起來，上標跟著主行", () => {
  const items = [it(72, 700, "the quick brown fox jumps over the lazy dog transform-", 10), it(72, 688, "ation of the input x", 10)];
  items.push({ str: "2", transform: [6, 0, 0, 6, 72 + 20 * 5, 691], width: 3, height: 6 });
  const out = layoutPages([{ view: VIEW, items: [...items, ...para(72, 650, w("w", 24), { perLine: 12 })[0]] }]);
  assert.equal(out.length, 2, out.map((x) => x.source).join(" | "));
  assert.ok(out[0].source.startsWith("the quick brown fox jumps over the lazy dog transformation of the input x"), out[0].source);
  assert.ok(!out[0].source.includes("- "));
});

test("雙欄：跨欄的標題先、左欄全部、再右欄", () => {
  const items = [it(72, 740, "A Study of Column Reading Order Across Two Columns", 16)];
  items[0].width = 468; // 標題橫跨兩欄
  let [left] = para(72, 700, w("L", 96), { perLine: 8 });
  let [right] = para(322, 700, w("R", 96), { perLine: 8 });
  // 左右欄都要夠長才找得到中縫：每欄 12 行
  assert.equal(left.length, 12);
  for (const x of left) x.width = 218;
  for (const x of right) x.width = 218;
  const out = layoutPages([{ view: VIEW, items: [...items, ...right, ...left] }]);
  const texts = out.map((s) => s.source);
  assert.ok(texts[0].startsWith("A Study"), texts[0]);
  assert.equal(out[0].kind, "heading");
  const iLeft = texts.findIndex((t) => t.startsWith("Laa "));
  const iRight = texts.findIndex((t) => t.startsWith("Raa "));
  assert.ok(iLeft > 0 && iRight > iLeft, `${iLeft} ${iRight}`);
  // 左欄的段落沒有混進右欄的字
  assert.ok(!texts[iLeft].includes("Raa"));
  assert.ok(texts.some((t) => t.includes("Lcu")));
});

test("頁首頁尾與頁碼不翻", () => {
  const pages = [];
  for (let p = 1; p <= 4; p++) {
    const items = [it(72, 770, "Journal of Examples 2024", 8), it(300, 30, String(p), 9)];
    items.push(...para(72, 700, w("pw", 24), { perLine: 12 })[0]);
    pages.push({ view: VIEW, items });
  }
  const out = layoutPages(pages);
  assert.equal(out.length, 4);
  for (const s of out) {
    assert.ok(!s.source.includes("Journal of Examples"), s.source);
    assert.ok(!/^\d+$/.test(s.source.trim()));
  }
});

test("離上下緣稍遠、在多頁同一高度重複的一行短字也是頁首頁尾", () => {
  // 頁尾的下緣在頁高 7% 之外（期刊的「Article number, page N」）
  const pages = [];
  for (let p = 1; p <= 4; p++) {
    const items = [it(72, 64, `Example Journal 12, Article number, page ${p}`, 9)];
    items.push(...para(72, 600, w("pw", 24), { perLine: 12 })[0]);
    // 只在第 2 頁、同一個位置的一行字：不是頁尾，照翻
    if (p === 2) items.push(it(72, 720, "Received in final form without further changes", 9));
    pages.push({ view: VIEW, items });
  }
  const out = layoutPages(pages);
  assert.ok(out.every((s) => !s.source.includes("Article number")), out.map((s) => s.source).join(" | "));
  assert.equal(out.filter((s) => s.source.startsWith("pwaa")).length, 4);
  assert.ok(out.some((s) => s.source.startsWith("Received in final form")));
});

test("每頁同一個位置的多行內文不當成頁首頁尾", () => {
  const pages = [];
  for (let p = 1; p <= 4; p++) pages.push({ view: VIEW, items: para(72, 70, w("same", 24), { perLine: 12 })[0] });
  const out = layoutPages(pages);
  assert.equal(out.length, 4, out.map((s) => s.source).join(" | "));
});

test("圖說要翻；座標軸標籤、純數字、公式不翻", () => {
  const [body, y] = para(72, 700, w("body", 24), { perLine: 12 });
  const figure = [
    it(100, y - 30, "Epoch", 7),
    it(60, y - 60, "0.9", 7),
    it(200, y - 70, "Accuracy", 7),
  ];
  const caption = para(72, y - 120, ["Figure", "1:", "Accuracy", "of", "the", "model", "over", "epochs", "on", "the", "test", "set."], {
    perLine: 12,
    size: 9,
  })[0];
  const formula = [it(200, y - 160, "= Σ ∫ ∂ ± ≤ ≥ ∞ → ←", 10)];
  const out = layoutPages([{ view: VIEW, items: [...body, ...figure, ...caption, ...formula] }]);
  const kinds = out.map((s) => `${s.kind}:${s.source.slice(0, 10)}`);
  assert.equal(out.length, 2, kinds.join(" | "));
  assert.equal(out[1].kind, "caption");
  assert.ok(out[1].source.startsWith("Figure 1:"));
});

test("參考文獻不翻，之後的附錄標題恢復", () => {
  const items = [];
  const [body, y] = para(72, 700, w("text", 24), { perLine: 12 });
  items.push(...body);
  items.push(it(72, y - 24, "References", 13));
  let yy = y - 44;
  for (let i = 1; i <= 3; i++) {
    items.push(it(72, yy, `[${i}] Smith J, Lee K. A paper about things and stuff. Journal of Things, 20${10 + i}.`, 10));
    yy -= 24;
  }
  items.push(it(72, yy - 10, "Appendix A Proofs of everything", 13));
  const [more] = para(72, yy - 34, w("proof", 24), { perLine: 12 });
  items.push(...more);
  const out = layoutPages([{ view: VIEW, items }]);
  const texts = out.map((s) => s.source);
  assert.ok(texts.every((t) => !t.startsWith("[")), texts.join(" | "));
  assert.ok(texts.every((t) => t !== "References"));
  assert.ok(texts.some((t) => t.startsWith("Appendix A")));
  assert.ok(texts.some((t) => t.startsWith("proofaa")));
  // 另外交回的參考文獻區：三條依順序（這組合成的條目併成一段），「References」標題本身與附錄不算
  const refs = layoutDocument([{ view: VIEW, items }]).references.map((s) => s.source).join(" | ");
  assert.ok(refs.indexOf("[1]") === 0 && refs.indexOf("[2]") > 0 && refs.indexOf("[3]") > refs.indexOf("[2]"), refs);
  assert.ok(!refs.includes("References") && !refs.includes("Appendix") && !refs.includes("proofaa"), refs);
});

test("字體：行首的粗體小標開新段，整行粗體的短行是標題", () => {
  // 內文用字型 R，粗體用 B；三段之間沒有空行也沒有縮排，只靠字體分開
  const f = (x) => ({ ...x, fontName: x.fontName ?? "R" });
  const items = [];
  let y = 700;
  const line = (parts) => {
    let x = 72;
    for (const [str, font] of parts) {
      items.push(f({ ...it(x, y, str, 10), fontName: font }));
      x += str.length * 5 + 2.5;
    }
    y -= 12;
  };
  for (const row of [w("ra", 12), w("rb", 12)]) line([[row.join(" "), "R"]]);
  line([[w("rc", 11).join(" ") + " end.", "R"]]);
  line([["Inner Product on Action Space.", "B"], [w("sa", 7).join(" "), "R"]]);
  for (const row of [w("sb", 12), w("sc", 12)]) line([[row.join(" "), "R"]]);
  line([["Supplemental Material", "B"]]);
  for (const row of [w("ta", 12), w("tb", 12)]) line([[row.join(" "), "R"]]);
  const out = layoutPages([{ view: VIEW, items }]);
  const texts = out.map((s) => `${s.kind}:${s.source.slice(0, 20)}`);
  assert.equal(out.length, 4, texts.join(" | "));
  assert.ok(out[1].source.startsWith("Inner Product on Action Space. saaa"), texts.join(" | "));
  assert.equal(out[2].source, "Supplemental Material");
  assert.equal(out[2].kind, "heading");
  assert.ok(out[3].source.startsWith("taaa"));
});

test("沒有字型資訊時照舊：同一段不因字體切開", () => {
  const [a] = para(72, 700, w("alpha", 36), { perLine: 12 });
  const out = layoutPages([{ view: VIEW, items: a }]);
  assert.equal(out.length, 1);
});

test("首頁篇名與摘要之間的作者與單位不翻，摘要沒有標籤也認得", () => {
  const items = [
    it(150, 740, "A Study of Things Under Many Conditions", 16),
    it(200, 715, "R. R. Laher12", 11),
    it(180, 700, "Department of Examples, Some Place 12345, Country", 9),
  ];
  const prose = "we study how things behave and find that they mostly do what is expected of them".split(" ");
  const [abs, y] = para(100, 670, [...prose, ...prose, ...prose], { perLine: 10, size: 9 });
  const [body] = para(72, y - 30, w("body", 24), { perLine: 12 });
  const out = layoutPages([{ view: VIEW, items: [...items, ...abs, ...body] }]);
  const texts = out.map((s) => s.source);
  assert.ok(texts[0].startsWith("A Study"), texts.join(" | "));
  assert.ok(!texts.some((t) => t.includes("Laher") || t.includes("Department")), texts.join(" | "));
  assert.ok(texts.some((t) => t.startsWith("we study")));
  assert.ok(texts.some((t) => t.startsWith("bodyaa")));
});

test("首頁小字的摘要與圖說提到機構或 mAP@0.5 也要翻；頁底的作者註腳不翻", () => {
  const items = [it(150, 740, "A Study of Things Under Many Conditions", 16), it(72, 700, "Abstract", 10)];
  const abs = "our laboratory reports mAP@0.5 gains and we study how things behave in many settings".split(" ");
  const [a, y] = para(72, 686, [...abs, ...abs, ...abs], { perLine: 12, size: 9 });
  const [body, y2] = para(72, y - 20, w("body", 36), { perLine: 12 });
  const cap = it(72, y2 - 20, "Figure 1: Results from the laboratory at the university over many runs.", 9);
  const [more] = para(72, y2 - 50, w("more", 36), { perLine: 12 });
  const note = it(72, 60, "The authors are with the Department of Examples, Some University.", 8);
  const out = layoutPages([{ view: VIEW, items: [...items, ...a, ...body, cap, ...more, note] }]);
  const texts = out.map((s) => s.source);
  assert.ok(texts.some((t) => t.startsWith("our laboratory")), texts.join(" | "));
  assert.ok(texts.some((t) => t.startsWith("Figure 1:")), texts.join(" | "));
  assert.ok(!texts.some((t) => t.includes("are with the Department")), texts.join(" | "));
});

// ---------- 真論文的情況（片段取自公開組 arXiv 2611.16305、16841、17159、17242、17250，CC BY 4.0） ----------

test("公式的碎片認得出來；篇名、章節標題、短句、小型大寫的圖說不算", () => {
  const fragments = [
    "X H 2 X H 2 2", // 分母（式 7）
    "X3 e 1",
    "H X H",
    "1 + VM",
    "= 1 + VM + VP",
    "K sg λi",
    "(49) C(W) ≤ Γorig ;",
    "W = Wrep, F(Wc) = 0 and F(Wrep) = 1,",
    "SKRtheory = Rs(1 − FER) (βIAB − χBE)−∆(n),",
    " CX 0 VX 0 ", // 矩陣的括號是私用區的字
    "irrelevant 40 40", // 圖的座標軸
    "0.487 Mbit/s", // 表格的格子
  ];
  for (const f of fragments) assert.equal(formulaFragment(f), true, f);
  const text = [
    "Two Unconventional Hall Effects in the Kagome Magnet TmMn4Cr2Sn6",
    "2 Related Work",
    "I. INTRODUCTION",
    "A. Overview",
    "estimation",
    "where",
    "according to (34)–(37).",
    "TABLE VIII C ROSS - DATASET EVALUATION WITH M AND I AS HELD - OUT TARGETS",
    "The achievable rate of user k is",
    "Substituting the mass formula and letting c ≡ R200/rs:",
    // 短標題：一個英文字加數字、希臘字母或符號
    "3D Reconstruction",
    "γ-ray bursts",
    "C++ implementation",
    "5G networks",
    "IEEE 802.11ax",
  ];
  for (const t of text) assert.equal(formulaFragment(t), false, t);
  // 公式為主（產生譯本 PDF 時保留原文）包含碎片
  assert.equal(mathText("X H 2 X H 2 2"), true);
  assert.equal(mathText("where $\\mathcal{L}$ is the loss"), true);
  assert.equal(mathText("The achievable rate of user k is"), false);
});

test("公式的碎片不進版面紀錄，前後的內文照收", () => {
  const [a, y1] = para(72, 700, w("alpha", 24), { perLine: 12 });
  const frag = it(150, y1 - 30, "X H 2 X H 2 2", 10);
  const [b] = para(72, y1 - 60, w("theta", 24), { perLine: 12 });
  const out = layoutPages([{ view: VIEW, items: [...a, frag, ...b] }]);
  const texts = out.map((s) => s.source);
  assert.ok(!texts.some((t) => t.includes("X H 2")), texts.join(" | "));
  assert.ok(texts.some((t) => t.startsWith("alphaaa")) && texts.some((t) => t.startsWith("thetaaa")));
});

/** 雙欄頁的一欄：12 行內文（夠長才找得到中縫），交回 [文字片段, 下一行的 y]。 */
function column(x, prefix) {
  const [items, y] = para(x, 700, w(prefix, 96), { perLine: 8 });
  for (const t of items) t.width = 218;
  return [items, y];
}

test("APS 樣式的參考文獻：沒有標題、條目懸掛縮排、書名換了字型、跨頁接下去的條目都不翻；同頁另一欄的內文照翻", () => {
  const [left, ly] = column(72, "L");
  const [right, ry] = column(322, "R");
  const ref = (x, y, str, font = "F1") => ({ ...it(x, y, str, 8), fontName: font });
  const refs = [
    ref(76, ly - 30, "[1] N. Gisin, G. Ribordy, W. Tittel, and H. Zbinden, Quan-"),
    ref(90, ly - 40, "tum cryptography, Rev. Mod. Phys. 74, 145 (2002)."),
    ref(76, ly - 50, "[2] N. Gisin and R. Thew, Quantum communication, Nature"),
    ref(90, ly - 60, "Photonics 1, 165 (2007)."),
    // 右欄：內文之後接著條目（這一頁左欄已經有條目）；書名用斜體字型、沒有年份
    ref(326, ry - 30, "[4] R. Wolf, Quantum key distribution protocols, in Quan-"),
    ref(340, ry - 40, "tum Key Distribution: An Introduction with Exercises", "F2"),
    ref(326, ry - 50, "[5] M. F. Anka, J. A. Mora Rodr´ıguez, D. F. Pinto, L. Q."),
    ref(340, ry - 60, "Galv˜ao, M. A. Dias, and A. B. Tacla, An introductory review of the theory"),
  ];
  for (const t of [...left, ...right]) t.fontName = "F1";
  const p2 = [
    ref(90, 740, "Brazilian Journal of Physics 56, 72 (2026)."),
    ref(76, 728, "[6] F. Grosshans and P. Grangier, Continuous variable quan-"),
    ref(90, 718, "tum cryptography using coherent states, arXiv preprint quant-ph/0109084 (2001)."),
  ];
  const out = layoutPages([
    { view: VIEW, items: [...left, ...right, ...refs] },
    { view: VIEW, items: p2 },
  ]);
  const texts = out.map((s) => s.source);
  for (const name of ["Gisin", "Thew", "Photonics", "Wolf", "Key Distribution", "Anka", "Tacla", "Brazilian", "Grosshans", "coherent states"])
    assert.ok(!texts.some((t) => t.includes(name)), `${name} | ${texts.join(" | ")}`);
  assert.ok(texts.some((t) => t.startsWith("Laa ")), texts.join(" | "));
  assert.ok(texts.some((t) => t.startsWith("Raa ")), texts.join(" | "));
  // 沒有標題的參考文獻也另外交回，依閱讀順序（左欄、右欄、下一頁）
  const docRefs = layoutDocument([
    { view: VIEW, items: [...left, ...right, ...refs] },
    { view: VIEW, items: p2 },
  ]).references.map((s) => s.source);
  const order = ["Gisin, G. Ribordy", "Thew", "Wolf", "Anka", "Brazilian", "Grosshans"].map((n) =>
    docRefs.findIndex((t) => t.includes(n)),
  );
  assert.ok(
    order.every((i, k) => i >= 0 && (k === 0 || i >= order[k - 1])),
    `${order} | ${docRefs.join(" | ")}`,
  );
});

test("參考文獻之後，附錄的標題和第一段併在一起也恢復翻譯", () => {
  const items = [];
  const [body, y] = para(72, 700, w("text", 24), { perLine: 12 });
  items.push(...body);
  items.push(it(72, y - 24, "References", 13));
  let yy = y - 44;
  for (let i = 1; i <= 3; i++) {
    items.push(it(72, yy, `Smith, J., Lee, K., & Park, S. 20${10 + i}, ApJ, ${100 + i}, ${20 + i}`, 10));
    yy -= 14;
  }
  // 附錄標題下面沒有空行、又和內文一樣寬，切段的規則把它和第一段併成一段
  const head = it(72, yy - 20, "Appendix A: Computing R500 from R200 for SGCC", 10);
  const [more] = para(72, yy - 32, w("proof", 24), { perLine: 12 });
  head.width = more[0].width;
  items.push(head, ...more);
  const out = layoutPages([{ view: VIEW, items }]);
  const texts = out.map((s) => s.source);
  assert.ok(texts.some((t) => t.startsWith("Appendix A") && t.includes("proofaa")), texts.join(" | "));
  assert.ok(!texts.some((t) => t.includes("Smith")), texts.join(" | "));
});

test("篇名排成兩行、被切成兩段時併成一段", () => {
  const items = [
    it(140, 720, "Characterising a population of fast-declining SNe Ia in galaxy", 17),
    it(250, 690, "cluster cores", 17),
    it(200, 650, "A. Eranhalodi, J. Nordin, and M. Kowalski", 10),
  ];
  const [abs] = para(72, 600, ["Abstract.", ...w("abs", 40)], { perLine: 12 });
  const out = layoutPages([{ view: VIEW, items: [...items, ...abs] }]);
  const texts = out.map((s) => s.source);
  assert.ok(texts.includes("Characterising a population of fast-declining SNe Ia in galaxy cluster cores"), texts.join(" | "));
  assert.ok(!texts.includes("cluster cores"), texts.join(" | "));
});

test("表格的一列（同一高度三段以上的短字）不翻", () => {
  const [left, ly] = column(72, "L");
  const [right] = column(322, "R");
  const row = [it(72, ly - 40, "Work Channel", 9), it(250, ly - 40, "Security / Processing", 9), it(430, ly - 40, "Detection Rate", 9)];
  const out = layoutPages([{ view: VIEW, items: [...left, ...right, ...row] }]);
  const texts = out.map((s) => s.source);
  for (const cell of ["Work Channel", "Security", "Detection Rate"])
    assert.ok(!texts.some((t) => t.includes(cell)), `${cell} | ${texts.join(" | ")}`);
  assert.ok(texts.some((t) => t.startsWith("Laa ")) && texts.some((t) => t.startsWith("Raa ")));
});

test("沒有文字的頁面與空輸入不出錯", () => {
  assert.deepEqual(layoutPages([]), []);
  assert.deepEqual(layoutPages([{ view: VIEW, items: [] }, { view: VIEW, items: [it(10, 10, "   ", 10)] }]), []);
});

// ---------- 雙擊放大到欄寬：點的那一欄（`columnAt`） ----------

/** 雙欄頁面：跨欄的篇名，左欄從 72 到 290、右欄從 322 到 540，每欄 11 行。 */
function twoColumnPage() {
  const title = it(72, 740, "A Study of Column Reading Order Across Two Columns", 16);
  title.width = 468;
  const [left] = para(72, 700, w("L", 96), { perLine: 9 });
  const [right] = para(322, 700, w("R", 96), { perLine: 9 });
  for (const x of [...left, ...right]) x.width = 218;
  // 最後一行比較短（段落的末行），不影響邊界
  left[left.length - 1].width = 60;
  return { view: VIEW, items: [title, ...left, ...right] };
}

test("雙欄：點左欄、右欄，各自放大到那一欄；不同高度邊界一樣", () => {
  const page = twoColumnPage();
  for (const y of [695, 640, 590]) {
    assert.deepEqual(columnAt(page, 120, y), { x0: 72, x1: 290, kind: "column" }, `左欄 y=${y}`);
    assert.deepEqual(columnAt(page, 420, y), { x0: 322, x1: 540, kind: "column" }, `右欄 y=${y}`);
  }
  // 點在中縫偏左一點：還是左欄
  assert.equal(columnAt(page, 300, 650)?.x1, 290);
});

test("雙欄：點在跨欄的篇名上，放大到整個文字區", () => {
  assert.deepEqual(columnAt(twoColumnPage(), 120, 745), { x0: 72, x1: 540, kind: "span" });
});

test("單欄：文字區的寬度；表格的格子造出的「中縫」不算一欄", () => {
  const [body, y1] = para(72, 700, w("body", 192), { perLine: 16 });
  for (const x of body) x.width = 468;
  const single = { view: VIEW, items: body };
  assert.deepEqual(columnAt(single, 120, 650), { x0: 72, x1: 540, kind: "single" });
  // 表格：每列左右兩格數字，中間空著
  const rows = [];
  for (let i = 0; i < 6; i++) {
    const y = y1 - 20 - i * 12;
    rows.push(it(72, y, "12.34 0.567 0.890 1.234 5.678", 10), it(330, y, "98.76 0.543 0.210 9.876 5.432", 10));
  }
  const withTable = { view: VIEW, items: [...body, ...rows] };
  const c = columnAt(withTable, 120, y1 - 30);
  assert.equal(c?.kind, "single");
  assert.equal(c?.x0, 72);
  assert.equal(c?.x1, 540);
});

test("沒有橫排文字（掃描版、只有頁邊的直排浮水印）交回 null", () => {
  assert.equal(columnAt({ view: VIEW, items: [] }, 100, 100), null);
  const stamp = { str: "arXiv:2611.16305v1 [eess.SP] 12 Nov 2026", transform: [0, 20, -20, 0, 30, 300], width: 300, height: 20 };
  assert.equal(columnAt({ view: VIEW, items: [stamp] }, 100, 100), null);
});

// 公開組的真論文（`samples/public/`，不在版本庫裡；沒有就略過）：雙欄論文點不同高度邊界一樣、單欄論文是文字區
const here = path.dirname(fileURLToPath(import.meta.url));
const publicDir = path.resolve(here, "..", "..", "samples", "public");
const mainPublic = path.resolve(here, "..", "..", "..", "..", "..", "samples", "public"); // worktree 裡時用主工作資料夾的
const samples = [publicDir, mainPublic].find((d) => fs.existsSync(path.join(d, "arxiv_2611.16305.pdf")));

async function realPage(file, n) {
  const pdfjs = await import("pdfjs-dist/legacy/build/pdf.mjs");
  const doc = await pdfjs.getDocument({ data: new Uint8Array(fs.readFileSync(path.join(samples, file))), verbosity: 0 }).promise;
  const page = await doc.getPage(n);
  const content = await page.getTextContent();
  const items = content.items
    .filter((x) => "str" in x)
    .map((x) => ({ str: x.str, transform: x.transform, width: x.width, height: x.height, fontName: x.fontName }));
  const view = page.view;
  await doc.loadingTask.destroy();
  return { view, items };
}

test("真論文：雙欄（IEEE）與單欄的欄寬", { skip: samples ? false : "沒有 samples/public" }, async () => {
  const two = await realPage("arxiv_2611.16305.pdf", 2);
  const [, , W, H] = two.view;
  const lefts = [0.2, 0.4, 0.6, 0.8].map((f) => columnAt(two, 0.27 * W, (1 - f) * H));
  const rights = [0.2, 0.4, 0.6, 0.8].map((f) => columnAt(two, 0.73 * W, (1 - f) * H));
  for (const c of lefts) assert.deepEqual(c, lefts[0]);
  for (const c of rights) assert.deepEqual(c, rights[0]);
  assert.equal(lefts[0]?.kind, "column");
  assert.ok(Math.abs((lefts[0]?.x0 ?? 0) - 49) < 3 && Math.abs((lefts[0]?.x1 ?? 0) - 300) < 3, JSON.stringify(lefts[0]));
  assert.ok(Math.abs((rights[0]?.x0 ?? 0) - 312) < 3 && Math.abs((rights[0]?.x1 ?? 0) - 563) < 3, JSON.stringify(rights[0]));
  // 第 1 頁的摘要橫跨兩欄（17159）：點在摘要上是整個文字區，點在下半頁的內文是一欄
  const first = await realPage("arxiv_2611.17159.pdf", 1);
  const [, , W1, H1] = first.view;
  assert.equal(columnAt(first, 0.27 * W1, 0.75 * H1)?.kind, "span");
  assert.equal(columnAt(first, 0.27 * W1, 0.2 * H1)?.kind, "column");
  // 單欄論文（17331），有表格的那一頁也是文字區
  for (const n of [7, 10]) {
    const one = await realPage("arxiv_2611.17331.pdf", n);
    const [, , W2, H2] = one.view;
    const c = columnAt(one, 0.27 * W2, 0.6 * H2);
    assert.ok(c && c.kind !== "column" && c.x1 - c.x0 > 0.6 * W2, `第 ${n} 頁：${JSON.stringify(c)}`);
  }
});
