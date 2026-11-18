// 外掛的邊界：登記處以外沒有任何檔案引用外掛。
//
// 1. 外掛名稱不寫死：取 `ext/` 與 `src/ext/` 底下每個子資料夾的名稱 N。
// 2. 搜 app 底下的文字檔（略過 node_modules、target、dist、gen 與外掛自己的兩個資料夾），比對
//    `paperreader[-_]ext[-_]N` 與 `ext[/\]N`（後面不接字母數字）；在 `src/ext/` 底下另比對 `from "./N`、`from './N`。
// 3. 允許出現的檔：`src-tauri/Cargo.toml`、`Cargo.lock`、`src-tauri/src/ext.rs`、`src/ext/index.ts`。
// 4. 其他檔有任何一處就失敗，列出檔案與行號。
//
// 執行（在 app 底下）：node scripts/ext-boundary.mjs [app 資料夾]

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SKIP_DIRS = new Set(["node_modules", "target", "dist", "gen", ".git"]);
const ALLOWED = new Set(["src-tauri/Cargo.toml", "Cargo.lock", "src-tauri/src/ext.rs", "src/ext/index.ts"]);
const TEXT = /\.(?:ts|tsx|js|mjs|cjs|jsx|rs|toml|lock|json|css|html|md|ps1|sh|kt|kts|gradle|xml|yml|yaml|txt)$/i;

function subdirs(dir) {
  if (!fs.existsSync(dir)) return [];
  return fs
    .readdirSync(dir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name);
}

/** 外掛的名稱：`ext/` 與 `src/ext/` 底下的子資料夾。 */
export function pluginNames(app) {
  return [...new Set([...subdirs(path.join(app, "ext")), ...subdirs(path.join(app, "src", "ext"))])].sort();
}

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** 找出登記處以外引用外掛的地方：`[{ file, line, text, plugin }]`（`file` 是相對 app 的路徑，用 `/`）。 */
export function findViolations(app) {
  const names = pluginNames(app);
  const own = new Set(names.flatMap((n) => [`ext/${n}`, `src/ext/${n}`]));
  const patterns = names.map((n) => ({
    name: n,
    any: new RegExp(String.raw`paperreader[-_]ext[-_]${escape(n)}(?![\w-])|ext[/\\]${escape(n)}(?![\p{L}\p{N}_])`, "u"),
    local: new RegExp(String.raw`from\s+["']\./${escape(n)}(?![\p{L}\p{N}_])`, "u"),
  }));
  const out = [];
  const walk = (dir, rel) => {
    for (const d of fs.readdirSync(dir, { withFileTypes: true })) {
      const r = rel ? `${rel}/${d.name}` : d.name;
      if (d.isDirectory()) {
        if (SKIP_DIRS.has(d.name) || own.has(r)) continue;
        walk(path.join(dir, d.name), r);
      } else if (d.isFile() && TEXT.test(d.name) && !ALLOWED.has(r)) {
        const text = fs.readFileSync(path.join(dir, d.name), "utf8");
        const inSrcExt = r.startsWith("src/ext/");
        text.split(/\r?\n/).forEach((line, i) => {
          for (const p of patterns) {
            if (p.any.test(line) || (inSrcExt && p.local.test(line))) {
              out.push({ file: r, line: i + 1, text: line.trim(), plugin: p.name });
            }
          }
        });
      }
    }
  };
  walk(app, "");
  return out;
}

const here = path.dirname(fileURLToPath(import.meta.url));
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const app = path.resolve(process.argv[2] ?? path.join(here, ".."));
  const names = pluginNames(app);
  const bad = findViolations(app);
  if (bad.length > 0) {
    console.error(`登記處以外引用了外掛（${bad.length} 處）：`);
    for (const b of bad) console.error(`  ${b.file}:${b.line}  [${b.plugin}]  ${b.text}`);
    process.exit(1);
  }
  console.log(`外掛的邊界：${names.length} 個外掛（${names.join("、") || "沒有"}），登記處以外沒有引用。`);
}
