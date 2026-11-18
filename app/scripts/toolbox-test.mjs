// 閱讀畫面浮動工具箱留在畫面裡的算法（`src/screens/reader/toolboxPlace.ts`）與它依賴的樣式的測試。
// 執行：node --test scripts/toolbox-test.mjs（在 app 底下）
//
// 0.1.3 的錯：面板寬度自動，固定定位時寬度跟著「左緣到畫面右緣」的空間；每次重畫把面板往左夾 4 像素，寬度就多 4 像素，
// 越點越寬。這裡驗兩件事：夾位置的算法再夾一次不會再動；面板與評語卡片的寬度在樣式裡寫明照內容（不是自動）。

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { clampFloat, FLOAT_MARGIN } from "../src/screens/reader/toolboxPlace.ts";

const AREA = { width: 412, height: 915, insetTop: 24, insetBottom: 16 };

test("面板超出右緣：夾回畫面裡，離右緣留邊距", () => {
  const at = clampFloat({ left: 200, top: 300, width: 303, height: 180 }, AREA);
  assert.equal(at.left, 412 - 303 - FLOAT_MARGIN);
  assert.equal(at.top, 300);
});

test("夾過一次的位置再夾一次不動（每次重畫都會夾一次）", () => {
  let box = { left: 200, top: 880, width: 303, height: 180 };
  const first = clampFloat(box, AREA);
  for (let i = 0; i < 20; i++) {
    box = { ...box, ...clampFloat(box, AREA) };
    assert.deepEqual({ left: box.left, top: box.top }, first);
  }
});

test("面板比畫面還大：貼左上，不夾到負的", () => {
  const at = clampFloat({ left: 50, top: 50, width: 600, height: 2000 }, AREA);
  assert.equal(at.left, FLOAT_MARGIN);
  assert.equal(at.top, AREA.insetTop + FLOAT_MARGIN);
});

test("下緣：不蓋到手勢列", () => {
  const at = clampFloat({ left: 10, top: 900, width: 146, height: 387 }, AREA);
  assert.equal(at.top, 915 - 16 - 387 - FLOAT_MARGIN);
});

/** 取 CSS 裡某個選擇器那一段的內容（只認單一選擇器開頭的那一段）。 */
function rule(css, selector) {
  const re = new RegExp(`(^|\\n)${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} \\{([^}]*)\\}`);
  const m = re.exec(css);
  assert.ok(m, `reader.css 裡找不到 ${selector}`);
  return m[2];
}

test("浮動工具箱與評語卡片的寬度照內容，不跟著位置變", () => {
  const css = readFileSync(new URL("../src/ui/reader.css", import.meta.url), "utf8");
  assert.match(rule(css, ".tb-float"), /\n\s*width: max-content;/, ".tb-float 要寫明 width: max-content");
  assert.match(rule(css, ".cm-card"), /\n\s*width: max-content;/, ".cm-card 要寫明 width: max-content");
});
