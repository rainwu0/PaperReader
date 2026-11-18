// 匯入同步包的預覽在畫面這一側要做的整理：彙總、預設的選擇、送出的選擇、提醒分組。
// 只整理核心交來的資料，不推算合併規則。不依賴任何東西（只有型別），
// 所以 `scripts/sync-test.mjs` 可以直接測。

import type {
  ConflictChoice,
  ItemKind,
  NoticeKind,
  SyncConflict,
  SyncItem,
  SyncNotice,
  SyncPreview,
} from "../../api/types";

/** 彙總時種類的順序：文獻在前，細項在後。 */
export const ITEM_ORDER: readonly ItemKind[] = [
  "Paper",
  "Material",
  "Bib",
  "Highlight",
  "Thoughts",
  "Stroke",
  "Overlay",
  "Group",
  "Groups",
  "Pages",
  "NotesFile",
  "References",
  "TrashState",
];

export interface KindTotal {
  kind: ItemKind;
  count: number;
}

/** 依種類加總（不分篇），照 `ITEM_ORDER`；這個版本不認得的種類排在最後。數目是 0 的不列。 */
export function totals(items: readonly SyncItem[]): KindTotal[] {
  const sum = new Map<ItemKind, number>();
  for (const it of items) sum.set(it.kind, (sum.get(it.kind) ?? 0) + it.count);
  const rank = (k: ItemKind) => {
    const i = ITEM_ORDER.indexOf(k);
    return i < 0 ? ITEM_ORDER.length : i;
  };
  return [...sum.entries()]
    .filter(([, n]) => n > 0)
    .sort((a, b) => rank(a[0]) - rank(b[0]))
    .map(([kind, count]) => ({ kind, count }));
}

/** 每個衝突的預設選擇（核心給的 `default`）。 */
export function defaultChoices(conflicts: readonly SyncConflict[]): Record<string, ConflictChoice> {
  const out: Record<string, ConflictChoice> = {};
  for (const c of conflicts) out[c.id] = c.default;
  return out;
}

/**
 * 送出的選擇：預覽裡的每一個衝突一個（核心要求一個都不能少、也不能多），
 * 使用者沒選或選了這個衝突不提供的選項，用預設。
 */
export function choicesToSend(
  conflicts: readonly SyncConflict[],
  picked: Readonly<Record<string, ConflictChoice>>,
): Array<[string, ConflictChoice]> {
  return conflicts.map((c) => {
    const p = picked[c.id];
    return [c.id, p !== undefined && c.options.includes(p) ? p : c.default];
  });
}

/** 預覽裡沒有任何要合併的變更（也沒有衝突）。 */
export function isEmptyPreview(p: SyncPreview): boolean {
  return p.added.length === 0 && p.updated.length === 0 && p.removed.length === 0 && p.conflicts.length === 0;
}

export interface NoticeGroup {
  kind: NoticeKind;
  /** 文字表的鍵（`sync.notice.<key>`）：通常就是 `kind`；合起來太大的抽出的文字是 `TooLargeToMerge.text`。 */
  key: string;
  /** 這一種提醒有幾則。 */
  count: number;
  /** 帶得出路徑的（例如沒有覆寫的檔、衝突副本），依原順序、不重複。 */
  paths: string[];
}

/**
 * 照目前的選擇還會出現的提醒。核心在提醒上標出它只因為哪幾個衝突選了「兩個都留」才會出現（`conflicts`，
 * 例如衝突副本）；那些衝突目前都不是「兩個都留」時就不會有，隱藏。沒有標的一定會出現。
 */
export function visibleNotices(
  notices: readonly SyncNotice[],
  conflicts: readonly SyncConflict[],
  picked: Readonly<Record<string, ConflictChoice>>,
): SyncNotice[] {
  return notices.filter((n) => {
    const ids = n.conflicts ?? [];
    if (ids.length === 0) return true;
    return ids.some((id) => {
      const c = conflicts.find((x) => x.id === id);
      if (!c) return true;
      return (picked[id] ?? c.default) === "Both";
    });
  });
}

/** 一則提醒用哪一句：合起來太大的，指向 `text.json` 的是抽出的文字，其餘是參考文獻清單。 */
function noticeKey(n: SyncNotice): string {
  if (n.kind === "TooLargeToMerge" && n.path?.endsWith("text.json")) return "TooLargeToMerge.text";
  return n.kind;
}

/** 提醒依種類（同一句）分組：同一種只說一次，數目與路徑放在一起。順序照第一次出現。 */
export function groupNotices(notices: readonly SyncNotice[]): NoticeGroup[] {
  const groups = new Map<string, NoticeGroup>();
  for (const n of notices) {
    const key = noticeKey(n);
    const g = groups.get(key) ?? { kind: n.kind, key, count: 0, paths: [] };
    g.count++;
    if (n.path && !g.paths.includes(n.path)) g.paths.push(n.path);
    groups.set(key, g);
  }
  return [...groups.values()];
}

/** 衝突的兩邊各提供哪一個選項（並排的兩張卡片）；「兩個都留」另外一列。 */
export function sideOptions(c: SyncConflict): { local: boolean; incoming: boolean; both: boolean } {
  return {
    local: c.options.includes("Local"),
    incoming: c.options.includes("Incoming"),
    both: c.options.includes("Both"),
  };
}
