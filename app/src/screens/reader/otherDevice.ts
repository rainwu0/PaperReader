// 打開一篇時，最新的閱讀位置是不是另一台裝置記的。
// 是的話先停在這台自己讀到的地方，再問一次「要跳過去嗎」。只比對核心算好的閱讀位置，不推算。
// 不依賴任何東西（只有型別），所以 `scripts/sync-test.mjs` 可以直接測。

import type { ReadingProgress, ReadPosition } from "../../api/types";

export interface OtherDevicePosition {
  /** 另一台記的、比這台新的位置。 */
  other: ReadPosition;
  /** 這台自己在同一份材料上最後的位置（先停在這裡）。 */
  own: ReadPosition;
}

function time(t: string): number {
  const v = Date.parse(t);
  return Number.isNaN(v) ? -Infinity : v;
}

/**
 * 最新的位置是另一台記的、在正要打開的這份材料上、而且比這台自己在這份材料上的那一筆新：交回兩者。
 * 其餘一律 `null`（照原本的做法直接停在最新的位置）：不知道這台的代號、沒有位置、是這台自己記的、在別份材料上，
 * 或這台在這份材料上沒有自己的位置——沒有要保住的地方，而且停在第 1 頁的話，離開時記下的第 1 頁會比另一台的新，
 * 另一台讀到的地方就再也問不到了（新裝置剛加入時就是這樣）。
 */
export function otherDevicePosition(
  progress: ReadingProgress | null,
  deviceId: string | null,
  materialId: string,
): OtherDevicePosition | null {
  const pos = progress?.position;
  if (!progress || !pos || !deviceId || pos.device === deviceId || pos.material !== materialId) return null;
  const mine = progress.device_positions.find((p) => p.device === deviceId);
  if (!mine || mine.material !== materialId || time(mine.t) >= time(pos.t)) return null;
  return { other: pos, own: mine };
}
