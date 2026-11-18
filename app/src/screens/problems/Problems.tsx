// 檢查結果：打開文獻庫後背景檢查找到的問題清單。只列出來，不自動修改或刪除任何東西。
// 「完整檢查」（重新核對每份原文的內容）不放在介面，命令列工具保留。

import { useState } from "react";
import type { PaperSummary, Problem } from "../../api";
import { displayTitle } from "../../format";
import type { Nav } from "../../nav";
import { problemMessage, t } from "../../strings";
import { Button, Screen, Spinner } from "../../ui/components";

export function Problems(props: {
  nav: Nav;
  problems: Problem[];
  papers: PaperSummary[];
  /** 重新檢查一次（背景檢查的同一個動作）。 */
  recheck: () => Promise<void>;
}) {
  const { nav } = props;
  const [busy, setBusy] = useState(false);

  const paperName = (id: string | null) => {
    if (!id) return null;
    const p = props.papers.find((x) => x.id === id);
    return p ? displayTitle(p.bib, p.folder) : null;
  };

  const run = async () => {
    setBusy(true);
    try {
      await props.recheck();
    } finally {
      setBusy(false);
    }
  };

  return (
    <Screen title={t("problems.title")} onBack={() => void nav.back()}>
      <div className="content">
        <p className="muted small">{t("problems.intro")}</p>
        {props.problems.length === 0 ? <p role="status">{t("problems.none")}</p> : null}
        <ul className="problem-list">
          {props.problems.map((p, i) => (
            <li key={`${p.kind}-${p.path}-${i}`}>
              {problemMessage(p.kind)}
              {paperName(p.paper) ? (
                <div className="muted small">{t("problem.paper", { name: paperName(p.paper) ?? "" })}</div>
              ) : null}
            </li>
          ))}
        </ul>
        <Button disabled={busy} onClick={() => void run()}>
          {t("problems.recheck")}
        </Button>
        {busy ? (
          <p className="muted" role="status">
            <Spinner dark /> {t("problems.checking")}
          </p>
        ) : null}
      </div>
    </Screen>
  );
}
