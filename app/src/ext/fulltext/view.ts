// 「內文」一節每一行的內容：哪種、第幾頁、前後文切成標與不標的幾段、點了打開哪裡。

import type { HitTarget } from "../types";
import type { Hit } from "./api";
import type { FulltextKey } from "./strings";

/** 文字表的取法（`strings.ts` 的 `s`；由呼叫的一方交進來，這個檔才能直接給 Node 測試）。 */
type Say = (key: FulltextKey, params?: Record<string, string | number>) => string;

/** 前後文切成幾段：`mark` 的那幾段是命中的字（UTF-16 位置）；截掉的一頭加「…」。 */
export function pieces(h: Pick<Hit, "context" | "marks" | "cut_start" | "cut_end">): { text: string; mark: boolean }[] {
  const out: { text: string; mark: boolean }[] = [];
  const c = h.context;
  let at = 0;
  for (const [a, b] of [...h.marks].sort((x, y) => x[0] - y[0])) {
    const from = Math.max(a, at);
    if (b <= from || from >= c.length) continue;
    if (from > at) out.push({ text: c.slice(at, from), mark: false });
    out.push({ text: c.slice(from, Math.min(b, c.length)), mark: true });
    at = Math.min(b, c.length);
  }
  if (at < c.length) out.push({ text: c.slice(at), mark: false });
  // 換行與連續空白在一行裡看起來是一個空白
  const flat = out.map((p) => ({ ...p, text: p.text.replace(/\s+/g, " ") }));
  if (h.cut_start) flat.unshift({ text: "…", mark: false });
  if (h.cut_end) flat.push({ text: "…", mark: false });
  return flat;
}

/** 這一處的種類與頁：「原文 第 5 頁」「譯文 第 5 頁」「重點筆記 第 3 頁」「筆記」。 */
export function hitLabel(h: Pick<Hit, "kind" | "page">, s: Say): string {
  const kind = s(`kind.${h.kind}`);
  return h.page === null ? kind : `${kind} ${s("page", { n: h.page + 1 })}`;
}

/** 點了打開哪裡。 */
export function hitTarget(paperId: string, h: Hit): HitTarget {
  const page = h.page ?? undefined;
  const material = h.material ?? undefined;
  const t: HitTarget = { paperId };
  if (material !== undefined) t.material = material;
  if (page !== undefined) t.page = page;
  switch (h.kind) {
    case "source":
      if (h.range && h.quote !== null) {
        t.range = h.range;
        t.quote = h.quote;
      }
      return t;
    case "translation":
      t.view = "overlay";
      if (h.box) t.box = h.box;
      return t;
    case "digest":
      t.view = "digest";
      if (h.target) t.point = h.target;
      // 重點筆記的頁只是那一點的出處：打開時不去那一頁
      delete t.page;
      return t;
    case "note":
      // 劃線有頁；自由書寫沒有頁，打開筆記窗格
      if (page !== undefined && h.target) t.highlight = h.target;
      else {
        t.view = "notes";
        delete t.material;
      }
      return t;
  }
}
