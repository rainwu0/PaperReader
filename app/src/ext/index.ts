// 外掛在畫面一側唯一的登記處。只有這個檔引用外掛；拿掉一個外掛就是拿掉這裡的一行
// （`scripts/ext-boundary.mjs` 確認別的檔沒有引用外掛）。逐篇處理、文獻資訊、閱讀畫面、首頁搜尋都從 `plugins` 取外掛
// 交出的東西。

import { fulltext } from "./fulltext";
import { links } from "./links";
import type { ExtPlugin } from "./types";

export const plugins: ExtPlugin[] = [links, fulltext];
