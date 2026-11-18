import { zhTW, type StringKey } from "./zh-TW";
import type { ErrorKind } from "../api";
import { problemText } from "./problem";

export type { StringKey };

// 目前只有繁體中文。加語言時在這裡登記，並依系統語言選一份。
const dictionary: Record<StringKey, string> = zhTW;

/** 文字裡的 `{name}` 換成 `params` 裡的值（應用的文字表與外掛自己的文字共用）。 */
export function fill(text: string, params?: Record<string, string | number>): string {
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (_, name: string) => String(params[name] ?? ""));
}

/** 依代碼取出文字；`{name}` 換成 `params` 裡的值。 */
export function t(key: StringKey, params?: Record<string, string | number>): string {
  return fill(dictionary[key], params);
}

// 兩種「較新版本建立的」說的是同一件事，用同一句
const SAME_AS: Partial<Record<ErrorKind, StringKey>> = {
  FormatTooNew: "common.readOnly",
  ReadOnly: "common.readOnly",
};

function lookup(key: string): string | undefined {
  return Object.hasOwn(dictionary, key) ? dictionary[key as StringKey] : undefined;
}

/** 文字表裡有沒有這個鍵（核心之後可能新增種類，畫面用它決定要不要改用通用的一句）。 */
export function hasText(key: string): key is StringKey {
  return lookup(key) !== undefined;
}

/** 錯誤種類對應的表面那一句：發生了什麼。 */
export function errorMessage(kind: ErrorKind): string {
  const alias = SAME_AS[kind];
  return alias ? t(alias) : (lookup(`error.${kind}`) ?? t("error.Unknown"));
}

/** 錯誤的原因與細節，放在「詳情」裡；沒有就是 `undefined`。 */
export function errorDetail(kind: ErrorKind): string | undefined {
  return lookup(`error.${kind}.detail`);
}

/** 出錯的原因（放「詳情」）：表面那一句加上細節。 */
export function errorReason(kind: ErrorKind): string {
  return [errorMessage(kind), errorDetail(kind)].filter(Boolean).join(" ");
}

/** 檢查找到的一種問題的白話說法；這個版本不認得的種類用通用的一句（`problem.ts`）。 */
export function problemMessage(kind: string): string {
  return problemText(dictionary, kind);
}
