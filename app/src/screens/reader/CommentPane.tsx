// 短評窗格：點有短評的劃線時，畫面下方打開的窗格（不蓋住背後的頁面，頁面仍可以捲、可以點）。
//
// 窗格在畫面層，不在頁面元素裡，所以頁面的雙指縮放與雙擊放大不會動到它的位置與大小。
// 高度照內容、最多畫面高的 40%，超過就在窗格裡捲；上緣的把手往上拉可以拉高到 90%，往下滑關掉。
// 內容：頁碼與引句（點了回到劃線）、短評全文、「編輯」「刪除」、「上一則」「下一則」、「關閉」。

import { useEffect, useRef, useState } from "react";
import type { Highlight } from "../../api";
import { t } from "../../strings";
import { m as Motion, useMotion } from "../../ui/motion";
import { quoteHead } from "./commentLayout";
import { pageText } from "./NotesPane";

/** 往下滑超過這麼多（像素）算要關掉。 */
const CLOSE_DRAG = 60;
/** 拉高最多到畫面高的幾分之幾。 */
const MAX_FRACTION = 0.9;
/** 高度穩定這麼久（毫秒）才通知「窗格上緣在這裡」，拖動途中不重複捲頁面。 */
const SETTLE_MS = 220;

export function CommentPane(props: {
  highlight: Highlight;
  /** 這一則是第幾則、共幾則（照頁序）。 */
  index: number;
  total: number;
  onPrev: () => void;
  onNext: () => void;
  onClose: () => void;
  onEdit: () => void;
  onDelete: () => void;
  /** 點頁碼與引句：把劃線捲到窗格上方。 */
  onJump: () => void;
  /** 窗格的大小或內容穩定了：交回窗格上緣的位置（畫面座標），頁面把劃線捲到它的上方。 */
  onSettled: (top: number) => void;
}) {
  const { highlight: h } = props;
  const ref = useRef<HTMLElement>(null);
  const slide = useMotion("paneUp");
  /** 拉高之後的高度（像素）；`null` 是照內容。 */
  const [tall, setTall] = useState<number | null>(null);
  const drag = useRef<{ id: number; y: number; from: number } | null>(null);
  const settle = useRef(0);
  const notify = useRef(props.onSettled);
  useEffect(() => {
    notify.current = props.onSettled;
  });

  // 內容或大小變了，等穩定了再通知一次（開啟、換一則、拉高、字很長）
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fire = () => {
      window.clearTimeout(settle.current);
      settle.current = window.setTimeout(() => notify.current(el.getBoundingClientRect().top), SETTLE_MS);
    };
    fire();
    const ro = new ResizeObserver(fire);
    ro.observe(el);
    return () => {
      window.clearTimeout(settle.current);
      ro.disconnect();
    };
    // 換一則（劃線的代號變了）時再通知一次
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [h.id]);

  const onDown = (e: React.PointerEvent) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { id: e.pointerId, y: e.clientY, from: ref.current?.getBoundingClientRect().height ?? 0 };
  };
  const onMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dy = d.y - e.clientY;
    // 往上拉：變高；往下拉：變矮（還沒有拉高過時，往下滑超過門檻放開才關掉）
    if (dy > 0 || tall !== null) setTall(Math.min(window.innerHeight * MAX_FRACTION, Math.max(0, d.from + dy)));
  };
  const onUp = (e: React.PointerEvent) => {
    const d = drag.current;
    drag.current = null;
    if (!d) return;
    const dy = e.clientY - d.y;
    if (dy > CLOSE_DRAG) props.onClose();
    else if (tall !== null && tall < 120) setTall(null);
  };

  const style = tall === null ? undefined : { height: tall, maxHeight: `${MAX_FRACTION * 100}vh` };
  return (
    <Motion.section ref={ref} className="cm-pane" style={style} aria-label={t("reader.cpane.title")} data-testid="comment-pane" {...slide}>
      <div
        className="cmp-handle"
        role="separator"
        aria-orientation="horizontal"
        aria-label={t("reader.cpane.resize")}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={() => (drag.current = null)}
      >
        <span />
      </div>
      <div className="cmp-body">
        <button type="button" className="cmp-quote" aria-label={t("reader.cpane.toHighlight", { page: pageText(h) })} onClick={props.onJump}>
          <span className="cmp-page">{pageText(h)}</span>
          <span className="cmp-quote-text">「{quoteHead(h.quote.exact, 40)}」</span>
        </button>
        <p className="cmp-text" data-testid="comment-pane-text">
          {h.comment}
        </p>
      </div>
      <div className="cmp-actions">
        <button type="button" onClick={props.onEdit}>
          {t("reader.comments.edit")}
        </button>
        <button type="button" onClick={props.onDelete}>
          {t("reader.comments.delete")}
        </button>
        <span className="cmp-spacer" />
        <button type="button" disabled={props.index <= 0} onClick={props.onPrev}>
          {t("reader.cpane.prev")}
        </button>
        <span className="cmp-pos" aria-label={t("reader.cpane.position", { n: props.index + 1, total: props.total })}>
          {props.index + 1}／{props.total}
        </span>
        <button type="button" disabled={props.index >= props.total - 1} onClick={props.onNext}>
          {t("reader.cpane.next")}
        </button>
        <button type="button" onClick={props.onClose}>
          {t("reader.cpane.close")}
        </button>
      </div>
    </Motion.section>
  );
}
