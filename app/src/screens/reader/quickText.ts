// 選取文字的快速翻譯的純計算：選取前後各一句、選的是不是已經是譯文的語言、卡片放在哪裡、
// 送出的同意記在哪一個鍵。不碰畫面、不連網，`scripts/quick-translate-test.mjs` 直接測。

/** 選取最多幾個字（同外殼的 `MAX_QUICK_CHARS`）；更長的請使用者改用全文翻譯。 */
export const MAX_QUICK_CHARS = 2000;
/** 前文、後文各最多幾個字（同外殼的 `MAX_CONTEXT_CHARS`）。 */
const MAX_CONTEXT_CHARS = 400;

/** 句尾後面不算斷句的縮寫（論文常見的；只影響前後文多送或少送幾個字）。 */
const ABBREVIATIONS = new Set(["e.g", "i.e", "et al", "al", "fig", "figs", "eq", "eqs", "cf", "vs", "sec", "tab", "no", "ref", "refs", "etc", "approx", "resp"]);

/** 這一處的句點是不是縮寫的（`e.g.`、`Fig.`、`et al.`、單一個大寫字母的名字縮寫）。 */
function isAbbreviation(text: string, dot: number): boolean {
  const head = text.slice(Math.max(0, dot - 8), dot);
  const word = /([A-Za-z]+(?:\.[A-Za-z]+)*)$/.exec(head)?.[1] ?? "";
  if (word === "") return false;
  if (/^[A-Z]$/.test(word)) return true;
  const lower = word.toLowerCase();
  return ABBREVIATIONS.has(lower) || ABBREVIATIONS.has(`et ${lower}`);
}

/** 每一句的結束位置（句尾標點與它後面的引號、括號之後）。 */
export function sentenceEnds(text: string): number[] {
  const out: number[] = [];
  for (const m of text.matchAll(/[.!?]["'”’)\]]*(?=\s|$)|[。！？]["'”’）」』]*/g)) {
    const at = m.index ?? 0;
    if (m[0].startsWith(".") && isAbbreviation(text, at)) continue;
    out.push(at + m[0].length);
  }
  return out;
}

const hasLetters = (s: string) => /[\p{L}\p{N}]/u.test(s);

/**
 * 選取（`text` 裡 `start` 到 `end`）前後各一句，只用來決定詞義：選取從句子中間開始時，前文是這一句在選取前面的部分；
 * 從句首開始時是前一句。後文同理。各最多 400 字（前文留靠近選取的後段）。沒有就是 `null`。
 */
export function sentenceContext(text: string, start: number, end: number): { before: string | null; after: string | null } {
  const ends = sentenceEnds(text);
  const tidy = (s: string) => s.replace(/\s+/g, " ").trim();
  // 前文
  const s0 = ends.filter((e) => e <= start).at(-1) ?? 0;
  let before = text.slice(s0, start);
  if (!hasLetters(before)) {
    const s1 = ends.filter((e) => e < s0).at(-1) ?? 0;
    before = s0 > 0 ? text.slice(s1, s0) : "";
  }
  // 後文
  const e0 = ends.find((e) => e >= end) ?? text.length;
  let after = text.slice(end, e0);
  if (!hasLetters(after)) {
    const e1 = ends.find((e) => e > e0) ?? text.length;
    after = text.slice(e0, e1);
  }
  const b = tidy(before);
  const a = tidy(after);
  return {
    before: b === "" ? null : b.length > MAX_CONTEXT_CHARS ? b.slice(b.length - MAX_CONTEXT_CHARS) : b,
    after: a === "" ? null : a.slice(0, MAX_CONTEXT_CHARS),
  };
}

/**
 * 行尾的連字號接回下一行（選取與文字層在換行處補了空格，`it- erative` 變成 `it-erative`）：送出前整理，
 * 模型讀得順。連字號留著（分不出是斷字還是本來就有的 `near-field`）。
 */
export function joinHyphens(text: string): string {
  return text.replace(/(\p{L})- (?=\p{L})/gu, "$1-");
}

/** 文字大多是中日韓的字（選的是譯本上已經翻好的部分）。 */
export function mostlyCjk(text: string): boolean {
  const letters = text.match(/\p{L}/gu)?.length ?? 0;
  if (letters === 0) return false;
  const cjk = text.match(/[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/gu)?.length ?? 0;
  return cjk / letters > 0.5;
}

/**
 * 選取的文字要不要給「翻譯」：選的已經是譯文的語言（譯本 PDF 上的中文）時不給。原文就在旁邊（切回原文、
 * 疊譯文點一下暫看原文），離線的 Firefox 翻譯也只有英翻中。
 */
export function offersQuick(text: string, targetLang: string): boolean {
  if (text.trim() === "") return false;
  return !(/^(zh|ja|ko)/i.test(targetLang) && mostlyCjk(text));
}

export interface Rect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface CardSpot {
  left: number;
  width: number;
  /** 放在選取上方時從畫面下緣算的距離；放在下方時是 `undefined`。 */
  bottom?: number;
  /** 放在選取下方時的上緣；放在上方時是 `undefined`。 */
  top?: number;
  maxHeight: number;
  above: boolean;
}

const GAP = 8;
const SIDE = 12;
/** 卡片至少要這麼高（標題列、兩三行譯文、按鈕）。 */
const MIN_CARD = 170;
/** 卡片最多估到這麼高（再長的譯文在卡片裡捲）。 */
const MAX_NEED = 420;

/** 照選了幾個字估卡片要多高：標題列與按鈕約 130，譯文每行約 25（中文譯文的字數約是英文原文的一半）。 */
export function cardNeed(chars: number, width: number): number {
  const perLine = Math.max(10, Math.floor((width - 24) / 16));
  const lines = Math.max(2, Math.ceil((chars * 0.55) / perLine));
  return Math.min(MAX_NEED, Math.max(MIN_CARD, 130 + lines * 25));
}

/**
 * 卡片放在哪裡（打開時算一次，之後捲動或縮放就收起，不跟著重算）：選取上方；上方放不下估計的高度（`cardNeed`）
 * 而下方放得下就放下方；兩邊都放不下時放空間大的一邊，譯文在卡片裡捲。
 * 寬度最多 360，對齊選取的中間，左右不超出畫面（雙欄論文的右欄、放大時選取跑到畫面外的部分都夾回來）。
 * `topLimit` 是畫面上方被頂列占掉的高度；`chars` 是選了幾個字。
 */
export function placeCard(sel: Rect, vw: number, vh: number, topLimit: number, chars = 0): CardSpot {
  const width = Math.max(200, Math.min(vw - SIDE * 2, 360));
  const center = (Math.max(0, sel.left) + Math.min(vw, sel.right)) / 2;
  const left = Math.min(Math.max(SIDE, center - width / 2), Math.max(SIDE, vw - width - SIDE));
  const top = Math.max(sel.top, topLimit);
  const bottom = Math.min(sel.bottom, vh);
  const spaceAbove = top - topLimit - GAP;
  const spaceBelow = vh - bottom - GAP * 2;
  const need = cardNeed(chars, width);
  const above = spaceAbove >= need || (spaceBelow < need && spaceAbove >= spaceBelow);
  if (above) {
    return { left, width, bottom: vh - top + GAP, maxHeight: Math.max(120, spaceAbove - GAP), above: true };
  }
  return { left, width, top: bottom + GAP, maxHeight: Math.max(120, spaceBelow - GAP), above: false };
}

/** 送出的同意記在這台裝置上的鍵：服務與它實際送到的主機（主機換了要再問一次）。 */
export function consentKey(serviceId: string, host: string): string {
  return `${serviceId}@${host}`;
}
