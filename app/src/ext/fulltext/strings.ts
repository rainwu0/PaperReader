// 外掛「內文搜尋」的文字。
// `{name}` 的代換用應用的同一個函式（`src/strings` 的 `fill`）。

import { fill } from "../../strings";

const zhTW = {
  "body.heading": "內文",
  "body.building": "整理中",
  "body.none": "內文沒有找到",
  "body.unextracted": "還有 {n} 篇沒整理好，之後才找得到",
  "body.failed": "內文搜尋失敗",
  "body.loading": "找內文中",
  "body.more": "還有 {k} 處",
  "kind.source": "原文",
  "kind.translation": "譯文",
  "kind.digest": "重點筆記",
  "kind.note": "筆記",
  "page": "第 {n} 頁",
} as const;

export type FulltextKey = keyof typeof zhTW;

export function s(key: FulltextKey, params?: Record<string, string | number>): string {
  return fill(zhTW[key], params);
}
