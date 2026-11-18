// 切條目：版面分析交回的參考文獻區段落（`layoutDocument` 的 `references`）切成一條一條，
// 抽每條的編號、DOI、arXiv 編號、年份。只用 `import type` 與帶 `.ts` 的引入，Node 的測試可以直接引入。
//
// 規則是自己訂的（沒有參考別人的程式）：
//   1. 六成以上的段落以 `[n]`、`n.`、`(n)` 開頭是編號式：每個編號開一條，編號大致遞增（允許跳號）；不帶編號的段落
//      接到上一條；一段裡出現下一個編號時從那裡切開。第一個編號之前的段落（頁底的註腳被收進來的）不要。
//   2. 其餘是作者年份式：用版面分析認書目條目開頭的同一條規則（`isReferenceEntry`）判斷一段是不是新的一條，不是的
//      接到上一條；一段裡「年份加句點」之後緊接作者名單（「Zhang, Y.」）時從那裡切開。
//   3. 接段時斷行的連字號接回。DOI、arXiv 編號用 `pdfMeta.ts` 的同一組函式；年份取括號裡的，否則取最後一個
//      合理的年份，略過 DOI、arXiv 編號、網址、頁碼範圍裡的數字。
//   4. 一條超過 1000 字截斷；超過 2000 條只留前 2000 條（核心的上限）。

import { arxivIdsIn, doiCandidates } from "../../pdfMeta.ts";
import type { LayoutSegment } from "../../translate/layout";
import { isReferenceEntry } from "../../translate/layout.ts";

/** 核心的上限（`references.rs`）。 */
export const MAX_ENTRIES = 2000;
export const MAX_TEXT = 1000;

/** 一條（交給核心 `references_write` 的草稿）。 */
export interface SplitEntry {
  number: number | null;
  label: string | null;
  text: string;
  page: number;
  bbox: [number, number, number, number] | null;
  doi: string | null;
  arxiv: string | null;
  year: number | null;
}

type Style = "bracket" | "dot" | "paren";

const LEAD: Record<Style, RegExp> = {
  bracket: /^\s*\[(\d{1,5})\]\s*/,
  dot: /^\s*(\d{1,5})\.\s+/,
  paren: /^\s*\((\d{1,5})\)\s*/,
};

/** 一段開頭的編號（三種寫法裡的一種）。 */
function leadOf(s: string): { style: Style; n: number; label: string; rest: string } | null {
  for (const style of ["bracket", "paren", "dot"] as Style[]) {
    const m = LEAD[style].exec(s);
    if (m && m[1]) return { style, n: Number(m[1]), label: m[0].trim(), rest: s.slice(m[0].length) };
  }
  return null;
}

/** 這一段裡下一個編號的位置（只認同一種寫法）。點號式要接在句末之後、後面是大寫字母，免得把卷期當成編號。 */
function nextLabelAt(s: string, style: Style, n: number): { at: number; len: number } | null {
  const re =
    style === "bracket"
      ? new RegExp(String.raw`\[${n}\]\s*`)
      : style === "paren"
        ? new RegExp(String.raw`(?<=[.)]\s+)\(${n}\)\s*`)
        : new RegExp(String.raw`(?<=[.)]\s+)${n}\.\s+(?=\p{Lu})`, "u");
  const m = re.exec(s);
  return m ? { at: m.index, len: m[0].length } : null;
}

// 作者年份式一段裡又起了一條：「… 2025. Zhang, Y. and …」「… (2019). Smith, J. A., …」；
// 名字縮寫不帶點的寫法（SAGE）：「… Representations. Beik-Mohammadi H, Hauberg S, …」（前面那一截要已經有年份）
const AY_SPLIT = /(?<=\b(?:1[6-9]|20)\d{2}[a-z]?\)?\.)\s+(?=\p{Lu}[\p{L}'’-]+(?:\s\p{Lu}[\p{L}'’-]+)?,\s+\p{Lu}\.)/gu;
const AY_SPLIT_BARE = /(?<=[.)])\s+(?=\p{Lu}[\p{L}'’]+(?:-\p{Lu}[\p{L}'’]+)?\s\p{Lu}{1,3},\s)/gu;
// 一段以作者開頭：「Agarwal R, …」「Zhang, T., …」「Ba J and …」
const AUTHOR_LEAD = /^\p{Lu}[\p{L}'’-]+(?:\s\p{Lu}[\p{L}'’-]+)?,?\s+(?:\p{Lu}{1,3}\b|\p{Lu}\.)/u;
const HAS_YEAR = /\b(?:1[6-9]|20)\d{2}\b/;

/** 接段：前一段尾端是「字母-」、下一段以小寫開頭時拿掉連字號直接接上，否則接一個空白。 */
export function joinText(a: string, b: string): string {
  const x = a.trimEnd();
  const y = b.trimStart();
  if (x === "") return y;
  if (y === "") return x;
  if (/\p{L}-$/u.test(x) && /^\p{Ll}/u.test(y)) return x.slice(0, -1) + y;
  return `${x} ${y}`;
}

/** 合理的年份：1600 年到明年。 */
function plausibleYear(y: number, now: number): boolean {
  return y >= 1600 && y <= now + 1;
}

/** 條目的年份：括號裡的優先，否則最後一個合理的年份。DOI、arXiv 編號、網址、頁碼範圍裡的數字不算。 */
export function entryYear(text: string, now: number = new Date().getFullYear()): number | null {
  const cleaned = text
    .replace(/https?:\/\/\S+/gi, " ")
    .replace(/\b10\.\d{4,9}\/\S+/gi, " ")
    .replace(/(?:arXiv:\s*|CoRR\s+abs\/|arxiv\.org\/(?:abs|pdf)\/)\S+/gi, " ")
    .replace(/\b\d+\s*[–—-]\s*\d+\b/g, " ");
  const paren = [...cleaned.matchAll(/\((?:[^()]*?[,\s])?((?:1[6-9]|20)\d{2})[a-z]?\)/g)]
    .map((m) => Number(m[1]))
    .filter((y) => plausibleYear(y, now));
  if (paren.length > 0) return paren[0] ?? null;
  const all = [...cleaned.matchAll(/(?<![\d.])((?:1[6-9]|20)\d{2})[a-z]?(?![\d])/g)]
    .map((m) => Number(m[1]))
    .filter((y) => plausibleYear(y, now));
  return all.at(-1) ?? null;
}

/** 截到最多 `MAX_TEXT` 個字（Unicode 字元）。 */
function clip(s: string): string {
  const chars = Array.from(s);
  return chars.length > MAX_TEXT ? chars.slice(0, MAX_TEXT).join("") : s;
}

interface Draft {
  number: number | null;
  label: string | null;
  text: string;
  page: number;
  bbox: [number, number, number, number];
}

function finish(d: Draft): SplitEntry | null {
  const text = clip(d.text.replace(/\s+/g, " ").trim());
  if (text === "") return null;
  return {
    number: d.number,
    label: d.label,
    text,
    page: d.page,
    bbox: d.bbox,
    doi: doiCandidates(text)[0] ?? null,
    arxiv: arxivIdsIn(text)[0] ?? null,
    year: entryYear(text),
  };
}

/**
 * 這一份參考文獻區是不是編號式，是的話是哪一種寫法。六成以上的段落以編號開頭；或者開頭的編號本身就是一串從 1 起、
 * 大致連續的數（單欄的條目常被版面分析切成「編號那一行」與「續行」兩段，開頭有編號的只占一半）。
 */
export function numberedStyle(segs: readonly Pick<LayoutSegment, "source">[]): Style | null {
  if (segs.length === 0) return null;
  const counts: Record<Style, number> = { bracket: 0, dot: 0, paren: 0 };
  const nums: Record<Style, number[]> = { bracket: [], dot: [], paren: [] };
  let led = 0;
  for (const s of segs) {
    const l = leadOf(s.source);
    if (l) {
      led++;
      counts[l.style]++;
      nums[l.style].push(l.n);
    }
  }
  const style = (Object.entries(counts) as [Style, number][]).sort((a, b) => b[1] - a[1])[0]?.[0] ?? null;
  if (style && counts[style] > 0 && led >= 0.6 * segs.length) return style;
  // 方括號的編號在全文裡依序是 1、2、3……（雙欄的條目常被切在段落中間：「… 2022. 2 [4] David …」）
  const bracketed = segs.flatMap((s) => [...s.source.matchAll(/\[(\d{1,5})\]/g)].map((m) => Number(m[1])));
  if (sequential(bracketed)) return "bracket";
  return style && sequential(nums[style]) ? style : null;
}

/** 一串數裡依序有 1、2、3……（允許少數跳號），五條以上、涵蓋到的最大編號的八成以上。 */
function sequential(nums: readonly number[]): boolean {
  let last = 0;
  let chain = 0;
  for (const n of nums) {
    if (last === 0 ? n <= 3 : n > last && n <= last + 3) {
      chain++;
      last = n;
    }
  }
  return chain >= 5 && chain >= 0.8 * last;
}

function splitNumbered(segs: readonly LayoutSegment[], style: Style): Draft[] {
  const out: Draft[] = [];
  let last = 0;
  const open = (n: number, label: string, text: string, s: LayoutSegment) => {
    out.push({ number: n, label, text, page: s.page, bbox: s.bbox });
    last = n;
  };
  // 一段的文字（開頭的編號已經拿掉）接到目前這一條，遇到下一個編號就切開
  const feed = (text: string, s: LayoutSegment) => {
    let rest = text;
    for (;;) {
      const cur = out.at(-1);
      if (!cur) return;
      const nx = nextLabelAt(rest, style, last + 1);
      if (!nx) {
        cur.text = joinText(cur.text, rest);
        return;
      }
      cur.text = joinText(cur.text, rest.slice(0, nx.at));
      const label = rest.slice(nx.at, nx.at + nx.len).trim();
      open(last + 1, label, "", s);
      rest = rest.slice(nx.at + nx.len);
    }
  };
  for (const s of segs) {
    const l = leadOf(s.source);
    const fresh = l && l.style === style && (last === 0 ? l.n <= 3 : l.n > last && l.n <= last + 20);
    if (l && fresh) {
      open(l.n, l.label, "", s);
      feed(l.rest, s);
    } else if (out.length > 0) {
      feed(s.source, s);
    }
    // 第一個編號之前、不帶編號的段落（例如被收進來的註腳）：不要
  }
  return out;
}

/**
 * 一段裡又起了一條的地方切開；不帶點的縮寫只在前面那一截已經有年份時才切。`carry`：這一段之前、目前這一條已經有年份
 * （年份在上一段時）。
 */
function splitInside(s: string, carry: boolean): string[] {
  const out: string[] = [];
  for (const [k, part] of s.split(AY_SPLIT).entries()) {
    let rest = part;
    let from = 0;
    for (const m of part.matchAll(AY_SPLIT_BARE)) {
      const at = m.index ?? 0;
      if ((k === 0 && from === 0 && carry) || HAS_YEAR.test(part.slice(from, at))) {
        out.push(rest.slice(0, at - from));
        rest = rest.slice(at - from);
        from = at;
      }
    }
    out.push(rest);
  }
  return out.map((p) => p.trim()).filter((p) => p !== "");
}

function splitAuthorYear(segs: readonly LayoutSegment[]): Draft[] {
  const out: Draft[] = [];
  for (const s of segs) {
    const cur0 = out.at(-1);
    const carry = !!cur0 && HAS_YEAR.test(cur0.text) && !AUTHOR_LEAD.test(s.source);
    splitInside(s.source, carry).forEach((part, i) => {
      const cur = out.at(-1);
      // 新的一條：段落中間切出來的；或上一條已經完整（以句點、括號或數字結尾），而這一段像書目條目的開頭、
      // 或以作者開頭而上一條已有年份。上一條斷在逗號、「and」時（作者名單換行）一律接上去
      const complete = !cur || /[.)\d]\s*$/.test(cur.text);
      const fresh =
        i > 0 || (complete && (isReferenceEntry(part) || (!!cur && AUTHOR_LEAD.test(part) && HAS_YEAR.test(cur.text))));
      if (fresh || !cur) out.push({ number: null, label: null, text: part, page: s.page, bbox: s.bbox });
      else cur.text = joinText(cur.text, part);
    });
  }
  return out;
}

/** 參考文獻區的段落 → 條目。 */
export function splitEntries(segs: readonly LayoutSegment[]): SplitEntry[] {
  const style = numberedStyle(segs);
  const drafts = style ? splitNumbered(segs, style) : splitAuthorYear(segs);
  const out: SplitEntry[] = [];
  for (const d of drafts) {
    const e = finish(d);
    if (e) out.push(e);
    if (out.length >= MAX_ENTRIES) break;
  }
  return out;
}
