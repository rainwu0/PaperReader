// 使用者的偏好（首頁排序、閱讀的選項）：向外殼要、改了就存進設定檔。存不進去就改回原值並通知畫面。

import { useCallback, useEffect, useRef, useState } from "react";
import { api, AppError, type Prefs } from "./api";

export const DEFAULT_PREFS: Prefs = {
  sort_key: "recent",
  sort_reverse: false,
  dark_pages: false,
  keep_awake: false,
  open_to: "home",
  toolbox_mode: "mini",
  toolbox_dock: "bottom",
  toolbox_x: 900,
  toolbox_y: 600,
  organize_paused: false,
};

export interface PrefsState {
  prefs: Prefs;
  /** 已經從設定檔讀出來了（啟動時決定先到哪裡，要等它）。 */
  loaded: boolean;
  /** 改一部分偏好。存不進設定檔時改回原值，並用 `onSaveError` 通知。 */
  update: (patch: Partial<Prefs>) => void;
}

export function usePrefs(onSaveError: (e: AppError) => void): PrefsState {
  const [prefs, setPrefs] = useState<Prefs>(DEFAULT_PREFS);
  const [loaded, setLoaded] = useState(false);
  const latest = useRef(prefs);
  const onError = useRef(onSaveError);
  useEffect(() => {
    onError.current = onSaveError;
  });

  useEffect(() => {
    api
      .getPrefs()
      .then((p) => {
        latest.current = p;
        setPrefs(p);
      })
      .catch(() => {})
      .finally(() => setLoaded(true));
  }, []);

  const update = useCallback((patch: Partial<Prefs>) => {
    const previous = latest.current;
    const next = { ...previous, ...patch };
    latest.current = next;
    setPrefs(next);
    api.setPrefs(next).catch((e: unknown) => {
      // 只退回這次改的欄位，別的欄位（可能已經又改過）不動
      const back = { ...latest.current };
      for (const k of Object.keys(patch) as Array<keyof Prefs>) {
        (back as Record<string, unknown>)[k] = previous[k];
      }
      latest.current = back;
      setPrefs(back);
      onError.current(e instanceof AppError ? e : new AppError("Unknown", ""));
    });
  }, []);

  return { prefs, loaded, update };
}
