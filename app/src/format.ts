// 只給畫面顯示用的整理：把資料變成要顯示的字。不含資料規則（閱讀狀態、位置都由核心算好）。

import type { Author, Bib } from "./api";
import { t } from "./strings";

export function displayTitle(bib: Bib, fallback?: string): string {
  const title = bib.title?.trim();
  if (title) return title;
  const f = fallback?.trim();
  return f ? f : t("common.untitled");
}

function authorName(a: Author): string {
  return (a.family ?? a.literal ?? a.given ?? "").trim();
}

/** 「Zhao 等 2024」這樣的一行：第一作者、年份。 */
export function authorYear(bib: Bib): string {
  const first = bib.authors.map(authorName).find((n) => n !== "") ?? "";
  const more = bib.authors.length > 1 ? t("format.etAl") : "";
  const year = bib.year != null ? String(bib.year) : "";
  return [first ? first + more : "", year].filter(Boolean).join(" ");
}

/** 作者欄的文字：多位作者用分號隔開。 */
export function authorsLine(authors: Author[]): string {
  return authors
    .map((a) => {
      if (a.literal?.trim()) return a.literal.trim();
      return [a.family, a.given]
        .map((x) => x?.trim())
        .filter(Boolean)
        .join(", ");
    })
    .filter((s) => s !== "")
    .join("; ");
}

/** 作者欄的文字拆回作者：分號隔開多位；「姓, 名」拆成姓與名（引用格式才排得對），其餘整串當一個姓名。 */
export function parseAuthorsLine(line: string): Author[] {
  return line
    .split(/[;；]/)
    .map((s) => s.trim())
    .filter((s) => s !== "")
    .map((s) => {
      const m = /^([^,，]+)[,，]\s*(.+)$/.exec(s);
      return m ? { family: m[1]!.trim(), given: m[2]!.trim() } : { literal: s };
    });
}

/** 那一天的開始（手機的時區）。 */
function startOf(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** 相差幾天（照日曆算，不是照 24 小時）。 */
function daysBetween(then: Date, now: Date): number {
  return Math.round((startOf(now) - startOf(then)) / 86_400_000);
}

function dateOf(d: Date): string {
  return t("time.date", { y: d.getFullYear(), m: d.getMonth() + 1, d: d.getDate() });
}

/** 日期：「2026 年 11 月 15 日」。沒有或讀不懂是空字串。 */
export function shortDate(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : dateOf(d);
}

/** 「今天」「昨天」「3 天前」。`iso` 是核心給的世界協調時間字串。 */
export function relativeDay(iso: string | null | undefined, now: Date = new Date()): string {
  if (!iso) return "";
  const then = new Date(iso);
  if (Number.isNaN(then.getTime())) return "";
  const days = daysBetween(then, now);
  if (days <= 0) return t("time.today");
  if (days === 1) return t("time.yesterday");
  if (days <= 7) return t("time.daysAgo", { n: days });
  // 超過一週就寫日期，比「很久以前」清楚
  return dateOf(then);
}

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

/** 依手機的時區算「哪一天」，給閱讀紀錄分組用：`2026-11-15`。 */
export function dayKey(iso: string): string {
  const d = new Date(iso);
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}

/** 分組標題：今天、昨天，更早的寫日期。 */
export function dayHeading(iso: string, now: Date = new Date()): string {
  const then = new Date(iso);
  const days = daysBetween(then, now);
  if (days <= 0) return t("time.today");
  if (days === 1) return t("time.yesterday");
  return dateOf(then);
}

/** 一天裡的時間：`08:05`（手機的時區）。 */
export function timeOfDay(iso: string): string {
  const d = new Date(iso);
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

/** 檔名去掉副檔名 `.pdf`（不分大小寫）。加入時的預設篇名就是這個。 */
export function nameWithoutPdf(name: string): string {
  return name.toLowerCase().endsWith(".pdf") ? name.slice(0, -4) : name;
}
