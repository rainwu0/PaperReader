// 一次改好版號的所有位置（規格/05_版號規則.md 第 5、8、9 節）。
//
// 用法（在任何位置）：node app/scripts/bump-version.mjs <新版號>
//   新版號寫對外的寫法（a.b.c 或 a.b.c.d，規格/05 第 1 節），例：node app/scripts/bump-version.mjs 0.1.1  或  0.1.1.1
//   設定檔裡寫工具收得下的寫法，由這支換算：0.1.1.1 → 0.1.1+1。對外的 + 一律不接受。
//
// 改的地方：
//   - src-tauri/tauri.conf.json 的 version 與 bundle.android.versionCode（照公式）
//   - Cargo.toml 工作區的版號，以及 Cargo.lock 裡我們自己的套件（沒有 source 的那幾筆）
//   - package.json 與 package-lock.json 的版號
//   - <版本庫>/CHANGELOG.md（標題用對外的版號）：把 `## [Unreleased]` 底下的內容移到新的一節 `## [新版號] - 今天`，留一個空的 [Unreleased]
// 新版號不合規格、或不比現在的新，就拒絕，什麼都不改。改完跑 version-check 確認一致。
// 不建標籤、不提交；那些由人決定。

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { checkApp, compareVersions, parseVersion, parsePublicVersion, toPublicVersion, toToolVersion, versionCodeOf } from "./version-check.mjs";

/** 把 [Unreleased] 底下的內容移到新的一節。回新的全文；找不到 [Unreleased] 或該版已存在就丟錯。 */
export function updateChangelog(text, version, date) {
  const heading = /^## \[Unreleased\][ \t]*\r?\n/m.exec(text);
  if (!heading) throw new Error("CHANGELOG.md 找不到「## [Unreleased]」");
  if (new RegExp(`^## \\[${escapeRe(version)}\\]`, "m").test(text)) {
    throw new Error(`CHANGELOG.md 已經有「## [${version}]」這一節`);
  }
  const eol = heading[0].endsWith("\r\n") ? "\r\n" : "\n";
  const bodyStart = heading.index + heading[0].length;
  const next = /^## \[/m.exec(text.slice(bodyStart));
  const bodyEnd = next ? bodyStart + next.index : text.length;
  const body = text.slice(bodyStart, bodyEnd).replace(/^(\r?\n)+/, "").replace(/(\r?\n)+$/, "");
  const head = text.slice(0, heading.index);
  const tail = text.slice(bodyEnd);
  const section = `## [${version}] - ${date}${eol}${body ? eol + body + eol : ""}`;
  return `${head}## [Unreleased]${eol}${eol}${section}${tail ? eol + tail : ""}`;
}

function escapeRe(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** 改 tauri.conf.json（保留原本的排版）：version 與 bundle.android.versionCode。 */
export function updateTauriConf(text, version, versionCode) {
  let out = text.replace(/^(\s*"version"\s*:\s*)"[^"]*"/m, `$1"${version}"`);
  if (out === text && !text.includes(`"version": "${version}"`)) throw new Error("tauri.conf.json 找不到 version");
  if (/"versionCode"\s*:/.test(out)) {
    out = out.replace(/("versionCode"\s*:\s*)\d+/, `$1${versionCode}`);
  } else {
    const conf = JSON.parse(out);
    if (conf.bundle?.android) throw new Error("tauri.conf.json 的 bundle.android 存在但沒有 versionCode，請手動處理");
    // 沒有 android 區塊：加在 bundle 的最後
    const m = /("bundle"\s*:\s*\{[\s\S]*?)(\n(\s*)\}\s*\n?\}\s*\n?)$/.exec(out);
    if (!m) throw new Error("tauri.conf.json 找不到 bundle 區塊的結尾，請手動加 bundle.android.versionCode");
    const indent = m[3];
    const lastLine = m[1].replace(/\s+$/, "");
    out =
      out.slice(0, m.index) +
      `${lastLine},\n${indent}  "android": {\n${indent}    "versionCode": ${versionCode}\n${indent}  }` +
      m[2];
  }
  JSON.parse(out); // 確認還是合法的 JSON
  return out;
}

/** 改 Cargo.toml 工作區的版號。 */
export function updateCargoToml(text, version) {
  const re = /(\[workspace\.package\][\s\S]*?^version\s*=\s*)"[^"]*"/m;
  if (!re.test(text)) throw new Error("Cargo.toml 的 [workspace.package] 找不到 version");
  return text.replace(re, `$1"${version}"`);
}

/** 改 Cargo.lock：只動版號等於舊版號、又沒有 source（來自本機路徑）的套件，也就是工作區自己的成員。 */
export function updateCargoLock(text, oldVersion, version) {
  return text.replace(/(\[\[package\]\]\r?\nname = "[^"]+"\r?\nversion = )"([^"]*)"(\r?\n(?!source ))/g, (all, pre, v, post) =>
    v === oldVersion ? `${pre}"${version}"${post}` : all,
  );
}

/** 改 package.json／package-lock.json 的版號（lock 的根與 packages[""]）。保留排版。 */
export function updatePackageJson(text, version) {
  const out = text.replace(/^(\s*"version"\s*:\s*)"[^"]*"/m, `$1"${version}"`);
  return out;
}

export function updatePackageLock(text, version) {
  const lock = JSON.parse(text);
  lock.version = version;
  if (lock.packages?.[""]) lock.packages[""].version = version;
  const indent = /^(\s+)"/m.exec(text)?.[1] ?? "  ";
  return JSON.stringify(lock, null, indent) + (text.endsWith("\n") ? "\n" : "");
}

function today() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/**
 * 執行一次版號更新。dirs = { app, changelog }；date 預設今天。
 * 回 { oldVersion, version, versionCode }；有問題丟錯（此時尚未寫任何檔）。
 */
export function bump(newVersion, dirs, date = today()) {
  const target = parsePublicVersion(newVersion);
  if (target.error) throw new Error(`新版號不合規格：${target.error}`);
  const toolVersion = toToolVersion(newVersion);

  const before = checkApp(dirs.app);
  if (before.errors.length) {
    throw new Error(`更新前版號就不一致，先修好：\n  - ${before.errors.join("\n  - ")}`);
  }
  const oldVersion = before.versions.tauri;
  const current = parseVersion(oldVersion);
  const oldPublic = toPublicVersion(oldVersion);
  if (compareVersions(target, current) <= 0) {
    throw new Error(`新版號 ${newVersion} 不比現在的 ${oldPublic} 新（只能往上加）`);
  }
  const versionCode = versionCodeOf(target);

  const read = (p) => readFileSync(p, "utf8");
  const files = {
    conf: join(dirs.app, "src-tauri", "tauri.conf.json"),
    cargo: join(dirs.app, "Cargo.toml"),
    lock: join(dirs.app, "Cargo.lock"),
    pkg: join(dirs.app, "package.json"),
    pkgLock: join(dirs.app, "package-lock.json"),
    changelog: dirs.changelog,
  };
  // 全部先算好再寫：任何一個算不出來就整個不動
  const next = {
    [files.conf]: updateTauriConf(read(files.conf), toolVersion, versionCode),
    [files.cargo]: updateCargoToml(read(files.cargo), toolVersion),
    [files.lock]: updateCargoLock(read(files.lock), oldVersion, toolVersion),
    [files.pkg]: updatePackageJson(read(files.pkg), toolVersion),
    [files.pkgLock]: updatePackageLock(read(files.pkgLock), toolVersion),
    [files.changelog]: updateChangelog(read(files.changelog), newVersion, date),
  };
  for (const [file, text] of Object.entries(next)) writeFileSync(file, text);

  const after = checkApp(dirs.app);
  if (after.errors.length) {
    throw new Error(`更新後的檢查沒過（檔案已寫入，請檢查 git diff）：\n  - ${after.errors.join("\n  - ")}`);
  }
  return { oldVersion: oldPublic, version: newVersion, toolVersion, versionCode };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const arg = process.argv[2];
  if (!arg || process.argv.length > 3) {
    console.error("用法：node app/scripts/bump-version.mjs <新版號>（對外的寫法，例：0.1.1 或 0.1.1.1）");
    process.exit(2);
  }
  const app = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  try {
    const r = bump(arg, { app, changelog: join(app, "..", "CHANGELOG.md") });
    console.log(`版號 ${r.oldVersion} → ${r.version}（設定檔寫 ${r.toolVersion}），versionCode ${r.versionCode}。`);
    console.log("已改：tauri.conf.json、Cargo.toml、Cargo.lock、package.json、package-lock.json、CHANGELOG.md。沒有提交、沒有建標籤。");
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}
