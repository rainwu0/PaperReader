// 外掛自己的指令（外殼的 `ext.rs`）：呼叫用應用的 `call`，交回的型別和 Rust 一側同形。

import { call } from "../../api";

export type LinkBy = "doi" | "arxiv" | "title" | "manual" | "confirm";

export interface LinkTo {
  paper: string;
  title: string;
  by: LinkBy;
}

export interface EntryView {
  /** 在參考文獻清單裡的位置（加判斷時帶）。 */
  index: number;
  number: number | null;
  label: string | null;
  text: string;
  /** 條目起頭的頁序，從 0 起。 */
  page: number;
  bbox: [number, number, number, number] | null;
  link: LinkTo | null;
  /** 待確認的候選。 */
  pending: { paper: string; title: string }[];
  /** 被否決的（`judgment` 是那一項判斷的代號，撤銷時刪它）。 */
  rejected: { paper: string; title: string; judgment: string }[];
}

export interface LinksView {
  /** `references.json` 的版本記號，加刪判斷時帶。 */
  token: string;
  /** 索引在背景整理中。 */
  building: boolean;
  entries: EntryView[];
  /** 找不回的判斷。 */
  lost: { judgment: string; kind: string; text: string; paper: string }[];
}

export interface CitingPaper {
  paper: string;
  title: string;
  year: number | null;
  numbers: (number | null)[];
}

export interface ExtractStates {
  building: boolean;
  papers: Record<string, { source_sha256: string; version: number } | null>;
}

export const linksApi = {
  /** 那篇的每一條與連結；沒抽過是 `null`。 */
  linksFor: (paperId: string) => call<LinksView | null>("links_for", { paperId }),
  /** 庫裡引用那篇的。 */
  citedBy: (paperId: string) => call<CitingPaper[]>("links_cited_by", { paperId }),
  /** 逐篇處理判斷做過沒有（一次一批）。 */
  extractState: (paperIds: string[]) => call<ExtractStates>("links_extract_state", { paperIds }),
};
