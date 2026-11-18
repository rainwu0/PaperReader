// 首頁搜尋的一節「內文」：書目的結果之下，每篇用首頁的文獻列，列下最多三行「原文 第 5 頁」接前後文，
// 命中的字粗體加底線（不用顏色）。點一行開到那一處；「還有 K 處」就地展開；捲到最後一篇再要下一批。

import "./fulltext.css";
import { useEffect, useRef, useState } from "react";
import type { SearchSectionProps } from "../types";
import { fulltextApi, type Hit, type PaperHits } from "./api";
import { BodySearch, longEnough, type BodyState } from "./poll";
import { s } from "./strings";
import { hitLabel, hitTarget, pieces } from "./view";

function HitLine(props: { hit: Hit; onOpen: () => void }) {
  const { hit } = props;
  return (
    <li>
      <button type="button" className="ft-hit" onClick={props.onOpen}>
        <span className="ft-where">{hitLabel(hit, s)}</span>
        <span className="ft-context">
          {pieces(hit).map((p, i) => (p.mark ? <mark key={i}>{p.text}</mark> : <span key={i}>{p.text}</span>))}
        </span>
      </button>
    </li>
  );
}

function PaperHitsView(props: { result: PaperHits; query: string; open: SearchSectionProps["openHit"] }) {
  const { result, query } = props;
  // 「還有 K 處」：就地展開那篇的每一處
  const [all, setAll] = useState<Hit[] | null>(null);
  const [busy, setBusy] = useState(false);
  const hits = all ?? result.hits;
  const rest = all ? 0 : result.total - result.hits.length;
  const expand = () => {
    setBusy(true);
    fulltextApi
      .hits(result.paper, query)
      .then((h) => setAll(h.length > 0 ? h : result.hits))
      .catch(() => setAll(result.hits))
      .finally(() => setBusy(false));
  };
  return (
    <>
      <ul className="ft-hits">
        {hits.map((h, i) => (
          <HitLine key={`${h.kind}-${h.material}-${h.page}-${h.target}-${i}`} hit={h} onOpen={() => props.open(hitTarget(result.paper, h))} />
        ))}
      </ul>
      {rest > 0 ? (
        <button type="button" className="ft-more" disabled={busy} onClick={expand}>
          {s("body.more", { k: rest })}
        </button>
      ) : null}
    </>
  );
}

export function BodySection(props: SearchSectionProps) {
  const { query, scope, Heading, Paper } = props;
  const [state, setState] = useState<BodyState | null>(null);
  const search = useRef<BodySearch | null>(null);
  if (!search.current) search.current = new BodySearch(fulltextApi.search, setState);
  const run = longEnough(query);
  const scopeKey = scope === null ? "" : scope.join(",");
  useEffect(() => {
    const b = search.current!;
    if (run) b.start(query, scope);
    else b.stop();
    return () => b.stop();
    // 範圍以內容比，不以陣列本身比（每次重算都是新的陣列）
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [query, scopeKey, run]);

  // 捲到最後一篇：要下一批
  const sentinel = useRef<HTMLDivElement>(null);
  const more = state?.more ?? false;
  useEffect(() => {
    const el = sentinel.current;
    if (!el || !more) return;
    const io = new IntersectionObserver((es) => {
      if (es.some((e) => e.isIntersecting)) search.current?.next();
    });
    io.observe(el);
    return () => io.disconnect();
  }, [more, state?.papers.length]);

  if (!run || !state || state.query !== query) return null;
  const empty = !state.loading && !state.failed && state.papers.length === 0;
  return (
    <section className="ft-body" aria-busy={state.loading || state.pending}>
      <div className="ft-head">
        <Heading>{s("body.heading")}</Heading>
        {state.building ? <span className="ft-building">{s("body.building")}</span> : null}
      </div>
      {state.loading ? <p className="ft-note">{s("body.loading")}</p> : null}
      {state.failed ? <p className="ft-note">{s("body.failed")}</p> : null}
      {empty && !state.building && !state.pending ? <p className="ft-note">{s("body.none")}</p> : null}
      {state.papers.length > 0 ? (
        <ul className="paper-list ft-papers">
          {state.papers.map((p) => (
            <Paper key={p.paper} paperId={p.paper}>
              <PaperHitsView result={p} query={query} open={props.openHit} />
            </Paper>
          ))}
        </ul>
      ) : null}
      {more ? <div ref={sentinel} className="ft-sentinel" /> : null}
      {props.unextracted > 0 ? <p className="ft-note">{s("body.unextracted", { n: props.unextracted })}</p> : null}
    </section>
  );
}
