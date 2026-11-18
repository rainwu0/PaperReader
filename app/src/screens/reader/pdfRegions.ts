// 頁面上的可點區域：閱讀畫面畫好原文的一頁之後，用那一頁的文字片段向登記的
// 外掛要那一頁的區域，存在那一頁上；那一頁收掉、換材料時丟掉。點按的判斷與先後在閱讀畫面（`useReaderViews.ts`）。

import type { PageRegion } from "../../ext/types";
import type { PageInput } from "../../translate/layout";

/** 交出一頁的區域（頁序從 0 起）。 */
export type RegionSource = (page: number, input: PageInput, signal: AbortSignal) => Promise<PageRegion[]>;

/** 手指點不準：方框外這麼多點以內也算點到。 */
const TOLERANCE = 2;

export class RegionLayer {
  private source: RegionSource | null = null;
  /** 第幾頁（從 1 起）→ 那一頁的區域。 */
  private readonly byPage = new Map<number, PageRegion[]>();
  private readonly pending = new Map<number, AbortController>();
  private readonly pageText: (n: number) => Promise<PageInput | null>;

  constructor(pageText: (n: number) => Promise<PageInput | null>) {
    this.pageText = pageText;
  }

  /** 換上（或拿掉）來源；已經畫好的幾頁（`rendered`，從 1 起）重新要。 */
  set(source: RegionSource | null, rendered: number[]): void {
    for (const n of [...this.pending.keys()]) this.drop(n);
    this.byPage.clear();
    this.source = source;
    for (const n of rendered) this.load(n);
  }

  /** 那一頁畫好了：向來源要區域。 */
  load(n: number): void {
    const source = this.source;
    if (!source || this.byPage.has(n) || this.pending.has(n)) return;
    const ctrl = new AbortController();
    this.pending.set(n, ctrl);
    void this.pageText(n)
      .then((input) => (input && !ctrl.signal.aborted ? source(n - 1, input, ctrl.signal) : []))
      .then((list) => {
        if (!ctrl.signal.aborted && list.length > 0) this.byPage.set(n, list);
      })
      .catch(() => {})
      .finally(() => {
        if (this.pending.get(n) === ctrl) this.pending.delete(n);
      });
  }

  /** 那一頁收掉了。 */
  drop(n: number): void {
    this.pending.get(n)?.abort();
    this.pending.delete(n);
    this.byPage.delete(n);
  }

  /** 第 `n` 頁（從 1 起）上一點（PDF 頁面座標）落在哪一個區域；有好幾個時取面積最小的。 */
  at(n: number, x: number, y: number): PageRegion | null {
    let best: PageRegion | null = null;
    let area = Infinity;
    for (const r of this.byPage.get(n) ?? []) {
      const [x0, y0, x1, y1] = r.bbox;
      if (x < x0 - TOLERANCE || x > x1 + TOLERANCE || y < y0 - TOLERANCE || y > y1 + TOLERANCE) continue;
      const a = (x1 - x0) * (y1 - y0);
      if (a < area) {
        best = r;
        area = a;
      }
    }
    return best;
  }
}
