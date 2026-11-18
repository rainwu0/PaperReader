// 首頁與文獻資訊顯示一篇文獻時用到的小整理：有沒有譯本、封面用哪份材料、讀到哪一頁。
// 只整理核心交回的資料，不推算資料規則。

import { FORM_MARKDOWN, FORM_OVERLAY, ROLE_DIGEST, ROLE_SOURCE, ROLE_TRANSLATION, isGeneratedPdf } from "../../api/types";
import type { Material, MaterialSummary, PaperSummary } from "../../api";
import { t } from "../../strings";

/** 抽出文字（整批匯入收得到）。 */
const ROLE_TEXT = "text";

/**
 * 一項材料給人看的名稱：原文、譯本（產生的 PDF／自己加入的 PDF／Markdown）、逐段的譯文、重點筆記、抽出文字。
 */
export function materialLabel(m: Material): string {
  if (m.role === ROLE_SOURCE) return t("reader.tool.source");
  if (m.role === ROLE_TRANSLATION && m.form === FORM_OVERLAY) return t("info.material.overlay");
  if (m.role === ROLE_TRANSLATION && m.form === FORM_MARKDOWN) return t("info.material.markdown");
  if (m.role === ROLE_TRANSLATION) return t(isGeneratedPdf(m) ? "reader.view.generated" : "reader.view.added");
  if (m.role === ROLE_DIGEST) return t("reader.view.digest");
  if (m.role === ROLE_TEXT) return t("info.material.text");
  return t("info.material.other");
}

/**
 * 重點筆記的來歷與是否已校對：整批匯入進來的（`provenance.method` 是 `imported`）寫「匯入」，
 * 應用產生的寫「AI 產生」；是否已校對照 `proofread`。閱讀畫面的重點筆記與文獻資訊都用這一條。
 */
export function digestStateLabel(method: string | null | undefined, proofread: boolean): string {
  if (method === "imported") return t(proofread ? "digest.view.importedProofread" : "digest.view.imported");
  return t(proofread ? "digest.view.proofread" : "digest.view.unproofread");
}

/**
 * 這份材料要用其他應用打開：閱讀畫面這一版開不了的 Markdown（匯入的譯本、抽出文字）。
 * 重點筆記有自己的檢視，不算。
 */
export function opensElsewhere(m: { role: string; form: string }): boolean {
  return m.form === FORM_MARKDOWN && m.role !== ROLE_DIGEST;
}

/**
 * 有沒有譯本：只算閱讀畫面打得開的（PDF、譯文疊加檔）；匯入的 Markdown 譯本不算。
 * 首頁的「有譯本」篩選用同一條（`catalog.ts`）。
 */
export function hasTranslation(materials: ReadonlyArray<{ role: string; form: string }>): boolean {
  return materials.some((m) => m.role === ROLE_TRANSLATION && (m.form === "pdf" || m.form === FORM_OVERLAY));
}

/** 封面用的材料：原文的 PDF，而且檔案在。沒有就是 `null`（封面維持篇名首字的色塊）。 */
export function coverMaterial(p: PaperSummary): MaterialSummary | null {
  return p.materials.find((m) => m.role === ROLE_SOURCE && m.form === "pdf" && m.status === "Ok") ?? null;
}

export interface ReadPage {
  page: number;
  /** 讀到的那份材料的總頁數；不知道就是 `null`。 */
  total: number | null;
  /** 0 到 1；不知道總頁數時是 `null`。 */
  fraction: number | null;
}

/** 讀到第幾頁（頁數取讀到的那份材料）。沒有閱讀位置就是 `null`。 */
export function readPage(p: PaperSummary): ReadPage | null {
  const pos = p.progress.position;
  if (!pos) return null;
  const total = p.materials.find((m) => m.id === pos.material)?.pages ?? null;
  const fraction = total !== null && total > 0 ? Math.min(1, Math.max(0, pos.page / total)) : null;
  return { page: pos.page, total: total !== null && total > 0 ? total : null, fraction };
}

/** 「第 7／30 頁」；不知道總頁數時只寫「第 7 頁」。 */
export function pageText(rp: ReadPage): string {
  return rp.total !== null
    ? t("home.continue.pageOf", { page: rp.page, total: rp.total })
    : t("home.continue.page", { page: rp.page });
}

/** 篇名的第一個字（封面色塊用）：跳過標點與括號。 */
export function firstLetter(title: string): string {
  const m = /[\p{L}\p{N}]/u.exec(title);
  return (m ? m[0] : (Array.from(title.trim())[0] ?? "?")).toUpperCase();
}
