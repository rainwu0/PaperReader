// 文字層空隙的墊片：哪些瀏覽器要、拖的是哪一端（`src/screens/reader/textLayerGap.ts`）。
// 執行：node --test scripts/textlayergap-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { movingEnd, needsGapFix } from "../src/screens/reader/textLayerGap.ts";

const webview = (v) =>
  `Mozilla/5.0 (Linux; Android 14; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/${v} Mobile Safari/537.36`;

test("Chromium 148 之前要墊片，148 起不要", () => {
  assert.equal(needsGapFix(webview("133.0.6943.137")), true);
  assert.equal(needsGapFix(webview("147.0.0.0")), true);
  assert.equal(needsGapFix(webview("148.0.0.0")), false);
  assert.equal(needsGapFix(webview("150.0.7000.1")), false);
});

test("有 userAgentData 時以它的 Chromium 版本為準", () => {
  assert.equal(needsGapFix(webview("150.0.0.0"), [{ brand: "Chromium", version: "140" }]), true);
  assert.equal(needsGapFix(webview("133.0.0.0"), [{ brand: "Not A Brand", version: "8" }, { brand: "Chromium", version: "148" }]), false);
});

test("不是 Chromium 的瀏覽器不要墊片", () => {
  assert.equal(needsGapFix("Mozilla/5.0 (Android 14; Mobile; rv:124.0) Gecko/124.0 Firefox/124.0"), false);
  assert.equal(needsGapFix(""), false);
});

test("起點不變、終點變了：在拖終點；終點不變、起點變了：在拖起點", () => {
  assert.equal(movingEnd(true, false, false), "end");
  assert.equal(movingEnd(false, true, false), "start");
  // 方向不影響這兩種
  assert.equal(movingEnd(true, false, true), "end");
  assert.equal(movingEnd(false, true, true), "start");
});

test("兩端都變了或沒有上一次可比：看選取的方向", () => {
  assert.equal(movingEnd(false, false, false), "end");
  assert.equal(movingEnd(false, false, true), "start");
  assert.equal(movingEnd(true, true, true), "start");
});
