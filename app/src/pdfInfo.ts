// 加入成功之後，讀 PDF 自帶的 Title 與 Author，看起來不是垃圾就經由既有的「改書目」寫進去。
// 剛加入的那篇只在這台，改書目的時間不會蓋到別台。年份、DOI、arXiv 編號不在這裡補：
// 由逐篇處理的補書目（`perPaper.ts` 的 `bib.ids`）在這之後做，同一個欄位只有一處在補。
// 開 PDF 排進共用的 PDF 佇列（優先，和封面縮圖同一個順序）；這段做完（補好、失敗或逾時）之前，逐篇處理不碰這一篇。
// 加入指令裡取預設篇名（檔名）的規則不動；這裡只是事後補。做不成（讀不出、寫不進）一律安靜略過：
// 文獻已經加入了，維持用檔名當篇名就好。

import { api } from "./api";
import { sourcePdf } from "./api/types";
import { nameWithoutPdf } from "./format";
import { withPdf } from "./pdf/pdfSource";
import { enqueuePdf } from "./pdfQueue";
import { usableAuthors, usableTitle } from "./pdfMeta";
import { deferPerPaper } from "./perPaperApp";

const TIMEOUT_MS = 8000;

/** 補上篇名與作者。有補上交回 `true`。 */
export function fillFromPdfInfo(paperId: string, fileName: string): Promise<boolean> {
  const run = (async () => {
    try {
      return await Promise.race([fill(paperId, fileName), timeout(TIMEOUT_MS, false)]);
    } catch {
      return false;
    }
  })();
  deferPerPaper(paperId, run);
  return run;
}

function timeout<T>(ms: number, value: T): Promise<T> {
  return new Promise((resolve) => window.setTimeout(() => resolve(value), ms));
}

async function fill(paperId: string, fileName: string): Promise<boolean> {
  const detail = await api.getPaper(paperId);
  const source = sourcePdf(detail.paper.materials);
  if (!source) return false;
  // 只在還是剛加入時的樣子（篇名是檔名、沒有作者）才補；使用者已經改過的不動
  const bib = detail.paper.bib;
  const stillDefault = (bib.title ?? "") === nameWithoutPdf(fileName) && bib.authors.length === 0;
  if (!stillDefault) return false;

  const info = await enqueuePdf("urgent", () =>
    withPdf(paperId, source.id, async (pdf) => {
      const meta = await pdf.getMetadata();
      return (meta.info ?? {}) as { Title?: unknown; Author?: unknown };
    }),
  );
  const title = usableTitle(typeof info.Title === "string" ? info.Title : null, fileName);
  const authors = usableAuthors(typeof info.Author === "string" ? info.Author : null);
  if (title === null && authors.length === 0) return false;

  await api.updateBib(
    paperId,
    {
      title: title ?? bib.title ?? null,
      authors: authors.length > 0 ? authors : bib.authors,
      year: bib.year ?? null,
      venue: bib.venue ?? null,
      doi: bib.doi ?? null,
      url: bib.url ?? null,
    },
    detail.token,
  );
  return true;
}
