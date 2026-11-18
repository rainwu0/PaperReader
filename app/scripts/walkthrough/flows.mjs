// 「怎麼算成功」七件事，各一條主流程，在 Android 模擬器上從頭走到尾。
//
// 寫法：
//   - 點按一律是真的觸控：`adb shell input tap/swipe`（`L.tapReal`），打字用 `adb shell input text`。
//   - 找元件用的文字一律從字串表取（`T("字串鍵")`），不寫死畫面上的字；結構用畫面元件的 class 或 aria-label。
//   - 系統的畫面（授權頁、選檔畫面、存檔畫面）是 Android 的英文介面，用它們的文字找。
//   - 七條依序跑，後面的接著前面留下的文獻庫（第 1 條從剛安裝的狀態開始）。`--only 5` 只跑第 5 條，要先有跑過前面留下的文獻庫。
//   - 每條結束時記一筆「通過／不通過」與看到的內容；中途出錯就記成不通過、截下當時的畫面，接著跑下一條。

import * as L from "./lib.mjs";
import { prefix, T } from "./strings.mjs";

const { sleep, shell } = L;
const LIB = "/sdcard/Documents/PaperReader";
const sh = (cmd) => shell(cmd, { allowFail: true }).trim();

/** 呼叫應用的指令讀狀態（只用來核對結果，不拿來操作）。 */
const invoke = (cmd, args = {}) =>
  L.evalWeb(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)}).then(v => ({ ok: true, v }), e => ({ ok: false, e }))`,
  ).then((r) => {
    if (!r.ok) throw new Error(`${cmd} 失敗：${JSON.stringify(r.e)}`);
    return r.v;
  });

/** 首頁清單裡，篇名含 `title` 的那一列的文字。 */
const rowText = (title) =>
  L.evalWeb(
    `([...document.querySelectorAll('.paper-row')].filter(e => !e.closest('[inert]')).find(e => e.textContent.includes(${JSON.stringify(title)}))?.innerText ?? '')`,
  );

/** 首頁清單上看得到的篇名。 */
const listedTitles = () =>
  L.evalWeb(`[...document.querySelectorAll('.paper-title')].filter(e => !e.closest('[inert]')).map(e => e.textContent)`);

/** 用真的觸控點首頁清單裡篇名含 `title` 的那一列。 */
async function tapRow(h, title) {
  const idx = await L.evalWeb(
    `[...document.querySelectorAll('.paper-list > li')].filter(e => !e.closest('[inert]')).findIndex(e => e.textContent.includes(${JSON.stringify(title)}))`,
  );
  if (idx < 0) throw new Error(`首頁清單裡找不到：${title}`);
  await L.tapReal(`css=.paper-list > li:nth-child(${idx + 1}) .paper-main`);
}

/** 打開某一篇來讀，等頁面畫出來。 */
async function openPaper(h, title) {
  await tapRow(h, title);
  await h.waitReaderRendered();
  await sleep(1200);
}

/** 閱讀畫面：頂列收起來時點一下頁面叫出來。 */
async function showReaderBar() {
  const shown = await L.evalWeb(`!!document.querySelector('.reader-bar')`);
  if (!shown) {
    await L.tapReal("css=.reader-scroll");
    await sleep(600);
  }
}

/** 一路按返回，直到回到首頁（有清單或空的首頁）。 */
async function goHome(h) {
  for (let i = 0; i < 8; i++) {
    const home = await L.evalWeb(
      `(function(){const top=[...document.querySelectorAll('.layer')].filter(e=>!e.inert).pop();return !!top && !!top.querySelector('.home-inner, .home-empty') && !document.querySelector('.sheet')})()`,
    ).catch(() => false);
    if (home) return;
    h.back();
    await sleep(900);
  }
  throw new Error("回不到首頁");
}

/** 打開側邊欄，點其中一項。 */
async function sidebar(label) {
  await L.tapReal(`aria=${T("home.menu")}`);
  await L.waitWeb("css=.drawer");
  await sleep(700);
  await L.tapReal(`btnc=${label}`);
}

/** 用模擬器的鍵盤打字（只能打英數字；空白要換成 %s）。 */
function typeText(text) {
  shell(`input text '${text.replace(/ /g, "%s")}'`);
}

/** 系統的存檔畫面（「存到…」）按「SAVE」。 */
async function saveInDocumentsUi(h) {
  await h.onSystemScreen("documentsui");
  await sleep(1500);
  const save = await L.waitUi((a) => /^save$/i.test(a.text) || /^save$/i.test(a.desc), { label: "存檔鈕" });
  L.tapXY(save.x, save.y);
}

// ---------- 七條主流程 ----------

/** 1. 把一篇剛拿到的論文收進來：第一次打開 → 加入 → 說明並要求權限 → 系統選檔 → 出現在目錄。 */
async function flow1(h) {
  h.reset();
  h.night(false);
  await L.launchApp({ fresh: true });
  await h.waitText(T("home.empty.title"));
  h.shot("1-首頁是空的");
  await L.tapReal(`btn=${T("home.empty.action")}`);
  await h.waitText(T("add.intro.title"));
  h.shot("1-說明文獻庫放哪裡");
  await L.tapReal(`btn=${T("home.lostAccess.action")}`);
  await h.onSystemScreen("com.android.settings");
  await sleep(1200);
  const sw = await L.waitUi((a) => a.text === "Allow access to manage all files", { label: "授權開關" });
  L.tapXY(sw.x + 250, sw.y);
  await sleep(1500);
  h.back();
  await h.pickInSystemPicker(h.files.main);
  await h.waitText(h.titles.main, 30000);
  await sleep(800);
  h.shot("1-出現在目錄");
  const libFiles = sh(`ls ${LIB}`);
  const papers = await invoke("list_papers");
  h.rec(
    "1 收進來",
    libFiles.includes("library.json") && papers.length === 1,
    `從空的首頁按「${T("home.empty.action")}」，說明後開權限、在系統選檔畫面選一份，回到首頁就出現「${h.titles.main}」；文獻庫在 ${LIB}（${libFiles.split(/\s+/).join("、")}），目錄 ${papers.length} 篇`,
  );
}

/** 2. 用零碎的時間接著讀：讀到第 7 頁離開 → 重開應用 → 一次點擊回到那一頁。 */
async function flow2(h) {
  await goHome(h);
  await openPaper(h, h.titles.main);
  await h.scrollReaderTo(7, 0.3);
  await sleep(3500); // 停下來約兩秒之後位置寫進紀錄
  const left = (await h.pillText()).trim();
  h.shot("2-讀到第7頁");
  await L.launchApp({ fresh: true });
  await h.waitText(T("home.continue.heading"));
  await sleep(800);
  const card = await L.evalWeb(`document.querySelector('.cont-card')?.innerText ?? ''`);
  h.shot("2-重開後首頁的繼續閱讀");
  await L.tapReal("css=.cont-card");
  await h.waitReaderRendered();
  await sleep(1500);
  const back = (await h.pillText()).trim();
  h.shot("2-一次點擊回到那一頁");
  const wantCard = prefix("home.continue.pageOf") + "7";
  h.rec(
    "2 接著讀",
    card.includes(wantCard) && /(^|\D)7(\D|$)/.test(back),
    `離開時在 ${left}；重開應用，首頁最上面「${T("home.continue.heading")}」寫「${card.replace(/\s+/g, " ").trim()}」；點一下回到 ${back}`,
  );
}

/** 3. 知道自己讀過什麼、讀到哪：目錄上一篇未讀、一篇讀到第幾頁；標示為已讀之後寫「已讀」。 */
async function flow3(h) {
  await goHome(h);
  // 頂端的「＋」是選單（選 PDF、整批匯入）
  await L.tapReal(`aria=${T("home.add")}`);
  await L.tapReal(`btn=${T("add.menu.pdf")}`);
  await h.pickInSystemPicker(h.files.second);
  await h.waitText(h.titles.second, 30000);
  await sleep(1000);
  const reading = await rowText(h.titles.main);
  const unread = await rowText(h.titles.second);
  h.shot("3-目錄上看得出未讀與讀到第幾頁");
  // 標示為已讀：列的選單 → 文獻資訊 → 標示為已讀
  await L.tapReal(`aria=${T("home.row.menu", { title: h.titles.main })}`);
  await sleep(800);
  await L.tapReal(`btn=${T("info.title")}`);
  await h.waitText(T("info.markRead"));
  await L.tapReal(`text=${T("info.markRead")}`);
  await sleep(1200);
  await goHome(h);
  await sleep(800);
  const read = await rowText(h.titles.main);
  h.shot("3-標示之後寫已讀");
  const ok =
    reading.includes(prefix("home.status.pageOf") + "7") &&
    unread.includes(T("home.status.unread")) &&
    read.includes(T("home.status.read"));
  h.rec(
    "3 讀過什麼",
    ok,
    `「${h.titles.main}」：${reading.split("\n").slice(-1)[0]} → 標示後「${read.split("\n").slice(-1)[0]}」；剛加入的「${h.titles.second}」：${unread.split("\n").slice(-1)[0]}`,
  );
}

/** 4. 找到某一篇：記得篇名、作者或年份的任何一樣就找得到。PDF 沒帶年份，先在文獻資訊補上。 */
async function flow4(h) {
  await goHome(h);
  await L.tapReal(`aria=${T("home.row.menu", { title: h.titles.second })}`);
  await sleep(800);
  await L.tapReal(`btn=${T("info.title")}`);
  await h.waitText(T("info.field.year"));
  await L.tapReal("css=input[inputmode=numeric]");
  typeText(h.search.year);
  await sleep(600);
  await L.tapReal(`btn=${T("common.save")}`);
  await sleep(1500);
  h.shot("4-補上年份");
  await goHome(h);
  const results = {};
  for (const [label, query] of [
    ["作者", h.search.author],
    ["篇名", h.search.title],
    ["年份", h.search.year],
  ]) {
    if (!(await L.evalWeb(`!!document.querySelector('.search-input')`))) {
      await L.tapReal(`aria=${T("home.search")}`);
      await L.waitWeb("css=.search-input");
    }
    // 清掉上一次的字，再用鍵盤打
    await L.tapReal("css=.search-input");
    shell(`input keyevent KEYCODE_MOVE_END ${"KEYCODE_DEL ".repeat(20)}`);
    typeText(query);
    await sleep(1200);
    results[label] = { query, titles: await listedTitles() };
    h.shot(`4-用${label}找`);
  }
  // 收起搜尋列（鍵盤開著時第一下返回只收鍵盤）
  for (let i = 0; i < 3 && (await L.evalWeb(`!!document.querySelector('.search-input')`)); i++) {
    h.back();
    await sleep(800);
  }
  const ok = Object.entries(results).every(
    ([label, r]) => r.titles.length >= 1 && r.titles.every((x) => x.includes(h.search.expect[label])),
  );
  h.rec(
    "4 找到某一篇",
    ok,
    Object.entries(results)
      .map(([label, r]) => `用${label}「${r.query}」→ ${r.titles.join("、") || "（沒有）"}`)
      .join("；"),
  );
}

/** 5. 邊讀邊把想法記下來：選一段字劃線、寫一句短評，不離開那一頁；筆記檔裡看得到，筆記工具打得開。 */
async function flow5(h) {
  await goHome(h);
  await openPaper(h, h.titles.main);
  await h.scrollReaderTo(2, 0.1);
  await sleep(1500);
  // 長按文字層裡的一個字，系統選字（真的觸控），劃線的小工具列出來
  const word = await L.evalWeb(`(function(){
    const pg = document.querySelector('.reader-scroll .pg[data-n="2"]');
    const spans = [...pg.querySelectorAll('.textLayer span')].filter(s => (s.textContent||'').trim().length > 4);
    const s = spans.find(s => { const r = s.getBoundingClientRect(); return r.top > 260 && r.bottom < innerHeight - 260 && r.left > innerWidth * 0.5; });
    if (!s) return null;
    s.setAttribute('data-wt', 'pick');
    return s.textContent;
  })()`);
  if (!word) throw new Error("第 2 頁找不到可以選的字");
  // 長按之前腳本會把那個字捲到畫面中間，所以「原本在哪一頁」在選好字之後才記
  await L.longPressReal("css=[data-wt=pick]", 900);
  await sleep(1200);
  await L.waitWeb(`btn=${T("reader.highlight.withComment")}`, { timeoutMs: 8000 });
  const before = (await h.pillText()).trim();
  h.shot("5-選字後出現劃線");
  await L.tapReal(`btn=${T("reader.highlight.withComment")}`);
  await h.waitText(T("reader.edit.title"));
  const comment = h.comment;
  await L.tapReal("css=.sheet textarea");
  typeText(comment);
  await sleep(800);
  h.shot("5-寫一句短評");
  await L.tapReal(`btn=${T("common.save")}`);
  await sleep(2000);
  const after = (await h.pillText()).trim();
  const marked = await L.evalWeb(`document.querySelectorAll('.reader-scroll .pg[data-n="2"] .hl.has-comment').length`);
  h.shot("5-劃線與短評存好仍在同一頁");
  // 筆記檔在目前文獻庫的 notes/（單獨重跑這一條時，文獻庫可能已經是第 6 條還原出來的那一個）
  const lib = (await invoke("library_status")).path;
  const notes = sh(`grep -l '${comment}' '${lib}'/notes/*.md`);
  const quoted = notes ? sh(`grep -c '${comment}' '${notes.split("\n")[0]}'`) : "0";
  h.rec(
    "5 記下想法",
    !!notes && marked > 0 && before === after,
    `在 ${before} 長按選字「${word.trim().slice(0, 30)}」→「${T("reader.highlight.withComment")}」→ 打「${comment}」→「${T("common.save")}」；存好後仍在 ${after}，頁上帶短評的劃線 ${marked} 條；筆記檔 ${notes || "（沒找到）"} 裡有這句短評（${quoted} 處），是一般的 Markdown 檔，筆記工具直接打得開`,
  );
}

/** 6. 換裝置或資料出問題時不失去東西：匯出備份檔存到下載資料夾 → 從備份檔還原 → 文獻、筆記、紀錄都回來。 */
async function flow6(h) {
  await goHome(h);
  const before = await invoke("list_papers");
  sh("rm -f /sdcard/Download/PaperReader-backup*.zip");
  await sidebar(T("settings.title"));
  await h.waitText(T("backup.export"));
  await L.tapReal(`btnc=${T("backup.export")}`);
  await h.waitText(T("backup.deliver.title"), 120000);
  h.shot("6-備份檔做好了");
  await L.tapReal(`btn=${T("export.save")}`);
  await saveInDocumentsUi(h);
  let saved = "";
  for (let i = 0; i < 10 && !saved; i++) {
    await sleep(1500);
    saved = sh("find /storage/emulated/0/Download -name 'PaperReader-backup*.zip'");
  }
  await h.waitText(T("backup.deliver.saved.title"), 20000).catch(() => {});
  h.shot("6-存好了");
  await goHome(h);
  await sidebar(T("settings.title"));
  await h.waitText(T("backup.restore"));
  await L.tapReal(`btnc=${T("backup.restore")}`);
  await h.waitText(T("backup.confirm.title"));
  h.shot("6-從備份檔還原的確認");
  await L.tapReal(`btn=${T("backup.confirm.go")}`);
  await h.onSystemScreen("documentsui");
  await sleep(2000);
  const zip = saved.split("\n")[0].split("/").pop();
  await h.pickInSystemPicker(zip, null, "Downloads");
  await h.waitText(T("backup.result.title"), 120000);
  await sleep(1500);
  h.shot("6-還原的結果");
  const status = await invoke("library_status");
  const after = await invoke("list_papers");
  const notes = sh(`ls '${status.path}/notes'`);
  const logs = sh(`ls '${status.path}/logs'`);
  const same = before.length === after.length && before.every((p) => after.some((q) => q.id === p.id));
  h.rec(
    "6 不失去東西",
    !!saved && same && /\.md/.test(notes) && /reading-/.test(logs),
    `匯出備份檔、用系統的存檔畫面存到 ${saved || "（沒存到）"}；再從這個檔還原：新的文獻庫在 ${status.path}，文獻 ${before.length} → ${after.length} 篇（代號相同），筆記檔 ${notes.split(/\s+/).join("、")}，閱讀紀錄 ${logs.split(/\s+/).join("、")}`,
  );
  await goHome(h);
}

/** 7. 讀外文論文時對照母語：加入譯本 → 在第 3 頁切到譯本、再切回原文，停在同一頁。 */
async function flow7(h) {
  await goHome(h);
  await L.tapReal(`aria=${T("home.row.menu", { title: h.titles.main })}`);
  await sleep(800);
  await L.tapReal(`btn=${T("info.title")}`);
  await h.waitText(T("translation.add"));
  await L.tapReal(`btn=${T("translation.add")}`);
  await h.pickInSystemPicker(h.files.translation);
  await h.waitText(T("translation.added"), 30000).catch(() => {});
  await sleep(1500);
  h.shot("7-加入譯本");
  await goHome(h);
  await openPaper(h, h.titles.main);
  await h.scrollReaderTo(3, 0.2);
  await sleep(1500);
  await showReaderBar();
  const src = (await h.pillText()).trim();
  h.shot("7-原文第3頁");
  await L.tapReal(`aria=${T("reader.swap.toTranslation")}`);
  await sleep(2500);
  await h.waitReaderRendered();
  const tr = (await h.pillText()).trim();
  await showReaderBar();
  const onTr = await L.evalWeb(`!![...document.querySelectorAll('[aria-label]')].find(e => e.getAttribute('aria-label') === ${JSON.stringify(T("reader.swap.toSource"))})`);
  h.shot("7-切到譯本同一頁");
  await L.tapReal(`aria=${T("reader.swap.toSource")}`);
  await sleep(2500);
  const back = (await h.pillText()).trim();
  h.shot("7-切回原文同一頁");
  const page = (s) => /(\d+)/.exec(s)?.[1];
  h.rec(
    "7 對照母語",
    onTr && page(src) === "3" && page(tr) === "3" && page(back) === "3",
    `文獻資訊「${T("translation.add")}」選一份 PDF；閱讀時在 ${src} 按「${T("reader.swap.toTranslation")}」→ 譯本 ${tr}；再按「${T("reader.swap.toSource")}」→ 原文 ${back}`,
  );
}

export const FLOWS = [flow1, flow2, flow3, flow4, flow5, flow6, flow7];

/** 依序跑（`only` 是 1 到 7 時只跑那一條）。每條各自接住錯誤，記成不通過並截圖。 */
export async function runFlows(h, only) {
  for (const [i, flow] of FLOWS.entries()) {
    const n = i + 1;
    if (only && only !== n) continue;
    console.log(`\n=== 第 ${n} 條 ===`);
    try {
      await flow(h);
    } catch (e) {
      h.rec(`${n} 中斷`, false, `腳本在這一條中斷：${String(e?.message ?? e).slice(0, 300)}`);
      try {
        h.shot(`${n}-中斷當下`);
      } catch {
        // 忽略
      }
    }
  }
}
