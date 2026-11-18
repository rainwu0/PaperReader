// 模擬器驗收用的小工具：adb、uiautomator（操作系統的畫面）、Chrome 開發者協定（操作應用裡的網頁）。
// 只用 Node 內建的功能，沒有外部套件。

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

export const PKG = "io.github.rainwu0.paperreader";
/** adb 的位置：`ADB`，否則 `ANDROID_HOME` 底下的那一份，都沒設就用 PATH 上的 `adb`。 */
export const ADB =
  process.env.ADB ??
  (process.env.ANDROID_HOME ? `${process.env.ANDROID_HOME}\\platform-tools\\adb.exe` : "adb");
/**
 * 要操作哪一台（`adb -s`）。同一台電腦常同時開著幾台模擬器（各自操作的），
 * 不指定時 adb 會拒絕或打到別人那一台；`PR_SERIAL` 沒設時沿用 adb 自己認的 `ANDROID_SERIAL`。
 */
export const SERIAL = process.env.PR_SERIAL ?? process.env.ANDROID_SERIAL ?? "";
const target = () => (SERIAL ? ["-s", SERIAL] : []);

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function adb(args, { quiet = false, allowFail = false } = {}) {
  try {
    return execFileSync(ADB, [...target(), ...args], { encoding: "utf8", maxBuffer: 1 << 28, stdio: ["ignore", "pipe", "pipe"] });
  } catch (e) {
    if (allowFail) return String(e.stdout ?? "") + String(e.stderr ?? "");
    if (!quiet) console.error(`adb ${args.join(" ")} 失敗：${e.stderr ?? e.message}`);
    throw e;
  }
}

export function shell(cmd, opts) {
  return adb(["shell", cmd], opts);
}

/** 截圖存成 PNG（adb exec-out 直接交回二進位，不經換行轉換）。 */
export function screenshot(file) {
  const buf = execFileSync(ADB, [...target(), "exec-out", "screencap", "-p"], { maxBuffer: 1 << 28 });
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, buf);
}

// ---------- uiautomator：系統的畫面（授權頁、選檔畫面） ----------

export function uiDump() {
  // uiautomator 偶爾被系統結束（結束碼 137），重試幾次
  for (let i = 0; i < 4; i++) {
    const out = adb(["shell", "uiautomator", "dump", "/sdcard/pr-ui.xml"], { quiet: true, allowFail: true });
    if (/dumped to/.test(out)) {
      const xml = adb(["exec-out", "cat", "/sdcard/pr-ui.xml"], { quiet: true, allowFail: true });
      if (xml.includes("<hierarchy")) return xml;
    }
    execFileSync(process.execPath, ["-e", "setTimeout(()=>{},1200)"]);
  }
  return "";
}

function attr(node, name) {
  const m = new RegExp(`${name}="([^"]*)"`).exec(node);
  return m ? m[1].replace(/&amp;/g, "&").replace(/&quot;/g, '"').replace(/&lt;/g, "<").replace(/&gt;/g, ">") : "";
}

/** 在 uiautomator 的輸出裡找符合條件的節點，交回中心點。`match` 是 (屬性表) => 布林。 */
export function findUi(xml, match) {
  const nodes = xml.match(/<node [^>]*>/g) ?? [];
  for (const n of nodes) {
    const a = {
      text: attr(n, "text"),
      desc: attr(n, "content-desc"),
      id: attr(n, "resource-id"),
      cls: attr(n, "class"),
      clickable: attr(n, "clickable") === "true",
      checked: attr(n, "checked") === "true",
    };
    if (!match(a)) continue;
    const b = /\[(\d+),(\d+)\]\[(\d+),(\d+)\]/.exec(attr(n, "bounds"));
    if (!b) continue;
    const [x1, y1, x2, y2] = b.slice(1).map(Number);
    return { x: Math.round((x1 + x2) / 2), y: Math.round((y1 + y2) / 2), attrs: a };
  }
  return null;
}

export function tapXY(x, y) {
  shell(`input tap ${x} ${y}`);
}

/** 等到系統畫面出現符合條件的節點，點下去。 */
export async function tapUi(match, { timeoutMs = 15000, label = "" } = {}) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const hit = findUi(uiDump(), match);
    if (hit) {
      tapXY(hit.x, hit.y);
      return hit;
    }
    await sleep(700);
  }
  throw new Error(`系統畫面上找不到：${label}`);
}

export async function waitUi(match, { timeoutMs = 15000, label = "" } = {}) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const hit = findUi(uiDump(), match);
    if (hit) return hit;
    await sleep(700);
  }
  throw new Error(`系統畫面上等不到：${label}`);
}

// ---------- Chrome 開發者協定：應用裡的網頁 ----------

// 電腦這一端轉接的連接埠是整台電腦共用的：幾台模擬器同時跑驗收時各用各的（`PR_DEVTOOLS_PORT`），免得互相搶走
const port = Number(process.env.PR_DEVTOOLS_PORT ?? 9222);

export function forwardDevtools() {
  const pid = shell(`pidof ${PKG}`, { allowFail: true }).trim().split(/\s+/)[0];
  if (!pid) throw new Error("應用沒有在執行");
  adb(["forward", `tcp:${port}`, `localabstract:webview_devtools_remote_${pid}`]);
  return pid;
}

async function connect() {
  const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  const page = list.find((t) => t.type === "page");
  if (!page) throw new Error("找不到 WebView 頁面");
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((res, rej) => {
    ws.onopen = res;
    ws.onerror = rej;
  });
  let id = 0;
  const pending = new Map();
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.id && pending.has(d.id)) {
      pending.get(d.id)(d);
      pending.delete(d.id);
    }
  };
  const send = (method, params = {}) =>
    new Promise((res, rej) => {
      const i = ++id;
      pending.set(i, (d) => (d.error ? rej(new Error(JSON.stringify(d.error))) : res(d.result)));
      ws.send(JSON.stringify({ id: i, method, params }));
    });
  return { send, close: () => ws.close() };
}

/** 在網頁裡執行一段運算式，交回值（支援 await）。 */
export async function evalWeb(expression) {
  const c = await connect();
  try {
    const r = await c.send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    if (r.exceptionDetails) throw new Error(`網頁執行失敗：${JSON.stringify(r.exceptionDetails.exception?.description ?? r.exceptionDetails)}`);
    return r.result.value;
  } finally {
    c.close();
  }
}

// 在網頁裡找元素的小工具（跳過被 inert 蓋住的畫面）。選擇器：
//   aria=返回    aria-label 完全相同
//   btn=加入第一篇文獻    按鈕文字完全相同
//   btnc=回收區          按鈕文字含有這段（設定裡進下一頁的列，尾巴有 ›）
//   text=...     任何元素的文字完全相同（取最小的那個）
//   css=...      CSS 選擇器
const FINDER = `
(function(sel){
  const live = (e) => e && !e.closest('[inert]') && e.getClientRects().length > 0;
  const i = sel.indexOf('=');
  const kind = sel.slice(0, i), val = sel.slice(i + 1);
  let list = [];
  if (kind === 'aria') list = [...document.querySelectorAll('[aria-label]')].filter(e => e.getAttribute('aria-label') === val);
  else if (kind === 'btnc') list = [...document.querySelectorAll('button')].filter(e => e.textContent.includes(val));
  else if (kind === 'btn') list = [...document.querySelectorAll('button')].filter(e => e.textContent.trim() === val);
  else if (kind === 'text') list = [...document.querySelectorAll('body *')].filter(e => e.children.length === 0 && e.textContent.trim() === val);
  else if (kind === 'css') list = [...document.querySelectorAll(val)];
  return list.find(live) || null;
})`;

export async function waitWeb(selector, { timeoutMs = 15000 } = {}) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    try {
      const found = await evalWeb(`!!${FINDER}(${JSON.stringify(selector)})`);
      if (found) return true;
    } catch {
      // 頁面還沒準備好
    }
    await sleep(400);
  }
  throw new Error(`網頁上等不到：${selector}`);
}

/** 對網頁元素送出一次真正的觸控點擊（經瀏覽器的輸入管線，和手指走同一條路）。 */
export async function tapWeb(selector, { timeoutMs = 15000 } = {}) {
  await waitWeb(selector, { timeoutMs });
  const rect = await evalWeb(`(function(){
    const el = ${FINDER}(${JSON.stringify(selector)});
    el.scrollIntoView({ block: 'center' });
    const r = el.getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2, w: r.width, h: r.height };
  })()`);
  const c = await connect();
  try {
    await c.send("Input.synthesizeTapGesture", { x: rect.x, y: rect.y, gestureSourceType: "touch" });
  } finally {
    c.close();
  }
  return rect;
}

export async function pinchWeb(x, y, scaleFactor) {
  const c = await connect();
  try {
    await c.send("Input.synthesizePinchGesture", { x, y, scaleFactor, relativeSpeed: 400, gestureSourceType: "touch" });
  } finally {
    c.close();
  }
}

export async function pageText() {
  return evalWeb(`(function(){
    const tops = [...document.querySelectorAll('#root > div, body > div')].filter(e => !e.closest('[inert]'));
    return document.body.innerText;
  })()`);
}

/** 重新啟動應用（強制結束再開）。 */
export async function launchApp({ fresh = false } = {}) {
  if (fresh) adb(["shell", "am", "force-stop", PKG], { quiet: true });
  adb(["shell", "monkey", "-p", PKG, "-c", "android.intent.category.LAUNCHER", "1"], { quiet: true });
  await sleep(2500);
  // 模擬器忙的時候啟動要好幾秒：等到網頁真的接得上為止（最多 40 秒）
  let lastError = null;
  for (let i = 0; i < 40; i++) {
    try {
      forwardDevtools();
      const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      if (list.some((t) => t.type === "page")) return;
    } catch (e) {
      lastError = e;
    }
    await sleep(1000);
  }
  throw lastError ?? new Error("應用的網頁沒有起來");
}

// ---------- 截圖的像素（驗「畫面真的畫成什麼顏色」用） ----------

import { readFileSync } from "node:fs";
import { inflateSync } from "node:zlib";

/**
 * 讀 PNG（8 位元、RGB 或 RGBA、不交錯；adb screencap 交出的就是這種）某一點的顏色，交回 [r, g, b]。
 * 沒有圖片套件可用，所以自己解：zlib 解壓縮、還原每一列的濾波。
 */
export function pngPixel(file, x, y) {
  const buf = readFileSync(file);
  let pos = 8;
  let width = 0;
  let height = 0;
  let channels = 4;
  const idat = [];
  while (pos < buf.length) {
    const len = buf.readUInt32BE(pos);
    const type = buf.toString("ascii", pos + 4, pos + 8);
    const body = buf.subarray(pos + 8, pos + 8 + len);
    if (type === "IHDR") {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      const colorType = body[9];
      channels = colorType === 6 ? 4 : colorType === 2 ? 3 : 0;
      if (body[8] !== 8 || channels === 0 || body[12] !== 0) throw new Error("不支援的 PNG 格式");
    } else if (type === "IDAT") idat.push(body);
    pos += 12 + len;
  }
  if (x < 0 || y < 0 || x >= width || y >= height) throw new Error(`座標超出圖片：${x},${y}（${width}×${height}）`);
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const rows = [];
  let prev = Buffer.alloc(stride);
  for (let r = 0; r <= y; r++) {
    const off = r * (stride + 1);
    const filter = raw[off];
    const line = Buffer.from(raw.subarray(off + 1, off + 1 + stride));
    for (let i = 0; i < stride; i++) {
      const a = i >= channels ? line[i - channels] : 0;
      const b = prev[i];
      const c = i >= channels ? prev[i - channels] : 0;
      let add = 0;
      if (filter === 1) add = a;
      else if (filter === 2) add = b;
      else if (filter === 3) add = (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a);
        const pb = Math.abs(p - b);
        const pc = Math.abs(p - c);
        add = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      line[i] = (line[i] + add) & 255;
    }
    rows.push(line);
    prev = line;
  }
  const line = rows[y];
  return [line[x * channels], line[x * channels + 1], line[x * channels + 2]];
}

/** 兩個 [r, g, b] 的對比（WCAG 的算法）。 */
export function contrast(a, b) {
  const lum = ([r, g, bl]) => {
    const f = (v) => {
      v /= 255;
      return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
    };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(bl);
  };
  const x = lum(a);
  const y = lum(b);
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}

/** 在網頁上用觸控畫一條線（Chrome 開發者協定送出觸控事件；座標是網頁的 CSS 像素）。 */
export async function dragWeb(points, { stepMs = 30 } = {}) {
  const c = await connect();
  try {
    const [first, ...rest] = points;
    await c.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: first.x, y: first.y }] });
    for (const p of rest) {
      await sleep(stepMs);
      await c.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: p.x, y: p.y }] });
    }
    await sleep(stepMs);
    await c.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  } finally {
    c.close();
  }
}

/** 對一個座標送一次觸控點擊。 */
export async function tapAt(x, y) {
  const c = await connect();
  try {
    await c.send("Input.synthesizeTapGesture", { x, y, gestureSourceType: "touch" });
  } finally {
    c.close();
  }
}

// ---------- 真的觸控：adb shell input（手指在螢幕上的座標，不經網頁的事件管線） ----------

/** 網頁元素在螢幕上的位置（裝置像素）。網頁佔滿整個螢幕，所以 CSS 像素乘上 devicePixelRatio 就是螢幕座標。 */
export async function screenRect(selector, { timeoutMs = 15000 } = {}) {
  await waitWeb(selector, { timeoutMs });
  return evalWeb(`(function(){
    const el = ${FINDER}(${JSON.stringify(selector)});
    el.scrollIntoView({ block: 'center' });
    const r = el.getBoundingClientRect();
    const k = window.devicePixelRatio;
    return { x: Math.round((r.left + r.width / 2) * k), y: Math.round((r.top + r.height / 2) * k), w: r.width * k, h: r.height * k };
  })()`);
}

/** 用 `adb shell input tap` 點網頁元素（手指真的點在螢幕上）。 */
export async function tapReal(selector, opts) {
  await screenRect(selector, opts);
  await sleep(250); // 捲動到中間後等畫面停下來
  const r2 = await screenRect(selector, opts);
  shell(`input tap ${r2.x} ${r2.y}`);
  return r2;
}

let longPressTimeout = null;

/**
 * 這台裝置的長按時限（無障礙設定的「輕觸並按住的延遲時間」：短 400、中 1000、長 1500 毫秒）。
 * 按得比它短，系統不當成長按，網頁不會選字。
 */
export function systemLongPressMs() {
  if (longPressTimeout === null) {
    const v = Number(shell("settings get secure long_press_timeout", { allowFail: true }).trim());
    longPressTimeout = Number.isFinite(v) && v > 0 ? v : 400;
  }
  return longPressTimeout;
}

/**
 * 長按網頁元素（`adb shell input swipe x y x y 毫秒`）。至少按到系統的長按時限再多 0.6 秒：
 * 時限調成「中」「長」的模擬器上，固定按 0.9 秒不算長按（驗收時等不到「短評」按鈕就是這樣）。
 */
export async function longPressReal(selector, ms = 800, opts) {
  await screenRect(selector, opts);
  await sleep(250);
  const r = await screenRect(selector, opts);
  shell(`input swipe ${r.x} ${r.y} ${r.x} ${r.y} ${Math.max(ms, systemLongPressMs() + 600)}`);
  return r;
}
