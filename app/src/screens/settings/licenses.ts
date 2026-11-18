// 「關於」頁的組件與授權清單：建置前由 `scripts/licenses.mjs` 從 Cargo.lock、package-lock.json 與內含的檔產生，
// 放在安裝檔的 `licenses.json`（約 1 MB，所以不打包進程式，要看時才讀，讀一次就留著）。
// Android 與 Windows 共用同一份，只有一邊有的項目標 `platforms`，畫面依 `AppInfo.platform` 篩。

import type { Platform } from "../../api/types";

/** 一項組件。 */
export interface LicenseItem {
  name: string;
  version: string;
  /** 授權（SPDX 的寫法，例如 `MIT OR Apache-2.0`；少數是說明）。 */
  license: string;
  /** 原始碼在哪裡。 */
  source: string | null;
  /** 有 MPL-2.0：原樣使用，原始碼的網址要列出來。 */
  mpl: boolean;
  /** 補充（例如「pdf.js 內附」「第一次用時下載」）。 */
  note?: string;
  /** 只在這些作業系統的安裝檔裡；沒有表示都有。 */
  platforms?: Platform[];
  /** 授權全文的代號（`texts` 的鍵）。 */
  texts: string[];
}

export type LicenseGroupId = "ui" | "rust" | "android" | "windows" | "files" | "download";

export interface LicenseData {
  app: { name: string; license: string; texts: string[] };
  groups: Array<{ id: LicenseGroupId; platforms?: Platform[]; items: LicenseItem[] }>;
  texts: Record<string, { name: string; body: string }>;
}

/** 畫面上的一組：只留這個安裝檔有的項目，`index` 是在原本那一組裡的序號（路由用）。 */
export interface VisibleGroup {
  id: LicenseGroupId;
  rows: Array<{ item: LicenseItem; index: number }>;
}

/** 這個作業系統的安裝檔內含的組與項目（`platform` 還不知道時全部列出）。沒有項目的組不列。 */
export function visibleGroups(data: LicenseData, platform: Platform | null): VisibleGroup[] {
  const shown = (p?: Platform[]) => !p || platform === null || p.includes(platform);
  return data.groups
    .filter((g) => shown(g.platforms))
    .map((g) => ({
      id: g.id,
      rows: g.items.map((item, index) => ({ item, index })).filter((r) => shown(r.item.platforms)),
    }))
    .filter((g) => g.rows.length > 0);
}

let loading: Promise<LicenseData> | null = null;

export function loadLicenses(): Promise<LicenseData> {
  loading ??= fetch("licenses.json").then((r) => {
    if (!r.ok) throw new Error(`licenses.json: ${r.status}`);
    return r.json() as Promise<LicenseData>;
  });
  // 讀失敗的不留著，下次再試
  loading.catch(() => {
    loading = null;
  });
  return loading;
}

/** 路由裡指一項的寫法：`app`，或 `群組:序號`。 */
export function findItem(data: LicenseData, key: string): (LicenseItem & { app?: boolean }) | null {
  if (key === "app") {
    return { name: data.app.name, version: "", license: data.app.license, source: null, mpl: false, texts: data.app.texts, app: true };
  }
  const [group, index] = key.split(":");
  return data.groups.find((g) => g.id === group)?.items[Number(index)] ?? null;
}
