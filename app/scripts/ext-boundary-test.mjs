// 外掛邊界的檢查腳本（`scripts/ext-boundary.mjs`）：在暫存資料夾做一個假的 app，
// 外掛名稱也是假的，登記處以外加一行引用外掛，檢查要失敗並列出那一行；允許的檔不算。
// 執行：node --test scripts/ext-boundary-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { findViolations, pluginNames } from "./ext-boundary.mjs";

const script = path.join(path.dirname(fileURLToPath(import.meta.url)), "ext-boundary.mjs");

function fakeApp() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "ext-boundary-"));
  const put = (rel, text) => {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    fs.writeFileSync(path.join(root, rel), text);
  };
  // 外掛自己的兩個資料夾（裡面怎麼寫都可以）
  put("ext/demo/src/lib.rs", "//! paperreader-ext-demo\n");
  put("src/ext/demo/index.ts", 'export const demo = { name: "demo" };\n');
  // 允許的檔
  put("src/ext/index.ts", 'import { demo } from "./demo";\nexport const plugins = [demo];\n');
  put("src-tauri/Cargo.toml", 'paperreader-ext-demo = { path = "../ext/demo" }\n');
  put("src-tauri/src/ext.rs", "use paperreader_ext_demo as demo;\n");
  put("Cargo.lock", 'name = "paperreader-ext-demo"\n');
  // 一般的檔：提到「ext/」或別的字不算
  put("src/screens/Reader.tsx", 'import { plugins } from "../ext";\nconst demonstration = "ext/demonstration";\n');
  put("node_modules/x/index.js", 'require("../../ext/demo");\n');
  return { root, put };
}

test("外掛名稱取自兩個資料夾，不寫死", () => {
  const { root } = fakeApp();
  assert.deepEqual(pluginNames(root), ["demo"]);
});

test("登記處與允許的檔、外掛自己的資料夾、略過的資料夾都不算", () => {
  const { root } = fakeApp();
  assert.deepEqual(findViolations(root), []);
  const out = execFileSync(process.execPath, [script, root], { encoding: "utf8" });
  assert.match(out, /demo/);
});

test("登記處以外加一行引用外掛：失敗並列出檔案與行號", () => {
  const { root, put } = fakeApp();
  put("src/screens/Info.tsx", 'import { x } from "./y";\nimport { demo } from "../ext/demo";\n');
  put("src/ext/InfoSections.tsx", 'import { thing } from "./demo/thing";\n');
  put("src-tauri/src/lib.rs", "use paperreader_ext_demo::Index;\n");
  const bad = findViolations(root).map((b) => `${b.file}:${b.line}`);
  assert.deepEqual(bad.sort(), ["src-tauri/src/lib.rs:1", "src/ext/InfoSections.tsx:1", "src/screens/Info.tsx:2"]);
  let failed = null;
  try {
    execFileSync(process.execPath, [script, root], { encoding: "utf8", stdio: "pipe" });
  } catch (e) {
    failed = e;
  }
  assert.ok(failed, "要失敗");
  assert.equal(failed.status, 1);
  assert.match(String(failed.stderr), /src\/screens\/Info\.tsx:2/);
});
