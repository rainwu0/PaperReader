// 翻譯設定的共用規則：服務的名稱、設定完整不完整、支不支援目標語言、選單上列哪些服務。
// 要填什麼欄位、預設模型、官方主機、支援哪些目標語言、文字會不會送出裝置，都照引擎申報（`engines.ts`），
// 畫面不另外寫一份。

import type { EngineInfo, TranslateServiceView, TranslateSettingsView } from "../../api";

/** 這個引擎支不支援譯成這種語言。 */
export function supportsTarget(info: EngineInfo | null, code: string): boolean {
  return !!info && info.targets.some((l) => l.code.toLowerCase() === code.toLowerCase());
}

/** 實際用的模型（空白用引擎申報的預設）。 */
export function serviceModel(s: Pick<TranslateServiceView, "model">, info: EngineInfo | null): string {
  return s.model.trim() !== "" ? s.model.trim() : (info?.defaultModel ?? "");
}

/** 設定不完整的原因（對應文字表 `generate.confirm.why.*`）；完整交回 `null`。 */
export function missingSetting(
  s: Pick<TranslateServiceView, "model" | "endpoint" | "hasKey">,
  info: EngineInfo | null,
  secretsSupported: boolean,
): "platform" | "key" | "model" | "endpoint" | null {
  if (!info) return "key";
  if (!secretsSupported && info.key !== "no") return "platform";
  if (info.endpoint === "required" && s.endpoint.trim() === "") return "endpoint";
  if (info.model === "required" && serviceModel(s, info) === "") return "model";
  if (info.key === "required" && !s.hasKey) return "key";
  return null;
}

/**
 * 面板一開始選哪個服務（照上次的選擇直接做，另有換的入口）：這次面板上已經選了的照舊；
 * 否則上次用的服務（翻譯是設定的 `lastService`，重點筆記是 `lastDigestService`）；它不在選單上
 * （停用、刪掉，或重點筆記不能用的引擎）才用選單第一個（主要服務）。`menu` 是面板的選單（`menuServices` 的順序）。
 */
export function defaultService(
  menu: ReadonlyArray<Pick<TranslateServiceView, "id">>,
  last: string | null | undefined,
  current: string | null,
): string | null {
  const usable = (id: string | null | undefined): id is string => !!id && menu.some((s) => s.id === id);
  if (usable(current)) return current;
  if (usable(last)) return last;
  return menu[0]?.id ?? null;
}

/**
 * 「接著翻」面板的說明用哪一句：照引擎申報的「文字會不會送出裝置」。離線的不說送出；
 * 還不知道引擎時照會送出的說法。
 */
export function resumeNoteKey(info: Pick<EngineInfo, "leavesDevice"> | null): "generate.confirm.resumeNote" | "generate.confirm.resumeNote.local" {
  return info && !info.leavesDevice ? "generate.confirm.resumeNote.local" : "generate.confirm.resumeNote";
}

/**
 * 翻譯面板主按鈕的樣子：
 * - `start`：開始（或接著翻），`enabled` 照面板其餘的條件；
 * - `download`：離線引擎的模型還沒下載，按鈕改成「下載模型並翻譯」（先問，下載完自動開始）；
 * - `downloading`：模型正在下載，按鈕換成進度與停止。
 * `model` 是離線引擎的模型狀態（還沒讀到是 `null`）；`sameLang` 是要翻的語言就是模型狀態那一種
 * （接著翻舊譯本時語言照那份譯本，可能不同：不同時不提供下載）。
 */
export type StartButton = { kind: "start"; enabled: boolean } | { kind: "download"; enabled: boolean } | { kind: "downloading" };

export function startButton(p: {
  local: boolean;
  model: "ready" | "missing" | "noLang" | null;
  downloading: boolean;
  sameLang: boolean;
  /** 模型以外的條件都滿足（選了服務、設定完整、支援目標語言、已同意）。 */
  ready: boolean;
}): StartButton {
  if (!p.local) return { kind: "start", enabled: p.ready };
  if (p.downloading) return { kind: "downloading" };
  if (p.model === "missing" && p.sameLang) return { kind: "download", enabled: p.ready };
  return { kind: "start", enabled: p.ready && p.model === "ready" };
}

/** 翻譯時選單上的服務：啟用的，主要服務在最前面，接著依備用的順序，其餘照清單。 */
export function menuServices(v: Pick<TranslateSettingsView, "services" | "primary" | "fallbacks">): TranslateServiceView[] {
  const rank = (id: string) => {
    if (id === v.primary) return 0;
    const i = v.fallbacks.indexOf(id);
    return i >= 0 ? 1 + i : 100;
  };
  return v.services
    .filter((s) => s.enabled)
    .map((s, i) => ({ s, i }))
    .sort((a, b) => rank(a.s.id) - rank(b.s.id) || a.i - b.i)
    .map((x) => x.s);
}

/** 「跟全文翻譯相同」實際是哪一個服務：翻譯面板一打開會選的那一個（上次用的，否則主要服務）。 */
export function fullTranslationService(
  v: Pick<TranslateSettingsView, "services" | "primary" | "fallbacks" | "lastService">,
): string | null {
  return defaultService(menuServices(v), v.lastService, null);
}

/**
 * 快速翻譯實際用哪一個服務：設定指定了、而且還在翻譯選單上（啟用著）的那一個；
 * 否則跟全文翻譯相同。
 */
export function quickServiceId(
  v: Pick<TranslateSettingsView, "services" | "primary" | "fallbacks" | "lastService" | "quickService">,
): string | null {
  const menu = menuServices(v);
  const q = v.quickService;
  return q && menu.some((s) => s.id === q) ? q : fullTranslationService(v);
}

/**
 * 快速翻譯的同意（送出前的詢問只問一次）存在設定檔的 `quickConsent`。
 * 舊版（0.1.4 之前的測試版）記在網頁的本機儲存這個鍵下，讀到時搬進設定檔再刪掉。
 */
export const LEGACY_QUICK_CONSENT_KEY = "pr-quick-consent";
export const isStringList = (v: unknown): v is string[] => Array.isArray(v) && v.every((x) => typeof x === "string");

/** 兩份同意清單合在一起（去掉重複，原有的順序在前）。 */
export function mergeConsents(current: readonly string[], legacy: readonly string[]): string[] {
  return [...new Set([...current, ...legacy.filter((x) => x !== "")])];
}

/**
 * 服務網址是 `http://`（不加密）而且不是本機、模擬器或私人網段：金鑰與論文的文字會以明文送出。
 * 本機的 Ollama（`http://10.0.2.2:11434`、`http://192.168.x.x`）是正常用法，不警告。
 */
export function isInsecureRemote(endpoint: string): boolean {
  const e = endpoint.trim();
  if (!/^http:\/\//i.test(e)) return false;
  let host = "";
  try {
    host = new URL(e).hostname.toLowerCase();
  } catch {
    return false;
  }
  if (host === "localhost" || host.endsWith(".localhost") || host === "[::1]" || host === "::1") return false;
  const m = /^(\d+)\.(\d+)\.(\d+)\.(\d+)$/.exec(host);
  if (m) {
    const a = Number(m[1]);
    const b = Number(m[2]);
    if (a === 127 || a === 10) return false;
    if (a === 192 && b === 168) return false;
    if (a === 172 && b >= 16 && b <= 31) return false;
    if (a === 169 && b === 254) return false;
  }
  if (host.endsWith(".local")) return false;
  return true;
}
