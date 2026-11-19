// 版面紀錄：把 PDF.js 文字層的每一小段文字（字串加位置）組成「段落」。
//
// 規則（自己訂的；沒有參考別人的程式）：
//   1. 同一條基線上的字併成一行（上標、下標跟著主行）；
//   2. 偵測雙欄：找一條幾乎沒有行跨過的中縫；跨過中縫的行（標題、摘要、跨欄的圖說）獨立成一塊，
//      其餘的行先排左欄、再排右欄；
//   3. 同一欄裡相鄰的行，行距突然變大、首行縮排、上一行很短、字級變了、或這一行以「[12]」這類編號開頭，就分段；
//   4. 不翻：頁首頁尾與頁碼（上下緣、短、而且在多頁重複，或單純的頁碼）、圖內的字（很小的字級、
//      單字的座標軸標籤、純數字與符號）、公式（字母很少；或是公式的碎片，見 `formulaFragment`）、
//      表格的一列（同一高度三段以上的短字）、參考文獻（「References」標題之後的段落，到附錄為止；
//      沒有標題時，開頭是作者名單的書目條目、「[12] A. Name」起頭的條目與它的續段也算）。圖說要翻。
//   5. 第 1 頁的篇名被切成幾段時併回一段。
//
// 座標都是 PDF 頁面座標（點，原點在左下）。這個檔案不依賴任何套件，Node 的測試可以直接引入。

export interface RawItem {
  str: string;
  /** `[a, b, c, d, e, f]`：e、f 是這段文字起點（基線左端）。 */
  transform: number[];
  width: number;
  height: number;
  /** PDF.js 給的字型代號（`g_d0_f3`）。同一份文件裡同一個字型的代號相同；只拿來比「和旁邊的字是不是同一個字型」，
   *  不解讀它的名稱（真正的字型名稱要另外解析繪圖指令才拿得到）。沒有就不看字體。 */
  fontName?: string;
  /** PDF.js 標的「這一段之後換行」。分段規則不看它；補書目接成一頁的文字時用（`extract.ts` 的 `pageText`）。 */
  hasEOL?: boolean;
}

/** 一頁的文字接成一個字串（片段標了行尾的接換行）：補書目的規則看「同一行」用（`pdfMeta.ts`）。 */
export function pageText(items: readonly RawItem[]): string {
  return items.map((it) => it.str + (it.hasEOL ? "\n" : "")).join("");
}

export interface PageInput {
  /** 頁面可見範圍 `[x0, y0, x1, y1]`。 */
  view: [number, number, number, number];
  items: RawItem[];
}

/** `reference` 只出現在 `layoutDocument` 的 `references` 裡（參考文獻區），不會交給翻譯。 */
export type SegmentKind = "body" | "heading" | "caption" | "reference";

export interface LayoutSegment {
  /** 頁序，從 0 起算。 */
  page: number;
  /** `[x0, y0, x1, y1]`，PDF 頁面座標。 */
  bbox: [number, number, number, number];
  kind: SegmentKind;
  /** 主要的字級（點）。 */
  size: number;
  source: string;
}

interface Line {
  text: string;
  x0: number;
  x1: number;
  /** 基線的 y。 */
  y: number;
  size: number;
  /** 這一行最大的字級（至少三個字的片段）；行內的上下標與公式符號不影響它。 */
  big: number;
  col: "left" | "right" | "span";
  /** 同一條基線的格子有同一個編號（先切成格子、定了欄之後再併回整行）。 */
  band: number;
  /** 由左到右、同一個字型連在一起的片段（空白不算）。沒有字型資訊時是空的。 */
  runs: Run[];
}

interface Run {
  font: string;
  text: string;
}

const CJK = /[⺀-鿿豈-﫿＀-￯]/;
// 圖說：「Fig. 3.」「Figure 3:」「TABLE IV」「表 2」這類編號後面接標點或換行；
// 內文裡的「Figure 4 illustrates …」不算（編號後面直接接字）
const CAPTION =
  /^(fig\.?|figure|table|tab\.|algorithm|圖|表)\s*([\d]+|[IVXLC]+|[A-Z]\d*|[一二三四五六七八九十]+)\s*([.:：|]|$)/i;
// IEEE 的表格標題：「TABLE VI」接全大寫的標題（分大小寫，內文的「Table 3 shows」不算）
const CAPTION_CAPS = /^TABLE\s+([IVXLC]+|\d+)\s+[A-Z]{2}/;
const isCaptionText = (s: string) => CAPTION.test(s) || CAPTION_CAPS.test(s);
// 只有編號的圖說首行（IEEE 的「TABLE VI」，標題在下一行）
const CAPTION_LABEL = /^(fig\.?|figure|table|tab\.)\s*([\d]+|[IVXLC]+)\s*[.:]?$/i;
const HEADING_WORDS =
  /^(\d+(\.\d+)*\.?\s+|[A-Z]\.\s+|[IVXLC]+\.\s+|appendix\b|abstract\b|references\b|bibliography\b|acknowledg|conclusions?\b|introduction\b|related work\b|摘要|參考文獻|結論)/i;
const REFERENCES = /^(references|bibliography|參考文獻|参考文献)\b/i;
// 書目條目（沒有「References」標題的參考文獻清單也認得）。開頭是作者名單：
//   APA、芝加哥這類「姓, 名縮寫,」連續兩位以上，而且段落裡有年份：Greenland, S., Senn, S.J., … (2016)
//   作者後面直接接冒號再接篇名（Springer LNCS、評論文章的書目清單）：Lin, T.Y., Goyal, P.: Focal loss…；
//   Benjamin, Daniel J, and Berger, James O: A simple alternative…
// 行首可以帶編號「[12]」「12.」或清單符號（符號字型的清單符號在文字層裡常是控制字元或私用區的字，例如 U+0002）。
// 姓可以帶 LaTeX 重音拆出來的附加符號（「Galv˜ao」「Rodr´ıguez」的 ˜ ´ 在文字層裡是獨立的修飾字元）
const SURNAME = String.raw`[A-Z][\p{L}\p{M}\p{Sk}'’-]+`;
const INITIALS = String.raw`(?:[A-Z]\.\s?-?){1,3}`;
const GIVEN = String.raw`[A-Z][\p{L}.]*(?:\s+[A-Z][\p{L}.]*)*`;
const ENTRY_LEAD = String.raw`^\s*(?:\[\d{1,3}\]\s*|\d{1,3}\.\s+|[\p{Cc}\p{Co}\p{So}•▪◦●∙]\s*)?`;
const REF_INITIALS = new RegExp(
  `${ENTRY_LEAD}(?:${SURNAME},\\s+${INITIALS},?\\s+(?:and\\s+|&\\s+)?){2,}`,
  "u",
);
const REF_COLON = new RegExp(
  `${ENTRY_LEAD}${SURNAME},\\s+(?:${INITIALS}|${GIVEN})(?:,?\\s+(?:and\\s+)?${SURNAME},\\s+(?:${INITIALS}|${GIVEN}))*,?\\s*:`,
  "u",
);
// 名縮寫在前的作者名單（APS、IEEE 這類編號制）：「[1] N. Gisin, G. Ribordy, and H. Zbinden, Quantum …」。
// 帶編號時一位作者就算；不帶編號（跨頁、跨欄接下去的條目）要兩位以上
const REF_FIRST_LABELED = new RegExp(String.raw`^\s*\[\d{1,3}\]\s*${INITIALS}\s*${SURNAME}(?:,|\s+and\b|\s+et al)`, "u");
const REF_FIRST_LIST = new RegExp(String.raw`^\s*(?:${INITIALS}\s*${SURNAME},\s+){2,}`, "u");
// 條目的結尾「卷, 頁 (年).」「arXiv:2003.06557 (2020).」「(VDE, 2024) pp. 1416–1419.」：跨欄、跨頁接下去的條目片段只剩這一截
const CITE_TAIL =
  /\b\d{1,4},\s*[A-Z]?\d{1,7}\s*\((?:19|20)\d{2}\)\.?$|arXiv:\s?\d{4}\.\d{4,5}(?:v\d)?\s*\((?:19|20)\d{2}\)\.?$|\b(?:19|20)\d{2}\)\s*pp\.\s*\d+\s*[–-]\s*\d+\.?$/;
// 段落中間又起了一個條目（「… (2002). [2] N. Gisin and …」）
const REF_INSIDE = new RegExp(String.raw`[.)]\s*\[\d{1,3}\]\s*${INITIALS}\s*${SURNAME}`, "u");
const YEAR = /\b(19|20)\d{2}[a-z]?\b/;
/** 這段文字像不像書目條目的開頭（版面分析與切條目共用）。 */
export const isReferenceEntry = (s: string) =>
  REF_COLON.test(s) ||
  (REF_INITIALS.test(s) && YEAR.test(s)) ||
  ((REF_FIRST_LABELED.test(s) || REF_FIRST_LIST.test(s)) && YEAR.test(s)) ||
  (REF_INSIDE.test(s) && YEAR.test(s)) ||
  (CITE_TAIL.test(s) && s.split(/\s+/).length <= 40);
// 名縮寫加姓（「A. B. Tacla」「M. Queiroz」）
const REF_NAME = new RegExp(String.raw`(?:^|[\s,])${INITIALS}\s*${SURNAME}`, "u");
// 編號制條目的第一行（懸掛縮排：續行比它縮進，不算新段落；下一個條目開新段）
const REF_LABEL = new RegExp(String.raw`^\[\d{1,3}\]\s*${INITIALS}\s*${SURNAME}`, "u");
const ABSTRACT_START = /^(abstract|摘要)\b|^abstract[—–:.-]/i;
const HEADING_NUMBERED = /^((1|I)\.?\s+[A-Z]|introduction\b)/;
const APPENDIX = /^(appendix|appendices|附錄|[A-Z]\.\s+[A-Z])/i;
const EQ_NUMBER = /\(\s*[A-Z]?\d{1,3}[a-z]?\s*\)\s*$/;
// 作者與單位：電子郵件、機構、IEEE 會員頭銜、上標編號接機構
const AUTHORISH =
  /@|\b(universit|institut|department|dept\.|laborator|school of|college|academy|corporation|inc\.|ltd\.|member,? ieee|fellow,? ieee|senior member)/i;

// AUTHORISH 去掉單獨的「@」（「mAP@0.5」不是電子郵件），電子郵件另外比對
const AUTHORISH_WORDS = new RegExp(AUTHORISH.source.replace(/^@\|/, ""), "i");
const EMAIL = /[\w.+-]+@[\w-]+\.[\w.-]+/;
// 第 1 頁註腳裡的作者資料
const AUTHOR_NOTE = /\b(is|are) (now )?with\b|corresponding author|e-?mail|equal contribution|contributed equally/i;
// 經費與致謝的註腳（要翻）：提到機構名稱也不是作者資料
const FUNDING = /\b(support|fund|grant|partnership|sponsor|thank)/i;
// 前置區（作者與單位）結束的標題
const FRONT_END = /^(summary|introduction|background|keywords|key words|index terms)\b/i;

// ---------- 公式 ----------

// 數學運算與關係符號（含 + 與減號 U+2212；連字號「-」不算）
const MATH_SYM = /[=<>≤≥≈≃≅∼∝±×÷∑∏∫∂∇√∞→←↔⇒⇔∈∉⊂⊆⊃∪∩∀∃‖∥⟨⟩+−∗]/;
// 公式裡的函數名，不算英文字
const MATH_WORD = /^(log|ln|exp|max|min|sup|inf|arg|argmax|argmin|det|tr|diag|sin|cos|tan|atan2?|cov|var|lim|mod|erfc?|sgn|rank|dim|ker)$/i;
// 數字後面的單位（表格格子「0.487 Mbit/s」「500 kHz」）
const UNIT = /^(?:[kMGTmμn]?(?:Hz|bit|b|B|m|s|W|V|A|Baud|SNU)(?:\/s)?|dB|%|ms|km|nm|mm|cm)$/;

/**
 * 公式的碎片：顯示公式被拆成幾行之後的分母、矩陣的一列、上下標（「X H 2 X H 2 2」「X3 e 1」「1 + VM」「K sg λi」）。
 * 這種段落沒有幾個英文字，字母比例卻不低，靠「字母太少」那一條認不出來；翻了也只會原樣回來或譯錯，蓋上白框又會把公式蓋壞。
 * 判斷：沒有英文字（三個字母以上的全小寫、首字大寫或全大寫；函數名不算）時，有公式的跡象就算（運算符號、希臘字母、
 * 私用區或控制字元拆出來的括號、兩個以上的單一字母或數字、單一字母配數字、字母與數字黏在一起）；
 * 一個英文字時要有運算（單獨的運算符號或等號）、兩個以上的單一字母或數字（短標題「3D Reconstruction」不算）；
 * 兩個英文字時要有運算符號加兩個單一字母、三個以上的單一字母，或兩個以上的等號。
 * 只有數字與單位的短片段（表格的格子「0.487 Mbit/s」）也算。三個英文字以上的一律不算（是句子或標題）。
 */
export function formulaFragment(text: string): boolean {
  const core = text.replace(/\(\s*[A-Z]?\d{1,3}[a-z]?\s*\)/g, " ").trim(); // 公式編號「(B3)」先拿掉
  const tokens = core.split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return false;
  const bare = (t: string) => t.replace(/^[([{"'“‘]+|[)\]}"'”’.,;:]+$/g, "");
  // 英文字：全小寫、首字大寫、全大寫（小型大寫的標題）三種寫法；大小寫夾雜的（「TmMn」「SKRtheory」）是符號
  const wordCount = tokens.filter((t) => {
    const b = bare(t);
    return /^(?:[a-z]{3,}|[A-Z][a-z]{2,}|[A-Z]{3,})$/.test(b) && !MATH_WORD.test(b);
  }).length;
  if (wordCount >= 3) return false;
  const singles = tokens.filter((t) => /^\p{L}$/u.test(bare(t))).length;
  const numbers = tokens.filter((t) => /^[-+−]?[\d.,]+%?$/.test(bare(t))).length;
  const sym = MATH_SYM.test(core) || /\p{Script=Greek}/u.test(core) || /[\p{Co}\p{Cc}ˆ˜¯]/u.test(core);
  // 運算：單獨的運算符號，或帶等號、不等號、箭頭的詞（「C++」「γ-ray」裡的符號不算）
  const op = tokens.some((t) => /^[=<>≤≥≈∼∝±×÷+−∗|√∑∏∫∂∇]$/.test(t) || /[=≤≥≈<>←→⇒]/.test(t));
  if (wordCount === 1) {
    // 一個英文字加一兩個別的詞：短標題（「3D Reconstruction」「γ-ray bursts」「IEEE 802.11ax」）居多，要有更明確的公式跡象
    return op || singles >= 2 || numbers >= 2;
  }
  if (wordCount === 0) {
    return (
      sym ||
      singles >= 2 ||
      numbers >= 2 ||
      (singles >= 1 && numbers >= 1) ||
      (tokens.length >= 2 && tokens.some((t) => /[A-Za-z]\d|\d[A-Za-z]/.test(bare(t)))) ||
      (tokens.length <= 3 && numbers >= 1 && tokens.every((t) => /^[-+−]?[\d.,]+%?$/.test(bare(t)) || UNIT.test(bare(t))))
    );
  }
  return (sym && singles >= 2) || singles >= 3 || (core.match(/=/g)?.length ?? 0) >= 2;
}

/**
 * 以公式為主的段落：有 TeX 原始碼（`$…$` 或 `\指令{`）、數學符號占全部非空白字元的一成五以上，或是公式的碎片。
 * 版面紀錄不收公式的碎片；已經翻好的段落裡若有這種段落，產生譯本 PDF 時保留原文、不蓋（`buildPdf.ts`）。
 */
export function mathText(text: string): boolean {
  if (/\$[^$\s][^$]*\$|\\[a-zA-Z]+\{/.test(text)) return true;
  const chars = [...text.replace(/\s+/g, "")];
  if (chars.length === 0) return false;
  const math = chars.filter((c) => /[∀-⋿←-⇿⨀-⫿±×÷=<>^_|]/.test(c)).length;
  return math / chars.length >= 0.15 || formulaFragment(text);
}

/** 像文章的長段落：40 個詞以上、多半是小寫開頭的詞（作者與單位的詞多半大寫開頭）。 */
function prose(text: string): boolean {
  const words = text.split(/\s+/).filter((w) => /\p{L}/u.test(w));
  if (words.length < 40) return false;
  return words.filter((w) => /^[(["'“]?\p{Ll}/u.test(w)).length >= 0.5 * words.length;
}

// 清單項目的開頭：「1)」「(a)」「•」「–」；「1.」只在後面接大寫字母時算（避免「1.5 times」）
const LIST_MARK = /^(\d{1,2}\)|\([a-zivx\d]{1,4}\)|[•▪◦●∙–-]\s|\d{1,2}\.\s+[A-Z])/;

function percentile(v: number[], q: number): number {
  const s = [...v].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(q * s.length))] ?? 0;
}

function median(v: number[]): number {
  if (v.length === 0) return 0;
  const s = [...v].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? (s[m] ?? 0) : ((s[m - 1] ?? 0) + (s[m] ?? 0)) / 2;
}

// ---------- 1. 行 ----------

function buildLines(items: RawItem[]): Line[] {
  const prepared = items
    .filter((it) => it.str.trim() !== "" && it.transform.length >= 6)
    .map((it) => {
      const [a = 1, b = 0, c = 0, d = 1, e = 0, f = 0] = it.transform;
      return {
        str: it.str,
        x: e,
        y: f,
        w: it.width,
        size: Math.max(1, Math.hypot(c, d) || it.height || 1),
        font: it.fontName ?? "",
        // 斜放的字（頁邊的浮水印）不處理
        rotated: Math.abs(b) > 0.2 * Math.abs(a) && Math.abs(c) > 0.2 * Math.abs(d),
      };
    })
    .filter((it) => !it.rotated && Number.isFinite(it.x) && Number.isFinite(it.y));
  prepared.sort((p, q) => q.y - p.y || p.x - q.x);

  const bands: (typeof prepared)[] = [];
  for (const it of prepared) {
    const band = bands.find((b) => {
      const ref = b[0];
      if (!ref) return false;
      const size = Math.max(ref.size, it.size);
      return Math.abs(ref.y - it.y) <= 0.55 * size;
    });
    if (band) band.push(it);
    else bands.push([it]);
  }

  // 先切成「格子」：同一條基線上兩段文字之間隔了一大段空白（可能是欄與欄的中縫）就切開。
  // 定了欄之後，同一欄同一條基線的格子再併回整行（`mergeCells`）。
  const lines: Line[] = [];
  bands.forEach((band, bi) => {
    band.sort((p, q) => p.x - q.x);
    let group: typeof band = [];
    const flush = () => {
      if (group.length === 0) return;
      lines.push(makeLine(group, bi));
      group = [];
    };
    for (const it of band) {
      const prev = group[group.length - 1];
      if (prev && it.x - (prev.x + prev.w) > 1.0 * Math.max(prev.size, it.size)) flush();
      group.push(it);
    }
    flush();
  });
  return lines;
}

function makeLine(
  group: { str: string; x: number; y: number; w: number; size: number; font: string }[],
  band: number,
): Line {
  let text = "";
  let prevEnd = Number.NaN;
  let prevSize = 0;
  const runs: Run[] = [];
  for (const it of group) {
    if (it.font !== "" && it.str.trim() !== "") {
      const last = runs[runs.length - 1];
      if (last && last.font === it.font) last.text += it.str;
      else runs.push({ font: it.font, text: it.str });
    }
    if (text !== "") {
      const gap = it.x - prevEnd;
      const needSpace =
        gap > 0.18 * Math.max(prevSize, it.size) &&
        !text.endsWith(" ") &&
        !it.str.startsWith(" ") &&
        !(CJK.test(text.slice(-1)) && CJK.test(it.str.charAt(0)));
      if (needSpace) text += " ";
    }
    text += it.str;
    prevEnd = it.x + it.w;
    prevSize = it.size;
  }
  // 行的字級：各段文字依字數加權後最多的那個（上標、下標比較小，不算）
  const weight = new Map<number, number>();
  for (const it of group) {
    const k = Math.round(it.size * 2) / 2;
    weight.set(k, (weight.get(k) ?? 0) + it.str.length);
  }
  let size = group[0]?.size ?? 10;
  let best = -1;
  for (const [k, w] of weight) {
    if (w > best) {
      best = w;
      size = k;
    }
  }
  // 小型大寫（「D」+「IRECT」）：整行的字母都是大寫，字首比其餘的字大一級；行的字級取大的那一級
  const letters = text.replace(/[^\p{L}]/gu, "");
  if (letters.length >= 4 && letters === letters.toUpperCase() && /\p{Lu}/u.test(letters)) {
    const top = Math.max(...group.map((g) => g.size));
    if (size < top && size >= 0.6 * top) size = top;
  }
  const first = group[0];
  const last = group[group.length - 1];
  const baseline = median(group.map((g) => g.y));
  const wordy = group.filter((g) => g.str.trim().length >= 3);
  const big = wordy.length ? Math.max(...wordy.map((g) => g.size)) : size;
  return {
    text: text.replace(/\s+/g, " ").trim(),
    x0: first?.x ?? 0,
    x1: last ? last.x + last.w : 0,
    y: baseline,
    size,
    big,
    col: "span",
    band,
    runs,
  };
}

/** 一行裡字數最多的字型；沒有字型資訊交回空字串。 */
function mainFont(ln: Line): string {
  const count = new Map<string, number>();
  for (const r of ln.runs) count.set(r.font, (count.get(r.font) ?? 0) + r.text.replace(/\s/g, "").length);
  let best = "";
  let n = -1;
  for (const [f, c] of count) {
    if (c > n) {
      best = f;
      n = c;
    }
  }
  return best;
}

/** 整行只有一個字型：交回那個字型，否則空字串。 */
function wholeFont(ln: Line): string {
  const f = ln.runs[0]?.font ?? "";
  return f !== "" && ln.runs.every((r) => r.font === f) ? f : "";
}

const SENTENCE_END = /[.!?:;]["'’”)\]]?$/;
// 行首的粗體或斜體小標：「Inner Product on Action Space.」「Aims.」「Definition 1」
const LEAD_IN = /^[A-Z][\p{L}\d\s,()'’-]*[\p{L})]\s*[.:]$|^[A-Z][a-z]+\s+\d+(\.\d+)*$/u;

/** 這一行以另一個字型的小標起頭（後面接內文字型），而且是新的一段。 */
function leadIn(ln: Line, prev: Line): boolean {
  const first = ln.runs[0];
  if (!first || ln.runs.length < 2) return false;
  const rest = mainFont({ ...ln, runs: ln.runs.slice(1) });
  if (rest === first.font || rest !== mainFont(prev)) return false;
  const t = first.text.replace(/\s+/g, " ").trim();
  if ((t.match(/\p{L}/gu) ?? []).length < 4 || !LEAD_IN.test(t)) return false;
  return SENTENCE_END.test(prev.text) || wholeFont(prev) !== "";
}

/** 整行一個字型、和內文不同、短、不以句點結尾、有字：多半是粗體或斜體的標題。 */
function fontHeadingLine(ln: Line, bodyFont: string): boolean {
  const f = wholeFont(ln);
  if (f === "" || f === bodyFont) return false;
  const words = ln.text.split(/\s+/).filter(Boolean);
  return words.length <= 12 && (/[A-Za-z]{3}/.test(ln.text) || CJK.test(ln.text)) && !/[.。]$/.test(ln.text);
}

/** 同一條基線、同一欄的格子併回一整行。 */
function mergeCells(cells: Line[]): Line[] {
  const groups = new Map<string, Line[]>();
  for (const c of cells) {
    const k = `${c.band}|${c.col}`;
    const g = groups.get(k);
    if (g) g.push(c);
    else groups.set(k, [c]);
  }
  const out: Line[] = [];
  for (const g of groups.values()) {
    g.sort((a, b) => a.x0 - b.x0);
    const first = g[0];
    if (!first) continue;
    if (g.length === 1) {
      out.push(first);
      continue;
    }
    let text = "";
    for (const c of g) {
      if (text === "") text = c.text;
      else if (CJK.test(text.slice(-1)) && CJK.test(c.text.charAt(0))) text += c.text;
      else text += " " + c.text;
    }
    const biggest = g.reduce((m, c) => (c.text.length > m.text.length ? c : m), first);
    const runs: Run[] = [];
    for (const r of g.flatMap((c) => c.runs)) {
      const last = runs[runs.length - 1];
      if (last && last.font === r.font) last.text += " " + r.text;
      else runs.push({ ...r });
    }
    out.push({
      runs,
      text,
      x0: first.x0,
      x1: Math.max(...g.map((c) => c.x1)),
      y: median(g.map((c) => c.y)),
      size: biggest.size,
      big: Math.max(...g.map((c) => c.big)),
      col: first.col,
      band: first.band,
    });
  }
  return out;
}

// ---------- 2. 欄 ----------

/** 雙欄的中縫：找不到交回 `null`。
 *  只用「窄行」找：寬度超過文字區 65% 的行本來就是橫跨兩欄的（篇名、摘要、作者單位、跨欄的公式與圖說），
 *  算進去會讓第 1 頁這種上半頁跨欄、下半頁雙欄的頁面被誤判成單欄。 */
function findGutter(lines: Line[], left: number, right: number): number | null {
  const width = right - left;
  if (lines.length < 12 || width < 200) return null;
  const narrow = lines.filter((ln) => ln.x1 - ln.x0 <= width * 0.65);
  if (narrow.length < 10) return null;
  let best: { x: number; cross: number } | null = null;
  for (let x = left + width * 0.3; x <= left + width * 0.7; x += 2) {
    let crossing = 0;
    let l = 0;
    let r = 0;
    for (const ln of narrow) {
      if (ln.x0 < x - 1 && ln.x1 > x + 1) crossing++;
      else if (ln.x1 <= x + 1) l++;
      else if (ln.x0 >= x - 1) r++;
    }
    if (l < 5 || r < 5) continue;
    if (!best || crossing < best.cross) best = { x, cross: crossing };
  }
  if (!best || best.cross > narrow.length * 0.2) return null;
  // 中縫取得寬一點：向左右各延伸到碰到窄行的文字為止，取中間
  let lo = best.x;
  let hi = best.x;
  const clear = (x: number) => !narrow.some((ln) => ln.x0 < x && ln.x1 > x);
  while (clear(lo - 1) && lo > left) lo -= 1;
  while (clear(hi + 1) && hi < right) hi += 1;
  return (lo + hi) / 2;
}

function assignColumns(lines: Line[], gutter: number | null) {
  for (const ln of lines) {
    if (gutter === null) ln.col = "left";
    else if (ln.x1 <= gutter + 2) ln.col = "left";
    else if (ln.x0 >= gutter - 2) ln.col = "right";
    else ln.col = "span";
  }
}

/** 一欄的左右邊界（PDF 頁面座標）。`kind`：雙欄的其中一欄、雙欄頁面上跨欄的區塊、單欄頁面的文字區。 */
export interface ColumnSpan {
  x0: number;
  x1: number;
  kind: "column" | "span" | "single";
}

/** 一批行的左右邊界：取較長的行的左緣與右緣（圖裡的標籤、縮排的首行、很短的末行不會把邊界拉歪）。 */
function lineExtent(lines: Line[]): { x0: number; x1: number } {
  const long = lines.filter((l) => l.text.length >= 25);
  const use = long.length >= 3 ? long : lines;
  return {
    x0: percentile(
      use.map((l) => l.x0),
      0.05,
    ),
    x1: percentile(
      use.map((l) => l.x1),
      0.95,
    ),
  };
}

/**
 * 雙擊放大到欄寬用（閱讀畫面，`pdfView.ts`）：頁面上 (x, y) 那一點（PDF 頁面座標）所在那一欄的左右邊界。
 * 欄照分段同一套規則找（`findGutter`）：雙欄頁面點在中縫左邊是左欄、右邊是右欄；點的那個高度是跨欄的區塊
 * （篇名、摘要、跨欄的圖說）時是整個文字區。單欄頁面是文字區。邊界取整頁那一欄的長行，所以同一欄點在不同高度，
 * 邊界都一樣。頁面沒有橫排的文字（掃描版）交回 `null`。
 */
export function columnAt(page: PageInput, x: number, y: number): ColumnSpan | null {
  const cells = buildLines(page.items);
  if (cells.length === 0) return null;
  const left = Math.min(...cells.map((l) => l.x0));
  const right = Math.max(...cells.map((l) => l.x1));
  const gutter = findGutter(cells, left, right);
  assignColumns(cells, gutter);
  const lines = mergeCells(cells);
  if (gutter === null) return { ...lineExtent(lines), kind: "single" };
  const side: Line["col"] = x <= gutter ? "left" : "right";
  // 點的那個高度最近的行（點在行裡是 0，行與行之間取到較近那一行的距離）
  const dist = (l: Line) => {
    const top = l.y + 0.9 * l.size;
    const bottom = l.y - 0.3 * l.size;
    return y > top ? y - top : y < bottom ? bottom - y : 0;
  };
  const all = lineExtent(lines);
  const best = Math.min(...lines.map(dist));
  const near = lines.filter((l) => dist(l) <= best + 0.5);
  const spanHere = near.some((l) => l.col === "span") && !near.some((l) => l.col === side);
  if (spanHere) return { ...all, kind: "span" };
  // 這一邊要像一欄內文：五行以上像文章的長行、寬度是文字區的三成五到六成，而且整頁在欄裡的文章行不比跨欄的少。
  // 圖表多的單欄頁面（表格的格子、圖裡的標籤）也會找到「中縫」，放大到那種「欄」會太窄，改放大到整個文字區
  const proseLine = (l: Line) => {
    const compact = l.text.replace(/\s+/g, "");
    return compact.length >= 25 && (l.text.match(/\p{L}/gu) ?? []).length >= 0.6 * compact.length;
  };
  const count = (col: Line["col"]) => lines.filter((l) => l.col === col && proseLine(l)).length;
  const mine = lines.filter((l) => l.col === side);
  const col = mine.length > 0 ? lineExtent(mine) : all;
  const ratio = (col.x1 - col.x0) / Math.max(1, all.x1 - all.x0);
  const twoColumns = count("left") + count("right") >= count("span");
  if (!twoColumns || count(side) < 5 || ratio < 0.35 || ratio > 0.62) return { ...all, kind: "single" };
  return { ...col, kind: "column" };
}

/** 閱讀順序：由上到下走；跨欄的行自成一塊，兩塊之間先排左欄、再排右欄。 */
function readingOrder(lines: Line[]): Line[] {
  const sorted = [...lines].sort((a, b) => b.y - a.y || a.x0 - b.x0);
  const out: Line[] = [];
  let left: Line[] = [];
  let right: Line[] = [];
  const flush = () => {
    out.push(...left, ...right);
    left = [];
    right = [];
  };
  for (const ln of sorted) {
    if (ln.col === "span") {
      flush();
      out.push(ln);
    } else if (ln.col === "left") left.push(ln);
    else right.push(ln);
  }
  flush();
  return out;
}

// ---------- 3. 段落 ----------

interface Para {
  lines: Line[];
  col: Line["col"];
}

function splitParagraphs(ordered: Line[], gapMedian: number, bodyFont: string): Para[] {
  const paras: Para[] = [];
  let cur: Para | null = null;
  // 每一欄的左右邊界（用來判斷縮排與「上一行很短」）：取較長的行的左緣與右緣的百分位數，
  // 圖裡的標籤、縮排的首行、很短的末行不會把邊界拉歪
  const bounds = new Map<string, { x0: number; x1: number }>();
  for (const col of ["left", "right", "span"] as const) {
    const all = ordered.filter((l) => l.col === col);
    if (all.length === 0) continue;
    const long = all.filter((l) => l.text.length >= 25);
    const use = long.length >= 3 ? long : all;
    bounds.set(col, {
      x0: percentile(
        use.map((l) => l.x0),
        0.2,
      ),
      x1: percentile(
        use.map((l) => l.x1),
        0.8,
      ),
    });
  }
  for (const ln of ordered) {
    const prev: Line | undefined = cur?.lines[cur.lines.length - 1];
    let brk = !cur || !prev;
    if (cur && prev) {
      const b = bounds.get(ln.col) ?? { x0: ln.x0, x1: ln.x1 };
      const gap = prev.y - ln.y;
      const size = Math.max(prev.size, ln.size);
      const indent = 0.7 * size + 1;
      // 這一段已有的行（加上這一行）最右到哪裡：比欄窄的區塊（摘要、置中的圖說與篇名）用它當右緣
      const curRight = Math.max(ln.x1, ...cur.lines.map((l) => l.x1));
      const inList = LIST_MARK.test(cur.lines[0]?.text ?? "") || REF_LABEL.test(cur.lines[0]?.text ?? "");
      // 縮排：比欄的左緣縮進，而且比上一行也縮進（整塊都縮排的區塊不會每行都切）；
      // 清單項目的續行是懸掛縮排，不算
      const indented = ln.x0 - b.x0 > indent && ln.x0 - prev.x0 > indent && !inList;
      // 上一行很短：離欄的右緣很遠，而且離這一段自己的右緣也很遠
      const shortPrev = prev.x1 < b.x1 - 4.5 * size && prev.x1 < curRight - 2.5 * size;
      // 字級變了：主要字級和最大字級都變了才算（行內的上下標、公式符號會讓主要字級跳動）
      const sizeChanged =
        Math.abs(prev.size - ln.size) > 0.14 * size && Math.abs(prev.big - ln.big) > 0.14 * Math.max(prev.big, ln.big);
      // 字體：行首換了字型的小標開新段；整行粗體或斜體的短標題前後都切
      const prevHead = fontHeadingLine(prev, bodyFont);
      const lnHead = fontHeadingLine(ln, bodyFont);
      // 只有編號的圖說首行，下一行接著是它的標題（只看行距）
      const label = cur.lines.length === 1 && CAPTION_LABEL.test(prev.text);
      // 置中的區塊（多行的篇名、置中的圖說）：兩行的中心對齊、左緣不對齊，不算縮排也不算上一行太短
      // （兩行都不是整行：置中的章節標題下面接整行的內文，中心也對齊，不算）
      const centered =
        Math.abs((ln.x0 + ln.x1) / 2 - (prev.x0 + prev.x1) / 2) <= 0.6 * size &&
        Math.abs(ln.x0 - prev.x0) > indent &&
        Math.min(ln.x0, prev.x0) > b.x0 + 0.5 * size &&
        Math.max(ln.x1, prev.x1) < b.x1 - 0.5 * size;
      const fontChanged =
        leadIn(ln, prev) ||
        (prevHead && mainFont(ln) !== wholeFont(prev)) ||
        (lnHead && mainFont(prev) !== wholeFont(ln));
      brk =
        fontChanged ||
        cur.col !== ln.col ||
        gap < -size * 0.3 || // 換欄之後回到上面
        gap > Math.max(gapMedian * 1.38, size * 1.75) ||
        (sizeChanged && !label) ||
        (indented && !label && !centered) ||
        (shortPrev && !label && !centered) ||
        LIST_MARK.test(ln.text) ||
        REF_LABEL.test(ln.text) ||
        isCaptionText(ln.text);
    }
    if (brk) {
      cur = { lines: [ln], col: ln.col };
      paras.push(cur);
    } else cur?.lines.push(ln);
  }
  return paras;
}

/** 把下一行接在後面：行尾的連字號接回同一個字，中日韓的字之間不加空白。 */
export function joinText(s: string, next: string): string {
  if (s === "") return next;
  if (/[A-Za-z]-$/.test(s) && /^[a-z]/.test(next)) return s.slice(0, -1) + next;
  if (CJK.test(s.slice(-1)) || CJK.test(next.charAt(0))) return s + next;
  return s + " " + next;
}

function joinLines(lines: Line[]): string {
  return lines.reduce((s, ln) => joinText(s, ln.text), "");
}

/**
 * 上標、下標貼回主行：比旁邊的字小、基線差不到主行字級六成、水平上挨著主行的片段，基線改成主行的。
 * `buildLines` 以一條基線的第一個片段為準分行，主行前面先排到上標時，同一行的下標會另成一行，
 * 接字時就跑到行尾（「ϵ = 10⁻³ pro-」後面接「s」，連字號接不回去）。只給 `segmentOf` 用，規則的分段不經過這裡。
 */
function snapScripts(items: RawItem[]): RawItem[] {
  const sizeOf = (it: RawItem) => Math.max(1, Math.hypot(it.transform[2] ?? 0, it.transform[3] ?? 1) || it.height || 1);
  const main = items.filter((it) => it.str.trim() !== "" && it.transform.length >= 6);
  return items.map((it) => {
    if (it.transform.length < 6) return it;
    const size = sizeOf(it);
    const x = it.transform[4] ?? 0;
    const y = it.transform[5] ?? 0;
    let best: RawItem | null = null;
    let bestDy = Infinity;
    for (const m of main) {
      const ms = sizeOf(m);
      if (size >= 0.85 * ms) continue;
      const mx = m.transform[4] ?? 0;
      const dy = Math.abs((m.transform[5] ?? 0) - y);
      if (dy > 0.6 * ms || x < mx - ms || x > mx + m.width + ms) continue;
      if (dy < bestDy) {
        bestDy = dy;
        best = m;
      }
    }
    if (!best) return it;
    const transform = [...it.transform];
    transform[5] = best.transform[5] ?? y;
    return { ...it, transform };
  });
}

/**
 * 一組文字片段（版面模型的一個區塊裡的，`regions.ts`）組成一段：上標、下標貼回主行之後依基線分行（`buildLines`），
 * 同一條基線的幾格由左到右接起來，行與行由上到下照 `joinText` 接。方框與字級的算法和規則的段落相同。
 * 沒有字（或都是斜放的字）交回 `null`。
 */
export function segmentOf(page: number, items: RawItem[], kind: SegmentKind): LayoutSegment | null {
  const cells = buildLines(snapScripts(items));
  if (cells.length === 0) return null;
  const bands = new Map<number, Line[]>();
  for (const c of cells) {
    const b = bands.get(c.band);
    if (b) b.push(c);
    else bands.set(c.band, [c]);
  }
  const rows = [...bands.values()]
    .map((cs) => {
      cs.sort((a, b) => a.x0 - b.x0);
      const text = cs.reduce((s, c) => (s === "" ? c.text : CJK.test(s.slice(-1)) && CJK.test(c.text.charAt(0)) ? s + c.text : `${s} ${c.text}`), "");
      return { text, y: median(cs.map((c) => c.y)) };
    })
    .sort((a, b) => b.y - a.y);
  const text = rows.reduce((s, r) => joinText(s, r.text), "").replace(/\s+/g, " ").trim();
  if (text === "") return null;
  // 上下緣也要包住貼回主行之前的上標、下標
  const raw = buildLines(items);
  const bbox: [number, number, number, number] = [
    Math.min(...cells.map((l) => l.x0)) - 0.5,
    Math.min(...[...cells, ...raw].map((l) => l.y - 0.22 * l.size)) - 0.5,
    Math.max(...cells.map((l) => l.x1)) + 0.5,
    Math.max(...[...cells, ...raw].map((l) => l.y + 0.85 * l.size)) + 0.5,
  ];
  return { page, bbox, kind, size: median(cells.map((l) => l.size)), source: text };
}

// ---------- 全部頁面 ----------

interface Candidate {
  page: number;
  seg: LayoutSegment;
  /** 在頁面上下緣。 */
  edge: boolean;
  /** 離上下緣稍遠的一行短字（期刊的頁尾常在 7% 之外）：在多頁同一高度重複出現才當成頁首頁尾。 */
  outer: string | null;
  col: Line["col"];
  /** 整段是一兩行和內文不同字型的短行（粗體或斜體的標題）。 */
  fontHead: boolean;
  /** 在雙欄頁面的其中一欄裡（不跨欄），而且有三行以上（被切開的作者行不算）。 */
  inColumn: boolean;
}

/** 頁首頁尾比對用：數字換成 #、空白收乾淨。 */
function edgeKey(s: string): string {
  return s.replace(/\d+/g, "#").replace(/\s+/g, " ").trim().toLowerCase();
}

/** 要翻的段落（版面紀錄、翻譯、重點筆記用）。等於 `layoutDocument(pages).segments`。 */
export function layoutPages(pages: PageInput[]): LayoutSegment[] {
  return layoutDocument(pages).segments;
}

/** `layoutDocument` 的結果。 */
export interface DocumentLayout {
  /** 要翻的段落，和 `layoutPages` 交出的逐項相同；沒有 `reference`。 */
  segments: LayoutSegment[];
  /**
   * 參考文獻區的段落（`kind` 是 `"reference"`），依閱讀順序：「References」標題之後到附錄為止的，以及沒有標題時
   * 認成書目條目與它續段的。「References」標題本身不算。給逐篇處理的外掛切條目用。
   */
  references: LayoutSegment[];
}

/** 整份文件的版面：要翻的段落，另外交回參考文獻區。 */
export function layoutDocument(pages: PageInput[]): DocumentLayout {
  const cands: Candidate[] = [];
  const sizes: number[] = [];
  // 內文的字型：整份文件字數最多的字型
  const fontChars = new Map<string, number>();
  for (const p of pages)
    for (const it of p.items) if (it.fontName) fontChars.set(it.fontName, (fontChars.get(it.fontName) ?? 0) + it.str.length);
  let bodyFont = "";
  for (const [f, n] of fontChars) if (n > (fontChars.get(bodyFont) ?? -1)) bodyFont = f;
  pages.forEach((p, pi) => {
    const [, vy0, , vy1] = p.view;
    const cells = buildLines(p.items);
    if (cells.length === 0) return;
    const left = Math.min(...cells.map((l) => l.x0));
    const right = Math.max(...cells.map((l) => l.x1));
    const gutter = findGutter(cells, left, right);
    assignColumns(cells, gutter);
    const lines = mergeCells(cells);
    for (const ln of lines) for (let i = 0; i < Math.min(ln.text.length, 80); i++) sizes.push(ln.size);
    const ordered = readingOrder(lines);
    const gaps: number[] = [];
    for (let i = 1; i < ordered.length; i++) {
      const a = ordered[i - 1];
      const b = ordered[i];
      if (a && b && a.col === b.col && a.y - b.y > 0 && Math.abs(a.size - b.size) < 0.5) gaps.push(a.y - b.y);
    }
    const gapMedian = median(gaps);
    const h = vy1 - vy0;
    for (const para of splitParagraphs(ordered, gapMedian, bodyFont)) {
      const ls = para.lines;
      const text = joinLines(ls);
      if (text === "") continue;
      const size = median(ls.map((l) => l.size));
      const y1 = Math.max(...ls.map((l) => l.y + 0.85 * l.size));
      const y0 = Math.min(...ls.map((l) => l.y - 0.22 * l.size));
      const bbox: [number, number, number, number] = [
        Math.min(...ls.map((l) => l.x0)) - 0.5,
        y0 - 0.5,
        Math.max(...ls.map((l) => l.x1)) + 0.5,
        y1 + 0.5,
      ];
      const nearTop = y1 > vy1 - h * 0.07;
      const nearBottom = y0 < vy0 + h * 0.07;
      const outerBand = y1 > vy1 - h * 0.12 || y0 < vy0 + h * 0.12;
      cands.push({
        page: pi,
        seg: { page: pi, bbox, kind: "body", size, source: text },
        edge: (nearTop || nearBottom) && text.length < 120,
        outer: outerBand && ls.length === 1 && text.length <= 120 ? `${edgeKey(text)}@${Math.round((y0 - vy0) / 4)}` : null,
        col: para.col,
        fontHead: ls.length <= 2 && ls.every((l) => fontHeadingLine(l, bodyFont)),
        inColumn: gutter !== null && para.col !== "span" && ls.length >= 3,
      });
    }
  });

  const body = median(sizes);

  // 表格的一列、圖裡並排的標籤：同一頁同一條水平線上有三段以上像格子的單行短文字——帶數字，或四個詞以內
  // （雙欄的內文同一高度最多兩段；內文被顯示公式隔開的半句「where γ denotes the minimum required …」不帶數字、詞也多）
  const singleLine = (c: Candidate) => {
    const words = c.seg.source.split(/\s+/).length;
    return c.seg.bbox[3] - c.seg.bbox[1] < 1.8 * c.seg.size && words <= 8 && (words <= 4 || /\d/.test(c.seg.source));
  };
  const inRow = (c: Candidate): boolean =>
    singleLine(c) &&
    cands.filter(
      (d) => d.page === c.page && singleLine(d) && Math.abs(d.seg.bbox[3] - c.seg.bbox[3]) < 0.6 * c.seg.size,
    ).length >= 3;

  // 在欄內文字旁：正上方或正下方不遠處有一段像內文的長句（左緣對齊或置中對齊）。
  // 圖裡的標籤、座標軸的字旁邊沒有這種文字；欄裡的「where」、「Funding」這種短行有
  const solid = cands.filter((c) => {
    const t = c.seg.source;
    const compact = t.replace(/\s+/g, "").length;
    const letters = (t.match(/\p{L}/gu) ?? []).length;
    return t.length >= 60 && t.split(/\s+/).length >= 8 && letters >= 0.75 * compact && !isCaptionText(t) && !c.edge;
  });
  /** 在註腳的位置：頁面下方三成以內，而且正下方沒有內文字級的段落。 */
  const footnote = (c: Candidate): boolean => {
    const [, vy0, , vy1] = pages[c.page]?.view ?? [0, 0, 0, 0];
    const [x0, y0, x1] = c.seg.bbox;
    if (y0 > vy0 + 0.3 * (vy1 - vy0)) return false;
    return !cands.some(
      (d) =>
        d !== c &&
        d.page === c.page &&
        d.seg.bbox[3] < y0 &&
        d.seg.bbox[0] < x1 &&
        d.seg.bbox[2] > x0 &&
        d.seg.size >= body * 0.95 &&
        d.seg.source.split(/\s+/).length >= 8,
    );
  };
  /** `sameSize`：旁邊的長句要和它同一個字級（小字的註腳旁邊是註腳；公式的上下標旁邊是大一級的內文，不算） */
  const beside = (c: Candidate, sameSize: boolean): boolean => {
    const [x0, y0, x1, y1] = c.seg.bbox;
    const sz = c.seg.size;
    return solid.some((d) => {
      if (d === c || d.page !== c.page) return false;
      if (sameSize && Math.abs(d.seg.size - sz) > 0.12 * sz) return false;
      const [dx0, dy0, dx1, dy1] = d.seg.bbox;
      const vgap = Math.max(dy0 - y1, y0 - dy1);
      if (vgap > 2 * Math.max(sz, d.seg.size)) return false;
      return Math.abs(x0 - dx0) <= 1.5 * sz || Math.abs((x0 + x1) / 2 - (dx0 + dx1) / 2) <= 1.5 * sz;
    });
  };

  // 頁首頁尾：上下緣的短文字，單純的頁碼，或在三頁以上重複出現的；離上下緣稍遠的一行短字在三頁以上同一高度重複出現的
  const edgeCount = new Map<string, Set<number>>();
  for (const c of cands) {
    if (!c.edge) continue;
    const k = edgeKey(c.seg.source);
    const set = edgeCount.get(k) ?? new Set<number>();
    set.add(c.page);
    edgeCount.set(k, set);
  }
  const repeatMin = Math.min(3, Math.max(2, Math.ceil(pages.length * 0.3)));
  // 稍遠的一行短字：同樣的字（數字不計）在同一高度出現的頁數
  const outerCount = new Map<string, Set<number>>();
  for (const c of cands) {
    if (c.outer === null) continue;
    const set = outerCount.get(c.outer) ?? new Set<number>();
    set.add(c.page);
    outerCount.set(c.outer, set);
  }
  const repeatedOuter = (c: Candidate) => c.outer !== null && (outerCount.get(c.outer)?.size ?? 0) >= repeatMin;
  const isPageNumber = (s: string) => /^\s*(page\s*)?(\d{1,4}|[ivxlc]{1,6})\s*(of\s*\d+)?\s*$/i.test(s);

  // 第 1 頁的篇名：第 1 頁字級最大的段落（要翻）；它之後、摘要或第一個章節標題之前的是作者與單位（不翻）
  // （至少兩個詞：首字放大的「F」不算）
  // （靠近頁頂的篇名也算，只排除在多頁重複的頁首）
  const firstPage = cands.filter(
    (c) =>
      c.page === 0 &&
      /\S\s+\S/.test(c.seg.source) &&
      !(c.edge && (edgeCount.get(edgeKey(c.seg.source))?.size ?? 0) >= repeatMin),
  );
  const titleSize = firstPage.length ? Math.max(...firstPage.map((c) => c.seg.size)) : 0;
  let seenTitle = false;
  let inFrontMatter = true;

  const out: LayoutSegment[] = [];
  const references: LayoutSegment[] = [];
  let inReferences = false;
  let title: LayoutSegment | null = null;
  // 上一個候選段落是參考文獻條目（在哪一頁哪一欄）；最近一次看到條目的頁
  let prevRef: { page: number; col: Line["col"] } | null = null;
  let refPage = -2;
  for (const c of cands) {
    const s = c.seg;
    const text = s.source;
    if (c.edge && (isPageNumber(text) || (edgeCount.get(edgeKey(text))?.size ?? 0) >= repeatMin)) continue;
    if (repeatedOuter(c)) continue;
    if (isPageNumber(text)) continue;

    // 篇名排成兩三行、被切成幾段（置中的行寬不一、上一行很短）：緊接在篇名下面、同樣字級的段落併進篇名，一起翻
    if (title && c.page === 0 && s.size >= titleSize - 0.01) {
      const gap = title.bbox[1] - s.bbox[3];
      if (gap < 1.2 * s.size && gap > -0.5 * s.size) {
        title.source = joinText(title.source, text);
        title.bbox = [
          Math.min(title.bbox[0], s.bbox[0]),
          Math.min(title.bbox[1], s.bbox[1]),
          Math.max(title.bbox[2], s.bbox[2]),
          Math.max(title.bbox[3], s.bbox[3]),
        ];
        continue;
      }
    }

    // 作者與單位：第 1 頁篇名之後、摘要之前的全部不翻（圖說除外）。摘要的開頭是「Abstract」這類標籤、
    // 第一個章節標題，或第一段像文章的長段落（物理期刊的摘要沒有標籤）。
    // 前置區之後，第 1 頁的小字註腳裡的作者資料（「X is with the Department…」、通訊作者、電子郵件）也不翻
    if (c.page === 0 && inFrontMatter) {
      // 篇名之前已經有一段內文：這一頁不是論文的首頁（或認不出篇名），不判斷前置區
      // （頁面最上面一成的投稿聲明這類文字不算）
      const [, vy0, , vy1] = pages[0]?.view ?? [0, 0, 0, 0];
      const top = s.bbox[1] > vy1 - 0.1 * (vy1 - vy0);
      if (!seenTitle && !top && text.split(/\s+/).length >= 20) inFrontMatter = false;
      else if (!seenTitle && s.size >= titleSize - 0.01 && titleSize > 0) {
        seenTitle = true;
        title = s;
      } else if (seenTitle && s.size < titleSize - 0.5) {
        title = null;
        const columnText = c.inColumn && text.split(/\s+/).length >= 20;
        // （「1 Institut für Physik…」這種帶編號的單位不是章節標題）
        const section = HEADING_NUMBERED.test(text) && text.split(/\s+/).length <= 10 && !AUTHORISH_WORDS.test(text);
        if (
          ABSTRACT_START.test(text) ||
          section ||
          FRONT_END.test(text) ||
          (prose(text) && !isCaptionText(text)) ||
          columnText
        )
          inFrontMatter = false;
        else if (!isCaptionText(text)) continue;
      }
    } else if (c.page === 0) {
      if (
        s.size < body * 0.92 &&
        footnote(c) &&
        !isCaptionText(text) &&
        (AUTHORISH_WORDS.test(text) || AUTHOR_NOTE.test(text) || EMAIL.test(text)) &&
        !FUNDING.test(text)
      )
        continue;
    } else {
      inFrontMatter = false;
    }

    const words = text.split(/\s+/).filter(Boolean).length;
    const letters = (text.match(/\p{L}/gu) ?? []).length;
    const compact = text.replace(/\s+/g, "").length;
    const isCaption = isCaptionText(text);
    const heading =
      !isCaption &&
      words <= 14 &&
      (s.size > body * 1.08 || HEADING_WORDS.test(text) || c.fontHead) &&
      !/[.。]$/.test(text);
    s.kind = isCaption ? "caption" : heading ? "heading" : "body";

    if (REFERENCES.test(text) && words <= 3) {
      inReferences = true;
      continue; // 「References」本身不必翻
    }
    if (inReferences) {
      // 參考文獻一直到下一個明顯的章節標題（附錄等）為止全部不翻，跨欄、跨頁都一樣；
      // 不再要求每個條目帶年份或編號（物理、天文期刊的條目常常沒有）。
      // 附錄的標題和下面的第一段併成一段時（標題後面沒有空行）也算附錄開始
      if (
        (s.kind === "heading" && !REFERENCES.test(text) && (s.size > body * 1.05 || APPENDIX.test(text))) ||
        /^(appendix|appendices|附錄)\b/i.test(text)
      )
        inReferences = false;
      else {
        references.push({ ...s, kind: "reference" });
        continue;
      }
    }
    // 沒有標題的參考文獻（APS 這類）：開頭是作者名單的書目條目，整段不翻（圖說不算；
    // 編號制條目只有一行時會被當成標題，也算）。條目常被切成幾段（換了字型的書名、跨欄、跨頁），所以
    // 這一頁或上一頁已經有條目時，「[12] A. Name」起頭的段落也算；緊接在條目後面、同一頁同一欄的段落是它的續段，
    // 碰到章節標題、圖說或不帶年份的長段落才停
    const afterRef = prevRef !== null && prevRef.page === c.page && prevRef.col === c.col;
    const refStart = REF_LABEL.test(text) && refPage >= c.page - 1;
    // 續段要像書目：帶年份、名縮寫加姓、或很短（換了字型的書名）；不帶這些的一般句子不算
    const refTail =
      afterRef &&
      !isCaption &&
      !(s.kind === "heading" && (s.size > body * 1.05 || HEADING_WORDS.test(text))) &&
      (YEAR.test(text) || REF_NAME.test(text) || text.split(/\s+/).length <= 10);
    prevRef = null;
    if (((s.kind === "body" || REF_LABEL.test(text)) && isReferenceEntry(text)) || refStart || refTail) {
      prevRef = { page: c.page, col: c.col };
      refPage = c.page;
      references.push({ ...s, kind: "reference" });
      continue;
    }
    // 公式的碎片（分母、矩陣的一列、上下標）：不翻
    if (!isCaption && formulaFragment(text)) continue;
    // 表格的格子、圖裡並排的標籤：不翻
    if (!isCaption && inRow(c)) continue;

    // 純網址（註腳常見，前面可能帶註腳編號）
    if (/^[\d*†‡]*\s*(https?:\/\/|www\.)\S+$/i.test(text)) continue;
    // 公式與符號：字母太少
    if (compact > 0 && letters / compact < 0.45 && s.kind !== "heading") continue;
    // 顯示公式：行尾是公式編號「(3)」，字母比例又不像一般句子
    if (EQ_NUMBER.test(text) && words <= 25 && letters / Math.max(1, compact) < 0.75 && !isCaption) continue;
    // 表格：一半以上的詞是數字（含小數、百分比、±）
    const tokens = text.split(/\s+/).filter(Boolean);
    const numeric = tokens.filter((w) => /^[-+±]?[\d.,]+%?$|^[\d.]+±[\d.]+$/.test(w)).length;
    if (!isCaption && tokens.length >= 4 && numeric / tokens.length >= 0.5) continue;
    // 圖內的字：很小的字級（圖說除外）、單字或兩個字的標籤、純數字。
    // 前兩條只對不在欄內文字旁的段落生效；只憑字型認出的標題不算豁免（圖裡的字常用另一個字型）
    if (!isCaption) {
      if (s.size < body * 0.78 && !beside(c, true)) continue;
      const sureHeading = s.kind === "heading" && (s.size > body * 1.08 || HEADING_WORDS.test(text));
      // 兩個字以內的行：欄裡的一般英文字（「where」「Funding」「Keywords」）保留，公式的碎片（「XK」「l l l」）不留
      // 顯示公式之後單獨一行的「where」「with」是正文的開頭，旁邊常常只有公式
      // （公式編號「(B2)」可能和它落在同一條基線上，先拿掉）
      const core = text.replace(/\(\s*[A-Z]?\d{1,3}[a-z]?\s*\)/g, "").trim();
      const word =
        (/^[A-Za-z][a-z]{2,}\b/.test(core) && beside(c, false)) || /^(where|wherein|whereby|with)[,:]?$/.test(core);
      if (words <= 2 && !sureHeading && !word && !CJK.test(text)) continue;
      if (words < 1 || letters < 2) continue;
    }
    out.push(s);
  }
  return { segments: out, references };
}
