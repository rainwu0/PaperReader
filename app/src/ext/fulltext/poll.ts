// 「內文」一節的查詢與再問：一次 10 篇，捲到最後再要下一批；結果帶 `building` 時 3 秒後用同一個
// 查詢再問，帶 `pending` 時 1 秒後再問，兩者都是假就停；查詢換了就不再問舊的。畫面以外的部分寫在這裡，Node 測試直接用。

import type { PaperHits, SearchResult } from "./api";

/** 一批幾篇（和外殼的 `PAGE` 相同）。 */
export const PAGE = 10;
export const BUILDING_AGAIN_MS = 3000;
export const PENDING_AGAIN_MS = 1000;
/** 查詢最少幾個字（Unicode 字元，和外掛的 `MIN_QUERY_CHARS` 相同；去頭尾空白）。 */
export const MIN_CHARS = 2;

/** 查不查內文：去頭尾空白後至少 2 個字（Unicode 字元，不是 UTF-16 單位）。 */
export function longEnough(query: string): boolean {
  return [...query.trim()].length >= MIN_CHARS;
}

/** 多久之後再問；不用再問是 `null`。 */
export function againAfter(r: Pick<SearchResult, "building" | "pending">): number | null {
  if (r.building) return BUILDING_AGAIN_MS;
  if (r.pending) return PENDING_AGAIN_MS;
  return null;
}

export interface BodyState {
  query: string;
  papers: PaperHits[];
  more: boolean;
  building: boolean;
  pending: boolean;
  /** 第一批還沒回來。 */
  loading: boolean;
  failed: boolean;
}

export type Fetch = (query: string, scope: string[] | null, offset: number) => Promise<SearchResult>;

export interface Timers {
  set: (fn: () => void, ms: number) => unknown;
  clear: (id: unknown) => void;
}

const realTimers: Timers = {
  set: (fn, ms) => setTimeout(fn, ms),
  clear: (id) => clearTimeout(id as ReturnType<typeof setTimeout>),
};

/** 一節的查詢狀態。`start` 換查詢（舊的回來也不理）；`next` 要下一批；`stop` 停掉。 */
export class BodySearch {
  private gen = 0;
  private timer: unknown = null;
  private scope: string[] | null = null;
  private nextOffset = 0;
  private busy = false;
  state: BodyState = { query: "", papers: [], more: false, building: false, pending: false, loading: false, failed: false };

  private readonly fetch: Fetch;
  private readonly onChange: (s: BodyState) => void;
  private readonly timers: Timers;

  constructor(fetch: Fetch, onChange: (s: BodyState) => void, timers: Timers = realTimers) {
    this.fetch = fetch;
    this.onChange = onChange;
    this.timers = timers;
  }

  private set(s: Partial<BodyState>) {
    this.state = { ...this.state, ...s };
    this.onChange(this.state);
  }

  private clearTimer() {
    if (this.timer !== null) this.timers.clear(this.timer);
    this.timer = null;
  }

  start(query: string, scope: string[] | null) {
    this.gen += 1;
    this.clearTimer();
    this.scope = scope;
    this.busy = false;
    this.set({ query, papers: [], more: false, building: false, pending: false, loading: true, failed: false });
    this.ask(this.gen, false);
  }

  stop() {
    this.gen += 1;
    this.clearTimer();
  }

  /** 捲到最後一篇：要下一批。 */
  next() {
    if (this.busy || !this.state.more || this.state.loading) return;
    this.ask(this.gen, true);
  }

  private ask(gen: number, append: boolean) {
    const offset = append ? this.nextOffset : 0;
    this.busy = true;
    this.fetch(this.state.query, this.scope, offset).then(
      (r) => {
        if (gen !== this.gen) return;
        this.busy = false;
        this.nextOffset = offset + PAGE;
        this.set({
          papers: append ? [...this.state.papers, ...r.papers] : r.papers,
          more: r.more,
          building: r.building,
          pending: r.pending,
          loading: false,
          failed: false,
        });
        const wait = againAfter(r);
        if (wait !== null && !append) {
          // 再問：用同一個查詢從第一批重來（背景整理完，結果可能變了）
          this.clearTimer();
          this.timer = this.timers.set(() => {
            this.timer = null;
            if (gen === this.gen) this.ask(gen, false);
          }, wait);
        }
      },
      () => {
        if (gen !== this.gen) return;
        this.busy = false;
        this.set({ loading: false, failed: true });
      },
    );
  }
}
