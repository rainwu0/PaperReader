// 應用內檢查更新在畫面這一側的狀態：首頁的提示、設定的那一列、更新頁共用一份。
// 連不連網由外殼決定（沒同意過一律不連；自動檢查一天最多一次），這裡只問狀態、在打開應用時請外殼看要不要查。

import { useSyncExternalStore } from "react";
import { api, type UpdateStatus } from "./api";

let status: UpdateStatus | null = null;
const listeners = new Set<() => void>();

function emit() {
  for (const l of listeners) l();
}

export const updates = {
  subscribe(l: () => void): () => void {
    listeners.add(l);
    return () => listeners.delete(l);
  },
  get(): UpdateStatus | null {
    return status;
  },
  /** 向外殼重新要一次狀態。 */
  async refresh(): Promise<UpdateStatus | null> {
    try {
      status = await api.updateStatus();
    } catch {
      status = null;
    }
    emit();
    return status;
  },
  /** 打開應用時：同意過、自動檢查開著，外殼才會在超過一天時連網；其餘交回已知的結果。失敗不打擾使用者。 */
  async startup(): Promise<void> {
    const s = await updates.refresh();
    if (!s?.supported || !s.consented || !s.autoCheck) return;
    await api.updateCheck(false).catch(() => null);
    await updates.refresh();
  },
};

export function useUpdates(): UpdateStatus | null {
  return useSyncExternalStore(updates.subscribe, updates.get);
}
