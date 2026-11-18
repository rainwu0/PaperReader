// 整批匯入的流程：
//   首頁「＋」→ 選單「整批匯入」→ 選資料夾或 zip → 讀取（有進度、可以取消）→ 預覽頁（`ImportPreview.tsx`）
//   →「匯入」→ 一篇一步寫入（有進度、可以取消，已加入的保留）→ 結果面板，首頁重新整理。
// 離開預覽頁（返回鍵、返回手勢、頂端的返回）就放棄預覽；寫入時預覽已過期（清單被改過）請使用者重新匯入。
// 寫完之後，沒有書目的篇照「加入」的做法讀 PDF 自帶的篇名與作者（`pdfInfo.ts`），在背景做。
// 面板與進度畫在最上層（App），忙碌時攔住返回，不讓使用者在中途離開。

import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, kindOf, type ErrorKind, type ImportPreview, type ImportReport } from "../../api";
import { sourcePdf } from "../../api/types";
import { usePoll } from "../../hooks";
import type { Library } from "../../library";
import { topSheet, type Nav } from "../../nav";
import { fillFromPdfInfo } from "../../pdfInfo";
import { holdPerPaper } from "../../perPaperApp";
import { errorDetail, errorMessage, t } from "../../strings";
import { Button, Details, ResultSheet, Sheet, Spinner, type ShowUndo } from "../../ui/components";
import { AnimatePresence, m, useMotion } from "../../ui/motion";
import { removeAdded, restoreAll, type NoticeMore } from "./useAdd";
import { skipLine } from "./importModel";

export interface ImportUi {
  /** 首頁「＋」：選 PDF 或整批匯入。 */
  openMenu: () => void;
  /** 目前的預覽（預覽頁顯示它）；沒有是 `null`。 */
  preview: ImportPreview | null;
  /** 預覽頁的「匯入」。 */
  run: () => void;
  overlays: ReactNode;
}

type Notify = (title: string, message: string, more?: NoticeMore) => void;

type Busy = { kind: "scan" } | { kind: "step"; done: number; total: number };

export function useImport(lib: Library, nav: Nav, notify: Notify, addPdf: () => void, undo: ShowUndo): ImportUi {
  const sheet = topSheet(nav.stack);
  const [busy, setBusy] = useState<Busy | null>(null);
  const busyRef = useRef(false);
  const [pct, setPct] = useState<number | null>(null);
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const running = useRef(false);
  const stopAsked = useRef(false);
  const seenPreviewRoute = useRef(false);
  const { setGuard } = nav;

  // 忙碌時攔住返回（含系統返回鍵）：讀取、寫入做到一半由「取消」停下
  useEffect(() => {
    if (busy === null) return;
    setGuard(() => true);
    return () => setGuard(null);
  }, [busy, setGuard]);

  // 讀取時顯示百分比；第一輪還不知道總量時只寫「正在讀取」
  const scanning = busy?.kind === "scan";
  useEffect(() => {
    if (!scanning) setPct(null);
  }, [scanning]);
  usePoll(
    scanning,
    async () => {
      const [done, total] = await api.importProgress().catch(() => [0, 0]);
      setPct(total > 0 ? Math.min(100, Math.floor((done * 100) / total)) : null);
    },
    300,
  );

  // 預覽頁離開了而且沒有匯入：放棄預覽（核心丟掉計畫，外殼刪掉 zip 的複本）
  const onPreviewRoute = nav.stack.some((r) => r.name === "importPreview");
  useEffect(() => {
    if (!preview) {
      seenPreviewRoute.current = false;
      return;
    }
    if (onPreviewRoute) {
      seenPreviewRoute.current = true;
      return;
    }
    if (seenPreviewRoute.current && !running.current) {
      void api.finishImport(preview.preview_id).catch(() => {});
      setPreview(null);
    }
  }, [onPreviewRoute, preview]);

  const begin = (b: Busy): boolean => {
    if (busyRef.current) return false;
    busyRef.current = true;
    stopAsked.current = false;
    setBusy(b);
    return true;
  };
  const end = () => {
    busyRef.current = false;
    setBusy(null);
    setGuard(null);
  };

  const showError = (kind: ErrorKind) => {
    if (kind === "Unreadable") {
      notify(t("import.failed.title"), t("import.error.Unreadable"), { detail: t("import.error.Unreadable.detail") });
    } else if (kind === "FormatTooNew") {
      notify(t("import.failed.title"), t("import.error.FormatTooNew"));
    } else if (kind === "PermissionDenied") {
      notify(t("import.failed.title"), t("import.error.PermissionDenied"), {
        action: { label: t("home.lostAccess.action"), run: () => void api.openAllFilesSettings().catch(() => {}) },
      });
    } else {
      notify(t("import.failed.title"), errorMessage(kind), { detail: errorDetail(kind) });
    }
  };

  const openSource = () => nav.push({ name: "sheet", kind: "importSource" });

  /** 選資料夾或 zip、讀取、比對成預覽。 */
  const start = async (zip: boolean) => {
    await nav.back(); // 收起選資料夾或 zip 的面板
    if (!begin({ kind: "scan" })) return;
    try {
      const r = await api.scanImport(zip);
      if (r.cancelled) return;
      if (r.unsupported) {
        notify(t("import.failed.title"), t("import.unsupported"));
        return;
      }
      const p = await api.prepareImport();
      setPreview(p);
      end();
      nav.push({ name: "importPreview" });
    } catch (e) {
      showError(kindOf(e));
    } finally {
      if (busyRef.current) end();
    }
  };

  /** 寫完之後：沒有書目的篇讀 PDF 自帶的篇名與作者（照「加入」的做法；有書目或使用者改過的不動）。 */
  const fillAdded = async (added: Array<[number, string]>, release: () => void) => {
    try {
      const details = await Promise.all(added.map(([, id]) => api.getPaper(id).catch(() => null)));
      const runs: Array<Promise<boolean>> = [];
      for (const d of details) {
        const source = d ? sourcePdf(d.paper.materials) : undefined;
        const name = source?.provenance?.origin_name;
        if (d && name) runs.push(fillFromPdfInfo(d.paper.id, name));
      }
      release(); // 每一篇都排好「補完之前不做逐篇處理」了
      await Promise.all(runs);
    } finally {
      release();
      await lib.refresh().catch(() => {});
    }
  };

  /** 一篇一步寫入；「取消」在兩篇之間停下。 */
  const run = () => {
    const p = preview;
    if (!p || !begin({ kind: "step", done: 0, total: p.accept.length })) return;
    running.current = true;
    // 寫入與補篇名作者排好之前，逐篇處理不開始新的一篇（同「加入」）
    let released = false;
    const holdRelease = holdPerPaper();
    const release = () => {
      if (!released) {
        released = true;
        holdRelease();
      }
    };
    void (async () => {
      try {
        let r: ImportReport;
        for (;;) {
          if (stopAsked.current) {
            r = await api.finishImport(p.preview_id);
            break;
          }
          const s = await api.importStep(p.preview_id);
          if ("Finished" in s) {
            r = s.Finished;
            break;
          }
          setBusy({ kind: "step", done: s.Progress.done, total: s.Progress.total });
        }
        setReport(r);
        setPreview(null);
        await lib.refresh().catch(() => {});
        end();
        await nav.back(); // 離開預覽頁
        nav.push({ name: "sheet", kind: "importResult" });
        void fillAdded(r.added, release);
      } catch (e) {
        release();
        const kind = kindOf(e);
        setPreview(null);
        await lib.refresh().catch(() => {});
        end();
        await nav.back();
        if (kind === "Conflict" || kind === "InvalidInput") {
          notify(t("import.failed.title"), t("import.expired"), {
            action: { label: t("sync.apply.again"), run: openSource },
          });
        } else {
          showError(kind);
        }
      } finally {
        running.current = false;
        if (busyRef.current) end();
      }
    })();
  };

  const cancel = () => {
    if (busy?.kind === "scan") void api.importCancel().catch(() => {});
    else stopAsked.current = true;
  };

  const close = () => void nav.back();

  /** 結果面板的「移除這次加入的 n 篇」：關掉面板、移到回收區，提示給「復原」。 */
  const removeImported = async (ids: string[]) => {
    await nav.back();
    try {
      const done = await removeAdded(ids);
      await lib.refresh().catch(() => {});
      undo(t("bulk.removed", { n: done.length }), () => restoreAll(done));
    } catch (e) {
      await lib.refresh().catch(() => {});
      const kind = kindOf(e);
      notify(t("paper.remove.failed.title"), errorMessage(kind), { detail: errorDetail(kind) });
    }
  };

  const label =
    busy === null
      ? ""
      : busy.kind === "scan"
        ? pct !== null
          ? t("import.scan.percent", { pct })
          : t("import.scan.busy")
        : t("import.step.busy", { done: busy.done, total: busy.total });

  const pickPdf = async () => {
    await nav.back();
    addPdf();
  };
  const toImport = async () => {
    await nav.back();
    openSource();
  };

  const overlays = (
    <>
      <Sheet open={sheet === "addMenu"} title={t("home.add")} onDismiss={close}>
        <div className="actions">
          <Button block primary onClick={() => void pickPdf()}>
            {t("add.menu.pdf")}
          </Button>
          <Button block onClick={() => void toImport()}>
            {t("import.title")}
          </Button>
        </div>
        <p className="muted small">{t("add.menu.import.hint")}</p>
      </Sheet>

      <Sheet open={sheet === "importSource"} title={t("import.title")} onDismiss={close}>
        <p>{t("import.source.lead")}</p>
        <Details>
          <p>{t("import.source.details")}</p>
        </Details>
        <div className="actions">
          <Button block primary onClick={() => void start(false)}>
            {t("import.source.folder")}
          </Button>
          <Button block onClick={() => void start(true)}>
            {t("import.source.zip")}
          </Button>
        </div>
      </Sheet>

      <AnimatePresence>
        {sheet === "importResult" && report ? (
          <ResultSheet key="importResult" open title={t("import.result.title")} onDone={close}>
            <p data-testid="import-result">{t("import.result.added", { n: report.added.length })}</p>
            {report.skipped.length > 0 ? (
              <Details label={t("import.result.skipped", { n: report.skipped.length })}>
                <ul className="problem-list">
                  {report.skipped.map((s) => (
                    <li key={`s-${s.index}`}>{skipLine(s, lib.papers)}</li>
                  ))}
                </ul>
              </Details>
            ) : null}
            {report.failed.length > 0 ? (
              <Details label={t("import.result.failed", { n: report.failed.length })}>
                <ul className="problem-list">
                  {report.failed.map((s) => (
                    <li key={`f-${s.index}`}>{skipLine(s, lib.papers)}</li>
                  ))}
                </ul>
              </Details>
            ) : null}
            {report.not_done.length > 0 ? <p>{t("import.result.notDone", { n: report.not_done.length })}</p> : null}
            {report.added.length > 0 ? (
              <Button block onClick={() => void removeImported(report.added.map(([, id]) => id))}>
                {t("add.batch.removeAdded", { n: report.added.length })}
              </Button>
            ) : null}
          </ResultSheet>
        ) : null}
      </AnimatePresence>

      <ImportBusy active={busy !== null} label={label} onCancel={cancel} />
    </>
  );

  return {
    // 還沒有文獻庫時（第一次加入要先說明放哪裡、要權限）照原本的「加入」走，有了文獻庫才給整批匯入的選單
    openMenu: () => (lib.status?.state === "ready" ? nav.push({ name: "sheet", kind: "addMenu" }) : addPdf()),
    preview,
    run,
    overlays,
  };
}

/** 讀取與寫入的進度：和忙碌提示同一張卡片，多一顆「取消」（耗時的動作可以取消）。 */
function ImportBusy(props: { active: boolean; label: string; onCancel: () => void }) {
  const fade = useMotion("fade");
  return (
    <AnimatePresence>
      {props.active ? (
        <m.div key="import-busy" className="busy" role="alert" aria-live="assertive" {...fade}>
          <div className="busy-card">
            <Spinner dark />
            <span data-testid="import-progress">{props.label}</span>
            <Button onClick={props.onCancel}>{t("common.cancel")}</Button>
          </div>
        </m.div>
      ) : null}
    </AnimatePresence>
  );
}
