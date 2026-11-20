// 兩份頁面幾何相同的 PDF 之間「同一處」的換算（`src/screens/reader/viewSync.ts`）的測試：
// 原文與譯本切換、中英對照兩格同步共用。
// 執行：node --test scripts/viewsync-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  clampSplit,
  isEcho,
  pageIndexAt,
  scrollFor,
  splitAt,
  switchAt,
  SPLIT_DEFAULT,
  SPLIT_MAX,
  SPLIT_MIN,
} from "../src/screens/reader/viewSync.ts";

/** 照 `pdfView.ts` 的排法排出頁框：邊距與頁間距 8 × 縮放，頁高 = 頁寬 × 高寬比。 */
function pages(n, { width, ratio = 842 / 595, zoom = 1 }) {
  const pad = 8 * zoom;
  const w = (width - 2 * 8) * zoom;
  const h = w * ratio;
  return Array.from({ length: n }, (_, i) => ({ top: pad + i * (h + pad), height: h }));
}
/** 內容的寬度：頁寬加兩側邊距。 */
const contentWidth = (width, zoom) => (width - 2 * 8) * zoom + 2 * 8 * zoom;

test("同一份位置在兩格之間來回：頁、頁內比例、水平比例、縮放都不變", () => {
  const zoom = 2.4;
  // 上格 412 × 380、下格 412 × 420（直拿上下分，兩格高度不同）
  const top = pages(12, { width: 412, zoom });
  const p = switchAt(top, { top: 5321, left: 210, height: 380, width: 412, scrollWidth: contentWidth(412, zoom) }, zoom);
  const bottom = pages(12, { width: 412, zoom });
  const to = scrollFor(bottom, p, { height: 420, width: 412, scrollWidth: contentWidth(412, zoom) });
  const back = switchAt(bottom, { top: to.top, left: to.left, height: 420, width: 412, scrollWidth: contentWidth(412, zoom) }, zoom);
  assert.equal(back.page, p.page);
  assert.ok(Math.abs(back.mid - p.mid) < 1e-9);
  assert.ok(Math.abs(back.hx - p.hx) < 1e-9);
  assert.equal(back.zoom, zoom);
  assert.equal(to.clamped, false);
});

test("兩格高度不同：畫面中線落在同一頁的同一個比例", () => {
  const a = pages(5, { width: 400 });
  const p = switchAt(a, { top: 1000, left: 0, height: 600, width: 400, scrollWidth: 400 }, 1);
  const b = pages(5, { width: 400 });
  const to = scrollFor(b, p, { height: 300, width: 400, scrollWidth: 400 });
  // 中線（捲到的位置加半個畫面高）落在同一頁的同一個比例
  const midA = 1000 + 300;
  const midB = to.top + 150;
  const i = pageIndexAt(a, midA);
  assert.equal(pageIndexAt(b, midB), i);
  assert.ok(Math.abs((midA - a[i].top) / a[i].height - (midB - b[i].top) / b[i].height) < 1e-9);
});

test("譯本 PDF 的頁面大小和原文不同：照頁與比例對齊", () => {
  const letter = pages(8, { width: 412, ratio: 11 / 8.5 });
  const a4 = pages(8, { width: 412, ratio: 842 / 595 });
  const p = switchAt(letter, { top: 2000, left: 0, height: 500, width: 412, scrollWidth: 412 }, 1);
  const to = scrollFor(a4, p, { height: 500, width: 412, scrollWidth: 412 });
  const q = switchAt(a4, { top: to.top, left: 0, height: 500, width: 412, scrollWidth: 412 }, 1);
  assert.equal(q.page, p.page);
  assert.ok(Math.abs(q.mid - p.mid) < 1e-9);
});

test("左右分、兩格寬度不同：放大到欄寬時左緣落在頁寬的同一個比例（同一欄）", () => {
  const zoom = 2.2;
  const wa = 500;
  const wb = 300;
  const a = pages(4, { width: wa, zoom });
  const p = switchAt(a, { top: 400, left: 380, height: 350, width: wa, scrollWidth: contentWidth(wa, zoom) }, zoom);
  const b = pages(4, { width: wb, zoom });
  const to = scrollFor(b, p, { height: 350, width: wb, scrollWidth: contentWidth(wb, zoom) });
  // 畫面左緣在頁寬的幾分之幾（扣掉左邊距）
  const frac = (left, w) => (left - 8 * zoom) / ((w - 16) * zoom);
  assert.ok(Math.abs(frac(380, wa) - frac(to.left, wb)) < 0.01, `${frac(380, wa)} vs ${frac(to.left, wb)}`);
});

test("沒有放大（不能左右捲）：水平比例是 0，設回去也是 0", () => {
  const a = pages(3, { width: 412 });
  const p = switchAt(a, { top: 0, left: 0, height: 800, width: 412, scrollWidth: 412 }, 1);
  assert.equal(p.hx, 0);
  assert.equal(scrollFor(a, { ...p, hx: 0.7 }, { height: 800, width: 412, scrollWidth: 412 }).left, 0);
});

test("頁序超出這份 PDF 的頁數：停在最後一頁的中間，標出夾過", () => {
  const a = pages(3, { width: 412 });
  const to = scrollFor(a, { page: 9, mid: 0.1, hx: 0, zoom: 1 }, { height: 600, width: 412, scrollWidth: 412 });
  assert.equal(to.clamped, true);
  const last = a[2];
  assert.ok(Math.abs(to.top - (last.top + last.height / 2 - 300)) < 1e-9);
});

test("比例超出 0 到 1 時夾住", () => {
  const a = pages(3, { width: 412 });
  const lo = scrollFor(a, { page: 2, mid: -1, hx: -3, zoom: 1 }, { height: 600, width: 412, scrollWidth: 900 });
  assert.equal(lo.top, a[1].top - 300);
  assert.equal(lo.left, 0);
  const hi = scrollFor(a, { page: 2, mid: 5, hx: 3, zoom: 1 }, { height: 600, width: 412, scrollWidth: 900 });
  assert.equal(hi.top, a[1].top + a[1].height - 300);
  assert.equal(hi.left, 900 - 412);
});

test("沒有頁：位置是第 1 頁開頭，捲到 0", () => {
  assert.deepEqual(switchAt([], { top: 0, left: 0, height: 100, width: 100, scrollWidth: 100 }, 1), {
    page: 1,
    mid: 0,
    hx: 0,
    zoom: 1,
  });
  assert.equal(scrollFor([], { page: 1, mid: 0.5, hx: 0, zoom: 1 }, { height: 100, width: 100, scrollWidth: 100 }).top, 0);
});

test("回聲：位置還在剛才被設的地方（容許取整誤差）才算；縮放不同、捲動差得多都不算", () => {
  const mark = { top: 1000, left: 40, zoom: 2 };
  assert.equal(isEcho(mark, { top: 1001.5, left: 39, zoom: 2 }), true);
  assert.equal(isEcho(mark, { top: 1010, left: 40, zoom: 2 }), false);
  assert.equal(isEcho(mark, { top: 1000, left: 40, zoom: 2.5 }), false);
  assert.equal(isEcho(null, { top: 1000, left: 40, zoom: 2 }), false);
});

test("分隔線：譯本那一格的比例照拖到的位置算，夾在兩格都讀得到的範圍", () => {
  // 兩格合起來從 y = 24 到 y = 24 + 800
  assert.equal(splitAt(424, 24, 800), 0.5);
  assert.equal(splitAt(224, 24, 800), 0.75);
  assert.equal(splitAt(30, 24, 800), SPLIT_MAX);
  assert.equal(splitAt(820, 24, 800), SPLIT_MIN);
  assert.equal(splitAt(100, 24, 0), SPLIT_DEFAULT);
  assert.equal(clampSplit(Number.NaN), SPLIT_DEFAULT);
  assert.equal(clampSplit(0.4), 0.4);
});
