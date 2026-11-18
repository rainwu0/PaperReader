// 譯文：一份譯文疊加檔的段落清單，兩種模式——
//   閱讀：排好的中文全文（標題、正文、圖說），點一段在下面展開原文、看完再點收起；長按或展開後按「改」就地改；
//         目錄、字級、看版面（到譯本 PDF 的同一段）。不顯示自動檢查的警示。
//   檢查（頁頂選單的「檢查譯文」，偶爾用）：一段原文配一段譯文、自動檢查的警示、只看有問題的、
//         並排比較服務（單段重翻）、匯出給翻譯軟體。
// 讀檔（閱讀畫面的 `useOverlayDoc`，和 PDF 上的小框、原頁疊譯文同一份）、捲動定位、改段落兩種模式共用。
// 蓋滿整個畫面（導覽堆疊裡的一層，返回鍵先關掉它）；檢查、目錄、頁頂選單、正在改的那一段也各是一層。

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Nav, SheetKind } from "../../nav";
import { api, type OverlaySegment, type TranslateSettingsView } from "../../api";
import { useLocalPref } from "../../hooks";
import { t } from "../../strings";
import { segmentServiceName, servicesUsed } from "../generate/engines";
import { Button, IconButton, InfoTip, Sheet, Spinner } from "../../ui/components";
import { BackIcon, OutlineIcon, ShareIcon } from "../../ui/icons";
import { m as Motion, useExitingStyle, useMotion } from "../../ui/motion";
import { CompareServicesSheet, type CompareTarget } from "./CompareServices";
import { OverlayTerms } from "./OverlayTerms";
import { Paragraph, RebuildBar, SegmentEditor, type OverlayDoc, type Rebuild } from "./SegmentParts";
import { editMark, tableOfContents } from "./segments";
import { LayoutIcon, MoreIcon } from "./toolIcons";

/** 自動檢查的問題代號 → 白話；不認得的代號用通用的說法。 */
const ISSUE_TEXT = {
  missing_number: "reader.compare.issue.missing_number",
  missing_citation: "reader.compare.issue.missing_citation",
  missing_formula: "reader.compare.issue.missing_formula",
  missing_url: "reader.compare.issue.missing_url",
  untranslated: "reader.compare.issue.untranslated",
  garbled: "reader.compare.issue.garbled",
  too_short: "reader.compare.issue.too_short",
} as const;

function issueText(code: string): string {
  const key = (ISSUE_TEXT as Record<string, (typeof ISSUE_TEXT)[keyof typeof ISSUE_TEXT]>)[code];
  return key ? t(key) : t("reader.compare.issue.other");
}

/** 字級（CSS 像素，約等於 sp）：預設 18，行高 1.7（A1、A7）。記在這台裝置上。 */
const SIZES = [14, 16, 18, 20, 22, 24, 26];
const isSize = (v: unknown): v is number => typeof v === "number" && SIZES.includes(v);

export function CompareView(props: {
  paperId: string;
  overlayId: string;
  overlay: OverlayDoc;
  /** 打開時從哪一段讀起（PDF 上正在讀的那一段）。 */
  startAt: string | null;
  /** 捲動時回報正在讀的那一段（回到 PDF 時停在它，A6、A15）。 */
  onReading: (segmentId: string) => void;
  /** 看版面：到譯本 PDF（或原文）的這一段（J4）。 */
  onLayout: (segmentId: string) => void;
  rebuild: Rebuild;
  /** 「譯文改過了」說過了、使用者收起來了。 */
  rebuildDismissed: boolean;
  onDismissRebuild: () => void;
  /** 原文的總頁數（頁碼標用）。 */
  totalPages: number;
  nav: Nav;
  /** 閱讀畫面上疊著的面板（檢查、目錄、選單、改、並排比較）。 */
  sheets: SheetKind[];
  onClose: () => void;
}) {
  const view = useMotion("view");
  const exiting = useExitingStyle();
  const { nav, sheets, overlay } = props;
  const doc = overlay.doc;
  const segs = useMemo(() => doc?.segments ?? [], [doc]);
  const check = sheets.includes("compareCheck");
  const [size, setSize] = useLocalPref("pr-text-size", 18, isSize);
  const [open, setOpen] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [onlyFlagged, setOnlyFlagged] = useState(false);
  const [target, setTarget] = useState<CompareTarget | null>(null);
  const [current, setCurrent] = useState<string | null>(props.startAt);
  const bodyRef = useRef<HTMLDivElement>(null);
  // 使用者自己捲過（或從目錄跳過）才回報讀到哪：只是打開再關掉，PDF 停在原處不動
  const userMoved = useRef(false);

  // 返回鍵退掉了「改」那一層：放棄這次修改
  const editLayer = sheets.includes("segEdit");
  useEffect(() => {
    if (!editLayer) setEditing(null);
  }, [editLayer]);
  const startEdit = (id: string) => {
    setOpen(null);
    setEditing(id);
    if (!editLayer) nav.push({ name: "sheet", kind: "segEdit" });
  };
  const save = async (id: string, text: string) => {
    const message = await overlay.edit(id, text);
    if (!message) await nav.back();
    return message;
  };

  // ---------- 捲動定位 ----------

  /** 讓某一段出現在畫面頂端。 */
  const scrollToSeg = (id: string) => {
    const body = bodyRef.current;
    const el = body?.querySelector<HTMLElement>(`[data-sid="${CSS.escape(id)}"]`);
    if (body && el) body.scrollTop = el.offsetTop - 8;
  };
  // 第一次有內容時捲到開始的那一段
  const positioned = useRef(false);
  useLayoutEffect(() => {
    if (positioned.current || segs.length === 0) return;
    positioned.current = true;
    if (props.startAt) scrollToSeg(props.startAt);
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [segs.length]);
  // 開始改一段：那一段捲到頂端，鍵盤彈出後原文、輸入框、「儲存」都在鍵盤上方
  useLayoutEffect(() => {
    if (editing) scrollToSeg(editing);
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [editing]);
  // 換模式、換字級時停在同一段（字變大，前面的段落變高，不重新定位就會跑掉）
  useLayoutEffect(() => {
    if (current) scrollToSeg(current);
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [check, size]);

  const raf = useRef(0);
  const track = () => {
    if (raf.current) return;
    raf.current = requestAnimationFrame(() => {
      raf.current = 0;
      const body = bodyRef.current;
      if (!body || !userMoved.current) return;
      const els = body.querySelectorAll<HTMLElement>("[data-sid]");
      // 頂端那一段：最後一個上緣不低於畫面頂端（多留一點）的
      const line = body.scrollTop + 12;
      let lo = 0;
      let hi = els.length - 1;
      while (lo < hi) {
        const mid = (lo + hi + 1) >> 1;
        if ((els[mid]?.offsetTop ?? 0) <= line) lo = mid;
        else hi = mid - 1;
      }
      const id = els[lo]?.dataset["sid"];
      if (id && id !== current) {
        setCurrent(id);
        props.onReading(id);
      }
    });
  };
  useEffect(() => () => cancelAnimationFrame(raf.current), []);
  const currentPage = (segs.find((s) => s.id === current) ?? segs[0])?.page ?? 0;

  // ---------- 目錄 ----------

  const toc = useMemo(() => tableOfContents(segs), [segs]);
  // 目錄裡標出正在讀的那一節：正在讀的那一段之前最後一個標題
  const order = useMemo(() => new Map(segs.map((s, i) => [s.id, i])), [segs]);
  const at = current ? (order.get(current) ?? -1) : -1;
  const section = toc.filter((e) => (order.get(e.id) ?? Infinity) <= at).at(-1)?.id ?? null;
  const goTo = async (id: string) => {
    await nav.back();
    userMoved.current = true;
    scrollToSeg(id);
    setCurrent(id);
    props.onReading(id);
  };

  // ---------- 檢查：服務名稱、篩選、並排比較 ----------

  const [cfg, setCfg] = useState<TranslateSettingsView | null>(null);
  useEffect(() => {
    if (!check || cfg) return;
    let alive = true;
    api
      .translateSettings()
      .then((c) => alive && setCfg(c))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [check, cfg]);
  const done = useMemo(() => segs.filter((s) => s.status === "done").length, [segs]);
  const flagged = useMemo(() => segs.filter((s) => (s.issues?.length ?? 0) > 0).length, [segs]);
  // 已翻的段落用了幾個服務：只有一個時在頂端說一次，多於一個時每一段各自標出
  const services = useMemo(() => servicesUsed(segs), [segs]);
  const perSegment = services.length > 1;
  const groups = useMemo(() => {
    const out: Array<{ page: number; segs: OverlaySegment[] }> = [];
    for (const s of segs) {
      if (onlyFlagged && !(s.issues?.length ?? 0)) continue;
      const last = out[out.length - 1];
      if (last && last.page === s.page) last.segs.push(s);
      else out.push({ page: s.page, segs: [s] });
    }
    return out;
  }, [segs, onlyFlagged]);
  const compare = (seg: OverlaySegment, scope: "segment" | "page") => {
    const list = scope === "page" ? segs.filter((x) => x.page === seg.page) : [seg];
    setTarget({ segments: list, page: seg.page, scope });
    nav.push({ name: "sheet", kind: "compareServices" });
  };

  // ---------- 畫面 ----------

  const status =
    overlay.error && !doc ? (
      <p className="field-error">{overlay.error}</p>
    ) : !doc ? (
      <p className="muted" role="status">
        <Spinner /> {t("reader.compare.loading")}
      </p>
    ) : segs.length === 0 ? (
      <p>{t("reader.compare.empty")}</p>
    ) : null;

  const { rebuild } = props;
  const banner = !check && rebuild.pdf ? <RebuildBar rebuild={rebuild} dismissed={props.rebuildDismissed} onDismiss={props.onDismissRebuild} /> : null;

  return (
    <Motion.div
      className={check ? "compare" : "compare text-view"}
      role="dialog"
      aria-label={t(check ? "reader.check.title" : "reader.compare.title")}
      style={{ ...exiting, "--text-size": `${size}px` } as React.CSSProperties}
      {...view}
    >
      {check ? (
        <header className="compare-bar">
          <IconButton label={t("common.back")} onClick={() => void nav.back()}>
            <BackIcon />
          </IconButton>
          <div className="compare-title">
            <h1>{t("reader.check.title")}</h1>
            {doc ? <span className="muted small">{t("reader.compare.count", { done, total: segs.length })}</span> : null}
          </div>
          <IconButton label={t("export.title")} onClick={() => nav.push({ name: "sheet", kind: "exportMenu" })}>
            <ShareIcon />
          </IconButton>
        </header>
      ) : (
        <header className="compare-bar">
          <IconButton label={t("common.back")} onClick={props.onClose}>
            <BackIcon />
          </IconButton>
          <div className="compare-title">
            <h1>{t("reader.compare.title")}</h1>
          </div>
          <IconButton label={t("reader.text.toc")} onClick={() => nav.push({ name: "sheet", kind: "compareToc" })}>
            <OutlineIcon />
          </IconButton>
          <IconButton label={t("reader.text.layout")} onClick={() => current && props.onLayout(current)}>
            <LayoutIcon />
          </IconButton>
          <IconButton label={t("reader.text.menu")} onClick={() => nav.push({ name: "sheet", kind: "compareMenu" })}>
            <MoreIcon />
          </IconButton>
        </header>
      )}
      {banner}
      <div
        className={check ? "compare-body" : "compare-body text-body"}
        ref={bodyRef}
        onScroll={track}
        onTouchStart={() => (userMoved.current = true)}
        onWheel={() => (userMoved.current = true)}
      >
        {status}
        {check ? (
          <>
            {services.length === 1 && services[0] ? (
              <p className="muted small compare-hint">
                {t("reader.compare.by", { name: segmentServiceName(services[0], cfg?.services ?? null) })}
              </p>
            ) : null}
            {doc ? (
              <OverlayTerms
                paperId={props.paperId}
                overlayId={props.overlayId}
                terms={doc.terms ?? []}
                onSaved={() => overlay.reload()}
              />
            ) : null}
            {flagged > 0 ? (
              <div className="compare-flag-bar">
                <span>
                  {t("reader.compare.flagged", { n: flagged })}
                  <InfoTip label={t("reader.compare.flagged", { n: flagged })}>{t("reader.compare.flaggedAbout")}</InfoTip>
                </span>
                <Button onClick={() => setOnlyFlagged((v) => !v)}>
                  {onlyFlagged ? t("reader.compare.showAll") : t("reader.compare.showFlagged")}
                </Button>
              </div>
            ) : null}
            {groups.map((g) => (
              <section key={`${g.page}-${g.segs[0]?.id}`} className="compare-page">
                <h2 className="compare-page-title">{t("reader.compare.page", { n: g.page + 1 })}</h2>
                {g.segs.map((s) => (
                  <div key={s.id} className="compare-item" data-sid={s.id}>
                    <div className="compare-seg">
                      <span className="compare-src" lang="en">
                        {s.source}
                      </span>
                      {editing === s.id ? (
                        <SegmentEditor initial={s.target ?? ""} onSave={(text) => save(s.id, text)} onCancel={() => void nav.back()} />
                      ) : s.status === "done" && s.target ? (
                        <span className="compare-tgt">
                          {s.target}
                          {s.issues?.length ? <span className="compare-issues">{s.issues.map(issueText).join("、")}</span> : null}
                          {perSegment && s.service ? (
                            <span className="compare-by">
                              {t("reader.compare.by", { name: segmentServiceName(s.service, cfg?.services ?? null) })}
                            </span>
                          ) : null}
                          {editMark(s) ? <span className="compare-by">{t(editMark(s) === "edited" ? "reader.text.edited" : "reader.compare.byUser")}</span> : null}
                        </span>
                      ) : (
                        <span className="compare-tgt compare-missing">
                          {s.status === "failed" ? t("reader.compare.failed") : t("reader.compare.pending")}
                        </span>
                      )}
                    </div>
                    {editing === s.id ? null : (
                      <div className="compare-tools">
                        <button type="button" className="btn small" onClick={() => startEdit(s.id)}>
                          {t("reader.text.edit")}
                        </button>
                        <button type="button" className="btn small" onClick={() => compare(s, "segment")}>
                          {t("compare.thisSegment")}
                        </button>
                        <button type="button" className="btn small" onClick={() => compare(s, "page")}>
                          {t("compare.thisPage")}
                        </button>
                        <button type="button" className="btn small" onClick={() => props.onLayout(s.id)}>
                          {t("reader.text.layout")}
                        </button>
                      </div>
                    )}
                  </div>
                ))}
              </section>
            ))}
          </>
        ) : (
          segs.map((s) => (
            <Paragraph
              key={s.id}
              seg={s}
              open={open === s.id}
              editing={editing === s.id}
              onToggle={() => setOpen((cur) => (cur === s.id ? null : s.id))}
              onEdit={() => startEdit(s.id)}
              onLayout={() => props.onLayout(s.id)}
              onSave={(text) => save(s.id, text)}
              onCancel={() => void nav.back()}
            />
          ))
        )}
      </div>
      {!check && segs.length > 0 ? (
        <span className="page-pill text-pill" aria-hidden="true">
          <span className="pill-face">{t("reader.page", { page: currentPage + 1, total: props.totalPages || currentPage + 1 })}</span>
        </span>
      ) : null}

      <Sheet open={sheets.includes("compareToc")} title={t("reader.text.toc")} onDismiss={() => void nav.back()}>
        {toc.length === 0 ? <p>{t("reader.text.tocNone")}</p> : null}
        <ul className="outline-list">
          {toc.map((e) => (
            <li key={e.id}>
              <button type="button" className="outline-item toc-item" aria-current={e.id === section} onClick={() => void goTo(e.id)}>
                <span>{e.text}</span>
                <span className="muted small">{t("reader.compare.page", { n: e.page + 1 })}</span>
              </button>
            </li>
          ))}
        </ul>
      </Sheet>
      <Sheet open={sheets.includes("compareMenu")} title={t("reader.text.menu")} onDismiss={() => void nav.back()}>
        <div className="tx-size-row" role="group" aria-label={t("reader.text.size")}>
          <span>{t("reader.text.size")}</span>
          <Button disabled={size <= (SIZES[0] ?? 14)} onClick={() => setSize(SIZES[SIZES.indexOf(size) - 1] ?? size)}>
            {t("reader.text.smaller")}
          </Button>
          <span className="tx-size-value">{size}</span>
          <Button disabled={size >= (SIZES.at(-1) ?? 26)} onClick={() => setSize(SIZES[SIZES.indexOf(size) + 1] ?? size)}>
            {t("reader.text.bigger")}
          </Button>
        </div>
        <ul className="outline-list">
          <li>
            <button
              type="button"
              className="outline-item"
              onClick={() => void nav.back().then(() => nav.push({ name: "sheet", kind: "compareCheck" }))}
            >
              {t("reader.check.title")}
              <span className="muted small block-hint">{t("reader.check.about")}</span>
            </button>
          </li>
        </ul>
      </Sheet>
      <CompareServicesSheet
        open={sheets.includes("compareServices") && target !== null}
        nav={nav}
        paperId={props.paperId}
        materialId={props.overlayId}
        target={target}
        onApplied={() => void overlay.reload()}
      />
    </Motion.div>
  );
}
