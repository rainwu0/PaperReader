// 譯文排字：把一段譯文排進原段落的範圍。純函式，不碰 PDF，所以有單元測試（`scripts/typeset-test.mjs`）。
//
// - 換行：中日韓的字隨處可斷、英文單字整個移到下一行，太長的字（網址）才在字中間斷；
//   不讓「，。、」等收尾標點出現在行首（排不下時吊在行尾外，最多一個字寬）、不讓「（「」等開頭標點留在行尾。
// - 字級：從原文的字級開始，放不下就一點一點縮小，不小於可讀的下限；縮到下限還放不下時，
//   允許往下多出一點（`maxOverflow`），再不行就截斷並在最後一行補「…」。
// - 對齊：內文左右對齊（行末的空隙平均分給每個字，每個字最多加字級的五分之一，空隙太大就靠左），
//   置中的標題逐行置中，其餘靠左。最後一行永遠靠左。
// 座標單位是 PDF 的點；寬度由呼叫的人用 `advance` 提供（字型怎麼量就怎麼排）。

export type Align = "left" | "justify" | "center";

export interface TypesetInput {
  text: string;
  /** 範圍的寬與高（點）。 */
  width: number;
  height: number;
  /** 原文的字級；排的時候從這裡開始。 */
  size: number;
  align: Align;
  /** 一個字的寬度（千分之一字級）；字型沒有這個字回傳 `null`（那個字不畫，寬度算 0）。 */
  advance: (cp: number) => number | null;
  /** 字級下限；預設是原字級的六成，且不低於 5 點。 */
  minSize?: number;
  /** 行距是字級的幾倍。 */
  lineHeight?: number;
  /** 縮到下限仍放不下時，高度最多超出範圍的比例（預設 0.25）。 */
  maxOverflow?: number;
}

export interface TypesetLine {
  text: string;
  /** 這一行左端離範圍左緣多遠。 */
  dx: number;
  /** 每個字多加的間距（左右對齊用）。 */
  charSpace: number;
}

export interface TypesetResult {
  size: number;
  lineHeight: number;
  lines: TypesetLine[];
  /** 文字比範圍多出多少（點）；0 是放得下。 */
  overflow: number;
  /** 放不下而砍掉了後面的字。 */
  truncated: boolean;
}

const SIZE_STEP = 0.25;
const LINE_HEIGHT = 1.3;
const MAX_OVERFLOW = 0.25;
const MAX_CHAR_SPACE = 0.2; // 每個字最多加字級的幾倍

const CJK = /[⺀-鿿가-힯豈-﫿＀-￯　-〿\u{20000}-\u{2fa1f}]/u;
/** 不能出現在行首的標點。 */
const NO_START = new Set("，。、；：！？）」』】》〉〕］｝”’…—、,.;:!?)]}%‰．・ー".split(""));
/** 不能出現在行尾的標點。 */
const NO_END = new Set("（「『【《〈〔［｛“‘([{".split(""));

interface Atom {
  text: string;
  /** 能不能在它前面換行。 */
  breakable: boolean;
  space: boolean;
}

function atomize(text: string): Atom[] {
  const out: Atom[] = [];
  let word = "";
  const flush = () => {
    if (word !== "") out.push({ text: word, breakable: true, space: false });
    word = "";
  };
  for (const ch of text.replace(/\s+/g, " ").trim()) {
    if (ch === " ") {
      flush();
      out.push({ text: " ", breakable: true, space: true });
    } else if (CJK.test(ch)) {
      flush();
      out.push({ text: ch, breakable: true, space: false });
    } else word += ch;
  }
  flush();
  return out;
}

function widthOf(s: string, size: number, adv: TypesetInput["advance"]): number {
  let w = 0;
  for (const ch of s) w += adv(ch.codePointAt(0) ?? 0) ?? 0;
  return (w * size) / 1000;
}

function glyphCount(s: string, adv: TypesetInput["advance"]): number {
  let n = 0;
  for (const ch of s) if (adv(ch.codePointAt(0) ?? 0) !== null) n++;
  return n;
}

/** 在某個字級下換行：交回每一行的文字（已去掉行首行尾的空白）。 */
function wrap(atoms: Atom[], size: number, maxW: number, adv: TypesetInput["advance"]): string[] {
  const lines: string[] = [];
  let cur: Atom[] = [];
  let curW = 0;
  const text = (xs: Atom[]) => xs.map((a) => a.text).join("").replace(/\s+$/, "");
  const push = () => {
    const t = text(cur);
    if (t !== "") lines.push(t);
    cur = [];
    curW = 0;
  };
  const add = (a: Atom) => {
    cur.push(a);
    curW += widthOf(a.text, size, adv);
  };
  for (const a of atoms) {
    if (a.space && cur.length === 0) continue;
    const w = widthOf(a.text, size, adv);
    if (curW + w <= maxW + 0.01) {
      add(a);
      continue;
    }
    if (a.space) {
      push();
      continue;
    }
    // 收尾標點排不下：吊在行尾（最多多出一個字寬），不放到下一行的行首
    const first = a.text[0] ?? "";
    if (a.text.length === 1 && NO_START.has(first) && cur.length > 0 && curW + w <= maxW + size) {
      add(a);
      continue;
    }
    // 換行；行尾若是開頭標點，連它一起帶到下一行
    const carry: Atom[] = [];
    while (cur.length > 0) {
      const last = cur[cur.length - 1];
      if (last && last.text.length === 1 && NO_END.has(last.text)) carry.unshift(cur.pop() as Atom);
      else break;
    }
    if (cur.length > 0) push();
    else cur = [];
    curW = 0;
    for (const c of carry) add(c);
    // 單字比一整行還寬（網址）：在字中間斷開
    if (widthOf(a.text, size, adv) > maxW && !CJK.test(a.text)) {
      let piece = "";
      for (const ch of a.text) {
        const next = piece + ch;
        if (piece !== "" && curW + widthOf(next, size, adv) > maxW) {
          add({ text: piece, breakable: true, space: false });
          push();
          piece = ch;
        } else piece = next;
      }
      if (piece !== "") add({ text: piece, breakable: true, space: false });
    } else add(a);
  }
  push();
  return lines;
}

export function typeset(input: TypesetInput): TypesetResult {
  const { width, height, advance } = input;
  const lhRatio = input.lineHeight ?? LINE_HEIGHT;
  const s0 = Math.max(1, input.size);
  const minSize = Math.min(s0, input.minSize ?? Math.max(5, s0 * 0.6));
  const maxOverflow = input.maxOverflow ?? MAX_OVERFLOW;
  const atoms = atomize(input.text);

  let size = s0;
  let lines: string[] = [];
  for (;;) {
    lines = wrap(atoms, size, width, advance);
    if (lines.length * size * lhRatio <= height + 0.01 || size <= minSize + 1e-9) break;
    size = Math.max(minSize, size - SIZE_STEP);
  }
  const lineHeight = size * lhRatio;
  let overflow = Math.max(0, lines.length * lineHeight - height);
  let truncated = false;
  if (overflow > height * maxOverflow + 0.01) {
    // 放不下：留住放得下的行數，最後一行補「…」
    const keep = Math.max(1, Math.floor((height * (1 + maxOverflow)) / lineHeight));
    if (lines.length > keep) {
      lines = lines.slice(0, keep);
      let last = lines[keep - 1] ?? "";
      while (last !== "" && widthOf(last + "…", size, advance) > width) last = last.slice(0, -1);
      lines[keep - 1] = last + "…";
      truncated = true;
      overflow = Math.max(0, lines.length * lineHeight - height);
    }
  }

  const out: TypesetLine[] = lines.map((text, i) => {
    const w = widthOf(text, size, advance);
    const isLast = i === lines.length - 1;
    if (input.align === "center") return { text, dx: Math.max(0, (width - w) / 2), charSpace: 0 };
    if (input.align === "justify" && !isLast) {
      const n = glyphCount(text, advance);
      const extra = width - w;
      const space = n > 1 ? extra / (n - 1) : 0;
      if (space > 0 && space <= size * MAX_CHAR_SPACE) return { text, dx: 0, charSpace: space };
    }
    return { text, dx: 0, charSpace: 0 };
  });
  return { size, lineHeight, lines: out, overflow, truncated };
}
