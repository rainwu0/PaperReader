// 閱讀畫面的面板與浮在頁面上的小元件：檢視切換、跳頁、大綱、一條劃線的短評、筆刷列、首次提示（清除筆跡的確認用共用的 `ConfirmSheet`）。
// 狀態由閱讀畫面保管，這裡只畫、把使用者的選擇交回去。

import { useEffect, useState } from "react";
import type { Highlight } from "../../api";
import type { HintId } from "../../hints";
import { t } from "../../strings";
import { Button, Sheet, Spinner } from "../../ui/components";
import { m as Motion, useMotion } from "../../ui/motion";
import { pageText } from "./NotesPane";
import { DEFAULT_PEN_COLOR } from "./pdfInk";
import type { BrushStyle, OutlineEntry } from "./pdfView";
import { ChevronDownIcon, ChevronUpIcon, EraserIcon, TrashIcon, UndoIcon } from "./toolIcons";

// ---------- 筆刷列 ----------

const BRUSHES: Array<{ id: string; label: "reader.brush.red" | "reader.brush.blue" | "reader.brush.yellow"; style: BrushStyle }> = [
  { id: "red", label: "reader.brush.red", style: { color: DEFAULT_PEN_COLOR, width: 1.8, alpha: 1 } },
  { id: "blue", label: "reader.brush.blue", style: { color: "#1a56db", width: 1.8, alpha: 1 } },
  { id: "yellow", label: "reader.brush.yellow", style: { color: "#f2c300", width: 1.8, alpha: 0.45 } },
];
const WIDTHS = [
  { id: "thin", label: "reader.brush.thin", factor: 1, bar: 3 },
  { id: "thick", label: "reader.brush.thick", factor: 2.4, bar: 8 },
] as const;

/** 選的顏色與粗細（`BRUSHES`、`WIDTHS` 的序號）換成畫筆的樣子；螢光筆再粗一倍。 */
export function brushStyleOf(color: number, width: number): BrushStyle {
  const b = BRUSHES[color] ?? BRUSHES[0];
  const w = WIDTHS[width] ?? WIDTHS[0];
  const base = b?.style ?? { color: DEFAULT_PEN_COLOR, width: 1.8, alpha: 1 };
  return { ...base, width: base.width * w.factor * (b?.id === "yellow" ? 2 : 1) };
}

export interface BrushPick {
  color: number;
  width: number;
  eraser: boolean;
}

export function BrushBar(props: {
  pick: BrushPick;
  onPick: (p: BrushPick) => void;
  aboveDock: boolean;
  onUndo: () => void;
  onClear: () => void;
  onFlip: (delta: number) => void;
}) {
  const motion = useMotion("barUp");
  const { pick, onPick } = props;
  return (
    <Motion.div
      key="brush"
      className={props.aboveDock ? "brush-bar above-dock" : "brush-bar"}
      role="toolbar"
      aria-label={t("reader.tool.brush")}
      {...motion}
    >
      {BRUSHES.map((b, i) => (
        <button
          key={b.id}
          type="button"
          className="brush-swatch"
          aria-label={t(b.label)}
          aria-pressed={pick.color === i}
          onClick={() => onPick({ ...pick, color: i })}
        >
          <span style={{ background: b.style.color }} />
        </button>
      ))}
      {WIDTHS.map((wd, i) => (
        <button
          key={wd.id}
          type="button"
          className="brush-width"
          aria-label={t(wd.label)}
          aria-pressed={pick.width === i}
          onClick={() => onPick({ ...pick, width: i })}
        >
          <span style={{ height: wd.bar }} />
        </button>
      ))}
      <button
        type="button"
        aria-label={t("reader.brush.eraser")}
        aria-pressed={pick.eraser}
        onClick={() => onPick({ ...pick, eraser: !pick.eraser })}
      >
        <EraserIcon />
      </button>
      <button type="button" aria-label={t("reader.brush.undo")} onClick={props.onUndo}>
        <UndoIcon />
      </button>
      <button type="button" aria-label={t("reader.brush.clear")} onClick={props.onClear}>
        <TrashIcon />
      </button>
      <button type="button" aria-label={t("reader.brush.prev")} onClick={() => props.onFlip(-1)}>
        <ChevronUpIcon />
      </button>
      <button type="button" aria-label={t("reader.brush.next")} onClick={() => props.onFlip(1)}>
        <ChevronDownIcon />
      </button>
    </Motion.div>
  );
}

// ---------- 面板 ----------

export interface ViewOption {
  id: string;
  label: string;
  current: boolean;
  run: () => void;
  /** 可以從這裡移除的那一份（譯本 PDF、逐段譯文）：材料的代號。 */
  removable?: string;
  /** 「移除」讀屏念的名稱（和這一列的名稱不同時，例如疊譯文那一列移除的是逐段譯文）。 */
  removeName?: string;
}

/**
 * 檢視切換：工具箱「譯本」長按或旁邊的小箭頭打開（平常按一下直接切到上次那份）。
 * 譯本每一份旁邊有「移除」（移到回收區，提示給「復原」）。
 */
export function ViewsSheet(props: {
  open: boolean;
  options: ViewOption[];
  onDismiss: () => void;
  onRemove: (materialId: string) => void;
}) {
  return (
    <Sheet open={props.open} title={t("reader.view.title")} onDismiss={props.onDismiss}>
      <ul className="outline-list">
        {props.options.map((o) => (
          <li key={o.id} className={o.removable ? "view-row" : undefined}>
            <button type="button" className="outline-item" aria-current={o.current} onClick={o.run}>
              {o.label}
            </button>
            {o.removable ? (
              <button
                type="button"
                className="view-remove"
                aria-label={t("reader.view.removeLabel", { name: o.removeName ?? o.label })}
                onClick={() => props.onRemove(o.removable ?? "")}
              >
                {t("reader.view.remove")}
              </button>
            ) : null}
          </li>
        ))}
      </ul>
    </Sheet>
  );
}

/** 跳到第幾頁。 */
export function JumpSheet(props: { open: boolean; total: number; onGo: (page: number) => void; onCancel: () => void }) {
  const { total } = props;
  const [text, setText] = useState("");
  const [error, setError] = useState(false);
  useEffect(() => {
    if (!props.open) return;
    setText("");
    setError(false);
  }, [props.open]);
  const submit = () => {
    const n = Number(text.trim());
    if (text.trim() === "" || !Number.isInteger(n) || n < 1 || n > total) {
      setError(true);
      return;
    }
    props.onGo(n);
  };
  return (
    <Sheet open={props.open} title={t("reader.jump.title")} center onDismiss={props.onCancel}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <label className="field">
          <span>{t("reader.jump.label")}</span>
          <input
            className="input"
            inputMode="numeric"
            autoFocus
            value={text}
            placeholder={t("reader.jump.range", { total })}
            aria-invalid={error}
            onChange={(e) => {
              setText(e.target.value);
              setError(false);
            }}
          />
        </label>
        <p className={error ? "field-error" : "muted small"} role={error ? "alert" : undefined}>
          {error ? t("reader.jump.invalid", { total }) : t("reader.jump.range", { total })}
        </p>
        <div className="actions">
          <button type="submit" className="btn primary block">
            {t("reader.jump.go")}
          </button>
          <Button block onClick={props.onCancel}>
            {t("common.cancel")}
          </Button>
        </div>
      </form>
    </Sheet>
  );
}

export type OutlineState = { phase: "loading" } | { phase: "ready"; items: OutlineEntry[] } | { phase: "failed" };

/** PDF 自帶的大綱；沒有大綱時改用跳頁。`onPick` 解不出頁碼時交回 `false`，面板說一聲。 */
export function OutlineSheet(props: {
  open: boolean;
  outline: OutlineState;
  onPick: (entry: OutlineEntry) => Promise<boolean>;
  onJumpInstead: () => void;
  onDismiss: () => void;
}) {
  const { outline } = props;
  const [cantJump, setCantJump] = useState(false);
  useEffect(() => {
    if (props.open) setCantJump(false);
  }, [props.open]);
  return (
    <Sheet open={props.open} title={t("reader.tool.outline")} onDismiss={props.onDismiss}>
      {outline.phase === "loading" ? (
        <p className="muted" role="status">
          {t("reader.outline.loading")}
        </p>
      ) : null}
      {outline.phase === "failed" ? <p>{t("reader.outline.failed")}</p> : null}
      {outline.phase === "ready" && outline.items.length === 0 ? (
        <>
          <p>{t("reader.outline.none")}</p>
          <div className="actions">
            <Button block onClick={props.onJumpInstead}>
              {t("reader.tool.jump")}
            </Button>
          </div>
        </>
      ) : null}
      {cantJump ? (
        <p className="field-error" role="alert">
          {t("reader.outline.cantJump")}
        </p>
      ) : null}
      {outline.phase === "ready" && outline.items.length > 0 ? (
        <ul className="outline-list">
          {outline.items.map((item, i) => (
            <li key={`${i}-${item.title}`}>
              <button
                type="button"
                className="outline-item"
                style={{ paddingLeft: 12 + Math.min(item.level, 5) * 16 }}
                onClick={() => void props.onPick(item).then((ok) => setCantJump(!ok))}
              >
                {item.title}
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </Sheet>
  );
}

/**
 * 點一條劃線出的小選單（同類閱讀器的慣例：點劃線直接出「刪除」）：刪除（可以從提示復原一次）、短評。
 * 放在劃線的下面，下面放不下就放上面；返回鍵、點別處、捲動都收起（導覽堆疊的一層 `hlPop`）。
 */
export function HighlightMenu(props: {
  highlight: Highlight;
  anchor: { left: number; top: number; right: number; bottom: number };
  onDelete: () => void;
  onComment: () => void;
}) {
  const motion = useMotion("pop");
  const h = props.highlight;
  const { anchor } = props;
  const below = window.innerHeight - anchor.bottom > 120;
  const style: React.CSSProperties = below ? { top: anchor.bottom + 8 } : { top: Math.max(8, anchor.top - 64) };
  return (
    <Motion.div className="sel-pill hl-menu" style={style} role="toolbar" aria-label={t("reader.highlight.menu")} {...motion}>
      {h.complete ? (
        <button type="button" onClick={props.onDelete}>
          <TrashIcon />
          {t("reader.highlight.delete")}
        </button>
      ) : null}
      <button type="button" onClick={props.onComment}>
        {t("reader.highlight.comment")}
      </button>
    </Motion.div>
  );
}

/** 一條劃線的短評：看引句、寫或改短評、刪除這條劃線（刪了可以從提示復原一次）。 */
export function HighlightEdit(props: {
  highlight: Highlight;
  onSave: (text: string) => Promise<void>;
  onDelete: () => Promise<void>;
  onCancel: () => void;
}) {
  const h = props.highlight;
  const [text, setText] = useState(h.comment);
  return (
    <Sheet open title={t("reader.edit.title")} onDismiss={props.onCancel}>
      <p className="muted small">{pageText(h)}</p>
      <blockquote className="edit-quote">{h.quote.exact}</blockquote>
      {h.complete ? (
        <label className="field">
          <span>{t("reader.tool.comment")}</span>
          <textarea
            className="input"
            rows={4}
            value={text}
            autoFocus
            placeholder={t("reader.edit.hint")}
            onChange={(e) => setText(e.target.value)}
          />
        </label>
      ) : (
        <p className="notice">{t("reader.notes.incomplete")}</p>
      )}
      <div className="actions">
        {h.complete ? (
          <Button primary block onClick={() => void props.onSave(text)}>
            {t("common.save")}
          </Button>
        ) : null}
        {h.complete ? (
          <Button block onClick={() => void props.onDelete()}>
            {t("reader.edit.delete")}
          </Button>
        ) : null}
        <Button block onClick={props.onCancel}>
          {t("common.cancel")}
        </Button>
      </div>
    </Sheet>
  );
}

// ---------- 浮在頁面上的 ----------

/** 首次提示的卡片：說一次不明顯的手勢，按「知道了」就不再出現。 */
/** 首次提示。`text` 是外掛自己的文字（可點區域的卡片），沒有就用應用的那一句。 */
export function HintCard(props: { id: HintId; text?: string | undefined; onClose: () => void }) {
  const motion = useMotion("toast");
  return (
    <Motion.div className="reader-note reader-hint" role="status" {...motion}>
      <span>{props.text ?? t(`hint.${props.id}`)}</span>
      <button type="button" className="reader-hint-ok" onClick={props.onClose}>
        {t("hint.ok")}
      </button>
    </Motion.div>
  );
}

export function LoadingHint() {
  return (
    <div className="center-fill" style={{ position: "absolute", inset: 0, pointerEvents: "none" }}>
      <Spinner dark />
    </div>
  );
}
