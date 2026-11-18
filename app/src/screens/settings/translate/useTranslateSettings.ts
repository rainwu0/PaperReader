// 翻譯設定的讀取與修改（設定的翻譯各畫面共用）。改設定一律只送改了的欄位（`translate_settings_patch`），
// 不拿畫面上的舊資料整份蓋回去。

import { useCallback, useEffect, useRef, useState } from "react";
import { api, kindOf, type ErrorKind, type TranslateSettingsPatch, type TranslateSettingsView } from "../../../api";

// 閱讀畫面的並排比較還從這裡取 `kindOf`（那個檔留給譯文檢視那一批改）
export { kindOf };

export interface TranslateSettingsState {
  view: TranslateSettingsView | null;
  /** 上一次讀取或修改失敗的原因。 */
  error: ErrorKind | null;
  reload: () => Promise<void>;
  /** 改一部分設定，成功後重新讀一次。交回有沒有成功。 */
  patch: (p: TranslateSettingsPatch) => Promise<boolean>;
  /** 執行一個修改的動作（新增、刪除服務……），之後重新讀一次。交回有沒有成功。 */
  run: (f: () => Promise<unknown>) => Promise<boolean>;
}

export function useTranslateSettings(): TranslateSettingsState {
  const [view, setView] = useState<TranslateSettingsView | null>(null);
  const [error, setError] = useState<ErrorKind | null>(null);
  const alive = useRef(true);

  const reload = useCallback(async () => {
    try {
      const v = await api.translateSettings();
      if (alive.current) setView(v);
    } catch (e) {
      if (alive.current) setError(kindOf(e));
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    void reload();
    return () => {
      alive.current = false;
    };
  }, [reload]);

  const run = useCallback(
    async (f: () => Promise<unknown>) => {
      try {
        await f();
        if (alive.current) setError(null);
        await reload();
        return true;
      } catch (e) {
        if (alive.current) setError(kindOf(e));
        await reload();
        return false;
      }
    },
    [reload],
  );

  const patch = useCallback((p: TranslateSettingsPatch) => run(() => api.patchTranslateSettings(p)), [run]);

  return { view, error, reload, patch, run };
}
