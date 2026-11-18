// 安裝失敗時畫面說的話與「複製錯誤資訊」（`src/screens/update/installReport.ts`）的測試。
// 執行：node --test scripts/update-report-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { headline, note, reportText, statusName, systemLines } from "../src/screens/update/installReport.ts";

const xiaomiAbort = {
  status: 3,
  statusMessage: "INSTALL_FAILED_ABORTED: User rejected permissions",
  legacyStatus: -115,
  sessionId: 7,
  commitAt: 1_000_000,
  confirmAt: 1_000_200,
  at: 1_001_434,
  targetCode: 1_000_300,
  confirmPackage: "com.miui.packageinstaller",
  confirmComponent: "com.miui.packageinstaller/com.android.packageinstaller.PackageInstallerActivity",
  confirmAction: "android.content.pm.action.CONFIRM_INSTALL",
  installedName: "0.1.2.1",
  installedCode: 1_000_201,
  installer: "com.android.packageinstaller",
  initiator: "com.mi.android.globalFileexplorer",
  canRequestInstalls: true,
  device: {
    manufacturer: "Xiaomi",
    brand: "Xiaomi",
    model: "2201122G",
    display: "TKQ1.220807.001 test-keys",
    release: "13",
    sdk: 33,
    incremental: "V14.0.6.0.TLBMIXM",
    miuiVersion: "V140",
    hyperOsVersion: "",
    miuiOptimization: "",
  },
};

test("中止照實說「系統中止了安裝」，不說成你取消了", () => {
  assert.equal(headline("aborted"), "系統中止了安裝。");
  assert.doesNotMatch(headline("aborted"), /取消/);
  assert.match(note("aborted"), /用系統安裝程式開啟/);
  assert.match(headline("cancelled"), /取消/);
  assert.match(headline("invalid"), /無效/);
  assert.match(headline("stalled"), /沒有回報/);
  assert.match(note("stalled"), /用系統安裝程式開啟/);
});

test("小字是系統的原話與代碼", () => {
  assert.deepEqual(systemLines(xiaomiAbort), [
    "系統的原話：INSTALL_FAILED_ABORTED: User rejected permissions",
    "代碼：STATUS_FAILURE_ABORTED（3）",
  ]);
  assert.equal(systemLines({ ...xiaomiAbort, statusMessage: null })[0], "系統沒有附上說明。");
  assert.equal(statusName(99), "STATUS_99");
  assert.equal(statusName(-1), "STATUS_PENDING_USER_ACTION");
});

test("複製的內容有診斷要的每一項", () => {
  const text = reportText(xiaomiAbort, "aborted", "0.1.3");
  for (const want of [
    "PaperReader 更新失敗的錯誤資訊",
    "已安裝：0.1.2.1（1000201）",
    "要裝的：0.1.3（1000300）",
    "結果：STATUS_FAILURE_ABORTED（3），系統中止了安裝。",
    "系統訊息：INSTALL_FAILED_ABORTED: User rejected permissions",
    "舊式代碼：-115",
    "確認畫面：com.miui.packageinstaller，com.miui.packageinstaller/com.android.packageinstaller.PackageInstallerActivity",
    "時間差：確認畫面到結果 1234 毫秒；交給系統到結果 1434 毫秒",
    "裝置：Xiaomi 2201122G（Xiaomi）",
    "Android：13（API 33）",
    "MIUI：V140；HyperOS：（無）",
    "MIUI 最佳化：開（persist.sys.miui_optimization=）",
    "安裝者紀錄：com.android.packageinstaller（由 com.mi.android.globalFileexplorer 發起）",
    "允許安裝不明應用程式：是",
  ]) {
    assert.ok(text.includes(want), `少了「${want}」：\n${text}`);
  }
});

test("讀不到的欄位寫「未知」或「無」，不會整段出錯", () => {
  const bare = { status: 4, at: 5, device: { manufacturer: "Google", brand: "google", model: "Pixel", display: "", release: "16", sdk: 36, incremental: "" } };
  const text = reportText(bare, "invalid", null);
  assert.match(text, /系統訊息：（無）/);
  assert.match(text, /舊式代碼：（無）/);
  assert.match(text, /確認畫面：（無）/);
  assert.match(text, /時間差：確認畫面到結果 未知；交給系統到結果 未知/);
  assert.match(text, /MIUI：未知；HyperOS：未知/);
  assert.match(text, /MIUI 最佳化：不是小米的系統/);
  // 小米但讀不到屬性
  const unread = reportText({ ...xiaomiAbort, device: { ...xiaomiAbort.device, miuiOptimization: null } }, "aborted", "0.1.3");
  assert.match(unread, /MIUI 最佳化：未知/);
  assert.match(text, /要裝的：未知/);
  const off = reportText({ ...xiaomiAbort, device: { ...xiaomiAbort.device, miuiOptimization: "false" } }, "aborted", "0.1.3");
  assert.match(off, /MIUI 最佳化：關（persist\.sys\.miui_optimization=false）/);
});
