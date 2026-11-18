// 打開文獻庫後在背景做的快速檢查：沒有問題什麼都不顯示，有問題才在首頁上方說一行。
// 檢查只讀不寫。快速檢查很快；只在文獻庫就緒時做一次，之後回到前景時（別的工具可能動過檔案）再做，
// 但 30 秒內不重複（檢查時握著文獻庫的鎖，不要擋住使用者正在做的事）。
// 檢查本身失敗（例如讀不了）不打擾使用者：下一次再試。

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Problem } from "./api";

const MIN_GAP_MS = 30_000;

export interface Checks {
  problems: Problem[];
  /** 做一次檢查。`force` 為真時不理會間隔（使用者按「再檢查一次」）。 */
  run: (force?: boolean) => Promise<void>;
}

export function useBackgroundCheck(ready: boolean): Checks {
  const [problems, setProblems] = useState<Problem[]>([]);
  const last = useRef(0);
  const running = useRef(false);

  const run = useCallback(async (force = false) => {
    if (running.current) return;
    if (!force && Date.now() - last.current < MIN_GAP_MS) return;
    running.current = true;
    try {
      setProblems(await api.check(false));
      last.current = Date.now();
    } catch {
      // 檢查本身失敗不算使用者的問題；留著原本的結果，下一次再試
    } finally {
      running.current = false;
    }
  }, []);

  // 文獻庫就緒之後稍等一下（讓首頁的目錄先顯示出來），再檢查
  useEffect(() => {
    if (!ready) {
      setProblems([]);
      return;
    }
    const id = window.setTimeout(() => void run(), 600);
    return () => window.clearTimeout(id);
  }, [ready, run]);

  return { problems, run };
}
