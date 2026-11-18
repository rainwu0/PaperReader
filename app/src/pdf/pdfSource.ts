// 打開 PDF 的共用部分：以指令分段唯讀取得位元組，交給 PDF.js。
// 開著讀的閱讀畫面（`screens/reader/pdfView.ts`）用 `startPdf`；開一下做完就關的（讀出篇名與作者、首頁封面、
// 產生時分段）用 `withPdf`；要整份位元組的（匯出帶標註的副本、產生譯本 PDF）用 `readWhole`、`openBytes`。
// PDF.js 用相容版（legacy：預設版在舊的系統網頁元件上開不了檔）。

import * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs";
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";
import { api } from "../api";
import { filterChunk, type ChunkLike, type Rect } from "../translate/textFilter";

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

export const CHUNK = 262_144; // 分段讀取的大小
const CONCURRENCY = 6;
/**
 * 外殼一次最多交出這麼多位元組（`src-tauri/src/commands/reading.rs` 的 `MAX_RANGE`）。PDF.js 會把相鄰、還沒讀的區塊
 * 併成一次要（一張 8 MB 的圖就是一次要 8 MB 以上），超過的拆成幾段讀再接起來。是 `CHUNK` 的倍數，每一段都從區塊的邊界開始。
 */
export const MAX_READ = 16 * CHUNK;
/** 一次讀取失敗之後隔多久重試一次（只重試一次）。 */
const RETRY_MS = 400;

/** 讀原文的 [begin, end)：超過一次的上限就拆成幾段依序讀，接成一塊。檔案比預期短時讀到檔尾為止。 */
export async function readRange(paperId: string, materialId: string, begin: number, end: number): Promise<Uint8Array> {
  if (end - begin <= MAX_READ) return new Uint8Array(await api.readMaterialRange(paperId, materialId, begin, end - begin));
  const out = new Uint8Array(end - begin);
  let got = 0;
  for (let at = begin; at < end; at += MAX_READ) {
    const part = new Uint8Array(await api.readMaterialRange(paperId, materialId, at, Math.min(MAX_READ, end - at)));
    out.set(part, at - begin);
    got += part.length;
    if (part.length < Math.min(MAX_READ, end - at)) break;
  }
  return got === out.length ? out : out.subarray(0, got);
}

/**
 * 以指令分段讀原文的傳輸層，同時最多 `CONCURRENCY` 個請求。讀不出來（重試一次之後）時不能讓 PDF.js 一直等：
 * 記下錯誤、通知訂閱的人（`onFailure`），`failure` 也會以那個錯誤結束，等著的人可以一起等它。
 */
export class MaterialRangeTransport extends pdfjs.PDFDataRangeTransport {
  private active = 0;
  private readonly queue: Array<[number, number]> = [];
  private dead = false;
  private error: unknown = null;
  private readonly listeners = new Set<(e: unknown) => void>();
  private rejectFailure: (e: unknown) => void = () => {};
  /** 讀取失敗時以那個錯誤結束（不會成功結束）。和 PDF.js 的工作一起等，讀不出來就不會卡住。 */
  readonly failure: Promise<never>;

  constructor(
    private readonly paperId: string,
    private readonly materialId: string,
    length: number,
    initial: Uint8Array,
  ) {
    super(length, initial);
    this.failure = new Promise<never>((_, reject) => {
      this.rejectFailure = reject;
    });
    // 沒有人等的時候不要變成「未處理的拒絕」
    this.failure.catch(() => {});
  }

  /** 每次讀不出來時通知（訂閱時已經失敗過就馬上通知一次）。交回取消訂閱的函式。 */
  onFailure(cb: (e: unknown) => void): () => void {
    if (this.error !== null) cb(this.error);
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }

  destroy() {
    this.dead = true;
    this.queue.length = 0;
    this.listeners.clear();
  }

  override requestDataRange(begin: number, end: number) {
    this.queue.push([begin, end]);
    this.pump();
  }

  private async read(begin: number, end: number): Promise<Uint8Array> {
    try {
      return await readRange(this.paperId, this.materialId, begin, end);
    } catch {
      await new Promise((r) => setTimeout(r, RETRY_MS));
      return readRange(this.paperId, this.materialId, begin, end);
    }
  }

  /** 每一次讀不出來都通知（後來要的頁也讀不出來時，畫面可以再說一次）；`failure` 只在第一次結束。 */
  private fail(e: unknown) {
    if (this.dead) return;
    const err = e ?? new Error("read failed");
    if (this.error === null) {
      this.error = err;
      this.rejectFailure(err);
    }
    for (const cb of this.listeners) cb(err);
  }

  private pump() {
    while (!this.dead && this.active < CONCURRENCY) {
      const next = this.queue.shift();
      if (!next) return;
      const [begin, end] = next;
      this.active++;
      this.read(begin, end)
        .then((buf) => {
          if (!this.dead) this.onDataRange(begin, buf);
        })
        .catch((e: unknown) => this.fail(e))
        .finally(() => {
          this.active--;
          this.pump();
        });
    }
  }
}

/**
 * 文字層：頁面上透明、可以選取的文字（劃線要用瀏覽器原生的選取）。
 * `covers` 是產生的譯本 PDF 在這一頁蓋住的範圍：被蓋掉的原文不放進文字層，選字只選得到譯文（`translate/buildPdf.ts`）。
 */
export function makeTextLayer(
  page: pdfjs.PDFPageProxy,
  container: HTMLDivElement,
  viewport: pdfjs.PageViewport,
  covers?: Rect[],
): pdfjs.TextLayer {
  pdfjs.setLayerDimensions(container, viewport);
  const raw = page.streamTextContent(covers ? { includeMarkedContent: true } : {});
  const stack: boolean[] = [];
  const source = covers
    ? raw.pipeThrough(
        new TransformStream({
          transform(chunk, controller) {
            controller.enqueue(filterChunk(chunk as unknown as ChunkLike, covers, stack));
          },
        }),
      )
    : raw;
  return new pdfjs.TextLayer({
    textContentSource: source as never,
    container,
    viewport,
  });
}

export interface PdfSource {
  task: pdfjs.PDFDocumentLoadingTask;
  transport: MaterialRangeTransport;
  /** 打開的文件：PDF.js 的 `task.promise`，但讀取失敗時以那個錯誤結束（不會一直等）。 */
  opened: Promise<pdfjs.PDFDocumentProxy>;
}

/**
 * 等一件要讀原文的事（取一頁、取文字），讀取失敗時不再等、丟出那個錯誤；`cancelled()` 變成真（每 200 毫秒看一次）時
 * 也不再等，交回 `null`。PDF.js 的工作本身由 `closePdf` 收掉。
 */
export async function unlessStopped<T>(
  work: Promise<T>,
  src: Pick<PdfSource, "transport">,
  cancelled: () => boolean = () => false,
): Promise<T | null> {
  let timer = 0;
  const stopped = new Promise<null>((resolve) => {
    const check = () => {
      if (cancelled()) resolve(null);
      else timer = window.setTimeout(check, 200);
    };
    check();
  });
  try {
    return await Promise.race([work, src.transport.failure, stopped]);
  } finally {
    window.clearTimeout(timer);
  }
}

/** 開始載入一份原文（先讀開頭一段，其餘由 PDF.js 需要時再要）。呼叫的人要等 `opened`，用完呼叫 `closePdf`。 */
export async function startPdf(paperId: string, materialId: string, size: number): Promise<PdfSource> {
  const head = new Uint8Array(await api.readMaterialRange(paperId, materialId, 0, Math.min(size, CHUNK)));
  const transport = new MaterialRangeTransport(paperId, materialId, size, head);
  const task = pdfjs.getDocument({
    range: transport,
    rangeChunkSize: CHUNK,
    disableAutoFetch: true,
    disableStream: true,
    ...assets(),
  });
  const opened = Promise.race([task.promise, transport.failure]);
  opened.catch(() => {}); // 呼叫的人會等它；沒等到的不要變成未處理的拒絕
  return { task, transport, opened };
}

/** PDF.js 要的字型、字碼表、解碼器等檔案（`scripts/copy-pdfjs-assets.mjs` 複製到應用裡）。 */
function assets() {
  const base = import.meta.env.BASE_URL;
  return {
    standardFontDataUrl: `${base}pdfjs/standard_fonts/`,
    cMapUrl: `${base}pdfjs/cmaps/`,
    cMapPacked: true,
    wasmUrl: `${base}pdfjs/wasm/`,
    iccUrl: `${base}pdfjs/iccs/`,
  };
}

/** 從已經在記憶體裡的位元組打開（產生譯本 PDF 時看原文的圖形）。PDF.js 會把位元組搬走，要留著用的給一份複本。用完 `task.destroy()`。 */
export function openBytes(data: Uint8Array): pdfjs.PDFDocumentLoadingTask {
  return pdfjs.getDocument({ data, ...assets() });
}

/** 收掉載入工作與傳輸層（停止還在排隊的讀取）。 */
export async function closePdf(src: PdfSource): Promise<void> {
  src.transport.destroy();
  await src.task.destroy();
}

type PdfWork<T> = (pdf: pdfjs.PDFDocumentProxy, src: PdfSource) => Promise<T>;

/**
 * 打開一份材料、等文件打開、做一件事，做完（成功或失敗）都收掉。`work` 拿得到傳輸層（`src`），
 * 一頁一頁讀時可以用 `unlessStopped` 等。給了 `cancelled`：開檔途中它變成真就不做，交回 `null`。
 */
export function withPdf<T>(paperId: string, materialId: string, work: PdfWork<T>, cancelled: () => boolean): Promise<T | null>;
export function withPdf<T>(paperId: string, materialId: string, work: PdfWork<T>): Promise<T>;
export async function withPdf<T>(
  paperId: string,
  materialId: string,
  work: PdfWork<T>,
  cancelled?: () => boolean,
): Promise<T | null> {
  const size = await api.materialSize(paperId, materialId);
  const src = await startPdf(paperId, materialId, size);
  try {
    const pdf = cancelled ? await unlessStopped(src.opened, src, cancelled) : await src.opened;
    return pdf ? await work(pdf, src) : null;
  } finally {
    await closePdf(src).catch(() => {});
  }
}

/** 整份材料的位元組（照外殼一次的上限分段讀，`readRange`）：匯出帶標註的副本、產生譯本 PDF 用。 */
export async function readWhole(paperId: string, materialId: string): Promise<Uint8Array> {
  const size = await api.materialSize(paperId, materialId);
  return readRange(paperId, materialId, 0, size);
}
