// 閱讀紀錄：依日期分組，列出哪天打開了哪篇、讀到第幾頁；點一列打開那篇。
// 資料來自核心的讀事件（外殼的 `reading_history`，每次打開一列）；同一天打開同一篇好幾次，合成一列（取最近一次）。

import { useEffect, useMemo, useState } from "react";
import { api, kindOf, type ErrorKind, type HistoryEntry, type PaperSummary } from "../../api";
import { dayHeading, dayKey, displayTitle, timeOfDay } from "../../format";
import type { Nav } from "../../nav";
import { t } from "../../strings";
import { Button, KindNotice, Screen, Spinner } from "../../ui/components";

interface Row {
  paper: PaperSummary;
  t: string;
  page: number | null;
}

interface Day {
  key: string;
  heading: string;
  rows: Row[];
}

/** 每次打開一列（最近的在前）→ 依手機的日期分組、同一天同一篇合成一列。 */
export function groupByDay(entries: HistoryEntry[], papers: PaperSummary[], now: Date = new Date()): Day[] {
  const byId = new Map(papers.map((p) => [p.id, p]));
  const days: Day[] = [];
  for (const e of entries) {
    const paper = byId.get(e.paper_id);
    if (!paper) continue;
    const key = dayKey(e.t);
    let day = days.at(-1);
    if (!day || day.key !== key) {
      day = { key, heading: dayHeading(e.t, now), rows: [] };
      days.push(day);
    }
    const same = day.rows.find((r) => r.paper.id === paper.id);
    if (same) {
      // 同一天同一篇：留最近那次的時間；頁碼取最近一次有記下的
      if (same.page === null && e.page !== null) same.page = e.page;
      continue;
    }
    day.rows.push({ paper, t: e.t, page: e.page });
  }
  return days;
}

export function History(props: { nav: Nav; papers: PaperSummary[] }) {
  const { nav } = props;
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [error, setError] = useState<ErrorKind | null>(null);

  const load = () => {
    setError(null);
    api
      .readingHistory()
      .then(setEntries)
      .catch((e: unknown) => setError(kindOf(e)));
  };
  useEffect(load, []);

  const days = useMemo(() => (entries ? groupByDay(entries, props.papers) : []), [entries, props.papers]);

  return (
    <Screen title={t("history.title")} onBack={() => void nav.back()}>
      {error ? <KindNotice kind={error} lead={t("history.loadFailed")} onAction={load} /> : null}
      {!error && entries === null ? (
        <div className="center-fill">
          <Spinner dark />
          <span className="muted">{t("common.loading")}</span>
        </div>
      ) : null}
      {entries !== null && days.length === 0 ? (
        <div className="empty">
          <h2>{t("history.empty.title")}</h2>
          <p className="muted">{t("history.empty.body")}</p>
          <Button primary onClick={() => void nav.back()}>
            {t("common.back")}
          </Button>
        </div>
      ) : null}
      {days.map((day) => (
        <section key={day.key} aria-label={day.heading}>
          <h2 className="day-heading">{day.heading}</h2>
          <ul className="list">
            {day.rows.map((r) => (
              <li key={r.paper.id} className="row">
                <button
                  type="button"
                  className="row-main"
                  onClick={() => nav.push({ name: "reader", paperId: r.paper.id })}
                >
                  <span className="row-text">
                    <span className="row-title">{displayTitle(r.paper.bib, r.paper.folder)}</span>
                    <span className="row-sub">
                      {[r.page !== null ? t("home.status.page", { page: r.page }) : "", timeOfDay(r.t)]
                        .filter(Boolean)
                        .join(" · ")}
                    </span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </Screen>
  );
}
