// 首次提示：不明顯的手勢第一次用到時說一次，一次一則；之後可在說明頁重看。
// 看過哪些只是這台裝置上的介面偏好，記在網頁的本機儲存，不寫進文獻庫或設定檔。

export type HintId = "select" | "toolbox" | "overlay" | "paragraph" | "card";

const KEY = "paperreader.hints.seen";

function load(): Set<string> {
  try {
    const raw = window.localStorage.getItem(KEY);
    return new Set(raw ? (JSON.parse(raw) as string[]) : []);
  } catch {
    return new Set();
  }
}

/** 這則提示還沒看過。 */
export function hintPending(id: HintId): boolean {
  return !load().has(id);
}

/** 記下看過了，之後不再出現。 */
export function markHintSeen(id: HintId): void {
  const s = load();
  s.add(id);
  try {
    window.localStorage.setItem(KEY, JSON.stringify([...s]));
  } catch {
    // 存不進去只會讓提示再出現一次，不影響使用
  }
}

/** 說明頁的「重新顯示首次提示」。 */
export function resetHints(): void {
  try {
    window.localStorage.removeItem(KEY);
  } catch {
    // 同上
  }
}
