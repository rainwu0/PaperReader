// 轉換。時間與 easing 只用 `theme.css` 的 token，這裡是同樣的數值（Motion 用秒）。
// 退場由 `AnimatePresence` 處理：元素在卸載前播完退場；播放中仍可操作，新的操作直接打斷。
// 系統開了「移除動畫」（prefers-reduced-motion）時，位移與縮放拿掉，只留 100ms 的淡入淡出。

import {
  AnimatePresence,
  LazyMotion,
  MotionConfig,
  domAnimation,
  m,
  useIsPresent,
  useReducedMotion,
  type TargetAndTransition,
} from "motion/react";
import type { ReactNode } from "react";

export { AnimatePresence, m, useIsPresent, useReducedMotion };

/** 時間（秒）：`--motion-short`、`--motion-quick`、`--motion-medium`、`--motion-long`。 */
export const DUR = { short: 0.1, quick: 0.15, medium: 0.2, long: 0.3 } as const;
/** easing：`--ease-standard`、`--ease-enter`、`--ease-exit`。 */
type Bezier = [number, number, number, number];
export const EASE: Record<"standard" | "enter" | "exit", Bezier> = {
  standard: [0.2, 0, 0, 1],
  enter: [0.05, 0.7, 0.1, 1],
  exit: [0.3, 0, 0.8, 0.15],
};

/** 包在整個應用外面：只載入用到的功能；跟著系統的「移除動畫」。 */
export function MotionRoot(props: { children: ReactNode }) {
  return (
    <LazyMotion features={domAnimation} strict>
      <MotionConfig reducedMotion="user">{props.children}</MotionConfig>
    </LazyMotion>
  );
}

interface Preset {
  initial: TargetAndTransition;
  animate: TargetAndTransition;
  exit: TargetAndTransition;
}

/**
 * 轉換的種類：
 * - `screen`：上下層的前進與後退，共軸 X（位移 30px 加淡化）。
 * - `sheet`：底部面板，從下緣滑入。
 * - `dialog`：置中對話框與選單，淡入加縮放 0.95 → 1。
 * - `scrim`：面板與側邊欄後面的遮罩。
 * - `drawerLeft`／`paneRight`／`paneUp`：側邊欄、筆記窗格、筆刷列從邊緣滑入。
 * - `toast`：短暫提示，淡入加上移 8px。
 * - `pop`：選字後浮出的小按鈕（很頻繁，要短）。
 * - `tool`：工具箱在縮小、浮動、停靠之間換形（容器轉換的簡化版：縮放展開，只有進場）。
 * - `view`：蓋滿畫面的另一種檢視（逐段對照）：淡入 200ms、淡出 150ms。
 * - `fade`：畫面內的元素、同層切換（淡入 150ms、淡出 100ms）。
 */
export type PresetKind =
  | "screen"
  | "sheet"
  | "dialog"
  | "scrim"
  | "drawerLeft"
  | "paneRight"
  | "paneUp"
  | "barUp"
  | "toast"
  | "pop"
  | "tool"
  | "view"
  | "fade";

function make(kind: PresetKind): Preset {
  const enterLong = { duration: DUR.long, ease: EASE.enter };
  const exitMedium = { duration: DUR.medium, ease: EASE.exit };
  switch (kind) {
    case "screen":
      return { initial: { opacity: 0, x: 30 }, animate: { opacity: 1, x: 0, transition: enterLong }, exit: { opacity: 0, x: 30, transition: exitMedium } };
    case "sheet":
      return { initial: { y: "100%" }, animate: { y: 0, transition: enterLong }, exit: { y: "100%", transition: exitMedium } };
    case "drawerLeft":
      return { initial: { x: "-100%" }, animate: { x: 0, transition: enterLong }, exit: { x: "-100%", transition: exitMedium } };
    case "paneRight":
      return { initial: { x: "100%" }, animate: { x: 0, transition: enterLong }, exit: { x: "100%", transition: exitMedium } };
    case "paneUp":
      return { initial: { y: "100%" }, animate: { y: 0, transition: enterLong }, exit: { y: "100%", transition: exitMedium } };
    case "barUp":
      return {
        initial: { opacity: 0, y: 24 },
        animate: { opacity: 1, y: 0, transition: { duration: DUR.medium, ease: EASE.enter } },
        exit: { opacity: 0, y: 24, transition: { duration: DUR.quick, ease: EASE.exit } },
      };
    case "scrim":
      return { initial: { opacity: 0 }, animate: { opacity: 1, transition: enterLong }, exit: { opacity: 0, transition: exitMedium } };
    case "dialog":
      return {
        initial: { opacity: 0, scale: 0.95 },
        animate: { opacity: 1, scale: 1, transition: { duration: DUR.quick, ease: EASE.enter } },
        exit: { opacity: 0, transition: { duration: DUR.short, ease: "linear" } },
      };
    case "toast":
      return {
        initial: { opacity: 0, y: 8 },
        animate: { opacity: 1, y: 0, transition: { duration: DUR.quick, ease: EASE.enter } },
        exit: { opacity: 0, transition: { duration: DUR.short, ease: "linear" } },
      };
    case "pop":
      return {
        initial: { opacity: 0, scale: 0.9 },
        animate: { opacity: 1, scale: 1, transition: { duration: DUR.short, ease: EASE.enter } },
        exit: { opacity: 0, transition: { duration: DUR.short, ease: "linear" } },
      };
    case "tool":
      return {
        initial: { opacity: 0, scale: 0.9 },
        animate: { opacity: 1, scale: 1, transition: { duration: DUR.medium, ease: EASE.standard } },
        exit: { opacity: 0, transition: { duration: DUR.short, ease: "linear" } },
      };
    case "view":
      return {
        initial: { opacity: 0 },
        animate: { opacity: 1, transition: { duration: DUR.medium, ease: EASE.standard } },
        exit: { opacity: 0, transition: { duration: DUR.quick, ease: EASE.standard } },
      };
    case "fade":
      return {
        initial: { opacity: 0 },
        animate: { opacity: 1, transition: { duration: DUR.quick, ease: EASE.standard } },
        exit: { opacity: 0, transition: { duration: DUR.short, ease: EASE.standard } },
      };
  }
}

/** 減少動態：只淡入淡出，100ms 以內。 */
const REDUCED: Preset = {
  initial: { opacity: 0 },
  animate: { opacity: 1, transition: { duration: DUR.short, ease: "linear" } },
  exit: { opacity: 0, transition: { duration: DUR.short, ease: "linear" } },
};

const PRESETS = Object.fromEntries(
  (["screen", "sheet", "dialog", "scrim", "drawerLeft", "paneRight", "paneUp", "barUp", "toast", "pop", "tool", "view", "fade"] as const).map((k) => [k, make(k)]),
) as Record<PresetKind, Preset>;

/** 一種轉換的 `initial`／`animate`／`exit`，已照系統的「移除動畫」調好。 */
export function useMotion(kind: PresetKind): Preset {
  const reduced = useReducedMotion() ?? false;
  return reduced ? REDUCED : PRESETS[kind];
}

/**
 * 不是 React 管的元素（PDF 的頁面）淡入：原文與譯本互換時用（淡出淡入）。
 * 移除動畫時直接換。只動透明度。
 */
export function fadeInElement(el: HTMLElement): void {
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  el.animate([{ opacity: 0 }, { opacity: 1 }], {
    duration: DUR.quick * 1000,
    easing: `cubic-bezier(${EASE.standard.join(", ")})`,
  });
}

/** 退場中的元素不接收觸控，底下新的一層可以直接操作。 */
export function useExitingStyle(): { pointerEvents?: "none" } {
  return useIsPresent() ? {} : { pointerEvents: "none" };
}
