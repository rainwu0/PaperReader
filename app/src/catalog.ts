// 首頁清單的整理：目前的檢視、搜尋、閱讀狀態的篩選、排序。都是純函式，只決定「列哪些、依什麼順序」，
// 不改資料，也不推算資料規則（閱讀狀態、閱讀位置由核心算好）。
//
// 這個檔案只用 `import type`（沒有執行期的相依），所以 `scripts/catalog-test.mjs` 可以直接拿來測。

import type { Author, Bib, PaperSummary, Prefs, ReadStatus, SortKey } from "./api/types";

/** 首頁目前看的是哪一批文獻：全部文獻、未分組、某一個群組。 */
export type View = { kind: "all" } | { kind: "ungrouped" } | { kind: "group"; id: string };

export const ALL_VIEW: View = { kind: "all" };
export const UNGROUPED_VIEW: View = { kind: "ungrouped" };

/** 目前存在的群組代號。文獻上記的群組代號可能已不存在（群組被刪、檔案被別處改），這種代號一律略過。 */
export type GroupIds = ReadonlySet<string>;

/** 一篇文獻所屬的群組，只算存在的。沒給 `ids` 時全部照算。 */
export function liveGroups(paper: PaperSummary, ids?: GroupIds): string[] {
  return ids ? paper.groups.filter((g) => ids.has(g)) : paper.groups;
}

/** 一篇文獻在不在這個檢視裡。未分組＝存在的群組一個都沒有。 */
export function inView(paper: PaperSummary, view: View, ids?: GroupIds): boolean {
  switch (view.kind) {
    case "all":
      return true;
    case "ungrouped":
      return liveGroups(paper, ids).length === 0;
    case "group":
      return liveGroups(paper, ids).includes(view.id);
  }
}

export function sameView(a: View, b: View): boolean {
  if (a.kind === "group" && b.kind === "group") return a.id === b.id;
  return a.kind === b.kind;
}

/** 清單裡當作 key 用的字串。 */
export function viewKey(view: View): string {
  return view.kind === "group" ? `group:${view.id}` : view.kind;
}

// ---------- 搜尋 ----------

function authorNames(a: Author): string[] {
  return [a.family, a.given, a.literal].map((x) => (x ?? "").trim()).filter((x) => x !== "");
}

/** 一篇文獻可以被搜尋到的文字：篇名（沒有就用資料夾名）、全部作者、年份。 */
export function searchText(p: PaperSummary): string {
  const title = p.bib.title?.trim() || p.folder;
  const authors = p.bib.authors.flatMap(authorNames);
  const year = p.bib.year != null ? String(p.bib.year) : "";
  return [title, ...authors, year].join("\n").toLowerCase();
}

/** 搜尋字串切成幾個詞（空白分隔，不分大小寫）；每個詞都要在篇名、作者或年份裡找得到。 */
export function matchesQuery(p: PaperSummary, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const text = searchText(p);
  return words.every((w) => text.includes(w));
}

// ---------- 排序 ----------

/**
 * 第一作者的名字（排序用，小寫）。沒有就是 `null`。
 * 分開寫了姓的用姓；只有一整串（`literal`）的：有逗號取逗號前（「Zhao, Yian」），沒有就取最後一個詞當姓
 * （「Ashish Vaswani」；結尾的「et al.」先去掉），這樣依第一作者排序時才是依姓排。畫面上顯示的名字不受影響。
 */
export function firstAuthor(bib: Bib): string | null {
  const a = bib.authors[0];
  if (!a) return null;
  const family = (a.family ?? "").trim();
  if (family !== "") return family.toLowerCase();
  const literal = (a.literal ?? "").trim().replace(/\s+et al\.?$/i, "");
  if (literal !== "") {
    const sur = literal.includes(",") ? (literal.split(",")[0] ?? "") : (literal.split(/\s+/).pop() ?? "");
    return sur.trim().toLowerCase() || null;
  }
  const given = (a.given ?? "").trim();
  return given === "" ? null : given.toLowerCase();
}

function cmpText(a: string, b: string): number {
  return a.localeCompare(b, "zh-Hant");
}

type Keyed = { missing: true } | { missing: false; value: number | string };

function keyOf(p: PaperSummary, key: SortKey): Keyed {
  switch (key) {
    case "recent": {
      const t = p.progress.last_read_at;
      return t ? { missing: false, value: t } : { missing: true };
    }
    case "added":
      // 舊的文獻沒有加入時間：排最後
      return p.added ? { missing: false, value: p.added } : { missing: true };
    case "title":
      return { missing: false, value: (p.bib.title?.trim() || p.folder).toLowerCase() };
    case "author": {
      const a = firstAuthor(p.bib);
      return a === null ? { missing: true } : { missing: false, value: a };
    }
    case "year":
      return p.bib.year != null ? { missing: false, value: p.bib.year } : { missing: true };
  }
}

/** 每種排序的預設方向：最近、加入時間、年份是新的在前；篇名、第一作者是 A 到 Z。 */
const NEWEST_FIRST: Record<SortKey, boolean> = {
  recent: true,
  added: true,
  title: false,
  author: false,
  year: true,
};

function cmpValue(a: number | string, b: number | string): number {
  if (typeof a === "number" && typeof b === "number") return a - b;
  return cmpText(String(a), String(b));
}

/**
 * 排序。`reverse` 把預設方向反過來。沒有值的（沒讀過、沒有作者、沒有年份、沒有加入時間）不論正反都排最後，
 * 它們之間依加入時間由新到舊（沒有加入時間的用代號，代號依時間排序）。
 */
export function sortPapers(papers: PaperSummary[], key: SortKey, reverse: boolean): PaperSummary[] {
  const desc = NEWEST_FIRST[key] !== reverse;
  const byAdded = (a: PaperSummary, b: PaperSummary) => {
    const ka = a.added ?? a.id;
    const kb = b.added ?? b.id;
    return ka < kb ? 1 : ka > kb ? -1 : 0;
  };
  return [...papers].sort((a, b) => {
    const ka = keyOf(a, key);
    const kb = keyOf(b, key);
    if (ka.missing || kb.missing) {
      if (ka.missing && kb.missing) return byAdded(a, b);
      return ka.missing ? 1 : -1;
    }
    const c = cmpValue(ka.value, kb.value);
    if (c !== 0) return desc ? -c : c;
    return byAdded(a, b);
  });
}

// ---------- 內容的篩選 ----------

/** 依內容篩選：有譯本、有重點筆記、有筆記（劃線或想法）。可以同時選幾項，要每一項都有。 */
export type HasFilter = "translation" | "digest" | "notes";

export const HAS_FILTERS: readonly HasFilter[] = ["translation", "digest", "notes"];

// 材料的角色與形式（同 `api/types.ts` 的 `ROLE_TRANSLATION`、`ROLE_DIGEST`、`FORM_MARKDOWN`；這個檔案不引入執行期的東西）
// 譯本只算閱讀畫面打得開的形式（PDF、譯文疊加檔）；匯入的 Markdown 譯本不算（同 `paperMeta.ts`）
const isTranslation = (m: { role: string; form: string }) =>
  m.role === "translation" && (m.form === "pdf" || m.form === "overlay");
const isDigest = (m: { role: string; form: string }) => m.role === "digest" && m.form === "markdown";

/** 這一篇有沒有這項內容。「有筆記」看目錄摘要的 `has_notes`（核心讀筆記檔算好，畫面不自己讀筆記檔）。 */
export function hasContent(p: PaperSummary, what: HasFilter): boolean {
  switch (what) {
    case "translation":
      return p.materials.some(isTranslation);
    case "digest":
      return p.materials.some(isDigest);
    case "notes":
      return p.has_notes;
  }
}

// ---------- 合在一起 ----------

export interface CatalogQuery {
  view: View;
  query: string;
  /** 閱讀狀態篩選；`null` 是不篩。 */
  status: ReadStatus | null;
  /** 內容的篩選（每一項都要有）；空的或沒給是不篩。 */
  has?: readonly HasFilter[];
  sortKey: Prefs["sort_key"];
  sortReverse: boolean;
  /** 存在的群組代號（沒給就當文獻上記的群組都存在）。 */
  groupIds?: GroupIds;
}

/** 最近加入的幾篇（由新到舊，沒有加入時間的排最後）。 */
export function recentlyAdded(papers: PaperSummary[], n: number): PaperSummary[] {
  return sortPapers(papers, "added", false).slice(0, n);
}

/** 目前的檢視 → 搜尋 → 篩選 → 排序。 */
export function applyCatalog(papers: PaperSummary[], q: CatalogQuery): PaperSummary[] {
  const has = q.has ?? [];
  const shown = papers.filter(
    (p) =>
      inView(p, q.view, q.groupIds) &&
      matchesQuery(p, q.query) &&
      (q.status === null || p.progress.status === q.status) &&
      has.every((h) => hasContent(p, h)),
  );
  return sortPapers(shown, q.sortKey, q.sortReverse);
}

/**
 * 首頁搜尋的一節的範圍：目前的群組與篩選下看得到的文獻代號，**不套搜尋字串本身**（書目的清單已經
 * 套了，內文要找的正是篇名裡沒有的字）。看全部文獻又沒有篩選時是 `null`（不限）。
 */
export function searchScope(
  papers: PaperSummary[],
  q: Pick<CatalogQuery, "view" | "status" | "has" | "groupIds">,
): string[] | null {
  const has = q.has ?? [];
  if (q.view.kind === "all" && q.status === null && has.length === 0) return null;
  return papers
    .filter(
      (p) =>
        inView(p, q.view, q.groupIds) &&
        (q.status === null || p.progress.status === q.status) &&
        has.every((h) => hasContent(p, h)),
    )
    .map((p) => p.id);
}

/** 有搜尋或篩選：清單是篩過的（沒有符合時說「沒有符合的文獻」並給清除；「最近加入」不出現）。 */
export function isNarrowed(q: Pick<CatalogQuery, "query" | "status" | "has">): boolean {
  return q.query.trim() !== "" || q.status !== null || (q.has?.length ?? 0) > 0;
}
