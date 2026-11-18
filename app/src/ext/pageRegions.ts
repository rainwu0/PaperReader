// 頁面上的可點區域：閱讀畫面向登記處裡的每個外掛要一頁的區域。應用只經這裡與登記處接觸外掛。

import { plugins } from "./index";
import type { PageRegion, PageRegionInput } from "./types";

/** 有沒有外掛交出可點區域（沒有就不必讀每頁的文字）。 */
export function hasPageRegions(): boolean {
  return plugins.some((p) => p.pageRegions);
}

/** 一頁的可點區域（每個外掛的合起來）；外掛出錯時當成沒有。 */
export async function pageRegionsFor(input: PageRegionInput, signal: AbortSignal): Promise<PageRegion[]> {
  const all = await Promise.all(
    plugins.map((p) => p.pageRegions?.regionsFor(input, signal).catch(() => [] as PageRegion[]) ?? Promise.resolve([])),
  );
  return all.flat();
}

/** 第一次出卡片時的首次提示（外掛自己的文字；沒有就用應用的那一句）。 */
export function pageRegionHint(): string | undefined {
  return plugins.find((p) => p.pageRegions?.hint)?.pageRegions?.hint;
}
