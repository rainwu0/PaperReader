import type { ReactNode } from "react";
import { Segmented } from "../../ui/components";

/** 設定的一個分區：標題加一張圓角卡片。 */
export function Section(props: { title: string; children: ReactNode; className?: string }) {
  return (
    <section className={`set-section${props.className ? ` ${props.className}` : ""}`}>
      <h2 className="set-heading">{props.title}</h2>
      <div className="set-card">{props.children}</div>
    </section>
  );
}

/** 卡片裡的一列：左邊圖示、中間標題與淡色說明、右邊的值或 ›。有 `onClick` 時整列可以按。 */
export function SettingRow(props: {
  icon?: ReactNode;
  title: string;
  hint?: string;
  value?: string;
  chevron?: boolean;
  onClick?: () => void;
  /** 右邊放的控制項（開關）。 */
  control?: ReactNode;
}) {
  const body = (
    <>
      {props.icon ? <span className="set-icon">{props.icon}</span> : null}
      <span className="set-text">
        <span className="set-title">{props.title}</span>
        {props.hint ? <span className="set-hint">{props.hint}</span> : null}
      </span>
      {props.value ? <span className="set-value">{props.value}</span> : null}
      {props.chevron ? (
        <span className="set-chev" aria-hidden="true">
          ›
        </span>
      ) : null}
      {props.control}
    </>
  );
  if (props.onClick) {
    return (
      <button type="button" className="set-row set-row-btn" onClick={props.onClick}>
        {body}
      </button>
    );
  }
  return <div className="set-row">{body}</div>;
}

/** 開關列：整列可以按；開關用 Android 慣用的樣式（滑軌加圓鈕）。 */
export function SwitchRow(props: {
  icon?: ReactNode;
  title: string;
  hint?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="set-row">
      {props.icon ? <span className="set-icon">{props.icon}</span> : null}
      <span className="set-text">
        <span className="set-title">{props.title}</span>
        {props.hint ? <span className="set-hint">{props.hint}</span> : null}
      </span>
      <input
        type="checkbox"
        role="switch"
        className="set-switch"
        checked={props.checked}
        onChange={(e) => props.onChange(e.target.checked)}
      />
    </label>
  );
}

/** 一列放一組分段選擇（標題在上、選項在下）。 */
export function SegmentRow<T extends string>(props: {
  icon?: ReactNode;
  title: string;
  options: Array<[T, string]>;
  value: T;
  onChange: (v: T) => void;
}) {
  return (
    <div className="set-row set-row-col">
      <div className="set-row-head">
        {props.icon ? <span className="set-icon">{props.icon}</span> : null}
        <span className="set-title">{props.title}</span>
      </div>
      <Segmented label={props.title} options={props.options} value={props.value} onChange={props.onChange} />
    </div>
  );
}
