// 帶標註的 PDF 副本（`src/screens/export/annotate.ts`）的測試：先用 pdf-lib 做一份兩頁的 PDF，
// 加上劃線與筆跡，再讀回來確認標註物件、座標、短評都在，原文的位元組沒被改。
// 執行：node --test scripts/export-pdf-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { PDFArray, PDFDict, PDFDocument, PDFHexString, PDFName, PDFNumber, decodePDFRawStream } from "pdf-lib";
import { HIGHLIGHT_COLOR } from "../src/api/types.ts";
import { annotatePdf } from "../src/screens/export/annotate.ts";

async function blankPdf(pages) {
  const doc = await PDFDocument.create();
  for (let i = 0; i < pages; i++) doc.addPage([612, 792]);
  return await doc.save();
}

const highlight = (page, rects, comment = "", color = null) => ({
  id: "h-1",
  kind: "highlight",
  material: "m-1",
  page: { index: page, label: null },
  rects,
  position: null,
  quote: { exact: "x" },
  color,
  created: "",
  modified: "",
  comment,
  complete: true,
});

const stroke = (page, points, extra = {}) => ({
  id: "s-1",
  material: "m-1",
  page,
  color: "#d93025",
  width: 1.8,
  opacity: 1,
  created: "",
  points,
  ...extra,
});

async function annotsOf(bytes, pageIndex) {
  const doc = await PDFDocument.load(bytes);
  const arr = doc.getPage(pageIndex).node.Annots();
  if (!arr) return [];
  return arr.asArray().map((r) => doc.context.lookup(r, PDFDict));
}

const numbers = (arr) => arr.asArray().map((n) => n.asNumber());

test("劃線寫成 Highlight 標註：座標、顏色、短評、外觀串流", async () => {
  const src = await blankPdf(2);
  const out = await annotatePdf(
    src,
    [highlight(1, [[100, 200, 300, 214], [100, 180, 250, 194]], "這段很重要", "#ff0000")],
    [],
  );
  assert.equal(out.highlights, 1);
  assert.equal(out.strokes, 0);
  assert.equal((await annotsOf(out.bytes, 0)).length, 0, "另一頁沒有標註");
  const [a] = await annotsOf(out.bytes, 1);
  assert.equal(a.get(PDFName.of("Subtype")).toString(), "/Highlight");
  assert.deepEqual(numbers(a.get(PDFName.of("Rect"))), [100, 180, 300, 214]);
  // 每一行四個角：左上、右上、左下、右下
  assert.deepEqual(numbers(a.get(PDFName.of("QuadPoints"))), [
    100, 214, 300, 214, 100, 200, 300, 200, 100, 194, 250, 194, 100, 180, 250, 180,
  ]);
  assert.deepEqual(numbers(a.get(PDFName.of("C"))), [1, 0, 0]);
  assert.equal(a.get(PDFName.of("Contents")).decodeText(), "這段很重要");
  assert.ok(a.get(PDFName.of("AP")) instanceof PDFDict, "有外觀串流，沒有外觀的閱讀器也畫得出來");
  assert.ok(a.get(PDFName.of("F")) instanceof PDFNumber);
});

// 沒記顏色的劃線用閱讀畫面同一個黃（`HIGHLIGHT_COLOR`）
test("沒有短評的劃線沒有 Contents；沒記顏色就是畫面上的黃", async () => {
  const out = await annotatePdf(await blankPdf(1), [highlight(0, [[10, 10, 50, 22]])], []);
  const [a] = await annotsOf(out.bytes, 0);
  assert.equal(a.get(PDFName.of("Contents")), undefined);
  const hex = numbers(a.get(PDFName.of("C")))
    .map((v) => Math.round(v * 255).toString(16).padStart(2, "0"))
    .join("");
  assert.equal(`#${hex}`, HIGHLIGHT_COLOR);
});

test("筆跡寫成 Ink 標註：點、線寬、不透明度", async () => {
  const out = await annotatePdf(
    await blankPdf(1),
    [],
    [stroke(0, [[10, 20], [40, 60], [80, 30]], { color: "#f2c300", opacity: 0.45, width: 4 })],
  );
  assert.equal(out.strokes, 1);
  const [a] = await annotsOf(out.bytes, 0);
  assert.equal(a.get(PDFName.of("Subtype")).toString(), "/Ink");
  const list = a.get(PDFName.of("InkList"));
  assert.ok(list instanceof PDFArray);
  assert.deepEqual(numbers(list.get(0)), [10, 20, 40, 60, 80, 30]);
  assert.equal(a.get(PDFName.of("CA")).asNumber(), 0.45);
  assert.equal(a.get(PDFName.of("BS")).get(PDFName.of("W")).asNumber(), 4);
  const rect = numbers(a.get(PDFName.of("Rect")));
  assert.ok(rect[0] < 10 && rect[1] < 20 && rect[2] > 80 && rect[3] > 60, "外框把整筆包進去");
});

test("只有一個點的筆跡也寫得出來", async () => {
  const out = await annotatePdf(await blankPdf(1), [], [stroke(0, [[100, 100]])]);
  assert.equal(out.strokes, 1);
});

test("頁序超出範圍的標註略過；沒有標註時只是重存一份", async () => {
  const out = await annotatePdf(await blankPdf(1), [highlight(5, [[1, 1, 2, 2]])], [stroke(9, [[1, 1]])]);
  assert.equal(out.highlights, 0);
  assert.equal(out.strokes, 0);
  assert.equal((await annotsOf(out.bytes, 0)).length, 0);
});

test("原本就有標註的頁面，新的標註接在後面，舊的不動", async () => {
  const first = await annotatePdf(await blankPdf(1), [highlight(0, [[10, 10, 50, 22]], "舊的")], []);
  const second = await annotatePdf(first.bytes, [highlight(0, [[10, 40, 50, 52]], "新的")], []);
  const annots = await annotsOf(second.bytes, 0);
  assert.equal(annots.length, 2);
  assert.equal(annots[0].get(PDFName.of("Contents")).decodeText(), "舊的");
  assert.ok(annots[1].get(PDFName.of("Contents")) instanceof PDFHexString);
});

test("跨頁的劃線：後半放在下一頁", async () => {
  const h = { ...highlight(0, [[72, 60, 300, 72]]), next_page_rects: [[72, 700, 200, 712]] };
  const out = await annotatePdf(await blankPdf(2), [h], []);
  assert.equal(out.highlights, 1);
  assert.equal((await annotsOf(out.bytes, 0)).length, 1);
  const [b] = await annotsOf(out.bytes, 1);
  assert.deepEqual(numbers(b.get(PDFName.of("Rect"))), [72, 700, 200, 712]);
});

test("列印用：不寫標註物件，劃線與筆跡畫進頁面內容", async () => {
  const out = await annotatePdf(
    await blankPdf(1),
    [highlight(0, [[10, 10, 50, 22]], "短評不印")],
    [stroke(0, [[10, 20], [40, 60]])],
    { flatten: true },
  );
  assert.equal(out.highlights, 1);
  assert.equal(out.strokes, 1);
  assert.equal((await annotsOf(out.bytes, 0)).length, 0, "沒有標註物件");
  const doc = await PDFDocument.load(out.bytes);
  const contents = doc.getPage(0).node.Contents();
  const refs = contents instanceof PDFArray ? contents.asArray() : [contents];
  const text = refs
    .map((r) => doc.context.lookup(r))
    .map((s) => Buffer.from(decodePDFRawStream(s).decode()).toString("latin1"))
    .join("\n");
  assert.ok(/\bf\b/.test(text), "有填色的矩形（劃線）");
  assert.ok(/\bl\b/.test(text) && /\bS\b/.test(text), "有畫折線（筆跡）");
});
