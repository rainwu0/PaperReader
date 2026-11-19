// 驗收用的假翻譯服務：OpenAI 相容的 /v1/chat/completions，把每一段回成「【譯】」加原文；重點筆記的請求（<para id=…>）回一份假的重點筆記 JSON。
// 不連任何真的服務。用法：node scripts/fake-llm-server.mjs [埠，預設 8787] [每次回應的延遲毫秒，預設 1200]
// 模擬器裡用 http://10.0.2.2:<埠>/v1/ 連到電腦上的這個服務。
// 第三個參數給 `zh`：譯文改成一段長度約為原文四成五的中文句子（驗譯本 PDF 的排字用），不再回原文。
// 快速翻譯的請求（`<selection>…</selection>`）回「【快譯】」加選取（選一個詞時回兩行）。
// 環境變數 `FAKE_LLM_LOG` 給一個檔名：每個請求收到的訊息記一行（驗收時看實際送了什麼）。

import { appendFileSync } from "node:fs";
import { createServer } from "node:http";

const port = Number(process.argv[2] ?? 8787);
const delay = Number(process.argv[3] ?? 1200);
const zh = process.argv[4] === "zh";
const POOL = [..."我們提出一種新的方法，在標準基準上取得了更好的準確率，並且大幅降低計算成本。實驗結果顯示，該模型在不同資料集上均具有良好的泛化能力。"];

/** 假的中文譯文：長度約為原文的四成五，起點依段落代號輪替；開頭一定是字、不是標點。 */
function fakeZh(id, source) {
  const n = Math.max(6, Math.round(source.length * 0.45));
  let at = [...id].reduce((a, c) => a + c.charCodeAt(0), 0) % POOL.length;
  while (/[，。]/.test(POOL[at])) at = (at + 1) % POOL.length;
  let out = "";
  for (let i = 0; i < n; i++) out += POOL[(at + i) % POOL.length];
  return out;
}
let requests = 0;

const unescape = (s) => s.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");

/**
 * 重點筆記（`<para id=… page=…>` 的請求）：從送來的段落挑幾段當出處，交回固定形狀的 JSON。
 * 合併的請求（`<notes part=…>`）：把各部分的點依區段接起來、每區最多三點。不認得的段落代號照樣放一個進去，
 * 驗「對不上原文的點會被略過」。
 */
function fakeDigest(text) {
  const notes = [...text.matchAll(/<notes part="\d+">([\s\S]*?)<\/notes>/g)];
  if (notes.length > 0) {
    const merged = { question: [], method: [], results: [], limitations: [], quotes: [] };
    for (const m of notes) {
      let part = {};
      try {
        part = JSON.parse(unescape(m[1]));
      } catch {
        continue;
      }
      for (const k of Object.keys(merged)) for (const it of part[k] ?? []) if (merged[k].length < 3) merged[k].push(it);
    }
    return JSON.stringify(merged);
  }
  const paras = [...text.matchAll(/<para id="([^"]+)" page="(\d+)">([\s\S]*?)<\/para>/g)].map((m) => ({
    id: m[1],
    page: Number(m[2]),
    text: unescape(m[3]),
  }));
  if (paras.length === 0) return null;
  const at = (f) => paras[Math.min(paras.length - 1, Math.floor(paras.length * f))];
  const withNumber = paras.find((p) => /\d+(\.\d+)?\s*%|\d+\.\d+/.test(p.text)) ?? at(0.6);
  const quote = withNumber.text.split(/(?<=\.)\s/)[0].slice(0, 120);
  return JSON.stringify({
    question: [{ text: `這篇想解決的問題：第 ${at(0).page} 頁開頭提出的研究動機與目標。`, refs: [at(0).id] }],
    method: [
      { text: `提出的方法與實驗設定（第 ${at(0.25).page} 頁起）。`, refs: [at(0.25).id, at(0.35).id] },
      { text: "使用的資料與比較的基準方法。", refs: [at(0.45).id] },
    ],
    results: [
      { text: `主要結果：在主要基準上優於比較的方法（第 ${withNumber.page} 頁）。`, refs: [withNumber.id] },
      { text: "消融實驗顯示每個組成部分都有貢獻。", refs: [at(0.7).id] },
      { text: "這一點的出處對不上，應該被略過。", refs: ["g9999"] },
    ],
    limitations: [{ text: "作者提到的限制與之後要做的事。", refs: [at(0.9).id] }],
    quotes: [{ text: `「${quote}」——值得記下的原句。`, refs: [withNumber.id] }],
  });
}

createServer((req, res) => {
  let body = "";
  req.on("data", (c) => (body += c));
  req.on("end", () => {
    requests++;
    const text = (() => {
      try {
        return JSON.parse(body).messages.map((m) => String(m.content)).join("\n");
      } catch {
        return "";
      }
    })();
    if (process.env.FAKE_LLM_LOG) {
      // 驗收時記下每一次實際收到的內容（快速翻譯送了什麼）：一行一個請求
      try {
        appendFileSync(process.env.FAKE_LLM_LOG, `${JSON.stringify({ n: requests, url: req.url, messages: JSON.parse(body).messages })}\n`);
      } catch {
        // 讀不懂的請求不記
      }
    }
    let out = "";
    const digest = fakeDigest(text);
    // 系統提示詞裡也寫著 `<selection>…</selection>`：取最後一個（使用者訊息裡的）
    const selection = [...text.matchAll(/<selection>([\s\S]*?)<\/selection>/g)].at(-1);
    if (selection) {
      // 快速翻譯：選一個詞回兩行（詞義、在這句裡的意思），選一句以上回一行譯文
      const sel = selection[1];
      out = text.includes("exactly two lines") ? `【詞義】${sel}\n【此處】${sel}` : `【快譯】${zh ? fakeZh("q", sel) : sel}`;
      console.log(`#${requests} ${req.method} ${req.url} 快速翻譯 ${sel.length} 字`);
    } else if (digest !== null) {
      out = digest;
      console.log(`#${requests} ${req.method} ${req.url} 重點筆記`);
    } else {
      for (const m of text.matchAll(/<seg id="([^"]+)">([\s\S]*?)<\/seg>/g)) out += `<seg id="${m[1]}">【譯】${zh ? fakeZh(m[1], m[2]) : m[2]}</seg>\n`;
      console.log(`#${requests} ${req.method} ${req.url} ${out.split("</seg>").length - 1} 段`);
    }
    setTimeout(() => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(
        JSON.stringify({
          id: "fake",
          object: "chat.completion",
          created: 1,
          model: "fake",
          choices: [{ index: 0, message: { role: "assistant", content: out }, finish_reason: "stop" }],
          usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
        }),
      );
    }, delay);
  });
}).listen(port, "0.0.0.0", () => console.log(`假翻譯服務在 ${port}，延遲 ${delay} ms`));
