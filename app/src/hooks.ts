import { useCallback, useContext, useEffect, useRef, useState } from "react";
import { api, AppError, type Theme } from "./api";
import { NavContext, type Nav, type Route } from "./nav";

/**
 * 一個自己管開關的面板，打開時在導覽堆疊推一層（`sheet` 的 `ext`），系統返回鍵先關掉它。給沒有自己的
 * `SheetKind` 的地方用（外掛在文獻資訊的一節裡開的選單與挑一篇）。
 */
export function useStackLayer(): { open: boolean; show: () => void; hide: () => Promise<void> } {
  const nav = useContext(NavContext);
  const [open, setOpen] = useState(false);
  const depth = useRef(0);
  /** 推了一層、還沒在堆疊上看到它。深度等看到了才記：呼叫 `show` 的可能是舊的一次渲染（例如先關另一個面板再開這個），
   *  那時記下的堆疊長度已經不對。 */
  const armed = useRef(false);
  const stackLen = nav?.stack.length ?? 0;
  const topIsLayer = nav?.top.name === "sheet" && nav.top.kind === "ext";
  useEffect(() => {
    if (!open) return;
    if (armed.current) {
      if (topIsLayer) {
        depth.current = stackLen;
        armed.current = false;
      }
      return;
    }
    // 返回鍵把這一層退掉了：跟著關
    if (stackLen < depth.current) setOpen(false);
  }, [open, stackLen, topIsLayer]);
  const show = useCallback(() => {
    if (nav) {
      armed.current = true;
      nav.push({ name: "sheet", kind: "ext" });
    }
    setOpen(true);
  }, [nav]);
  const hide = useCallback(async () => {
    setOpen(false);
    if (nav && nav.top.name === "sheet" && nav.top.kind === "ext") await nav.back();
  }, [nav]);
  return { open, show, hide };
}

/**
 * 疊在這個畫面上的另一個畫面退掉、這個畫面又回到最上面時呼叫 `cb`（面板不算疊上去）。
 * 例如閱讀畫面從產生畫面、文獻資訊回來時重讀材料清單。
 */
export function useBackOnTop(nav: Nav, screen: Route["name"], cb: () => void) {
  const covered = [...nav.stack].reverse().find((r) => r.name !== "sheet")?.name !== screen;
  const was = useRef(covered);
  const ref = useRef(cb);
  useEffect(() => {
    ref.current = cb;
  });
  useEffect(() => {
    if (was.current && !covered) ref.current();
    was.current = covered;
  }, [covered]);
}

/** 視窗的寬高（旋轉、分割畫面、鍵盤彈出時跟著變）。 */
export function useViewportSize(): { w: number; h: number } {
  const [size, setSize] = useState({ w: window.innerWidth, h: window.innerHeight });
  useEffect(() => {
    const on = () => setSize({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener("resize", on);
    return () => window.removeEventListener("resize", on);
  }, []);
  return size;
}

/**
 * 這台裝置上的介面偏好（記在網頁的本機儲存，不進文獻庫與設定檔）：讀不到、讀不懂、不合 `valid` 時用 `fallback`。
 * 存不進去只會讓下次回到預設，不影響使用。
 */
export function readLocal<T>(key: string, fallback: T, valid: (v: unknown) => v is T): T {
  try {
    const v: unknown = JSON.parse(window.localStorage.getItem(key) ?? "null");
    return valid(v) ? v : fallback;
  } catch {
    return fallback;
  }
}

/** 某一項本機偏好改了（`detail` 是鍵）：同一項偏好在別的畫面（疊著的畫面都留著）也跟著換。 */
const LOCAL_PREF_CHANGED = "pr-local-pref";

export function writeLocal(key: string, value: unknown): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // 同上：下次用預設
  }
}

/** `readLocal` 加上狀態：改了就記住；別的畫面用同一個鍵改了，這裡也跟著換（例如設定頁與工具箱的圖釘）。 */
export function useLocalPref<T>(key: string, fallback: T, valid: (v: unknown) => v is T): [T, (next: T) => void] {
  const [value, setValue] = useState<T>(() => readLocal(key, fallback, valid));
  const latest = useRef({ fallback, valid });
  useEffect(() => {
    latest.current = { fallback, valid };
  });
  useEffect(() => {
    const on = (e: Event) => {
      if ((e as CustomEvent<string>).detail !== key) return;
      setValue(readLocal(key, latest.current.fallback, latest.current.valid));
    };
    window.addEventListener(LOCAL_PREF_CHANGED, on);
    return () => window.removeEventListener(LOCAL_PREF_CHANGED, on);
  }, [key]);
  const set = useCallback(
    (next: T) => {
      setValue(next);
      writeLocal(key, next);
      window.dispatchEvent(new CustomEvent(LOCAL_PREF_CHANGED, { detail: key }));
    },
    [key],
  );
  return [value, set];
}

export const isBool = (v: unknown): v is boolean => typeof v === "boolean";

/** 回到前景（切回應用、關螢幕後解鎖、從系統設定頁回來）時呼叫 `cb`。 */
export function useForeground(cb: () => void) {
  const ref = useRef(cb);
  useEffect(() => {
    ref.current = cb;
  });
  useEffect(() => {
    let last = 0;
    const fire = () => {
      const now = Date.now();
      if (now - last < 400) return; // 同一次回到前景可能觸發多個事件
      last = now;
      ref.current();
    };
    const onVisibility = () => {
      if (document.visibilityState === "visible") fire();
    };
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("focus", fire);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("focus", fire);
    };
  }, []);
}

/** 畫面還在不在（非同步的工作做完時，畫面已經關掉就不再更新）。 */
export function useAlive() {
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  return alive;
}

/**
 * `active` 期間每隔一下做一次 `tick`：第一次等 `first` 毫秒，之後每 `every` 毫秒（上一次做完才算下一次）。
 * `tick` 交回 `false` 就停。外殼沒有進度事件，背景工作的進度都靠畫面來問。
 */
export function usePoll(active: boolean, tick: () => Promise<boolean | void>, every: number, first = every) {
  const ref = useRef(tick);
  useEffect(() => {
    ref.current = tick;
  });
  useEffect(() => {
    if (!active) return;
    let stop = false;
    let timer = 0;
    const run = async () => {
      if (stop) return;
      const more = await ref.current().catch(() => false);
      if (!stop && more !== false) timer = window.setTimeout(() => void run(), every);
    };
    timer = window.setTimeout(() => void run(), first);
    return () => {
      stop = true;
      window.clearTimeout(timer);
    };
  }, [active, every, first]);
}

/**
 * 讀出並套用深淺色設定：跟隨系統、淺色、深色。
 * 存不進設定檔時改回原本的選擇，並用 `onSaveError` 通知畫面（不假裝存好了）。
 */
export function useTheme(onSaveError: (e: AppError) => void): [Theme, (t: Theme) => void] {
  const [theme, setThemeState] = useState<Theme>("system");

  useEffect(() => {
    api
      .getTheme()
      .then(setThemeState)
      .catch(() => {});
  }, []);

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && mq.matches);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
      // 狀態列與手勢列的圖示配合明暗；非 Android 時沒有這個功能，忽略
      api.systemBars(dark).catch(() => {});
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);

  const set = (next: Theme) => {
    const previous = theme;
    setThemeState(next);
    api.setTheme(next).catch((e: unknown) => {
      setThemeState(previous);
      onSaveError(e instanceof AppError ? e : new AppError("Unknown", ""));
    });
  };
  return [theme, set];
}
