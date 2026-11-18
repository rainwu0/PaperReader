// 「關於」頁的組件與授權清單。介面建置前執行，產生 public/licenses.json（不進版本庫）。
//
// 清單不手抄，從這幾個來源產生：
//   - Rust：`cargo metadata --locked`（讀 Cargo.lock），只取應用（paperreader-app）在 Android 與 Windows 上一般相依的套件，
//     建置時用的（build-dependencies）、測試用的（dev-dependencies）、只在別的平台用的都不算；我們自己的套件不列。
//     只有一邊有的標 `platforms`，「關於」頁只列這個安裝檔有的。
//   - 畫面：package-lock.json 裡不是開發用（dev）也不是選用（optional，Node 才用的原生模組）的套件；只有型別宣告的 @types 不列。
//   - Android 元件：兩份 build.gradle.kts 裡直接宣告的 implementation。
//   - Windows 元件：系統提供、不隨附的 WebView2（`windowsComponents`）。
//   - 套件管理工具管不到、但安裝檔內含的檔（pdf.js 內附的字型與解碼器、蓋過 pdf.js 那一份的 Liberation Sans 2.x、CSL 格式檔、
//     Firefox 翻譯的引擎）與第一次用時才下載的模型：
//     下面的 EXTRAS，每一項指到內含的檔與它附的授權檔；檔不在就建置失敗。
//   - 版面模型的 ONNX Runtime 原生程式庫與它帶進的組件、libc++_shared.so：建置時才下載，清單照
//     核對過的版本（`onnxRuntimeComponents`）；`ort-sys` 換版本時建置失敗，要重新核對。
// 授權全文用套件自己附的授權檔（LICENSE、COPYING、NOTICE 等，保留各自的著作權聲明）；套件沒附的，依授權代號用
// scripts/licenses/ 的標準全文（MIT、BSD 的著作權那一行填入套件登記的作者）。內容相同的全文只存一份。
//
// 用法：node scripts/licenses.mjs（`npm run assets` 會執行）。其他腳本可以 import { collect } 取得同一份資料。

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const APP = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const LICENSE_FILE = /^(licen[cs]e|copying|notice|unlicense)/i;

/** 套件沒附授權檔時用的標準全文（`{copyright}` 換成作者）。 */
const STANDARD = {
  MIT: "scripts/licenses/MIT.txt",
  "Apache-2.0": "../LICENSE",
  "MPL-2.0": "scripts/licenses/MPL-2.0.txt",
  "BSD-3-Clause": "scripts/licenses/BSD-3-Clause.txt",
  "BSD-2-Clause": "scripts/licenses/BSD-2-Clause.txt",
  "BSL-1.0": "scripts/licenses/BSL-1.0.txt",
  "0BSD": "scripts/licenses/0BSD.txt",
  "CC-BY-SA-3.0": "scripts/licenses/CC-BY-SA-3.0.txt",
};

const must = (rel) => {
  const p = resolve(APP, rel);
  if (!existsSync(p)) throw new Error(`授權清單：找不到 ${rel}（應用內含的檔或它的授權檔）`);
  return p;
};
const read = (p) => readFileSync(p, "utf8").replace(/\r\n/g, "\n").trim();

/** 授權全文的存放處：同樣的內容只存一份，以雜湊值當代號。 */
class Texts {
  constructor() {
    this.map = new Map();
  }
  add(name, body) {
    const id = createHash("sha256").update(body).digest("hex").slice(0, 16);
    if (!this.map.has(id)) this.map.set(id, { name, body });
    return id;
  }
  file(path, name = path.split(/[\\/]/).pop()) {
    return this.add(name, read(path));
  }
  /** 依授權代號用標準全文；`who` 填進著作權那一行。 */
  standard(spdx, who) {
    const rel = STANDARD[spdx];
    if (!rel) return null;
    const body = read(must(rel)).replace("{copyright}", who);
    return this.add(spdx, body);
  }
  toJSON() {
    return Object.fromEntries(this.map);
  }
}

/** 授權式子裡出現的代號（`MIT OR Apache-2.0`、`Apache-2.0/MIT`、`(MIT AND Zlib)`）。 */
export function spdxIds(expr) {
  return [
    ...new Set(
      String(expr ?? "")
        .split(/\s+(?:OR|AND|WITH)\s+|[()/]/)
        .map((s) => s.trim())
        .filter(Boolean),
    ),
  ];
}

/** 一個套件資料夾裡附的授權檔的全文代號；沒有就依授權代號用標準全文。 */
function packageTexts(texts, dir, license, who) {
  const files = readdirSync(dir)
    .filter((f) => LICENSE_FILE.test(f) && statSync(join(dir, f)).isFile())
    .sort();
  if (files.length > 0) return files.map((f) => texts.file(join(dir, f)));
  const ids = spdxIds(license)
    .map((s) => texts.standard(s, who))
    .filter(Boolean);
  if (ids.length === 0) throw new Error(`授權清單：${dir} 沒有授權檔，也沒有「${license}」的標準全文`);
  return ids;
}

/** 版本庫的網址整理成可以打開的網頁網址。 */
function webUrl(repo) {
  if (!repo) return null;
  let u = typeof repo === "string" ? repo : repo.url;
  if (!u) return null;
  u = u.replace(/^git\+/, "").replace(/^git:\/\//, "https://").replace(/\.git$/, "");
  if (/^github:/.test(u)) u = `https://github.com/${u.slice(7)}`;
  if (/^[\w.-]+\/[\w.-]+$/.test(u)) u = `https://github.com/${u}`;
  return u;
}

/** `platforms`：只在這些作業系統的安裝檔裡（`android`、`windows`）；不給表示都有。 */
const item = (name, version, license, source, textIds, note, platforms) => ({
  name,
  version,
  license,
  source,
  mpl: spdxIds(license).includes("MPL-2.0"),
  ...(note ? { note } : {}),
  ...(platforms ? { platforms } : {}),
  texts: textIds,
});

const byName = (a, b) => a.name.localeCompare(b.name, "en", { sensitivity: "base" }) || a.version.localeCompare(b.version);

// ---------- Rust ----------

function cargoMetadata(platform) {
  const out = execFileSync(
    "cargo",
    [
      "metadata",
      "--format-version",
      "1",
      "--locked",
      "--filter-platform",
      platform,
      "--manifest-path",
      join(APP, "src-tauri", "Cargo.toml"),
    ],
    { encoding: "utf8", maxBuffer: 1 << 28, stdio: ["ignore", "pipe", "inherit"] },
  );
  return JSON.parse(out);
}

/** 編譯目標 → 安裝檔的作業系統（`AppInfo.platform`）。 */
const RUST_TARGETS = [
  ["aarch64-linux-android", "android"],
  ["x86_64-linux-android", "android"],
  ["x86_64-pc-windows-msvc", "windows"],
];
const OSES = [...new Set(RUST_TARGETS.map(([, os]) => os))];

/**
 * 應用在 Android（真機 aarch64 與模擬器 x86_64）與 Windows 上編進去的 Rust 套件。
 * 只有一邊有的標 `platforms`，「關於」頁只列這個安裝檔有的。
 */
export function rustComponents(texts) {
  const found = new Map();
  const oses = new Map();
  for (const [platform, os] of RUST_TARGETS) {
    const meta = cargoMetadata(platform);
    const pkgs = new Map(meta.packages.map((p) => [p.id, p]));
    const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
    const own = new Set(meta.workspace_members);
    const root = meta.packages.find((p) => p.name === "paperreader-app");
    if (!root) throw new Error("授權清單：cargo metadata 裡沒有 paperreader-app");
    const seen = new Set();
    const stack = [root.id];
    while (stack.length > 0) {
      const id = stack.pop();
      if (seen.has(id)) continue;
      seen.add(id);
      for (const d of nodes.get(id)?.deps ?? []) {
        // 只走一般相依（kind 是 null）；build、dev 的不進執行檔
        if (d.dep_kinds.some((k) => k.kind === null)) stack.push(d.pkg);
      }
    }
    for (const id of seen) {
      if (own.has(id)) continue;
      if (!oses.has(id)) oses.set(id, new Set());
      oses.get(id).add(os);
      if (found.has(id)) continue;
      const p = pkgs.get(id);
      const who = p.authors?.length ? p.authors.map((a) => a.replace(/\s*<[^>]*>/, "")).join(", ") : `the ${p.name} authors`;
      const license = p.license ?? (p.license_file ? `見 ${p.license_file}` : "未標明");
      const source = webUrl(p.repository) ?? `https://crates.io/crates/${p.name}/${p.version}`;
      found.set(id, item(p.name, p.version, license, source, packageTexts(texts, dirname(p.manifest_path), license, who)));
    }
  }
  for (const [id, it] of found) {
    const on = oses.get(id);
    if (on.size < OSES.length) it.platforms = OSES.filter((os) => on.has(os));
  }
  return [...found.values()].sort(byName);
}

// ---------- 畫面（npm） ----------

export function npmComponents(texts) {
  const lock = JSON.parse(readFileSync(join(APP, "package-lock.json"), "utf8"));
  const out = [];
  for (const [key, v] of Object.entries(lock.packages)) {
    if (!key || v.dev || v.devOptional || v.optional) continue;
    const name = key.replace(/^.*node_modules\//, "");
    if (name.startsWith("@types/")) continue;
    const dir = join(APP, key);
    if (!existsSync(join(dir, "package.json"))) throw new Error(`授權清單：找不到 ${key}（先執行 npm ci）`);
    const pj = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
    const who = typeof pj.author === "string" ? pj.author.replace(/\s*<[^>]*>/, "") : (pj.author?.name ?? `the ${name} authors`);
    const license = v.license ?? pj.license ?? "未標明";
    const source = webUrl(pj.repository) ?? `https://www.npmjs.com/package/${name}/v/${v.version}`;
    out.push(item(name, v.version, license, source, packageTexts(texts, dir, license, who)));
  }
  return out.sort(byName);
}

// ---------- Android 元件（Gradle） ----------

const ANDROIDX = "https://android.googlesource.com/platform/frameworks/support";
const GRADLE_SOURCES = {
  "com.google.android.material": "https://github.com/material-components/material-components-android",
};

/** 兩份 build.gradle.kts 直接宣告的 Maven 元件（同一個元件宣告了不同版本時，Gradle 用較新的那個）。 */
export function androidComponents(texts) {
  const files = ["src-tauri/gen/android/app/build.gradle.kts", "plugins/tauri-plugin-paperreader/android/build.gradle.kts"];
  const found = new Map();
  const newer = (a, b) => {
    const x = a.split(".").map(Number);
    const y = b.split(".").map(Number);
    for (let i = 0; i < Math.max(x.length, y.length); i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) > (y[i] ?? 0);
    return false;
  };
  for (const f of files) {
    for (const m of read(must(f)).matchAll(/implementation\("([\w.-]+):([\w.-]+):([\w.-]+)"\)/g)) {
      const [, group, artifact, version] = m;
      const key = `${group}:${artifact}`;
      if (!found.has(key) || newer(version, found.get(key).version)) found.set(key, { group, artifact, version });
    }
  }
  if (found.size === 0) throw new Error("授權清單：build.gradle.kts 裡找不到 implementation 宣告");
  const apache = texts.standard("Apache-2.0", "");
  return [...found.values()]
    .map(({ group, artifact, version }) =>
      item(`${group}:${artifact}`, version, "Apache-2.0", GRADLE_SOURCES[group] ?? ANDROIDX, [apache]),
    )
    .sort(byName);
}

// ---------- 套件管理工具管不到的內含檔與下載的模型 ----------

/** 蓋過 pdf.js 內附字型的 Liberation Sans 2.x（`scripts/copy-pdfjs-assets.mjs`）。 */
const LIBERATION = "assets/fonts/liberation-sans";

function pdfjsVersion() {
  return JSON.parse(readFileSync(must("node_modules/pdfjs-dist/package.json"), "utf8")).version;
}

/** CSL 格式檔的來源提交（`core/csl/README.md` 的表格）。 */
function cslCommits() {
  const md = read(must("core/csl/README.md"));
  const styles = /styles @ `([0-9a-f]{7,})/.exec(md)?.[1];
  const locales = /locales @ `([0-9a-f]{7,})/.exec(md)?.[1];
  if (!styles || !locales) throw new Error("授權清單：core/csl/README.md 裡找不到格式檔與語系檔的提交");
  return `styles ${styles.slice(0, 7)}、locales ${locales.slice(0, 7)}`;
}

/** Liberation Sans 的版本（`assets/fonts/liberation-sans/README.md` 的表格「| 版本 | X |」）。 */
function liberationVersion() {
  const md = read(must(`${LIBERATION}/README.md`));
  const v = /^\| 版本 \| ([\d.]+) \|/m.exec(md)?.[1];
  if (!v) throw new Error(`授權清單：${LIBERATION}/README.md 裡找不到版本`);
  return v;
}

function bergamotVersion() {
  const js = read(must("public/bergamot/bergamot-translator.js"));
  const v = /BERGAMOT_VERSION_FULL = "([^"]+)"/.exec(js)?.[1];
  if (!v) throw new Error("授權清單：bergamot-translator.js 裡找不到版本");
  return v;
}

/** Firefox 翻譯的語言模型版本（`src-tauri/src/ondevice.rs` 的說明：Remote Settings `translations-models-v2`，X 版）。 */
function modelVersion() {
  const rs = read(must("src-tauri/src/ondevice.rs"));
  const v = /translations-models-v2`，([\d.]+) 版/.exec(rs)?.[1];
  if (!v) throw new Error("授權清單：ondevice.rs 裡找不到語言模型的版本");
  return v;
}

// ---------- 版面模型：ONNX Runtime 的原生程式庫與它帶進的組件 ----------

/** 這份清單核對過的 `ort-sys` 版本（它決定下載哪一版的 ONNX Runtime）。版本變了要照新版的 `cmake/deps.txt` 重新核對。 */
const ORT_SYS = "2.0.0-rc.15";
const ORT = "1.31.0";

/** `Cargo.lock` 裡的 `ort-sys` 版本要和核對過的一樣，不然建置失敗（免得清單和實際帶進的對不上）。 */
function checkOrtVersion() {
  const lock = read(must("Cargo.lock"));
  const v = /name = "ort-sys"\nversion = "([^"]+)"/.exec(lock)?.[1];
  if (v !== ORT_SYS) throw new Error(`授權清單：ort-sys 是 ${v ?? "（沒有）"}，ONNX Runtime 的組件清單只核對過 ${ORT_SYS}`);
}

/**
 * 靜態連結進應用原生程式庫的 ONNX Runtime 與它帶進的組件（版本照 ONNX Runtime 1.31.0 的
 * `cmake/deps.txt`）。程式庫檔在建置時才從 cdn.pyke.io 取得，不在版本庫裡，所以這裡不檢查檔在不在。
 * 授權全文：這些組件的授權檔不在本機，用標準全文填入各自的著作權聲明（查閱日期 2026-11-17）。
 */
function onnxRuntimeComponents(texts) {
  checkOrtVersion();
  const apache = texts.standard("Apache-2.0", "");
  const mit = (who) => texts.standard("MIT", who);
  const inOrt = "ONNX Runtime 內含（版面模型）";
  return [
    item("ONNX Runtime（原生程式庫，pyke.io 的建置）", ORT, "MIT", "https://github.com/microsoft/onnxruntime", [mit("Microsoft Corporation")], "版面模型的執行；建置與修補：https://github.com/pykeio/ort-artifacts"),
    item("ONNX", "1.22.0", "Apache-2.0", "https://github.com/onnx/onnx", [apache], inOrt),
    item("Abseil", "20250814.0", "Apache-2.0", "https://github.com/abseil/abseil-cpp", [apache], inOrt),
    item("Protocol Buffers", "33.6", "BSD-3-Clause", "https://github.com/protocolbuffers/protobuf", [texts.standard("BSD-3-Clause", "2008 Google Inc.")], inOrt),
    item("utf8_range", "33.6", "MIT", "https://github.com/protocolbuffers/protobuf/tree/main/third_party/utf8_range", [mit("2019 Yibo Cai\nCopyright 2022 Google LLC")], "Protocol Buffers 內附"),
    item("RE2", "2024-07-02", "BSD-3-Clause", "https://github.com/google/re2", [texts.standard("BSD-3-Clause", "2009 The RE2 Authors")], inOrt),
    item("FlatBuffers", "23.5.26", "Apache-2.0", "https://github.com/google/flatbuffers", [apache], inOrt),
    item("Microsoft GSL", "4.2.1", "MIT", "https://github.com/microsoft/GSL", [mit("2015 Microsoft Corporation. All rights reserved.")], inOrt),
    item("nlohmann/json", "3.11.3", "MIT", "https://github.com/nlohmann/json", [mit("2013-2022 Niels Lohmann")], inOrt),
    // date 的 LICENSE.txt 沒有著作權那一行，只寫「以 MIT 授權發布」加上貢獻者名單
    item("date（Howard Hinnant）", "3.0.1", "MIT", "https://github.com/HowardHinnant/date", [mit("Howard Hinnant and the contributors listed in LICENSE.txt")], inOrt),
    item("SafeInt", "3.0.28", "MIT", "https://github.com/dcleblanc/SafeInt", [mit("2018 Microsoft")], inOrt),
    item(
      "MurmurHash3",
      ORT,
      "MIT",
      "https://github.com/aappleby/smhasher",
      [texts.add("MurmurHash3", "MurmurHash3 was written by Austin Appleby, and is placed in the public domain. The author hereby disclaims copyright to this source code.")],
      `${inOrt}；ONNX Runtime 的第三方聲明標為 MIT，作者聲明為公有領域`,
    ),
    item(
      "Eigen",
      "1d8b82b0740839c0de7f1242a3585e3390ff5f33",
      "MPL-2.0",
      "https://github.com/eigen-mirror/eigen/tree/1d8b82b0740839c0de7f1242a3585e3390ff5f33",
      [texts.standard("MPL-2.0", "")],
      "ONNX Runtime 內含，建置時套上 ONNX Runtime 1.31.0 的 cmake/patches/eigen；原始碼是這個版本加上那些修補",
    ),
    item(
      "cpuinfo（PyTorch）",
      "66ee79c038d70dad9f08705b2c9b3e58f6d8f512",
      "BSD-2-Clause",
      "https://github.com/pytorch/cpuinfo",
      [
        texts.standard(
          "BSD-2-Clause",
          [
            "Copyright (c) 2019 Google LLC",
            "Copyright (c) 2017-2018 Facebook Inc.",
            "Copyright (C) 2012-2017 Georgia Institute of Technology",
            "Copyright (C) 2010-2012 Marat Dukhan",
            "",
            "All rights reserved.",
          ].join("\n"),
        ),
      ],
      inOrt,
    ),
    item("Boost.MP11", "1.82.0", "BSL-1.0", "https://github.com/boostorg/mp11", [texts.standard("BSL-1.0", "")], `${inOrt}；只有編譯期的型別清單`),
    item("KleidiAI（Arm）", "1.30.0", "Apache-2.0", "https://github.com/ARM-software/kleidiai", [apache], `${inOrt}；只在 arm64`),
    item(
      "libc++ 與 libc++abi（Android NDK 的 libc++_shared.so）",
      "NDK 28.2.13676358",
      "Apache-2.0 WITH LLVM-exception",
      "https://github.com/llvm/llvm-project/tree/main/libcxx",
      [texts.file(must("assets/licenses/libcxx/LICENSE.TXT"), "libc++ 與 libc++abi 的授權（NDK 附的原文）")],
      "原樣內含，只剝掉除錯資訊",
      ["android"],
    ),
  ];
}

// ---------- Windows 元件 ----------

/**
 * Windows 版用到、由系統提供而不隨應用安裝的元件。DirectML.dll 也靠系統內建的那一份，
 * 不隨附、不另列；ONNX Runtime 本身列在「內含的字型、資料與引擎」。
 */
export function windowsComponents(texts) {
  return [
    item(
      "Microsoft Edge WebView2 執行環境",
      "系統內建（Evergreen）",
      "Microsoft 軟體授權條款",
      "https://developer.microsoft.com/microsoft-edge/webview2/",
      [
        texts.add(
          "WebView2 執行環境",
          "Microsoft Edge WebView2 執行環境是 Windows 的系統元件，由 Microsoft 依它自己的軟體授權條款提供與更新。PaperReader 不隨附、不修改它，只透過它顯示介面。",
        ),
      ],
      "顯示介面用；由 Windows 提供，不隨應用安裝",
    ),
  ];
}

export function fileComponents(texts) {
  const pv = pdfjsVersion();
  const pd = (rel) => texts.file(must(`node_modules/pdfjs-dist/${rel}`));
  const mpl = texts.standard("MPL-2.0", "");
  // 每一項內含的檔都要在（建置會複製或內含它們）
  for (const f of [
    "node_modules/pdfjs-dist/standard_fonts/FoxitSerif.pfb",
    `${LIBERATION}/LiberationSans-Regular.ttf`,
    `${LIBERATION}/LiberationSans-Bold.ttf`,
    `${LIBERATION}/LiberationSans-Italic.ttf`,
    `${LIBERATION}/LiberationSans-BoldItalic.ttf`,
    "node_modules/pdfjs-dist/cmaps/UniGB-UCS2-H.bcmap",
    "node_modules/pdfjs-dist/wasm/openjpeg.wasm",
    "node_modules/pdfjs-dist/wasm/jbig2.wasm",
    "node_modules/pdfjs-dist/wasm/qcms_bg.wasm",
    "node_modules/pdfjs-dist/wasm/quickjs-eval.wasm",
    "node_modules/pdfjs-dist/iccs/CGATS001Compat-v2-micro.icc",
    "core/csl/apa.csl",
    "core/csl/locales-zh-TW.xml",
    "public/bergamot/bergamot-translator.wasm",
  ]) {
    must(f);
  }
  const files = [
    item("pdf.js 標準字型（Foxit）", pv, "BSD-3-Clause", "https://pdfium.googlesource.com/pdfium/", [pd("standard_fonts/LICENSE_FOXIT")], "pdf.js 內附"),
    item(
      "Liberation Sans 字型（PDF 標準字型 Helvetica 的替代）",
      liberationVersion(),
      "OFL-1.1",
      "https://github.com/liberationfonts/liberation-fonts",
      [texts.file(must(`${LIBERATION}/LICENSE`), "SIL Open Font License 1.1"), texts.file(must(`${LIBERATION}/AUTHORS`), "Liberation Fonts 的作者名單")],
      // 關於頁會顯示這句說明，所以只寫給使用者看的內容
      "官方發布的原樣字型檔，取代 pdf.js 內附的 1.x",
    ),
    item("Adobe CMap 字元對照表", pv, "BSD-3-Clause", "https://github.com/adobe-type-tools/cmap-resources", [pd("cmaps/LICENSE")], "pdf.js 內附"),
    item(
      "JBIG2 解碼器（PDFium，編成 WASM）",
      pv,
      "BSD-3-Clause AND Apache-2.0",
      "https://pdfium.googlesource.com/pdfium/",
      [pd("wasm/LICENSE_JBIG2"), pd("wasm/LICENSE_PDFJS_JBIG2")],
      "pdf.js 內附",
    ),
    item(
      "OpenJPEG（編成 WASM）",
      pv,
      "BSD-2-Clause",
      "https://github.com/uclouvain/openjpeg",
      [pd("wasm/LICENSE_OPENJPEG"), pd("wasm/LICENSE_PDFJS_OPENJPEG")],
      "pdf.js 內附",
    ),
    item("qcms 色彩管理（編成 WASM）", pv, "MIT", "https://github.com/mozilla/pdf.js", [pd("wasm/LICENSE_QCMS"), pd("wasm/LICENSE_PDFJS_QCMS")], "pdf.js 內附"),
    item(
      "QuickJS（編成 WASM）",
      pv,
      "MIT",
      "https://bellard.org/quickjs/",
      [pd("wasm/LICENSE_QUICKJS"), pd("wasm/LICENSE_PDFJS_QUICKJS")],
      "pdf.js 內附",
    ),
    item("CGATS001Compat 色彩描述檔", pv, "CC0-1.0", "https://github.com/mozilla/pdf.js", [pd("iccs/LICENSE")], "pdf.js 內附"),
    item(
      "CSL 格式檔與語系檔（APA、IEEE、Chicago、MLA；英文、繁體中文）",
      cslCommits(),
      "CC-BY-SA-3.0",
      "https://citationstyles.org/",
      [texts.standard("CC-BY-SA-3.0", ""), texts.file(must("core/csl/README.md"), "CSL 檔案的來源與作者名單")],
    ),
    item(
      "Firefox 翻譯 bergamot-translator（JS 接口與 WASM 引擎）",
      bergamotVersion(),
      "MPL-2.0",
      "https://github.com/mozilla/translations",
      [mpl],
    ),
  ];
  const download = [
    item(
      "Firefox 翻譯的語言模型（英→繁中、英→簡中）",
      modelVersion(),
      "MPL-2.0",
      "https://github.com/mozilla/translations",
      [mpl],
    ),
    item(
      "PP-DocLayoutV3 版面模型（PaddlePaddle）",
      layoutModelVersion(),
      "Apache-2.0",
      "https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_onnx",
      [texts.standard("Apache-2.0", "")],
      "只用在翻譯的分段",
    ),
  ];
  files.push(...onnxRuntimeComponents(texts));
  files.push(sqliteComponent(texts));
  return { files: files.sort(byName), download };
}

const LIBSQLITE3_SYS = "0.40.0";
const SQLITE = "3.54.1";

/**
 * libsqlite3-sys 以 `bundled` 編進來的 SQLite 本身（內文搜尋的索引）。libsqlite3-sys 這個套件（MIT）由 Rust 那一段
 * 列出；它內含的 SQLite C 程式是公有領域，這裡另列一項。版本照 libsqlite3-sys 0.40.0 內附的 `sqlite3/sqlite3.h`；
 * `Cargo.lock` 的 libsqlite3-sys 換版本時建置失敗，要重新核對。
 */
function sqliteComponent(texts) {
  const lock = read(must("Cargo.lock"));
  const v = /name = "libsqlite3-sys"\nversion = "([^"]+)"/.exec(lock)?.[1];
  if (v !== LIBSQLITE3_SYS) throw new Error(`授權清單：libsqlite3-sys 是 ${v ?? "（沒有）"}，SQLite 的版本只核對過 ${LIBSQLITE3_SYS}`);
  const blessing = texts.add(
    "SQLite",
    "The author disclaims copyright to this source code. In place of a legal notice, here is a blessing:\n\n" +
      "May you do good and not evil.\nMay you find forgiveness for yourself and forgive others.\nMay you share freely, never taking more than you give.",
  );
  return item("SQLite", SQLITE, "Public Domain", "https://sqlite.org/", [blessing], "libsqlite3-sys 內含（內文搜尋的索引）");
}

/** 版面模型的版本：外殼下載時釘住的 Hugging Face 版本（`src-tauri/src/commands/layout.rs` 的網址）。 */
function layoutModelVersion() {
  const rs = read(must("src-tauri/src/commands/layout.rs"));
  const v = /PP-DocLayoutV3_onnx\/resolve\/([0-9a-f]{40})\//.exec(rs)?.[1];
  if (!v) throw new Error("授權清單：layout.rs 裡找不到版面模型的版本");
  return v.slice(0, 12);
}

// ---------- 合起來 ----------

/** 產生「關於」頁要的全部資料（不寫檔）。 */
export function collect() {
  const texts = new Texts();
  // 應用本身：著作權聲明（NOTICE，Apache-2.0 第 4 節 d）在前，授權全文在後
  const app = {
    name: "PaperReader",
    license: "Apache-2.0",
    texts: [texts.file(must("../NOTICE"), "NOTICE"), texts.file(must("../LICENSE"), "LICENSE")],
  };
  const { files, download } = fileComponents(texts);
  const groups = [
    { id: "ui", items: npmComponents(texts) },
    { id: "rust", items: rustComponents(texts) },
    { id: "android", platforms: ["android"], items: androidComponents(texts) },
    { id: "windows", platforms: ["windows"], items: windowsComponents(texts) },
    { id: "files", items: files },
    { id: "download", items: download },
  ];
  return { app, groups, texts: texts.toJSON() };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const data = collect();
  const out = join(APP, "public", "licenses.json");
  mkdirSync(dirname(out), { recursive: true });
  const json = JSON.stringify(data);
  writeFileSync(out, json);
  const n = data.groups.reduce((s, g) => s + g.items.length, 0);
  console.log(`授權清單已產生：${n} 項組件、${Object.keys(data.texts).length} 份授權全文，${Math.round(json.length / 1024)} KB → public/licenses.json`);
}
