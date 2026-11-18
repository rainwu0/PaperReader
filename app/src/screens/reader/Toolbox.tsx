// 閱讀畫面的工具箱：像繪圖軟體的工具面板，三種狀態——
//   縮小：貼在畫面邊緣的一顆小圓鈕（預設）；
//   浮動：拖到哪裡停在哪裡，位置在左右兩側時排成一直欄，其餘排成橫的；
//   停靠：拖到畫面上緣或下緣附近就吸附成一整列（橫向握持時也可以停靠左右兩側成一欄）。
// 停靠在下緣時頁碼併進工具列。從停靠的列上拖出來就回到浮動。狀態與位置交給呼叫的一方記住。
// 位置用畫面寬高的千分比，所以旋轉或換螢幕大小之後仍在合理的地方。
// 浮動時選了檢視類的工具（原文、譯本、譯文、筆記）就縮回圓鈕，不一直蓋在頁面上；面板上的圖釘釘住就不縮。
// 有「換一個」的工具（譯本）：按一下照上次的直接做，長按或旁邊的小箭頭才出選單。

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { ToolboxDock, ToolboxMode } from "../../api";
import { isBool, useLocalPref, useViewportSize } from "../../hooks";
import { t } from "../../strings";
import { m as Motion, useMotion } from "../../ui/motion";
import { ChevronDownIcon, GripIcon, MinimizeIcon, PinIcon, ToolboxIcon } from "./toolIcons";

/**
 * 工具箱釘住（浮動時選了檢視也不收）：記在這台裝置上。工具箱的圖釘與設定頁的開關讀寫同一項，
 * 改了任一邊，另一邊（疊著的畫面都留著）也跟著換。
 */
export function useToolboxKeepOpen(): [boolean, (on: boolean) => void] {
  return useLocalPref("pr-toolbox-keep-open", false, isBool);
}

export interface ToolDef {
  id: string;
  label: string;
  icon: ReactNode;
  onClick: () => void;
  /** 這個工具目前開著（劃線、筆刷）或目前選的檢視（原文、譯本）。 */
  active?: boolean;
  /** 現在不能用：按下去不做事，由 `onClick` 說明為什麼。 */
  unavailable?: boolean;
  /** 檢視類（換一種看法）：浮動時選了就縮回圓鈕。 */
  view?: boolean;
  /** 換一個：長按或旁邊的小箭頭打開選單；`moreLabel` 是小箭頭的說明。 */
  onMore?: () => void;
  moreLabel?: string;
}

export interface ToolboxLayout {
  mode: ToolboxMode;
  dock: ToolboxDock;
  /** 浮動時面板左上角、縮小時圓鈕中心的位置，畫面寬高的千分比。 */
  x: number;
  y: number;
}

const SNAP_PX = 72; // 拖到離邊緣這麼近就吸附
const MINI = 48; // 縮小的圓鈕邊長
const MOVE_THRESHOLD = 8; // 動超過這麼多像素才算拖動，否則是點一下
const LONG_PRESS_MS = 500; // 按住這麼久算長按（打開「換一個」的選單）

function inset(name: string): number {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name)) || 0;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

type DragKind = "mini" | "panel" | "dockedOut";

interface Drag {
  kind: DragKind;
  startX: number;
  startY: number;
  /** 指標在被拖的東西裡的位置（讓它不會跳到指標的左上角）。 */
  offX: number;
  offY: number;
  moved: boolean;
}

export function Toolbox(props: {
  layout: ToolboxLayout;
  onLayout: (l: ToolboxLayout) => void;
  tools: ToolDef[];
  /** 停靠在下緣時併進工具列的頁碼，例如「7／30」；還不知道就是 `null`。 */
  pageText: string | null;
  onPage: () => void;
  /** 頂端的列顯示著時，停靠在上緣的工具列要放在它下面：它的高度（像素，不含狀態列）；沒有顯示是 0。 */
  topOffset: number;
  /** 釘住：浮動時選了檢視類的工具也不縮。 */
  keepOpen: boolean;
  onKeepOpen: (on: boolean) => void;
}) {
  const { layout, tools } = props;
  const { w, h } = useViewportSize();
  const el = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const layoutRef = useRef(layout);
  const [snap, setSnap] = useState<ToolboxDock | null>(null);
  const shape = useMotion("tool");
  const landscape = w > h;

  useEffect(() => {
    layoutRef.current = layout;
  });

  /** 指標現在在哪個邊緣附近（可以停靠）；橫向握持時才有左右。 */
  const snapAt = useCallback(
    (px: number, py: number): ToolboxDock | null => {
      if (py < SNAP_PX + inset("--inset-top")) return "top";
      if (py > window.innerHeight - SNAP_PX - inset("--inset-bottom")) return "bottom";
      if (landscape) {
        if (px < SNAP_PX) return "left";
        if (px > window.innerWidth - SNAP_PX) return "right";
      }
      return null;
    },
    [landscape],
  );

  // 把浮動的面板留在畫面裡（尺寸要等排版完才量得到）
  useLayoutEffect(() => {
    const node = el.current;
    if (!node || layout.mode !== "floating") return;
    const r = node.getBoundingClientRect();
    const top = inset("--inset-top");
    const bottom = window.innerHeight - inset("--inset-bottom");
    const left = clamp(r.left, 4, Math.max(4, window.innerWidth - r.width - 4));
    const topPx = clamp(r.top, top + 4, Math.max(top + 4, bottom - r.height - 4));
    node.style.left = `${left}px`;
    node.style.top = `${topPx}px`;
  });

  const onMove = useCallback(
    (e: PointerEvent) => {
      const d = drag.current;
      if (!d) return;
      const dx = e.clientX - d.startX;
      const dy = e.clientY - d.startY;
      if (!d.moved && Math.hypot(dx, dy) < MOVE_THRESHOLD) return;
      d.moved = true;
      if (d.kind === "dockedOut") {
        // 從停靠的列上拖出來：變成浮動，面板跟著指標
        d.kind = "panel";
        const l = layoutRef.current;
        props.onLayout({
          mode: "floating",
          dock: l.dock,
          x: clamp(Math.round(((e.clientX - d.offX) / window.innerWidth) * 1000), 0, 1000),
          y: clamp(Math.round(((e.clientY - d.offY) / window.innerHeight) * 1000), 0, 1000),
        });
        return;
      }
      const node = el.current;
      if (node) {
        node.style.left = `${e.clientX - d.offX}px`;
        node.style.top = `${e.clientY - d.offY}px`;
        node.style.right = "auto";
        node.style.bottom = "auto";
      }
      setSnap(d.kind === "panel" ? snapAt(e.clientX, e.clientY) : null);
    },
    // oxlint-disable-next-line react-hooks/exhaustive-deps
    [snapAt],
  );

  const onUp = useCallback(
    (e: PointerEvent) => {
      const d = drag.current;
      drag.current = null;
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      setSnap(null);
      if (!d) return;
      const l = layoutRef.current;
      if (d.kind === "mini") {
        if (!d.moved) {
          props.onLayout({ ...l, mode: "floating" });
          return;
        }
        // 圓鈕放開後貼到最近的左右邊緣
        const cx = e.clientX - d.offX + MINI / 2;
        const cy = e.clientY - d.offY + MINI / 2;
        props.onLayout({
          ...l,
          mode: "mini",
          x: cx < window.innerWidth / 2 ? 0 : 1000,
          y: clamp(Math.round((cy / window.innerHeight) * 1000), 0, 1000),
        });
        return;
      }
      if (!d.moved) return;
      const side = snapAt(e.clientX, e.clientY);
      if (side) {
        props.onLayout({ ...l, mode: "docked", dock: side });
        return;
      }
      props.onLayout({
        ...l,
        mode: "floating",
        x: clamp(Math.round(((e.clientX - d.offX) / window.innerWidth) * 1000), 0, 1000),
        y: clamp(Math.round(((e.clientY - d.offY) / window.innerHeight) * 1000), 0, 1000),
      });
    },
    // oxlint-disable-next-line react-hooks/exhaustive-deps
    [onMove, snapAt],
  );

  const startDrag = (e: React.PointerEvent, kind: DragKind) => {
    const node = el.current;
    const r = node?.getBoundingClientRect();
    drag.current = {
      kind,
      startX: e.clientX,
      startY: e.clientY,
      offX: kind === "dockedOut" ? 28 : e.clientX - (r?.left ?? e.clientX),
      offY: kind === "dockedOut" ? 24 : e.clientY - (r?.top ?? e.clientY),
      moved: false,
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  const minimize = () => props.onLayout({ ...layout, mode: "mini" });

  // 長按：按住超過一段時間、沒有移動，打開「換一個」的選單；之後的那一下點擊不算
  const press = useRef<{ id: string; x: number; y: number; timer: number; fired: boolean } | null>(null);
  const endPress = () => window.clearTimeout(press.current?.timer);
  useEffect(() => () => window.clearTimeout(press.current?.timer), []);
  const run = (tool: ToolDef) => {
    tool.onClick();
    if (tool.view && layout.mode === "floating" && !props.keepOpen) props.onLayout({ ...layout, mode: "mini" });
  };
  // 換一個的選單蓋在上面；選了就是換了檢視，浮動的工具箱先縮回
  const more = (tool: ToolDef) => {
    tool.onMore?.();
    if (tool.view && layout.mode === "floating" && !props.keepOpen) props.onLayout({ ...layout, mode: "mini" });
  };

  const toolButtons = tools.map((tool) => (
    <span key={tool.id} className="tb-slot">
      <button
        type="button"
        className="tb-btn"
        aria-label={tool.label}
        aria-pressed={tool.active ?? undefined}
        aria-disabled={tool.unavailable ? true : undefined}
        data-tool={tool.id}
        onPointerDown={(e) => {
          if (!tool.onMore) return;
          endPress();
          press.current = {
            id: tool.id,
            x: e.clientX,
            y: e.clientY,
            fired: false,
            timer: window.setTimeout(() => {
              if (press.current) press.current.fired = true;
              more(tool);
            }, LONG_PRESS_MS),
          };
        }}
        onPointerMove={(e) => {
          const p = press.current;
          if (p && Math.hypot(e.clientX - p.x, e.clientY - p.y) > MOVE_THRESHOLD) endPress();
        }}
        onPointerUp={endPress}
        onPointerCancel={endPress}
        onContextMenu={(e) => tool.onMore && e.preventDefault()}
        onClick={() => {
          if (press.current?.id === tool.id && press.current.fired) {
            press.current = null;
            return;
          }
          run(tool);
        }}
      >
        {tool.icon}
        <span className="tb-label">{tool.label}</span>
      </button>
      {tool.onMore ? (
        <button type="button" className="tb-more" aria-label={tool.moreLabel ?? tool.label} onClick={() => more(tool)}>
          <ChevronDownIcon />
        </button>
      ) : null}
    </span>
  ));

  // ---------- 縮小 ----------
  // 換形時縮放展開（進場 200ms，容器轉換的簡化版）；拖動只改 left／top，不碰 transform
  const { initial, animate } = shape;
  if (layout.mode === "mini") {
    const top = inset("--inset-top");
    const bottom = inset("--inset-bottom");
    const cx = (layout.x / 1000) * w;
    const left = clamp(cx - MINI / 2, 4, w - MINI - 4);
    const topPx = clamp((layout.y / 1000) * h - MINI / 2, top + 4, h - bottom - MINI - 4);
    return (
      <Motion.button
        key="mini"
        ref={el as unknown as React.RefObject<HTMLButtonElement>}
        type="button"
        className="tb-mini"
        aria-label={t("reader.toolbox.open")}
        style={{ left, top: topPx }}
        onPointerDown={(e) => startDrag(e, "mini")}
        initial={initial}
        animate={animate}
      >
        <ToolboxIcon />
      </Motion.button>
    );
  }

  // ---------- 停靠 ----------
  if (layout.mode === "docked") {
    const vertical = layout.dock === "left" || layout.dock === "right";
    // 上緣：狀態列底下（CSS 的 `--inset-top`），頂列顯示著時再往下讓出頂列
    const style = layout.dock === "top" ? { top: `calc(var(--inset-top) + ${props.topOffset}px)` } : undefined;
    return (
      <Motion.div
        key={`docked-${layout.dock}`}
        ref={el}
        className={`tb-docked ${layout.dock}`}
        role="toolbar"
        aria-label={t("reader.toolbox")}
        style={style}
        initial={initial}
        animate={animate}
      >
        <button
          type="button"
          className="tb-grip"
          aria-label={t("reader.toolbox.grip")}
          onPointerDown={(e) => startDrag(e, "dockedOut")}
        >
          <GripIcon />
        </button>
        <div className={vertical ? "tb-scroll vertical" : "tb-scroll"}>{toolButtons}</div>
        {layout.dock === "bottom" && props.pageText ? (
          <button type="button" className="tb-page" aria-label={t("reader.jump.open", { page: props.pageText })} onClick={props.onPage}>
            {props.pageText}
          </button>
        ) : null}
        <button type="button" className="tb-min" aria-label={t("reader.toolbox.close")} onClick={minimize}>
          <MinimizeIcon />
        </button>
      </Motion.div>
    );
  }

  // ---------- 浮動 ----------
  const vertical = layout.x < 330 || layout.x > 670;
  return (
    <>
      {snap ? <div className={`tb-snap ${snap}`} aria-hidden="true" /> : null}
      <Motion.div
        key="float"
        ref={el}
        className={vertical ? "tb-float vertical" : "tb-float"}
        role="toolbar"
        aria-label={t("reader.toolbox")}
        style={{ left: (layout.x / 1000) * w, top: (layout.y / 1000) * h }}
        initial={initial}
        animate={animate}
      >
        <div className="tb-head">
          <button
            type="button"
            className="tb-grip"
            aria-label={t("reader.toolbox.grip")}
            onPointerDown={(e) => startDrag(e, "panel")}
          >
            <GripIcon />
          </button>
          <button
            type="button"
            className="tb-pin"
            aria-label={t("reader.toolbox.keepOpen")}
            aria-pressed={props.keepOpen}
            onClick={() => props.onKeepOpen(!props.keepOpen)}
          >
            <PinIcon />
          </button>
          <button type="button" className="tb-min" aria-label={t("reader.toolbox.close")} onClick={minimize}>
            <MinimizeIcon />
          </button>
        </div>
        <div className="tb-tools">{toolButtons}</div>
      </Motion.div>
    </>
  );
}
