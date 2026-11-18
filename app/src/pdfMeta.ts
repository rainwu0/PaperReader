// 加入之後讀出的 PDF 自帶資訊（Info 的 Title 與 Author）要不要採用：看起來不是垃圾才採用。
// 年份另外依序從第一頁文字裡的 arXiv 編號、第一頁的版權或會議年份、PDF 自帶資訊的建立日期取（`pickYear`）。
// DOI 與 arXiv 編號（`pickDoi`、`pickArxiv`）：逐篇處理的補書目用；外掛抽參考文獻條目的
// DOI 與 arXiv 編號用同一組（`doiCandidates`、`arxivIdsIn`）。
// 只用 `import type`（沒有執行期的相依），所以 `scripts/pdfmeta-test.mjs` 可以直接測。

import type { Author } from "./api/types";

/** 常見的、等於沒填的篇名（小寫）。 */
const PLACEHOLDER_TITLES = new Set([
  "untitled",
  "untitled document",
  "无标题",
  "無標題",
  "未命名",
  "未命名文件",
  "title",
  "document",
  "unknown",
  "none",
  "null",
  "n/a",
  "tmp",
  "temp",
  "microsoft word",
  "powerpoint presentation",
  "slide 1",
]);

/** 常見的、等於沒填的作者（小寫）。 */
const PLACEHOLDER_AUTHORS = new Set([
  "unknown",
  "author",
  "admin",
  "administrator",
  "user",
  "owner",
  "anonymous",
  "none",
  "null",
  "n/a",
  "default",
  "pc",
  "windows user",
  "microsoft",
]);

const TOOL_PREFIX = /^(microsoft (word|powerpoint|excel)\s*-|untitled\b|document\d*$|doc\d+$)/i;
const FILE_SUFFIX = /\.(pdf|docx?|pptx?|tex|dvi|ps|indd|rtf|txt)$/i;

/**
 * 篇名可以用嗎？可以就交回整理過的篇名（多個空白合成一個、去頭尾空白），不行交回 `null`。
 * 不行的：空白、太短或太長、只有數字與符號、常見的佔位字、文書軟體留下的前綴（「Microsoft Word - …」）、
 * 檔名樣子（以 .pdf、.docx 這類結尾）、等於加入時的預設篇名（檔名本身）。
 */
export function usableTitle(raw: string | null | undefined, fileName: string): string | null {
  const title = (raw ?? "").replace(/\s+/g, " ").trim();
  if (title.length < 3 || title.length > 400) return null;
  if (!/[\p{L}]/u.test(title)) return null; // 只有數字與符號
  const lower = title.toLowerCase();
  if (PLACEHOLDER_TITLES.has(lower)) return null;
  if (TOOL_PREFIX.test(title) || FILE_SUFFIX.test(title)) return null;
  const name = fileName.trim().toLowerCase();
  const bare = name.endsWith(".pdf") ? name.slice(0, -4) : name;
  if (lower === name || lower === bare) return null;
  return title;
}

/**
 * 作者欄可以用嗎？可以就拆成一位一位的作者交回，不行交回空的。
 * 分隔：分號、換行、「 and 」、「&」一定拆；逗號只在有兩個以上時才拆（一個逗號多半是「姓, 名」）。
 */
export function usableAuthors(raw: string | null | undefined): Author[] {
  const text = (raw ?? "").replace(/\s+/g, " ").trim();
  if (text.length < 2 || text.length > 600) return [];
  if (PLACEHOLDER_AUTHORS.has(text.toLowerCase())) return [];
  if (!/[\p{L}]/u.test(text)) return [];
  let parts = text
    .split(/\s*(?:;|\n|\s&\s|\sand\s|\s及\s|、)\s*/i)
    .map((s) => s.trim())
    .filter(Boolean);
  if (parts.length === 1 && (text.match(/,/g) ?? []).length >= 2) {
    parts = text.split(/\s*,\s*/).filter(Boolean);
  }
  return parts
    .filter((p) => !PLACEHOLDER_AUTHORS.has(p.toLowerCase()))
    .slice(0, 50)
    .map((literal) => ({ literal }));
}

// ---------- 年份 ----------

/** 合理的出版年份：1900 年到明年。 */
function plausible(year: number, now: number, from = 1900): number | null {
  return Number.isInteger(year) && year >= from && year <= now + 1 ? year : null;
}

/**
 * PDF 自帶資訊的日期（`CreationDate`，PDF 的日期寫法 `D:20230415120000+08'00'`）的年份。
 * 讀不出來、不合理（未來、1900 年以前）交回 `null`。
 */
export function yearFromPdfDate(raw: unknown, now: number = new Date().getFullYear()): number | null {
  if (typeof raw !== "string") return null;
  const m = /^\s*(?:D:)?(\d{4})(?:\d{2})?/.exec(raw);
  return m ? plausible(Number(m[1]), now) : null;
}

// arXiv 頁邊的戳記：「arXiv:2611.16305v1 [eess.SP] 12 Nov 2026」（PDF.js 的文字裡前面常黏著別的字，所以不要求字界）。
// 新式編號 YYMM.NNNNN（2007 年 4 月起）；舊式 類別/YYMMNNN（hep-th/9901001）。
const ARXIV_NEW = String.raw`(\d{2})(\d{2})\.\d{4,5}`;
const ARXIV_OLD = String.raw`[a-z-]+(?:\.[A-Z]{2})?\/(\d{2})(\d{2})\d{3}`;
const ARXIV_STAMP = new RegExp(String.raw`arXiv:\s*(?:${ARXIV_NEW}|${ARXIV_OLD})v\d+\s*\[[^\]\n]{1,30}\]`, "i");
const ARXIV_ANY = new RegExp(String.raw`arXiv:\s*(?:${ARXIV_NEW}|${ARXIV_OLD})`, "i");

/** arXiv 編號的年份（第一次投稿的那一年）：新式 `2304.08069` → 2023；舊式 `hep-th/9901001` → 1999。 */
function arxivYear(m: RegExpExecArray, now: number): number | null {
  const [, ny, nm, oy, om] = m;
  const yy = Number(ny ?? oy);
  const mm = Number(nm ?? om);
  if (!Number.isInteger(yy) || mm < 1 || mm > 12) return null;
  if (ny !== undefined) return yy >= 7 ? plausible(2000 + yy, now, 2007) : null;
  return plausible(yy >= 91 ? 1900 + yy : 2000 + yy, now, 1991);
}

// 版權：「© 2020 IEEE」「Copyright © 2019 by the authors」「©ESO 2026」「(c) 2021」
const COPYRIGHT = /(?:©|\(c\)|\bcopyright\b)[^\n\d]{0,40}?((?:19|20)\d{2})\b/i;
// 會議與期刊：「Proceedings of the 2021 Conference」「(NeurIPS 2023)」「Published as a conference paper at ICLR 2024」
// 「Accepted to ICRA 2024」。只看同一行裡關鍵字後面不遠的年份。摘要裡常見的「received much attention since 2017」
// 這類句子不算，所以不收單獨的 received、accepted、published。
const VENUE =
  /\b(?:proceedings|conference|symposium|workshop|journal|transactions|published as|accepted (?:at|to|by|for))\b[^\n]{0,80}?\b((?:19|20)\d{2})\b/i;
// 會議的縮寫分大小寫比對（小寫的「acl」「kdd」可能是別的字）
const VENUE_ACRONYM =
  /\b(?:NeurIPS|NIPS|ICLR|ICML|CVPR|ICCV|ECCV|ACL|EMNLP|NAACL|COLING|AAAI|IJCAI|KDD|SIGIR|SIGGRAPH|ICRA|IROS|CoRL|MICCAI|INTERSPEECH|ICASSP)\b[^\n]{0,20}?\b((?:19|20)\d{2})\b/;

/**
 * 第一頁文字裡的年份：先找 arXiv 頁邊的戳記（有版本號與類別的那種，是這一篇自己的編號）；再找版權年份；
 * 再找會議或期刊名稱後面的年份；都沒有才取第一個 arXiv 編號（已出版的論文第一頁常引用別篇的 arXiv 編號，
 * 所以排在版權與會議之後）。都找不到交回 `null`。
 */
export function yearFromFirstPage(text: string, now: number = new Date().getFullYear()): number | null {
  const stamp = ARXIV_STAMP.exec(text);
  const fromStamp = stamp ? arxivYear(stamp, now) : null;
  if (fromStamp !== null) return fromStamp;
  for (const re of [COPYRIGHT, VENUE, VENUE_ACRONYM]) {
    const m = re.exec(text);
    const y = m ? plausible(Number(m[1]), now) : null;
    if (y !== null) return y;
  }
  const any = ARXIV_ANY.exec(text);
  return any ? arxivYear(any, now) : null;
}

/**
 * 加入時的年份：依序取第一頁文字（arXiv 編號、版權、會議），最後才用 PDF 自帶資訊的建立日期。都沒有交回 `null`。
 * 建立日期排最後：改版重編過的舊 arXiv PDF、掃描檔，自帶的是重編或掃描那年，不是出版的年份。
 */
export function pickYear(
  creationDate: unknown,
  firstPageText: string,
  now: number = new Date().getFullYear(),
): number | null {
  return yearFromFirstPage(firstPageText, now) ?? yearFromPdfDate(creationDate, now);
}

// ---------- DOI 與 arXiv 編號 ----------

// Crossref 建議的 DOI 正規式（不分大小寫）：10.<4 到 9 位數字>/<字母數字與 -._;()/:>
const DOI_BODY = String.raw`10\.\d{4,9}\/[-._;()\/:A-Z0-9]+`;
// 第一頁「標了的」DOI：前面是「DOI」「doi:」、doi.org/（dx.doi.org/ 也算），或 IEEE 期刊印的「Digital Object Identifier」
const DOI_MARKED = new RegExp(
  String.raw`(?:\bdoi\b\s*[:：]?\s*|doi\.org\/|\bdigital\s+object\s+identifier\b\s*[:：]?\s*)(${DOI_BODY})`,
  "gi",
);

/** 抓到的 DOI 去掉尾端的 `.`、`,`、`;`、`:` 與沒有配對的 `)`、`]`，轉小寫（核心存的是小寫）。 */
export function trimDoi(raw: string): string {
  let s = raw.trim();
  for (;;) {
    const last = s.slice(-1);
    const count = (c: string) => s.split(c).length - 1;
    if (".,;:".includes(last) && last !== "") s = s.slice(0, -1);
    else if (last === ")" && count(")") > count("(")) s = s.slice(0, -1);
    else if (last === "]" && count("]") > count("[")) s = s.slice(0, -1);
    else break;
  }
  return s.toLowerCase();
}

/** 一段文字裡的 DOI，依出現的順序、去重。 */
export function doiCandidates(text: string): string[] {
  const out: string[] = [];
  for (const m of text.matchAll(new RegExp(DOI_BODY, "gi"))) {
    const d = trimDoi(m[0]);
    if (d.includes("/") && !out.includes(d)) out.push(d);
  }
  return out;
}

/** 只有一個不同的候選才採用；兩個以上（多半混了別篇的）不取。 */
function single(cands: readonly string[]): string | null {
  return cands.length === 1 ? (cands[0] ?? null) : null;
}

/** PDF 自帶資訊的 `doi` 鍵（PDF.js 把自訂的鍵放在 `Custom` 裡；大小寫不拘）。 */
export function doiFromInfo(info: Record<string, unknown>): string | null {
  const cands: string[] = [];
  const look = (o: unknown) => {
    if (typeof o !== "object" || o === null) return;
    for (const [k, v] of Object.entries(o)) {
      if (k.toLowerCase() === "doi" && typeof v === "string") cands.push(...doiCandidates(v));
    }
  };
  look(info);
  look(info.Custom);
  return single([...new Set(cands)]);
}

/** XMP 裡的 DOI：`prism:doi`（元素或屬性），或以 `doi:` 開頭的 `dc:identifier`。 */
export function doiFromXmp(xmp: string | null): string | null {
  if (!xmp) return null;
  const cands: string[] = [];
  for (const m of xmp.matchAll(/<prism:doi>([^<]*)<\/prism:doi>|prism:doi\s*=\s*"([^"]*)"/gi)) {
    cands.push(...doiCandidates(m[1] ?? m[2] ?? ""));
  }
  for (const m of xmp.matchAll(/<dc:identifier\b[^>]*>([\s\S]*?)<\/dc:identifier>/gi)) {
    for (const v of (m[1] ?? "").matchAll(/>?\s*doi:\s*([^<\s]+)/gi)) cands.push(...doiCandidates(v[1] ?? ""));
  }
  return single([...new Set(cands)]);
}

/** 第一頁標了「DOI」「doi:」或 `doi.org/` 的那一個；沒有標的 DOI（多半是別篇的）不算。 */
export function doiFromFirstPage(text: string): string | null {
  const cands: string[] = [];
  for (const m of text.matchAll(DOI_MARKED)) {
    const d = trimDoi(m[1] ?? "");
    if (d && !cands.includes(d)) cands.push(d);
  }
  return single(cands);
}

/** 這篇的 DOI：依序取 PDF 自帶資訊、XMP、第一頁標了的，取第一個有的。 */
export function pickDoi(info: Record<string, unknown>, xmp: string | null, firstPageText: string): string | null {
  return doiFromInfo(info) ?? doiFromXmp(xmp) ?? doiFromFirstPage(firstPageText);
}

// arXiv 編號（不含版本號）：新式 YYMM.NNNNN，舊式 類別/YYMMNNN
const ARXIV_ID = String.raw`(\d{4}\.\d{4,5}|[a-z-]+(?:\.[A-Z]{2})?\/\d{7})`;
const ARXIV_STAMP_ID = new RegExp(String.raw`arXiv:\s*${ARXIV_ID}v\d+\s*\[[^\]\n]{1,30}\]`, "i");
// 前綴不分大小寫（arXiv、ARXIV），編號本身分（舊式的類別是小寫、子類是大寫，和核心的檢查相同）
const ARXIV_VALID = new RegExp(String.raw`^${ARXIV_ID}$`);
const validArxiv = (id: string | undefined): string | null => (id && ARXIV_VALID.test(id) ? id : null);

// arXiv 的網址：arxiv.org/abs/<編號> 或 arxiv.org/pdf/<編號>（可帶 https://、www.）
const ARXIV_URL = String.raw`(?:https?:\/\/)?(?:www\.)?arxiv\.org\/(?:abs|pdf)\/`;

/** 一段文字裡的 arXiv 編號（`arXiv:`、`CoRR abs/` 或 arxiv.org 網址後面的），不含版本號，依出現的順序、去重。 */
export function arxivIdsIn(text: string): string[] {
  const out: string[] = [];
  for (const m of text.matchAll(new RegExp(String.raw`(?:arXiv:\s*|CoRR\s+abs\/|${ARXIV_URL})${ARXIV_ID}`, "gi"))) {
    const id = validArxiv(m[1]);
    if (id && !out.includes(id)) out.push(id);
  }
  return out;
}

/** 第一頁頁邊的戳記（`arXiv:2304.08069v2 [cs.CV] 12 Nov 2023` 這種帶版本號與類別的）裡的編號。 */
export function arxivFromStamp(firstPageText: string): string | null {
  return validArxiv(ARXIV_STAMP_ID.exec(firstPageText)?.[1]);
}

/**
 * XMP 裡的 arXiv 識別碼：以 `arXiv:` 開頭的，或整個值就是 arXiv 的網址（`https://arxiv.org/abs/2611.16305v1`，
 * arXiv 產生的 PDF 這樣寫）。說明文字裡順帶提到的不算。
 */
export function arxivFromXmp(xmp: string | null): string | null {
  if (!xmp) return null;
  const cands: string[] = [];
  for (const m of xmp.matchAll(
    new RegExp(String.raw`(?:>|")\s*(?:arXiv:\s*|${ARXIV_URL})${ARXIV_ID}(?:v\d+)?(?:\.pdf)?\s*(?=<|")`, "gi"),
  )) {
    const id = validArxiv(m[1]);
    if (id && !cands.includes(id)) cands.push(id);
  }
  return single(cands);
}

/** 這篇的 arXiv 編號：頁邊的戳記，再來是 XMP。只出現在正文或參考文獻裡的不算（多半是引用別篇）。 */
export function pickArxiv(xmp: string | null, firstPageText: string): string | null {
  return arxivFromStamp(firstPageText) ?? arxivFromXmp(xmp);
}
