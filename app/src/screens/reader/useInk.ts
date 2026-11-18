// 一篇文獻的筆跡（`ink.json`）在畫面這一側的狀態，與 `useNotes.ts` 對稱：資料與格式在核心，
// 這裡保管讀到的全部筆跡，畫的時候由閱讀畫面取目前這份材料的。
// 每畫完一筆就存；清除＝清掉一頁。存不進去就說一聲、重讀檔案。
// 復原：撤銷這次打開筆刷以來、畫面上看得到的頁上的最後一個動作（`inkHistory.ts`）：畫的刪掉、
// 擦的與清掉的畫回去（代號是新的；核心沒有「放回一筆」，畫回去是新加一筆）。

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Stroke } from "../../api";
import { lastOnPage, remapIds, type InkAction } from "./inkHistory";
import type { NewStroke } from "./pdfView";

export type InkStep = InkAction<Stroke>;

export interface InkState {
  strokes: Stroke[];
  add: (material: string, st: NewStroke) => Promise<void>;
  erase: (id: string) => Promise<void>;
  /** 撤銷畫面上看得到的這幾頁（頁序從 0 起）上最後一個動作；沒有可以撤銷的交回 `false`。 */
  undo: (material: string, pages: readonly number[]) => Promise<boolean>;
  /** 清掉一頁；交回這個動作（提示上的「復原」用 `revert` 撤銷它）。沒有筆跡或寫不進去交回 `null`。 */
  clearPage: (material: string, pageIndex: number) => Promise<InkStep | null>;
  /** 撤銷某一個動作（還在清單裡才做）。 */
  revert: (step: InkStep) => Promise<void>;
  /** 關掉筆刷：動作清單清空（下次打開重新算）。 */
  resetHistory: () => void;
}

/** `onFailed`：寫不進去（之後會重讀檔案，畫面回到檔案裡的樣子）。 */
export function useInk(paperId: string, onFailed: () => void): InkState {
  const [strokes, setStrokes] = useState<Stroke[]>([]);
  const strokesRef = useRef(strokes);
  useEffect(() => {
    strokesRef.current = strokes;
  });
  const history = useRef<InkStep[]>([]);
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
    history.current = [];
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
        history.current.push({ kind: "draw", material, page: saved.page, id: saved.id });
      } catch {
        failed();
      }
    },
    [paperId, failed],
  );

  const erase = useCallback(
    async (id: string) => {
      const stroke = strokesRef.current.find((s) => s.id === id);
      try {
        await api.deleteStroke(paperId, id);
        setStrokes((list) => list.filter((s) => s.id !== id));
        if (stroke) history.current.push({ kind: "erase", material: stroke.material, page: stroke.page, stroke });
      } catch {
        failed();
      }
    },
    [paperId, failed],
  );

  /** 把幾筆畫回去（新的代號），清單裡指著舊代號的改指新的。 */
  const redraw = async (list: Stroke[]) => {
    const ids = new Map<string, string>();
    const added: Stroke[] = [];
    for (const s of list) {
      const saved = await api.addStroke(paperId, {
        material: s.material,
        page_index: s.page,
        color: s.color,
        width: s.width,
        opacity: s.opacity,
        points: s.points,
      });
      ids.set(s.id, saved.id);
      added.push(saved);
    }
    setStrokes((cur) => [...cur, ...added]);
    history.current = remapIds(history.current, ids);
  };

  const apply = async (step: InkStep) => {
    try {
      if (step.kind === "draw") {
        await api.deleteStroke(paperId, step.id);
        setStrokes((list) => list.filter((s) => s.id !== step.id));
      } else if (step.kind === "erase") await redraw([step.stroke]);
      else await redraw(step.strokes);
    } catch {
      failed();
    }
  };

  const undo = async (material: string, pages: readonly number[]) => {
    const i = lastOnPage(history.current, material, pages);
    const step = history.current[i];
    if (i < 0 || !step) return false;
    history.current.splice(i, 1);
    await apply(step);
    return true;
  };

  const revert = async (step: InkStep) => {
    const i = history.current.indexOf(step);
    if (i < 0) return;
    history.current.splice(i, 1);
    await apply(step);
  };

  const clearPage = async (material: string, pageIndex: number): Promise<InkStep | null> => {
    const gone = strokesRef.current.filter((s) => s.material === material && s.page === pageIndex);
    if (gone.length === 0) return null;
    try {
      await api.clearInkPage(paperId, material, pageIndex);
      setStrokes((list) => list.filter((s) => !(s.material === material && s.page === pageIndex)));
      const step: InkStep = { kind: "clear", material, page: pageIndex, strokes: gone };
      history.current.push(step);
      return step;
    } catch {
      failed();
      return null;
    }
  };

  const resetHistory = useCallback(() => {
    history.current = [];
  }, []);

  return { strokes, add, erase, undo, clearPage, revert, resetHistory };
}
