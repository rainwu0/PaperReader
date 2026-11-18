// 設定的「搬到共用空間」：應用專屬空間的文獻庫整個搬到共用空間的「文件／PaperReader」。
//
// 按下去：還沒有「所有檔案存取」先說明並去系統頁開（回到前景再接著做）→ 問外殼估計（`libraryMovePlan`）→
// 確認框寫明搬去哪裡、要多少空間、搬完會刪掉應用專屬空間的舊資料→ 進度（核對之前可以取消）→
// 完成時說新的位置。共用空間已經有文獻庫、這裡的是空的：不搬，確認後改用那一個（`librarySwitchToShared`）。
// 搬的期間逐篇處理暫停（`holdPerPaper`）、攔住返回；翻譯、重點筆記或譯本 PDF 在做時不開始。
// 面板都是導覽堆疊的一層，畫在設定畫面的最上層（同 `useBackup`）。

import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, kindOf, type ErrorKind, type MovePlan, type MoveProgress, type MoveStep } from "../../api";
import { useAlive, useForeground, usePoll } from "../../hooks";
import type { Nav, SheetKind } from "../../nav";
import { holdPerPaper } from "../../perPaperApp";
import { getBuild, isExpecting } from "../../translate/autoBuild";
import { errorDetail, errorMessage, errorReason, t, type StringKey } from "../../strings";
import { AnimatePresence } from "../../ui/motion";
import { BusyOverlay, Button, ConfirmSheet, Sheet } from "../../ui/components";
import { ProgressBar } from "../generate/JobProgress";
import { usePermissionGate, type NoticeMore } from "../add/useAdd";
import { formatBytes } from "./useBackup";

const STEP_KEYS: Record<MoveStep, StringKey> = {
  idle: "move.step.prepare",
  prepare: "move.step.prepare",
  export: "move.step.export",
  copy: "move.step.copy",
  verify: "move.step.verify",
  switch: "move.step.switch",
  clean: "move.step.clean",
  done: "move.step.clean",
};

/** 換位置之前（核對做完之前）才能取消。 */
const CANCELLABLE: ReadonlySet<MoveStep> = new Set<MoveStep>(["idle", "prepare", "export", "copy", "verify"]);

type Notify = (title: string, message: string, more?: NoticeMore) => void;

export interface MoveUi {
  /** 按下「搬到共用空間」。`allFilesAccess`：目前有沒有「所有檔案存取」。 */
  start: (allFilesAccess: boolean) => void;
  /** 面板與忙碌提示，放在設定畫面的最上層。 */
  overlays: ReactNode;
}

/** 搬不成時表面那一句：幾種常見的原因直接說；其他的說「沒有搬成、仍在應用專屬空間」，原因放「詳情」。 */
function failure(kind: ErrorKind): { message: string; detail?: string } {
  switch (kind) {
    case "SharedNotEmpty":
    case "MoveMismatch":
    case "TranslateBusy":
      return { message: errorMessage(kind), detail: errorDetail(kind) };
    case "NoSpace":
      return { message: t("move.noSpace"), detail: t("move.noSpace.detail") };
    case "PermissionDenied":
      return { message: t("move.denied") };
    default:
      return { message: t("move.failed"), detail: errorReason(kind) };
  }
}

export function useMove(
  nav: Nav,
  sheet: SheetKind | null,
  refresh: () => Promise<void>,
  notify: Notify,
): MoveUi {
  // 問估計、改用共用空間的那一個時的忙碌提示
  const [busy, setBusy] = useState(false);
  const [plan, setPlan] = useState<MovePlan | null>(null);
  const [moving, setMoving] = useState(false);
  const [progress, setProgress] = useState<MoveProgress | null>(null);
  const running = useRef(false);
  const alive = useAlive();
  const gate = usePermissionGate<"move">();
  const { setGuard } = nav;

  // 忙碌與搬的時候攔住返回（含系統返回鍵）
  useEffect(() => {
    if (!busy && !moving) return;
    setGuard(() => true);
    return () => setGuard(null);
  }, [busy, moving, setGuard]);

  usePoll(
    moving,
    async () => {
      const p = await api.libraryMoveProgress();
      if (alive.current) setProgress(p);
    },
    300,
    0,
  );

  const fail = (e: unknown) => {
    const { message, detail } = failure(kindOf(e));
    notify(t("move.not.title"), message, detail ? { detail } : undefined);
  };

  /** 問估計，給確認框。 */
  const prepare = async () => {
    setBusy(true);
    try {
      const p = await api.libraryMovePlan();
      if (!alive.current) return;
      setPlan(p);
      nav.push({ name: "sheet", kind: "moveConfirm" });
    } catch (e) {
      if (alive.current) fail(e);
    } finally {
      setBusy(false);
    }
  };

  const start = (allFilesAccess: boolean) => {
    if (running.current || busy) return;
    // 譯本 PDF 在畫面這一側做：外殼看不到，這裡先擋（翻譯與重點筆記由外殼擋）
    if (getBuild().phase === "working" || isExpecting()) {
      notify(t("move.not.title"), errorMessage("TranslateBusy"), { detail: errorDetail("TranslateBusy") });
      return;
    }
    if (allFilesAccess) void prepare();
    else nav.push({ name: "sheet", kind: "moveIntro" });
  };

  // 從系統的授權頁回來：有權限就接著估計，沒有就說仍在應用專屬空間
  useForeground(() => {
    if (gate.take() !== "move") return;
    void (async () => {
      const s = await api.libraryStatus().catch(() => null);
      await refresh().catch(() => {});
      if (!alive.current) return;
      if (s?.allFilesAccess) await prepare();
      else notify(t("move.not.title"), t("move.denied"));
    })();
  });

  const grant = async () => {
    await nav.back(); // 關掉說明
    await gate.ask("move", () => notify(t("move.not.title"), t("move.denied")));
  };

  /** 共用空間已經有文獻庫、這裡的是空的：改用那一個（不搬）。 */
  const adopt = async () => {
    await nav.back(); // 關掉確認框
    setBusy(true);
    try {
      const s = await api.librarySwitchToShared();
      await refresh().catch(() => {});
      if (alive.current) notify(t("move.adopted.title"), t("move.done.where", { path: s.path ?? "" }));
    } catch (e) {
      if (alive.current) fail(e);
    } finally {
      setBusy(false);
    }
  };

  const run = async () => {
    if (running.current) return;
    running.current = true;
    await nav.back(); // 關掉確認框
    setProgress(null);
    setMoving(true);
    nav.push({ name: "sheet", kind: "moveProgress" });
    const release = holdPerPaper();
    let shown: () => void = () => {};
    try {
      const out = await api.libraryMoveToShared();
      await refresh().catch(() => {});
      shown = out.cancelled
        ? () => notify(t("move.not.title"), t("move.cancelled"))
        : () => notify(t("move.done.title"), t("move.done.where", { path: out.path }));
    } catch (e) {
      await refresh().catch(() => {});
      shown = () => fail(e);
    } finally {
      release();
      running.current = false;
      setMoving(false);
      // 先放開返回的攔截（effect 要等下一次畫面更新才清），才關得掉進度面板
      setGuard(null);
    }
    if (!alive.current) return;
    await nav.back(); // 關掉進度面板
    shown();
  };

  const cancel = () => {
    if (!CANCELLABLE.has(progress?.step ?? "prepare")) return;
    // 外殼收下了才顯示「正在取消…」（換位置起它不收）
    void api
      .libraryMoveCancel()
      .then((taken) => {
        if (taken) setProgress((p) => (p ? { ...p, cancelling: true } : p));
      })
      .catch(() => {});
  };

  const close = () => void nav.back();
  const step: MoveStep = progress?.step ?? "prepare";
  const pct =
    progress && progress.total > 0 ? Math.min(100, Math.floor((progress.done * 100) / progress.total)) : null;
  const label = progress?.cancelling ? t("move.cancelling") : t(STEP_KEYS[step]);

  const overlays = (
    <>
      <AnimatePresence>
        {sheet === "moveIntro" ? (
          <ConfirmSheet
            key="moveIntro"
            open
            title={t("move.title")}
            confirm={t("home.lostAccess.action")}
            cancel={t("switch.intro.decline")}
            onConfirm={() => void grant()}
            onCancel={close}
          >
            <p>{t("add.intro.why")}</p>
          </ConfirmSheet>
        ) : null}

        {sheet === "moveConfirm" && plan && plan.adopt ? (
          <ConfirmSheet
            key="moveAdopt"
            open
            title={t("move.adopt.title")}
            confirm={t("move.adopt.go")}
            onConfirm={() => void adopt()}
            onCancel={close}
          >
            <p>{t("move.adopt.body")}</p>
            <p className="muted small">{t("move.adopt.kept")}</p>
          </ConfirmSheet>
        ) : null}

        {sheet === "moveConfirm" && plan && !plan.adopt ? (
          <ConfirmSheet
            key="moveConfirm"
            open
            title={t("move.title")}
            confirm={t("move.confirm.go")}
            onConfirm={() => void run()}
            onCancel={close}
          >
            <p>{t("move.confirm.to", { size: formatBytes(plan.needed) })}</p>
            {/* 會取代、刪掉使用者資料的事留在做決定的畫面上 */}
            <p>{t("move.confirm.delete")}</p>
            {plan.free !== null ? (
              <p className="muted small">{t("move.confirm.free", { free: formatBytes(plan.free) })}</p>
            ) : null}
          </ConfirmSheet>
        ) : null}

        {sheet === "moveProgress" ? (
          <Sheet key="moveProgress" open center title={t("move.progress.title")}>
            <p role="status" data-testid="move-step">
              {pct !== null && !progress?.cancelling ? t("sync.busy.percent", { label, pct }) : label}
            </p>
            <ProgressBar n={progress?.done ?? 0} total={progress?.total ?? 0} />
            <p className="muted small">{t("move.progress.stay")}</p>
            {/* 換位置起不能取消：按鈕變灰，不收下沒有作用的取消 */}
            <div className="actions">
              <Button block disabled={progress?.cancelling === true || !CANCELLABLE.has(step)} onClick={cancel}>
                {t("common.cancel")}
              </Button>
            </div>
          </Sheet>
        ) : null}
      </AnimatePresence>
      <BusyOverlay active={busy} label={t("common.working")} />
    </>
  );

  return { start, overlays };
}
