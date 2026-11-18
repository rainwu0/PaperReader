// PDF 的顯示：PDF.js 的相容版（legacy：預設版在舊的系統網頁元件上開不了檔）。
//
// 做的事：上下捲動的連續頁面、雙指縮放、雙擊放大到欄寬（再雙擊縮回）、回報目前頁碼、讀出與回到閱讀位置（頁碼＋頁內位置＋縮放）。
// 原文的位元組只經 `api.readMaterialRange` 分段唯讀取得；
// PDF 的內容在 PDF.js 的 worker 裡處理，碰不到 Tauri 的指令。
// 文字層讓瀏覽器原生的選取可用（劃線）；筆刷的筆跡以 PDF 頁面座標記，存取由閱讀畫面經核心做。
//
// 版面的所有尺寸（頁寬、頁間距、邊距）都隨縮放等比例變化，所以雙指縮放前後，
// 手指下的那一點可以精確保持不動（試作二量到的「焦點偏移」就是尺寸沒有等比例造成的）。

import type * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs";
import { HIGHLIGHT_COLOR, type Rect } from "../../api/types";
import { columnAt, type ColumnSpan, type PageInput } from "../../translate/layout";
import { coversFromInfo, filterChunk, type ChunkLike, type Covers, type TextItemLike } from "../../translate/textFilter";
import { InkLayer, type BrushStyle, type InkStroke, type NewStroke } from "./pdfInk";
import { attachPinch } from "./pdfPinch";
import { captureSelection, firstVisibleLine, type SelectionCapture } from "./pdfSelection";
import { makeTextLayer, startPdf, type MaterialRangeTransport } from "../../pdf/pdfSource";
import type { PageRegion } from "../../ext/types";
import { toRawItems } from "../../translate/extract";
import { RegionLayer, type RegionSource } from "./pdfRegions";
import { segmentAt, shownText, type Seg } from "./segments";
import { rangeBox } from "./textRange";

// 選取、筆刷、雙指縮放各在自己的檔；型別從這裡一起交出去，閱讀畫面不必知道分在哪裡
export type { BrushStyle, InkStroke, NewStroke } from "./pdfInk";
export type { SelectionCapture } from "./pdfSelection";

const PAD = 8; // 縮放 1 倍時的邊距與頁間距（CSS 像素）
const MIN_ZOOM = 1;
const MAX_ZOOM = 4;
const MAX_CANVAS_PIXELS = 12_000_000;
/** 雙擊放大算不出欄時的縮放：頁寬的兩倍（縮放 1 倍是頁寬剛好填滿畫面）。 */
const FALLBACK_ZOOM = 2;

/** 大綱的一項（攤平後）：標題、第幾層（0 是最上層）、指向的位置（PDF.js 的目標，解成頁碼要再問一次）。 */
export interface OutlineEntry {
  title: string;
  level: number;
  dest: string | unknown[] | null;
}

interface OutlineNode {
  title?: string;
  dest?: string | unknown[] | null;
  items?: OutlineNode[];
}

/**
 * 原文與譯本之間切換時帶著的位置（兩份版面相同，所以這四個值就對得上）：
 * 頁序、畫面中線落在該頁高度的比例（0 到 1）、水平捲動占可捲範圍的比例（0 到 1）、縮放倍率。
 */
export interface SwitchPosition {
  page: number;
  mid: number;
  hx: number;
  zoom: number;
}

/**
 * 記進閱讀位置的位置：畫面頂端在第幾頁、頁內幾分之幾，縮放倍率，
 * 畫面左緣在那一頁寬度的幾分之幾（放大讀一欄時才有意義；沒有放大是 0）。
 */
export interface ViewPosition {
  page: number;
  offset: number;
  zoom: number;
  left: number;
}

/** 回到一個記下的位置；`left` 沒有記（舊的紀錄）時是 `null`，停在頁面左緣。 */
export interface StoredPosition {
  page: number;
  offset: number;
  zoom: number | null;
  left?: number | null;
}

interface PageSlot {
  n: number;
  box: HTMLDivElement;
  rendered: boolean;
  token: number;
  /** 畫好的這一頁的座標換算（頁面座標 ⇄ 畫面像素）。 */
  viewport: pdfjs.PageViewport | null;
  hlLayer: HTMLDivElement | null;
  textLayer: HTMLDivElement | null;
  strokeCanvas: HTMLCanvasElement | null;
  /** 譯文疊加：把譯文畫在原段落的位置上。 */
  ovLayer: HTMLDivElement | null;
}

/**
 * 一段在頁面上的方框（譯文疊加檔的段落；`page` 是頁序，從 0 起算；`bbox` 是 PDF 頁面座標）。
 * 原頁疊譯文與「點一段看對照」共用同一份：疊譯文時把 `text`（譯文）畫在方框裡，點一下用方框判斷點到哪一段。
 */
export interface SegmentBox extends Seg {
  size: number | null;
}

/** 一條要畫在頁面上的劃線（`n` 是第幾頁，從 1 起算）。 */
export interface DrawnHighlight {
  id: string;
  n: number;
  rects: Rect[];
  /** 有短評的劃線畫得稍微不同。 */
  hasComment: boolean;
}

export interface PdfViewOptions {
  scroll: HTMLElement;
  wrap: HTMLElement;
  /** 目前頁碼變了。 */
  onPage: (page: number) => void;
  /** 捲動或縮放了（用來安排「停下來之後存位置」）。 */
  onMove: () => void;
  /** 橡皮擦點到一筆已存好的筆跡（`id` 是 `ink.json` 裡的代號）。 */
  onErase?: (id: string) => void;
  /** 手指畫完一筆；存檔的動作結束時（成功或失敗）交回的承諾完成。 */
  onStroke?: (stroke: NewStroke) => Promise<void> | void;
  /** 開啟之後，原文有一段讀不出來（那幾頁會是空白）。 */
  onReadError?: (e: unknown) => void;
}

export class PdfView {
  private pdf: pdfjs.PDFDocumentProxy | null = null;
  private covers: Covers | null = null;
  private transport: MaterialRangeTransport | null = null;
  private slots: PageSlot[] = [];
  private pageW = 595;
  private pageH = 842;
  private zoom = 1;
  private lastPage = 0;
  /** 最後一次在畫面還接著時量到的位置。離開畫面時 DOM 可能已經被拿掉，這時量不到，改用它。 */
  private lastKnown: ViewPosition | null = null;
  private lastSwitch: SwitchPosition | null = null;
  private queue: number[] = [];
  private rendering = false;
  private destroyed = false;
  private observer: IntersectionObserver | null = null;
  private resizeObserver: ResizeObserver | null = null;
  private lastWidth = 0;
  private cleanups: Array<() => void> = [];
  /** 筆刷（`pdfInk.ts`）。 */
  private readonly inkLayer: InkLayer;

  constructor(private readonly o: PdfViewOptions) {
    this.inkLayer = new InkLayer({ scroll: o.scroll, onErase: o.onErase, onStroke: o.onStroke });
  }

  /** PDF.js 讀出的文件識別碼（劃線記下它，檔案換版時用得上）。 */
  get fingerprint(): string | null {
    return this.pdf?.fingerprints?.[0] ?? null;
  }

  get numPages(): number {
    return this.pdf?.numPages ?? 0;
  }

  /** 開啟原文。回傳總頁數。開啟之後才讀不出來的（某幾頁）經 `onReadError` 通知。 */
  async load(paperId: string, materialId: string, size: number): Promise<number> {
    const { opened, transport } = await startPdf(paperId, materialId, size);
    this.transport = transport;
    const pdf = await opened;
    transport.onFailure((e) => {
      if (!this.destroyed) this.o.onReadError?.(e);
    });
    if (this.destroyed) {
      void pdf.loadingTask.destroy();
      throw new Error("destroyed");
    }
    this.pdf = pdf;
    // 產生的譯本 PDF：蓋住的範圍記在文件資訊裡，文字層要濾掉被蓋的原文
    this.covers = await pdf
      .getMetadata()
      .then((m) => coversFromInfo(m.info))
      .catch(() => null);
    const first = await pdf.getPage(1);
    // 等的這段時間裡已經換了別份材料（原文與譯本之間快速切換）：不要再把這一份的頁面放進共用的容器
    if (this.destroyed) {
      void pdf.loadingTask.destroy();
      throw new Error("destroyed");
    }
    const vp = first.getViewport({ scale: 1 });
    this.pageW = vp.width;
    this.pageH = vp.height;
    first.cleanup();
    this.lastWidth = this.contentWidth();
    this.attach();
    this.layout();
    return pdf.numPages;
  }

  /**
   * 這條劃線的引句還在不在它原來的位置上。產生的譯本 PDF 重新產生之後，那一段的譯文若改了，
   * 舊的劃線（座標與引句是舊譯文的）就對不上；對不上的由呼叫的一方列為「找不回」，不畫、不刪。
   */
  async quoteStillAt(pageIndex: number, rects: Rect[], exact: string): Promise<boolean> {
    const pdf = this.pdf;
    if (!pdf) return true;
    const page = await pdf.getPage(pageIndex + 1);
    try {
      const content = await page.getTextContent({ includeMarkedContent: true });
      const cover = this.covers?.pages[String(pageIndex)];
      const items = (cover ? filterChunk(content as unknown as ChunkLike, cover, []).items : content.items) as TextItemLike[];
      // 劃線的矩形是選到的幾個字，文字項目是一整行：只要有重疊就算（引句要出現在重疊的那幾行裡）
      const inside = (it: TextItemLike) => {
        const tr = it.transform;
        if (!tr || it.str === undefined) return false;
        const x0 = tr[4] ?? 0;
        const y0 = tr[5] ?? 0;
        const x1 = x0 + (it.width ?? 0);
        const y1 = y0 + (it.height ?? 0);
        return rects.some(([l, b, r, t]) => x0 <= r + 1 && x1 >= l - 1 && y0 <= t + 1 && y1 >= b - 1);
      };
      const here = items.filter(inside).map((it) => it.str ?? "").join("").replace(/\s+/g, "");
      const want = exact.replace(/\s+/g, "");
      return want === "" || here.includes(want);
    } finally {
      page.cleanup();
    }
  }

  // ---------- 跳頁與大綱 ----------

  /** 跳到某一頁的頁首（超出範圍會夾到頭尾）。跳過去之後照一般的捲動流程記下位置。 */
  goToPage(page: number) {
    // 頁首再往下一點點：剛好卡在頁首時，捲動位置的取整會讓頁碼算成上一頁
    this.scrollToPosition(page, 0.002);
    this.o.onMove();
  }

  /** PDF 自帶的大綱，攤平成一串（帶層級）。沒有大綱交回空的。 */
  async outline(): Promise<OutlineEntry[]> {
    const pdf = this.pdf;
    if (!pdf) return [];
    const tree = await pdf.getOutline();
    const out: OutlineEntry[] = [];
    const walk = (nodes: OutlineNode[], level: number) => {
      for (const n of nodes) {
        const title = (n.title ?? "").replace(/\s+/g, " ").trim();
        if (title !== "") out.push({ title, level, dest: n.dest ?? null });
        walk(n.items ?? [], level + 1);
      }
    };
    walk((tree ?? []) as OutlineNode[], 0);
    return out;
  }

  /** 把大綱項目指向的位置換成頁碼；解不出來（外部連結、壞掉的目標）交回 `null`。 */
  async pageOfDest(dest: OutlineEntry["dest"]): Promise<number | null> {
    const pdf = this.pdf;
    if (!pdf || dest === null) return null;
    try {
      const explicit = typeof dest === "string" ? await pdf.getDestination(dest) : dest;
      const ref = explicit?.[0];
      if (ref === null || ref === undefined) return null;
      if (typeof ref === "number") return ref + 1;
      return (await pdf.getPageIndex(ref as never)) + 1;
    } catch {
      return null;
    }
  }

  // ---------- 位置 ----------

  /** 畫面頂端在文獻裡的位置：第幾頁、頁內幾分之幾（0 是頁首，1 是頁尾）。 */
  getPosition(): ViewPosition {
    if (!this.o.scroll.isConnected && this.lastKnown) return { ...this.lastKnown, zoom: this.zoom };
    const pos = this.measurePosition();
    this.lastKnown = pos;
    return pos;
  }

  private measurePosition(): ViewPosition {
    const n = this.slots.length;
    // DOM 已經被拿掉時什麼都量不到（每一頁的位置都是 0），不能把它當成「讀到最後一頁」
    if (n === 0 || !this.o.scroll.isConnected) return { page: 1, offset: 0, zoom: this.zoom, left: 0 };
    const top = this.o.scroll.scrollTop;
    // 二分搜尋：最後一個頂端不低於 `top` 的頁
    let lo = 0;
    let hi = n - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      const slot = this.slots[mid];
      if (slot && slot.box.offsetTop <= top) lo = mid;
      else hi = mid - 1;
    }
    const slot = this.slots[lo];
    if (!slot) return { page: 1, offset: 0, zoom: this.zoom, left: 0 };
    const h = slot.box.offsetHeight || 1;
    const offset = clamp((top - slot.box.offsetTop) / h, 0, 1);
    // 水平：畫面左緣落在這一頁寬度的幾分之幾（換了螢幕寬度也對得上同一欄）
    const w = slot.box.offsetWidth || 1;
    const left = clamp((this.o.scroll.scrollLeft - slot.box.offsetLeft) / w, 0, 1);
    return { page: slot.n, offset, zoom: this.zoom, left };
  }

  /** 切換用的位置：畫面中線在哪一頁的哪個高度、水平捲到哪、縮放多少。 */
  getSwitchPosition(): SwitchPosition {
    const sc = this.o.scroll;
    if (!sc.isConnected || this.slots.length === 0) {
      const last = this.lastSwitch;
      return last ? { ...last, zoom: this.zoom } : { page: 1, mid: 0, hx: 0, zoom: this.zoom };
    }
    const midY = sc.scrollTop + sc.clientHeight / 2;
    let lo = 0;
    let hi = this.slots.length - 1;
    while (lo < hi) {
      const m = (lo + hi + 1) >> 1;
      const slot = this.slots[m];
      if (slot && slot.box.offsetTop <= midY) lo = m;
      else hi = m - 1;
    }
    const slot = this.slots[lo];
    const h = slot?.box.offsetHeight || 1;
    const range = sc.scrollWidth - sc.clientWidth;
    const pos: SwitchPosition = {
      page: slot?.n ?? 1,
      mid: clamp((midY - (slot?.box.offsetTop ?? 0)) / h, 0, 1),
      hx: range > 1 ? clamp(sc.scrollLeft / range, 0, 1) : 0,
      zoom: this.zoom,
    };
    this.lastSwitch = pos;
    return pos;
  }

  /** 照切換用的位置捲到同一處。頁序超出這份 PDF 的頁數時停在最後一頁，交回 `false`。 */
  restoreSwitch(p: SwitchPosition): boolean {
    const sc = this.o.scroll;
    if (Math.abs(p.zoom - this.zoom) > 0.001) {
      this.zoom = clamp(p.zoom, MIN_ZOOM, MAX_ZOOM);
      this.layout();
    }
    const n = this.slots.length;
    const clamped = p.page > n;
    const slot = this.slots[clamp(p.page, 1, Math.max(1, n)) - 1];
    if (!slot) return true;
    const mid = clamped ? 0.5 : p.mid;
    sc.scrollTop = slot.box.offsetTop + mid * slot.box.offsetHeight - sc.clientHeight / 2;
    const range = sc.scrollWidth - sc.clientWidth;
    sc.scrollLeft = range > 1 ? p.hx * range : 0;
    this.reportPage();
    this.o.onMove();
    return !clamped;
  }

  /** 回到一個位置。縮放先套用（重新排版），再捲到那一頁的那個高度、水平捲到那一欄。 */
  restore(pos: StoredPosition) {
    if (pos.zoom && Math.abs(pos.zoom - this.zoom) > 0.001) {
      this.zoom = clamp(pos.zoom, MIN_ZOOM, MAX_ZOOM);
      this.layout();
    }
    const slot = this.slots[clamp(pos.page, 1, Math.max(1, this.slots.length)) - 1];
    if (slot && pos.left != null) {
      this.o.scroll.scrollLeft = slot.box.offsetLeft + clamp(pos.left, 0, 1) * slot.box.offsetWidth;
    }
    this.scrollToPosition(pos.page, pos.offset);
  }

  private scrollToPosition(page: number, offset: number) {
    const slot = this.slots[clamp(page, 1, Math.max(1, this.slots.length)) - 1];
    if (!slot) return;
    this.o.scroll.scrollTop = slot.box.offsetTop + clamp(offset, 0, 1) * slot.box.offsetHeight;
    this.reportPage();
  }

  // ---------- 版面 ----------

  private contentWidth(): number {
    const cs = getComputedStyle(this.o.scroll);
    return (
      this.o.scroll.clientWidth - parseFloat(cs.paddingLeft || "0") - parseFloat(cs.paddingRight || "0")
    );
  }

  /** 縮放 1 倍時，一個 PDF 單位對應幾個 CSS 像素：頁寬剛好填滿（扣掉邊距）。 */
  private fit(): number {
    return Math.max(50, this.contentWidth() - 2 * PAD) / this.pageW;
  }

  private scale(): number {
    return this.fit() * this.zoom;
  }

  private layout() {
    const { wrap } = this.o;
    this.observer?.disconnect();
    this.queue = [];
    wrap.replaceChildren();
    this.slots = [];
    const pdf = this.pdf;
    if (!pdf) return;
    const s = this.scale();
    const pad = PAD * this.zoom;
    wrap.style.padding = `${pad}px`;
    wrap.style.width = `${this.pageW * s + 2 * pad}px`;
    const w = this.pageW * s;
    const h = this.pageH * s;
    const frag = document.createDocumentFragment();
    for (let n = 1; n <= pdf.numPages; n++) {
      const box = document.createElement("div");
      box.className = "pg";
      box.dataset["n"] = String(n);
      box.style.width = `${w}px`;
      box.style.height = `${h}px`;
      box.style.marginBottom = `${n === pdf.numPages ? 0 : pad}px`;
      frag.appendChild(box);
      this.slots.push({
        n,
        box,
        rendered: false,
        token: 0,
        viewport: null,
        hlLayer: null,
        textLayer: null,
        strokeCanvas: null,
        ovLayer: null,
      });
    }
    wrap.appendChild(frag);
    this.observer = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          const slot = this.slots[Number((e.target as HTMLElement).dataset["n"]) - 1];
          if (!slot) continue;
          if (e.isIntersecting) this.enqueue(slot.n);
          else this.release(slot);
        }
      },
      { root: this.o.scroll, rootMargin: "120% 0px" },
    );
    for (const slot of this.slots) this.observer.observe(slot.box);
  }

  // ---------- 繪製 ----------

  private enqueue(n: number) {
    if (this.queue.includes(n)) return;
    this.queue.push(n);
    void this.drain();
  }

  private async drain() {
    if (this.rendering) return;
    this.rendering = true;
    try {
      while (this.queue.length > 0 && !this.destroyed) {
        const n = this.queue.shift();
        const slot = n === undefined ? undefined : this.slots[n - 1];
        if (!slot || slot.rendered) continue;
        try {
          await this.renderPage(slot);
        } catch {
          // 這一頁畫不出來就留白，不影響其他頁
        }
      }
    } finally {
      this.rendering = false;
    }
  }

  private async renderPage(slot: PageSlot) {
    const pdf = this.pdf;
    if (!pdf) return;
    const token = ++slot.token;
    const page = await pdf.getPage(slot.n);
    const viewport = page.getViewport({ scale: this.scale() });
    let dpr = Math.min(window.devicePixelRatio || 1, 3); // 螢幕的像素比，上限 3；畫布太大時下面再降
    while (viewport.width * dpr * viewport.height * dpr > MAX_CANVAS_PIXELS && dpr > 0.5) dpr -= 0.25;
    const canvas = document.createElement("canvas");
    canvas.width = Math.floor(viewport.width * dpr);
    canvas.height = Math.floor(viewport.height * dpr);
    canvas.style.width = `${viewport.width}px`;
    canvas.style.height = `${viewport.height}px`;
    await page.render({
      canvas,
      viewport,
      ...(dpr !== 1 ? { transform: [dpr, 0, 0, dpr, 0, 0] } : {}),
    }).promise;
    if (slot.token !== token || !slot.box.isConnected || this.destroyed) {
      page.cleanup();
      return;
    }
    slot.box.style.width = `${viewport.width}px`;
    slot.box.style.height = `${viewport.height}px`;
    // PDF.js 的文字層用 --scale-factor 換算尺寸，沒有它文字會縮成八分之一、選不到（reader.css 的 .pg 有其餘的變數）
    slot.box.style.setProperty("--scale-factor", String(viewport.scale));
    // 疊在畫面上的幾層：劃線（在文字下面，不擋選取）、文字層（透明、可選取）、筆刷
    const hl = document.createElement("div");
    hl.className = "hl-layer";
    hl.style.setProperty("--hl-fill", HIGHLIGHT_COLOR);
    const tl = document.createElement("div");
    tl.className = "textLayer";
    const sc = this.inkLayer.makeCanvas(slot, viewport);
    const ov = document.createElement("div");
    ov.className = "ov-layer";
    slot.box.replaceChildren(canvas, ov, hl, tl, sc);
    slot.viewport = viewport;
    slot.hlLayer = hl;
    slot.textLayer = tl;
    slot.strokeCanvas = sc;
    slot.ovLayer = ov;
    slot.rendered = true;
    this.drawOverlay(slot);
    this.drawHighlights(slot);
    this.inkLayer.draw(slot);
    // 文字層晚一點出來也沒關係：頁面圖先顯示
    try {
      await makeTextLayer(page, tl, viewport, this.covers?.pages[String(slot.n - 1)]).render();
    } catch {
      // 沒有文字層（掃描版或讀不出來）就只能看，不能選字
    }
    page.cleanup();
    // 文字層畫好了：向外掛要這一頁的可點區域（有登記來源時）
    if (slot.token === token && slot.rendered) this.regions.load(slot.n);
  }

  private release(slot: PageSlot) {
    if (!slot.rendered) return;
    slot.token++;
    const c = slot.box.querySelector("canvas");
    if (c) {
      c.width = 0;
      c.height = 0;
    }
    slot.box.replaceChildren();
    slot.rendered = false;
    this.regions.drop(slot.n);
    slot.viewport = null;
    slot.hlLayer = null;
    slot.textLayer = null;
    slot.strokeCanvas = null;
    slot.ovLayer = null;
  }

  // ---------- 段落的方框：原頁疊譯文、點一段看對照 ----------

  /** 這份 PDF 上每一段的方框，以第幾頁（從 1 起算）分。 */
  private segments = new Map<number, SegmentBox[]>();
  private overlayOn = false;
  /** 剛跳過來、要指出來的那一段（一兩秒後拿掉）。 */
  private focus: { page: number; bbox: SegmentBox["bbox"] } | null = null;
  private focusTimer = 0;
  /** 框出的那一處已經畫出來（計時開始了）。 */
  private focusShown = false;

  /** 換上這份 PDF 的段落方框（版面要和譯文疊加檔的原文相同：原文、或由它產生的譯本 PDF）；`null` 是沒有。 */
  setSegments(list: SegmentBox[] | null) {
    this.segments = new Map();
    for (const b of list ?? []) {
      const arr = this.segments.get(b.page + 1) ?? [];
      arr.push(b);
      this.segments.set(b.page + 1, arr);
    }
    for (const slot of this.slots) if (slot.rendered) this.drawOverlay(slot);
  }

  /**
   * 開關原頁疊譯文：翻好的段落把譯文畫在原段落的位置上、蓋住原文；點一下可以暫時看原文（`peek`）。
   * 沒翻好的段落不蓋。
   */
  setOverlay(on: boolean) {
    this.overlayOn = on;
    this.o.wrap.classList.toggle("overlay-on", on);
    for (const slot of this.slots) if (slot.rendered) this.drawOverlay(slot);
  }

  private drawOverlay(slot: PageSlot) {
    const layer = slot.ovLayer;
    const vp = slot.viewport;
    if (!layer || !vp) return;
    layer.replaceChildren();
    const place = (el: HTMLElement, bbox: SegmentBox["bbox"]) => {
      const [x1, y1] = vp.convertToViewportPoint(bbox[0], bbox[1]) as [number, number];
      const [x2, y2] = vp.convertToViewportPoint(bbox[2], bbox[3]) as [number, number];
      el.style.left = `${Math.min(x1, x2)}px`;
      el.style.top = `${Math.min(y1, y2)}px`;
      el.style.width = `${Math.abs(x2 - x1)}px`;
      el.style.height = `${Math.abs(y2 - y1)}px`;
    };
    for (const b of this.overlayOn ? (this.segments.get(slot.n) ?? []) : []) {
      const shown = shownText(b);
      if (shown.untranslated) continue;
      const el = document.createElement("div");
      el.className = "ov-seg";
      el.dataset["sid"] = b.id;
      el.textContent = shown.text;
      place(el, b.bbox);
      // 點一下暫看原文由閱讀畫面的點按處理（`peek`）：先等一下看是不是雙擊，雙擊是放大
      layer.appendChild(el);
      // 字級從原文的字級開始，放不下就一點一點縮小，直到放得下為止
      let px = Math.max(5, (b.size ?? 10) * vp.scale);
      el.style.fontSize = `${px}px`;
      for (let i = 0; i < 12 && el.scrollHeight > el.clientHeight + 1 && px > 5; i++) {
        px *= 0.93;
        el.style.fontSize = `${px}px`;
      }
    }
    if (this.focus && this.focus.page + 1 === slot.n) {
      const el = document.createElement("div");
      el.className = "seg-focus";
      place(el, this.focus.bbox);
      layer.appendChild(el);
      // 框出的時間從真的畫出來才算：跳到遠處的頁時，那一頁要等一陣子才畫好
      if (!this.focusShown) {
        this.focusShown = true;
        this.startFocusTimer();
      }
    }
  }

  /** 框出的那一處 1.6 秒後拿掉。 */
  private startFocusTimer() {
    window.clearTimeout(this.focusTimer);
    this.focusTimer = window.setTimeout(() => {
      const was = this.focus;
      this.focus = null;
      const at = was ? this.slots[was.page] : undefined;
      if (at?.rendered) this.drawOverlay(at);
    }, 1600);
  }

  /** 畫面上一點（`clientX`、`clientY`）落在哪一段；不在任何一段上交回 `null`。 */
  segmentAt(clientX: number, clientY: number): SegmentBox | null {
    const hit = this.pageAt(clientX, clientY);
    if (!hit) return null;
    return segmentAt(this.segments.get(hit.n) ?? [], hit.n - 1, hit.x, hit.y);
  }

  private peekTimers = new Map<string, number>();

  /** 原頁疊譯文上的一段：切換「暫看原文」（再點一下，或六秒後，蓋回去）。 */
  peek(id: string) {
    const el = this.o.wrap.querySelector<HTMLElement>(`.ov-seg[data-sid="${CSS.escape(id)}"]`);
    if (!el) return;
    window.clearTimeout(this.peekTimers.get(id));
    el.classList.toggle("peek");
    if (el.classList.contains("peek")) {
      this.peekTimers.set(
        id,
        window.setTimeout(() => el.classList.remove("peek"), 6000),
      );
    }
  }

  /** 一段在畫面上的位置（那一頁畫好了才有）。 */
  segmentRect(page: number, bbox: SegmentBox["bbox"]): { left: number; top: number; right: number; bottom: number } | null {
    const slot = this.slots[page];
    const vp = slot?.viewport;
    if (!slot || !vp) return null;
    const r = slot.box.getBoundingClientRect();
    const [x1, y1] = vp.convertToViewportPoint(bbox[0], bbox[1]) as [number, number];
    const [x2, y2] = vp.convertToViewportPoint(bbox[2], bbox[3]) as [number, number];
    return {
      left: r.left + Math.min(x1, x2),
      right: r.left + Math.max(x1, x2),
      top: r.top + Math.min(y1, y2),
      bottom: r.top + Math.max(y1, y2),
    };
  }

  /**
   * 捲到一段：那一段的上緣放在畫面上方約四分之一處；放大著時左右也捲到那一段的左緣。縮放不變。
   * 然後把那一段框出來一下，指出是哪裡。
   */
  showSegment(page: number, bbox: SegmentBox["bbox"]) {
    const sc = this.o.scroll;
    const slot = this.slots[clamp(page, 0, Math.max(0, this.slots.length - 1))];
    if (!slot) return;
    const s = this.scale();
    // 方框的上緣與左緣換成從頁面左上角起算的像素（頁面還沒畫好時，用頁面的大小換算）
    const vp = slot.viewport;
    const [left, top] = vp
      ? (vp.convertToViewportPoint(Math.min(bbox[0], bbox[2]), Math.max(bbox[1], bbox[3])) as [number, number])
      : [Math.min(bbox[0], bbox[2]) * s, (this.pageH - Math.max(bbox[1], bbox[3])) * s];
    sc.scrollTop = slot.box.offsetTop + top - sc.clientHeight / 4;
    if (sc.scrollWidth - sc.clientWidth > 1) {
      const padLeft = parseFloat(getComputedStyle(sc).paddingLeft || "0");
      sc.scrollLeft = slot.box.offsetLeft + left - padLeft - 8;
    }
    this.reportPage();
    this.o.onMove();
    window.clearTimeout(this.focusTimer);
    this.focus = { page: slot.n - 1, bbox };
    this.focusShown = false;
    // 那一頁已經畫好就馬上畫出框（計時從那時起）；還沒畫好的，畫好時（`drawOverlay`）才畫出、才開始計時
    if (slot.rendered) this.drawOverlay(slot);
  }

  /**
   * 到原文那一頁、框出那一頁文字（`text.json`）裡 `range` 那一段（首頁搜尋的「內文」）。方框取自那一頁
   * 的文字片段（PDF 座標，不看網頁的文字層）；對不上也找不到 `quote` 時只到那一頁。`page` 從 0 起。
   */
  async showRange(page: number, range: [number, number], quote: string): Promise<boolean> {
    const input = await this.pageInput(page + 1).catch(() => null);
    if (this.destroyed) return false;
    const box = input ? rangeBox(input.items, range, quote) : null;
    if (!box) {
      this.goToPage(page + 1);
      return false;
    }
    this.showSegment(page, box);
    return true;
  }

  /**
   * 正在讀的位置：畫面正中那一點落在哪一頁的哪裡（頁序從 0 起、PDF 頁面座標）。
   * 放大讀一欄時，正中就在那一欄裡，所以拿來找「正在讀的那一段」不會跳到另一欄。
   */
  readingPoint(): { page: number; x: number; y: number } | null {
    const sc = this.o.scroll;
    if (!sc.isConnected || this.slots.length === 0) return null;
    const r = sc.getBoundingClientRect();
    const cx = r.left + sc.clientWidth / 2;
    const cy = r.top + sc.clientHeight / 2;
    const hit = this.pageAt(cx, cy);
    if (hit) return { page: hit.n - 1, x: hit.x, y: hit.y };
    // 正中落在兩頁之間的空隙（或那一頁還沒畫好）：用切換位置的頁與高度換算
    const p = this.getSwitchPosition();
    return { page: p.page - 1, x: this.pageW / 2, y: this.pageH * (1 - p.mid) };
  }

  // ---------- 劃線的疊層 ----------

  private highlights = new Map<number, DrawnHighlight[]>();

  /** 換上這一批要畫的劃線（整批取代）。 */
  setHighlights(list: DrawnHighlight[]) {
    this.highlights = new Map();
    for (const h of list) {
      const arr = this.highlights.get(h.n) ?? [];
      arr.push(h);
      this.highlights.set(h.n, arr);
    }
    for (const slot of this.slots) if (slot.rendered) this.drawHighlights(slot);
  }

  private drawHighlights(slot: PageSlot) {
    const layer = slot.hlLayer;
    const vp = slot.viewport;
    if (!layer || !vp) return;
    layer.replaceChildren();
    for (const h of this.highlights.get(slot.n) ?? []) {
      for (const r of h.rects) {
        const [x1, y1] = vp.convertToViewportPoint(r[0], r[1]) as [number, number];
        const [x2, y2] = vp.convertToViewportPoint(r[2], r[3]) as [number, number];
        const el = document.createElement("div");
        el.className = h.hasComment ? "hl has-comment" : "hl";
        el.dataset["hid"] = h.id;
        el.style.left = `${Math.min(x1, x2)}px`;
        el.style.top = `${Math.min(y1, y2)}px`;
        el.style.width = `${Math.abs(x2 - x1)}px`;
        el.style.height = `${Math.abs(y2 - y1)}px`;
        layer.appendChild(el);
      }
    }
  }

  /** 讓一條劃線閃一下（從筆記抽屜或連結跳過來時指出是哪一條）。 */
  flash(id: string, tries = 15) {
    const els = this.o.wrap.querySelectorAll<HTMLElement>(`.hl[data-hid="${CSS.escape(id)}"]`);
    // 剛跳到的頁還沒畫好劃線：等一下再試（最多約 3 秒）
    if (els.length === 0 && tries > 0 && !this.destroyed) {
      window.setTimeout(() => this.flash(id, tries - 1), 200);
      return;
    }
    for (const el of els) {
      el.classList.add("flash");
      window.setTimeout(() => el.classList.remove("flash"), 1800);
    }
  }

  // ---------- 頁面上的可點區域（外掛的接入點，`pdfRegions.ts`） ----------

  private readonly regions = new RegionLayer((n) => this.pageInput(n));

  /** 換上（或拿掉）可點區域的來源；已經畫好的頁重新要。 */
  setRegionSource(source: RegionSource | null) {
    this.regions.set(
      source,
      this.slots.filter((s) => s.rendered).map((s) => s.n),
    );
  }

  /** 畫面上一點落在哪一個可點區域，連同它在畫面上的位置（給小框對齊）。 */
  regionAt(clientX: number, clientY: number): { region: PageRegion; rect: { left: number; top: number; right: number; bottom: number } } | null {
    const hit = this.pageAt(clientX, clientY);
    if (!hit) return null;
    const region = this.regions.at(hit.n, hit.x, hit.y);
    const rect = region ? this.segmentRect(hit.n - 1, region.bbox) : null;
    return region && rect ? { region, rect } : null;
  }

  /** 畫面上某一點落在哪一頁、哪條劃線上（點劃線要開短評用）。 */
  highlightAt(clientX: number, clientY: number): string | null {
    const hit = this.pageAt(clientX, clientY);
    if (!hit) return null;
    const tol = 3; // 容許幾點的誤差，手指點不準
    for (const h of this.highlights.get(hit.n) ?? []) {
      for (const [l, b, r, t] of h.rects) {
        if (hit.x >= l - tol && hit.x <= r + tol && hit.y >= b - tol && hit.y <= t + tol) return h.id;
      }
    }
    return null;
  }

  private pageAt(clientX: number, clientY: number): { n: number; x: number; y: number } | null {
    for (const slot of this.slots) {
      const vp = slot.viewport;
      if (!slot.rendered || !vp) continue;
      const r = slot.box.getBoundingClientRect();
      if (clientX < r.left || clientX > r.right || clientY < r.top || clientY > r.bottom) continue;
      const [x, y] = vp.convertToPdfPoint(clientX - r.left, clientY - r.top);
      return { n: slot.n, x: x ?? 0, y: y ?? 0 };
    }
    return null;
  }

  // ---------- 選取 → 劃線（`pdfSelection.ts`） ----------

  /**
   * 目前選取的文字（瀏覽器原生的選取）換算成劃線要存的樣子。只取起點那一頁的部分；沒有選取、
   * 選取不在文字層裡就交回 `null`。
   */
  captureSelection(): SelectionCapture | null {
    return captureSelection((n) => this.slots[n - 1]);
  }

  /** 目前畫面裡第一行看得到的文字，當成「這一頁的短評」的依據（沒有選字時用）。 */
  firstVisibleLine(): SelectionCapture | null {
    return firstVisibleLine(this.o.scroll, this.slots[this.getPosition().page - 1], () => this.captureSelection());
  }

  // ---------- 筆刷（`pdfInk.ts`） ----------

  /** 開（傳筆刷的樣子）或關（傳 `null`）筆刷。 */
  setBrush(style: BrushStyle | null) {
    this.inkLayer.setBrush(style);
  }

  /** 開關橡皮擦：開著時點到哪一筆就刪那一筆（畫筆暫時不畫）。 */
  setEraser(on: boolean) {
    this.inkLayer.setEraser(on);
  }

  /** 換上這一批已存下來的筆跡（整批取代）。 */
  setInk(list: InkStroke[]) {
    this.inkLayer.setInk(
      list,
      this.slots.filter((s) => s.rendered),
    );
  }

  // ---------- 事件：捲動、縮放、視窗大小 ----------

  private reportPage() {
    const p = this.getPosition().page;
    if (p !== this.lastPage) {
      this.lastPage = p;
      this.o.onPage(p);
    }
  }

  private attach() {
    const sc = this.o.scroll;
    let raf = 0;
    const onScroll = () => {
      if (raf) return;
      raf = requestAnimationFrame(() => {
        raf = 0;
        this.reportPage();
        this.o.onMove();
      });
    };
    sc.addEventListener("scroll", onScroll, { passive: true });
    this.cleanups.push(() => {
      sc.removeEventListener("scroll", onScroll);
      if (raf) cancelAnimationFrame(raf);
    });

    this.cleanups.push(
      attachPinch(sc, this.o.wrap, () => this.zoom, { min: MIN_ZOOM, max: MAX_ZOOM }, (z, focus) =>
        this.applyZoom(z, focus, null),
      ),
    );

    // 旋轉或視窗大小改變：重新排版，回到同一個位置
    this.resizeObserver = new ResizeObserver(() => {
      const w = this.contentWidth();
      if (Math.abs(w - this.lastWidth) < 1 || !this.pdf) return;
      this.lastWidth = w;
      const pos = this.getPosition();
      this.layout();
      this.scrollToPosition(pos.page, pos.offset);
    });
    this.resizeObserver.observe(sc);
  }

  /**
   * 換成新的縮放，重新排版後捲到對應的位置。所有尺寸等比例放大，所以焦點（手指下那一點）在新版面裡是
   * (px, py) 乘上倍率：垂直方向焦點留在畫面上原本的高度；水平方向沒給 `alignLeft` 時焦點也留在原處，
   * 給了就把第 `n` 頁的那個位置（從頁面左緣起算，單位是 PDF 的點）對齊畫面左緣。
   */
  private applyZoom(
    zoom: number,
    focus: { z0: number; fx: number; fy: number; px: number; py: number },
    alignLeft: { n: number; x: number } | null,
  ) {
    const sc = this.o.scroll;
    const wrap = this.o.wrap;
    this.zoom = clamp(zoom, MIN_ZOOM, MAX_ZOOM);
    this.layout();
    const r = this.zoom / focus.z0;
    sc.scrollTop = wrap.offsetTop + focus.py * r - focus.fy;
    const slot = alignLeft ? this.slots[alignLeft.n - 1] : undefined;
    if (alignLeft && slot) {
      const padLeft = parseFloat(getComputedStyle(sc).paddingLeft || "0");
      sc.scrollLeft = slot.box.offsetLeft + alignLeft.x * this.scale() - padLeft;
    } else {
      sc.scrollLeft = wrap.offsetLeft + focus.px * r - focus.fx;
    }
    this.reportPage();
    this.o.onMove();
  }

  /** 雙擊放大之前的縮放（再雙擊回到它）；沒有雙擊放大過是 `null`。 */
  private zoomBefore: number | null = null;
  /** 每頁的文字（雙擊放大到欄寬、可點區域用，讀過就留著）。 */
  private pageText = new Map<number, PageInput>();

  private async pageInput(n: number): Promise<PageInput | null> {
    const cached = this.pageText.get(n);
    if (cached) return cached;
    const pdf = this.pdf;
    if (!pdf) return null;
    const page = await pdf.getPage(n);
    try {
      const content = await page.getTextContent({ includeMarkedContent: true });
      // 產生的譯本 PDF：被蓋住的原文不算，只看看得到的譯文（和文字層一樣）
      const cover = this.covers?.pages[String(n - 1)];
      const items = (cover ? filterChunk(content as unknown as ChunkLike, cover, []).items : content.items) as TextItemLike[];
      // 文字片段的轉換和逐篇處理、翻譯共用一份（`extract.ts` 的 `toRawItems`）
      const input: PageInput = { view: page.view as PageInput["view"], items: toRawItems({ items }) };
      this.pageText.set(n, input);
      return input;
    } finally {
      page.cleanup();
    }
  }

  /**
   * 雙擊（畫面座標）：放大到點的那一欄的寬度，點的那一行留在畫面上原本的高度、那一欄的左緣對齊畫面左緣；
   * 已經放大了（比雙擊之前大）就縮回雙擊之前的大小。欄的範圍用這一頁的文字算（`layout.ts` 的 `columnAt`：
   * 雙欄找中縫、跨欄的區塊與單欄取文字區）；算不出來（沒有文字、頁面轉了方向）時放大到頁寬的兩倍。
   */
  async toggleColumnZoom(clientX: number, clientY: number): Promise<void> {
    const sc = this.o.scroll;
    const sr = sc.getBoundingClientRect();
    const wr = this.o.wrap.getBoundingClientRect();
    const focus = { z0: this.zoom, fx: clientX - sr.left, fy: clientY - sr.top, px: clientX - wr.left, py: clientY - wr.top };
    const base = this.zoomBefore ?? MIN_ZOOM;
    if (this.zoom > base + 0.05) {
      this.zoomBefore = null;
      this.applyZoom(base, focus, null);
      return;
    }
    const hit = this.pageAt(clientX, clientY);
    const slot = hit ? this.slots[hit.n - 1] : undefined;
    const vp = slot?.viewport ?? null;
    let col: ColumnSpan | null = null;
    if (hit && vp && vp.rotation % 360 === 0) {
      col = await this.pageInput(hit.n)
        .then((page) => (page ? columnAt(page, hit.x, hit.y) : null))
        .catch(() => null);
      if (this.destroyed || this.zoom !== focus.z0) return;
    }
    let target = FALLBACK_ZOOM;
    let alignLeft: { n: number; x: number } | null = null;
    if (hit && vp && col) {
      // 欄的左右緣換成從頁面左緣起算的點（PDF 的單位），兩側各留一點邊；新的倍率讓這一段剛好占滿畫面寬
      const s = this.scale();
      const [ax = 0] = vp.convertToViewportPoint(col.x0, hit.y);
      const [bx = 0] = vp.convertToViewportPoint(col.x1, hit.y);
      const left = Math.min(ax, bx) / s;
      const width = Math.abs(bx - ax) / s;
      const margin = Math.max(4, 0.02 * width);
      const z = this.contentWidth() / (width + 2 * margin) / this.fit();
      if (z > this.zoom * 1.1) {
        target = z;
        alignLeft = { n: hit.n, x: left - margin };
      }
    }
    this.zoomBefore = this.zoom;
    this.applyZoom(target, focus, alignLeft);
  }

  destroy() {
    this.destroyed = true;
    this.regions.set(null, []);
    for (const c of this.cleanups) c();
    this.cleanups = [];
    this.observer?.disconnect();
    this.resizeObserver?.disconnect();
    this.transport?.destroy();
    for (const slot of this.slots) this.release(slot);
    this.slots = [];
    void this.pdf?.loadingTask.destroy();
    this.pdf = null;
  }
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

