// 安裝失敗的說明：白話一句，下面小字是系統的原話與代碼；「複製錯誤資訊」與可以展開看要複製的內容；
// 退路「用系統安裝程式開啟」。紀錄只在這台手機上，不自動送出。

import { useState } from "react";
import { api, type InstallReport } from "../../api";
import { t } from "../../strings";
import { Button, Details } from "../../ui/components";
import { headline, note, reportText, systemLines, type FailureOutcome } from "./installReport";

export function InstallFailure(props: {
  outcome: FailureOutcome;
  report: InstallReport;
  /** 要裝的那一版的版號。 */
  target: string | null;
  onOpenInstaller: () => void;
  disabled?: boolean;
}) {
  const { outcome, report } = props;
  const [copied, setCopied] = useState<boolean | null>(null);
  const text = reportText(report, outcome, props.target);
  const copy = async () => {
    try {
      await api.updateCopyReport(text);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  return (
    <div data-testid="update-failure" data-outcome={outcome}>
      <div className="notice" role="status">
        <p>{headline(outcome)}</p>
        <p className="small">{note(outcome)}</p>
      </div>
      <div className="muted small" data-testid="update-failure-system">
        {systemLines(report).map((line) => (
          <p key={line}>{line}</p>
        ))}
      </div>
      <div className="actions">
        <Button block onClick={() => void copy()}>
          {t("update.fail.copy")}
        </Button>
        <Button block disabled={props.disabled} onClick={props.onOpenInstaller}>
          {t("update.installer")}
        </Button>
      </div>
      {copied === true ? (
        <p className="muted small" role="status" data-testid="update-copied">
          {t("update.fail.copied")}
        </p>
      ) : null}
      {copied === false ? (
        <p className="muted small" role="status">
          {t("update.fail.copyFailed")}
        </p>
      ) : null}
      <p className="muted small">{t("update.installer.hint")}</p>
      <Details label={t("update.fail.preview")}>
        <pre className="license-text" data-testid="update-report-text">
          {text}
        </pre>
        <p className="muted small">{t("update.fail.private")}</p>
      </Details>
    </div>
  );
}
