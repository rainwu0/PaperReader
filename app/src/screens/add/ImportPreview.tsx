// 整批匯入的預覽：寫入之前列出會加入幾篇、略過幾篇；每篇的篇名、提醒、
// 略過的原因收在可以展開的清單裡。按「匯入」才寫入；離開這一頁就放棄預覽（`useImport`）。

import type { PaperSummary } from "../../api";
import type { Nav } from "../../nav";
import { t } from "../../strings";
import { Button, Details, TopBar } from "../../ui/components";
import { formatBytes } from "../settings/useBackup";
import { noticeText, skipLine } from "./importModel";
import type { ImportUi } from "./useImport";

export function ImportPreviewScreen(props: { nav: Nav; imp: ImportUi; papers: ReadonlyArray<PaperSummary> }) {
  const { nav, imp, papers } = props;
  const p = imp.preview;
  const back = () => void nav.back();
  if (!p) {
    // 匯入完、正要離開這一頁
    return (
      <div className="screen">
        <TopBar title={t("import.title")} onBack={back} />
      </div>
    );
  }
  const none = p.accept.length === 0;
  return (
    <div className="screen sync-preview">
      <TopBar title={t("import.title")} onBack={back} />
      <div className="scroll">
        <div className="content">
          <p data-testid="import-summary">
            {t("import.preview.summary", { accept: p.accept.length, skip: p.skip.length })}
          </p>
          {none ? (
            <p>{t("import.preview.nothing")}</p>
          ) : (
            <p className="muted small">{t("import.preview.size", { size: formatBytes(p.bytes) })}</p>
          )}
          {none ? null : (
            <Details label={t("import.preview.accept", { n: p.accept.length })}>
              <ul className="problem-list">
                {p.accept.map((a) => (
                  <li key={`a-${a.index}`}>
                    {t("import.preview.line", { n: a.index + 1, text: a.title || t("common.untitled") })}
                    {a.notices.map((n, k) => {
                      const text = noticeText(n, papers);
                      return text ? (
                        <span key={k} className="muted small" style={{ display: "block" }}>
                          {text}
                        </span>
                      ) : null;
                    })}
                  </li>
                ))}
              </ul>
            </Details>
          )}
          {p.skip.length > 0 ? (
            <Details label={t("import.preview.skip", { n: p.skip.length })}>
              <ul className="problem-list">
                {p.skip.map((s) => (
                  <li key={`s-${s.index}`}>{skipLine(s, papers)}</li>
                ))}
              </ul>
            </Details>
          ) : null}
        </div>
      </div>
      <div className="sync-bar">
        <Button primary block disabled={none} onClick={imp.run}>
          {t("import.preview.go")}
        </Button>
      </div>
    </div>
  );
}
