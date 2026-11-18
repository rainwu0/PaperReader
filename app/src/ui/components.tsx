import { useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { NavContext } from "../nav";
import type { ErrorKind } from "../api";
import { errorDetail, errorMessage, errorReason, t } from "../strings";
import { BackIcon, InfoIcon } from "./icons";
import { AnimatePresence, m, useExitingStyle, useMotion } from "./motion";

export function IconButton(props: {
  label: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button type="button" className="icon-btn" aria-label={props.label} onClick={props.onClick}>
      {props.children}
    </button>
  );
}

export function Button(props: {
  children: ReactNode;
  onClick: () => void;
  primary?: boolean;
  block?: boolean;
  disabled?: boolean;
}) {
  const cls = ["btn", props.primary ? "primary" : "", props.block ? "block" : ""]
    .filter(Boolean)
    .join(" ");
  return (
    <button type="button" className={cls} disabled={props.disabled} onClick={props.onClick}>
      {props.children}
    </button>
  );
}

/** 次頁的頂端列：返回鍵、標題、右側的動作。 */
export function TopBar(props: { title: string; onBack: () => void; actions?: ReactNode }) {
  return (
    <header className="topbar">
      <IconButton label={t("common.back")} onClick={props.onBack}>
        <BackIcon />
      </IconButton>
      <h1>{props.title}</h1>
      {props.actions}
    </header>
  );
}

/** 一個次頁的骨架：頂端列加可以捲動的內容。 */
export function Screen(props: {
  title: string;
  onBack: () => void;
  actions?: ReactNode;
  className?: string;
  children?: ReactNode;
}) {
  return (
    <div className={props.className ? `screen ${props.className}` : "screen"}>
      <TopBar title={props.title} onBack={props.onBack} actions={props.actions} />
      {props.children !== undefined ? <div className="scroll">{props.children}</div> : null}
    </div>
  );
}

/** 一列勾選：勾選框加說明，可以多一行淡色的補充。 */
export function CheckRow(props: {
  checked: boolean;
  onChange: (v: boolean) => void;
  children: ReactNode;
  hint?: string;
  disabled?: boolean;
}) {
  return (
    <label className="switch-row">
      <input
        type="checkbox"
        checked={props.checked}
        disabled={props.disabled}
        onChange={(e) => props.onChange(e.target.checked)}
      />
      <span>
        {props.children}
        {props.hint ? <small className="muted block-hint">{props.hint}</small> : null}
      </span>
    </label>
  );
}

/** 單選清單：一直排的 `Choice`，選一個（排序方式、書目格式）。 */
export function ChoiceList(props: { label: string; children: ReactNode }) {
  return (
    <div role="radiogroup" aria-label={props.label} className="choice-list">
      {props.children}
    </div>
  );
}

/** 單選清單的一項：選著的打勾；可以多一行淡色的說明。 */
export function Choice(props: {
  checked: boolean;
  onPick: () => void;
  label: string;
  hint?: string;
  className?: string;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={props.checked}
      className={props.className ? `choice ${props.className}` : "choice"}
      onClick={props.onPick}
    >
      <span className="choice-mark" aria-hidden="true">
        {props.checked ? "✓" : ""}
      </span>
      {props.hint ? (
        <span className="choice-text">
          {props.label}
          <span className="muted small">{props.hint}</span>
        </span>
      ) : (
        props.label
      )}
    </button>
  );
}

/** 分段選擇：一排按鈕選一個，選中的 `aria-pressed`。 */
export function Segmented<T extends string>(props: {
  label: string;
  options: ReadonlyArray<readonly [T, string]>;
  value: T;
  onChange: (v: T) => void;
  className?: string;
}) {
  return (
    <div className={props.className ? `segmented ${props.className}` : "segmented"} role="group" aria-label={props.label}>
      {props.options.map(([v, label]) => (
        <button key={v} type="button" aria-pressed={props.value === v} onClick={() => props.onChange(v)}>
          {label}
        </button>
      ))}
    </div>
  );
}

export function Spinner(props: { dark?: boolean }) {
  return <div className={props.dark ? "spinner dark" : "spinner"} role="progressbar" aria-label={t("common.working")} />;
}

/**
 * 進行中的提示：動作超過 1 秒才出現，短的動作不閃一下。
 * `active` 為真並持續超過 `delayMs`，就蓋上一層提示。
 */
export function BusyOverlay(props: { active: boolean; label: string; delayMs?: number }) {
  const [elapsed, setElapsed] = useState(false);
  const delay = props.delayMs ?? 1000;
  useEffect(() => {
    if (!props.active) return;
    const id = window.setTimeout(() => setElapsed(true), delay);
    return () => {
      window.clearTimeout(id);
      setElapsed(false); // 動作結束（或換了一個）：下一次重新計時
    };
  }, [props.active, delay]);
  const fade = useMotion("fade");
  // 文字與轉圈放進有底色的卡片：不管底下是什麼，對比都夠
  return (
    <AnimatePresence>
      {props.active && elapsed ? (
        <m.div key="busy" className="busy" role="alert" aria-live="assertive" {...fade}>
          <div className="busy-card">
            <Spinner dark />
            <span>{props.label}</span>
          </div>
        </m.div>
      ) : null}
    </AnimatePresence>
  );
}

interface SheetProps {
  title: string;
  children: ReactNode;
  onDismiss?: () => void;
  center?: boolean;
}

/**
 * 面板或對話框。點背景關閉（若有 `onDismiss`）。`open` 由真變假時播完退場才卸載：
 * 底部面板從下緣滑入滑出、置中對話框淡入淡出。
 * 擁有它的畫面一直渲染它、用 `open` 控制；整個元件由條件決定掛不掛時，外面包一層 `AnimatePresence`。
 */
export function Sheet(props: SheetProps & { open: boolean }) {
  const { open, ...rest } = props;
  return <AnimatePresence propagate>{open ? <SheetBody key="sheet" {...rest} /> : null}</AnimatePresence>;
}

function SheetBody(props: SheetProps) {
  const scrim = useMotion("scrim");
  const panel = useMotion(props.center ? "dialog" : "sheet");
  const exiting = useExitingStyle();
  return (
    <m.div
      className={props.center ? "scrim center" : "scrim"}
      style={exiting}
      {...scrim}
      onClick={(e) => {
        if (e.target === e.currentTarget) props.onDismiss?.();
      }}
    >
      <m.div
        className={props.center ? "sheet center" : "sheet"}
        role="dialog"
        aria-modal="true"
        aria-label={props.title}
        {...panel}
      >
        <h2>{props.title}</h2>
        {props.children}
      </m.div>
    </m.div>
  );
}

/**
 * 確認（置中）：說明、主要的動作、取消。點背景或返回鍵等於取消。
 * 由條件決定掛不掛時（外面包 `AnimatePresence`），`open` 給 `true`。
 */
export function ConfirmSheet(props: {
  open: boolean;
  title: string;
  children: ReactNode;
  confirm: string;
  onConfirm: () => void;
  onCancel: () => void;
  cancel?: string;
  disabled?: boolean;
}) {
  return (
    <Sheet open={props.open} title={props.title} center onDismiss={props.onCancel}>
      {props.children}
      <div className="actions">
        <Button primary block disabled={props.disabled} onClick={props.onConfirm}>
          {props.confirm}
        </Button>
        <Button block onClick={props.onCancel}>
          {props.cancel ?? t("common.cancel")}
        </Button>
      </div>
    </Sheet>
  );
}

/**
 * 結果或訊息（置中）：內容與「確定」。可以多一個動作，放在「確定」前面；`primary` 為假時它是次要的。
 * `onDismiss`（點背景、返回鍵）沒給就同「確定」。
 */
export function ResultSheet(props: {
  open: boolean;
  title: string;
  children: ReactNode;
  onDone: () => void;
  onDismiss?: () => void;
  action?: { label: string; run: () => void; primary?: boolean };
}) {
  const actionPrimary = !!props.action && props.action.primary !== false;
  return (
    <Sheet open={props.open} title={props.title} center onDismiss={props.onDismiss ?? props.onDone}>
      {props.children}
      <div className="actions">
        {props.action ? (
          <Button primary={actionPrimary} block onClick={props.action.run}>
            {props.action.label}
          </Button>
        ) : null}
        <Button primary={!actionPrimary} block onClick={props.onDone}>
          {t("common.confirm")}
        </Button>
      </div>
    </Sheet>
  );
}

export interface ToastData {
  message: string;
  /** 例如「復原」：按下去做 `run`，提示就收掉。 */
  action?: { label: string; run: () => void };
}

/** 一則短暫的訊息（有動作的 8 秒、沒有的 4 秒後消失）。淡入加上移，淡出。 */
export function Toast(props: { toast: ToastData | null; onDone: () => void }) {
  const done = useRef(props.onDone);
  useEffect(() => {
    done.current = props.onDone;
  });
  const toast = props.toast;
  useEffect(() => {
    if (!toast) return;
    const id = window.setTimeout(() => done.current(), toast.action ? 8000 : 4000);
    return () => window.clearTimeout(id);
  }, [toast]);
  const motion = useMotion("toast");
  return (
    <AnimatePresence>
      {toast ? (
        <m.div key={toast.message} className="toast" role="status" aria-live="polite" {...motion}>
          <span className="toast-text">{toast.message}</span>
          {toast.action ? (
            <button
              type="button"
              className="toast-action"
              onClick={() => {
                const run = toast.action?.run;
                done.current();
                run?.();
              }}
            >
              {toast.action.label}
            </button>
          ) : null}
        </m.div>
      ) : null}
    </AnimatePresence>
  );
}

/** 出錯時的一塊提示：一句結論、可以做的事（按鈕），原因放「詳情」。 */
export function ErrorNotice(props: { message: string; detail?: string; onAction?: () => void; actionLabel?: string }) {
  return (
    <div className="notice" role="alert">
      <div>{props.message}</div>
      {props.detail ? <Details>{props.detail}</Details> : null}
      {props.onAction ? (
        <Button onClick={props.onAction}>{props.actionLabel ?? t("common.retry")}</Button>
      ) : null}
    </div>
  );
}

/**
 * 依錯誤種類的提示。有 `lead`（例如「沒辦法讀取閱讀紀錄。」）時它是表面那一句，
 * 錯誤本身的說法與細節都放「詳情」；沒有時表面是錯誤的說法、細節放「詳情」。
 */
export function KindNotice(props: { kind: ErrorKind; lead?: string; onAction?: () => void; actionLabel?: string }) {
  const detail = errorDetail(props.kind);
  const message = props.lead ?? errorMessage(props.kind);
  const more = props.lead ? errorReason(props.kind) : detail;
  return <ErrorNotice message={message} detail={more} onAction={props.onAction} actionLabel={props.actionLabel} />;
}

/**
 * 「詳情」展開區：結論下方，放原因、技術細節、統計。只有一層。
 * 箭頭轉向、內容淡入；不補間高度。
 */
export function Details(props: { children: ReactNode; label?: string }) {
  const [open, setOpen] = useState(false);
  const fade = useMotion("fade");
  return (
    <div className="details">
      <button type="button" className="details-toggle" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        <span className={open ? "details-arrow open" : "details-arrow"} aria-hidden="true">
          ›
        </span>
        {props.label ?? t("common.details")}
      </button>
      {open ? (
        <m.div className="details-body" {...fade}>
          {props.children}
        </m.div>
      ) : null}
    </div>
  );
}

/**
 * ⓘ 說明框：點了才開，點別處或按返回關閉；不靠長按或懸停。
 * 打開時在導覽堆疊推一層，系統返回鍵先關掉它。
 */
export function InfoTip(props: { label: string; children: ReactNode }) {
  const nav = useContext(NavContext);
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ left: number; top: number }>({ left: 16, top: 0 });
  const btn = useRef<HTMLButtonElement>(null);
  const depth = useRef(0);
  const fade = useMotion("dialog");
  // 返回鍵把這一層退掉了：跟著關
  const stackLen = nav?.stack.length ?? 0;
  useEffect(() => {
    if (open && stackLen < depth.current) setOpen(false);
  }, [open, stackLen]);
  const show = () => {
    // 說明框放在圖示下方，左右不超出畫面（寬度與 `.tip-bubble` 一致）
    const r = btn.current?.getBoundingClientRect();
    if (r) {
      const width = Math.min(300, window.innerWidth - 32);
      const left = Math.max(16, Math.min(r.left + r.width / 2 - width / 2, window.innerWidth - 16 - width));
      setPos({ left, top: r.bottom + 4 });
    }
    if (!nav) return setOpen(true);
    nav.push({ name: "sheet", kind: "tip" });
    depth.current = nav.stack.length + 1;
    setOpen(true);
  };
  const hide = () => {
    setOpen(false);
    if (nav && nav.top.name === "sheet" && nav.top.kind === "tip") void nav.back();
  };
  return (
    <span className="tip">
      <button
        ref={btn}
        type="button"
        className="tip-btn"
        aria-label={props.label}
        aria-expanded={open}
        onClick={(e) => {
          // 放在標籤裡時不要連帶切換勾選
          e.preventDefault();
          e.stopPropagation();
          if (open) hide();
          else show();
        }}
      >
        <InfoIcon />
      </button>
      <AnimatePresence>
        {open ? (
          <>
            <span key="catch" className="tip-catcher" onClick={hide} />
            <m.span key="bubble" className="tip-bubble" role="note" style={pos} onClick={hide} {...fade}>
              {props.children}
            </m.span>
          </>
        ) : null}
      </AnimatePresence>
    </span>
  );
}

