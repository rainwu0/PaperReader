// 重點筆記：依區段排好版顯示；每一點的頁碼點了回原文那一頁；可以標為已校對、刪掉一點、用其他應用打開、匯出。
// 蓋滿整個畫面（導覽堆疊裡的一層，返回鍵先關掉它）。內容以核心讀出的結構顯示，不把 Markdown 轉成 HTML。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Nav, SheetKind } from "../../nav";
import { api, kindOf, type DigestPoint, type DigestView as DigestData } from "../../api";
import { shortDate } from "../../format";
import { useForeground } from "../../hooks";
import { errorMessage, hasText, t, type StringKey } from "../../strings";
import { Button, ConfirmSheet, IconButton, Spinner, Toast } from "../../ui/components";
import { BackIcon, ShareIcon } from "../../ui/icons";
import { m as Motion, useExitingStyle, useMotion } from "../../ui/motion";
import { TrashIcon } from "./toolIcons";
import { digestStateLabel } from "../home/paperMeta";

type State = { phase: "loading" } | { phase: "failed"; message: string } | { phase: "ready"; data: DigestData };

export function DigestView(props: {
  paperId: string;
  materialId: string;
  /** 這份重點筆記的 `provenance.method`：`imported`（整批匯入）寫「匯入」，不寫「AI 產生」。 */
  method?: string | null;
  onClose: () => void;
  /** 回到原文的哪一頁（從 1 起算）。 */
  onJump: (page: number) => void;
  nav: Nav;
  /** 閱讀畫面上疊著的面板（刪一點的確認）。 */
  sheets: SheetKind[];
  /** 打開時捲到這一點、閃一下（首頁搜尋的「內文」來的）。 */
  focusPoint?: string | undefined;
  /** 移除這份重點筆記（移到回收區；由閱讀畫面關掉這一層、給「復原」）。 */
  onRemove?: () => void;
}) {
  const view = useMotion("view");
  const exiting = useExitingStyle();
  const { paperId, materialId, nav } = props;
  const [state, setState] = useState<State>({ phase: "loading" });
  const [toast, setToast] = useState<string | null>(null);
  const [pending, setPending] = useState<DigestPoint | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(
    (alive: () => boolean = () => true) =>
      api
        .digestRead(paperId, materialId)
        .then((data) => alive() && setState({ phase: "ready", data }))
        .catch((e) => alive() && setState({ phase: "failed", message: errorMessage(kindOf(e)) })),
    [paperId, materialId],
  );
  useEffect(() => {
    let cancelled = false;
    void load(() => !cancelled);
    return () => {
      cancelled = true;
    };
  }, [load]);
  // 在筆記工具（Obsidian 等）改完回來：重讀檔案
  useForeground(() => void load());

  const data = state.phase === "ready" ? state.data : null;
  const doc = data?.doc ?? null;
  const readOnly = !!doc && doc.format > 1;

  // 要找的那一點：讀好之後捲到畫面中間，標出來到下一次點按為止（閃一下；移除動畫時是靜態的選中樣式）
  const [found, setFound] = useState<string | null>(null);
  const focusPoint = props.focusPoint;
  // 每一個要找的點只做一次：之後重讀（標已校對、刪一點、回到前景）不再捲回去、不再閃
  const focusedRef = useRef<string | null>(null);
  useEffect(() => {
    if (!focusPoint || focusedRef.current === focusPoint || !doc?.points.some((p) => p.id === focusPoint)) return;
    focusedRef.current = focusPoint;
    setFound(focusPoint);
    const el = document.querySelector<HTMLElement>(`.digest-point[data-point="${CSS.escape(focusPoint)}"]`);
    el?.scrollIntoView({ block: "center" });
  }, [focusPoint, doc]);

  // 依檔案記下的區段分組（舊檔核心給內建五區、名稱空白，用文字表）；不在清單裡的區段放最後
  const groups = useMemo(() => {
    const pts = doc?.points ?? [];
    const sections = doc?.sections ?? [];
    const known = new Set<string>(sections.map((s) => s.id));
    const out: Array<{ key: string; label: string; points: DigestPoint[] }> = sections.map((s) => ({
      key: s.id,
      label: s.label.trim() || (hasText(`digest.section.${s.id}`) ? t(`digest.section.${s.id}` as StringKey) : s.id),
      points: pts.filter((p) => p.section === s.id),
    }));
    out.push({ key: "other", label: t("digest.section.other"), points: pts.filter((p) => !known.has(p.section)) });
    return out.filter((g) => g.points.length > 0);
  }, [doc]);

  /** 改了檔案之後：換上新的記號、重讀一次。衝突（別處改過）也重讀，讓畫面跟上檔案。 */
  const mutate = async (job: (token: string) => Promise<string>, done: string) => {
    if (!data || busy) return;
    setBusy(true);
    try {
      await job(data.token);
      await load();
      setToast(done);
    } catch (e) {
      setToast(errorMessage(kindOf(e)));
      void load();
    } finally {
      setBusy(false);
    }
  };

  const toggleProofread = () =>
    void mutate(
      (token) => api.digestSetProofread(paperId, materialId, !doc?.proofread, token),
      doc?.proofread ? t("digest.view.unmarked") : t("digest.view.marked"),
    );

  const askDelete = (p: DigestPoint) => {
    setPending(p);
    nav.push({ name: "sheet", kind: "digestDelete" });
  };
  const confirmDelete = async () => {
    const p = pending;
    await nav.back();
    if (!p) return;
    await mutate((token) => api.digestDeletePoint(paperId, materialId, p.id, token), t("digest.view.deleted"));
    setPending(null);
  };

  const openWith = async () => {
    try {
      const r = await api.digestOpenExternal(paperId, materialId);
      if (r === "no_app") setToast(t("digest.view.noApp"));
      else if (r !== "opened") setToast(t("digest.view.failed"));
    } catch (e) {
      setToast(errorMessage(kindOf(e)));
    }
  };

  const share = async () => {
    try {
      const f = await api.exportDigest(paperId, materialId);
      await api.deliverExport(f.path, f.name, "share");
    } catch (e) {
      setToast(errorMessage(kindOf(e)));
    }
  };

  return (
    <Motion.div className="compare digest" role="dialog" aria-label={t("digest.view.title")} style={exiting} {...view}>
      <header className="compare-bar">
        <IconButton label={t("common.back")} onClick={props.onClose}>
          <BackIcon />
        </IconButton>
        <div className="compare-title">
          <h1>{t("digest.view.title")}</h1>
          {doc
            ? (() => {
                // 匯入的重點筆記通常沒有記服務：只寫日期（沒有日期就不寫），不留一個孤零零的「・」
                const service = doc.service ?? doc.provider ?? "";
                const date = shortDate(doc.created);
                const meta = service ? t("digest.view.meta", { service, date }) : date;
                return meta ? <span className="muted small">{meta}</span> : null;
              })()
            : null}
        </div>
        <IconButton label={t("digest.view.export")} onClick={() => void share()}>
          <ShareIcon />
        </IconButton>
      </header>
      <div className="compare-body" onPointerDown={() => found && setFound(null)}>
        {state.phase === "loading" ? (
          <p className="muted" role="status">
            <Spinner /> {t("digest.view.loading")}
          </p>
        ) : null}
        {state.phase === "failed" ? <p className="field-error">{state.message}</p> : null}
        {doc ? (
          <>
            <div className={doc.proofread ? "digest-state done" : "digest-state"}>
              <span>{digestStateLabel(props.method, doc.proofread)}</span>
              {!readOnly ? (
                <Button onClick={toggleProofread} disabled={busy}>
                  {doc.proofread ? t("digest.view.markUnproofread") : t("digest.view.markProofread")}
                </Button>
              ) : null}
            </div>
            {readOnly ? <p className="muted small">{t("digest.view.newer")}</p> : null}
            {/* 區段那一行讀不懂：說一句；它也在讀不懂的清單裡，點數不算它 */}
            {doc.sections_unreadable ? <p className="muted small">{t("digest.view.sectionsUnreadable")}</p> : null}
            {doc.unreadable.length - (doc.sections_unreadable ? 1 : 0) > 0 ? (
              <p className="muted small">
                {t("digest.view.unreadable", { n: doc.unreadable.length - (doc.sections_unreadable ? 1 : 0) })}
              </p>
            ) : null}
            {groups.length === 0 ? <p>{t("digest.view.empty")}</p> : null}
            {groups.map((g) => (
              <section key={g.key} className="digest-section">
                <h2 className="digest-heading">{g.label}</h2>
                <ul className="digest-list">
                  {g.points.map((p) => (
                    <li key={p.id} className={p.id === found ? "digest-point found" : "digest-point"} data-point={p.id}>
                      <p className="digest-text">{p.text}</p>
                      <div className="digest-tools">
                        {p.pages.map((pg) => (
                          <button
                            key={pg}
                            type="button"
                            className="digest-page"
                            aria-label={t("digest.view.page", { page: pg + 1 })}
                            onClick={() => props.onJump(pg + 1)}
                          >
                            {t("digest.view.pageLabel", { page: pg + 1 })}
                          </button>
                        ))}
                        {!readOnly ? (
                          <IconButton label={t("digest.view.delete")} onClick={() => askDelete(p)}>
                            <TrashIcon />
                          </IconButton>
                        ) : null}
                      </div>
                    </li>
                  ))}
                </ul>
              </section>
            ))}
            <div className="gen-actions">
              <Button block onClick={() => void openWith()}>
                {t("digest.view.openWith")}
              </Button>
              {props.onRemove ? (
                <Button block onClick={props.onRemove}>
                  {t("digest.view.remove")}
                </Button>
              ) : null}
            </div>
          </>
        ) : null}
      </div>
      <ConfirmSheet
        open={props.sheets.includes("digestDelete") && pending !== null}
        title={t("digest.delete.title")}
        confirm={t("digest.delete.yes")}
        onConfirm={() => void confirmDelete()}
        onCancel={() => void nav.back()}
      >
        <p className="digest-quote">{pending?.text}</p>
        <p className="muted small">{t("digest.delete.body")}</p>
      </ConfirmSheet>
      <Toast toast={toast ? { message: toast } : null} onDone={() => setToast(null)} />
    </Motion.div>
  );
}
