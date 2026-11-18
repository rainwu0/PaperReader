// 版號檢查（version-check.mjs）與版號更新（bump-version.mjs）的測試。規則見 規格/05_版號規則.md。
// 執行：node --test scripts/version-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { bump, updateChangelog } from "./bump-version.mjs";
import { checkApp, parsePublicVersion, parseVersion, toPublicVersion, toToolVersion, validate, versionCodeOf } from "./version-check.mjs";

const appDir = join(dirname(fileURLToPath(import.meta.url)), "..");

const good = (over = {}) => ({
  tauri: "0.1.12",
  cargo: "0.1.12",
  pkg: "0.1.12",
  members: [],
  versionCode: 1001200,
  gradle: `        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0").replace('+', '.')
`,
  problems: [],
  ...over,
});

test("versionCode 公式：規格/05 第 5 節的例子", () => {
  const code = (v) => versionCodeOf(parseVersion(v));
  assert.equal(code("0.1.0"), 1_000_000);
  assert.equal(code("0.1.1"), 1_000_100);
  assert.equal(code("0.1.1+1"), 1_000_101);
  assert.equal(code("0.2.0"), 2_000_000);
  assert.equal(code("1.0.0"), 100_000_000);
  assert.equal(code("20.99.9999+99"), 2_099_999_999);
});

test("合規的版號通過", () => {
  assert.deepEqual(validate(good()), []);
  assert.deepEqual(validate(good({ tauri: "0.1.12+3", cargo: "0.1.12+3", pkg: "0.1.12+3", versionCode: 1001203 })), []);
  assert.deepEqual(validate(good({ tauri: "1.0.0", cargo: "1.0.0", pkg: "1.0.0", versionCode: 100_000_000 })), []);
});

test("補零不通過（含 d 的補零）", () => {
  for (const v of ["0.01.0", "0.1.012", "00.1.0", "0.1.1+01"]) {
    assert.ok(parseVersion(v).error, `${v} 應該不合規格`);
  }
  const errors = validate(good({ tauri: "0.1.012", cargo: "0.1.012", pkg: "0.1.012" }));
  assert.ok(errors.some((e) => e.includes("補零")));
});

test("+0 不通過", () => {
  assert.match(parseVersion("0.1.1+0").error, /\+0/);
  const errors = validate(good({ tauri: "0.1.12+0", cargo: "0.1.12+0", pkg: "0.1.12+0" }));
  assert.ok(errors.some((e) => e.includes("+0")));
});

test("其他不合格的寫法不通過", () => {
  for (const v of ["0.1", "0.1.0.1", "0.1.0_1", "0.1.0-rc.1", "v0.1.0", "0.1.0+", "0.1.0+1+2", ""]) {
    assert.ok(parseVersion(v).error, `「${v}」應該不合規格`);
  }
});

test("超過上限不通過；剛好在上限內通過", () => {
  for (const v of ["21.0.0", "0.100.0", "0.1.10000", "0.1.1+100"]) {
    assert.match(parseVersion(v).error, /超過上限/, v);
  }
  for (const v of ["20.0.0", "0.99.0", "0.1.9999", "0.1.1+99"]) {
    assert.equal(parseVersion(v).error, undefined, v);
  }
});

test("三處版號不同不通過，錯誤列出各處的值", () => {
  const errors = validate(good({ pkg: "0.1.11" }));
  assert.equal(errors.length, 1);
  assert.match(errors[0], /不一致/);
  assert.match(errors[0], /package\.json.*0\.1\.11/);
  assert.ok(validate(good({ cargo: "0.1.13" })).some((e) => e.includes("不一致")));
  assert.ok(validate(good({ members: [{ file: "x/Cargo.toml", version: "0.1.11" }] })).some((e) => e.includes("不一致")));
});

test("對外的版號與設定檔的版號互換：+ 只在設定檔裡", () => {
  assert.equal(toPublicVersion("0.1.12+3"), "0.1.12.3");
  assert.equal(toPublicVersion("0.1.12"), "0.1.12");
  assert.equal(toToolVersion("0.1.12.3"), "0.1.12+3");
  assert.equal(toToolVersion("0.1.12"), "0.1.12");
  assert.deepEqual(parsePublicVersion("0.1.12.3"), { a: 0, b: 1, c: 12, d: 3 });
  assert.match(parsePublicVersion("0.1.12+3").error, /用點/);
  assert.match(parsePublicVersion("0.1.12.0").error, /只寫三段/);
  assert.match(parsePublicVersion("0.1.12.03").error, /補零/);
  assert.match(parsePublicVersion("0.1.12.100").error, /上限/);
  assert.ok(parsePublicVersion("0.1.12.3.4").error);
  assert.throws(() => toToolVersion("0.1.12+3"));
});

test("Android versionName 的覆寫不見了就不通過", () => {
  const gone = validate(good({ gradle: `        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
` }));
  assert.equal(gone.length, 1);
  assert.match(gone[0], /versionName/);
  assert.ok(validate(good({ gradle: "" })).length > 0);
});

test("versionCode 錯、缺、不是整數都不通過", () => {
  const wrong = validate(good({ versionCode: 1012 }));
  assert.equal(wrong.length, 1);
  assert.match(wrong[0], /1012.*1001200/);
  assert.ok(validate(good({ versionCode: undefined })).some((e) => e.includes("沒有 bundle.android.versionCode")));
  assert.ok(validate(good({ versionCode: "1001200" })).some((e) => e.includes("不是整數")));
  // 丟掉 d 的預設算法會被抓到
  assert.ok(validate(good({ tauri: "0.1.1+1", cargo: "0.1.1+1", pkg: "0.1.1+1", versionCode: 1_000_100 })).length > 0);
});

test("這個版本庫現在的版號通過檢查", () => {
  const r = checkApp(appDir);
  assert.deepEqual(r.errors, []);
  assert.match(r.summary, /versionCode/);
});

// ---------- bump-version ----------

const CHANGELOG = `# 更新紀錄

說明文字。

## [Unreleased]

### Added

- 新功能甲

### Fixed

- 修正乙

## [0.1.0] - 2026-11-18

第一個預覽版。
`;

test("更新紀錄：Unreleased 的內容移到新的一節，留空的 Unreleased", () => {
  const out = updateChangelog(CHANGELOG, "0.1.1", "2026-12-01");
  assert.equal(
    out,
    `# 更新紀錄

說明文字。

## [Unreleased]

## [0.1.1] - 2026-12-01

### Added

- 新功能甲

### Fixed

- 修正乙

## [0.1.0] - 2026-11-18

第一個預覽版。
`,
  );
});

test("更新紀錄：Unreleased 是空的、或該版已存在", () => {
  const empty = CHANGELOG.replace(/### Added[\s\S]*?(?=## \[0\.1\.0\])/, "");
  const out = updateChangelog(empty, "0.1.1", "2026-12-01");
  assert.match(out, /## \[Unreleased\]\n\n## \[0\.1\.1\] - 2026-12-01\n\n## \[0\.1\.0\]/);
  assert.throws(() => updateChangelog(CHANGELOG, "0.1.0", "2026-12-01"), /已經有/);
  assert.throws(() => updateChangelog("# x\n", "0.1.1", "2026-12-01"), /Unreleased/);
});

/** 把真正的設定檔複製到暫存資料夾，在那裡試 bump。 */
function sandbox() {
  const root = mkdtempSync(join(tmpdir(), "version-test-"));
  const app = join(root, "app");
  mkdirSync(join(app, "src-tauri"), { recursive: true });
  mkdirSync(join(app, "src-tauri", "gen", "android", "app"), { recursive: true });
  for (const f of ["Cargo.toml", "Cargo.lock", "package.json", "package-lock.json", "src-tauri/tauri.conf.json", "src-tauri/gen/android/app/build.gradle.kts"]) {
    cpSync(join(appDir, f), join(app, f));
  }
  writeFileSync(join(root, "CHANGELOG.md"), CHANGELOG);
  return { root, app, changelog: join(root, "CHANGELOG.md") };
}
const text = (p) => readFileSync(p, "utf8");

test("bump：改好各處、versionCode 照公式、更新紀錄換節；之後檢查通過", () => {
  const sb = sandbox();
  try {
    // 現在的版號從真正的設定檔讀，不寫死：專案每發一版它就變。新版號用一個一定比它大的。
    const curTool = JSON.parse(text(join(appDir, "src-tauri", "tauri.conf.json"))).version;
    const r = bump("9.0.0.1", sb, "2026-12-01");
    assert.deepEqual(r, { oldVersion: toPublicVersion(curTool), version: "9.0.0.1", toolVersion: "9.0.0+1", versionCode: 900_000_001 });
    assert.equal(JSON.parse(text(join(sb.app, "src-tauri", "tauri.conf.json"))).version, "9.0.0+1");
    assert.deepEqual(checkApp(sb.app).errors, []);
    assert.equal(JSON.parse(text(join(sb.app, "package-lock.json"))).packages[""].version, "9.0.0+1");
    assert.match(text(join(sb.app, "Cargo.lock")), /name = "paperreader-core"\nversion = "9\.0\.0\+1"/);
    // 第三方套件的版號不動：和原檔比，只有我們自己的套件那幾行不同
    const before = text(join(appDir, "Cargo.lock")).split("\n");
    const after = text(join(sb.app, "Cargo.lock")).split("\n");
    assert.equal(before.length, after.length);
    const changed = before.filter((l, i) => l !== after[i]);
    assert.ok(changed.length > 0 && changed.every((l) => l === `version = "${curTool}"`));
    assert.match(text(sb.changelog), /## \[Unreleased\]\n\n## \[9\.0\.0\.1\] - 2026-12-01\n\n### Added/);
  } finally {
    rmSync(sb.root, { recursive: true, force: true });
  }
});

test("bump：不合規格、不比現在新的版號被拒絕，而且什麼都沒改", () => {
  const sb = sandbox();
  try {
    const snapshot = () => ["src-tauri/tauri.conf.json", "Cargo.toml", "Cargo.lock", "package.json", "package-lock.json"].map((f) => text(join(sb.app, f))).join("\0") + text(sb.changelog);
    const before = snapshot();
    assert.throws(() => bump("0.1.01", sb), /補零/);
    assert.throws(() => bump("0.1.1+1", sb), /用點/);
    assert.throws(() => bump("0.1.1.0", sb), /只寫三段/);
    assert.throws(() => bump("0.1.10000", sb), /上限/);
    assert.throws(() => bump("0.1.0", sb), /不比現在的/);
    assert.throws(() => bump("0.0.9", sb), /不比現在的/);
    assert.equal(snapshot(), before);
  } finally {
    rmSync(sb.root, { recursive: true, force: true });
  }
});
