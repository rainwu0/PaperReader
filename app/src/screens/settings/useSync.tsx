// 同步包的流程：匯出同步包、匯入同步包（選檔 → 預覽 → 合併）、把沒做完的合併做完。
//
// 匯出：忙碌提示（帶百分比）→ 交出面板（同備份檔，`ui/DeliverSheet.tsx`）說明匯出了什麼 →「存到…」或「分享」。
// 匯入：選檔 → 忙碌提示 → 預覽頁（`SyncPreview.tsx`）→「合併」→ 結果面板，首頁重新整理。
//   離開預覽頁（返回鍵、返回手勢、頂端的返回）就丟棄預覽；合併時預覽已過期（`Conflict`、`NotFound`）請使用者重新匯入。
// 合併沒做完（文獻庫因此唯讀）：「重試」把它做完；失敗照錯誤種類說原因。
// 面板與忙碌提示畫在最上層（App），所以從同步頁、預覽頁、首頁都叫得出來。忙碌時攔住返回，不讓使用者在中途離開。

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  api,
  kindOf,
  type ConflictChoice,
  type SyncExportResult,
  type SyncImportReport,
  type SyncPreview,
} from "../../api";
import type { Library } from "../../library";
import { usePoll } from "../../hooks";
import { topSheet, type Nav } from "../../nav";
import { errorDetail, errorMessage, t } from "../../strings";
import { BusyOverlay, Details, ResultSheet } from "../../ui/components";
import { DeliverSheet } from "../../ui/DeliverSheet";
import { AnimatePresence } from "../../ui/motion";
import { holdPerPaper } from "../../perPaperApp";
import { choicesToSend, defaultChoices, groupNotices } from "./syncModel";
import { formatBytes } from "./useBackup";
import type { NoticeMore } from "../add/useAdd";

export interface SyncUi {
  /** 匯出完整的同步包（預設只帶變更）。 */
  full: boolean;
  setFull: (v: boolean) => void;
  exportNow: () => void;
  startImport: () => void;
  /** 把上一次沒做完的合併做完。 */
  resume: () => void;
  /** 目前的預覽（預覽頁顯示它）；沒有是 `null`。 */
  preview: SyncPreview | null;
  /** 每個衝突目前選的（預設已選好）。 */
  picked: Record<string, ConflictChoice>;
  choose: (conflictId: string, choice: ConflictChoice) => void;
  apply: () => void;
  overlays: ReactNode;
}

type Notify = (title: string, message: string, more?: NoticeMore) => void;

/** 檔名裡的當地時間：`20261116-1030`。 */
function localStamp(d: Date = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}`;
}

function itemsTotal(items: SyncImportReport["added"]): number {
  return items.reduce((n, it) => n + it.count, 0);
}

export function useSync(lib: Library, nav: Nav, recheck: () => void, notify: Notify): SyncUi {
  const sheet = topSheet(nav.stack);
  const [busy, setBusy] = useState<{ label: string; progress: boolean } | null>(null);
  const busyRef = useRef(false);
  const [pct, setPct] = useState<number | null>(null);
  const [full, setFull] = useState(false);
  const [exported, setExported] = useState<{ r: SyncExportResult; requestedFull: boolean } | null>(null);
  const [preview, setPreview] = useState<SyncPreview | null>(null);
  const [picked, setPicked] = useState<Record<string, ConflictChoice>>({});
  const [report, setReport] = useState<SyncImportReport | null>(null);
  const applying = useRef(false);
  const seenPreviewRoute = useRef(false);
  const { setGuard } = nav;

  // 從讀同步包到合併寫完（或丟棄預覽）持有逐篇處理的暫停：有衝突的那篇，預覽時讀過的本機檔記成版本記號，
  // 期間逐篇處理替它寫了 text.json 之類的檔，整次合併就會交回 Conflict
  const perPaperHold = useRef<(() => void) | null>(null);
  const holdOrganize = () => {
    perPaperHold.current ??= holdPerPaper();
  };
  const releaseOrganize = useCallback(() => {
    perPaperHold.current?.();
    perPaperHold.current = null;
  }, []);
  useEffect(() => releaseOrganize, [releaseOrganize]);

  // 忙碌時攔住返回（含系統返回鍵）：匯出、讀取、合併做到一半不能離開
  useEffect(() => {
    if (busy === null) return;
    setGuard(() => true);
    return () => setGuard(null);
  }, [busy, setGuard]);

  // 匯出與讀取同步包時顯示百分比（知道進度就顯示進度）
  const polling = busy?.progress ?? false;
  useEffect(() => {
    if (!polling) setPct(null);
  }, [polling]);
  usePoll(
    polling,
    async () => {
      const [done, total] = await api.syncProgress().catch(() => [0, 0]);
      setPct(total > 0 ? Math.min(100, Math.floor((done * 100) / total)) : null);
    },
    300,
  );

  // 預覽頁離開了（返回鍵、返回手勢、頂端的返回）而且沒有合併：丟棄預覽（刪掉文獻庫裡的暫存）
  const onPreviewRoute = nav.stack.some((r) => r.name === "syncPreview");
  useEffect(() => {
    if (!preview) {
      seenPreviewRoute.current = false;
      return;
    }
    if (onPreviewRoute) {
      seenPreviewRoute.current = true;
      return;
    }
    if (seenPreviewRoute.current && !applying.current) {
      void api.syncImportDiscard(preview.preview_id).catch(() => {});
      setPreview(null);
      releaseOrganize();
    }
  }, [onPreviewRoute, preview, releaseOrganize]);

  const refreshAll = async () => {
    await lib.refresh().catch(() => {});
    recheck();
  };

  const begin = (label: string, progress: boolean): boolean => {
    if (busyRef.current) return false;
    busyRef.current = true;
    setBusy({ label, progress });
    return true;
  };
  const end = () => {
    busyRef.current = false;
    setBusy(null);
    // 馬上放開返回的攔截（effect 要等下一次畫面更新才清）：接著要退出預覽頁
    setGuard(null);
  };

  const exportNow = () => {
    if (!begin(t("sync.export.busy"), true)) return;
    const requestedFull = full;
    void (async () => {
      try {
        const r = await api.syncExport(requestedFull, localStamp());
        setExported({ r, requestedFull });
        nav.push({ name: "sheet", kind: "syncDeliver" });
      } catch (e) {
        const kind = kindOf(e);
        notify(t("sync.export.failed.title"), errorMessage(kind), { detail: errorDetail(kind) });
      } finally {
        end();
      }
    })();
  };

  const startImport = () => {
    if (!begin(t("sync.import.busy"), true)) return;
    holdOrganize();
    void (async () => {
      let previewing = false;
      try {
        const r = await api.syncImportPrepare();
        if (r.cancelled || !r.preview) return;
        setPicked(defaultChoices(r.preview.conflicts));
        setPreview(r.preview);
        previewing = true;
        nav.push({ name: "syncPreview" });
      } catch (e) {
        const kind = kindOf(e);
        notify(t("sync.import.failed.title"), errorMessage(kind), { detail: errorDetail(kind) });
      } finally {
        if (!previewing) releaseOrganize();
        end();
      }
    })();
  };

  const apply = () => {
    const p = preview;
    if (!p || !begin(t("sync.apply.busy"), false)) return;
    applying.current = true;
    void (async () => {
      try {
        const r = await api.syncImportApply(p.preview_id, choicesToSend(p.conflicts, picked));
        setReport(r);
        setPreview(null);
        await refreshAll();
        end(); // 先放開返回的攔截，才退得出預覽頁
        await nav.back();
        nav.push({ name: "sheet", kind: "syncResult" });
      } catch (e) {
        const kind = kindOf(e);
        setPreview(null);
        // 合併途中失敗時文獻庫可能變成「合併沒做完」：重讀狀況，首頁才會出現「重試」
        await refreshAll();
        end();
        await nav.back();
        if (kind === "Conflict" || kind === "NotFound") {
          notify(t("sync.apply.failed.title"), t("sync.apply.expired"), {
            action: { label: t("sync.apply.again"), run: startImport },
          });
        } else {
          notify(t("sync.apply.failed.title"), errorMessage(kind), { detail: errorDetail(kind) });
        }
      } finally {
        applying.current = false;
        releaseOrganize();
        if (busyRef.current) end();
      }
    })();
  };

  const resume = () => {
    if (!begin(t("sync.unfinished.busy"), false)) return;
    void (async () => {
      try {
        const r = await api.syncResume();
        setReport(r);
        await refreshAll();
        nav.push({ name: "sheet", kind: "syncResult" });
      } catch (e) {
        const kind = kindOf(e);
        await refreshAll();
        // 已經做完了（例如開啟文獻庫時自動做完）：照做完說
        if (kind === "NotFound") notify(t("sync.unfinished.done"), t("sync.unfinished.doneBody"));
        else notify(t("sync.unfinished.failed.title"), errorMessage(kind), { detail: errorDetail(kind) });
      } finally {
        end();
      }
    })();
  };

  const choose = (conflictId: string, choice: ConflictChoice) => {
    setPicked((cur) => ({ ...cur, [conflictId]: choice }));
  };

  const close = () => void nav.back();
  const busyLabel = busy ? (pct !== null ? t("sync.busy.percent", { label: busy.label, pct }) : busy.label) : "";

  const unmerged = report ? groupNotices(report.notices).find((g) => g.kind === "UnmergedWrite") : undefined;
  const overlays = (
    <>
      <AnimatePresence>
        {sheet === "syncDeliver" && exported ? (
          <DeliverSheet
            key={exported.r.path}
            title={t("sync.deliver.title")}
            file={exported.r}
            primary="save"
            closeLabel={t("backup.deliver.later")}
            onClose={close}
          >
            <p className="sheet-path">
              {t("sync.deliver.summary", { name: exported.r.name, size: formatBytes(exported.r.bytes) })}
            </p>
            {exported.r.mode === "full" && !exported.requestedFull ? (
              <p className="muted small">{t("sync.deliver.becameFull")}</p>
            ) : null}
            {exported.r.skipped.length > 0 ? (
              <Details label={t("sync.deliver.skipped", { n: exported.r.skipped.length })}>
                <ul className="problem-list">
                  {exported.r.skipped.map((s, i) => (
                    <li key={`${i}-${s.path}`} className="sheet-path">
                      {s.path}
                    </li>
                  ))}
                </ul>
              </Details>
            ) : null}
            {exported.r.changedDuringExport.length > 0 ? (
              <p>{t("backup.deliver.changed", { n: exported.r.changedDuringExport.length })}</p>
            ) : null}
            <p className="muted small">{t("backup.deliver.temp")}</p>
          </DeliverSheet>
        ) : null}

        {sheet === "syncResult" && report ? (
          <ResultSheet key="syncResult" open title={t("sync.result.title")} onDone={close}>
            <p data-testid="sync-result">
              {t("sync.result.summary", {
                added: itemsTotal(report.added),
                updated: itemsTotal(report.updated),
                removed: itemsTotal(report.removed),
              })}
            </p>
            {unmerged ? (
              <>
                <p>{t("sync.notice.UnmergedWrite")}</p>
                <ul className="problem-list">
                  {unmerged.paths.map((path) => (
                    <li key={path} className="sheet-path">
                      {path}
                    </li>
                  ))}
                </ul>
              </>
            ) : null}
            {report.skipped_papers.length > 0 ? (
              <p>{t("sync.result.skipped", { n: report.skipped_papers.length })}</p>
            ) : null}
            {report.problems.length > 0 ? (
              <p>{t("sync.result.problems", { n: report.problems.length })}</p>
            ) : null}
            {report.conflict_copies.length > 0 ? (
              <Details label={t("sync.result.copies", { n: report.conflict_copies.length })}>
                <ul className="problem-list">
                  {report.conflict_copies.map((c) => (
                    <li key={c} className="sheet-path">
                      {c}
                    </li>
                  ))}
                </ul>
              </Details>
            ) : null}
          </ResultSheet>
        ) : null}
      </AnimatePresence>
      <BusyOverlay active={busy !== null} label={busyLabel} />
    </>
  );

  return {
    full,
    setFull,
    exportNow,
    startImport,
    resume,
    preview,
    picked,
    choose,
    apply,
    overlays,
  };
}
