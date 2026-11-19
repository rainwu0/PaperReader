// 快速翻譯的純計算（`src/screens/reader/quickText.ts`）：前後各一句、要不要給「翻譯」、卡片的位置。
// 執行：node --test scripts/quick-translate-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { consentKey, joinHyphens, mostlyCjk, offersQuick, placeCard, sentenceContext, sentenceEnds } from "../src/screens/reader/quickText.ts";

const PAGE =
  "Deep networks are hard to train. We use residual learning (He et al. 2016) to ease this, as shown in Fig. 3. " +
  "The convolution layers share weights. Results improve by 3.5 points! Future work remains.";

const at = (s) => {
  const i = PAGE.indexOf(s);
  assert.ok(i >= 0, s);
  return [i, i + s.length];
};

test("縮寫與小數點不算句尾", () => {
  const ends = sentenceEnds(PAGE).map((e) => PAGE.slice(0, e).split(" ").at(-1));
  assert.deepEqual(ends, ["train.", "3.", "weights.", "points!", "remains."]);
});

test("選一個詞：前文是這一句在詞前面的部分，後文是這一句剩下的部分", () => {
  const [s, e] = at("convolution");
  assert.deepEqual(sentenceContext(PAGE, s, e), { before: "The", after: "layers share weights." });
});

test("選一整句：前後文是前一句與後一句", () => {
  const [s, e] = at("The convolution layers share weights.");
  const c = sentenceContext(PAGE, s, e);
  assert.equal(c.before, "We use residual learning (He et al. 2016) to ease this, as shown in Fig. 3.");
  assert.equal(c.after, "Results improve by 3.5 points!");
});

test("選到句尾但沒選句點：後文是下一句，不是只有一個句點", () => {
  const [s, e] = at("The convolution layers share weights");
  assert.equal(sentenceContext(PAGE, s, e).after, "Results improve by 3.5 points!");
});

test("頁首與頁尾：沒有前文或後文", () => {
  const [s, e] = at("Deep networks are hard to train.");
  assert.equal(sentenceContext(PAGE, s, e).before, null);
  const [s2, e2] = at("Future work remains.");
  assert.equal(sentenceContext(PAGE, s2, e2).after, null);
});

test("前後文各最多 400 字，前文留靠近選取的那一段", () => {
  const long = "x".repeat(900);
  const text = `${long} word ${long}.`;
  const s = text.indexOf("word");
  const c = sentenceContext(text, s, s + 4);
  assert.equal(c.before?.length, 400);
  assert.ok(c.before?.endsWith("x"));
  assert.equal(c.after?.length, 400);
});

test("選的是中文（譯本 PDF 上翻好的部分）不給翻譯；英文、公式、混一點中文的照給", () => {
  assert.equal(mostlyCjk("我們提出一種新的方法"), true);
  assert.equal(offersQuick("我們提出一種新的方法", "zh-TW"), false);
  assert.equal(offersQuick("ResNet（殘差網路）", "zh-TW"), true);
  assert.equal(offersQuick("convolution", "zh-TW"), true);
  assert.equal(offersQuick("我們", "en"), true);
  assert.equal(offersQuick("  ", "zh-TW"), false);
});

test("卡片放在選取上方；上方放不下放下方；左右夾在畫面裡", () => {
  const vw = 412;
  const vh = 900;
  const above = placeCard({ left: 100, top: 500, right: 200, bottom: 520 }, vw, vh, 50);
  assert.equal(above.above, true);
  assert.equal(above.bottom, vh - 500 + 8);
  assert.equal(above.width, 360);
  assert.ok(above.left >= 12 && above.left + above.width <= vw - 12 + 0.001);
  const below = placeCard({ left: 100, top: 90, right: 200, bottom: 110 }, vw, vh, 50);
  assert.equal(below.above, false);
  assert.equal(below.top, 118);
  // 雙欄論文右欄的邊緣、放大後選取伸出畫面右緣：夾回畫面裡
  const edge = placeCard({ left: 380, top: 500, right: 900, bottom: 520 }, vw, vh, 50);
  assert.equal(edge.left + edge.width, vw - 12);
  // 選一整段：上方放不下估計的高度、下方比較大，放下方
  const para = placeCard({ left: 100, top: 300, right: 380, bottom: 480 }, vw, vh, 70, 700);
  assert.equal(para.above, false);
  assert.ok(para.maxHeight >= 380);
  // 選一個詞：上方放得下就放上方
  assert.equal(placeCard({ left: 100, top: 300, right: 160, bottom: 316 }, vw, vh, 70, 12).above, true);
  const narrow = placeCard({ left: 0, top: 500, right: 40, bottom: 520 }, 320, vh, 50);
  assert.equal(narrow.left, 12);
  assert.equal(narrow.width, 296);
});

test("行尾的連字號接回下一行", () => {
  assert.equal(joinHyphens("employ it- erative methods"), "employ it-erative methods");
  assert.equal(joinHyphens("range - dependent [3]- [4]"), "range - dependent [3]- [4]");
});

test("同意記在服務與主機上", () => {
  assert.equal(consentKey("anthropic", "api.anthropic.com"), "anthropic@api.anthropic.com");
});
