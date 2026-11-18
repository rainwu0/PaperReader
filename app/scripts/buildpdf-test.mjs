// 譯本 PDF（`src/translate/buildPdf.ts`、`textFilter.ts`、`cff2.ts`）的測試：
// 用 pdf-lib 做一份兩頁的「原文」，組出譯本，再用 PDF.js 讀回來確認：頁數、沒翻的頁原樣、
// 蓋住的範圍記在文件資訊、文字層濾掉被蓋的原文只留譯文；再用合成的 CFF2 字型驗轉換。
// 字型檔本身不進版本庫：用 Helvetica 驗組裝；真的中日韓字型要設環境變數 `PR_TEST_CJK_FONT`
// （例如從模擬器 `adb pull /system/fonts/NotoSansCJK-Regular.ttc`），沒設就略過那一項。
// 執行：node --test scripts/buildpdf-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import { PDFDocument, PDFName, PDFOperator, PDFOperatorNames, StandardFonts, setFillingRgbColor, pushGraphicsState, popGraphicsState } from "pdf-lib";
import * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs";
import { buildTranslatedPdf, coverRects, graphicsFromOps, touchesGraphics } from "../src/translate/buildPdf.ts";
import { coversFromInfo, filterChunk, TEXT_TAG } from "../src/translate/textFilter.ts";
import { cff2ToCff1Subset } from "../src/translate/cff2.ts";

/** 兩頁的原文：第 1 頁有兩段英文與一條「圖」，第 2 頁一段英文。 */
async function sourcePdf() {
  const doc = await PDFDocument.create();
  const font = await doc.embedFont(StandardFonts.Helvetica);
  const p1 = doc.addPage([400, 300]);
  p1.drawText("Original paragraph one in English", { x: 40, y: 240, size: 10, font });
  p1.drawText("Original paragraph two in English", { x: 40, y: 200, size: 10, font });
  p1.drawRectangle({ x: 40, y: 100, width: 200, height: 60 }); // 圖
  const p2 = doc.addPage([400, 300]);
  p2.drawText("Second page keeps its English", { x: 40, y: 240, size: 10, font });
  return await doc.save();
}

const SEGMENTS = [
  { page: 0, bbox: [39, 237, 240, 252], kind: "body", size: 10, target: "Translated one" },
  { page: 0, bbox: [39, 197, 240, 212], kind: "body", size: 10, target: "Translated two" },
];

/** 測試用的字型：Helvetica 只有拉丁字母，足夠驗組裝。 */
async function helvetica(doc) {
  const font = await doc.embedFont(StandardFonts.Helvetica);
  const registered = new WeakSet();
  return {
    name: "Helvetica",
    advance: (cp) => (cp < 32 || cp > 126 ? null : font.widthOfTextAtSize(String.fromCodePoint(cp), 1000)),
    drawLine(page, text, x, y, st) {
      if (!registered.has(page)) {
        page.node.setFontDictionary(PDFName.of("PRTr"), font.ref);
        registered.add(page);
      }
      page.pushOperators(
        pushGraphicsState(),
        setFillingRgbColor(...st.color),
        PDFOperator.of(PDFOperatorNames.BeginText),
        PDFOperator.of(PDFOperatorNames.SetFontAndSize, [PDFName.of("PRTr"), pdfNum(st.size)]),
        PDFOperator.of(PDFOperatorNames.MoveText, [pdfNum(x), pdfNum(y)]),
        PDFOperator.of(PDFOperatorNames.ShowText, [font.encodeText(text)]),
        PDFOperator.of(PDFOperatorNames.EndText),
        popGraphicsState(),
      );
    },
    finish: async () => {},
  };
}
import { PDFNumber } from "pdf-lib";
const pdfNum = (n) => PDFNumber.of(n);

async function open(bytes) {
  return await pdfjs.getDocument({ data: new Uint8Array(bytes), useSystemFonts: false, isEvalSupported: false }).promise;
}

test("組出的譯本頁數不變、沒翻的頁原樣、蓋住的範圍記在文件資訊", async () => {
  const src = await sourcePdf();
  const r = await buildTranslatedPdf({
    sourceBytes: src,
    segments: SEGMENTS,
    fontBytes: new Uint8Array(),
    lang: "zh-TW",
    makeTypeface: helvetica,
  });
  assert.ok(r);
  assert.equal(r.pages, 2);
  assert.equal(r.placed, 2);
  assert.equal(r.truncated, 0);
  const pdf = await open(r.bytes);
  assert.equal(pdf.numPages, 2);
  const meta = await pdf.getMetadata();
  const covers = coversFromInfo(meta.info);
  assert.ok(covers, "文件資訊裡有蓋住的範圍");
  assert.deepEqual(Object.keys(covers.pages), ["0"]);
  assert.deepEqual(covers.pages["0"], [
    [39, 237, 240, 252],
    [39, 197, 240, 212],
  ]);
  // 第 2 頁沒有動：文字層還是原來的英文
  const p2 = await (await pdf.getPage(2)).getTextContent();
  assert.equal(p2.items.map((i) => i.str).join(""), "Second page keeps its English");
});

test("文字層濾掉被蓋住的原文，只留譯文；沒蓋的原文（圖旁的字）留著", async () => {
  const src = await sourcePdf();
  // 在圖旁多加一段沒翻的英文
  const doc = await PDFDocument.load(src);
  const font = await doc.embedFont(StandardFonts.Helvetica);
  doc.getPage(0).drawText("Untouched caption", { x: 40, y: 80, size: 9, font });
  const r = await buildTranslatedPdf({
    sourceBytes: await doc.save(),
    segments: SEGMENTS,
    fontBytes: new Uint8Array(),
    lang: "zh-TW",
    makeTypeface: helvetica,
  });
  const pdf = await open(r.bytes);
  const meta = await pdf.getMetadata();
  const covers = coversFromInfo(meta.info);
  const page = await pdf.getPage(1);
  const reader = page.streamTextContent({ includeMarkedContent: true }).getReader();
  const stack = [];
  const kept = [];
  const all = [];
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    for (const it of value.items) if (it.str !== undefined) all.push(it.str);
    kept.push(...filterChunk(value, covers.pages["0"], stack).items.map((i) => i.str));
  }
  const joined = kept.join("|");
  assert.ok(all.join("|").includes("Original paragraph one"), "未過濾時原文還在");
  assert.ok(!joined.includes("Original paragraph"), `濾掉被蓋的原文：${joined}`);
  assert.ok(joined.includes("Translated one") && joined.includes("Translated two"), joined);
  assert.ok(joined.includes("Untouched caption"), "沒蓋的原文留著");
  assert.equal(stack.length, 0, "標記內容成對");
  assert.ok(TEXT_TAG.length > 0);
});

test("被取消就交回 null；超出頁數的段落略過；空譯文略過", async () => {
  const src = await sourcePdf();
  const cancelled = await buildTranslatedPdf({
    sourceBytes: src,
    segments: SEGMENTS,
    fontBytes: new Uint8Array(),
    lang: "zh-TW",
    makeTypeface: helvetica,
    cancelled: () => true,
  });
  assert.equal(cancelled, null);
  const r = await buildTranslatedPdf({
    sourceBytes: src,
    segments: [
      { page: 9, bbox: [1, 1, 50, 20], kind: "body", size: 10, target: "beyond" },
      { page: 0, bbox: [39, 237, 240, 252], kind: "body", size: 10, target: "   " },
    ],
    fontBytes: new Uint8Array(),
    lang: "zh-TW",
    makeTypeface: helvetica,
  });
  assert.equal(r.placed, 0);
  const meta = await (await open(r.bytes)).getMetadata();
  assert.deepEqual(coversFromInfo(meta.info).pages, {});
});

test("放不下的譯文：縮字級；縮到下限仍放不下的截斷並計數", async () => {
  const src = await sourcePdf();
  const long = "word ".repeat(200).trim();
  const r = await buildTranslatedPdf({
    sourceBytes: src,
    segments: [{ page: 0, bbox: [39, 237, 140, 252], kind: "body", size: 10, target: long }],
    fontBytes: new Uint8Array(),
    lang: "zh-TW",
    makeTypeface: helvetica,
  });
  assert.equal(r.placed, 1);
  assert.equal(r.truncated, 1);
  assert.equal(r.shrunk, 1);
});

// ---------- CFF2 → CFF1 ----------

/** 做一個最小的 CFF2 表：兩個字形，第二個用到 blend、子程式與 hintmask。 */
function syntheticCff2() {
  const num = (v) => {
    if (v >= -107 && v <= 107) return [v + 139];
    if (v >= 108 && v <= 1131) return [247 + ((v - 108) >> 8), (v - 108) & 255];
    return [28, (v >> 8) & 255, v & 255];
  };
  const index2 = (items) => {
    const out = [0, 0, 0, items.length];
    if (items.length === 0) return out;
    let t = 1;
    const offs = [1];
    for (const it of items) offs.push((t += it.length));
    out.push(1, ...offs);
    for (const it of items) out.push(...it);
    return out;
  };
  // 字形 0：空。字形 1：
  //   vsindex 的預設是 0、有一個區域：blend 的每個值後面跟一個差量
  //   10 20 hstemhm / 隱含的 vstem 引數 + hintmask（遮罩 1 個位元組）
  //   100(+差量 5) 200(+差量 7) 2 blend rmoveto  →  引數留 100 200
  //   呼叫子程式（-107 偏移的第 0 個）：50 0 rlineto
  const subr = [...num(50), ...num(0), 5, 11]; // 50 0 rlineto return
  const g1 = [
    ...num(10), ...num(20), 18, // hstemhm
    ...num(30), ...num(40), 19, 0x80, // 隱含 vstem + hintmask 與一個位元組的遮罩
    ...num(100), ...num(200), ...num(5), ...num(7), ...num(2), 16, 21, // blend → rmoveto
    ...num(-107), 10, // callsubr（偏置 107 → 第 0 個）
    ...num(0), ...num(-30), 5, // rlineto
  ];
  const cs = index2([[], g1]);
  const gsubrs = index2([]);
  const lsubrs = index2([subr]);
  // 版面：header(5) top dict gsubrs … 全部用 29（五位元組整數）寫偏移，所以長度先知道
  const i32 = (v) => [29, (v >> 24) & 255, (v >> 16) & 255, (v >> 8) & 255, v & 255];
  const buildTop = (csAt, fdaAt, fdsAt, vsAt) => [...i32(csAt), 17, ...i32(fdaAt), 12, 36, ...i32(fdsAt), 12, 37, ...i32(vsAt), 24];
  const top0 = buildTop(0, 0, 0, 0);
  const afterTop = 5 + top0.length;
  const gAt = afterTop; // gsubrs
  const vsAt = gAt + gsubrs.length;
  // VarStore：uint16 長度 + ItemVariationStore(format=1, regionListOffset u32, dataCount u16, dataOffsets u32[1])
  //   + RegionList(axisCount u16, regionCount u16 + 1 個區域各 axisCount*6 位元組) + ItemVariationData(itemCount u16, wordDeltaCount u16, regionIndexCount u16, regionIndexes u16[1])
  const regionList = [0, 1, 0, 1, 0, 0, 0, 0, 0, 0]; // axisCount=1, regionCount=1, 一組座標（不讀）
  const ivdData = [0, 0, 0, 0, 0, 1, 0, 0]; // itemCount=0, wordDeltaCount=0, regionIndexCount=1, regionIndex[0]=0
  const storeHead = 2 + 4 + 2 + 4; // format, regionListOffset, dataCount, dataOffsets[1]
  const regionListOff = storeHead;
  const dataOff = regionListOff + regionList.length;
  const store = [
    0, 1,
    (regionListOff >> 24) & 255, (regionListOff >> 16) & 255, (regionListOff >> 8) & 255, regionListOff & 255,
    0, 1,
    (dataOff >> 24) & 255, (dataOff >> 16) & 255, (dataOff >> 8) & 255, dataOff & 255,
    ...regionList,
    ...ivdData,
  ];
  const vsBlock = [(store.length >> 8) & 255, store.length & 255, ...store];
  const csAt = vsAt + vsBlock.length;
  const fdsAt = csAt + cs.length;
  const fdSelect = [3, 0, 1, 0, 0, 0, 0, 2]; // 格式 3、一個範圍、FD 0、結尾 = 字形數 2
  const fdaAt = fdsAt + fdSelect.length;
  // Private DICT：Subrs（偏移相對 Private 起點）
  const privDictLen = 5 + 1; // i32 + op 19
  const fontDictLen = 5 + 5 + 1; // size offset 18
  const fda = index2([new Array(fontDictLen).fill(0)]);
  const privAt = fdaAt + fda.length;
  const lsubrsAt = privAt + privDictLen;
  const fontDict = [...i32(privDictLen), ...i32(privAt), 18];
  const fda2 = index2([fontDict]);
  assert.equal(fda2.length, fda.length);
  const priv = [...i32(lsubrsAt - privAt), 19];
  const top = buildTop(csAt, fdaAt, fdsAt, vsAt);
  return new Uint8Array([2, 0, 5, 0, top.length, ...top, ...gsubrs, ...vsBlock, ...cs, ...fdSelect, ...fda2, ...priv, ...lsubrs]);
}

test("CFF2 轉 CFF1：blend 只留預設值、子程式展開、提示丟掉、補 endchar", () => {
  const r = cff2ToCff1Subset(syntheticCff2(), [1]);
  assert.equal(r.glyphCount, 2); // .notdef + 一個
  assert.equal(r.cff[0], 1, "CFF1 的版本");
  // 找出字形 1 的字元串流：CharStrings INDEX 在 Top DICT 的 17 號運算子指的位置
  const dv = new DataView(r.cff.buffer, r.cff.byteOffset, r.cff.byteLength);
  const idx = (at) => {
    const count = dv.getUint16(at);
    const offSize = r.cff[at + 2];
    const off = (i) => {
      let v = 0;
      for (let k = 0; k < offSize; k++) v = v * 256 + r.cff[at + 3 + i * offSize + k];
      return v;
    };
    const data = at + 3 + (count + 1) * offSize - 1;
    return { count, item: (i) => r.cff.slice(data + off(i), data + off(i + 1)), end: data + off(count) };
  };
  const nameIdx = idx(4);
  const topIdx = idx(nameIdx.end);
  // 從 Top DICT 讀 CharStrings 偏移（運算子 17，前一個運算元是五位元組整數）
  const top = topIdx.item(0);
  let csAt = -1;
  for (let i = 0; i < top.length; i++) if (top[i] === 17 && top[i - 5] === 29) csAt = (top[i - 4] << 24) | (top[i - 3] << 16) | (top[i - 2] << 8) | top[i - 1];
  const cs = idx(csAt);
  assert.equal(cs.count, 2);
  const g1 = [...cs.item(1)];
  // 期望：100 200 rmoveto(21)、50 0 rlineto(5)、0 -30 rlineto(5)、endchar(14)；沒有提示（1、3、18、19、20）
  const enc = (v) => (v >= -107 && v <= 107 ? [v + 139] : [247 + ((v - 108) >> 8), (v - 108) & 255]);
  assert.deepEqual(g1, [...enc(100), ...enc(200), 21, ...enc(50), ...enc(0), 5, ...enc(0), ...enc(-30), 5, 14]);
  assert.deepEqual([...cs.item(0)], [14]);
});

// ---------- 真的字型（可選） ----------

const FONT = process.env.PR_TEST_CJK_FONT;
test("真的中日韓字型：譯本的文字在 PDF.js 讀得回來", { skip: FONT ? false : "沒有設 PR_TEST_CJK_FONT" }, async () => {
  const src = await sourcePdf();
  const r = await buildTranslatedPdf({
    sourceBytes: src,
    segments: [
      { page: 0, bbox: [39, 237, 300, 262], kind: "body", size: 10, target: "這是一段譯文：模型的準確率提高了。" },
      { page: 0, bbox: [39, 197, 300, 212], kind: "heading", size: 12, target: "標題" },
    ],
    fontBytes: new Uint8Array(fs.readFileSync(FONT)),
    lang: "zh-TW",
  });
  assert.ok(r);
  assert.ok(r.bytes.length < 200_000, `只嵌入用到的字：${r.bytes.length}`);
  const pdf = await open(r.bytes);
  const page = await pdf.getPage(1);
  const text = (await page.getTextContent()).items.map((i) => i.str).join("");
  assert.ok(text.includes("這是一段譯文：模型的準確率提高了。"), text);
  assert.ok(text.includes("標題"), text);
});

test("含行內公式的段落不蓋、不回填，保留原文", async () => {
  const src = await sourcePdf();
  const r = await buildTranslatedPdf({
    sourceBytes: src,
    segments: [
      { ...SEGMENTS[0], source: "where $\\mathcal{L} = \\sum_i x_i$ is the loss" },
      { ...SEGMENTS[1], source: "We set α = 0.05 and the error is ±3% (ordinary prose is replaced)." },
    ],
    fontBytes: new Uint8Array(),
    lang: "zh-TW",
    makeTypeface: helvetica,
  });
  assert.equal(r.placed, 1);
  assert.equal(r.keptMath, 1);
  const meta = await (await open(r.bytes)).getMetadata();
  assert.deepEqual(coversFromInfo(meta.info).pages["0"], [SEGMENTS[1].bbox]);
});

test("公式的碎片（沒有數學符號的分母「X H 2 X H 2 2」）也保留原文", async () => {
  const src = await sourcePdf();
  const r = await buildTranslatedPdf({
    sourceBytes: src,
    segments: [{ ...SEGMENTS[0], source: "X H 2 X H 2 2", target: "X H 2 X H 2 2" }, { ...SEGMENTS[1], source: "Original paragraph two in English" }],
    fontBytes: new Uint8Array(),
    lang: "zh-TW",
    makeTypeface: helvetica,
  });
  assert.equal(r.keptMath, 1);
  assert.equal(r.placed, 1);
});

// ---------- 圖形：運算子清單裡的圖片與路徑 ----------

const O = pdfjs.OPS;

test("運算子清單：追蹤座標變換與裁切，只算畫出來的路徑與圖片，白色填色與裁切用的路徑不算", () => {
  const fn = [];
  const args = [];
  const op = (f, a = []) => {
    fn.push(f);
    args.push(a);
  };
  op(O.save);
  op(O.transform, [1, 0, 0, 1, 100, 200]);
  op(O.constructPath, [O.stroke, [], new Float32Array([0, 0, 50, 0])]); // 橫線：(100,200)–(150,200)
  op(O.setFillRGBColor, ["#ffffff"]);
  op(O.constructPath, [O.fill, [], new Float32Array([0, 0, 300, 300])]); // 白色的底：不算
  op(O.restore);
  op(O.constructPath, [O.endPath, [], new Float32Array([0, 0, 10, 10])]); // 沒有畫：不算
  op(O.save);
  op(O.transform, [20, 0, 0, 10, 300, 400]);
  op(O.paintImageXObject, ["img", 1, 1]); // 圖片：(300,400)–(320,410)
  op(O.restore);
  op(O.save);
  op(O.clip);
  op(O.constructPath, [O.endPath, [], new Float32Array([0, 0, 100, 100])]); // 裁切到 (0,0)–(100,100)
  op(O.constructPath, [O.stroke, [], new Float32Array([50, 50, 200, 50])]); // 被裁成 (50,50)–(100,50)
  op(O.constructPath, [O.fill, [], new Float32Array([150, 150, 180, 180])]); // 整個在裁切範圍外：不算
  op(O.restore);
  op(O.setFillRGBColor, ["#f5e6b3"]);
  op(O.constructPath, [O.fill, [], new Float32Array([400, 400, 500, 450])]); // 有顏色的色塊：也記在 fills
  const g = graphicsFromOps(fn, args, O);
  assert.deepEqual(g.images, [[300, 400, 320, 410]]);
  assert.deepEqual(g.paths, [
    [100, 200, 150, 200],
    [50, 50, 100, 50],
    [400, 400, 500, 450],
  ]);
  assert.deepEqual(g.fills, [[400, 400, 500, 450]]);
});

test("段落碰到圖形：圖片蓋到、長線條伸進來才算；包住整段的底色、短的分數線、只貼著邊的線不算", () => {
  const seg = [100, 100, 300, 130];
  const none = { images: [], paths: [] };
  assert.equal(touchesGraphics(seg, undefined), false);
  assert.equal(touchesGraphics(seg, none), false);
  assert.equal(touchesGraphics(seg, { images: [[90, 90, 200, 140]], paths: [] }), true);
  assert.equal(touchesGraphics(seg, { images: [[295, 125, 310, 140]], paths: [] }), false); // 只碰到一角
  assert.equal(touchesGraphics(seg, { images: [], paths: [[120, 115, 280, 115]] }), true); // 表格線、顯示公式的分數線
  assert.equal(touchesGraphics(seg, { images: [], paths: [[150, 115, 160, 115]] }), false); // 行內公式的短分數線
  assert.equal(touchesGraphics(seg, { images: [], paths: [[0, 0, 600, 800]] }), false); // 包住整段的外框
  assert.equal(touchesGraphics(seg, { images: [], paths: [[50, 50, 400, 200]], fills: [[50, 50, 400, 200]] }), true); // 圖裡的色塊上的字
  assert.equal(touchesGraphics(seg, { images: [], paths: [[100, 130.2, 300, 130.2]] }), false); // 貼著上緣的線
});

test("碰到圖（向量圖形）的段落不蓋、不回填，計入 keptGraphics；不蓋的範圍也不交給刪字", async () => {
  // 原文第 1 頁的「圖」上再畫一條箭頭似的長線，穿過圖裡的字
  const doc = await PDFDocument.load(await sourcePdf());
  doc.getPage(0).drawLine({ start: { x: 50, y: 130 }, end: { x: 230, y: 130 }, thickness: 1 });
  const src = await doc.save();
  const pdf = await open(src);
  const page = await pdf.getPage(1);
  const ops = await page.getOperatorList();
  const graphics = [graphicsFromOps(ops.fnArray, ops.argsArray, pdfjs.OPS)];
  assert.ok(graphics[0].paths.length >= 2, "填色的矩形與一條線");
  // 圖裡的字：範圍落在 (40,100)–(240,160) 的矩形上，有線穿過（整個包住它的矩形本身不算）
  const figText = { page: 0, bbox: [60, 120, 200, 140], kind: "body", size: 10, target: "圖裡的字", source: "Encodes antenna user geometry" };
  const segments = [{ ...SEGMENTS[0], source: "Original paragraph one in English" }, figText];
  assert.deepEqual(coverRects(segments, graphics), { 0: [SEGMENTS[0].bbox] });
  const r = await buildTranslatedPdf({
    sourceBytes: src,
    segments,
    fontBytes: new Uint8Array(),
    lang: "zh-TW",
    makeTypeface: helvetica,
    graphics,
  });
  assert.equal(r.placed, 1);
  assert.equal(r.keptGraphics, 1);
  assert.equal(r.keptMath, 0);
  const meta = await (await open(r.bytes)).getMetadata();
  assert.deepEqual(coversFromInfo(meta.info).pages["0"], [SEGMENTS[0].bbox]);
});
