// 從另一台加入：第一次開啟、文獻庫還是空的時，用另一台匯出的同步包或備份檔
// 建立同一個文獻庫的複本（代號相同，之後才能互相匯入同步包）。外殼照清單的種類加入或還原，放在正式位置。
//
// 還沒有文獻庫時要先決定放哪裡：和加入第一篇一樣，說明並要求「所有檔案存取」，
// 「去開啟權限」→ 系統授權頁 → 回來後選檔；或放在應用專屬空間。已經有權限或已有（空的）文獻庫就直接選檔。
// 說明、結果都是導覽堆疊裡的一層，返回鍵先關掉它們。

import { useRef, useState, type ReactNode } from "react";
import { api, kindOf, type RestoreResult, type Storage } from "../../api";
import type { Library } from "../../library";
import { topSheet, type Nav } from "../../nav";
import { errorDetail, errorMessage, t } from "../../strings";
import { BusyOverlay, Button, Details, ResultSheet, Sheet } from "../../ui/components";
import { formatBytes } from "../settings/useBackup";
import { usePermissionGate, type NoticeMore } from "./useAdd";

export interface JoinUi {
  /** 按下「從另一台加入」。 */
  start: () => void;
  /** 回到前景時呼叫；正在等使用者從系統授權頁回來時接著選檔，交回 `true`。 */
  resumeAfterPermission: () => boolean;
  overlays: ReactNode;
}

type Notify = (title: string, message: string, more?: NoticeMore) => void;

export function useJoin(lib: Library, nav: Nav, notify: Notify, onDone: () => void): JoinUi {
  const sheet = topSheet(nav.stack);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [result, setResult] = useState<RestoreResult | null>(null);
  const { ask: askPermission, take: takePermission } = usePermissionGate<"join">();

  const run = async (storage: Storage | null) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      const r = await api.libraryJoinFromPicker(storage);
      if (r.cancelled) return;
      await lib.refresh().catch(() => {});
      onDone();
      setResult(r);
      nav.push({ name: "sheet", kind: "joinResult" });
    } catch (e) {
      const kind = kindOf(e);
      notify(t("join.failed.title"), errorMessage(kind), { detail: errorDetail(kind) });
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const start = () => {
    if (busyRef.current) return;
    nav.push({ name: "sheet", kind: "joinIntro" });
  };

  const pickNow = (storage: Storage | null) => {
    void nav.back().then(() => run(storage));
  };

  const grant = async () => {
    await nav.back();
    await askPermission("join", () => run(null));
  };

  const resumeAfterPermission = () => {
    if (!takePermission()) return false;
    void run(null);
    return true;
  };

  const status = lib.status;
  // 還沒有文獻庫、也還沒有權限：先問放哪裡
  const askWhere = status !== null && status.state !== "ready" && !status.allFilesAccess;
  const close = () => void nav.back();

  const overlays = (
    <>
      <Sheet open={sheet === "joinIntro"} title={t("join.title")} onDismiss={close}>
        <p>{t("join.hint")}</p>
        {askWhere ? <p>{t("add.intro.why")}</p> : null}
        <div className="actions">
          {askWhere ? (
            <>
              <Button primary block onClick={() => void grant()}>
                {t("home.lostAccess.action")}
              </Button>
              <Button block onClick={() => pickNow("private")}>
                {t("add.intro.decline")}
              </Button>
            </>
          ) : (
            <>
              <Button primary block onClick={() => pickNow(null)}>
                {t("join.pick")}
              </Button>
              <Button block onClick={close}>
                {t("common.cancel")}
              </Button>
            </>
          )}
        </div>
      </Sheet>
      <ResultSheet open={sheet === "joinResult" && result !== null} title={t("join.done.title")} onDone={close}>
        {result ? (
          <>
            <p data-testid="join-result">
              {t("join.done.summary", { files: result.files, size: formatBytes(result.bytes) })}
            </p>
            {result.oldPath ? <p>{t("join.done.kept")}</p> : null}
            {/* 沒有同意權限、放在應用專屬空間：拒絕的後果留在表面 */}
            {status?.storage === "private" ? <p>{t("add.privateNotice")}</p> : null}
            {result.problems > 0 ? <p>{t("backup.result.problems", { n: result.problems })}</p> : null}
            <Details>
              <p className="sheet-path">{t("join.done.where", { path: result.newPath })}</p>
              {result.oldPath ? (
                <p className="sheet-path">{t("backup.result.old", { path: result.oldPath })}</p>
              ) : null}
            </Details>
          </>
        ) : null}
      </ResultSheet>
      <BusyOverlay active={busy} label={t("join.busy")} />
    </>
  );

  return { start, resumeAfterPermission, overlays };
}
