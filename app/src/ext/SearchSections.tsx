// 首頁搜尋裡外掛的幾節：依登記處逐一放在書目的結果之後（同 `InfoSections.tsx`），套用應用的節
// 標題與文獻列，訂閱逐篇處理的進度交出「還沒抽完文字的篇數」，把「打開那一處」轉成閱讀畫面的路由。`Home.tsx` 只放
// 這一個元件。

import { createContext, useContext, useSyncExternalStore, type ReactNode } from "react";
import type { PaperSummary } from "../api";
import { authorYear, displayTitle } from "../format";
import type { Nav, Route } from "../nav";
import { perPaper } from "../perPaperApp";
import { Cover } from "../screens/home/Cover";
import { plugins } from "./index";
import type { HitTarget, SearchSection } from "./types";

/** 逐篇處理裡抽每頁文字的那件內建抽取（`perPaper.ts` 的 `textPagesTask`）。 */
const TEXT_TASK = "text.pages";

const sections: SearchSection[] = plugins.flatMap((p) => p.searchSections ?? []).sort((a, b) => a.order - b.order);

function Heading(props: { children: ReactNode }) {
  return <h2 className="sec-title pad">{props.children}</h2>;
}

const PapersContext = createContext<{ byId: Map<string, PaperSummary>; open: (id: string) => void }>({
  byId: new Map(),
  open: () => {},
});

/** 一篇文獻：首頁的文獻列（封面、篇名、作者年份），下面接外掛交的幾行。不在目錄裡（剛移除）就不顯示。 */
function Paper(props: { paperId: string; children?: ReactNode }) {
  const { byId, open } = useContext(PapersContext);
  const p = byId.get(props.paperId);
  if (!p) return null;
  const sub = authorYear(p.bib);
  return (
    <li className="hit-paper">
      <div className="paper-row">
        <button type="button" className="paper-main" onClick={() => open(p.id)}>
          <Cover paper={p} size="sm" />
          <span className="paper-text">
            <span className="paper-title">{displayTitle(p.bib, p.folder)}</span>
            {sub ? <span className="paper-meta">{sub}</span> : null}
          </span>
        </button>
      </div>
      {props.children}
    </li>
  );
}

/** 打開那一處 → 閱讀畫面的路由（頁碼在路由裡從 1 起）。 */
export function hitRoute(h: HitTarget): Route {
  return {
    name: "reader",
    paperId: h.paperId,
    ...(h.page !== undefined ? { page: h.page + 1 } : {}),
    ...(h.material ? { material: h.material } : {}),
    ...(h.range && h.quote !== undefined ? { range: h.range, quote: h.quote } : {}),
    ...(h.box ? { box: h.box } : {}),
    ...(h.view ? { view: h.view } : {}),
    ...(h.highlight ? { highlight: h.highlight } : {}),
    ...(h.point ? { point: h.point } : {}),
  };
}

export function ExtSearchSections(props: { query: string; scope: string[] | null; papers: PaperSummary[]; nav: Nav }) {
  const { nav } = props;
  const progress = useSyncExternalStore(perPaper.subscribe, perPaper.getProgress);
  const unextracted = progress.pending[TEXT_TASK] ?? 0;
  const byId = new Map(props.papers.map((p) => [p.id, p]));
  const open = (id: string) => nav.push({ name: "reader", paperId: id });
  const openHit = (h: HitTarget) => nav.push(hitRoute(h));
  if (sections.length === 0) return null;
  return (
    <PapersContext.Provider value={{ byId, open }}>
      {sections.map((s) => (
        <s.Component
          key={s.id}
          query={props.query}
          scope={props.scope}
          openHit={openHit}
          Heading={Heading}
          Paper={Paper}
          unextracted={unextracted}
        />
      ))}
    </PapersContext.Provider>
  );
}
