// 重點筆記要送出的內容：`layout.ts` 分好的段落已經不含參考文獻區與被略過的文字；
// 這裡只另外找出摘要，帶給模型當前後文。分段的規則不在這裡改（翻譯與量測共用）。

export interface DigestSegmentLike {
  page: number;
  kind: string;
  source: string;
}

const ABSTRACT = /^\s*(?:abstract|summary|摘要)(?![a-z])[\s.:：—–-]*/i;
const MAX = 3000;

function words(s: string): number {
  return s.split(/\s+/).filter(Boolean).length;
}

/**
 * 摘要：以「Abstract」開頭的段落（只有標題時取它後面第一段內文）；都沒有就用前兩頁第一段夠長的內文。
 * 找不到是 `null`。最多 3000 字。
 */
export function findAbstract(segs: DigestSegmentLike[]): string | null {
  for (let i = 0; i < segs.length; i++) {
    const s = segs[i];
    if (!s || s.page > 1) break;
    const m = ABSTRACT.exec(s.source);
    if (!m) continue;
    const rest = s.source.slice(m[0].length).trim();
    if (rest.length >= 100) return rest.slice(0, MAX);
    const next = segs.slice(i + 1).find((x) => x.kind === "body" && words(x.source) >= 20);
    if (next) return next.source.slice(0, MAX);
  }
  const first = segs.find((s) => s.page <= 1 && s.kind === "body" && words(s.source) >= 60);
  return first ? first.source.slice(0, MAX) : null;
}
