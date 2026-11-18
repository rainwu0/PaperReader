// PDF 上的筆刷（`pdfView.ts` 分出來的一塊）：每一頁一張畫布，手指畫、橡皮擦點掉、把存好的筆跡畫上去。
// 筆跡以 PDF 頁面座標記；存檔與刪除由閱讀畫面經核心做（`onStroke`、`onErase`）。

import type * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs";
import { getStroke } from "perfect-freehand";

/** 筆刷預設的紅（筆刷列的第一個顏色；匯出帶標註的 PDF 時，沒記顏色的筆跡也用它）。 */
export const DEFAULT_PEN_COLOR = "#d93025";

/** 筆刷：手指在頁面上畫。 */
export interface BrushStyle {
  color: string;
  /** 線寬，單位是 PDF 的點（與縮放無關）。 */
  width: number;
  alpha: number;
}

/** 一筆筆跡：點是 PDF 頁面座標 `[x, y]`。`page` 從 0 起算。 */
export interface InkStroke {
  id: string;
  page: number;
  color: string;
  width: number;
  opacity: number;
  points: Array<[number, number]>;
  /** 剛畫完、還沒確認存好的那幾筆用：存檔的動作結束了（成功或失敗）才算。 */
  settled?: boolean;
}

/** 手指剛畫完、要存起來的一筆。 */
export interface NewStroke {
  pageIndex: number;
  color: string;
  width: number;
  opacity: number;
  points: Array<[number, number]>;
}

/** 筆刷要用到的一頁：第幾頁（從 1 起算）、這一頁的畫布、座標換算。 */
export interface InkSlot {
  n: number;
  strokeCanvas: HTMLCanvasElement | null;
  viewport: pdfjs.PageViewport | null;
}

export interface InkOptions {
  /** 捲動容器：筆刷或橡皮擦開著時加上 `brush-on`（不捲動、讓手指畫）。 */
  scroll: HTMLElement;
  /** 橡皮擦點到一筆已存好的筆跡（`id` 是 `ink.json` 裡的代號）。 */
  onErase?: (id: string) => void;
  /** 手指畫完一筆；存檔的動作結束時（成功或失敗）交回的承諾完成。 */
  onStroke?: (stroke: NewStroke) => Promise<void> | void;
}

export class InkLayer {
  private brush: BrushStyle | null = null;
  private eraser = false;
  /** 已經存下來的筆跡，以第幾頁（從 1 起算）分。 */
  private ink = new Map<number, InkStroke[]>();
  /** 剛畫完、還沒確認存好的筆跡。 */
  private pending = new Map<number, InkStroke[]>();

  constructor(private readonly o: InkOptions) {}

  /** 開（傳筆刷的樣子）或關（傳 `null`）筆刷。 */
  setBrush(style: BrushStyle | null) {
    this.brush = style;
    this.o.scroll.classList.toggle("brush-on", style !== null || this.eraser);
  }

  /** 開關橡皮擦：開著時點到哪一筆就刪那一筆（畫筆暫時不畫）。 */
  setEraser(on: boolean) {
    this.eraser = on;
    this.o.scroll.classList.toggle("brush-on", this.brush !== null || on);
  }

  /**
   * 換上這一批已存下來的筆跡（整批取代）；存檔已經結束的暫時筆跡一併拿掉，手指正在畫、還在等存檔的留著。
   * `rendered` 是已經畫好的頁，重畫它們的筆跡。
   */
  setInk(list: InkStroke[], rendered: InkSlot[]) {
    this.ink = new Map();
    for (const st of list) {
      const arr = this.ink.get(st.page + 1) ?? [];
      arr.push(st);
      this.ink.set(st.page + 1, arr);
    }
    for (const [n, drawing] of this.pending) {
      const keep = drawing.filter((p) => !p.settled);
      if (keep.length > 0) this.pending.set(n, keep);
      else this.pending.delete(n);
    }
    for (const slot of rendered) this.draw(slot);
  }

  /** 畫面上一點（相對頁面左上角，CSS 像素）碰到哪一筆已存好的筆跡；沒有交回 `null`。 */
  private strokeAt(slot: InkSlot, vp: pdfjs.PageViewport, px: number, py: number): InkStroke | null {
    const list = this.ink.get(slot.n) ?? [];
    for (let i = list.length - 1; i >= 0; i--) {
      const st = list[i];
      if (!st) continue;
      const pts = st.points.map(([x, y]) => vp.convertToViewportPoint(x, y) as [number, number]);
      const reach = Math.max(12, (st.width * vp.scale) / 2 + 8); // 手指點不準，至少 12 像素
      for (let k = 0; k < pts.length; k++) {
        const a = pts[k];
        const b = pts[k + 1] ?? a;
        if (a && b && distToSegment(px, py, a, b) <= reach) return st;
      }
    }
    return null;
  }

  /** 這一頁的畫布：接手指的畫與擦。 */
  makeCanvas(slot: InkSlot, vp: pdfjs.PageViewport): HTMLCanvasElement {
    const c = document.createElement("canvas");
    c.className = "stroke-layer";
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    c.width = Math.floor(vp.width * dpr);
    c.height = Math.floor(vp.height * dpr);
    c.style.width = `${vp.width}px`;
    c.style.height = `${vp.height}px`;
    let current: InkStroke | null = null;
    const pdfPoint = (e: PointerEvent): [number, number] => {
      const r = c.getBoundingClientRect();
      const [x, y] = vp.convertToPdfPoint(
        ((e.clientX - r.left) / r.width) * vp.width,
        ((e.clientY - r.top) / r.height) * vp.height,
      );
      return [x ?? 0, y ?? 0];
    };
    const drop = () => {
      if (!current) return;
      const list = this.pending.get(slot.n) ?? [];
      this.pending.set(
        slot.n,
        list.filter((s) => s !== current),
      );
    };
    c.addEventListener("pointerdown", (e) => {
      if (this.eraser) {
        e.preventDefault();
        const r = c.getBoundingClientRect();
        const hit = this.strokeAt(slot, vp, ((e.clientX - r.left) / r.width) * vp.width, ((e.clientY - r.top) / r.height) * vp.height);
        if (hit && hit.id !== "") this.o.onErase?.(hit.id);
        return;
      }
      if (!this.brush) return;
      e.preventDefault();
      c.setPointerCapture(e.pointerId);
      current = {
        id: "",
        page: slot.n - 1,
        color: this.brush.color,
        width: this.brush.width,
        opacity: this.brush.alpha,
        points: [pdfPoint(e)],
      };
      const list = this.pending.get(slot.n) ?? [];
      list.push(current);
      this.pending.set(slot.n, list);
      this.draw(slot);
    });
    c.addEventListener("pointermove", (e) => {
      if (!current) return;
      current.points.push(pdfPoint(e));
      this.draw(slot);
    });
    c.addEventListener("pointerup", () => {
      const done = current;
      current = null;
      if (!done) return;
      const saving = this.o.onStroke?.({
        pageIndex: done.page,
        color: done.color,
        width: done.width,
        opacity: done.opacity,
        points: done.points,
      });
      void Promise.resolve(saving).finally(() => {
        done.settled = true;
      });
    });
    c.addEventListener("pointercancel", () => {
      drop();
      current = null;
      this.draw(slot);
    });
    return c;
  }

  /** 把這一頁存好的與正在畫的筆跡畫到它的畫布上。 */
  draw(slot: InkSlot) {
    const c = slot.strokeCanvas;
    const vp = slot.viewport;
    if (!c || !vp) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, c.width, c.height);
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    const k = c.width / vp.width; // 畫布像素對 CSS 像素
    const all = [...(this.ink.get(slot.n) ?? []), ...(this.pending.get(slot.n) ?? [])];
    for (const st of all) {
      // 存的是原始的點（頁面座標）；畫的時候才用 perfect-freehand 算成平滑、有粗細變化的輪廓
      const pts = st.points.map(([px, py]) => {
        const [x, y] = vp.convertToViewportPoint(px, py) as [number, number];
        return [x * k, y * k] as [number, number];
      });
      const outline = getStroke(pts, {
        size: Math.max(1.5, st.width * vp.scale * k),
        thinning: 0.5,
        smoothing: 0.5,
        streamline: 0.5,
        simulatePressure: true,
      });
      ctx.globalAlpha = st.opacity;
      ctx.fillStyle = st.color;
      ctx.beginPath();
      outline.forEach(([x, y], i) => {
        if (i === 0) ctx.moveTo(x ?? 0, y ?? 0);
        else ctx.lineTo(x ?? 0, y ?? 0);
      });
      ctx.closePath();
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }
}

/** 點到線段的距離。 */
function distToSegment(px: number, py: number, a: [number, number], b: [number, number]): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.min(1, Math.max(0, ((px - a[0]) * dx + (py - a[1]) * dy) / len2));
  return Math.hypot(px - (a[0] + t * dx), py - (a[1] + t * dy));
}
