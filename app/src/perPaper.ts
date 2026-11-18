// 逐篇處理：應用唯一的排程。一篇加入之後排進來；應用開著時，對還沒做過的補做。
// 一次一篇；同一篇要做的幾件抽取共用一次開檔、一次版面分析。閒著才做（應用在前景、閱讀畫面沒開著、沒有產生的工作、
// 最近 5 秒沒有封面縮圖的要求），做到一半條件不成立就停下這一篇，下次從頭做，不算失敗。
//
// 這個檔只有排程的規則與內建的「補書目」（`bib.ids`）；開檔、指令、畫面的狀態由呼叫的一方交進來（`PerPaperDeps`），
// 實際接上應用的是 `perPaperApp.ts`。分開是為了讓 `scripts/perpaper-test.mjs` 不必載入 PDF.js 與外殼就能測。

import type { BibFill, TextDraft, TextState } from "./api/types";
import type { PerPaperInput, PerPaperTask } from "./ext/types";
import { pickArxiv, pickDoi, pickYear } from "./pdfMeta.ts";
import type { PageInput } from "./translate/layout";
import { layoutDocument, pageText } from "./translate/layout.ts";

// ---------- 紀錄（應用空間的 `per-paper/<文獻庫代號>.json`） ----------

/** 一篇的一件抽取做到哪裡。 */
export interface TaskRecord {
  /** 做的時候原文的指紋。 */
  sha256: string;
  /** 做的時候抽取的版本。 */
  version: number;
  /** `done` 做過（含抽不到東西）；`failed` 暫時失敗；`skipped` 永久略過。 */
  result: "done" | "failed" | "skipped";
  reason: string | null;
  /** 同一份原文、同一個版本試過幾次。 */
  tries: number;
}

export interface PerPaperRecord {
  format: 1;
  /** 文獻代號 → 抽取的代號 → 紀錄。 */
  papers: Record<string, Record<string, TaskRecord>>;
}

/** 暫時失敗最多試幾次，之後當成永久略過。 */
export const MAX_TRIES = 3;
/** 一篇最多做多久（毫秒），超過算暫時失敗。 */
export const PAPER_TIMEOUT_MS = 120_000;

export const emptyRecord = (): PerPaperRecord => ({ format: 1, papers: {} });

/** 讀進來的紀錄（外殼交回的 JSON，可能是 `null` 或別的東西）：讀不懂的部分當成沒有。 */
export function parseRecord(raw: unknown): PerPaperRecord {
  const out = emptyRecord();
  if (typeof raw !== "object" || raw === null) return out;
  const papers = (raw as { papers?: unknown }).papers;
  if (typeof papers !== "object" || papers === null) return out;
  for (const [pid, tasks] of Object.entries(papers)) {
    if (typeof tasks !== "object" || tasks === null) continue;
    for (const [tid, r] of Object.entries(tasks as Record<string, unknown>)) {
      const t = r as Partial<TaskRecord>;
      if (
        typeof t?.sha256 !== "string" ||
        typeof t.version !== "number" ||
        (t.result !== "done" && t.result !== "failed" && t.result !== "skipped")
      )
        continue;
      (out.papers[pid] ??= {})[tid] = {
        sha256: t.sha256,
        version: t.version,
        result: t.result,
        reason: typeof t.reason === "string" ? t.reason : null,
        tries: typeof t.tries === "number" ? t.tries : 1,
      };
    }
  }
  return out;
}

/** 這一件對這份原文、這個版本算做完了：做過、永久略過、或暫時失敗已經試滿次數。 */
export function settled(r: TaskRecord | undefined, sha256: string, version: number): boolean {
  if (!r || r.sha256 !== sha256 || r.version < version) return false;
  return r.result !== "failed" || r.tries >= MAX_TRIES;
}

/** 記一次結果。暫時失敗時，同一份原文、同一個版本的次數累加。 */
export function recordResult(
  rec: PerPaperRecord,
  paperId: string,
  taskId: string,
  sha256: string,
  version: number,
  result: TaskRecord["result"],
  reason: string | null,
): void {
  const prev = rec.papers[paperId]?.[taskId];
  const same = prev && prev.sha256 === sha256 && prev.version === version;
  const tries = result === "failed" && same && prev.result === "failed" ? prev.tries + 1 : 1;
  (rec.papers[paperId] ??= {})[taskId] = { sha256, version, result, reason, tries };
}

// ---------- 失敗的分類 ----------

/** 這一篇在中途被叫停（條件不成立、換頁）：不算失敗。 */
export class Stopped extends Error {
  constructor() {
    super("stopped");
    this.name = "Stopped";
  }
}

/** 原文打不開或加密（PDF.js 的錯誤）、整份沒有文字：永久略過。 */
export class SourceUnusable extends Error {
  readonly reason: string;
  constructor(reason: string) {
    super(reason);
    this.reason = reason;
    this.name = "SourceUnusable";
  }
}

const TEMPORARY = new Set(["Conflict", "Io", "NoSpace", "NoLibrary", "Timeout", "ReadOnly", "NotFound"]);
const PERMANENT = new Set(["Unreadable", "InvalidInput", "FormatTooNew"]);

/** 一件抽取丟出的錯誤 → 暫時失敗或永久略過。指令的錯誤看 `kind`；不認得的當成暫時（試滿次數也會停）。 */
export function classifyError(e: unknown): { result: "failed" | "skipped"; reason: string } {
  if (e instanceof SourceUnusable) return { result: "skipped", reason: e.reason };
  const kind = typeof e === "object" && e !== null && "kind" in e ? String((e as { kind: unknown }).kind) : "Unknown";
  if (PERMANENT.has(kind)) return { result: "skipped", reason: kind };
  return { result: "failed", reason: TEMPORARY.has(kind) ? kind : "Unknown" };
}

// ---------- 內建的抽取：補書目 ----------

/** 要補的那一篇的書目（看三欄是不是都已有值）。 */
export interface BibLite {
  year?: number | null;
  doi?: string | null;
  url?: string | null;
}

/** 內建的抽取可以多一個「書目已經都有了就算做過」。 */
export interface BuiltinTask extends PerPaperTask {
  satisfied?(paper: PaperRef): boolean;
}

const filled = (s: string | null | undefined) => typeof s === "string" && s.trim() !== "";

/**
 * `bib.ids`：從 PDF 自帶資訊、XMP、第一頁抽 DOI、arXiv 編號、年份，經核心的 `fill_bib_blanks` 只補空著的欄位
 * （不改書目的時間）。arXiv 編號寫成 `url`（`https://arxiv.org/abs/<編號>`）。什麼都沒抽到也算做過。
 */
export function bibIdsTask(fill: (paperId: string, f: BibFill) => Promise<boolean>): BuiltinTask {
  return {
    id: "bib.ids",
    version: 1,
    needsLayout: false,
    satisfied: (p) => p.bib.year != null && filled(p.bib.doi) && filled(p.bib.url),
    async run(input: PerPaperInput, signal: AbortSignal) {
      const f = bibFromInput(input);
      if (f.year === null && f.doi === null && f.url === null) return;
      if (signal.aborted) throw new Stopped();
      await fill(input.paperId, f);
    },
  };
}

/** 從一篇的自帶資訊、XMP、第一頁算出要補的三欄。 */
export function bibFromInput(input: Pick<PerPaperInput, "info" | "xmp" | "firstPageText">): BibFill {
  const arxiv = pickArxiv(input.xmp, input.firstPageText);
  return {
    year: pickYear(input.info.CreationDate, input.firstPageText),
    doi: pickDoi(input.info, input.xmp, input.firstPageText),
    url: arxiv ? `https://arxiv.org/abs/${arxiv}` : null,
  };
}

// ---------- 內建的抽取：抽文字 ----------

/** 抽法的代號與版本（`text.json` 的 `extraction`）。 */
export const TEXT_EXTRACTOR = "pdfjs-text";
export const TEXT_VERSION = 1;
/** 一頁、一份最多幾個字（Unicode 字元，和核心的上限相同，`app/core/src/text.rs`）。 */
export const MAX_PAGE_CHARS = 100_000;
export const MAX_TOTAL_CHARS = 5_000_000;

/**
 * 單獨的代理碼換成 U+FFFD（`String.prototype.toWellFormed`；`tsconfig` 的 lib 是 ES2022，沒有它）。
 * UTF-16 的長度不變，所以換了之後的位置照樣對得上原字串（閱讀畫面框出也用這一個）。經 IPC 送出的字串要合法。
 */
export function wellFormed(s: string): string {
  if (!/[\uD800-\uDFFF]/.test(s)) return s;
  let out = "";
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c >= 0xd800 && c <= 0xdbff) {
      const d = i + 1 < s.length ? s.charCodeAt(i + 1) : 0;
      if (d >= 0xdc00 && d <= 0xdfff) {
        out += s[i]! + s[i + 1]!;
        i++;
        continue;
      }
      out += "�";
    } else if (c >= 0xdc00 && c <= 0xdfff) {
      out += "�";
    } else {
      out += s[i]!;
    }
  }
  return out;
}

/**
 * 每頁的文字套上限：先換掉單獨的代理碼；一頁超過 10 萬字就截斷那一頁；一份加上這一頁會超過 500 萬字就停在
 * 這一頁之前。字數逐碼點數（`for…of`），截斷不切開代理對。有截斷就 `truncated`。
 */
export function limitPages(pages: readonly string[]): { pages: string[]; truncated: boolean } {
  const out: string[] = [];
  let total = 0;
  let truncated = false;
  for (const raw of pages) {
    let text = wellFormed(raw);
    let n = 0;
    let cut = -1;
    let at = 0;
    for (const ch of text) {
      if (n === MAX_PAGE_CHARS) {
        cut = at;
        break;
      }
      n++;
      at += ch.length;
    }
    if (cut >= 0) {
      text = text.slice(0, cut);
      truncated = true;
    }
    if (total + n > MAX_TOTAL_CHARS) {
      truncated = true;
      break;
    }
    total += n;
    out.push(text);
  }
  return { pages: out, truncated };
}

/**
 * `text.pages`：把原文逐頁的文字（`pageText`，和補書目、版面分析同一份讀頁）經核心的 `write_text` 存成 `text.json`。
 * 做過沒有一次問一批 `text_states`：同一份原文、同一個抽法、版本不小於自己的算做過。整份沒有文字（掃描版）照樣寫，
 * 每頁空字串，別台就知道抽過了。
 */
export function textPagesTask(deps: {
  write: (paperId: string, draft: TextDraft) => Promise<unknown>;
  states: (paperIds: string[]) => Promise<Record<string, TextState | null>>;
}): BuiltinTask {
  return {
    id: "text.pages",
    version: TEXT_VERSION,
    needsLayout: false,
    needsPages: true,
    async doneFor(papers) {
      const st = await deps.states(papers.map((p) => p.paperId));
      const done = new Set<string>();
      for (const p of papers) {
        const s = st[p.paperId];
        if (s && s.extractor === TEXT_EXTRACTOR && s.source_sha256 === p.sourceSha256 && s.version >= TEXT_VERSION) {
          done.add(p.paperId);
        }
      }
      return done;
    },
    async run(input: PerPaperInput, signal: AbortSignal) {
      if (!input.pages) throw new Error("pages were not read");
      const limited = limitPages(input.pages);
      if (signal.aborted) throw new Stopped();
      await deps.write(input.paperId, {
        extractor: TEXT_EXTRACTOR,
        version: TEXT_VERSION,
        source_material: input.source.materialId,
        source_sha256: input.source.sha256,
        pages: limited.pages,
        truncated: limited.truncated,
      });
    },
  };
}

// ---------- 排程 ----------

/** 目錄裡的一篇（有原文的）。 */
export interface PaperRef {
  id: string;
  /** 加入的時間（新的先做）。 */
  added: string | null;
  source: { materialId: string; sha256: string; pages: number | null };
  bib: BibLite;
}

/** 開好的原文。 */
export interface OpenedSource {
  numPages: number;
  info: Record<string, unknown>;
  xmp: string | null;
  /** 讀前 `count` 頁的文字；每頁讀完問一次 `stop()`，為真就停下交回 `null`。 */
  readPages(count: number, stop: () => boolean): Promise<PageInput[] | null>;
}

export interface PerPaperDeps {
  /** 目錄裡有原文的每一篇。 */
  listPapers(): Promise<PaperRef[]>;
  loadRecord(): Promise<unknown>;
  saveRecord(rec: PerPaperRecord): Promise<void>;
  /** 經共用的 PDF 佇列（背景）開這篇的原文、做一件事、收掉。打不開時丟出 `SourceUnusable` 或指令的錯誤。 */
  withSource<T>(paper: PaperRef, work: (src: OpenedSource) => Promise<T>): Promise<T>;
  /** 記下原文的頁數（寫參考文獻清單之前要知道）。 */
  setPages(paperId: string, materialId: string, pages: number): Promise<void>;
  /** 現在不能做的原因（在背景、有產生的工作、剛要了縮圖）；可以做交回 `null`。閱讀畫面的暫停由排程自己記。 */
  busyReason(): Promise<string | null>;
  /** 有優先的 PDF 工作（縮圖、加入時補篇名）在等。 */
  urgentWaiting(): boolean;
  timeoutMs?: number;
}

export type StepResult = "paused" | "busy" | "nothing" | "worked" | "stopped";

/** 紀錄攢到這麼多篇、或這麼久沒寫，就寫一次；一輪做完、被叫停時也寫。 */
export const SAVE_EVERY_PAPERS = 20;
export const SAVE_EVERY_MS = 5000;

/**
 * 一輪：開頭讀一次目錄、問一次外掛做過沒有，排好要做的篇，之後一步做一篇（成本和篇數成正比）。
 * 做完、加入了新的一篇、換了文獻庫就結束，下一步再開新的一輪。
 */
interface Round {
  papers: PaperRef[];
  queue: Array<{ paper: PaperRef; tasks: BuiltinTask[] }>;
  /** 每件抽取都做過或永久略過的篇。 */
  complete: Set<string>;
  /** `doneFor` 交回的（這一輪裡當成做過）。 */
  external: Map<string, Set<string> | null>;
}

export interface Progress {
  /** 每件抽取都做過或永久略過的篇數。 */
  done: number;
  /** 有原文的篇數。 */
  total: number;
  paused: boolean;
  /**
   * 每件抽取還沒做完的篇數：有原文、紀錄不是做完、這一輪的 `doneFor` 也沒說做過。永久略過、
   * 暫時失敗試滿次數的算做完。首頁搜尋的一節要 `text.pages` 的這個數。
   */
  pending: Record<string, number>;
}

function samePending(a: Record<string, number>, b: Record<string, number>): boolean {
  const ka = Object.keys(a);
  return ka.length === Object.keys(b).length && ka.every((k) => a[k] === b[k]);
}

export class PerPaperScheduler {
  private record: PerPaperRecord | null = null;
  private paused = false;
  private holds = 0;
  private readonly deferred = new Map<string, Promise<unknown>>();
  /** 這次開應用暫時失敗過的（下次開應用再試）。 */
  private readonly failedThisRun = new Set<string>();
  private current: AbortController | null = null;
  private progress: Progress = { done: 0, total: 0, paused: false, pending: {} };
  private readonly listeners = new Set<() => void>();
  private round: Round | null = null;
  /** 還沒寫進紀錄檔的篇數、上次寫的時間。 */
  private unsaved = 0;
  private savedAt = 0;

  private readonly deps: PerPaperDeps;
  private readonly tasks: () => BuiltinTask[];

  constructor(deps: PerPaperDeps, tasks: () => BuiltinTask[]) {
    this.deps = deps;
    this.tasks = tasks;
  }

  // ---- 畫面的開關 ----

  /** 暫停或繼續（設定的「整理文獻庫」）。暫停時正在做的那一篇停下。 */
  setPaused(p: boolean): void {
    this.paused = p;
    if (p) this.stopCurrent();
    this.setProgress({ ...this.progress, paused: p });
  }

  /** 閱讀畫面掛上時持有一個暫停，交回放掉的函式。持有時正在做的那一篇停下。 */
  hold(): () => void {
    this.holds++;
    this.stopCurrent();
    let released = false;
    return () => {
      if (released) return;
      released = true;
      this.holds--;
    };
  }

  /** 這一篇在 `until` 結束之前不排進來（剛加入、還在補篇名與作者）。結束後下一步重讀目錄，先做它。 */
  defer(paperId: string, until: Promise<unknown>): void {
    const p = until.catch(() => {}).finally(() => {
      if (this.deferred.get(paperId) === p) this.deferred.delete(paperId);
      this.round = null;
    });
    this.deferred.set(paperId, p);
    this.round = null;
  }

  /** 換了文獻庫：紀錄每個文獻庫一份，下一步重新讀。 */
  reset(): void {
    this.stopCurrent();
    this.record = null;
    this.round = null;
    this.unsaved = 0;
    this.failedThisRun.clear();
  }

  /** 叫正在做的那一篇停下（下次從頭做，不算失敗）。 */
  stopCurrent(): void {
    this.current?.abort();
  }

  getProgress = (): Progress => this.progress;

  subscribe = (cb: () => void): (() => void) => {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  };

  private setProgress(p: Progress) {
    if (
      p.done === this.progress.done &&
      p.total === this.progress.total &&
      p.paused === this.progress.paused &&
      samePending(p.pending, this.progress.pending)
    )
      return;
    this.progress = p;
    for (const l of this.listeners) l();
  }

  // ---- 一步 ----

  private async loadRecord(): Promise<PerPaperRecord> {
    if (!this.record) {
      let raw: unknown = null;
      try {
        raw = await this.deps.loadRecord();
      } catch {
        raw = null;
      }
      this.record = parseRecord(raw);
    }
    return this.record;
  }

  /** 一篇還要做哪幾件、是不是每件都做完了；`doneFor` 交回 `null` 的抽取這一輪不做，也不算做完。 */
  private want(p: PaperRef, rec: PerPaperRecord, external: Map<string, Set<string> | null>) {
    const tasks: BuiltinTask[] = [];
    let complete = true;
    for (const t of this.tasks()) {
      const r = rec.papers[p.id]?.[t.id];
      if (settled(r, p.source.sha256, t.version)) continue;
      if (t.doneFor) {
        const set = external.get(t.id);
        if (set === null || set === undefined) {
          complete = false;
          continue;
        }
        if (set.has(p.id)) continue;
      } else if (!r && t.satisfied?.(p)) {
        continue;
      }
      complete = false;
      if (!this.failedThisRun.has(`${p.id}|${t.id}|${p.source.sha256}`)) tasks.push(t);
    }
    return { tasks, complete };
  }

  /**
   * 開新的一輪：讀一次目錄、每件有 `doneFor` 的抽取問一次做過沒有、排好要做的篇（新加入的先做）。
   * `doneFor` 只問紀錄裡那件還沒做完的篇，一篇都沒有就不問（`text_states` 要讀檔）；
   * 它說做過的記成 `done`（那時的原文指紋、那件的版本），下一輪就不再問（紀錄不見、別台抽過的只讀一次檔頭）。
   */
  private async startRound(rec: PerPaperRecord): Promise<Round> {
    const papers = await this.deps.listPapers();
    const external = new Map<string, Set<string> | null>();
    for (const t of this.tasks()) {
      if (!t.doneFor) continue;
      const ask = papers.filter((p) => !settled(rec.papers[p.id]?.[t.id], p.source.sha256, t.version));
      if (ask.length === 0) {
        external.set(t.id, new Set());
        continue;
      }
      let done: Set<string> | null;
      try {
        done = await t.doneFor(ask.map((p) => ({ paperId: p.id, sourceSha256: p.source.sha256 })));
      } catch {
        done = null;
      }
      external.set(t.id, done);
      for (const p of ask) {
        if (!done?.has(p.id)) continue;
        recordResult(rec, p.id, t.id, p.source.sha256, t.version, "done", null);
        this.unsaved++;
      }
    }
    const complete = new Set<string>();
    const queue: Round["queue"] = [];
    for (const p of papers) {
      const w = this.want(p, rec, external);
      if (w.complete) complete.add(p.id);
      if (w.tasks.length > 0 && !this.deferred.has(p.id)) queue.push({ paper: p, tasks: w.tasks });
    }
    queue.sort((a, b) => (b.paper.added ?? "").localeCompare(a.paper.added ?? ""));
    return { papers, queue, complete, external };
  }

  /** 寫紀錄（`force` 時只要有沒寫的就寫，否則攢夠篇數或時間才寫）。存不進去：下次開應用照紀錄重做，不影響這次。 */
  private async saveRecord(rec: PerPaperRecord, force: boolean): Promise<void> {
    if (this.unsaved === 0) return;
    if (!force && this.unsaved < SAVE_EVERY_PAPERS && Date.now() - this.savedAt < SAVE_EVERY_MS) return;
    this.unsaved = 0;
    this.savedAt = Date.now();
    try {
      await this.deps.saveRecord(rec);
    } catch {
      // 同上
    }
  }

  private showProgress(r: Round): void {
    this.setProgress({
      done: r.complete.size,
      total: r.papers.length,
      paused: this.paused,
      pending: this.pendingOf(r),
    });
  }

  /** 每件抽取還沒做完的篇數（`Progress.pending`）。 */
  private pendingOf(r: Round): Record<string, number> {
    const rec = this.record ?? emptyRecord();
    const out: Record<string, number> = {};
    for (const t of this.tasks()) {
      let n = 0;
      for (const p of r.papers) {
        const rr = rec.papers[p.id]?.[t.id];
        if (settled(rr, p.source.sha256, t.version)) continue;
        if (t.doneFor && r.external.get(t.id)?.has(p.id)) continue;
        if (!t.doneFor && !rr && t.satisfied?.(p)) continue;
        n++;
      }
      out[t.id] = n;
    }
    return out;
  }

  /** 做一步：這一輪的下一篇，做完它（或被叫停）。這一輪做完了交回 `nothing`，下一步開新的一輪。 */
  async step(): Promise<StepResult> {
    if (this.holds > 0) return "busy";
    if (!this.paused && (await this.deps.busyReason())) return "busy";
    const rec = await this.loadRecord();
    if (!this.round) {
      this.round = await this.startRound(rec);
      if (this.savedAt === 0) this.savedAt = Date.now();
    }
    const round = this.round;
    this.showProgress(round);
    // 暫停時照樣算「已處理幾篇」給設定看，不做
    if (this.paused) return "paused";
    // 這一輪排進來之後才延後的（剛加入、還在補篇名）跳過，下一輪再排
    while (round.queue[0] && this.deferred.has(round.queue[0].paper.id)) round.queue.shift();
    const next = round.queue[0];
    if (!next) {
      this.round = null;
      await this.saveRecord(rec, true);
      return "nothing";
    }
    if (this.paused || this.holds > 0) return "busy";
    const outcome = await this.runPaper(next.paper, next.tasks);
    if (outcome === "stopped") {
      // 被叫停時，已經做完的那幾件也記下來；這一篇留在隊伍前面，下一步從頭做
      await this.saveRecord(rec, true);
      return "stopped";
    }
    round.queue.shift();
    this.unsaved++;
    if (this.want(next.paper, rec, round.external).complete) round.complete.add(next.paper.id);
    if (this.round === round) this.showProgress(round);
    // 這一輪做完了就寫；否則攢夠篇數或時間才寫（應用要結束時不必補寫，下次重做那幾篇即可）
    await this.saveRecord(rec, round.queue.length === 0);
    return "worked";
  }

  private async runPaper(paper: PaperRef, tasks: BuiltinTask[]): Promise<"stopped" | "worked"> {
    const ctrl = new AbortController();
    this.current = ctrl;
    const rec = this.record ?? emptyRecord();
    const sha = paper.source.sha256;
    const note = (t: BuiltinTask, result: TaskRecord["result"], reason: string | null) => {
      recordResult(rec, paper.id, t.id, sha, t.version, result, reason);
      if (result === "failed") this.failedThisRun.add(`${paper.id}|${t.id}|${sha}`);
    };
    let timer: ReturnType<typeof setTimeout> | undefined;
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(() => {
        ctrl.abort();
        reject(Object.assign(new Error("timeout"), { kind: "Timeout" }));
      }, this.deps.timeoutMs ?? PAPER_TIMEOUT_MS);
    });
    const stop = () => {
      if (this.deps.urgentWaiting()) ctrl.abort();
      return ctrl.signal.aborted;
    };
    const finished = new Set<string>();
    try {
      await Promise.race([
        timeout,
        this.deps.withSource(paper, async (src) => {
          // 有一件要版面分析或要每頁的文字就讀全部的頁，一篇只讀一次；原文的頁數還不知道先記下（寫入要它）
          const layout = tasks.some((t) => t.needsLayout);
          const wantPages = tasks.some((t) => t.needsPages);
          const allPages = layout || wantPages;
          if (allPages && paper.source.pages === null) await this.deps.setPages(paper.id, paper.source.materialId, src.numPages);
          const pages = await src.readPages(allPages ? src.numPages : 1, stop);
          if (pages === null) throw new Stopped();
          const firstPageText = pageText(pages[0]?.items ?? []);
          const noText = pages.every((p) => p.items.every((it) => it.str.trim() === ""));
          const input: PerPaperInput = {
            paperId: paper.id,
            source: { materialId: paper.source.materialId, sha256: sha, pages: paper.source.pages ?? src.numPages },
            info: src.info,
            xmp: src.xmp,
            firstPageText,
            layout: layout ? layoutDocument(pages) : null,
            pages: wantPages ? pages.map((p) => pageText(p.items)) : null,
          };
          for (const t of tasks) {
            if (stop()) throw new Stopped();
            if (t.needsLayout && noText) {
              note(t, "skipped", "NoText");
              finished.add(t.id);
              continue;
            }
            try {
              await t.run(input, ctrl.signal);
              note(t, "done", null);
            } catch (e) {
              if (ctrl.signal.aborted || e instanceof Stopped) throw new Stopped();
              const c = classifyError(e);
              note(t, c.result, c.reason);
            }
            finished.add(t.id);
          }
        }),
      ]);
      return "worked";
    } catch (e) {
      if (e instanceof Stopped || (ctrl.signal.aborted && !(e instanceof Error && e.message === "timeout"))) {
        return "stopped";
      }
      // 打不開、讀不出、逾時：還沒做的每一件都記
      const c = classifyError(e);
      for (const t of tasks) if (!finished.has(t.id)) note(t, c.result, c.reason);
      return "worked";
    } finally {
      clearTimeout(timer);
      if (this.current === ctrl) this.current = null;
    }
  }
}
