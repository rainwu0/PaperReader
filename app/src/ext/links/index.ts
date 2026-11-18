// 外掛「文獻之間的連結」在畫面一側登記的東西。第一期完全離線：不連網。

import type { ExtPlugin } from "../types";
import { CitedBySection } from "./CitedBySection";
import { ReferencesSection } from "./ReferencesSection";
import { pageRegions } from "./regions";
import { referencesTask } from "./task";

export const links: ExtPlugin = {
  name: "links",
  network: [],
  infoSections: [
    { id: "links.references", order: 10, Component: ReferencesSection },
    { id: "links.citedBy", order: 20, Component: CitedBySection },
  ],
  pageRegions,
  perPaper: [referencesTask],
};
