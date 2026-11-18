// 一篇文獻的筆記（劃線、短評、整篇的想法）在畫面這一側的狀態。
// 資料與格式都在核心；這裡只保管「讀到的筆記」與筆記檔的版本記號，寫入時原樣交回。
// 檔案在別處被改過（`Conflict`）就重新讀、告訴使用者剛才的修改沒有存進去。
// 「整篇的想法」沒存進去的那段留在 `stash`，由筆記窗格顯示，讓使用者複製或加回去。

import { useCallback, useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { api, AppError, type AddedHighlight, type HighlightInput, type NotesView } from "../../api";
import { errorMessage, t } from "../../strings";

export interface NotesState {
  view: NotesView | null;
  /** 第一次讀就失敗時的白話原因。 */
  loadError: string | null;
  reload: () => Promise<void>;
  /** 新增一條劃線；失敗時交回要告訴使用者的話（`null` 是成功）。 */
  addHighlight: (draft: HighlightInput) => Promise<{ added: AddedHighlight | null; message: string | null }>;
  setComment: (id: string, comment: string) => Promise<string | null>;
  remove: (id: string) => Promise<string | null>;
  /** 存「整篇的想法」。`conflict`：檔案在別處改過，沒有寫入，筆記已重新讀回（`latest()` 拿得到）。 */
  setThoughts: (text: string) => Promise<{ message: string | null; conflict: boolean }>;
  /** 最後讀到的筆記（剛重讀完、畫面還沒更新時也拿得到）。 */
  latest: () => NotesView | null;
  /** 存想法時撞上別處的修改、沒存進去的那段文字。放在這裡，關掉窗格再打開還在。 */
  stash: string | null;
  setStash: Dispatch<SetStateAction<string | null>>;
}

export function useNotes(paperId: string): NotesState {
  const [view, setView] = useState<NotesView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [stash, setStash] = useState<string | null>(null);
  const latest = useRef<NotesView | null>(null);

  const reload = useCallback(async () => {
    try {
      const v = await api.readNotes(paperId);
      latest.current = v;
      setView(v);
      setLoadError(null);
    } catch (e) {
      setLoadError(errorMessage(e instanceof AppError ? e.kind : "Unknown"));
    }
  }, [paperId]);

  useEffect(() => {
    latest.current = null;
    setView(null);
    setStash(null);
    void reload();
    // 換了一篇才重讀（reload 只依這一篇）
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [paperId]);

  /** 做一次寫入：成功就重新讀；`Conflict` 就重新讀並說明；其他失敗說原因。 */
  const guarded = useCallback(
    async <T>(
      run: (token: string | null) => Promise<T>,
    ): Promise<{ value: T | null; message: string | null; conflict: boolean }> => {
      try {
        const value = await run(latest.current?.token ?? null);
        await reload();
        return { value, message: null, conflict: false };
      } catch (e) {
        const kind = e instanceof AppError ? e.kind : "Unknown";
        if (kind === "Conflict") {
          await reload();
          return { value: null, message: t("reader.notes.conflict"), conflict: true };
        }
        return { value: null, message: errorMessage(kind), conflict: false };
      }
    },
    [reload],
  );

  const addHighlight = useCallback(
    async (draft: HighlightInput) => {
      const r = await guarded((token) => api.addHighlight(paperId, draft, token));
      return { added: r.value, message: r.message };
    },
    [guarded, paperId],
  );

  const setComment = useCallback(
    async (id: string, comment: string) => {
      const r = await guarded((token) =>
        token === null
          ? Promise.reject(new AppError("NotFound", ""))
          : api.setHighlightComment(paperId, id, comment, token),
      );
      return r.message;
    },
    [guarded, paperId],
  );

  const remove = useCallback(
    async (id: string) => {
      const r = await guarded((token) =>
        token === null ? Promise.reject(new AppError("NotFound", "")) : api.deleteHighlight(paperId, id, token),
      );
      return r.message;
    },
    [guarded, paperId],
  );

  const setThoughts = useCallback(
    async (text: string) => {
      const r = await guarded((token) => api.setThoughts(paperId, text, token));
      return { message: r.message, conflict: r.conflict };
    },
    [guarded, paperId],
  );

  const getLatest = useCallback(() => latest.current, []);

  return {
    view,
    loadError,
    reload,
    addHighlight,
    setComment,
    remove,
    setThoughts,
    latest: getLatest,
    stash,
    setStash,
  };
}
