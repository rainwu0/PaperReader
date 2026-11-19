// 快速翻譯的同意搬家：舊版記在網頁的本機儲存，現在存進設定檔。
// 讀到舊紀錄就搬過去一次（和設定檔裡已有的合併），搬成功才刪舊紀錄；搬不成功下次再試。

import { api, type TranslateSettingsView } from "../../api";
import { isStringList, LEGACY_QUICK_CONSENT_KEY, mergeConsents } from "./translateConfig";

/** 舊的本機紀錄：沒有、讀不懂都當作沒有（`null` 是沒有要搬的東西）。 */
function readLegacy(): string[] | null {
  try {
    const raw = window.localStorage.getItem(LEGACY_QUICK_CONSENT_KEY);
    if (raw === null) return null;
    const v: unknown = JSON.parse(raw);
    return isStringList(v) ? v : [];
  } catch {
    return null;
  }
}

function dropLegacy(): void {
  try {
    window.localStorage.removeItem(LEGACY_QUICK_CONSENT_KEY);
  } catch {
    // 刪不掉也沒關係：下次再搬一次是同樣的結果
  }
}

/** 設定讀進來之後呼叫：有舊紀錄就搬進設定檔，交回搬完的設定。 */
export async function migrateLegacyConsent(v: TranslateSettingsView): Promise<TranslateSettingsView> {
  const legacy = readLegacy();
  if (legacy === null) return v;
  const merged = mergeConsents(v.quickConsent, legacy);
  if (merged.length === v.quickConsent.length) {
    dropLegacy();
    return v;
  }
  try {
    await api.patchTranslateSettings({ quickConsent: merged });
  } catch {
    return v;
  }
  dropLegacy();
  return { ...v, quickConsent: merged };
}
