// 文獻資訊的書目那一塊：篇名、作者、年份、DOI、網址（DOI 與網址放在年份下面，空的也顯示、可以填）。
// 欄位的狀態、改過沒有、存檔時交給核心的草稿都在這裡；`PaperInfo.tsx` 只放元件、按「儲存」。
// 使用者清空年份、DOI 或網址時，核心的 `update_bib` 記進 `cleared`，自動補書目與同步都不補回。

import { useCallback, useState } from "react";
import type { Bib, BibDraft } from "../../api";
import { authorsLine, parseAuthorsLine } from "../../format";
import { t } from "../../strings";

export interface BibForm {
  title: string;
  authors: string;
  year: string;
  doi: string;
  url: string;
  set: (field: "title" | "authors" | "year" | "doi" | "url", value: string) => void;
  /** 讀回核心存好的書目（打開、存檔、重新載入之後）。 */
  reset: (bib: Bib) => void;
  /** 年份是整數（或空的）。 */
  valid: boolean;
  /** 和 `bib` 比，有沒有改過。 */
  dirty: (bib: Bib) => boolean;
  /** 存檔時交給核心的草稿（`venue` 照原樣；作者沒改就原樣交回）。 */
  draft: (bib: Bib) => BibDraft;
}

const blank = (s: string) => (s.trim() === "" ? null : s.trim());

export function useBibForm(): BibForm {
  const [v, setV] = useState({ title: "", authors: "", year: "", doi: "", url: "" });
  const set = useCallback((field: keyof typeof v, value: string) => setV((cur) => ({ ...cur, [field]: value })), []);
  const reset = useCallback(
    (bib: Bib) =>
      setV({
        title: bib.title ?? "",
        authors: authorsLine(bib.authors),
        year: bib.year != null ? String(bib.year) : "",
        doi: bib.doi ?? "",
        url: bib.url ?? "",
      }),
    [],
  );
  const yearNumber = v.year.trim() === "" ? null : Number(v.year);
  const valid = yearNumber === null || Number.isInteger(yearNumber);
  return {
    ...v,
    set,
    reset,
    valid,
    dirty: (bib) =>
      v.title !== (bib.title ?? "") ||
      v.authors !== authorsLine(bib.authors) ||
      (yearNumber ?? null) !== (bib.year ?? null) ||
      v.doi.trim() !== (bib.doi ?? "") ||
      v.url.trim() !== (bib.url ?? ""),
    draft: (bib) => ({
      title: blank(v.title),
      // 作者欄沒改就原樣交回；改了就依分號拆成幾位，「姓, 名」拆成姓與名
      authors: v.authors === authorsLine(bib.authors) ? bib.authors : parseAuthorsLine(v.authors),
      year: yearNumber,
      venue: bib.venue ?? null,
      doi: blank(v.doi),
      url: blank(v.url),
    }),
  };
}

/** 書目的欄位。 */
export function BibFields(props: { form: BibForm; disabled: boolean }) {
  const { form, disabled } = props;
  return (
    <>
      <label className="field">
        <span>{t("info.field.title")}</span>
        <input className="input" value={form.title} disabled={disabled} onChange={(e) => form.set("title", e.target.value)} />
      </label>
      <label className="field">
        <span>{t("info.field.authors")}</span>
        <input
          className="input"
          value={form.authors}
          disabled={disabled}
          placeholder={t("info.field.authorsPlaceholder")}
          onChange={(e) => form.set("authors", e.target.value)}
        />
      </label>
      <label className="field">
        <span>{t("info.field.year")}</span>
        <input
          className="input"
          inputMode="numeric"
          value={form.year}
          disabled={disabled}
          onChange={(e) => form.set("year", e.target.value)}
          style={{ maxWidth: 160 }}
        />
      </label>
      <label className="field">
        <span>{t("info.field.doi")}</span>
        <input
          className="input"
          inputMode="url"
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
          value={form.doi}
          disabled={disabled}
          placeholder={t("info.field.doiPlaceholder")}
          onChange={(e) => form.set("doi", e.target.value)}
        />
      </label>
      <label className="field">
        <span>{t("info.field.url")}</span>
        <input
          className="input"
          type="url"
          inputMode="url"
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
          value={form.url}
          disabled={disabled}
          onChange={(e) => form.set("url", e.target.value)}
        />
      </label>
    </>
  );
}
