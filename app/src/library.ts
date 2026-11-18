// 文獻庫與目錄在畫面這一側的狀態：向核心要結果，存起來給畫面用。不自己推算任何資料規則。

import { useCallback, useRef, useState } from "react";
import { api, AppError, type Group, type LibraryStatus, type PaperSummary } from "./api";

export interface Library {
  /** `null`：還在讀（啟動中），或第一次讀就失敗了（看 `error`）。 */
  status: LibraryStatus | null;
  papers: PaperSummary[];
  /** 群組（依核心給的順序）。讀不到時是空的，首頁照常可用。 */
  groups: Group[];
  /** 「繼續閱讀」那一篇的代號。 */
  continueId: string | null;
  /** 回收區裡有幾篇。 */
  trashCount: number;
  /** 第一次讀取就失敗：首頁要說明出了什麼事，不能停在轉圈。 */
  error: AppError | null;
  /** 已經載入過之後重新讀取失敗：畫面上的目錄可能不是最新的。 */
  stale: boolean;
  /** 重新向核心問文獻庫的狀況與目錄。 */
  refresh: () => Promise<void>;
  /** 回到前景時：先重新掃描磁碟（別的工具可能改過檔案），再重新整理。 */
  foreground: () => Promise<void>;
}

/**
 * 建立一個群組並把幾篇放進去（文獻資訊與首頁的多選共用）。群組建不成丟出錯誤；
 * 建好了但有幾篇放不進去時交回 `memberFailed`（群組照樣留著）。
 */
export async function createGroupWith(name: string, paperIds: string[]): Promise<{ group: Group; memberFailed: boolean }> {
  const group = await api.createGroup(name);
  let memberFailed = false;
  for (const id of paperIds) {
    try {
      await api.setGroupMember(id, group.id, true);
    } catch {
      memberFailed = true;
    }
  }
  return { group, memberFailed };
}

function asAppError(e: unknown): AppError {
  return e instanceof AppError ? e : new AppError("Unknown", "");
}

export function useLibrary(): Library {
  const [status, setStatus] = useState<LibraryStatus | null>(null);
  const [papers, setPapers] = useState<PaperSummary[]>([]);
  const [groups, setGroups] = useState<Group[]>([]);
  const [continueId, setContinueId] = useState<string | null>(null);
  const [trashCount, setTrashCount] = useState(0);
  const [error, setError] = useState<AppError | null>(null);
  const [stale, setStale] = useState(false);
  const seq = useRef(0);
  const loaded = useRef(false);

  const refresh = useCallback(async () => {
    const mine = ++seq.current;
    try {
      const st = await api.libraryStatus();
      let list: PaperSummary[] = [];
      let grp: Group[] = [];
      let cont: string | null = null;
      let trash = 0;
      if (st.state === "ready" || st.state === "read_only") {
        const trashed = api.listTrash().then(
          (l) => l.length,
          () => 0,
        );
        // 群組讀不到（例如群組檔讀不懂）不影響首頁：當作沒有群組
        const groupsRead = api.listGroups().then(
          (g) => g,
          () => [] as Group[],
        );
        [list, cont, trash, grp] = await Promise.all([
          api.listPapers(),
          api.continueReading(),
          trashed,
          groupsRead,
        ]);
      }
      if (mine !== seq.current) return; // 有更新的一次在跑，這次的結果作廢
      loaded.current = true;
      setStatus(st);
      setPapers(list);
      setGroups(grp);
      setContinueId(cont);
      setTrashCount(trash);
      setError(null);
      setStale(false);
    } catch (e) {
      if (mine !== seq.current) return;
      if (loaded.current) setStale(true);
      else setError(asAppError(e));
    }
  }, []);

  const foreground = useCallback(async () => {
    let rescanFailed = false;
    try {
      await api.rescan();
    } catch (e) {
      // 還沒有文獻庫時掃描本來就會失敗（NoLibrary），不算；其他的失敗代表目錄可能不是最新的
      if (asAppError(e).kind !== "NoLibrary") rescanFailed = true;
    }
    await refresh();
    if (rescanFailed) setStale(true);
  }, [refresh]);

  return { status, papers, groups, continueId, trashCount, error, stale, refresh, foreground };
}
