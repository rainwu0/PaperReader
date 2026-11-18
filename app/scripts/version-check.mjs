// 版號一致性檢查（規格/05_版號規則.md 第 1、5、9 節）。
//
// 檢查：
//   1. 版號格式：`a.b.c` 或 `a.b.c+d`，四段是不補零的十進位整數，d 為 0 時不寫 `+0`，
//      各段在上限內（a ≤ 20、b ≤ 99、c ≤ 9999、d ≤ 99）。
//   2. 三處版號相同：src-tauri/tauri.conf.json 的 version、Cargo.toml 工作區的版號、package.json 的 version；
//      工作區成員若自己寫了版號（沒有 version.workspace = true）也要相同。
//   3. src-tauri/tauri.conf.json 的 bundle.android.versionCode 等於 a×10⁸ ＋ b×10⁶ ＋ c×10² ＋ d。
//
//   4. Android 的 versionName 覆寫還在：gen/android/app/build.gradle.kts 把 Tauri 給的 a.b.c+d 換成對外的 a.b.c.d。
//
// 對外的版號（標籤、安裝檔名、變更紀錄、「關於」頁、versionName）寫 a.b.c 或 a.b.c.d，由設定檔的版號換算
// （toPublicVersion）；+ 不出現在任何對外的地方（規格/05 第 1 節）。
//
// 用法（在任何位置）：node app/scripts/version-check.mjs
// 通過印一行摘要、結束碼 0；不通過列出哪裡錯、結束碼 1。

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const LIMITS = { a: 20, b: 99, c: 9999, d: 99 };

const NUM = "(0|[1-9][0-9]*)";
const VERSION_RE = new RegExp(`^${NUM}\\.${NUM}\\.${NUM}(?:\\+${NUM})?$`);

/** 拆版號。格式不合回 { error }；合格回 { a, b, c, d }（沒有 +d 時 d 為 0）。 */
export function parseVersion(text) {
  if (typeof text !== "string") return { error: "版號不是字串" };
  if (/^\d+\.\d+\.\d+(\+\d+)?$/.test(text)) {
    for (const seg of text.split(/[.+]/)) {
      if (seg.length > 1 && seg.startsWith("0")) return { error: `「${text}」有補零的段「${seg}」（不補零）` };
    }
    if (/\+0$/.test(text)) return { error: `「${text}」寫了 +0（d 為 0 時省略）` };
  }
  const m = VERSION_RE.exec(text);
  if (!m) return { error: `「${text}」不是 a.b.c 或 a.b.c+d 的寫法（四段都是不補零的十進位整數）` };
  const [a, b, c] = [m[1], m[2], m[3]].map(Number);
  const d = m[4] === undefined ? 0 : Number(m[4]);
  if (m[4] === "0") return { error: `「${text}」寫了 +0（d 為 0 時省略）` };
  const over = [];
  for (const [k, v] of Object.entries({ a, b, c, d })) {
    if (v > LIMITS[k]) over.push(`${k}=${v} 超過上限 ${LIMITS[k]}`);
  }
  if (over.length) return { error: `「${text}」${over.join("；")}` };
  return { a, b, c, d };
}

/** 設定檔的版號（`a.b.c` 或 `a.b.c+d`）換成對外的版號（`a.b.c` 或 `a.b.c.d`）。 */
export function toPublicVersion(toolVersion) {
  return toolVersion.replace("+", ".");
}

/** 對外的版號（`a.b.c` 或 `a.b.c.d`）換成設定檔的版號。不合規格丟錯。 */
export function toToolVersion(publicVersion) {
  const p = parsePublicVersion(publicVersion);
  if (p.error) throw new Error(p.error);
  return p.d ? `${p.a}.${p.b}.${p.c}+${p.d}` : `${p.a}.${p.b}.${p.c}`;
}

/** 拆對外的版號。合格回 { a, b, c, d }；不合回 { error }。 */
export function parsePublicVersion(text) {
  if (typeof text !== "string") return { error: "版號不是字串" };
  if (text.includes("+")) return { error: `「${text}」有 +：對外的版號用點（0.1.12.3），+ 只在設定檔裡` };
  const m = /^(\d+)\.(\d+)\.(\d+)(?:\.(\d+))?$/.exec(text);
  if (!m) return { error: `「${text}」不是 a.b.c 或 a.b.c.d 的寫法（四段都是不補零的十進位整數）` };
  for (const seg of m.slice(1)) {
    if (seg !== undefined && seg.length > 1 && seg.startsWith("0")) return { error: `「${text}」有補零的段「${seg}」（不補零）` };
  }
  if (m[4] === "0") return { error: `「${text}」寫了 .0 的第四段（d 為 0 時只寫三段）` };
  // 上限和設定檔的寫法共用同一組
  return parseVersion(m[4] === undefined ? text : `${m[1]}.${m[2]}.${m[3]}+${m[4]}`);
}

/** versionCode ＝ a×10⁸ ＋ b×10⁶ ＋ c×10² ＋ d */
export function versionCodeOf({ a, b, c, d }) {
  return a * 100_000_000 + b * 1_000_000 + c * 100 + d;
}

/** 比較兩個拆好的版號：負數表示 x 較舊。 */
export function compareVersions(x, y) {
  for (const k of ["a", "b", "c", "d"]) {
    if (x[k] !== y[k]) return x[k] - y[k];
  }
  return 0;
}

/** 讀各處的版號。回 { tauri, versionCode, cargo, pkg, members: [{ file, version }] }，讀不到的欄位為 undefined 並放進 problems。 */
export function readVersions(appDir) {
  const problems = [];
  const out = { members: [], problems };

  const confPath = join(appDir, "src-tauri", "tauri.conf.json");
  try {
    const conf = JSON.parse(readFileSync(confPath, "utf8"));
    out.tauri = conf.version;
    out.versionCode = conf.bundle?.android?.versionCode;
  } catch (e) {
    problems.push(`讀不到 ${confPath}：${e.message}`);
  }

  const pkgPath = join(appDir, "package.json");
  try {
    out.pkg = JSON.parse(readFileSync(pkgPath, "utf8")).version;
  } catch (e) {
    problems.push(`讀不到 ${pkgPath}：${e.message}`);
  }

  const cargoPath = join(appDir, "Cargo.toml");
  try {
    const text = readFileSync(cargoPath, "utf8");
    const section = /\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/.exec(text);
    const m = section && /^version\s*=\s*"([^"]*)"/m.exec(section[1]);
    if (m) out.cargo = m[1];
    else problems.push(`${cargoPath} 的 [workspace.package] 沒有 version`);
  } catch (e) {
    problems.push(`讀不到 ${cargoPath}：${e.message}`);
  }

  const gradlePath = join(appDir, "src-tauri", "gen", "android", "app", "build.gradle.kts");
  try {
    out.gradle = readFileSync(gradlePath, "utf8");
  } catch (e) {
    problems.push(`讀不到 ${gradlePath}：${e.message}`);
  }

  // 工作區成員：各自的 Cargo.toml 若寫了自己的版號（沒有 version.workspace = true）也要檢查
  for (const file of memberManifests(appDir)) {
    const text = readFileSync(file, "utf8");
    const pkgSection = /\[package\]([\s\S]*?)(?=\n\[|$)/.exec(text);
    if (!pkgSection) continue;
    const m = /^version\s*=\s*"([^"]*)"/m.exec(pkgSection[1]);
    if (m) out.members.push({ file, version: m[1] });
  }
  return out;
}

function memberManifests(appDir) {
  const files = [];
  let cargo;
  try {
    cargo = readFileSync(join(appDir, "Cargo.toml"), "utf8");
  } catch {
    return files;
  }
  const list = /members\s*=\s*\[([\s\S]*?)\]/.exec(cargo);
  if (!list) return files;
  for (const [, pattern] of list[1].matchAll(/"([^"]+)"/g)) {
    const dirs = pattern.endsWith("/*")
      ? readdirSafe(join(appDir, pattern.slice(0, -2))).map((n) => join(appDir, pattern.slice(0, -2), n))
      : [join(appDir, pattern)];
    for (const dir of dirs) {
      const f = join(dir, "Cargo.toml");
      if (existsSync(f)) files.push(f);
    }
  }
  return files;
}

function readdirSafe(dir) {
  try {
    return readdirSync(dir);
  } catch {
    return [];
  }
}

/** 檢查讀到的內容，回錯誤清單（空的就是通過）。 */
export function validate(v) {
  const errors = [...(v.problems ?? [])];

  const sources = [
    ["tauri.conf.json 的 version", v.tauri],
    ["app/Cargo.toml 工作區的版號", v.cargo],
    ["app/package.json 的 version", v.pkg],
    ...(v.members ?? []).map((m) => [`${m.file} 自己寫的版號`, m.version]),
  ];
  for (const [name, value] of sources) {
    if (value === undefined) {
      if (!(v.problems ?? []).length) errors.push(`${name}：沒有這個欄位`);
      continue;
    }
    const p = parseVersion(value);
    if (p.error) errors.push(`${name}：${p.error}`);
  }

  const known = sources.filter(([, value]) => value !== undefined);
  if (new Set(known.map(([, value]) => value)).size > 1) {
    errors.push(`版號不一致：${known.map(([n, value]) => `${n}＝${value}`).join("；")}`);
  }

  const parsed = v.tauri === undefined ? undefined : parseVersion(v.tauri);
  if (v.versionCode === undefined) {
    errors.push("tauri.conf.json 沒有 bundle.android.versionCode（要明訂，Tauri 預設的算法會丟掉 d）");
  } else if (!Number.isInteger(v.versionCode)) {
    errors.push(`bundle.android.versionCode「${v.versionCode}」不是整數`);
  } else if (parsed && !parsed.error) {
    const want = versionCodeOf(parsed);
    if (v.versionCode !== want) {
      errors.push(`bundle.android.versionCode 是 ${v.versionCode}，版號 ${v.tauri} 依公式應該是 ${want}`);
    }
  }

  // Android 的 versionName：覆寫要在，且把 + 換成點
  if (v.gradle !== undefined) {
    const line = /^\s*versionName\s*=.*$/m.exec(v.gradle)?.[0] ?? "";
    if (!/tauri\.android\.versionName/.test(line) || !/\.replace\(\s*['"]\+['"]\s*,\s*['"]\.['"]\s*\)/.test(line)) {
      errors.push(
        "gen/android/app/build.gradle.kts 的 versionName 沒有把 Tauri 給的 a.b.c+d 換成 a.b.c.d（要有 .replace('+', '.')；Tauri 重新產生專案時可能蓋掉）",
      );
    }
  }
  return errors;
}

export function checkApp(appDir) {
  const v = readVersions(appDir);
  const errors = validate(v);
  const summary = errors.length ? null : `版號 ${toPublicVersion(v.tauri)}（設定檔 ${v.tauri}），versionCode ${v.versionCode}，三處一致，versionName 覆寫在`;
  return { errors, summary, versions: v };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const appDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const { errors, summary } = checkApp(appDir);
  if (errors.length) {
    console.error("版號檢查失敗（規則見 規格/05_版號規則.md）：");
    for (const e of errors) console.error(`  - ${e}`);
    process.exit(1);
  }
  console.log(summary);
}
