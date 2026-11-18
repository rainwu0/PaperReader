// 介面相依的已知漏洞：跑 npm audit，列在 audit-exceptions.json 的通報放行，其他一律算失敗。
// 用法（在 app 底下）：node scripts/npm-audit.mjs
// 例外的寫法比照 deny.toml 的 ignore：按通報編號列、每則寫理由與何時再看。
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const appDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const allowed = new Map(
  JSON.parse(readFileSync(join(appDir, "audit-exceptions.json"), "utf8")).exceptions.map((e) => [e.id, e]),
);

let raw;
try {
  raw = execSync("npm audit --json", { cwd: appDir, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 64 * 1024 * 1024 });
} catch (e) {
  // npm audit 找到漏洞時結束碼不是 0，輸出仍是 JSON。
  raw = e.stdout;
}

let report;
try {
  report = JSON.parse(raw);
} catch {
  console.error("npm audit 的輸出不是 JSON（可能沒有網路）：");
  console.error(String(raw).slice(0, 2000));
  process.exit(1);
}
if (report.error) {
  console.error("npm audit 失敗：", JSON.stringify(report.error));
  process.exit(1);
}

// 每個有漏洞的套件，via 裡的物件是通報本身，字串是「經由哪個套件」。
const advisories = new Map();
for (const v of Object.values(report.vulnerabilities ?? {})) {
  for (const via of v.via ?? []) {
    if (typeof via !== "object") continue;
    const id = String(via.url ?? "").split("/").pop() || String(via.source);
    advisories.set(id, { id, name: via.name, severity: via.severity, title: via.title, url: via.url });
  }
}

const blocked = [...advisories.values()].filter((a) => !allowed.has(a.id));
const excused = [...advisories.values()].filter((a) => allowed.has(a.id));

for (const a of excused) console.log(`例外放行：${a.id}（${a.name}，${a.severity}）：${allowed.get(a.id).reason}`);
for (const id of allowed.keys()) {
  if (!advisories.has(id)) console.log(`提醒：例外 ${id} 已經不在 npm audit 的結果裡，可以從 audit-exceptions.json 拿掉。`);
}
if (blocked.length > 0) {
  console.log("");
  for (const a of blocked) console.log(`未處理的通報：${a.id}（${a.name}，${a.severity}）${a.title} ${a.url ?? ""}`);
  console.log(`共 ${blocked.length} 則未處理。修好的版本有的話更新相依；沒有而且碰不到的，寫進 audit-exceptions.json 並附理由。`);
  process.exit(1);
}
console.log(`介面相依的漏洞：${advisories.size} 則通報，${excused.length} 則在例外清單，其餘 0 則。`);
