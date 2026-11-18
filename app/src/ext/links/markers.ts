// 引用標記：一頁的文字片段裡，哪些是編號式的引用標記（`[12]`、`[3, 5–7]`）、哪些像作者年份式
// （`(Zhao et al., 2024)`）。規則自己訂的。只用 `import type`，Node 的測試可以直接引入。
//
// 以一行為單位：同一條基線上的文字片段依 x 排好接成一行（被切在兩個片段的標記也認得），兩段之間隔得很開（欄的中縫）
// 就是兩行。每個字的位置依片段的寬度平均分配；標記的方框是它每個字的方框的聯集。

import type { RawItem } from "../../translate/layout";

export type Box = [number, number, number, number];

/** 這篇參考文獻清單的一條（只要認標記用得到的）。 */
export interface MarkerEntry {
  number: number | null;
  text: string;
  /** 條目起頭的頁序。 */
  page: number;
}

/** 編號式的引用標記。 */
export interface NumberMarker {
  bbox: Box;
  /** 標記本身，例如「[3, 5–7]」。 */
  text: string;
  /** 標記裡的每個編號，依出現的順序。 */
  numbers: number[];
}

/** 作者年份式的引用：只給「可能是」的候選（最多 3 條，在清單裡的位置）。 */
export interface AuthorYearMarker {
  bbox: Box;
  text: string;
  candidates: number[];
}

/** 一個範圍最多幾個編號。 */
const MAX_RANGE = 50;
/** `[` 前面是這些符號的不是引用，是數學（區間、集合）。 */
const MATH_BEFORE = new Set(["∈", "∉", "=", "≤", "≥", "<", ">", "⊂", "⊆", "×", "→", ":"]);
const MATH_AFTER = new Set(["^", "_"]);

interface Glyph {
  ch: string;
  box: Box | null;
}

/** 一頁的文字片段排成一行一行（每行是一串字，各帶方框；補上的空白沒有方框）。 */
export function textLines(items: readonly RawItem[]): Glyph[][] {
  const flat = items
    .filter((it) => it.str !== "" && Math.abs(it.transform[1] ?? 0) < 0.01 && Math.abs(it.transform[2] ?? 0) < 0.01)
    .map((it) => {
      const h = Math.abs(it.transform[3] ?? it.height) || it.height || 10;
      return { it, x: it.transform[4] ?? 0, y: it.transform[5] ?? 0, h };
    })
    .sort((a, b) => b.y - a.y || a.x - b.x);
  // 同一條基線
  const bands: (typeof flat)[] = [];
  for (const f of flat) {
    const band = bands.at(-1);
    const ref = band?.[0];
    if (band && ref && Math.abs(ref.y - f.y) <= 0.5 * Math.min(ref.h, f.h)) band.push(f);
    else bands.push([f]);
  }
  const out: Glyph[][] = [];
  for (const band of bands) {
    band.sort((a, b) => a.x - b.x);
    let line: Glyph[] = [];
    let end = -Infinity;
    for (const f of band) {
      const chars = Array.from(f.it.str);
      const w = f.it.width / Math.max(1, chars.length);
      if (line.length > 0 && f.x - end > 1.5 * f.h) {
        out.push(line);
        line = [];
      } else if (line.length > 0 && f.x - end > 0.12 * f.h && !/\s$/.test(line.at(-1)?.ch ?? "") && !/^\s/.test(f.it.str)) {
        line.push({ ch: " ", box: null });
      }
      chars.forEach((ch, i) => {
        const x0 = f.x + i * w;
        line.push({ ch, box: [x0, f.y - 0.2 * f.h, x0 + w, f.y + 0.8 * f.h] });
      });
      end = Math.max(end, f.x + f.it.width);
    }
    if (line.length > 0) out.push(line);
  }
  return out;
}

function unionBox(glyphs: readonly Glyph[]): Box | null {
  let b: Box | null = null;
  for (const g of glyphs) {
    if (!g.box) continue;
    b = b ? [Math.min(b[0], g.box[0]), Math.min(b[1], g.box[1]), Math.max(b[2], g.box[2]), Math.max(b[3], g.box[3])] : [...g.box];
  }
  return b;
}

/** 標記裡的編號清單：`3, 5–7`、`3;5-7`。不合就交回 `null`（小數點、負號、字母、太大的範圍）。 */
export function parseNumbers(inner: string): number[] | null {
  if (!/^[\d\s,;–—-]+$/.test(inner)) return null;
  const out: number[] = [];
  for (const raw of inner.split(/[,;]/)) {
    const part = raw.trim();
    const one = /^(\d{1,5})$/.exec(part);
    const range = /^(\d{1,5})\s*[–—-]\s*(\d{1,5})$/.exec(part);
    if (one) out.push(Number(one[1]));
    else if (range) {
      const a = Number(range[1]);
      const b = Number(range[2]);
      if (b <= a || b - a + 1 > MAX_RANGE) return null;
      for (let n = a; n <= b; n++) out.push(n);
    } else return null;
  }
  return out.length > 0 && out.every((n) => n >= 1 && n <= 99999) ? out : null;
}

const fold = (s: string) =>
  s
    .normalize("NFKD")
    .replace(/\p{M}/gu, "")
    .toLowerCase();

const NAME = String.raw`\p{Lu}[\p{L}'’-]+`;
const YEAR = String.raw`(?:1[6-9]|20)\d{2}[a-z]?`;
// 括號裡的一則：「Zhao et al., 2024」「Smith and Jones 2020」「see Lee, 2019」
const PAREN_ITEM = new RegExp(
  String.raw`^(?:(?:see|e\.g\.|cf\.|also)[,\s]+)?(${NAME})(?:\s+(?:et\s+al\.?|(?:and|&)\s+${NAME}))?,?\s+(${YEAR})$`,
  "u",
);
// 句中的：「Zhao et al. (2024)」
const NARRATIVE = new RegExp(String.raw`(${NAME})(?:\s+(?:et\s+al\.?|(?:and|&)\s+${NAME}))?\s+\((${YEAR})\)`, "gu");

/** 作者年份式的候選：條目原文以那個姓開頭、含那個年份的，最多 3 條。 */
function authorYearCandidates(entries: readonly MarkerEntry[], surname: string, year: string): number[] {
  const s = fold(surname);
  const y = year.slice(0, 4);
  const out: number[] = [];
  entries.forEach((e, i) => {
    if (out.length < 3 && fold(e.text).trimStart().startsWith(s) && e.text.includes(y)) out.push(i);
  });
  return out;
}

/**
 * 一頁上的引用標記。`page` 從 0 起；`entries` 是這篇參考文獻清單的每一條。
 * 編號式要三個條件都成立：每個編號都對得到某一條的 `number`；不是數學區間；不是條目自己的編號（不在一行的開頭，
 * 或這一頁不是任何一條的起頭頁）。
 */
export function findMarkers(
  items: readonly RawItem[],
  page: number,
  entries: readonly MarkerEntry[],
): { numbered: NumberMarker[]; authorYear: AuthorYearMarker[] } {
  const numbers = new Set(entries.flatMap((e) => (e.number === null ? [] : [e.number])));
  const entryPages = new Set(entries.map((e) => e.page));
  const numbered: NumberMarker[] = [];
  const authorYear: AuthorYearMarker[] = [];
  for (const glyphs of textLines(items)) {
    const text = glyphs.map((g) => g.ch).join("");
    // 字串的位置（UTF-16）→ 第幾個字：數學斜體這類字占兩個位置
    const at16: number[] = [];
    glyphs.forEach((g, i) => {
      for (let k = 0; k < g.ch.length; k++) at16.push(i);
    });
    const line = {
      slice: (a: number, b: number) => glyphs.slice(at16[a] ?? glyphs.length, (at16[b - 1] ?? glyphs.length - 1) + 1),
    };
    if (numbers.size > 0) {
      for (const m of text.matchAll(/\[([^[\]]{1,80})\]/g)) {
        const at = m.index ?? 0;
        const nums = parseNumbers(m[1] ?? "");
        if (!nums || !nums.every((n) => numbers.has(n))) continue;
        const before = text.slice(0, at).trimEnd();
        if (MATH_BEFORE.has(before.slice(-1))) continue;
        const after = text.slice(at + m[0].length).trimStart();
        if (MATH_AFTER.has(after.slice(0, 1))) continue;
        if (before === "" && entryPages.has(page)) continue;
        const bbox = unionBox(line.slice(at, at + m[0].length));
        if (bbox) numbered.push({ bbox, text: m[0], numbers: nums });
      }
    }
    if (entries.some((e) => e.number === null)) {
      for (const m of text.matchAll(/\(([^()]{3,200})\)/g)) {
        let from = (m.index ?? 0) + 1;
        for (const part of (m[1] ?? "").split(";")) {
          const p = PAREN_ITEM.exec(part.trim());
          const start = from + (part.length - part.trimStart().length);
          from += part.length + 1;
          if (!p) continue;
          const cands = authorYearCandidates(entries, p[1] ?? "", p[2] ?? "");
          const bbox = unionBox(line.slice(start, start + part.trim().length));
          if (cands.length > 0 && bbox) authorYear.push({ bbox, text: part.trim(), candidates: cands });
        }
      }
      for (const m of text.matchAll(NARRATIVE)) {
        const at = m.index ?? 0;
        const cands = authorYearCandidates(entries, m[1] ?? "", m[2] ?? "");
        const bbox = unionBox(line.slice(at, at + m[0].length));
        if (cands.length > 0 && bbox) authorYear.push({ bbox, text: m[0], candidates: cands });
      }
    }
  }
  return { numbered, authorYear };
}
