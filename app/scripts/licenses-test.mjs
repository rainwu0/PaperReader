// 「關於」頁的組件與授權清單（`scripts/licenses.mjs`）的測試：清單是從鎖定檔產生的，這裡確認該有的都在、不該有的不在。
// 執行：node --test scripts/licenses-test.mjs（在 app 底下；要有 cargo 與 npm ci 裝好的 node_modules）

import assert from "node:assert/strict";
import { test } from "node:test";
import { collect, spdxIds } from "./licenses.mjs";

const data = collect();
const group = (id) => data.groups.find((g) => g.id === id)?.items ?? [];
const names = (id) => group(id).map((i) => i.name);
const all = data.groups.flatMap((g) => g.items);

test("授權式子拆成代號", () => {
  assert.deepEqual(spdxIds("MIT OR Apache-2.0"), ["MIT", "Apache-2.0"]);
  assert.deepEqual(spdxIds("Apache-2.0/MIT"), ["Apache-2.0", "MIT"]);
  assert.deepEqual(spdxIds("(MIT AND Zlib)"), ["MIT", "Zlib"]);
});

test("應用本身是 Apache-2.0：先附著作權聲明（NOTICE），再附全文", () => {
  assert.equal(data.app.license, "Apache-2.0");
  assert.match(data.texts[data.app.texts[0]].body, /Copyright 2026 rainwu0/);
  assert.match(data.texts[data.app.texts[1]].body, /Apache License\s+Version 2\.0/);
});

test("Rust：直接用的與帶進來的都在；我們自己的套件、只在建置或測試用的不在", () => {
  const rust = names("rust");
  for (const n of ["tauri", "serde", "hayagriva", "genai", "reqwest", "rustls", "ring", "webpki-roots", "zip", "lopdf", "ruzstd", "tokio"]) {
    assert.ok(rust.includes(n), `缺 ${n}`);
  }
  for (const n of ["paperreader-core", "paperreader-app", "paperreader-translate", "tauri-plugin-paperreader", "tempfile", "tauri-build"]) {
    assert.ok(!rust.includes(n), `不該列 ${n}`);
  }
});

test("Android 與 Windows 共用一份清單：只有一邊有的標 platforms", () => {
  const rust = group("rust");
  const find = (n) => rust.find((i) => i.name === n);
  // 兩邊都有的不標
  for (const n of ["tauri", "serde", "zip"]) assert.equal(find(n)?.platforms, undefined, n);
  // 電腦的平台轉接器用的
  for (const n of ["tauri-plugin-dialog", "tauri-plugin-opener", "rfd"]) assert.deepEqual(find(n)?.platforms, ["windows"], n);
  // 只在 Android 的
  assert.deepEqual(find("jni")?.platforms, ["android"]);
  const android = data.groups.find((g) => g.id === "android");
  assert.deepEqual(android.platforms, ["android"]);
  const windows = data.groups.find((g) => g.id === "windows");
  assert.deepEqual(windows.platforms, ["windows"]);
  assert.ok(windows.items.some((i) => i.name.includes("WebView2")));
  // DirectML 不隨附，不列
  assert.ok(!all.some((i) => /DirectML/i.test(i.name)));
  // libc++_shared.so 只在 Android 的安裝檔
  assert.deepEqual(group("files").find((i) => i.name.startsWith("libc++")).platforms, ["android"]);
});

test("畫面：編進安裝檔的套件在；開發工具、Node 才用的原生模組、型別宣告不在", () => {
  const ui = names("ui");
  for (const n of ["react", "react-dom", "pdfjs-dist", "pdf-lib", "@pdf-lib/fontkit", "micromark", "motion", "perfect-freehand", "@codemirror/view", "@tauri-apps/api"]) {
    assert.ok(ui.includes(n), `缺 ${n}`);
  }
  for (const n of ["vite", "typescript", "oxlint", "@tauri-apps/cli", "@napi-rs/canvas", "@types/react", "@types/debug"]) {
    assert.ok(!ui.includes(n), `不該列 ${n}`);
  }
});

test("MPL-2.0 的組件都註明原始碼的網址", () => {
  const mpl = all.filter((i) => i.mpl);
  for (const n of ["cssparser", "selectors", "option-ext", "dtoa-short"]) {
    assert.ok(mpl.some((i) => i.name === n), `缺 MPL 的 ${n}`);
  }
  assert.ok(mpl.some((i) => i.name.includes("bergamot")), "缺 Firefox 翻譯的引擎");
  for (const i of mpl) assert.match(i.source ?? "", /^https:\/\//, `${i.name} 沒有原始碼網址`);
});

test("CSL 格式檔註明並連到 citationstyles.org，附作者名單", () => {
  const csl = group("files").find((i) => i.name.startsWith("CSL"));
  assert.ok(csl);
  assert.equal(csl.license, "CC-BY-SA-3.0");
  assert.equal(csl.source, "https://citationstyles.org/");
  const bodies = csl.texts.map((id) => data.texts[id].body).join("\n");
  assert.match(bodies, /creativecommons\.org\/licenses\/by-sa\/3\.0/);
  assert.match(bodies, /Brenton M\. Wiernik/);
});

test("Liberation Sans 是官方 2.x、SIL OFL 1.1，不再是 1.x 的 GPL 授權", () => {
  const lib = group("files").filter((i) => i.name.includes("Liberation"));
  assert.equal(lib.length, 1);
  const [it] = lib;
  assert.equal(it.license, "OFL-1.1");
  assert.match(it.version, /^2\.\d+\.\d+$/);
  assert.equal(it.source, "https://github.com/liberationfonts/liberation-fonts");
  const bodies = it.texts.map((id) => data.texts[id].body).join("\n");
  assert.match(bodies, /SIL OPEN FONT LICENSE Version 1\.1/);
  assert.match(bodies, /Reserved Font Name Liberation/);
  // 1.x 的授權檔（Red Hat 的 GPL-2.0 加字型例外）不在任何一份全文裡
  for (const t of Object.values(data.texts)) assert.doesNotMatch(t.body, /LIBERATION FONT SOFTWARE/);
});

test("版面模型：ONNX Runtime 與它帶進的組件、libc++、下載的模型都列出", () => {
  const files = group("files");
  const find = (prefix) => files.find((i) => i.name.startsWith(prefix));
  const body = (i) => i.texts.map((id) => data.texts[id].body).join("\n");
  const ort = find("ONNX Runtime");
  assert.equal(ort.license, "MIT");
  assert.match(body(ort), /Copyright \(c\) Microsoft Corporation/);
  const cpuinfo = find("cpuinfo");
  assert.equal(cpuinfo.license, "BSD-2-Clause");
  assert.match(body(cpuinfo), /Marat Dukhan/);
  assert.match(body(cpuinfo), /Redistributions in binary form must reproduce/);
  assert.doesNotMatch(body(cpuinfo), /Neither the name/);
  assert.equal(find("Boost.MP11").license, "BSL-1.0");
  assert.match(body(find("Boost.MP11")), /machine-executable object code/);
  const eigen = find("Eigen");
  assert.ok(eigen.mpl);
  assert.match(eigen.source, /1d8b82b0740839c0de7f1242a3585e3390ff5f33/);
  const cxx = find("libc++");
  assert.match(body(cxx), /LLVM Exceptions/);
  assert.match(body(cxx), /libc\+\+abi/);
  for (const n of ["ONNX", "Abseil", "Protocol Buffers", "RE2", "FlatBuffers", "Microsoft GSL", "nlohmann/json", "SafeInt", "MurmurHash3", "KleidiAI"]) {
    assert.ok(find(n), `缺 ${n}`);
  }
  const model = group("download").find((i) => i.name.startsWith("PP-DocLayoutV3"));
  assert.equal(model.license, "Apache-2.0");
  assert.match(model.source, /huggingface\.co\/PaddlePaddle\/PP-DocLayoutV3_onnx/);
});

test("每一項都有版本、授權與至少一份全文；全文都找得到", () => {
  for (const i of all) {
    assert.ok(i.version, `${i.name} 沒有版本`);
    assert.ok(i.license, `${i.name} 沒有授權`);
    assert.ok(i.texts.length > 0, `${i.name} 沒有全文`);
    for (const id of i.texts) assert.ok(data.texts[id]?.body.length > 0, `${i.name} 的全文 ${id} 是空的`);
  }
});

test("沒附授權檔的套件用標準全文，著作權那一行填上作者", () => {
  const sel = group("rust").find((i) => i.name === "selectors");
  assert.match(data.texts[sel.texts[0]].body, /Mozilla Public License Version 2\.0/);
  const alloc = group("rust").find((i) => i.name === "alloc-stdlib");
  const body = data.texts[alloc.texts[0]].body;
  assert.doesNotMatch(body, /\{copyright\}/);
  assert.match(body, /Redistribution and use in source and binary forms/);
});
