// 驗收用的假 GitHub（應用內更新）：模擬 Releases API 與附件下載，不連任何真的服務、不對公開版本庫建發布。
//
// 用法：node scripts/fake-github-server.mjs --dir <放安裝檔的資料夾> --tag v0.1.2 [--port 8790] [--asset-port 8791]
//        [--mode ok|corrupt|ratelimit] [--notes "變更摘要"]
//   - 資料夾裡的 PaperReader-<版號>-<架構>.apk 都當成這個標籤的附件；SHA256SUMS.txt 由這裡依檔案內容算。
//   - 除錯版在編譯時設 PAPERREADER_UPDATE_BASE=http://10.0.2.2:<port>（模擬器連到電腦的位址），應用就問這裡。
//
// 和 GitHub 一樣的地方（讓測試抓得到真的會遇到的情況）：
//   - 沒有 User-Agent 的請求回 403；清單裡有草稿、標籤不合規格的、沒有附件的舊版；發布都標成預覽版（prerelease）。
//   - 附件的下載網址在 /rainwu0/PaperReader/releases/download/<標籤>/<檔名>，回 302 轉到另一個來源（另一個埠）。
//   - 附件帶 GitHub 的 `digest`（sha256:…）。
// 模式：`corrupt` 下載時把安裝檔的一個位元組改掉（核對碼與 digest 照真的算，驗下載後的核對）；
//       `ratelimit` 清單回 403（不帶金鑰的請求每小時上限）。
// 每個請求印一行（方法、路徑、User-Agent、Referer），驗「使用者拒絕連網時一個請求都沒有」與「轉址不帶 Referer」。
// 只開在 127.0.0.1（同一個網路的其他裝置連不到測試金鑰簽的安裝檔）；模擬器的 10.0.2.2 就是電腦的本機位址。
// 驗「發布被撤下」：用別的 --tag 重開，原本那一版的附件就回 404。

import { createHash } from "node:crypto";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";

const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : fallback;
};
const dir = opt("dir", null);
const tag = opt("tag", "v0.1.2");
const port = Number(opt("port", 8790));
const assetPort = Number(opt("asset-port", 8791));
const mode = opt("mode", "ok");
const notes = opt("notes", `PaperReader ${tag.slice(1)} 的變更摘要（假伺服器）`);
if (!dir) {
  console.error("要給 --dir <放安裝檔的資料夾>");
  process.exit(2);
}

const REPO = "/repos/rainwu0/PaperReader/releases";
const DOWNLOAD = `/rainwu0/PaperReader/releases/download/`;

function files() {
  const out = new Map();
  for (const name of readdirSync(dir)) {
    if (!/^PaperReader-.+\.apk$/.test(name)) continue;
    const body = readFileSync(join(dir, name));
    out.set(name, { body, sha: createHash("sha256").update(body).digest("hex") });
  }
  const sums = [...out].map(([name, f]) => `${f.sha}  ${name}`).join("\n") + "\n";
  out.set("SHA256SUMS.txt", { body: Buffer.from(sums), sha: createHash("sha256").update(sums).digest("hex") });
  return out;
}

function releases(host) {
  const base = `http://${host}${DOWNLOAD}`;
  const asset = (t, name, f) => ({
    name,
    size: f.body.length,
    digest: `sha256:${f.sha}`,
    browser_download_url: `${base}${t}/${name}`,
  });
  const fs = files();
  return [
    { tag_name: "v9.9.9", draft: true, prerelease: true, body: "草稿", assets: [] },
    { tag_name: "nightly", draft: false, prerelease: true, body: "標籤不合規格", assets: [] },
    {
      tag_name: tag,
      name: `PaperReader ${tag.slice(1)}（預覽版）`,
      draft: false,
      prerelease: true,
      body: notes,
      assets: [...fs].map(([name, f]) => asset(tag, name, f)),
    },
    { tag_name: "v0.1.0", draft: false, prerelease: true, body: "沒有附件的舊版", assets: [] },
  ];
}

let count = 0;
function log(req, status) {
  count += 1;
  console.log(
    `#${count} ${req.method} ${req.url} ua=${req.headers["user-agent"] ?? "-"} referer=${req.headers.referer ?? "-"} -> ${status}`,
  );
}

createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  if (!req.headers["user-agent"]) {
    log(req, 403);
    res.writeHead(403, { "content-type": "application/json" });
    res.end(JSON.stringify({ message: "Request forbidden by administrative rules. Please make sure your request has a User-Agent header" }));
    return;
  }
  if (url.pathname === REPO) {
    if (mode === "ratelimit") {
      log(req, 403);
      res.writeHead(403, { "content-type": "application/json", "x-ratelimit-remaining": "0" });
      res.end(JSON.stringify({ message: "API rate limit exceeded" }));
      return;
    }
    log(req, 200);
    res.writeHead(200, { "content-type": "application/json; charset=utf-8" });
    res.end(JSON.stringify(releases(req.headers.host)));
    return;
  }
  if (url.pathname.startsWith(DOWNLOAD)) {
    const [t, name] = url.pathname.slice(DOWNLOAD.length).split("/");
    if (t !== tag || !files().has(name)) {
      log(req, 404);
      res.writeHead(404).end();
      return;
    }
    // 轉到另一個來源（GitHub 轉到 release-assets.githubusercontent.com）
    const host = (req.headers.host ?? "127.0.0.1").replace(/:\d+$/, "");
    log(req, 302);
    res.writeHead(302, { location: `http://${host}:${assetPort}/assets/${encodeURIComponent(name)}` });
    res.end();
    return;
  }
  log(req, 404);
  res.writeHead(404).end();
}).listen(port, "127.0.0.1", () => console.log(`fake GitHub API on 127.0.0.1:${port}（${mode}，${tag}）`));

createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  const name = decodeURIComponent(url.pathname.replace(/^\/assets\//, ""));
  const f = files().get(name);
  if (!f) {
    log(req, 404);
    res.writeHead(404).end();
    return;
  }
  let body = f.body;
  if (mode === "corrupt" && name.endsWith(".apk")) {
    body = Buffer.from(body);
    body[Math.floor(body.length / 2)] ^= 0xff;
  }
  log(req, 200);
  res.writeHead(200, { "content-type": "application/octet-stream", "content-length": body.length });
  res.end(body);
}).listen(assetPort, "127.0.0.1", () => console.log(`fake asset host on 127.0.0.1:${assetPort}`));

// 讀檔失敗時（例如資料夾不存在）直接結束，免得測試等不到
statSync(dir);
