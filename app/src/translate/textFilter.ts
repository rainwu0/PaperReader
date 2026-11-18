// 譯本 PDF 的文字層：蓋住的原文還在頁面內容裡，選字時要把它濾掉，只留譯文（`buildPdf.ts` 的說明）。
// 純函式，不碰 PDF.js，所以可以在 node 裡測（`scripts/buildpdf-test.mjs`）。

export const COVERS_KEY = "PaperReaderCovers";
export const TEXT_TAG = "PRText";

export type Rect = [number, number, number, number];

export interface Covers {
  /** 頁序（從 0 起算，字串）→ 蓋住的矩形。 */
  pages: Record<string, Rect[]>;
}

/** 讀文件資訊裡的 `PaperReaderCovers`；沒有或讀不懂交回 `null`。 */
export function parseCovers(raw: unknown): Covers | null {
  if (typeof raw !== "string") return null;
  try {
    const v = JSON.parse(raw) as { v?: number; pages?: Record<string, Rect[]> };
    if (v.v !== 1 || typeof v.pages !== "object" || v.pages === null) return null;
    return { pages: v.pages };
  } catch {
    return null;
  }
}

/** 從 PDF.js 的 `getMetadata().info` 取出蓋住的範圍（自訂欄位在 `Custom`，是一個 Map）。 */
export function coversFromInfo(info: unknown): Covers | null {
  const custom = (info as { Custom?: unknown } | null)?.Custom;
  const raw = custom instanceof Map ? custom.get(COVERS_KEY) : (custom as Record<string, unknown> | undefined)?.[COVERS_KEY];
  return parseCovers(raw);
}

/** PDF.js 文字內容的一項（只用到這幾個欄位）。 */
export interface TextItemLike {
  str?: string;
  transform?: number[];
  width?: number;
  height?: number;
  type?: string;
  tag?: string;
}

export interface ChunkLike {
  items: TextItemLike[];
}

const TOL = 1;

function inside(item: TextItemLike, rects: Rect[]): boolean {
  const t = item.transform;
  if (!t) return false;
  const cx = (t[4] ?? 0) + (item.width ?? 0) / 2;
  const cy = (t[5] ?? 0) + (item.height ?? 0) * 0.35;
  return rects.some(([x0, y0, x1, y1]) => cx >= x0 - TOL && cx <= x1 + TOL && cy >= y0 - TOL && cy <= y1 + TOL);
}

/**
 * 濾一批文字內容：標記內容 `PRText` 裡的（譯文）全留；其餘落在蓋住範圍內的（被蓋掉的原文）丟掉；
 * 標記內容本身的項目也拿掉。`stack` 在批與批之間要沿用（一段標記內容可能跨批）。
 */
export function filterChunk<T extends ChunkLike>(chunk: T, rects: Rect[], stack: boolean[]): T {
  const items: TextItemLike[] = [];
  for (const it of chunk.items) {
    if (it.type === "beginMarkedContent") {
      stack.push(it.tag === TEXT_TAG || (stack[stack.length - 1] ?? false));
      continue;
    }
    if (it.type === "endMarkedContent") {
      stack.pop();
      continue;
    }
    if (stack[stack.length - 1] === true || !inside(it, rects)) items.push(it);
  }
  return { ...chunk, items };
}
