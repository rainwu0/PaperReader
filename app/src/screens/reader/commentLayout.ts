// 短評顯示層的排法：有短評的劃線在頁緣的標記放哪一側、哪個高度，展開的卡片怎麼互相讓位。
// 只算位置，不碰畫面。單位是那一頁畫好時的畫面像素，原點在頁面左上角。
//
// 窄螢幕（這一版）：標記在劃線那一行的頁緣，卡片浮在那一行下面、蓋在頁面上。
// 寬版面（電腦版件二）之後把卡片放在頁面右側一欄：用同一份輸入（`CardInput`）另寫一種排法，標記的算法不變。

export type Side = "left" | "right";

/** 一條有短評的劃線在這一頁上的位置：第一行（標記對齊它）與整條的左右範圍（決定放哪一側）。 */
export interface CommentAnchor {
  id: string;
  /** 第一行的上下緣。 */
  lineTop: number;
  lineBottom: number;
  /** 整條劃線的左右緣。 */
  left: number;
  right: number;
  /**
   * 劃線所在那一欄文字的左右緣（知道時）：標記貼著那一欄的外側放，不貼頁緣。
   * 雙擊放大到欄寬時，頁緣在畫面外，欄的外側還在畫面裡（放大時留了標記的位置）。
   */
  colLeft?: number;
  colRight?: number;
}

/**
 * 標記放哪一側：劃線整條落在頁面左邊（右緣不超過頁寬的 55%）放左緣，其他（右欄、跨過中線的單欄內文）放右緣。
 * 雙欄論文的左欄放左邊、右欄放右邊，雙擊放大讀一欄時標記就在那一欄外側的頁緣。
 */
export function sideOf(a: Pick<CommentAnchor, "left" | "right">, pageW: number): Side {
  return a.right <= pageW * 0.55 ? "left" : "right";
}

export interface MarkerSpot {
  id: string;
  side: Side;
  /** 標記方塊的左上角。 */
  x: number;
  y: number;
}

/**
 * 標記的位置：對齊劃線第一行的中間；橫向貼著那一欄文字的外側（`colLeft`、`colRight` 知道時，留 `gap`），
 * 不知道時貼著那一側的頁緣（`inset`）；都不超出頁面。同一側靠得太近的往下讓，不疊在一起。`size` 是標記方塊的邊長。
 */
export function placeMarkers(
  anchors: readonly CommentAnchor[],
  pageW: number,
  size: number,
  inset: number,
  gap = 2,
): MarkerSpot[] {
  const out: MarkerSpot[] = [];
  const lo = inset;
  const hi = pageW - inset - size;
  for (const side of ["left", "right"] as const) {
    const mine = anchors
      .filter((a) => sideOf(a, pageW) === side)
      .map((a) => {
        const want =
          side === "left"
            ? a.colLeft !== undefined
              ? a.colLeft - gap - size
              : lo
            : a.colRight !== undefined
              ? a.colRight + gap
              : hi;
        return { id: a.id, x: Math.min(hi, Math.max(lo, want)), y: (a.lineTop + a.lineBottom) / 2 - size / 2 };
      })
      .sort((a, b) => a.y - b.y);
    let next = -Infinity;
    for (const m of mine) {
      const y = Math.max(m.y, next);
      next = y + size + 2;
      out.push({ id: m.id, side, x: m.x, y });
    }
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

export interface CardInput {
  id: string;
  side: Side;
  /** 想放的上緣（劃線第一行的下面）。 */
  top: number;
  /** 量到的卡片高度。 */
  height: number;
  /**
   * 卡片靠齊的位置（知道時）：左側的是卡片左緣、右側的是卡片右緣，通常緊靠在標記旁邊（不蓋住標記）。放大讀一欄時頁緣在畫面外，
   * 卡片跟著標記靠在那一欄旁邊才看得到。不給就靠頁緣。
   */
  edge?: number;
}

export interface CardSpot {
  id: string;
  left: number;
  top: number;
  width: number;
}

export interface CardOptions {
  /** 卡片最寬多少（頁面窄時再縮）。 */
  width: number;
  /** 離頁緣多遠。 */
  inset: number;
  /** 卡片之間的間距。 */
  gap: number;
}

/** 卡片的寬度：頁面窄時縮到頁寬扣掉兩邊的邊距。 */
export function cardWidth(pageW: number, o: CardOptions): number {
  return Math.max(0, Math.min(o.width, pageW - 2 * o.inset));
}

/**
 * 展開的卡片放在哪裡：左側的靠左（或靠齊 `edge`）、右側的靠右（或靠齊 `edge`）；同一欄裡從上往下排，後面的往下讓，
 * 不疊在一起；排到頁尾放不下時整串往上推（推到頁首還放不下才超出頁尾）。左右兩側的卡片會左右重疊時（頁面窄），
 * 兩側當成同一欄排。
 */
export function placeCards(cards: readonly CardInput[], pageW: number, pageH: number, o: CardOptions): CardSpot[] {
  const width = cardWidth(pageW, o);
  const lo = o.inset;
  const hi = pageW - o.inset - width;
  const leftOf = (c: CardInput) =>
    Math.min(hi, Math.max(lo, c.side === "left" ? (c.edge ?? lo) : (c.edge ?? pageW - o.inset) - width));
  const lefts = new Map(cards.map((c) => [c.id, leftOf(c)]));
  const ls = cards.filter((c) => c.side === "left").map((c) => lefts.get(c.id) ?? lo);
  const rs = cards.filter((c) => c.side === "right").map((c) => lefts.get(c.id) ?? hi);
  // 左側最右邊的一張和右側最左邊的一張左右重疊：當成同一欄
  const oneColumn = ls.length > 0 && rs.length > 0 && Math.max(...ls) + width > Math.min(...rs);
  const columns: CardInput[][] = oneColumn
    ? [[...cards]]
    : [cards.filter((c) => c.side === "left"), cards.filter((c) => c.side === "right")];
  const tops = new Map<string, number>();
  for (const col of columns) {
    const sorted = [...col].sort((a, b) => a.top - b.top);
    const ys: number[] = [];
    let next = o.inset;
    for (const c of sorted) {
      const y = Math.max(c.top, next);
      ys.push(y);
      next = y + c.height + o.gap;
    }
    // 頁尾放不下：從最後一張往上推
    let limit = pageH - o.inset;
    for (let i = sorted.length - 1; i >= 0; i--) {
      const c = sorted[i];
      const y = ys[i];
      if (!c || y === undefined) continue;
      const moved = Math.min(y, limit - c.height);
      ys[i] = moved;
      limit = moved - o.gap;
    }
    // 推過頁首了：再從上往下排一次（這時超出的是頁尾）
    let floor = o.inset;
    sorted.forEach((c, i) => {
      const y = Math.max(ys[i] ?? 0, floor);
      tops.set(c.id, y);
      floor = y + c.height + o.gap;
    });
  }
  return cards.map((c) => ({
    id: c.id,
    left: lefts.get(c.id) ?? lo,
    top: tops.get(c.id) ?? c.top,
    width,
  }));
}

/** 卡片上引句的開頭幾個字（太長就截斷加刪節號）。 */
export function quoteHead(quote: string, max = 24): string {
  const q = quote.replace(/\s+/g, " ").trim();
  return q.length <= max ? q : `${q.slice(0, max).trimEnd()}…`;
}
