// 驗收腳本的主程式：在 Android 模擬器上走「怎麼算成功」的七條主流程（`flows.mjs`），每一步截圖。由 `../walkthrough.ps1` 啟動。
//
// 環境變數（由 walkthrough.ps1 設定）：PR_APK（要安裝的 APK）、PR_SAMPLES（測試用 PDF 的資料夾）、PR_OUT（輸出資料夾）、
// PR_SERIAL（哪一台模擬器，`adb -s`）、PR_DEVTOOLS_PORT（電腦這一端轉接網頁除錯用的連接埠）。
// 參數：--only 1..7（只跑那一條；要先有跑過前面留下的文獻庫）。

import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as L from "./lib.mjs";
import { runFlows } from "./flows.mjs";
import { EXTRA_SAMPLES } from "./make-pdfs.mjs";

const { adb, shell, sleep, PKG } = L;
const OUT = process.env.PR_OUT;
const APK = process.env.PR_APK;
const SAMPLES = process.env.PR_SAMPLES;
const onlyArg = process.argv.includes("--only") ? process.argv[process.argv.indexOf("--only") + 1] : "all";
const only = onlyArg === "all" ? null : Number(onlyArg);
if (!OUT || !APK || !SAMPLES) throw new Error("請用 walkthrough.ps1 啟動（要設定 PR_APK、PR_SAMPLES、PR_OUT）");
if (!L.SERIAL) throw new Error("要指定模擬器（PR_SERIAL）：同一台電腦上常開著好幾台，不指定會打到別人那一台");

const results = [];
function rec(id, ok, note) {
  results.push({ id, ok, note });
  console.log(`${ok ? "通過" : "不通過"}　${id}：${note}`);
}

function shot(name) {
  const file = join(OUT, `${name}.png`);
  // 網頁裡已經有字，不代表畫面已經畫出來；等一下再截
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 700);
  L.screenshot(file);
  return file;
}

// ---------- 裝置準備 ----------

const LIB_SHARED = "/sdcard/Documents/PaperReader";
const DIR = "PR-test";
/** 測試用 PDF：檔名（英文：adb 在 Windows 上傳中文檔名會出問題）→ 來源。都是合成的，不是使用者的論文。 */
const FILES = {
  main: "Paper-A.pdf",
  second: "Outlined-Paper.pdf",
  translation: "Paper-A-translation.pdf",
};

function pushSamples() {
  const stage = mkdtempSync(join(tmpdir(), "pr-samples-"));
  shell(`rm -rf /sdcard/Download/${DIR} && mkdir -p /sdcard/Download/${DIR}`);
  const put = (local, name) => adb(["push", local, `/sdcard/Download/${DIR}/${name}`], { quiet: true });
  for (const [src, dst] of [
    ["合成-30頁.pdf", FILES.main],
    ["合成-5MB.pdf", FILES.translation],
  ]) {
    const local = join(stage, dst);
    copyFileSync(join(SAMPLES, src), local);
    put(local, dst);
  }
  const outlined = join(stage, FILES.second);
  writeFileSync(outlined, EXTRA_SAMPLES[FILES.second]);
  put(outlined, FILES.second);
  // adb 推進去的檔案要通知媒體庫掃描，系統選檔畫面才看得到
  for (const f of Object.values(FILES)) {
    shell(`am broadcast -a android.intent.action.MEDIA_SCANNER_SCAN_FILE -d file:///sdcard/Download/${DIR}/${f}`, {
      allowFail: true,
    });
  }
}

/** 回到剛安裝的狀態：清掉應用的資料、收回權限、刪掉共用空間的文獻庫。只動 PR_SERIAL 那一台。 */
function reset() {
  shell(`am force-stop ${PKG}`, { allowFail: true });
  shell(`pm clear ${PKG}`, { allowFail: true });
  // 授權在「應用」與「使用者代號」兩層各記一份，兩層都要收回
  shell(`appops set --uid ${PKG} MANAGE_EXTERNAL_STORAGE default`, { allowFail: true });
  shell(`appops set ${PKG} MANAGE_EXTERNAL_STORAGE default`, { allowFail: true });
  shell(`rm -rf ${LIB_SHARED} /sdcard/Documents/PaperReader-restored-*`);
  shell("wm user-rotation lock 0", { allowFail: true });
}

const night = (on) => shell(`cmd uimode night ${on ? "yes" : "no"}`);
const back = () => shell("input keyevent KEYCODE_BACK");

async function waitText(text, timeoutMs = 20000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    try {
      if ((await L.pageText()).includes(text)) return true;
    } catch {
      // 頁面還沒準備好
    }
    await sleep(400);
  }
  throw new Error(`網頁上等不到文字：${text}`);
}

async function onSystemScreen(pkgPart, timeoutMs = 20000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    if (L.uiDump().includes(pkgPart)) return true;
    await sleep(700);
  }
  throw new Error(`等不到系統畫面：${pkgPart}`);
}

/**
 * 在系統選檔畫面裡選「下載」底下的一個檔案（`folder` 是下載裡的子資料夾；`null` 表示直接在下載裡）。
 * 選檔畫面每次停的位置不同（最近、文件、下載），所以不假設在哪：看到要選的檔案就點；看到左側位置清單就進「下載」；
 * 看到子資料夾就進去；都沒有就打開位置清單。
 */
async function pickInSystemPicker(name, folder = DIR) {
  await onSystemScreen("documentsui");
  await sleep(1000);
  for (let step = 0; step < 14; step++) {
    const dump = L.uiDump();
    const want = L.findUi(dump, (a) => a.text === name);
    if (want) {
      L.tapXY(want.x, want.y);
      return;
    }
    const drawer = /text="Open from"/.test(dump);
    const downloads = L.findUi(dump, (a) => a.text === "Downloads");
    const sub = folder ? L.findUi(dump, (a) => a.text === folder) : null;
    const roots = L.findUi(dump, (a) => a.desc === "Show roots");
    const hit = drawer && downloads ? downloads : (sub ?? (folder ? null : downloads) ?? roots);
    if (hit) L.tapXY(hit.x, hit.y);
    await sleep(1200);
  }
  throw new Error(`系統選檔畫面找不到 ${name}`);
}

async function pillText() {
  return L.evalWeb(`document.querySelector('.page-pill')?.textContent ?? ''`);
}

async function scrollReaderTo(page, offset = 0.3) {
  await L.evalWeb(`(function(){
    const sc = [...document.querySelectorAll('.reader-scroll')].filter(e => !e.closest('[inert]')).pop();
    const b = sc.querySelector('.pg[data-n="${page}"]');
    sc.scrollTop = b.offsetTop + ${offset} * b.offsetHeight;
  })()`);
}

async function waitReaderRendered(timeoutMs = 20000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const n = await L.evalWeb(`document.querySelectorAll('.reader-scroll canvas').length`).catch(() => 0);
    if (n > 0) return n;
    await sleep(400);
  }
  throw new Error("閱讀畫面沒有畫出頁面");
}

async function main() {
  mkdirSync(OUT, { recursive: true });
  adb(["wait-for-device"]);
  // 螢幕不要在跑到一半時睡著（睡著了截圖是全黑的）
  shell("svc power stayon true", { allowFail: true });
  shell("input keyevent KEYCODE_WAKEUP", { allowFail: true });
  shell("wm dismiss-keyguard", { allowFail: true });
  adb(["install", "-r", APK]);
  if (!only || only === 1) pushSamples();
  const h = {
    rec,
    shot,
    reset,
    night,
    back,
    waitText,
    onSystemScreen,
    pickInSystemPicker,
    pillText,
    scrollReaderTo,
    waitReaderRendered,
    files: FILES,
    titles: { main: "Paper-A", second: "Attention Is All You Need: A Survey" },
    search: { author: "Vaswani", title: "Paper-A", year: "2017", expect: { 作者: "Attention", 篇名: "Paper-A", 年份: "Attention" } },
    comment: "Key point for chapter two",
  };
  // 只跑一條時沒有第 1 條幫忙啟動：剛重新安裝過，應用沒有在執行，先打開它
  if (only) await L.launchApp({ fresh: true });
  await runFlows(h, only);
  let all = results;
  if (only) {
    // 只重跑一條：把這一條的記錄換進原來的結果檔，其餘保留
    try {
      const old = JSON.parse(readFileSync(join(OUT, "results.json"), "utf8"));
      all = [...old.filter((r) => !r.id.startsWith(`${only} `)), ...results].sort((a, b) => a.id.localeCompare(b.id));
    } catch {
      // 沒有舊的結果檔就只存這次的
    }
  }
  writeFileSync(join(OUT, "results.json"), JSON.stringify(all, null, 2));
  const bad = all.filter((r) => r.ok === false);
  console.log(`\n完成：${results.length} 條記錄，${bad.length} 條不通過。`);
  process.exit(bad.length ? 1 : 0);
}

main().catch((e) => {
  console.error("驗收腳本失敗：", e);
  try {
    L.screenshot(join(OUT, "_失敗當下.png"));
  } catch {
    // 忽略
  }
  process.exit(2);
});
