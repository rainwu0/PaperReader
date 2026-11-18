// 整批匯入的預覽與結果裡，一篇略過的原因、一則提醒怎麼寫成一句話。
// 核心之後可能新增代碼：不認得的用通用的一句。相關文獻的代號換成篇名。

import type { ImportNotice, ImportSkip, PaperSummary } from "../../api";
import { hasText, t, type StringKey } from "../../strings";

/** 目錄裡那一篇的篇名；找不到（例如在回收區）是「沒有篇名」。 */
function titleOf(id: string | null, papers: ReadonlyArray<PaperSummary>): string {
  const p = id ? papers.find((x) => x.id === id) : undefined;
  return p?.bib.title || t("common.untitled");
}

/** 略過的原因：`import.reason.<代碼>`，細節是欄位位置或路徑；「已經有了」換成那一篇的篇名。 */
export function reasonText(s: ImportSkip, papers: ReadonlyArray<PaperSummary>): string {
  const key = `import.reason.${s.reason}`;
  // 不認得的代碼：通用的一句「沒有加入」（和加入出錯的標題同一句）
  if (!hasText(key)) return t("add.error.title");
  const detail = s.reason === "already-present" ? titleOf(s.detail, papers) : (s.detail ?? "");
  return t(key as StringKey, { detail });
}

/** 一篇略過的一行：「第 3 篇：找不到檔案：L003/a.pdf」（篇數從 1 起算，和清單的順序一致）。 */
export function skipLine(s: ImportSkip, papers: ReadonlyArray<PaperSummary>): string {
  return t("import.preview.line", { n: s.index + 1, text: reasonText(s, papers) });
}

/** 一則提醒；不認得的種類是 `null`（不顯示）。 */
export function noticeText(n: ImportNotice, papers: ReadonlyArray<PaperSummary>): string | null {
  const key = `import.notice.${n.kind}`;
  if (!hasText(key)) return null;
  const detail = n.kind === "same-doi" ? titleOf(n.detail, papers) : (n.detail ?? "");
  return t(key as StringKey, { detail });
}
