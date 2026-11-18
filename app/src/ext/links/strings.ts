// 外掛「文獻之間的連結」的文字。
// `{name}` 的代換用應用的同一個函式（`src/strings` 的 `fill`）。

import { fill } from "../../strings";

const zhTW = {
  "refs.heading": "參考文獻（{n} 條，庫裡有 {k} 篇）",
  "refs.building": "整理中",
  "refs.maybe": "可能是",
  "refs.isThis": "是這篇",
  "refs.more": "更多",
  "refs.menu.title": "第 {n} 條",
  "refs.menu.titleNoNumber": "這一條",
  "refs.notThis": "不是這篇",
  "refs.pickOther": "指定別篇",
  "refs.pick": "指定",
  "refs.showSource": "看原文",
  "refs.undo": "撤銷否決：{title}",
  "refs.lost.heading": "找不回的判斷（{n}）",
  "refs.lost.hint": "重新抽取之後對不上原來那一條的判斷。",
  "refs.lost.clear": "清除",
  "refs.lost.clearAll": "全部清除",
  "refs.full": "判斷已到上限，請先清除找不回的判斷。",
  "refs.failed": "沒有改成，請再試一次。",
  "refs.pick.title": "指定一篇",
  "refs.pick.search": "搜尋篇名、作者、年份",
  "refs.pick.none": "沒有符合的文獻",
  "refs.untitled": "（沒有篇名）",
  "citedBy.heading": "庫裡引用這篇的（{m} 篇）",
  "citedBy.numbers": "第 {numbers} 條",
  "card.maybe": "可能是：{titles}",
  "card.maybeThis": "可能是這一條",
  "card.hint": "點引用的編號看那一條參考文獻，庫裡有的可以直接開啟。不需要時按工具箱的「點內容」關掉。",
} as const;

export type LinksKey = keyof typeof zhTW;

export function s(key: LinksKey, params?: Record<string, string | number>): string {
  return fill(zhTW[key], params);
}
