// 外掛自己的指令（外殼的 `ext.rs`）：呼叫用應用的 `call`，交回的型別和 Rust 一側同形。

import { call } from "../../api";
import type { Box } from "../types";

export type HitKind = "source" | "translation" | "digest" | "note";

/** 一處。`marks` 是 `context` 裡的 UTF-16 位置；`range`、`quote` 只有原文才有。 */
export interface Hit {
  kind: HitKind;
  material: string | null;
  /** 頁序，從 0 起；筆記檔的自由書寫沒有。 */
  page: number | null;
  /** 段落、點、劃線的代號；自由書寫是 `t<段落序>`。 */
  target: string | null;
  box: Box | null;
  context: string;
  cut_start: boolean;
  cut_end: boolean;
  marks: [number, number][];
  range: [number, number] | null;
  quote: string | null;
}

export interface PaperHits {
  paper: string;
  total: number;
  hits: Hit[];
}

export interface SearchResult {
  /** 一次 50 篇以上在背景整理。 */
  building: boolean;
  /** 背景在比對或更新。 */
  pending: boolean;
  indexed: number;
  more: boolean;
  papers: PaperHits[];
}

export const fulltextApi = {
  search: (query: string, scope: string[] | null, offset: number) =>
    call<SearchResult>("fulltext_search", { query, scope, offset }),
  hits: (paperId: string, query: string) => call<Hit[]>("fulltext_hits", { paperId, query }),
};
