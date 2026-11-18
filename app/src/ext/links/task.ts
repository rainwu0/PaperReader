// 逐篇處理的一件抽取 `links.references`：從版面分析交回的參考文獻區切出條目，經核心的
// `references_write` 寫成那篇的 `references.json`。找不到參考文獻區時照樣寫一份空的，別台也就知道做過。

import { api } from "../../api";
import type { PerPaperTask } from "../types";
import { linksApi } from "./api";
import { splitEntries } from "./entries";

/** 代號（應用空間紀錄裡的鍵）與抽取的代號（`references.json` 的 `extraction.extractor`）。 */
export const TASK_ID = "links.references";
export const EXTRACTOR = "links";
/** 切條目的規則改了就加一，做過的篇會重做。 */
export const VERSION = 1;

export const referencesTask: PerPaperTask = {
  id: TASK_ID,
  version: VERSION,
  needsLayout: true,
  /** 做過了：`references.json` 是從現在的原文、不舊於這一版的規則抽的。索引整理中先不判斷。 */
  async doneFor(papers) {
    const st = await linksApi.extractState(papers.map((p) => p.paperId));
    if (st.building) return null;
    const done = new Set<string>();
    for (const p of papers) {
      const s = st.papers[p.paperId];
      if (s && s.source_sha256 === p.sourceSha256 && s.version >= VERSION) done.add(p.paperId);
    }
    return done;
  },
  async run(input, signal) {
    const entries = splitEntries(input.layout?.references ?? []).filter((e) => e.page < input.source.pages);
    if (signal.aborted) throw new Error("stopped");
    await api.referencesWrite(input.paperId, {
      extractor: EXTRACTOR,
      version: VERSION,
      source_material: input.source.materialId,
      source_sha256: input.source.sha256,
      entries,
    });
  },
};
