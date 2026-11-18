// 設定裡的備份（F1）：匯出備份檔、從備份檔還原。
//
// 匯出：忙碌提示 → 交出面板（`ui/DeliverSheet.tsx`）說明匯出了什麼（沒放進去的檔列出原因）→ 「存到…」或「分享」。
// 從備份檔還原：確認面板（目前的文獻庫是空的：「建立文獻庫」；不是空的：「做出新的、換過去、目前的原樣留著」）
// → 選檔 → 忙碌提示 → 結果面板（不是空的才提示原本的還在）。出錯用 App 的訊息面板（`notify`）說。
// 面板都是導覽堆疊的一層（`nav.ts`），返回鍵先關掉它。忙碌時攔住返回，不讓使用者在中途離開。
// 離開設定畫面之後才做完的事，只更新目錄，不再開面板（那時面板已沒有畫面可以掛）。

import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, kindOf, type ExportResult, type RestoreResult } from "../../api";
import { useAlive } from "../../hooks";
import type { Nav, SheetKind } from "../../nav";
import { errorDetail, errorMessage, t, type StringKey } from "../../strings";
import { AnimatePresence } from "../../ui/motion";
import { BusyOverlay, ConfirmSheet, Details, ResultSheet } from "../../ui/components";
import { DeliverSheet } from "../../ui/DeliverSheet";
import type { NoticeMore } from "../add/useAdd";

const SKIP_KEYS: Record<string, StringKey> = {
  "non-utf8-name": "backup.skip.non-utf8-name",
  "unsafe-path": "backup.skip.unsafe-path",
  "case-duplicate": "backup.skip.case-duplicate",
  symlink: "backup.skip.symlink",
  "not-a-file": "backup.skip.not-a-file",
  unreadable: "backup.skip.unreadable",
};

/** 沒放進備份檔的原因代碼 → 白話；不認得的代碼用通用的一句。 */
function skipReason(reason: string): string {
  return t(SKIP_KEYS[reason] ?? "backup.skip.other");
}

export function formatBytes(n: number): string {
  const num = (v: number) => (v < 10 ? v.toFixed(1) : String(Math.round(v)));
  if (n < 1024) return t("backup.size.b", { n });
  if (n < 1024 * 1024) return t("backup.size.kb", { n: num(n / 1024) });
  if (n < 1024 * 1024 * 1024) return t("backup.size.mb", { n: num(n / 1024 / 1024) });
  return t("backup.size.gb", { n: num(n / 1024 / 1024 / 1024) });
}

export interface BackupUi {
  exportNow: () => void;
  startRestore: () => void;
  /** 面板與忙碌提示，放在設定畫面的最上層。 */
  overlays: ReactNode;
}

type Notify = (title: string, message: string, more?: NoticeMore) => void;

/** `libraryEmpty`：目前的文獻庫是空的（還原會放在原位，不另做一個新的）。 */
export function useBackup(
  nav: Nav,
  sheet: SheetKind | null,
  refresh: () => Promise<void>,
  libraryEmpty: boolean,
  notify: Notify,
): BackupUi {
  const [busy, setBusy] = useState<string | null>(null);
  const busyRef = useRef(false);
  const alive = useAlive();
  const [exported, setExported] = useState<ExportResult | null>(null);
  const [restored, setRestored] = useState<RestoreResult | null>(null);
  const { setGuard } = nav;

  // 忙碌時攔住返回（含系統返回鍵）：匯出或還原做到一半不能離開
  useEffect(() => {
    if (busy === null) return;
    setGuard(() => true);
    return () => setGuard(null);
  }, [busy, setGuard]);

  const fail = (title: StringKey, e: unknown) => {
    const kind = kindOf(e);
    notify(t(title), errorMessage(kind), { detail: errorDetail(kind) });
  };

  const exportNow = () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(t("backup.export.busy"));
    void (async () => {
      try {
        const r = await api.exportBackupFile();
        if (!alive.current) return;
        setExported(r);
        nav.push({ name: "sheet", kind: "backupDeliver" });
      } catch (e) {
        if (alive.current) fail("backup.export.failed.title", e);
      } finally {
        busyRef.current = false;
        setBusy(null);
      }
    })();
  };

  const startRestore = () => {
    if (busyRef.current) return;
    nav.push({ name: "sheet", kind: "restoreConfirm" });
  };

  const runRestore = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(t("backup.restore.busy"));
    await nav.back(); // 關掉確認面板
    try {
      const r = await api.restoreBackupFromPicker();
      if (r.cancelled) return;
      // 文獻庫已經換成還原出來的那個：先更新目錄，再說明結果
      await refresh().catch(() => {});
      if (alive.current) {
        setRestored(r);
        nav.push({ name: "sheet", kind: "backupResult" });
      }
    } catch (e) {
      if (alive.current) fail("backup.restore.failed.title", e);
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  };

  const close = () => void nav.back();

  const overlays = (
    <>
      <AnimatePresence>
        {sheet === "backupDeliver" && exported ? (
          <DeliverSheet
            key={exported.path}
            title={t("backup.deliver.title")}
            file={exported}
            primary="save"
            closeLabel={t("backup.deliver.later")}
            onClose={close}
          >
            <p>
              {t("backup.deliver.summary", {
                name: exported.name,
                files: exported.files,
                size: formatBytes(exported.bytes),
              })}
            </p>
            {exported.skipped.length > 0 ? (
              <Details label={t("backup.deliver.skipped", { n: exported.skipped.length })}>
                <ul className="problem-list">
                  {exported.skipped.map((s, i) => (
                    <li key={`${i}-${s.path}`} className="sheet-path">
                      {t("backup.deliver.skippedLine", { path: s.path, reason: skipReason(s.reason) })}
                    </li>
                  ))}
                </ul>
              </Details>
            ) : null}
            {exported.changedDuringExport.length > 0 ? (
              <p>{t("backup.deliver.changed", { n: exported.changedDuringExport.length })}</p>
            ) : null}
            <p className="muted small">{t("backup.deliver.temp")}</p>
          </DeliverSheet>
        ) : null}

        {sheet === "restoreConfirm" ? (
          <ConfirmSheet
            key="restoreConfirm"
            open
            title={t("backup.confirm.title")}
            confirm={t("backup.confirm.go")}
            onConfirm={() => void runRestore()}
            onCancel={close}
          >
            <p>{t(libraryEmpty ? "backup.confirm.bodyEmpty" : "backup.confirm.body")}</p>
            <p className="muted small">{t("backup.confirm.time")}</p>
          </ConfirmSheet>
        ) : null}

        {sheet === "backupResult" && restored ? (
          <ResultSheet key="result" open title={t("backup.result.title")} onDone={close}>
            <p>{t("backup.result.summary", { files: restored.files, size: formatBytes(restored.bytes) })}</p>
            {restored.problems > 0 ? <p>{t("backup.result.problems", { n: restored.problems })}</p> : null}
            {restored.mismatches > 0 ? <p>{t("backup.result.mismatches", { n: restored.mismatches })}</p> : null}
            {/* 不是空的文獻庫：新的放在旁邊，原本的還在；放在原位時不必說 */}
            {restored.oldPath ? <p>{t("backup.result.kept")}</p> : null}
            <Details>
              {restored.oldPath ? (
                <p className="sheet-path">{t("backup.result.old", { path: restored.oldPath })}</p>
              ) : null}
              <p className="sheet-path">{t("backup.result.new", { path: restored.newPath })}</p>
            </Details>
          </ResultSheet>
        ) : null}
      </AnimatePresence>
      <BusyOverlay active={busy !== null} label={busy ?? ""} />
    </>
  );

  return { exportNow, startRestore, overlays };
}
