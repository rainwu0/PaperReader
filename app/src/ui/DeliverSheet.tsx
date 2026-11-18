// 交出一份準備好的檔案：存到…、分享、列印（PDF 才有）。備份檔、同步包、匯出與列印、書目都用這一個。
// 存好或交給分享之後換成置中的「已存好」與檔名；取消就什麼都不說，面板留著讓使用者再選。
// 檔案先由外殼放在應用暫存資料夾的 `exports/`（`deliver_export` 交出；備份檔與同步包存好之後外殼就刪暫存）。

import { useState, type ReactNode } from "react";
import { api, kindOf, type DeliverAction, type ErrorKind } from "../api";
import { errorReason, t } from "../strings";
import { Button, ErrorNotice, ResultSheet, Sheet } from "./components";
import { AnimatePresence } from "./motion";

/** 準備好的一份檔案：檔名（存到…時的預設檔名）與暫存的路徑。 */
export interface Deliverable {
  name: string;
  path: string;
}

export function DeliverSheet(props: {
  title: string;
  file: Deliverable;
  /** 檔名下面的說明：做了什麼、略過哪些、還沒存下來。 */
  children?: ReactNode;
  /** 主要的按鈕：備份檔與同步包是「存到…」，其他匯出是「分享」。 */
  primary: "save" | "share";
  /** 可以列印（PDF）；列印要另一份檔（例如把標註畫進頁面）時給一個準備它的函式。 */
  print?: boolean | (() => Promise<Deliverable>);
  closeLabel: string;
  onClose: () => void;
}) {
  const [working, setWorking] = useState(false);
  const [done, setDone] = useState<"saved" | "shared" | null>(null);
  const [error, setError] = useState<ErrorKind | null>(null);

  const run = async (action: DeliverAction) => {
    if (working) return;
    setWorking(true);
    setError(null);
    try {
      const target = action === "print" && typeof props.print === "function" ? await props.print() : props.file;
      const r = await api.deliverExport(target.path, target.name, action);
      if (r === "saved" || r === "shared") setDone(r);
    } catch (e) {
      setError(kindOf(e));
    } finally {
      setWorking(false);
    }
  };

  const save = (
    <Button key="save" primary={props.primary === "save"} block disabled={working} onClick={() => void run("save")}>
      {t("export.save")}
    </Button>
  );
  const share = (
    <Button key="share" primary={props.primary === "share"} block disabled={working} onClick={() => void run("share")}>
      {t("export.share")}
    </Button>
  );

  return (
    <AnimatePresence propagate>
      {done ? (
        <ResultSheet
          key="done"
          open
          title={t(done === "saved" ? "backup.deliver.saved.title" : "backup.deliver.shared.title")}
          onDone={props.onClose}
        >
          <p className="sheet-path">{t("backup.deliver.file", { name: props.file.name })}</p>
        </ResultSheet>
      ) : (
        <Sheet key="deliver" open title={props.title} onDismiss={working ? undefined : props.onClose}>
          {props.children}
          {error ? <ErrorNotice message={t("backup.deliver.failed")} detail={errorReason(error)} /> : null}
          <div className="actions">
            {props.primary === "save" ? [save, share] : [share, save]}
            {props.print ? (
              <Button block disabled={working} onClick={() => void run("print")}>
                {t("export.print")}
              </Button>
            ) : null}
            <Button block disabled={working} onClick={props.onClose}>
              {props.closeLabel}
            </Button>
          </div>
        </Sheet>
      )}
    </AnimatePresence>
  );
}
