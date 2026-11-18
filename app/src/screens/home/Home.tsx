import { useEffect, useMemo, useRef, useState } from "react";
import { useLongPress } from "./useLongPress";
import { AppError, type Group, type PaperSummary, type Problem, type ReadStatus, type SortKey } from "../../api";
import {
  ALL_VIEW,
  applyCatalog,
  hasContent,
  HAS_FILTERS,
  isNarrowed,
  recentlyAdded,
  searchScope,
  viewKey,
  type HasFilter,
  type View,
} from "../../catalog";
import { ExtSearchSections } from "../../ext/SearchSections";
import { authorYear, displayTitle, relativeDay } from "../../format";
import type { Library } from "../../library";
import type { Nav, SheetKind } from "../../nav";
import type { PrefsState } from "../../prefs";
import { errorDetail, errorMessage, t, type StringKey } from "../../strings";
import { Button, CheckRow, Choice, ChoiceList, Details, ErrorNotice, IconButton, Sheet, Spinner } from "../../ui/components";
import { CloseIcon, MenuIcon, PlusIcon, SearchIcon, StatusDot } from "../../ui/icons";
import { Cover } from "./Cover";
import { MoreIcon, StackIcon } from "./homeIcons";
import { hasTranslation, pageText, readPage } from "./paperMeta";
import { Sidebar } from "./Sidebar";
import { AnimatePresence, m as Motion, useMotion as useMotionPreset } from "../../ui/motion";
import { useHomeActions } from "./useHomeActions";

/** 最近加入那一排最多幾篇。 */
const RECENT_MAX = 10;
/** 搜尋字串停頓多久才交給外掛的搜尋各節。 */
const SECTION_QUERY_DELAY_MS = 300;

/** 列上的閱讀狀態寫成字：未讀、讀到第 7／30 頁、已讀。狀態本身由核心算好。 */
function statusText(p: PaperSummary): string {
  switch (p.progress.status) {
    case "Read":
      return t("home.status.read");
    case "Reading": {
      const rp = readPage(p);
      if (rp && rp.page) {
        return rp.total !== null
          ? t("home.status.pageOf", { page: rp.page, total: rp.total })
          : t("home.status.page", { page: rp.page });
      }
      return t("home.status.reading");
    }
    default:
      return t("home.status.unread");
  }
}

const SORT_OPTIONS: Array<[SortKey, StringKey]> = [
  ["recent", "home.sort.recent"],
  ["added", "home.sort.added"],
  ["title", "info.field.title"],
  ["author", "home.sort.author"],
  ["year", "info.field.year"],
];

const STATUS_CHIPS: Array<[ReadStatus, StringKey]> = [
  ["Unread", "home.status.unread"],
  ["Reading", "home.status.reading"],
  ["Read", "home.status.read"],
];

const HAS_LABELS: Record<HasFilter, StringKey> = {
  translation: "home.filter.translation",
  digest: "home.filter.digest",
  notes: "home.filter.notes",
};

// 上次的篩選記在這台裝置上（下次打開首頁照上次的篩選）。記不住也沒關係，就是不篩
const FILTER_KEY = "pr-home-filter";

function readFilter(): { status: ReadStatus | null; has: HasFilter[] } {
  try {
    const v = JSON.parse(localStorage.getItem(FILTER_KEY) ?? "null") as { status?: unknown; has?: unknown } | null;
    const status = STATUS_CHIPS.some(([s]) => s === v?.status) ? (v?.status as ReadStatus) : null;
    const raw: unknown = v?.has;
    const has = Array.isArray(raw) ? HAS_FILTERS.filter((h) => raw.includes(h)) : [];
    return { status, has };
  } catch {
    return { status: null, has: [] };
  }
}

function writeFilter(v: { status: ReadStatus | null; has: HasFilter[] }) {
  try {
    localStorage.setItem(FILTER_KEY, JSON.stringify(v));
  } catch {
    // 記不住下次就不篩
  }
}

function viewTitle(view: View, groups: Group[]): string {
  switch (view.kind) {
    case "all":
      return t("sidebar.all");
    case "ungrouped":
      return t("sidebar.ungrouped");
    case "group":
      return groups.find((g) => g.id === view.id)?.name ?? t("sidebar.all");
  }
}

function listHeading(view: View): string {
  switch (view.kind) {
    case "all":
      return t("sidebar.all");
    case "ungrouped":
      return t("home.section.ungrouped");
    case "group":
      return t("home.section.group");
  }
}

/** 列上的一字標記（「譯」「筆」）：讀屏軟體念完整的說法。 */
function Tag(props: { short: string; full: string }) {
  return (
    <span className="tag">
      <span aria-hidden="true">{props.short}</span>
      <span className="sr-only">{props.full}</span>
    </span>
  );
}

/** 一條細的進度條（只是看個大概，真正的頁數寫在旁邊的字裡，所以不唸給讀屏軟體）。 */
function Bar(props: { fraction: number }) {
  return (
    <div className="bar" aria-hidden="true">
      <i style={{ width: `${Math.round(props.fraction * 100)}%` }} />
    </div>
  );
}

export function Home(props: {
  lib: Library;
  nav: Nav;
  /** 蓋在首頁上的面板（側邊欄、排序選單、搜尋列、群組與文獻列的選單）。 */
  sheets: SheetKind[];
  prefs: PrefsState;
  /** 目前看的是哪一批文獻（全部文獻、未分組、某個群組）。 */
  view: View;
  onView: (v: View) => void;
  /** 打開文獻庫後背景檢查找到的問題；空的就什麼都不顯示。 */
  problems: Problem[];
  onAdd: () => void;
  /** 頂端的「＋」：選 PDF 或整批匯入。空的首頁上的「加入第一篇文獻」仍直接選 PDF。 */
  onAddMenu: () => void;
  /** 原本的文獻庫不見了：使用者確認要在原位置建一個新的。 */
  onCreateHere: () => void;
  /** 從另一台加入（文獻庫還沒建立或還是空的時）。 */
  onJoin: () => void;
  /** 上一次的合併沒做完：把它做完。 */
  onResumeSync: () => void;
  highlightId: string | null;
  /** 顯示一則要使用者按「確定」的訊息（群組與譯本的操作出錯時用）。 */
  notify: (title: string, message: string, more?: { detail?: string }) => void;
  /** 一則短暫的訊息（成功時用）。 */
  toast: (message: string) => void;
}) {
  const { lib, nav, prefs } = props;
  const status = lib.status;

  const groupIds = useMemo(() => new Set(lib.groups.map((g) => g.id)), [lib.groups]);
  // 看著的群組被刪掉了（或在別處被刪）：退回全部文獻
  const view: View = props.view.kind === "group" && !groupIds.has(props.view.id) ? ALL_VIEW : props.view;

  // 多選：長按一列進入，是導覽堆疊裡的一層（返回鍵先結束多選）；勾選的文獻代號只活在多選期間
  const selecting = nav.stack.some((r) => r.name === "sheet" && r.kind === "select");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  // 多選那一層上面沒有別的面板時才會呼叫，所以退一層就是結束多選
  const endSelect = () => void nav.back();
  useEffect(() => {
    if (!selecting) setSelected(new Set());
  }, [selecting]);

  const actions = useHomeActions({
    lib,
    nav,
    view,
    onView: props.onView,
    notify: props.notify,
    toast: props.toast,
    onBulkDone: endSelect,
  });

  // 搜尋列是導覽堆疊裡的一層（返回鍵先收起它）；只要那一層還在堆疊裡，搜尋就還在（點進結果、再回來）
  const searchOpen = nav.stack.some((r) => r.name === "sheet" && r.kind === "search");
  const [query, setQuery] = useState("");
  // 篩選：閱讀狀態一次選一個；內容（有譯本、有重點筆記、有筆記）可以同時選幾項。照上次的篩選開始
  const [filter, setFilter] = useState<ReadStatus | null>(() => readFilter().status);
  const [has, setHas] = useState<HasFilter[]>(() => readFilter().has);
  useEffect(() => writeFilter({ status: filter, has }), [filter, has]);
  const toggleHas = (h: HasFilter) => setHas((cur) => (cur.includes(h) ? cur.filter((x) => x !== h) : [...cur, h]));
  // 照上次的篩選開始時，選著的那一顆可能在篩選列捲出去的地方：第一次出現篩選列時把它捲進來，看得出清單是篩過的
  const chipsRef = useRef<HTMLDivElement>(null);
  const showsChips = lib.papers.length > 0;
  useEffect(() => {
    if (!showsChips) return;
    chipsRef.current?.querySelector('.chip[aria-pressed="true"]')?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [showsChips]);
  useEffect(() => {
    if (!searchOpen) setQuery("");
  }, [searchOpen]);
  // 外掛的搜尋各節（內文）：字串停頓 300 ms 後才換；範圍只套群組與篩選，不套搜尋字串
  const [settledQuery, setSettledQuery] = useState("");
  useEffect(() => {
    const id = window.setTimeout(() => setSettledQuery(query), SECTION_QUERY_DELAY_MS);
    return () => window.clearTimeout(id);
  }, [query]);
  const scope = useMemo(
    () => searchScope(lib.papers, { view, status: filter, has, groupIds }),
    [lib.papers, view, filter, has, groupIds],
  );

  // 「有筆記」由核心在目錄摘要裡算好（`has_notes`），不必另外問
  const shown = useMemo(
    () =>
      applyCatalog(lib.papers, {
        view,
        query,
        status: filter,
        has,
        sortKey: prefs.prefs.sort_key,
        sortReverse: prefs.prefs.sort_reverse,
        groupIds,
      }),
    [lib.papers, view, query, filter, has, prefs.prefs.sort_key, prefs.prefs.sort_reverse, groupIds],
  );
  // 剛加入或帶到的那一篇被記住的篩選擋住了：清掉篩選，讓它出現在清單上（加入之後要在首頁看得到）
  useEffect(() => {
    const id = props.highlightId;
    if (!id || shown.some((p) => p.id === id) || (filter === null && has.length === 0)) return;
    setFilter(null);
    setHas([]);
    // 只在換了要指出的那一篇時看一次；之後使用者自己再選的篩選不動
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [props.highlightId]);
  const narrowed = isNarrowed({ query, status: filter, has });
  // 「繼續閱讀」只在搜尋時不出現（免得被誤認成搜尋結果）；篩選會記住，所以篩著時照樣一點就回到上次那一頁。
  // 「最近加入」只在沒有搜尋、沒有篩選時出現。兩者都只在看全部文獻時
  const showCont = query.trim() === "" && view.kind === "all";
  const cont = lib.continueId ? lib.papers.find((p) => p.id === lib.continueId) : undefined;
  const recent = useMemo(() => recentlyAdded(lib.papers, RECENT_MAX), [lib.papers]);
  const showRecent = !narrowed && view.kind === "all" && !(recent.length === 1 && recent[0]?.id === cont?.id);

  const highlightRef = useRef<HTMLLIElement | null>(null);
  // 篩選被清掉（上面）之後那一列才出現，所以篩選變了也再捲一次
  useEffect(() => {
    if (props.highlightId) highlightRef.current?.scrollIntoView({ block: "center" });
  }, [props.highlightId, narrowed]);

  const open = (p: PaperSummary) => nav.push({ name: "reader", paperId: p.id });
  const sortName = t(SORT_OPTIONS.find(([k]) => k === prefs.prefs.sort_key)?.[1] ?? "home.sort.recent");
  const sortLabel = prefs.prefs.sort_reverse ? t("home.sort.reversed", { name: sortName }) : sortName;
  const clearNarrowing = () => {
    setQuery("");
    setFilter(null);
    setHas([]);
  };

  const toggle = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const startSelect = (id: string) => {
    if (!selecting) nav.push({ name: "sheet", kind: "select" });
    setSelected((prev) => new Set(prev).add(id));
  };
  const selectedIds = lib.papers.filter((p) => selected.has(p.id)).map((p) => p.id);

  const press = useLongPress(startSelect, !selecting);

  // 頂列換成多選列或搜尋列：在頂列內淡入；換群組時清單淡入
  const fade = useMotionPreset("fade");
  const topBar = selecting ? (
    <Motion.header key="select" className="topbar select-bar" initial={fade.initial} animate={fade.animate}>
      <IconButton label={t("select.exit")} onClick={endSelect}>
        <CloseIcon />
      </IconButton>
      <h1 aria-live="polite">{t("select.count", { n: selectedIds.length })}</h1>
      <button
        type="button"
        className="topbar-text"
        disabled={selectedIds.length === 0}
        onClick={() => actions.openBibExport(selectedIds, view.kind === "group" ? viewTitle(view, lib.groups) : null)}
      >
        {t("bib.title")}
      </button>
      <button
        type="button"
        className="topbar-text"
        disabled={selectedIds.length === 0}
        onClick={() => actions.openBulkGroup(selectedIds)}
      >
        {t("group.pick.title")}
      </button>
    </Motion.header>
  ) : searchOpen ? (
    <Motion.header key="search" className="topbar search-bar" initial={fade.initial} animate={fade.animate}>
      <IconButton label={t("home.search.close")} onClick={() => void nav.back()}>
        <CloseIcon />
      </IconButton>
      <input
        className="search-input"
        type="search"
        inputMode="search"
        enterKeyHint="search"
        autoFocus
        value={query}
        placeholder={t("home.search.placeholder")}
        aria-label={t("home.search")}
        onChange={(e) => setQuery(e.target.value)}
      />
      {query !== "" ? (
        <IconButton label={t("home.search.clear")} onClick={() => setQuery("")}>
          <CloseIcon />
        </IconButton>
      ) : null}
    </Motion.header>
  ) : (
    <Motion.header key="main" className="topbar" initial={fade.initial} animate={fade.animate}>
      <IconButton label={t("home.menu")} onClick={() => nav.push({ name: "sheet", kind: "sidebar" })}>
        <MenuIcon />
      </IconButton>
      <h1>{viewTitle(view, lib.groups)}</h1>
      <IconButton label={t("home.search")} onClick={() => nav.push({ name: "sheet", kind: "search" })}>
        <SearchIcon />
      </IconButton>
      <IconButton label={t("home.add")} onClick={props.onAddMenu}>
        <PlusIcon />
      </IconButton>
    </Motion.header>
  );

  const overlays = (
    <>
      <AnimatePresence>
      {props.sheets.includes("sidebar") ? (
        <Sidebar
          key="sidebar"
          lib={lib}
          nav={nav}
          view={view}
          onView={props.onView}
          groupIds={groupIds}
          onGroupMenu={actions.openGroupMenu}
          onNewGroup={actions.newGroup}
        />
      ) : null}
      </AnimatePresence>
      <Sheet open={props.sheets.includes("sort")} title={t("home.sort.title")} onDismiss={() => void nav.back()}>
        <ChoiceList label={t("home.sort.title")}>
          {SORT_OPTIONS.map(([key, label]) => (
            <Choice
              key={key}
              checked={prefs.prefs.sort_key === key}
              label={t(label)}
              onPick={() => {
                prefs.update({ sort_key: key });
                void nav.back();
              }}
            />
          ))}
        </ChoiceList>
        <CheckRow checked={prefs.prefs.sort_reverse} onChange={(v) => prefs.update({ sort_reverse: v })}>
          {t("home.sort.reverse")}
        </CheckRow>
      </Sheet>
      {actions.overlays(props.sheets)}
    </>
  );

  const wrap = (body: React.ReactNode) => (
    <>
      <div className="screen home">
        {topBar}
        {body}
      </div>
      {overlays}
    </>
  );

  // 第一次讀取就失敗：說明出了什麼事，不停在轉圈
  if (status === null && lib.error) {
    const kind = lib.error instanceof AppError ? lib.error.kind : "Unknown";
    return wrap(
      <div className="scroll">
        <ErrorNotice
          message={t("home.loadFailed")}
          detail={[errorMessage(kind), errorDetail(kind)].filter(Boolean).join(" ")}
          onAction={() => void lib.refresh()}
        />
      </div>,
    );
  }

  if (status === null) {
    return wrap(
      <div className="center-fill">
        <Spinner dark />
        <span className="muted">{t("common.loading")}</span>
      </div>,
    );
  }

  if (status.state === "access_lost") {
    return wrap(
      <div className="empty">
        <h2>{t("home.lostAccess.title")}</h2>
        <p className="muted">{t("home.lostAccess.body")}</p>
        <Button primary onClick={props.onAdd}>
          {t("home.lostAccess.action")}
        </Button>
      </div>,
    );
  }

  if (status.state === "missing") {
    return wrap(
      <div className="empty">
        <h2>{t("home.missing.title")}</h2>
        <p className="muted">{t("home.missing.body")}</p>
        <Button primary onClick={props.onCreateHere}>
          {t("home.missing.action")}
        </Button>
        <Details>
          <p>{t("home.missing.detail")}</p>
          {status.path ? <p style={{ overflowWrap: "anywhere" }}>{status.path}</p> : null}
        </Details>
      </div>,
    );
  }

  const hasPapers = lib.papers.length > 0;
  const contPage = cont ? readPage(cont) : null;
  const contSub = cont ? authorYear(cont.bib) : "";

  return wrap(
    <>
      {hasPapers ? (
        <div className="controls">
          <div className="chips" role="group" aria-label={t("home.filter.label")} ref={chipsRef}>
            {STATUS_CHIPS.map(([value, label]) => (
              <button
                key={value}
                type="button"
                className="chip"
                aria-pressed={filter === value}
                onClick={() => setFilter(filter === value ? null : value)}
              >
                <span className="chip-face">{t(label)}</span>
              </button>
            ))}
            <span className="chips-sep" aria-hidden="true" />
            {HAS_FILTERS.map((h) => (
              <button key={h} type="button" className="chip" aria-pressed={has.includes(h)} onClick={() => toggleHas(h)}>
                <span className="chip-face">{t(HAS_LABELS[h])}</span>
              </button>
            ))}
          </div>
        </div>
      ) : null}
      <Motion.div key={viewKey(view)} className="scroll" initial={fade.initial} animate={fade.animate}>
        {status.unfinishedSync ? (
          <div className="notice" role="status">
            {t("sync.unfinished")}
            <div>
              <Button onClick={props.onResumeSync}>{t("sync.unfinished.retry")}</Button>
            </div>
          </div>
        ) : status.state === "read_only" ? (
          <div className="notice">{t("common.readOnly")}</div>
        ) : null}
        {lib.stale ? (
          <div className="notice" role="status">
            {t("home.stale")}
            <div>
              <Button onClick={() => void lib.foreground()}>{t("common.retry")}</Button>
            </div>
          </div>
        ) : null}
        {props.problems.length > 0 ? (
          <div className="notice" role="status" data-problems={props.problems.length}>
            {t("home.problems", { n: props.problems.length })}
            <div>
              <Button onClick={() => nav.push({ name: "problems" })}>{t("home.problems.action")}</Button>
            </div>
          </div>
        ) : null}
        {!hasPapers ? (
          <div className="empty home-empty">
            <div className="home-empty-art">
              <StackIcon />
            </div>
            <h2>{t("home.empty.title")}</h2>
            <p className="muted">{t("home.empty.body")}</p>
            <Button primary onClick={props.onAdd}>
              {t("home.empty.action")}
            </Button>
            {/* 第一次開啟（還沒有文獻庫或還是空的）：可以從另一台的同步包或備份檔加入同一個文獻庫 */}
            {lib.trashCount === 0 && lib.groups.length === 0 && status.state !== "read_only" ? (
              <div className="home-join">
                <Button onClick={props.onJoin}>{t("join.action")}</Button>
              </div>
            ) : null}
            {lib.trashCount > 0 ? (
              <div className="notice" style={{ marginTop: 24 }}>
                {t("home.trashHint", { n: lib.trashCount })}
                <div>
                  <Button onClick={() => nav.push({ name: "trash" })}>{t("home.trashHint.action")}</Button>
                </div>
              </div>
            ) : null}
          </div>
        ) : (
          <div className="home-inner">
            {cont && showCont ? (
              <button type="button" className="cont-card" onClick={() => open(cont)}>
                <Cover paper={cont} size="lg" />
                <span className="cc-body">
                  <span className="cc-label">{t("home.continue.heading")}</span>
                  <span className="cc-title">{displayTitle(cont.bib, cont.folder)}</span>
                  {contSub ? <span className="cc-sub">{contSub}</span> : null}
                  <span className="cc-sub">
                    {[contPage ? pageText(contPage) : "", relativeDay(cont.progress.last_read_at)]
                      .filter(Boolean)
                      .join(" · ")}
                  </span>
                  {contPage?.fraction != null ? <Bar fraction={contPage.fraction} /> : null}
                  <span className="cc-go">{t("home.continue.go")}</span>
                </span>
              </button>
            ) : null}

            {showRecent && recent.length > 0 ? (
              <section>
                <h2 className="sec-title pad">{t("home.section.recent")}</h2>
                <ul className="rail" aria-label={t("home.rail.label")}>
                  {recent.map((p) => (
                    <li key={p.id}>
                      <button type="button" className="rail-card" onClick={() => open(p)}>
                        <Cover paper={p} size="md" />
                        <span className="rail-title">{displayTitle(p.bib, p.folder)}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              </section>
            ) : null}

            <div className="sec-head">
              <div className="sec-titles">
                <h2 className="sec-title">{listHeading(view)}</h2>
                <span className="sec-count" aria-live="polite">
                  {t("home.count", { n: shown.length })}
                </span>
              </div>
              <button
                type="button"
                className="sort-btn"
                aria-label={t("home.sort.label", { name: sortLabel })}
                onClick={() => nav.push({ name: "sheet", kind: "sort" })}
              >
                <span>{sortLabel}</span>
                <span aria-hidden="true">▾</span>
              </button>
            </div>

            {shown.length === 0 ? (
              narrowed ? (
                <div className="empty">
                  <h2>{t("home.noMatch.title")}</h2>
                  <Button onClick={clearNarrowing}>{t("home.noMatch.clear")}</Button>
                </div>
              ) : (
                <div className="empty">
                  <h2>
                    {view.kind === "ungrouped" ? t("group.ungrouped.empty.title") : t("group.empty.title")}
                  </h2>
                  <p className="muted">
                    {view.kind === "ungrouped" ? t("group.ungrouped.empty.body") : t("group.empty.body")}
                  </p>
                  <Button onClick={() => props.onView(ALL_VIEW)}>{t("group.empty.back")}</Button>
                </div>
              )
            ) : (
              <ul className="paper-list">
                {shown.map((p) => {
                  const title = displayTitle(p.bib, p.folder);
                  const sub = authorYear(p.bib);
                  const rp = p.progress.status === "Reading" ? readPage(p) : null;
                  return (
                    <li
                      key={p.id}
                      className={[p.id === props.highlightId ? "paper-row hl" : "paper-row", selecting ? "selecting" : ""]
                        .filter(Boolean)
                        .join(" ")}
                      ref={p.id === props.highlightId ? highlightRef : undefined}
                    >
                      <button
                        type="button"
                        className="paper-main"
                        role={selecting ? "checkbox" : undefined}
                        aria-checked={selecting ? selected.has(p.id) : undefined}
                        aria-label={selecting ? t("select.item", { title }) : undefined}
                        {...press.bind(p.id)}
                        onClick={() => {
                          if (press.consumeLong()) return;
                          if (selecting) toggle(p.id);
                          else open(p);
                        }}
                      >
                        {selecting ? (
                          <span className="gcheck sel-check" aria-hidden="true" data-on={selected.has(p.id)}>
                            {selected.has(p.id) ? "✓" : ""}
                          </span>
                        ) : null}
                        <Cover paper={p} size="sm" fraction={rp?.fraction ?? null} />
                        <span className="paper-text">
                          <span className="paper-title">{title}</span>
                          {/* 一行：讀到哪（圓點在行首，上下列對齊、一眼看得出）、作者與年份（太長就省略）、「譯」「筆」 */}
                          <span className="paper-meta">
                            <StatusDot status={p.progress.status} />
                            <span className="paper-status">{statusText(p)}</span>
                            {sub ? <span className="paper-sub">{sub}</span> : null}
                            {hasTranslation(p.materials) ? <Tag short={t("home.row.translation")} full={t("translation.has")} /> : null}
                            {hasContent(p, "digest") ? <Tag short={t("home.row.digest")} full={t("home.row.digestFull")} /> : null}
                          </span>
                        </span>
                      </button>
                      {selecting ? null : (
                        <IconButton
                          label={t("home.row.menu", { title })}
                          onClick={() => actions.openPaperMenu(p.id)}
                        >
                          <MoreIcon />
                        </IconButton>
                      )}
                    </li>
                  );
                })}
              </ul>
            )}
            {searchOpen && settledQuery.trim() !== "" ? (
              <ExtSearchSections query={settledQuery} scope={scope} papers={lib.papers} nav={nav} />
            ) : null}
          </div>
        )}
      </Motion.div>
    </>,
  );
}
