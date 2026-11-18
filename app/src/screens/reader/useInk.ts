// 一篇文獻的筆跡（`ink.json`）在畫面這一側的狀態，與 `useNotes.ts` 對稱：資料與格式在核心，
// 這裡保管讀到的全部筆跡，畫的時候由閱讀畫面取目前這份材料的。
// 每畫完一筆就存；復原＝刪掉這份材料最後一筆；清除＝清掉一頁。存不進去就說一聲、重讀檔案。

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Stroke } from "../../api";
import type { NewStroke } from "./pdfView";

export interface InkState {
  strokes: Stroke[];
  add: (material: string, st: NewStroke) => Promise<void>;
  erase: (id: string) => Promise<void>;
  undo: (material: string) => Promise<void>;
  clearPage: (material: string, pageIndex: number) => Promise<void>;
}

/** `onFailed`：寫不進去（之後會重讀檔案，畫面回到檔案裡的樣子）。 */
export function useInk(paperId: string, onFailed: () => void): InkState {
  const [strokes, setStrokes] = useState<Stroke[]>([]);
  const failedRef = useRef(onFailed);
  useEffect(() => {
    failedRef.current = onFailed;
  });

  const load = useCallback(async () => {
    try {
      setStrokes((await api.readInk(paperId)).strokes);
    } catch {
      // 讀不懂的 ink.json 由「檢查」列出；這裡就沒有筆跡可畫
      setStrokes([]);
    }
  }, [paperId]);
  useEffect(() => {
    void load();
  }, [load]);

  const failed = useCallback(() => {
    failedRef.current();
    void load();
  }, [load]);

  const add = useCallback(
    async (material: string, st: NewStroke) => {
      try {
        const saved = await api.addStroke(paperId, {
          material,
          page_index: st.pageIndex,
          color: st.color,
          width: st.width,
          opacity: st.opacity,
          points: st.points,
        });
        setStrokes((list) => [...list, saved]);
      } catch {
        failed();
      }
    },
    [paperId, failed],
  );

  const erase = useCallback(
    async (id: string) => {
      try {
        await api.deleteStroke(paperId, id);
        setStrokes((list) => list.filter((s) => s.id !== id));
      } catch {
        failed();
      }
    },
    [paperId, failed],
  );

  const undo = async (material: string) => {
    const last = [...strokes].reverse().find((s) => s.material === material);
    if (last) await erase(last.id);
  };

  const clearPage = async (material: string, pageIndex: number) => {
    try {
      await api.clearInkPage(paperId, material, pageIndex);
      setStrokes((list) => list.filter((s) => !(s.material === material && s.page === pageIndex)));
    } catch {
      failed();
    }
  };

  return { strokes, add, erase, undo, clearPage };
}
