// 安裝失敗時畫面說的話與「複製錯誤資訊」的內容。
// 只用平台轉接器存下的那一筆紀錄組字，不送出任何地方。純函式，`scripts/update-report-test.mjs` 測。

import type { InstallOutcome, InstallReport } from "../../api/types.ts";
import { updateStrings } from "../../strings/update.ts";

type Key = keyof typeof updateStrings;

function say(key: Key, params?: Record<string, string | number>): string {
  const text: string = updateStrings[key];
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (_, name: string) => String(params[name] ?? ""));
}

/** `PackageInstaller.STATUS_*` 的名稱（照原文，回報時好對照 Android 文件）。 */
const STATUS_NAMES: Record<number, string> = {
  [-1]: "STATUS_PENDING_USER_ACTION",
  0: "STATUS_SUCCESS",
  1: "STATUS_FAILURE",
  2: "STATUS_FAILURE_BLOCKED",
  3: "STATUS_FAILURE_ABORTED",
  4: "STATUS_FAILURE_INVALID",
  5: "STATUS_FAILURE_CONFLICT",
  6: "STATUS_FAILURE_STORAGE",
  7: "STATUS_FAILURE_INCOMPATIBLE",
  8: "STATUS_FAILURE_TIMEOUT",
};

export function statusName(status: number): string {
  return STATUS_NAMES[status] ?? `STATUS_${status}`;
}

/** 失敗的種類（`waiting`、`none` 以外的結果）。 */
export type FailureOutcome = Exclude<InstallOutcome, "waiting" | "none">;

export function isFailure(o: InstallOutcome): o is FailureOutcome {
  return o !== "waiting" && o !== "none";
}

/** 白話的那一句。 */
export function headline(o: FailureOutcome): string {
  return say(`update.fail.${o}` as Key);
}

/** 白話下面的補充：中止與取消都可能是對方，照實說。 */
export function note(o: FailureOutcome): string {
  if (o === "cancelled") return say("update.fail.cancelled.note");
  if (o === "aborted") return `${say("update.fail.next")}${say("update.fail.aborted.note")}`;
  if (o === "stalled") return `${say("update.fail.stalled.note")}${say("update.fail.next")}`;
  return say("update.fail.next");
}

/** 小字：系統的原話與代碼。 */
export function systemLines(r: InstallReport): string[] {
  const msg = r.statusMessage?.trim();
  return [
    msg ? say("update.fail.message", { message: msg }) : say("update.fail.noMessage"),
    say("update.fail.code", { name: statusName(r.status), status: r.status }),
  ];
}

/** 當地時間，帶時差（`2026-11-18 10:23:45 +08:00`）。 */
export function localTime(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(Math.abs(n)).padStart(2, "0");
  const off = -d.getTimezoneOffset();
  const sign = off >= 0 ? "+" : "-";
  return (
    `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ` +
    `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())} ` +
    `${sign}${p(Math.trunc(off / 60))}:${p(off % 60)}`
  );
}

function or(v: string | null | undefined, empty: Key = "update.report.none"): string {
  const s = v?.trim();
  return s ? s : say(empty);
}

function gap(from: number | null | undefined, to: number): string {
  return typeof from === "number" ? say("update.report.ms", { ms: to - from }) : say("update.report.unknown");
}

/** 小米的系統（和外殼 `commands/update.rs` 的 `is_xiaomi` 同一個判斷）。 */
function isXiaomi(d: InstallReport["device"]): boolean {
  return !!d.miuiVersion?.trim() || !!d.hyperOsVersion?.trim() || d.manufacturer.toLowerCase() === "xiaomi";
}

function optimization(d: InstallReport["device"]): string {
  const raw = d.miuiOptimization;
  if (!isXiaomi(d)) return say("update.report.notXiaomi");
  if (raw === null || raw === undefined) return say("update.report.unknown");
  const v = raw.trim();
  const state = v === "0" || v === "false" ? say("update.report.off") : say("update.report.on");
  return `${state}（persist.sys.miui_optimization=${v}）`;
}

/**
 * 「複製錯誤資訊」的純文字。`target`：要裝的那一版的版號（畫面知道，紀錄只有 versionCode）。
 * 只放診斷要用的：版本、系統的訊息與代碼、確認畫面屬於誰、時間差、廠牌型號與系統版本、MIUI 的屬性、安裝者紀錄。
 */
export function reportText(r: InstallReport, o: FailureOutcome, target: string | null): string {
  const d = r.device;
  const installed =
    r.installedName || r.installedCode ? `${or(r.installedName)}（${r.installedCode ?? "?"}）` : say("update.report.unknown");
  const targetText =
    target || (r.targetCode ?? 0) > 0 ? `${target ?? "?"}（${(r.targetCode ?? 0) > 0 ? r.targetCode : "?"}）` : say("update.report.unknown");
  const confirm = r.confirmPackage || r.confirmComponent
    ? [r.confirmPackage, r.confirmComponent && r.confirmComponent !== r.confirmPackage ? r.confirmComponent : null]
        .filter(Boolean)
        .join("，")
    : say("update.report.none");
  const installer = r.installer
    ? r.initiator && r.initiator !== r.installer
      ? say("update.report.initiator", { installer: r.installer, initiator: r.initiator })
      : r.installer
    : say("update.report.unknown");
  const unknownSources =
    r.canRequestInstalls === true ? say("update.report.yes") : r.canRequestInstalls === false ? say("update.report.no") : say("update.report.unknown");
  const lines = [
    say("update.report.title"),
    say("update.report.time", { value: localTime(r.at) }),
    say("update.report.installed", { value: installed }),
    say("update.report.target", { value: targetText }),
    say("update.report.result", { value: `${statusName(r.status)}（${r.status}），${headline(o)}` }),
    say("update.report.message", { value: or(r.statusMessage) }),
    say("update.report.legacy", { value: typeof r.legacyStatus === "number" ? String(r.legacyStatus) : say("update.report.none") }),
    say("update.report.confirm", { value: confirm }),
    say("update.report.timing", { confirm: gap(r.confirmAt, r.at), commit: gap(r.commitAt, r.at) }),
    say("update.report.device", { value: `${or(d.manufacturer, "update.report.unknown")} ${or(d.model, "update.report.unknown")}（${or(d.brand, "update.report.unknown")}）` }),
    say("update.report.android", { value: `${or(d.release, "update.report.unknown")}（API ${d.sdk || "?"}）` }),
    say("update.report.build", { value: `${or(d.display, "update.report.unknown")}；${or(d.incremental, "update.report.unknown")}` }),
    say("update.report.rom", {
      miui: d.miuiVersion === undefined || d.miuiVersion === null ? say("update.report.unknown") : or(d.miuiVersion),
      hyper:
        d.hyperOsVersion === undefined || d.hyperOsVersion === null ? say("update.report.unknown") : or(d.hyperOsVersion),
    }),
    say("update.report.optimization", { value: optimization(d) }),
    say("update.report.installer", { value: installer }),
    say("update.report.unknownSources", { value: unknownSources }),
  ];
  return lines.join("\n");
}
