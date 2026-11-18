// 筆記窗格開多大（與 `useNotes.ts` 的筆記內容對稱：這裡只管窗格）。
// 大小記在這台裝置上（下次打開還是這個大小）；半開或全開時是返回堆疊的一層：
// 返回鍵先把它縮成一條，再按一次才離開閱讀畫面。

import { useEffect, useRef, useState } from "react";
import type { Nav } from "../../nav";
import { readLocal, writeLocal } from "../../hooks";
import type { PaneSize } from "./NotesPane";

const PANE_KEY = "pr-notes-pane";

interface Saved {
  size: PaneSize;
  split: number;
}

const isSaved = (v: unknown): v is Saved => {
  const o = v as Partial<Saved> | null;
  return (
    !!o &&
    (o.size === "peek" || o.size === "half" || o.size === "full") &&
    typeof o.split === "number" &&
    o.split >= 0.2 &&
    o.split <= 0.85
  );
};

const readPane = (): Saved => readLocal(PANE_KEY, { size: "half", split: 0.5 }, isSaved);

export interface PaneState {
  open: boolean;
  size: PaneSize;
  split: number;
}

export function useNotesPane(nav: Nav) {
  const [pane, setPane] = useState<PaneState>(() => ({ open: false, ...readPane() }));
  const update = (next: Partial<PaneState>) =>
    setPane((p) => {
      const n = { ...p, ...next };
      // 只有改了大小才記（打開、關掉不算）
      if (next.size !== undefined || next.split !== undefined) writeLocal(PANE_KEY, { size: n.size, split: n.split });
      return n;
    });

  const paneUp = pane.open && pane.size !== "peek";
  const entry = useRef<number | null>(null); // 窗格推進去的那一層在堆疊的第幾層（還在的話）
  const isNotesAt = (i: number | null) => {
    const r = i === null ? undefined : nav.stack[i];
    return r?.name === "sheet" && r.kind === "notes";
  };
  const notesEntry = isNotesAt(entry.current);
  useEffect(() => {
    if (paneUp && entry.current === null) {
      entry.current = nav.stack.length;
      nav.push({ name: "sheet", kind: "notes" });
    } else if (!paneUp && entry.current !== null) {
      const at = entry.current;
      entry.current = null;
      if (at === nav.stack.length - 1 && isNotesAt(at)) void nav.back();
    }
    // 只看窗格開到多大
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [paneUp]);
  useEffect(() => {
    if (entry.current !== null && !notesEntry) {
      // 返回鍵把那一層退掉了：縮成一條（記住的大小不變，下次打開還是使用者選的大小）
      entry.current = null;
      setPane((p) => ({ ...p, size: "peek" }));
    }
  }, [notesEntry]);

  /** 頂列的返回鈕：離開閱讀畫面（窗格那一層一起退掉，窗格的大小照舊記著）。 */
  const leave = async () => {
    const at = entry.current;
    if (at !== null && at === nav.stack.length - 1 && isNotesAt(at)) {
      entry.current = null;
      await nav.back();
    }
    await nav.back();
  };

  /** 工具箱的「筆記」：打開時用記住的大小（返回鍵縮成一條不算使用者選的大小）。 */
  const toggle = () => update(pane.open ? { open: false } : { open: true, size: readPane().size });

  return { pane, update, toggle, leave };
}
