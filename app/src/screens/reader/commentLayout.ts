// 短評標記的排法：有短評的劃線，標記像註腳的上標，放在劃線第一行行尾的右上方、行與行之間的空白，不蓋字。
// 只算位置，不碰畫面。單位是那一頁畫好時的畫面像素，原點在頁面左上角。

/** 一條有短評的劃線在這一頁上的位置：第一行的上下緣與右緣。 */
export interface CommentAnchor {
  id: string;
  /** 第一行的上下緣。 */
  lineTop: number;
  lineBottom: number;
  /** 第一行的右緣（標記接在它後面）。 */
  lineRight: number;
}

export interface MarkerSpot {
  id: string;
  /** 標記方塊的左上角與邊長。 */
  x: number;
  y: number;
  size: number;
}

/** 標記的邊長：行高的 0.42 倍（約一個字高的六成），至少 `min`、最多 `max`。 */
export function markerSize(lineH: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, Math.round(lineH * 0.42)));
}

/**
 * 標記的位置：接在第一行行尾（留 `gap`），在那一行的上緣、也就是上一行與這一行之間的空白（上緣往上 `rise` 倍邊長），
 * 像註腳的上標；不超出頁面（離頁緣 `inset`）。兩條劃線的標記會疊在一起時（行尾靠得太近、高度也接近），
 * 後面的往右讓，讓出一個標記的寬度。
 */
export function placeMarkers(
  anchors: readonly CommentAnchor[],
  pageW: number,
  size: { min: number; max: number },
  inset: number,
  gap = 1,
  rise = 0.5,
): MarkerSpot[] {
  const out: MarkerSpot[] = [];
  const sorted = [...anchors].sort((a, b) => a.lineTop - b.lineTop || a.lineRight - b.lineRight);
  for (const a of sorted) {
    const s = markerSize(a.lineBottom - a.lineTop, size.min, size.max);
    const hi = pageW - inset - s;
    const y = a.lineTop - s * rise;
    let x = Math.max(inset, Math.min(hi, a.lineRight + gap));
    // 同一行已經有標記佔著這裡：往旁邊讓（到頁緣放不下時往左讓）
    for (const o of out) {
      if (Math.abs(o.y - y) < s && Math.abs(o.x - x) < s) {
        x = o.x + o.size + gap <= hi ? o.x + o.size + gap : Math.max(inset, o.x - s - gap);
      }
    }
    out.push({ id: a.id, x, y, size: s });
  }
  return out;
}

/**
 * 放大到一欄的寬度時，欄的兩側各要留多少（PDF 的點），標記才放得進畫面：`room` 是標記要的像素（邊長加間距），
 * `width` 是欄寬（點）、`screen` 是畫面寬（像素）。放大後一點等於 `screen / (width + 2 × 留白)` 像素。
 */
export function markerRoom(room: number, width: number, screen: number): number {
  const free = screen - 2 * room;
  return free > 0 ? (room * width) / free : 0;
}

/** 引句的開頭幾個字（太長就截斷加刪節號）。 */
export function quoteHead(quote: string, max = 24): string {
  const q = quote.replace(/\s+/g, " ").trim();
  return q.length <= max ? q : `${q.slice(0, max).trimEnd()}…`;
}
