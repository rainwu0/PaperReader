// 長按一列：按住不動超過半秒就觸發（首頁進入多選用）。放開後瀏覽器仍會送出一次點擊，
// 用 `consumeLong()` 吃掉它，免得長按完又被當成點一下。手指移動超過一小段（要捲動）或被系統接手就取消。

import { useRef, type PointerEvent } from "react";

const LONG_MS = 500;
const MOVE_PX = 10;

export function useLongPress(onLong: (id: string) => void, enabled: boolean) {
  const timer = useRef<number | null>(null);
  const fired = useRef(false);
  const origin = useRef({ x: 0, y: 0 });

  const clear = () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
  };

  return {
    bind: (id: string) => ({
      onPointerDown: (e: PointerEvent) => {
        fired.current = false;
        clear();
        if (!enabled) return;
        origin.current = { x: e.clientX, y: e.clientY };
        timer.current = window.setTimeout(() => {
          timer.current = null;
          fired.current = true;
          onLong(id);
        }, LONG_MS);
      },
      onPointerMove: (e: PointerEvent) => {
        if (Math.hypot(e.clientX - origin.current.x, e.clientY - origin.current.y) > MOVE_PX) clear();
      },
      onPointerUp: clear,
      onPointerCancel: clear,
      onPointerLeave: clear,
      // 長按時系統會跳出內容選單（選字、複製），在這一列上不要
      onContextMenu: (e: { preventDefault: () => void }) => e.preventDefault(),
    }),
    /** 這次點擊是長按放開後送出的：交回 `true`，並把它吃掉。 */
    consumeLong: () => {
      const f = fired.current;
      fired.current = false;
      return f;
    },
  };
}
