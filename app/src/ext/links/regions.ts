// 頁面上的可點區域：原文一頁上的引用標記與它們的卡片。卡片只是資料，閱讀畫面用自己的小框顯示。

import type { PageRegion, PageRegionProvider, RegionCard } from "../types";
import { linksApi, type EntryView, type LinksView } from "./api";
import { findMarkers } from "./markers";
import { s } from "./strings";

/** 一篇的參考文獻與連結：同一篇的幾頁共用一次查詢，過一陣子再查（文獻資訊裡改了判斷之後）。 */
const KEEP_MS = 15_000;
const views = new Map<string, { at: number; view: Promise<LinksView | null> }>();

export function linksOf(paperId: string): Promise<LinksView | null> {
  const c = views.get(paperId);
  if (c && Date.now() - c.at < KEEP_MS) return c.view;
  const view = linksApi.linksFor(paperId).catch(() => null);
  views.set(paperId, { at: Date.now(), view });
  return view;
}

/** 判斷改了：下次重查。 */
export function forgetLinks(paperId: string): void {
  views.delete(paperId);
}

const titleOf = (t: string) => t.trim() || s("refs.untitled");

function itemOf(e: EntryView, note?: string): RegionCard["items"][number] {
  if (e.link) return { text: e.text, open: e.link.paper, ...(note ? { note } : {}) };
  if (e.pending.length > 0) return { text: e.text, note: s("card.maybe", { titles: e.pending.map((p) => titleOf(p.title)).join("、") }) };
  return note ? { text: e.text, note } : { text: e.text };
}

export const pageRegions: PageRegionProvider = {
  hint: s("card.hint"),
  async regionsFor(input, signal) {
    const view = await linksOf(input.paperId);
    if (!view || signal.aborted || view.entries.length === 0) return [];
    const { numbered, authorYear } = findMarkers(input.items, input.page, view.entries);
    const out: PageRegion[] = [];
    for (const m of numbered) {
      const items = m.numbers.flatMap((n) => {
        const e = view.entries.find((x) => x.number === n);
        return e ? [itemOf(e)] : [];
      });
      if (items.length > 0) out.push({ bbox: m.bbox, card: { title: m.text, items } });
    }
    for (const m of authorYear) {
      const items = m.candidates.flatMap((i) => {
        const e = view.entries[i];
        return e ? [itemOf(e, s("card.maybeThis"))] : [];
      });
      if (items.length > 0) out.push({ bbox: m.bbox, card: { title: m.text, items } });
    }
    return out;
  },
};
