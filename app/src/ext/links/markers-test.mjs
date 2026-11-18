// 引用標記（`markers.ts`）。
// 執行：npm run test:ui（或 node --test src/ext/links/markers-test.mjs，在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { findMarkers, parseNumbers, textLines } from "./markers.ts";

/** 一個文字片段：字串、左下角的 x、基線 y、字級（寬度依字數平均）。 */
function item(str, x, y, size = 10) {
  return { str, transform: [size, 0, 0, size, x, y], width: str.length * size * 0.5, height: size };
}

const entries = Array.from({ length: 30 }, (_, i) => ({ number: i + 1, text: `Author ${i + 1}. Title ${i + 1}. 2020.`, page: 9 }));

test("編號式：[12]、[3, 5–7]、[3,5-7]", () => {
  const items = [item("as shown in [12] and in [3, 5–7] or [3,5-7].", 72, 700)];
  const { numbered } = findMarkers(items, 2, entries);
  assert.deepEqual(
    numbered.map((m) => [m.text, m.numbers]),
    [
      ["[12]", [12]],
      ["[3, 5–7]", [3, 5, 6, 7]],
      ["[3,5-7]", [3, 5, 6, 7]],
    ],
  );
  // 方框是那幾個字的聯集：在那一行上、寬度約四個字
  const b = numbered[0].bbox;
  assert.ok(b[0] > 72 && b[2] > b[0] && b[1] < 700 && b[3] > 700);
  assert.ok(Math.abs(b[2] - b[0] - 4 * 5) < 0.01);
});

test("標記被切在兩個文字片段也認得", () => {
  const items = [item("methods [1", 72, 650), item("4] work", 72 + 10 * 5, 650)];
  const { numbered } = findMarkers(items, 2, entries);
  assert.deepEqual(numbered.map((m) => m.numbers), [[14]]);
});

test("數學區間與集合不算：[0, 1]、x ∈ [1, 2]、[1, 2]^2、[a, b]、[0.5]", () => {
  const items = [
    item("values in [0, 1] and x ∈ [1, 2] or y = [3, 4]", 72, 600),
    item("the square [1, 2]^2 with [a, b] and [0.5] or [-1]", 72, 580),
  ];
  const { numbered } = findMarkers(items, 2, entries);
  assert.deepEqual(numbered, []);
});

test("編號不在清單裡不算；超過清單的範圍不算", () => {
  const items = [item("see [31] and [29–31] but [29, 30]", 72, 500)];
  const { numbered } = findMarkers(items, 2, entries);
  assert.deepEqual(numbered.map((m) => m.text), ["[29, 30]"]);
});

test("參考文獻區自己的編號（一行的開頭、條目起頭的那一頁）不算；別頁一行開頭的照算", () => {
  const items = [item("[7] A. Author. A title. 2020.", 72, 400), item("text [8] here", 72, 380)];
  assert.deepEqual(findMarkers(items, 9, entries).numbered.map((m) => m.text), ["[8]"]);
  assert.deepEqual(findMarkers(items, 2, entries).numbered.map((m) => m.text), ["[7]", "[8]"]);
});

test("欄的中縫隔開兩行：右欄一行的開頭也是開頭", () => {
  const lines = textLines([item("left column text", 72, 300), item("[5] right column", 320, 300)]);
  assert.equal(lines.length, 2);
});

test("編號清單的解析", () => {
  assert.deepEqual(parseNumbers("3, 5–7"), [3, 5, 6, 7]);
  assert.deepEqual(parseNumbers("3;5-7"), [3, 5, 6, 7]);
  assert.equal(parseNumbers("7–5"), null);
  assert.equal(parseNumbers("1–60"), null, "一個範圍最多 50 個");
  assert.equal(parseNumbers("1.5"), null);
  assert.equal(parseNumbers("i"), null);
});

test("作者年份式：只給可能是的候選，最多 3 條", () => {
  const ay = [
    { number: null, text: "Zhao, Y., Lv, W. DETRs beat YOLOs. CVPR, 2024.", page: 9 },
    { number: null, text: "Zhao, Q. Another one. 2023.", page: 9 },
    { number: null, text: "Ba, J. L., Kiros, J. R., and Hinton, G. E. Layer normalization. 2016.", page: 9 },
  ];
  const items = [item("as in (Zhao et al., 2024; Ba and Kiros, 2016) and Zhao (2023) showed", 72, 300)];
  const { numbered, authorYear } = findMarkers(items, 2, ay);
  assert.deepEqual(numbered, []);
  assert.deepEqual(
    authorYear.map((m) => [m.text, m.candidates]),
    [
      ["Zhao et al., 2024", [0]],
      ["Ba and Kiros, 2016", [2]],
      ["Zhao (2023)", [1]],
    ],
  );
});
