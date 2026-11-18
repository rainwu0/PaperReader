// 同一時間只開一份 PDF 的佇列。手機上閱讀、封面縮圖、逐篇處理各開一份 PDF 會吃掉記憶體，
// 所以除了閱讀畫面，開 PDF 的工作都排進這裡，一次做一件：
//   - 「優先」：封面縮圖、加入時補篇名與作者（使用者正在看、正在等的）；
//   - 「背景」：逐篇處理。每讀完一頁問一次 `urgentWaiting()`，有優先的在等就自己停下讓路。
// 原本在 `screens/home/thumbs.ts` 的「一次只畫一張」抽出來放這裡。不依賴任何東西，Node 的測試可以直接測。

export type PdfPriority = "urgent" | "background";

interface Job {
  run: () => Promise<unknown>;
  resolve: (v: unknown) => void;
  reject: (e: unknown) => void;
}

const queues: Record<PdfPriority, Job[]> = { urgent: [], background: [] };
let running = false;
let lastUrgentAt = Number.NEGATIVE_INFINITY;

function pump() {
  if (running) return;
  const next = queues.urgent.shift() ?? queues.background.shift();
  if (!next) return;
  running = true;
  let done: Promise<unknown>;
  try {
    done = next.run();
  } catch (e) {
    done = Promise.reject(e);
  }
  done.then(next.resolve, next.reject).finally(() => {
    running = false;
    pump();
  });
}

/** 排一件要開 PDF 的工作；輪到它才執行。優先的排在所有背景的前面。 */
export function enqueuePdf<T>(priority: PdfPriority, job: () => Promise<T>): Promise<T> {
  if (priority === "urgent") lastUrgentAt = Date.now();
  return new Promise<T>((resolve, reject) => {
    queues[priority].push({ run: job, resolve: resolve as (v: unknown) => void, reject });
    pump();
  });
}

/** 有優先的工作在等（背景的工作該停下讓路）。 */
export const urgentWaiting = (): boolean => queues.urgent.length > 0;

/** 畫面要了一張封面縮圖（還沒畫好的）：逐篇處理在這之後 5 秒內不開始新的一篇（閒著才做）。 */
export function noteUrgentRequest(): void {
  lastUrgentAt = Date.now();
}

/** 距離上一次優先的要求過了多久（毫秒）。 */
export const msSinceUrgent = (): number => Date.now() - lastUrgentAt;
