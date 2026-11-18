// 文獻資訊的一節「參考文獻（N 條，庫裡有 K 篇）」：沒抽過不顯示。每條一列：編號、原文
// （兩行，點了看全文）、狀態（連上的篇名、待確認的候選與「是這篇」）；列尾的「⋯」是第二層：不是這篇、指定、看原文、
// 撤銷否決。重抽後對不上的判斷收在最後，可以清除。

import "./links.css";
import { useCallback, useEffect, useState } from "react";
import { api, kindOf, type JudgmentKind, type PaperSummary } from "../../api";
import { matchesQuery } from "../../catalog";
import { usePoll, useStackLayer } from "../../hooks";
import { Button, Details, IconButton, Sheet } from "../../ui/components";
import type { InfoSectionProps } from "../types";
import { linksApi, type EntryView, type LinksView } from "./api";
import { forgetLinks } from "./regions";
import { s } from "./strings";

/** 整理中時隔多久重讀一次。 */
const BUILDING_POLL_MS = 3000;
/** 挑一篇的清單最多列幾篇（其餘用搜尋找）。 */
const PICK_LIMIT = 60;

const titleOf = (t: string) => t.trim() || s("refs.untitled");

export function ReferencesSection(props: InfoSectionProps) {
  const { paperId, refresh, Heading, openPaper, showSource } = props;
  const [view, setView] = useState<LinksView | null>(null);
  const [expanded, setExpanded] = useState<ReadonlySet<number>>(new Set());
  const [target, setTarget] = useState<EntryView | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const menu = useStackLayer();
  const picker = useStackLayer();

  const load = useCallback(async () => {
    try {
      setView(await linksApi.linksFor(paperId));
    } catch {
      setView(null);
    }
  }, [paperId]);
  // `refresh` 變了（文獻資訊又回到最上面）就重讀
  useEffect(() => {
    void load();
    // oxlint-disable-next-line react/exhaustive-effect-dependencies
  }, [load, refresh]);
  usePoll(view?.building === true, load, BUILDING_POLL_MS);

  if (!view || view.entries.length === 0) return null;

  /** 改判斷（加或刪），之後重讀；判斷到上限時說一句。 */
  const act = async (run: (token: string) => Promise<unknown>) => {
    setNote(null);
    try {
      await run(view.token);
      forgetLinks(paperId);
    } catch (e) {
      setNote(kindOf(e) === "InvalidInput" ? s("refs.full") : s("refs.failed"));
    }
    await load();
  };
  const judge = (e: EntryView, kind: JudgmentKind, paper: string) =>
    act((token) => api.linkJudgmentAdd(paperId, e.index, kind, paper, token));
  const clear = (ids: string[]) => act((token) => api.linkJudgmentsRemove(paperId, ids, token));

  const linked = new Set(view.entries.flatMap((e) => (e.link ? [e.link.paper] : [])));
  const toggle = (i: number) =>
    setExpanded((cur) => {
      const next = new Set(cur);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
  /** 關掉選單再做（返回鍵那一層先退掉）。 */
  const fromMenu = (run: () => void | Promise<void>) => async () => {
    await menu.hide();
    await run();
  };

  return (
    <>
      <Heading>
        {s("refs.heading", { n: view.entries.length, k: linked.size })}
        {view.building ? <span className="muted small"> {s("refs.building")}</span> : null}
      </Heading>
      {note ? (
        <p className="notice" role="alert">
          {note}
        </p>
      ) : null}
      <ul className="list lk-list">
        {view.entries.map((e) => (
          <li key={e.index} className="row">
            <div className="row-main">
              <span className="lk-num">{e.label ?? ""}</span>
              <span className="row-text">
                <span
                  className={expanded.has(e.index) ? "lk-text" : "lk-text clamp"}
                  lang="en"
                  role="button"
                  tabIndex={0}
                  aria-expanded={expanded.has(e.index)}
                  onClick={() => toggle(e.index)}
                  onKeyDown={(k) => (k.key === "Enter" || k.key === " ") && toggle(e.index)}
                >
                  {e.text}
                </span>
                {e.link ? (
                  <span className="lk-status">
                    <button type="button" className="lk-open" lang="en" onClick={() => openPaper(e.link?.paper ?? "")}>
                      {titleOf(e.link.title)}
                    </button>
                  </span>
                ) : e.pending.length > 0 ? (
                  <span className="lk-status">
                    <span className="lk-note">{s("refs.maybe")}</span>
                    {e.pending.map((p) => (
                      <span key={p.paper} className="lk-status">
                        <button type="button" className="lk-open" lang="en" onClick={() => openPaper(p.paper)}>
                          {titleOf(p.title)}
                        </button>
                        <Button onClick={() => void judge(e, "confirm", p.paper)}>{s("refs.isThis")}</Button>
                      </span>
                    ))}
                  </span>
                ) : null}
              </span>
            </div>
            <IconButton
              label={s("refs.more")}
              onClick={() => {
                setTarget(e);
                menu.show();
              }}
            >
              ⋯
            </IconButton>
          </li>
        ))}
      </ul>
      {view.lost.length > 0 ? (
        <Details label={s("refs.lost.heading", { n: view.lost.length })}>
          <p className="muted small">{s("refs.lost.hint")}</p>
          <ul className="list lk-list">
            {view.lost.map((j) => (
              <li key={j.judgment} className="row">
                <span className="row-main">
                  <span className="row-text">
                    <span className="lk-text" lang="en">
                      {j.text}
                    </span>
                  </span>
                </span>
                <Button onClick={() => void clear([j.judgment])}>{s("refs.lost.clear")}</Button>
              </li>
            ))}
          </ul>
          {view.lost.length > 1 ? (
            <Button block onClick={() => void clear(view.lost.map((j) => j.judgment))}>
              {s("refs.lost.clearAll")}
            </Button>
          ) : null}
        </Details>
      ) : null}

      <Sheet
        open={menu.open && target !== null}
        title={target?.number != null ? s("refs.menu.title", { n: target.number }) : s("refs.menu.titleNoNumber")}
        onDismiss={() => void menu.hide()}
      >
        {target ? (
          <div className="lk-menu">
            {target.link ? (
              <Button block onClick={fromMenu(() => judge(target, "reject", target.link?.paper ?? ""))}>
                {s("refs.notThis")}
              </Button>
            ) : null}
            <Button block onClick={fromMenu(() => picker.show())}>
              {target.link ? s("refs.pickOther") : s("refs.pick")}
            </Button>
            <Button block onClick={fromMenu(() => showSource(target.page, target.bbox))}>
              {s("refs.showSource")}
            </Button>
            {target.rejected.map((r) => (
              <Button key={r.judgment} block onClick={fromMenu(() => clear([r.judgment]))}>
                {s("refs.undo", { title: titleOf(r.title) })}
              </Button>
            ))}
          </div>
        ) : null}
      </Sheet>
      <PickPaper
        open={picker.open && target !== null}
        exclude={paperId}
        onPick={async (id) => {
          await picker.hide();
          if (target) await judge(target, "manual", id);
        }}
        onDismiss={() => void picker.hide()}
      />
    </>
  );
}

/** 從庫裡搜篇名挑一篇（「指定」）。 */
function PickPaper(props: { open: boolean; exclude: string; onPick: (id: string) => void; onDismiss: () => void }) {
  const [papers, setPapers] = useState<PaperSummary[] | null>(null);
  const [query, setQuery] = useState("");
  const { open } = props;
  useEffect(() => {
    if (!open) return;
    setQuery("");
    api
      .listPapers()
      .then(setPapers)
      .catch(() => setPapers([]));
  }, [open]);
  const shown = (papers ?? []).filter((p) => p.id !== props.exclude && matchesQuery(p, query)).slice(0, PICK_LIMIT);
  return (
    <Sheet open={open} title={s("refs.pick.title")} onDismiss={props.onDismiss}>
      <input
        className="input"
        type="search"
        value={query}
        placeholder={s("refs.pick.search")}
        aria-label={s("refs.pick.search")}
        onChange={(e) => setQuery(e.target.value)}
      />
      <ul className="list lk-pick">
        {papers !== null && shown.length === 0 ? <li className="muted small">{s("refs.pick.none")}</li> : null}
        {shown.map((p) => (
          <li key={p.id} className="row">
            <button type="button" className="row-main" onClick={() => props.onPick(p.id)}>
              <span className="row-text">
                <span className="row-title">{titleOf(p.bib.title ?? p.folder)}</span>
                <span className="row-sub">{p.bib.year ?? " "}</span>
              </span>
            </button>
          </li>
        ))}
      </ul>
    </Sheet>
  );
}
