// 介面相依（npm）的授權檢查：讀 `node_modules` 裡每個套件 `package.json` 的授權欄位，對照允許清單。
// 和 Rust 那邊的 cargo-deny（`app/deny.toml`）用同一份寬鬆授權；npm 沒有內建授權檢查，所以自己寫。
// 執行：node scripts/npm-licenses.mjs（在 app 底下；check.ps1 的一步）。有不在清單裡的授權就列出來、結束碼 1。
//
// 範圍：package-lock.json 列的、這台電腦實際裝了的套件，包含只在開發時用的工具。
// 其他平台才裝的選用套件（例如別的作業系統的原生檔）這台沒有，不檢查。

import { readFileSync } from "node:fs";
import { join } from "node:path";

/** 和 `app/deny.toml` 的 `[licenses] allow` 相同（MPL-2.0：原樣使用、不修改），另加一項：
 *  0BSD：tslib（pdf-lib、motion 帶進來的）。比 MIT 還寬鬆、連保留聲明都不要求。 */
const ALLOW = new Set([
  "MIT",
  "Apache-2.0",
  "Unicode-3.0",
  "Zlib",
  "BSD-3-Clause",
  "ISC",
  "CDLA-Permissive-2.0",
  "MPL-2.0",
  "0BSD",
]);

/** SPDX 授權運算式（`MIT OR Apache-2.0`、`(MIT AND Zlib)`）是否只用到允許的授權：OR 取一個可以的，AND 要全部可以。 */
function allowed(expr) {
  const tokens = expr.replace(/[()]/g, (p) => ` ${p} `).trim().split(/\s+/);
  let i = 0;
  const atom = () => {
    const tok = tokens[i++];
    if (tok === "(") {
      const v = or();
      i++; // ")"
      return v;
    }
    // 「X WITH 例外」：只有 X 在清單裡還不夠，例外條款要另外看，所以一律算不允許
    if (tokens[i] === "WITH") {
      i += 2;
      return false;
    }
    return ALLOW.has(tok ?? "");
  };
  const and = () => {
    let v = atom();
    while (tokens[i] === "AND") {
      i++;
      v = atom() && v;
    }
    return v;
  };
  const or = () => {
    let v = and();
    while (tokens[i] === "OR") {
      i++;
      v = and() || v;
    }
    return v;
  };
  return or() && i === tokens.length;
}

/** 套件宣告的授權：`license` 字串；舊的寫法 `license: {type}` 或 `licenses: [{type}]` 也接受（多個算「或」）。 */
function declared(pkg) {
  if (typeof pkg.license === "string") return pkg.license;
  if (pkg.license && typeof pkg.license.type === "string") return pkg.license.type;
  if (Array.isArray(pkg.licenses)) return pkg.licenses.map((l) => l.type ?? l).join(" OR ");
  return null;
}

function main() {
  const app = join(import.meta.dirname, "..");
  const lock = JSON.parse(readFileSync(join(app, "package-lock.json"), "utf8"));
  const bad = [];
  let checked = 0;
  for (const path of Object.keys(lock.packages ?? {})) {
    if (path === "") continue; // 應用自己
    let pkg;
    try {
      pkg = JSON.parse(readFileSync(join(app, path, "package.json"), "utf8"));
    } catch {
      continue; // 這台沒裝（別的平台的選用套件）
    }
    checked++;
    const license = declared(pkg);
    if (license === null || !allowed(license)) {
      bad.push(`${pkg.name ?? path}@${pkg.version ?? "?"}：${license ?? "（沒有寫授權）"}`);
    }
  }
  if (bad.length > 0) {
    console.log(`有 ${bad.length} 個套件的授權不在允許清單裡（共檢查 ${checked} 個）：`);
    for (const b of bad) console.log(`  ${b}`);
    console.log("先看授權條款再決定；確定可以用，就加進這支程式的 ALLOW。");
    process.exit(1);
  }
  console.log(`npm 相依的授權：${checked} 個套件都在允許清單裡。`);
}

main();
