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
import { columnAt, layoutPages, type ColumnSpan, type PageInput } from "../../translate/layout";
import { t as say } from "../../strings";
import {
  cardWidth,
  markerRoom,
  placeCards,
  placeMarkers,
  quoteHead,
  sideOf,
  type CardOptions,
  type CommentAnchor,
} from "./commentLayout";
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
/**
 * 一頁畫布最多幾個像素（超過就降低清晰度）。原本 1,200 萬（每頁約 48 MB），放大後上下預先畫的幾頁加起來很重；
 * PDF.js 官方檢視器在手機上限 524 萬（`web/app_options.js` 的 `maxCanvasPixels`，再用局部的細節畫布補清晰度）。
 * 這裡沒有局部畫布，取 800 萬：放大到 2.5 倍仍接近螢幕的像素比，4 倍時略降。
 */
const MAX_CANVAS_PIXELS = 8_000_000;
/** 畫面上下各預先畫多遠（畫面高度的比例）。原本 1.2，縮放 1 倍時一次畫 8 到 10 頁；PDF.js 只多畫前後一頁。 */
const PRELOAD = 0.6;
/** 雙擊放大與縮回的過場（毫秒）；系統開了「移除動畫」時直接換。 */
const ZOOM_ANIM_MS = 220;
/** 雙擊放大算不出欄時的縮放：頁寬的兩倍（縮放 1 倍是頁寬剛好填滿畫面）。 */
const FALLBACK_ZOOM = 2;
/** 短評顯示層：頁緣標記的邊長與離頁緣多遠、卡片的寬與間距（CSS 像素，不隨縮放變）。 */
const MARK_SIZE = 22;
const MARK_INSET = 3;
/** 標記和那一欄文字之間的間距。 */
const MARK_GAP = 3;
const CARD: CardOptions = { width: 280, inset: 6, gap: 6 };

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
  /**
   * 畫好的這一頁的座標換算（頁面座標 ⇄ 畫好時的畫面像素）。換了縮放、新的比例還沒畫好時（`k` 不是 1），
   * 頁框已經是新的大小，裡面幾層暫時用 CSS 放大 `k` 倍撐著（`.pg.stale`）：畫面上的位置是這裡換出來的乘上 `k`。
   */
  viewport: pdfjs.PageViewport | null;
  /** 頁框的大小是畫好時的幾倍（1 是不必重畫）。 */
  k: number;
  /** 文字層（PDF.js 的物件）：換縮放時更新比例就好，不重新向 worker 要文字、不重建。 */
  textLayerObj: pdfjs.TextLayer | null;
  hlLayer: HTMLDivElement | null;
  textLayer: HTMLDivElement | null;
  strokeCanvas: HTMLCanvasElement | null;
  /** 譯文疊加：把譯文畫在原段落的位置上。 */
  ovLayer: HTMLDivElement | null;
  /** 短評顯示層：頁緣的標記與展開的卡片（在最上面，接得到點按）。 */
  cmLayer: HTMLDivElement | null;
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
  /** 短評與引句（短評顯示層的卡片用）。 */
  comment?: string;
  quote?: string;
}

/** 短評顯示層開著沒有、全部展開沒有（`setCommentDisplay`）。 */
export interface CommentDisplay {
  show: boolean;
  openAll: boolean;
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
  /** 短評卡片上的「編輯」「刪除」（`id` 是劃線的代號）。 */
  onCommentEdit?: (id: string) => void;
  onCommentDelete?: (id: string) => void;
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
  /** 正在畫的那一頁（換縮放時取消：它的比例已經不對了）。 */
  private renderTask: { slot: PageSlot; task: pdfjs.RenderTask } | null = null;
  /** 雙指縮放或雙擊放大的過場進行中：「看得到哪幾頁」的判斷暫停（內容在做 transform，量到的不是真的位置）。 */
  private gesturing = false;
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

  /** 畫面上看得到（有一部分在畫面裡）的頁，頁序從 0 起（筆刷的「復原」只撤銷這幾頁上的動作）。 */
  visiblePages(): number[] {
    const sc = this.o.scroll;
    const top = sc.scrollTop;
    const bottom = top + sc.clientHeight;
    const out: number[] = [];
    for (const slot of this.slots) {
      const y0 = slot.box.offsetTop;
      if (y0 > bottom) break;
      if (y0 + slot.box.offsetHeight >= top) out.push(slot.n - 1);
    }
    return out;
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
    this.cancelRender();
    for (const slot of this.slots) this.release(slot);
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
        k: 1,
        textLayerObj: null,
        hlLayer: null,
        textLayer: null,
        strokeCanvas: null,
        ovLayer: null,
        cmLayer: null,
      });
    }
    wrap.appendChild(frag);
    this.observer = new IntersectionObserver(
      (entries) => {
        // 縮放的手勢或過場途中不理：內容在做 transform，結束時 `refreshPages` 照真的位置重新判斷一次
        if (this.gesturing) return;
        for (const e of entries) {
          const slot = this.slots[Number((e.target as HTMLElement).dataset["n"]) - 1];
          if (!slot) continue;
          if (e.isIntersecting) this.enqueue(slot.n);
          else this.release(slot);
        }
      },
      { root: this.o.scroll, rootMargin: `${PRELOAD * 100}% 0px` },
    );
    for (const slot of this.slots) this.observer.observe(slot.box);
  }

  /**
   * 換成新的縮放而不丟掉已經畫好的頁（PDF.js 檢視器的做法：舊畫布用 CSS 撐到新的大小，新的比例畫好再換上）。
   * 頁框照新的比例改大小；畫好的頁標成 `stale`，裡面幾層暫時放大 `k` 倍。正在畫的那一頁取消。
   * 要畫哪幾頁由呼叫的一方在捲到新位置之後呼叫 `refreshPages` 決定。
   */
  private rescale() {
    const { wrap } = this.o;
    if (!this.pdf) return;
    this.cancelRender();
    const s = this.scale();
    const pad = PAD * this.zoom;
    wrap.style.padding = `${pad}px`;
    wrap.style.width = `${this.pageW * s + 2 * pad}px`;
    const w = `${this.pageW * s}px`;
    const h = `${this.pageH * s}px`;
    const last = this.slots.length;
    for (const slot of this.slots) {
      const box = slot.box;
      box.style.width = w;
      box.style.height = h;
      box.style.marginBottom = `${slot.n === last ? 0 : pad}px`;
      if (!slot.rendered || !slot.viewport) continue;
      slot.k = s / slot.viewport.scale;
      const stale = Math.abs(slot.k - 1) > 1e-4;
      box.classList.toggle("stale", stale);
      if (stale) box.style.setProperty("--k", String(slot.k));
      else {
        slot.k = 1;
        box.style.removeProperty("--k");
      }
    }
  }

  /**
   * 照現在的捲動位置決定要畫哪幾頁：畫面裡與上下 `PRELOAD` 以內的頁，離畫面中間近的先畫（還沒畫的、比例不對的）；
   * 離得遠的放掉。縮放之後呼叫（手勢途中「看得到哪幾頁」的判斷暫停了）。
   */
  private refreshPages() {
    const sc = this.o.scroll;
    const margin = sc.clientHeight * PRELOAD;
    const top = sc.scrollTop - margin;
    const bottom = sc.scrollTop + sc.clientHeight + margin;
    const mid = sc.scrollTop + sc.clientHeight / 2;
    const near: Array<{ slot: PageSlot; d: number }> = [];
    for (const slot of this.slots) {
      const y0 = slot.box.offsetTop;
      const y1 = y0 + slot.box.offsetHeight;
      if (y1 >= top && y0 <= bottom) near.push({ slot, d: Math.abs((y0 + y1) / 2 - mid) });
      else this.release(slot);
    }
    near.sort((a, b) => a.d - b.d);
    this.queue = near.filter((x) => !x.slot.rendered || x.slot.k !== 1).map((x) => x.slot.n);
    void this.drain();
  }

  private cancelRender() {
    const t = this.renderTask;
    this.renderTask = null;
    t?.task.cancel();
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
      // 縮放的手勢或過場途中停下（結束時 `refreshPages` 重排隊伍、再叫一次）
      while (this.queue.length > 0 && !this.destroyed && !this.gesturing) {
        const n = this.queue.shift();
        const slot = n === undefined ? undefined : this.slots[n - 1];
        // 畫好而且比例對的不再畫；比例不對的（換了縮放，`k` 不是 1）重畫
        if (!slot || (slot.rendered && slot.k === 1)) continue;
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
    const scale = this.scale();
    const viewport = page.getViewport({ scale });
    let dpr = Math.min(window.devicePixelRatio || 1, 3); // 螢幕的像素比，上限 3；畫布太大時下面再降
    while (viewport.width * dpr * viewport.height * dpr > MAX_CANVAS_PIXELS && dpr > 0.5) dpr -= 0.25;
    const canvas = document.createElement("canvas");
    canvas.width = Math.floor(viewport.width * dpr);
    canvas.height = Math.floor(viewport.height * dpr);
    canvas.style.width = `${viewport.width}px`;
    canvas.style.height = `${viewport.height}px`;
    const task = page.render({
      canvas,
      viewport,
      ...(dpr !== 1 ? { transform: [dpr, 0, 0, dpr, 0, 0] } : {}),
    });
    this.renderTask = { slot, task };
    try {
      await task.promise;
    } catch (e) {
      // 換了縮放而取消（或畫不出來）：新畫布丟掉，舊的那一份留著撐到下一次畫好
      canvas.width = 0;
      canvas.height = 0;
      page.cleanup();
      throw e;
    } finally {
      if (this.renderTask?.task === task) this.renderTask = null;
    }
    // 畫的這段時間裡被放掉、換了縮放、或整個關掉：這一份不用了
    if (slot.token !== token || !slot.box.isConnected || this.destroyed || Math.abs(this.scale() - scale) > 1e-6) {
      canvas.width = 0;
      canvas.height = 0;
      page.cleanup();
      return;
    }
    const old = slot.box.querySelector<HTMLCanvasElement>(":scope > canvas");
    slot.box.style.width = `${viewport.width}px`;
    slot.box.style.height = `${viewport.height}px`;
    // PDF.js 的文字層用 --scale-factor 換算尺寸，沒有它文字會縮成八分之一、選不到（reader.css 的 .pg 有其餘的變數）
    slot.box.style.setProperty("--scale-factor", String(viewport.scale));
    // 疊在畫面上的幾層：劃線（在文字下面，不擋選取）、文字層（透明、可選取）、筆刷
    const hl = document.createElement("div");
    hl.className = "hl-layer";
    hl.style.setProperty("--hl-fill", HIGHLIGHT_COLOR);
    // 文字層已經有了（換縮放重畫）：沿用，只更新比例（PDF.js 文字層的位置用 --scale-factor 換算）
    const reuse = slot.textLayer && slot.textLayerObj ? { tl: slot.textLayer, obj: slot.textLayerObj } : null;
    const tl = reuse?.tl ?? document.createElement("div");
    if (!reuse) tl.className = "textLayer";
    const sc = this.inkLayer.makeCanvas(slot, viewport);
    const ov = document.createElement("div");
    ov.className = "ov-layer";
    slot.box.replaceChildren(canvas, ov, hl, tl, sc);
    slot.box.classList.remove("stale");
    slot.box.style.removeProperty("--k");
    slot.k = 1;
    // 換下來的舊畫布馬上交還記憶體（不等回收）
    if (old && old !== canvas) {
      old.width = 0;
      old.height = 0;
    }
    slot.viewport = viewport;
    slot.hlLayer = hl;
    slot.textLayer = tl;
    slot.strokeCanvas = sc;
    slot.ovLayer = ov;
    slot.cmLayer = null;
    slot.rendered = true;
    this.drawOverlay(slot);
    this.drawHighlights(slot);
    this.drawComments(slot);
    this.inkLayer.draw(slot);
    if (reuse) {
      try {
        reuse.obj.update({ viewport });
      } catch {
        // 更新不了就維持原樣（只影響選字的精準度）
      }
      page.cleanup();
      return;
    }
    // 文字層晚一點出來也沒關係：頁面圖先顯示
    const layer = makeTextLayer(page, tl, viewport, this.covers?.pages[String(slot.n - 1)]);
    try {
      await layer.render();
      if (slot.token === token && slot.textLayer === tl) slot.textLayerObj = layer;
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
    if (this.renderTask?.slot === slot) this.cancelRender();
    for (const c of slot.box.querySelectorAll("canvas")) {
      c.width = 0;
      c.height = 0;
    }
    slot.textLayerObj?.cancel();
    slot.textLayerObj = null;
    slot.box.replaceChildren();
    slot.box.classList.remove("stale");
    slot.box.style.removeProperty("--k");
    slot.k = 1;
    slot.rendered = false;
    this.regions.drop(slot.n);
    slot.viewport = null;
    slot.hlLayer = null;
    slot.textLayer = null;
    slot.strokeCanvas = null;
    slot.ovLayer = null;
    slot.cmLayer = null;
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
    const k = slot.k;
    const [x1, y1] = vp.convertToViewportPoint(bbox[0], bbox[1]) as [number, number];
    const [x2, y2] = vp.convertToViewportPoint(bbox[2], bbox[3]) as [number, number];
    return {
      left: r.left + Math.min(x1, x2) * k,
      right: r.left + Math.max(x1, x2) * k,
      top: r.top + Math.min(y1, y2) * k,
      bottom: r.top + Math.max(y1, y2) * k,
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
    // 換了縮放、新的比例還沒畫好時，畫好時的位置要再乘上 `k`
    const vpPoint = vp
      ? (vp.convertToViewportPoint(Math.min(bbox[0], bbox[2]), Math.max(bbox[1], bbox[3])) as [number, number])
      : null;
    const [left, top] = vpPoint
      ? [vpPoint[0] * slot.k, vpPoint[1] * slot.k]
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
    for (const slot of this.slots) {
      if (!slot.rendered) continue;
      this.drawHighlights(slot);
      this.drawComments(slot);
    }
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
    // 色彩增值只設在真的有劃線的層上：空的層也混合，每一頁都多一個要合成的混合層
    layer.classList.toggle("has-hl", layer.childElementCount > 0);
  }

  /** 一條劃線在畫面上占的範圍（點劃線出的小選單對齊它）；那一頁沒畫出來交回 `null`。 */
  highlightRect(id: string): { left: number; top: number; right: number; bottom: number } | null {
    const els = this.o.wrap.querySelectorAll<HTMLElement>(`.hl[data-hid="${CSS.escape(id)}"]`);
    let out: { left: number; top: number; right: number; bottom: number } | null = null;
    for (const el of els) {
      const r = el.getBoundingClientRect();
      out = out
        ? { left: Math.min(out.left, r.left), top: Math.min(out.top, r.top), right: Math.max(out.right, r.right), bottom: Math.max(out.bottom, r.bottom) }
        : { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
    }
    return out;
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

  // ---------- 短評顯示層：頁緣的標記、點了展開的卡片 ----------
  // 標記與卡片放在每一頁自己的一層（`cmLayer`）裡，和劃線一樣跟著頁面捲動與縮放，捲動時不必重算。
  // 只在這一頁畫好（`renderPage`）、換了劃線（`setHighlights`）、開關或展開收起時排一次；縮放的手勢途中不畫頁，
  // 所以也不重排。排法在 `commentLayout.ts`。

  private commentShow = false;
  private commentAll = false;
  /** 一張一張點開的（沒有全部展開時）、全部展開之後又收起的。 */
  private commentOpen = new Set<string>();
  private commentClosed = new Set<string>();

  /** 開關短評顯示層、全部展開或收起。全部展開或收起時，一張一張點的記錄清掉。 */
  setCommentDisplay(d: CommentDisplay) {
    if (d.openAll !== this.commentAll) {
      this.commentOpen.clear();
      this.commentClosed.clear();
    }
    if (d.show === this.commentShow && d.openAll === this.commentAll) return;
    this.commentShow = d.show;
    this.commentAll = d.openAll;
    for (const slot of this.slots) if (slot.rendered) this.drawComments(slot);
  }

  /** 收起所有展開的卡片（「全部收起」；全部展開的狀態另由 `setCommentDisplay` 關掉）。 */
  collapseComments() {
    this.commentOpen.clear();
    this.commentClosed.clear();
    for (const slot of this.slots) if (slot.rendered) this.drawComments(slot);
  }

  private commentIsOpen(id: string): boolean {
    return this.commentAll ? !this.commentClosed.has(id) : this.commentOpen.has(id);
  }

  private toggleComment(id: string, n: number) {
    const open = this.commentIsOpen(id);
    const set = this.commentAll ? this.commentClosed : this.commentOpen;
    if (open === this.commentAll) set.add(id);
    else set.delete(id);
    const slot = this.slots[n - 1];
    if (slot?.rendered) this.drawComments(slot);
  }

  private drawComments(slot: PageSlot) {
    const vp = slot.viewport;
    if (!slot.rendered || !vp) return;
    const list = this.commentShow
      ? (this.highlights.get(slot.n) ?? []).filter((h) => (h.comment ?? "").trim() !== "" && h.rects.length > 0)
      : [];
    // 這一層只在這一頁真的有標記時才放進頁面：沒有短評的頁、關掉顯示時不多一個疊放的元素（縮放途中每格都要合成）
    if (list.length === 0) {
      slot.cmLayer?.remove();
      slot.cmLayer = null;
      return;
    }
    let layer = slot.cmLayer;
    if (!layer || !layer.isConnected) {
      layer = document.createElement("div");
      layer.className = "cm-layer";
      slot.box.appendChild(layer);
      slot.cmLayer = layer;
    } else if (layer.childElementCount > 0) layer.replaceChildren();
    const pageW = vp.width;
    const pageH = vp.height;
    // 欄的範圍要這一頁的文字（和雙擊放大同一份，讀過就留著）：還沒讀過先貼頁緣，讀好再排一次
    const text = this.pageText.get(slot.n);
    if (!text) {
      const token = slot.token;
      void this.pageInput(slot.n)
        .then((input) => {
          if (input && !this.destroyed && slot.token === token && slot.rendered) this.drawComments(slot);
        })
        .catch(() => {});
    }
    const anchors: CommentAnchor[] = list.map((h) => {
      let first: { top: number; bottom: number; rect: Rect } | null = null;
      let left = Infinity;
      let right = -Infinity;
      for (const r of h.rects) {
        const [x1, y1] = vp.convertToViewportPoint(r[0], r[1]) as [number, number];
        const [x2, y2] = vp.convertToViewportPoint(r[2], r[3]) as [number, number];
        const top = Math.min(y1, y2);
        const bottom = Math.max(y1, y2);
        left = Math.min(left, x1, x2);
        right = Math.max(right, x1, x2);
        if (!first || top < first.top) first = { top, bottom, rect: r };
      }
      const a: CommentAnchor = { id: h.id, lineTop: first?.top ?? 0, lineBottom: first?.bottom ?? 0, left, right };
      const r = first?.rect;
      const col = text && r && vp.rotation % 360 === 0 ? this.columnOf(slot.n, text, (r[0] + r[2]) / 2, (r[1] + r[3]) / 2) : null;
      if (col && r) {
        const cy = (r[1] + r[3]) / 2;
        const [ax = 0] = vp.convertToViewportPoint(col.x0, cy);
        const [bx = 0] = vp.convertToViewportPoint(col.x1, cy);
        a.colLeft = Math.min(ax, bx);
        a.colRight = Math.max(ax, bx);
      }
      return a;
    });
    const byId = new Map(list.map((h) => [h.id, h]));
    const anchorOf = new Map(anchors.map((a) => [a.id, a]));
    const frag = document.createDocumentFragment();
    const marks = placeMarkers(anchors, pageW, MARK_SIZE, MARK_INSET, MARK_GAP);
    const markOf = new Map(marks.map((m) => [m.id, m]));
    for (const m of marks) {
      const h = byId.get(m.id);
      const el = document.createElement("button");
      el.type = "button";
      el.className = "cm-mark";
      el.style.left = `${m.x}px`;
      el.style.top = `${m.y}px`;
      el.setAttribute("aria-label", say("reader.comments.mark", { quote: quoteHead(h?.quote ?? "", 16) }));
      el.setAttribute("aria-expanded", String(this.commentIsOpen(m.id)));
      el.addEventListener("click", (e) => {
        e.stopPropagation();
        this.toggleComment(m.id, slot.n);
      });
      frag.appendChild(el);
    }
    // 展開的卡片：先放進去量高度，再排位置（一頁一次版面計算）
    const width = cardWidth(pageW, CARD);
    const cards: Array<{ id: string; el: HTMLElement }> = [];
    for (const h of list) {
      if (!this.commentIsOpen(h.id)) continue;
      const el = this.commentCard(h);
      el.style.width = `${width}px`;
      frag.appendChild(el);
      cards.push({ id: h.id, el });
    }
    layer.appendChild(frag);
    if (cards.length === 0) return;
    // 卡片緊靠在標記旁邊（不蓋住標記，點標記才收得起來）：放大讀一欄時頁緣在畫面外，卡片跟著標記靠在那一欄旁邊
    const inputs = cards.map((c) => {
      const a = anchorOf.get(c.id);
      const m = markOf.get(c.id);
      const side = a ? sideOf(a, pageW) : ("right" as const);
      return {
        id: c.id,
        side,
        top: (a?.lineBottom ?? 0) + 4,
        height: c.el.offsetHeight,
        ...(m ? { edge: side === "left" ? m.x + MARK_SIZE + MARK_GAP : m.x - MARK_GAP } : {}),
      };
    });
    const spots = new Map(placeCards(inputs, pageW, pageH, CARD).map((s) => [s.id, s]));
    for (const c of cards) {
      const s = spots.get(c.id);
      if (!s) continue;
      c.el.style.left = `${s.left}px`;
      c.el.style.top = `${s.top}px`;
    }
  }

  /** 每一頁、每個位置所在的欄（`columnAt` 每次都把整頁的行重排一次；同一條劃線每次排卡片都要問，記下來）。 */
  private columns = new Map<string, ColumnSpan | null>();
  private columnOf(n: number, text: PageInput, x: number, y: number): ColumnSpan | null {
    const key = `${n}:${Math.round(x)}:${Math.round(y)}`;
    if (!this.columns.has(key)) this.columns.set(key, columnAt(text, x, y));
    return this.columns.get(key) ?? null;
  }

  /** 這一頁有沒有顯示著的短評標記（雙擊放大時要不要在欄外留標記的位置）。 */
  private hasCommentMarks(n: number): boolean {
    return this.commentShow && (this.highlights.get(n) ?? []).some((h) => (h.comment ?? "").trim() !== "");
  }

  /** 一張評語卡片：引句的開頭幾個字、短評全文、「編輯」「刪除」。 */
  private commentCard(h: DrawnHighlight): HTMLElement {
    const card = document.createElement("div");
    card.className = "cm-card";
    card.dataset["cid"] = h.id;
    card.setAttribute("role", "note");
    // 點在卡片上不算點頁面（不收頂列、不開小框）
    card.addEventListener("click", (e) => e.stopPropagation());
    const quote = document.createElement("p");
    quote.className = "cm-quote";
    quote.textContent = `「${quoteHead(h.quote ?? "")}」`;
    const text = document.createElement("p");
    text.className = "cm-text";
    text.textContent = h.comment ?? "";
    const actions = document.createElement("div");
    actions.className = "cm-actions";
    const button = (label: string, run: () => void) => {
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = label;
      b.addEventListener("click", (e) => {
        e.stopPropagation();
        run();
      });
      return b;
    };
    actions.append(
      button(say("reader.comments.edit"), () => this.o.onCommentEdit?.(h.id)),
      button(say("reader.comments.delete"), () => this.o.onCommentDelete?.(h.id)),
    );
    card.append(quote, text, actions);
    return card;
  }

  // ---------- 整段寫短評：一段的範圍換成一次選取 ----------

  /**
   * 畫面上一點所在的那一段：有譯文疊加檔的段落（版面相同時）就用它，沒有就用這一頁的版面分析（`layout.ts`，
   * 和翻譯切段落同一套）。頁序從 0 起、PDF 頁面座標；找不到交回 `null`。
   */
  async paragraphAt(clientX: number, clientY: number): Promise<{ page: number; bbox: SegmentBox["bbox"] } | null> {
    const hit = this.pageAt(clientX, clientY);
    if (!hit) return null;
    const known = segmentAt(this.segments.get(hit.n) ?? [], hit.n - 1, hit.x, hit.y);
    if (known) return { page: known.page, bbox: known.bbox };
    const input = await this.pageInput(hit.n).catch(() => null);
    if (!input || this.destroyed) return null;
    const segs = layoutPages([input]).map((s, i) => ({
      id: String(i),
      page: hit.n - 1,
      bbox: s.bbox,
      kind: s.kind,
      source: s.source,
      target: null,
      status: "pending",
    }));
    const seg = segmentAt(segs, hit.n - 1, hit.x, hit.y, 6);
    return seg ? { page: seg.page, bbox: seg.bbox } : null;
  }

  /**
   * 選起一段（頁序從 0 起、PDF 頁面座標的方框）裡的文字，換算成劃線要存的樣子，再取消選取：和手選的劃線存的欄位相同。
   * 那一頁要畫好、而且是現在的比例（縮放後還沒重畫好時交回 `null`）。DOM 裡的順序夾進了別欄的行時，矩形只留方框裡的。
   */
  captureBox(page: number, bbox: SegmentBox["bbox"]): SelectionCapture | null {
    const slot = this.slots[page];
    const tl = slot?.textLayer;
    if (!slot?.rendered || !tl || slot.k !== 1) return null;
    const box = this.segmentRect(page, bbox);
    if (!box) return null;
    const tol = 2;
    const spans = Array.from(tl.querySelectorAll<HTMLElement>("span")).filter((s) => {
      if (!(s.firstChild instanceof Text) || (s.textContent ?? "").trim() === "") return false;
      const r = s.getBoundingClientRect();
      const cx = r.left + r.width / 2;
      const cy = r.top + r.height / 2;
      return cx >= box.left - tol && cx <= box.right + tol && cy >= box.top - tol && cy <= box.bottom + tol;
    });
    const first = spans[0]?.firstChild;
    const last = spans.at(-1)?.firstChild;
    if (!(first instanceof Text) || !(last instanceof Text)) return null;
    const range = document.createRange();
    range.setStart(first, 0);
    range.setEnd(last, last.length);
    const sel = window.getSelection();
    sel?.removeAllRanges();
    sel?.addRange(range);
    const cap = this.captureSelection();
    sel?.removeAllRanges();
    if (!cap) return null;
    const [bl, bb, br, bt] = [Math.min(bbox[0], bbox[2]), Math.min(bbox[1], bbox[3]), Math.max(bbox[0], bbox[2]), Math.max(bbox[1], bbox[3])];
    const inside = cap.rects.filter(([l, b, r, tp]) => l <= br + tol && r >= bl - tol && b <= bt + tol && tp >= bb - tol);
    return inside.length > 0 ? { ...cap, rects: inside } : cap;
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
      // 換了縮放、新的比例還沒畫好時，頁框比畫好時大 `k` 倍
      const [x, y] = vp.convertToPdfPoint((clientX - r.left) / slot.k, (clientY - r.top) / slot.k);
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
      attachPinch(
        sc,
        this.o.wrap,
        () => this.zoom,
        { min: MIN_ZOOM, max: MAX_ZOOM },
        () => {
          // 手勢途中：不判斷看得到哪幾頁（內容在做 transform），也不在主執行緒上畫頁
          this.gesturing = true;
          this.cancelRender();
        },
        (z, focus) => this.applyZoom(z, focus, null),
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
    animate = false,
  ) {
    const sc = this.o.scroll;
    const wrap = this.o.wrap;
    const next = clamp(zoom, MIN_ZOOM, MAX_ZOOM);
    const before = wrap.getBoundingClientRect();
    const z0 = this.zoom;
    // 兩指放上去又拿開、沒有真的縮放：什麼都不必做
    if (Math.abs(next - z0) < 1e-3 && !alignLeft) {
      this.gesturing = false;
      this.refreshPages();
      return;
    }
    this.zoom = next;
    // 不重建頁面：頁框改大小，畫好的頁先用 CSS 撐著（`rescale`），捲到新位置之後只重畫看得到的
    this.rescale();
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
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!animate || reduce || typeof wrap.animate !== "function") {
      this.gesturing = false;
      this.refreshPages();
      return;
    }
    // 雙擊的過場（FLIP）：版面已經是新的，先用 transform 把它擺回看起來和之前一樣，再放回原樣。
    // 只動 transform；過場途中不判斷看得到哪幾頁、不開始畫，結束才重畫（不和過場搶主執行緒）
    const after = wrap.getBoundingClientRect();
    const k = z0 / this.zoom;
    this.gesturing = true;
    wrap.style.willChange = "transform";
    const anim = wrap.animate(
      [
        { transformOrigin: "0 0", transform: `translate(${before.left - after.left}px, ${before.top - after.top}px) scale(${k})` },
        { transformOrigin: "0 0", transform: "none" },
      ],
      { duration: ZOOM_ANIM_MS, easing: "cubic-bezier(0.2, 0, 0, 1)" },
    );
    const done = () => {
      wrap.style.willChange = "";
      if (this.destroyed) return;
      this.gesturing = false;
      this.refreshPages();
    };
    anim.onfinish = done;
    anim.oncancel = done;
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
    // 上一次的過場還沒結束（版面在做 transform，量到的位置不準）：這一下不算
    if (this.gesturing) return;
    const sc = this.o.scroll;
    const sr = sc.getBoundingClientRect();
    const wr = this.o.wrap.getBoundingClientRect();
    const focus = { z0: this.zoom, fx: clientX - sr.left, fy: clientY - sr.top, px: clientX - wr.left, py: clientY - wr.top };
    const base = this.zoomBefore ?? MIN_ZOOM;
    if (this.zoom > base + 0.05) {
      this.zoomBefore = null;
      this.applyZoom(base, focus, null, true);
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
      // 換算用畫好時的比例（新的比例可能還沒畫好）
      const s = vp.scale;
      const [ax = 0] = vp.convertToViewportPoint(col.x0, hit.y);
      const [bx = 0] = vp.convertToViewportPoint(col.x1, hit.y);
      const left = Math.min(ax, bx) / s;
      const width = Math.abs(bx - ax) / s;
      // 這一頁有短評標記時，欄外多留標記的位置（標記貼著欄的外側，放大後才看得到）
      const marks = this.hasCommentMarks(hit.n) ? markerRoom(MARK_SIZE + 2 * MARK_GAP, width, this.contentWidth()) : 0;
      const margin = Math.max(4, 0.02 * width, marks);
      const z = this.contentWidth() / (width + 2 * margin) / this.fit();
      if (z > this.zoom * 1.1) {
        target = z;
        alignLeft = { n: hit.n, x: left - margin };
      }
    }
    this.zoomBefore = this.zoom;
    this.applyZoom(target, focus, alignLeft, true);
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

