// 雙指縮放（`pdfView.ts` 分出來的一塊）：手指移動時只用 `transform` 縮放內容（不重排），
// 放開時把最後的倍率與焦點交給 `onEnd`，由 PDF 的檢視重新排版、捲到對應的位置。

/** 縮放的焦點：開始時的倍率、手指中心在畫面（捲動容器）裡與在文件（排版後的內容）裡的位置。 */
export interface PinchFocus {
  z0: number;
  fx: number;
  fy: number;
  px: number;
  py: number;
}

/**
 * 在捲動容器上接兩指的手勢。`zoom()` 是現在的倍率，倍率夾在 `min` 到 `max` 之間。
 * 交回拆掉這些監聽的函式。
 */
export function attachPinch(
  sc: HTMLElement,
  wrap: HTMLElement,
  zoom: () => number,
  limits: { min: number; max: number },
  onEnd: (zoom: number, focus: PinchFocus) => void,
): () => void {
  type Pinch = PinchFocus & { d0: number; ratio: number };
  let pinch: Pinch | null = null;
  const dist = (t: TouchList) => {
    const a = t[0];
    const b = t[1];
    return a && b ? Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY) : 0;
  };
  const center = (t: TouchList) => {
    const a = t[0];
    const b = t[1];
    return a && b
      ? { x: (a.clientX + b.clientX) / 2, y: (a.clientY + b.clientY) / 2 }
      : { x: 0, y: 0 };
  };
  const start = (e: TouchEvent) => {
    if (e.touches.length !== 2) return;
    const c = center(e.touches);
    const sr = sc.getBoundingClientRect();
    const wr = wrap.getBoundingClientRect();
    pinch = {
      d0: dist(e.touches),
      z0: zoom(),
      ratio: 1,
      // 手指中心在畫面（捲動容器）裡的位置
      fx: c.x - sr.left,
      fy: c.y - sr.top,
      // 手指中心在文件（排版後的內容）裡的位置，縮放前的座標
      px: c.x - wr.left,
      py: c.y - wr.top,
    };
    wrap.style.transformOrigin = `${pinch.px}px ${pinch.py}px`;
  };
  const move = (e: TouchEvent) => {
    if (!pinch || e.touches.length !== 2) return;
    e.preventDefault();
    const raw = dist(e.touches) / pinch.d0;
    pinch.ratio = Math.min(limits.max / pinch.z0, Math.max(limits.min / pinch.z0, raw));
    wrap.style.transform = `scale(${pinch.ratio})`;
  };
  const end = (e: TouchEvent) => {
    if (!pinch || e.touches.length >= 2) return;
    const p = pinch;
    pinch = null;
    wrap.style.transform = "";
    wrap.style.transformOrigin = "";
    onEnd(p.z0 * p.ratio, p);
  };
  sc.addEventListener("touchstart", start, { passive: true });
  sc.addEventListener("touchmove", move, { passive: false });
  sc.addEventListener("touchend", end, { passive: true });
  sc.addEventListener("touchcancel", end, { passive: true });
  return () => {
    sc.removeEventListener("touchstart", start);
    sc.removeEventListener("touchmove", move);
    sc.removeEventListener("touchend", end);
    sc.removeEventListener("touchcancel", end);
  };
}
