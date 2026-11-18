// 「整篇的想法」存檔撞上別處的修改（`Conflict`）時，要告訴使用者哪一段沒存進去。
// 編輯器換成檔案的新內容；使用者剛才打的字另外留著，讓他複製或加回去。

/** 拼音文字的字母與數字（中日韓的字本身就是一個詞，不算）。 */
const WORDY = /[\p{Script=Latin}\p{Script=Greek}\p{Script=Cyrillic}\p{N}_'’-]/u;

/**
 * 使用者把 `base`（筆記檔裡原本的想法）改成 `mine`：交回改過的那一段。
 * 比對頭尾相同的部分，中間不同的就是改過的地方（改了好幾處時，交回從第一處到最後一處的整段）；
 * 切在英文單字中間時補到整個字。只刪了字、沒有新打的字時交回空字串（沒有東西要留）。
 */
export function unsavedPart(base: string, mine: string): string {
  if (base === mine) return "";
  let head = 0;
  while (head < base.length && head < mine.length && base[head] === mine[head]) head++;
  let tail = 0;
  while (
    tail < base.length - head &&
    tail < mine.length - head &&
    base[base.length - 1 - tail] === mine[mine.length - 1 - tail]
  )
    tail++;
  let end = mine.length - tail;
  if (end <= head) return "";
  const wordy = (i: number) => i >= 0 && i < mine.length && WORDY.test(mine[i] ?? "");
  while (head > 0 && wordy(head - 1) && wordy(head)) head--;
  while (end < mine.length && wordy(end - 1) && wordy(end)) end++;
  return mine.slice(head, end).trim();
}

/** 把留下的那段加到想法的最後（中間空一行）。 */
export function appendPart(text: string, part: string): string {
  const body = text.replace(/\s+$/, "");
  return body === "" ? part : `${body}\n\n${part}`;
}
