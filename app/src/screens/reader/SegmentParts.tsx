// 一段譯文的共用部分：譯文檢視（`CompareView.tsx`）與 PDF 上點一段的小框都用。
//   - `useOverlayDoc`：讀譯文疊加檔、就地改一段（經核心的 `update_overlay_segments`，記成使用者決定的 `by_user`）；
//   - `useRebuild`、`RebuildBar`：譯文改過之後，由它產生的譯本 PDF 要不要更新、在背景重排；
//   - `SegmentEditor`、`SegmentMarks`：改一段的輸入框、一段的小標記（已改、你挑的、沒翻）；
//   - `Paragraph`：譯文（閱讀模式）的一段；
//   - `SegmentPop`：PDF 上點一段時旁邊的小框（譯本上放原文、原文上放譯文，有「改」）；
//   - `CardPop`：同一個小框，放點引用標記出的卡片（外掛交出的資料）。
// 讀檔、改段落只有這一份：譯文檢視、疊譯文、PDF 上的小框看到的是同一份內容，改了到處都是改過的。

import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { api, AppError, type Material, type OverlayFile, type OverlaySegment } from "../../api";
import { isGeneratedPdf } from "../../api/types";
import type { RegionCard } from "../../ext/types";
import { useViewportSize } from "../../hooks";
import { errorMessage, t } from "../../strings";
import { getBuild, startBuild, subscribeBuild, type BuildState } from "../../translate/autoBuild";
import { Button, Spinner } from "../../ui/components";
import { m as Motion, useMotion } from "../../ui/motion";
import { editMark, pdfBehind, shownText } from "./segments";

export interface OverlayDoc {
  doc: OverlayFile | null;
  /** 讀不出來時的白話原因。 */
  error: string | null;
  reload: () => Promise<void>;
  /** 改一段譯文。成功交回 `null`；失敗交回要說的話（呼叫的一方留著打好的字）。 */
  edit: (segmentId: string, text: string) => Promise<string | null>;
}

/** 一份譯文疊加檔（`overlayId` 沒給就是沒有）。重讀時舊的內容留著，畫面不閃、捲動位置不丟。 */
export function useOverlayDoc(paperId: string, overlayId: string | undefined): OverlayDoc {
  const [doc, setDoc] = useState<OverlayFile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const current = useRef(overlayId);
  useEffect(() => {
    current.current = overlayId;
  });

  const reload = useCallback(async () => {
    if (!overlayId) return;
    try {
      const d = await api.readOverlay(paperId, overlayId);
      if (current.current !== overlayId) return;
      setDoc(d);
      setError(null);
    } catch (e) {
      if (current.current !== overlayId) return;
      setError(errorMessage(e instanceof AppError ? e.kind : "Unreadable"));
    }
  }, [paperId, overlayId]);

  useEffect(() => {
    setDoc(null);
    setError(null);
    void reload();
  }, [reload]);

  const edit = useCallback(
    async (segmentId: string, text: string) => {
      if (!overlayId) return errorMessage("NotFound");
      if (text.trim() === "") return t("reader.text.editEmpty");
      try {
        await api.overlayApply(paperId, overlayId, segmentId, text.trim(), null);
        await reload();
        return null;
      } catch (e) {
        return errorMessage(e instanceof AppError ? e.kind : "Unknown");
      }
    },
    [paperId, overlayId, reload],
  );

  return { doc, error, reload, edit };
}

export interface Rebuild {
  /** 由這份譯文產生的譯本 PDF（沒有就是沒有，不必更新）。 */
  pdf: Material | undefined;
  /** 有改過的段落在譯本 PDF 產生之後：要更新（A5）。 */
  behind: boolean;
  /** 這份譯文的產生工作（正在排、排好了、失敗）；沒有在做是 `null`。 */
  build: Exclude<BuildState, { phase: "idle" }> | null;
  /** 在背景重排譯本 PDF（沿用產生的流程，離開畫面也會繼續）。 */
  run: () => void;
}

/** 譯文改過之後，由它產生的譯本 PDF 要不要更新、更新到哪裡了。 */
export function useRebuild(paperId: string, overlayId: string | undefined, doc: OverlayFile | null, materials: readonly Material[]): Rebuild {
  const pdf = materials.filter((m) => isGeneratedPdf(m) && m.provenance?.generated_from === overlayId).at(-1);
  const state = useSyncExternalStore(subscribeBuild, getBuild);
  const build = state.phase !== "idle" && state.paperId === paperId && state.overlayId === overlayId ? state : null;
  // 排好但內容和原本相同（改的段落含公式、保留原文）時核心不改產生的時間：以排好的那一刻為準
  const [builtAt, setBuiltAt] = useState<string | null>(null);
  const phase = build?.phase ?? "idle";
  const lastPhase = useRef(phase);
  useEffect(() => {
    if (lastPhase.current === "working" && phase === "done") setBuiltAt(new Date().toISOString());
    lastPhase.current = phase;
  }, [phase]);
  const made = [pdf?.provenance?.time, builtAt].filter((x): x is string => !!x).sort().at(-1);
  const behind = !!pdf && !!doc && build?.phase !== "working" && pdfBehind(doc.segments, made);
  const run = () => {
    if (overlayId) void startBuild(paperId, overlayId);
  };
  return { pdf, behind, build, run };
}

/** 改一段譯文：輸入框隨內容長高；「存」把整段換成打好的字。失敗時說原因、字留著。 */
export function SegmentEditor(props: { initial: string; onSave: (text: string) => Promise<string | null>; onCancel: () => void }) {
  const [text, setText] = useState(props.initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    setBusy(true);
    setError(null);
    const message = await props.onSave(text);
    setBusy(false);
    if (message) setError(message);
  };
  return (
    <div className="seg-editor" onClick={(e) => e.stopPropagation()}>
      <textarea
        className="input seg-input"
        value={text}
        autoFocus
        aria-label={t("reader.text.editLabel")}
        onChange={(e) => setText(e.target.value)}
      />
      {error ? (
        <p className="field-error" role="alert">
          {error}
        </p>
      ) : null}
      <div className="seg-editor-actions">
        <Button primary disabled={busy || text.trim() === ""} onClick={() => void save()}>
          {t("common.save")}
        </Button>
        <Button onClick={props.onCancel}>{t("common.cancel")}</Button>
      </div>
    </div>
  );
}

/** 這一段的小標記：已改、你挑的、沒翻（A5、A9）。 */
export function SegmentMarks(props: { seg: OverlaySegment }) {
  const mark = editMark(props.seg);
  const untranslated = shownText(props.seg).untranslated;
  if (!mark && !untranslated) return null;
  return (
    <span className="seg-marks">
      {untranslated ? <span className="seg-mark">{t("reader.text.untranslated")}</span> : null}
      {mark ? <span className="seg-mark">{t(mark === "edited" ? "reader.text.edited" : "reader.text.picked")}</span> : null}
    </span>
  );
}

const POP_GAP = 8;

/**
 * PDF 上點一段時，那段旁邊的小框：在譯本上放原文、在原文上放譯文（`show`），有「改」。
 * 放在那段下面，下面放不下就放上面；改的時候移到畫面上方，不被鍵盤蓋住。點框外或再點一次那段由閱讀畫面收起。
 */
type Anchor = { left: number; top: number; right: number; bottom: number };

/** 小框的位置：在點的地方下面（下面放不下就上面），左右不超出畫面；改的時候占滿鍵盤以上的空間。 */
function usePopStyle(anchor: Anchor, editing: boolean): React.CSSProperties {
  const { w, h } = useViewportSize();
  const width = Math.min(w - 24, 380);
  const left = Math.min(Math.max(12, anchor.left), w - width - 12);
  const below = h - anchor.bottom > Math.min(260, h * 0.35);
  // 改的時候占滿鍵盤以上的空間（畫面隨鍵盤縮小），輸入框與「儲存」都看得到
  return editing
    ? { left: 12, right: 12, top: "calc(var(--inset-top) + 8px)", bottom: 8 }
    : below
      ? { left, width, top: anchor.bottom + POP_GAP }
      : { left, width, bottom: h - anchor.top + POP_GAP };
}

/**
 * 點引用標記出的卡片：和點一段的小框同一套，內容是外掛交出的資料（標題、幾條文字、可以打開的那篇）。
 */
export function CardPop(props: { card: RegionCard; anchor: Anchor; onOpen: (paperId: string) => void }) {
  const style = usePopStyle(props.anchor, false);
  const motion = useMotion("pop");
  return (
    <Motion.div
      className="seg-pop"
      role="dialog"
      aria-label={props.card.title}
      style={{ ...style, maxHeight: "50vh", overflowY: "auto" }}
      onClick={(e) => e.stopPropagation()}
      {...motion}
    >
      <div className="seg-pop-head">
        <span className="muted small">{props.card.title}</span>
      </div>
      {props.card.items.map((it, i) => (
        <div key={i} className="card-item">
          <p className="seg-pop-text" lang="en">
            {it.text}
          </p>
          {it.note ? <p className="muted small">{it.note}</p> : null}
          {it.open ? (
            <div className="seg-pop-actions">
              <Button onClick={() => props.onOpen(it.open ?? "")}>{t("reader.card.open")}</Button>
            </div>
          ) : null}
        </div>
      ))}
    </Motion.div>
  );
}

export function SegmentPop(props: {
  seg: OverlaySegment;
  show: "source" | "target";
  anchor: Anchor;
  editing: boolean;
  onEdit: () => void;
  onSave: (text: string) => Promise<string | null>;
  onCancelEdit: () => void;
  /** 整段短評；原頁疊譯文上不能劃線時不給。 */
  onComment?: (() => void) | undefined;
}) {
  const { seg, anchor } = props;
  const motion = useMotion("pop");
  const style = usePopStyle(anchor, props.editing);
  const shown = shownText(seg);
  const text = props.show === "source" ? seg.source : shown.untranslated ? null : shown.text;
  return (
    <Motion.div
      className={props.editing ? "seg-pop editing" : "seg-pop"}
      role="dialog"
      aria-label={t(props.show === "source" ? "reader.pop.source" : "reader.pop.target")}
      style={style}
      onClick={(e) => e.stopPropagation()}
      {...motion}
    >
      <div className="seg-pop-head">
        <span className="muted small">{t(props.show === "source" ? "reader.pop.source" : "reader.pop.target")}</span>
        <SegmentMarks seg={seg} />
      </div>
      {props.editing ? (
        <>
          <p className="seg-pop-ref" lang="en">
            {seg.source}
          </p>
          <SegmentEditor initial={shown.untranslated ? "" : shown.text} onSave={props.onSave} onCancel={props.onCancelEdit} />
        </>
      ) : (
        <>
          <p className="seg-pop-text" lang={props.show === "source" ? "en" : undefined}>
            {text ?? t("reader.text.untranslated")}
          </p>
          <div className="seg-pop-actions">
            <Button onClick={props.onEdit}>{t("reader.text.edit")}</Button>
            {props.onComment ? <Button onClick={props.onComment}>{t("reader.highlight.paragraph")}</Button> : null}
          </div>
        </>
      )}
    </Motion.div>
  );
}

const LONG_PRESS_MS = 550; // 按住這麼久算長按（就地改）

/** 閱讀模式的一段：點一下在下面展開原文（再點收起）；長按或展開後按「改」就地改。 */
export function Paragraph(props: {
  seg: OverlaySegment;
  open: boolean;
  editing: boolean;
  onToggle: () => void;
  onEdit: () => void;
  onLayout: () => void;
  onSave: (text: string) => Promise<string | null>;
  onCancel: () => void;
}) {
  const { seg } = props;
  const shown = shownText(seg);
  const role = seg.kind === "heading" ? "heading" : seg.kind === "caption" ? "caption" : "body";
  const press = useRef<{ x: number; y: number; timer: number; fired: boolean } | null>(null);
  const endPress = () => window.clearTimeout(press.current?.timer);
  useEffect(() => () => window.clearTimeout(press.current?.timer), []);
  if (props.editing) {
    return (
      <div className={`tx-seg tx-${role} editing`} data-sid={seg.id}>
        <p className="tx-ref" lang="en">
          {seg.source}
        </p>
        <SegmentEditor initial={shown.untranslated ? "" : shown.text} onSave={props.onSave} onCancel={props.onCancel} />
      </div>
    );
  }
  return (
    <div className={`tx-seg tx-${role}${props.open ? " open" : ""}`} data-sid={seg.id}>
      <div
        className="tx-text"
        role="button"
        tabIndex={0}
        aria-expanded={props.open}
        lang={shown.untranslated ? "en" : undefined}
        onPointerDown={(e) => {
          endPress();
          press.current = {
            x: e.clientX,
            y: e.clientY,
            fired: false,
            timer: window.setTimeout(() => {
              if (press.current) press.current.fired = true;
              props.onEdit();
            }, LONG_PRESS_MS),
          };
        }}
        onPointerMove={(e) => {
          const p = press.current;
          if (p && Math.hypot(e.clientX - p.x, e.clientY - p.y) > 10) endPress();
        }}
        onPointerUp={endPress}
        onPointerCancel={endPress}
        onContextMenu={(e) => e.preventDefault()}
        onClick={() => {
          if (press.current?.fired) return;
          props.onToggle();
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            props.onToggle();
          }
        }}
      >
        {shown.text}
        <SegmentMarks seg={seg} />
      </div>
      {props.open ? (
        <div className="tx-source">
          {shown.untranslated ? null : (
            <p className="tx-ref" lang="en">
              {seg.source}
            </p>
          )}
          <div className="tx-actions">
            <Button onClick={props.onEdit}>{t("reader.text.edit")}</Button>
            <Button onClick={props.onLayout}>{t("reader.text.layout")}</Button>
          </div>
        </div>
      ) : null}
    </div>
  );
}

/** 頁頂說一次「譯文改過了，更新譯本 PDF」；按了在背景重排，這裡顯示排到哪（A5）。 */
export function RebuildBar(props: { rebuild: Rebuild; dismissed: boolean; onDismiss: () => void }) {
  const { build, behind, run } = props.rebuild;
  if (build?.phase === "working") {
    return (
      <div className="tx-banner" role="status">
        <Spinner />
        <span>
          {build.total > 0 ? t("reader.text.rebuilding", { n: build.n, total: build.total }) : t("reader.text.rebuildStart")}
        </span>
      </div>
    );
  }
  if (build?.phase === "failed") {
    return (
      <div className="tx-banner" role="alert">
        <span>{t("reader.text.rebuildFailed")}</span>
        <Button onClick={run}>{t("common.retry")}</Button>
      </div>
    );
  }
  if (!behind || props.dismissed) return null;
  return (
    <div className="tx-banner" role="status">
      <span>{t("reader.text.behind")}</span>
      <Button primary onClick={run}>
        {t("reader.text.rebuild")}
      </Button>
      <button type="button" className="tx-banner-close" aria-label={t("reader.text.later")} onClick={props.onDismiss}>
        ×
      </button>
    </div>
  );
}
