// 文字層的空隙（行距、欄距、段落之間、短行的行尾）：拖選取的把手經過空白時，選取的一端不跳走、頁面不被捲動。
//
// PDF.js 的文字層是一個個絕對定位的字串，中間是空白。Chromium 148 之前，選取的一端落在空白上時，
// 瀏覽器把那一點換算成文件順序上遠處的位置（兩欄的論文常是另一欄同一高度的行，或頁首），選取一下子變成一大段；
// 那個位置在畫面外時，瀏覽器還會把頁面捲過去，把手也跟著跑到畫面邊緣。
//
// 做法（參考 pdfjs-dist 6.5.438 檢視器 `web/text_layer_builder.js` 的 endOfContent）：每一層放一塊不含文字的墊片，
// 平常縮在層的底緣；有選取碰到這一層時撐滿整層、墊在字串下面（字串 z-index 1、墊片 0），
// 並在文件順序上移到「正在移動的那一端」所在的字串旁邊。落在空白時打到的是墊片，選取停在那個字串的頭或尾。
// 和 PDF.js 不同的兩處：
//   - 墊片裡放一個換行（`<br>`，不算文字，不影響文字層的字與位移）。空的墊片不是能放游標的位置，
//     第二次落在上面時瀏覽器會改換到頁首。
//   - 墊片用內距把游標放在那個字串的尾端（或頭），高度和那一行相同。不然游標在整層的左上角：
//     瀏覽器會把頁面捲到那裡，把手也畫在那裡。游標離字串 2 像素：剛好在字串邊上時，瀏覽器認為把手被字蓋住而不畫。
// Chromium 148 起瀏覽器自己處理空白上的選取，墊片只在之前的版本放；pdfjs-dist 6.5.438 的檢視器也是這樣。版本只能從 UA 判斷。

/** 這個瀏覽器要不要墊片：Chromium 148 之前要，其餘（較新的 Chromium、非 Chromium）不要。 */
export function needsGapFix(userAgent: string, brands?: readonly { brand: string; version: string }[]): boolean {
  const fromBrands = brands?.find((b) => b.brand === "Chromium")?.version;
  const version = fromBrands ?? /\bChrome\/(\d+)\b/.exec(userAgent)?.[1];
  if (!version) return false;
  const major = parseInt(version, 10);
  return Number.isFinite(major) && major < 148;
}

/**
 * 選取改變之後，正在移動的是哪一端：起點不變、終點變了是終點；終點不變、起點變了是起點。
 * 兩端都變了（或沒有上一次可比）時看選取的方向：拖把手時，被拖的那一端是選取的「焦點」。
 */
export function movingEnd(startSame: boolean, endSame: boolean, backward: boolean): "start" | "end" {
  if (startSame && !endSame) return "end";
  if (endSame && !startSame) return "start";
  return backward ? "start" : "end";
}

/** 游標離字串邊緣的距離（CSS 像素）。 */
const CARET_GAP = 2;

/** 一層文字層的墊片、它所在的捲動區，與「落在墊片上時要把選取的一端拉回哪裡」（`null` 是不必拉回）。 */
interface Gap {
  pad: HTMLElement;
  scroller: HTMLElement | null;
  hold: { node: Node; offset: number } | null;
}

const layers = new Map<HTMLElement, Gap>();
let stop: (() => void) | null = null;
let enabled: boolean | null = null;

function gapFixOn(): boolean {
  if (enabled === null) {
    const nav = navigator as Navigator & { userAgentData?: { brands?: { brand: string; version: string }[] } };
    enabled = needsGapFix(navigator.userAgent, nav.userAgentData?.brands);
  }
  return enabled;
}

/** 最近的一個會捲動的上層。 */
function scrollerOf(el: HTMLElement): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const o = getComputedStyle(p);
    if (/(auto|scroll)/.test(o.overflowX + o.overflowY)) return p;
  }
  return null;
}

/** 畫面上看得到的範圍：捲動區與視窗的交集。 */
function visibleBox(scroller: HTMLElement | null): { left: number; top: number; right: number; bottom: number } {
  const v = { left: 0, top: 0, right: window.innerWidth, bottom: window.innerHeight };
  if (!scroller) return v;
  const r = scroller.getBoundingClientRect();
  return { left: Math.max(v.left, r.left), top: Math.max(v.top, r.top), right: Math.min(v.right, r.right), bottom: Math.min(v.bottom, r.bottom) };
}

/** 墊片放回層的最後、縮回底緣。 */
function park(layer: HTMLElement, gap: Gap) {
  if (layer.lastChild !== gap.pad) layer.append(gap.pad);
  layer.classList.remove("selecting");
  gap.pad.style.removeProperty("padding-left");
  gap.pad.style.removeProperty("padding-top");
  gap.pad.style.removeProperty("line-height");
  gap.hold = null;
}

/**
 * 把一層文字層登記起來（文字層畫好之後呼叫）。交回取消登記的函式（這一頁釋放時呼叫）。
 * 不需要墊片的瀏覽器什麼都不做。
 */
export function trackTextLayer(layer: HTMLElement): () => void {
  if (!gapFixOn()) return () => {};
  const pad = document.createElement("div");
  pad.className = "tl-gap";
  pad.append(document.createElement("br"));
  layer.append(pad);
  layers.set(layer, { pad, scroller: scrollerOf(layer), hold: null });
  if (!stop) stop = listen();
  return () => {
    pad.remove();
    layer.classList.remove("selecting");
    layers.delete(layer);
    if (layers.size === 0 && stop) {
      stop();
      stop = null;
    }
  };
}

/**
 * 全部文字層共用一個監聽：選取改變時把墊片搬到移動中那一端的旁邊；選取清掉時收回。
 * 選取還在時不收回：拖把手的觸控由系統直接處理、網頁收不到，下一次拖動開始時墊片要已經在對的地方。
 *
 * 落在墊片上時，選取的一端是那個字串的尾（或頭）。字串的那一頭在畫面外時（放大後一行比畫面寬），
 * 游標改放在原本那一端的位置，選取落到墊片上之後再拉回那裡：不然瀏覽器會把頁面捲過去。
 */
function listen(): () => void {
  const ac = new AbortController();
  let prev: Range | null = null;
  document.addEventListener(
    "selectionchange",
    () => {
      const sel = document.getSelection();
      if (!sel || sel.rangeCount === 0 || sel.isCollapsed) {
        prev = null;
        for (const [layer, gap] of layers) park(layer, gap);
        return;
      }
      const range = sel.getRangeAt(0);
      for (const [layer, gap] of layers) {
        if (range.intersectsNode(layer)) layer.classList.add("selecting");
        else park(layer, gap);
      }
      const startSame = !!prev && range.compareBoundaryPoints(Range.START_TO_START, prev) === 0;
      const endSame = !!prev && range.compareBoundaryPoints(Range.END_TO_END, prev) === 0;
      prev = range.cloneRange();
      const backward = sel.anchorNode !== range.startContainer || sel.anchorOffset !== range.startOffset;
      const end = movingEnd(startSame, endSame, backward);
      const node = end === "start" ? range.startContainer : range.endContainer;
      const offset = end === "start" ? range.startOffset : range.endOffset;
      const el = node instanceof Element ? node : node.parentElement;
      // 落在墊片上、而且要拉回：把移動中的那一端放回原本的位置（焦點就是移動中的那一端）
      const onPad = el?.closest<HTMLElement>(".tl-gap");
      if (onPad) {
        const gap = layers.get(onPad.closest<HTMLElement>(".textLayer") ?? onPad);
        if (gap?.hold && gap.hold.node.isConnected) sel.extend(gap.hold.node, gap.hold.offset);
        return;
      }
      // 移動中那一端所在的字串（`markedContent` 包著時，字串在它裡面）；落在空白上時墊片留在原處
      const span = el?.closest<HTMLElement>(".textLayer span");
      const layer = span?.closest<HTMLElement>(".textLayer");
      const gap = layer ? layers.get(layer) : undefined;
      if (!span || !layer || !gap) return;
      const { pad } = gap;
      // 終點在移動：墊片放在那個字串後面，游標在字串尾；起點在移動：放在前面，游標在字串頭
      if (end === "end" && span.nextSibling !== pad) span.after(pad);
      else if (end === "start" && pad.nextSibling !== span) span.before(pad);
      const lr = layer.getBoundingClientRect();
      const sr = span.getBoundingClientRect();
      const edge = end === "end" ? sr.right : sr.left;
      const vb = visibleBox(gap.scroller);
      let x = end === "end" ? edge + CARET_GAP : edge - CARET_GAP;
      gap.hold = null;
      if (edge < vb.left || edge > vb.right) {
        const caret = document.createRange();
        caret.setStart(node, offset);
        const cr = caret.getBoundingClientRect();
        if (cr.height > 0) {
          x = cr.left;
          gap.hold = { node, offset };
        }
      }
      pad.style.setProperty("padding-left", `${Math.max(0, x - lr.left)}px`);
      pad.style.setProperty("padding-top", `${Math.max(0, sr.top - lr.top)}px`);
      pad.style.setProperty("line-height", `${sr.height}px`);
    },
    { signal: ac.signal },
  );
  return () => ac.abort();
}
