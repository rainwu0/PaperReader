// 外掛「內文搜尋」在畫面一側登記的東西。完全離線：不連網。抽文字是內建的逐篇處理
// （`text.pages`），這裡只有首頁搜尋的一節。

import type { ExtPlugin } from "../types";
import { BodySection } from "./BodySection";

export const fulltext: ExtPlugin = {
  name: "fulltext",
  network: [],
  searchSections: [{ id: "fulltext.body", order: 10, Component: BodySection }],
};
