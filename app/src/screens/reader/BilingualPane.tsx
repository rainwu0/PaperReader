// 中英對照的譯本那一格：上面（橫拿時左邊）是平常的閱讀畫面（原文），這一格放上次選的那一份譯本——
// 譯本 PDF，或原文加上譯文疊加（原頁疊譯文）。兩份的頁面幾何相同，所以照頁與位置同步（換算在 `viewSync.ts`）。
//
// 同步：由正在被手指操作的那一格帶，另一格只跟。手指（或滑鼠滾輪）落在哪一格，那一格就是帶頭的；
// 跟著的那一格被設了位置之後自己也會回報「動了」，那是回聲，不往回帶。原文那一格不經手指也會動（跳頁、大綱、
// 回到上次的位置），這時它回報的位置不是剛才被設的那裡：改由它帶。
//
// 這一格是另一個 `PdfView`（自己的捲動區），和原文那一格各自只畫看得到的頁。為什麼不放進閱讀畫面：
// 閱讀畫面的劃線、筆刷、短評、選字都綁著原文那一份檢視，這一格只要看與縮放，另成一個元件比較不會牽動它們。

import { useEffect, useRef, useState, type MutableRefObject, type RefObject } from "react";
import { api } from "../../api";
import { t } from "../../strings";
import { PdfView, type SegmentBox } from "./pdfView";
import { shownText } from "./segments";
import { isEcho, splitAt, type FollowMark } from "./viewSync";

const DOUBLE_TAP_MS = 300; // 兩下點在這麼短的時間內、這麼近的地方算雙擊（Android 的雙擊時限也是 300ms）
const DOUBLE_TAP_SLOP = 40; // 像素

/**
 * 點頁面：單擊要先等一下，確定不是雙擊；雙擊交給 `double`（放大到那一欄的寬度，再雙擊縮回）。
 * 原文那一格與譯本那一格共用。交回的函式接在捲動區的 `onClick`。
 */
export function useTapOrDouble(
  single: (x: number, y: number) => void,
  double: (x: number, y: number) => void,
): (e: React.MouseEvent) => void {
  const last = useRef<{ t: number; x: number; y: number; timer: number } | null>(null);
  const handlers = useRef({ single, double });
  useEffect(() => {
    handlers.current = { single, double };
  });
  useEffect(() => () => window.clearTimeout(last.current?.timer), []);
  return (e) => {
    const { clientX: x, clientY: y } = e;
    const prev = last.current;
    if (prev && e.timeStamp - prev.t < DOUBLE_TAP_MS && Math.hypot(x - prev.x, y - prev.y) < DOUBLE_TAP_SLOP) {
      window.clearTimeout(prev.timer);
      last.current = null;
      handlers.current.double(x, y);
      return;
    }
    window.clearTimeout(prev?.timer);
    const timer = window.setTimeout(() => {
      last.current = null;
      handlers.current.single(x, y);
    }, DOUBLE_TAP_MS);
    last.current = { t: e.timeStamp, x, y, timer };
  };
}

export function BilingualPane(props: {
  paperId: string;
  /** 這一格開哪一份材料：譯本 PDF，或原頁疊譯文時是原文。 */
  materialId: string;
  /** 原頁疊譯文：在原文上畫譯文。 */
  overlay: boolean;
  /** 譯文疊加檔的段落（原頁疊譯文要畫的）。 */
  segments: SegmentBox[] | null;
  /** 原文那一格（閱讀畫面的檢視與捲動區）。 */
  mainRef: RefObject<PdfView | null>;
  mainScrollRef: RefObject<HTMLDivElement | null>;
  /** 原文那一格開好了（換了材料、重新開都會再變一次）。 */
  mainReady: boolean;
  /** 原文那一格捲動或縮放時，閱讀畫面呼叫這裡放的函式。 */
  mainMoved: MutableRefObject<() => void>;
  /** 左右分（橫拿、視窗寬）；否則上下分。 */
  landscape: boolean;
  onSplit: (fraction: number) => void;
  /** 點內容帶出功能（點疊譯文暫看原文）開著沒有。 */
  tapContent: boolean;
  /** 單擊沒有別的事做：顯示或隱藏頂列。 */
  onTap: () => void;
  onMessage: (text: string) => void;
  /** 譯本打不開：回到只看原文。 */
  onFailed: () => void;
}) {
  const { paperId, materialId, mainRef } = props;
  const scrollRef = useRef<HTMLDivElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<PdfView | null>(null);
  const [ready, setReady] = useState(false);
  // 哪一格帶頭；跟著的那一格最後被設到的位置（認回聲用）
  const leader = useRef<"main" | "pane">("main");
  const mark = useRef<{ main: FollowMark | null; pane: FollowMark | null }>({ main: null, pane: null });

  const syncPane = () => {
    const main = mainRef.current;
    const pane = viewRef.current;
    if (main && pane && main.numPages > 0) mark.current.pane = pane.follow(main.syncPosition());
  };

  // 原文那一格動了：它帶頭時這一格跟上；這一格帶頭時，位置還是剛才被設的那裡就是回聲，不然是它自己動了，改由它帶
  const mainMoved = props.mainMoved;
  useEffect(() => {
    mainMoved.current = () => {
      const main = mainRef.current;
      if (!main || !viewRef.current) return;
      if (leader.current !== "main") {
        if (isEcho(mark.current.main, main.followMark())) return;
        leader.current = "main";
      }
      syncPane();
    };
    return () => {
      mainMoved.current = () => {};
    };
  });

  // 手指（或滾輪）落在哪一格，那一格帶頭
  useEffect(() => {
    const main = props.mainScrollRef.current;
    const pane = scrollRef.current;
    if (!main || !pane) return;
    const toMain = () => (leader.current = "main");
    const toPane = () => (leader.current = "pane");
    const opts = { capture: true, passive: true } as const;
    for (const ev of ["pointerdown", "touchstart", "wheel"] as const) {
      main.addEventListener(ev, toMain, opts);
      pane.addEventListener(ev, toPane, opts);
    }
    return () => {
      for (const ev of ["pointerdown", "touchstart", "wheel"] as const) {
        main.removeEventListener(ev, toMain, opts);
        pane.removeEventListener(ev, toPane, opts);
      }
    };
  }, [props.mainScrollRef]);

  // 打開這一份；開好就到原文那一格的同一處
  const onMessage = useRef(props.onMessage);
  const onFailed = useRef(props.onFailed);
  useEffect(() => {
    onMessage.current = props.onMessage;
    onFailed.current = props.onFailed;
  });
  useEffect(() => {
    const scroll = scrollRef.current;
    const wrap = wrapRef.current;
    if (!scroll || !wrap) return;
    let cancelled = false;
    setReady(false);
    const view = new PdfView({
      scroll,
      wrap,
      onPage: () => {},
      onMove: () => {
        // 這一格帶頭時原文那一格跟上；不是它帶頭時是回聲（或它自己重新排版），不往回帶
        if (leader.current !== "pane") return;
        const main = mainRef.current;
        if (main && main.numPages > 0) mark.current.main = main.follow(view.syncPosition());
      },
      onReadError: () => onMessage.current(t("reader.readFailed")),
    });
    viewRef.current = view;
    (async () => {
      try {
        const size = await api.materialSize(paperId, materialId);
        await view.load(paperId, materialId, size);
        if (cancelled) return;
        leader.current = "main";
        setReady(true);
      } catch {
        if (cancelled) return;
        onMessage.current(t("reader.bilingual.failed"));
        onFailed.current();
      }
    })();
    return () => {
      cancelled = true;
      view.destroy();
      viewRef.current = null;
    };
  }, [paperId, materialId, mainRef]);

  // 開好了、原文那一格換了材料或重新開：對齊一次
  useEffect(() => {
    if (ready && props.mainReady) syncPane();
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, props.mainReady]);

  // 原頁疊譯文：段落的方框與譯文
  useEffect(() => {
    const view = viewRef.current;
    if (!view || !ready) return;
    view.setSegments(props.overlay ? props.segments : null);
    view.setOverlay(props.overlay);
  }, [ready, props.overlay, props.segments]);

  // 兩格的大小變了（換了直橫、拖了分隔線、筆記窗格開關）：兩格各自重新排版之後，照原文那一格再對齊一次。
  // 只改高度時兩格都不重新排版，但畫面中線換了位置，也要再對齊
  useEffect(() => {
    const main = props.mainScrollRef.current;
    const pane = scrollRef.current;
    if (!ready || !main || !pane) return;
    let id = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(id);
      id = requestAnimationFrame(() => (id = requestAnimationFrame(syncPane)));
    });
    observer.observe(main);
    observer.observe(pane);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(id);
    };
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, props.mainScrollRef]);

  // 單擊：原頁疊譯文上點一段暫看原文（「點內容」開著時），其餘顯示或隱藏頂列；雙擊放大到欄寬（原文那一格跟著）
  const onTap = useTapOrDouble(
    (x, y) => {
      const view = viewRef.current;
      if (props.overlay && props.tapContent && view) {
        const seg = view.segmentAt(x, y);
        if (seg && !shownText(seg).untranslated) {
          view.peek(seg.id);
          return;
        }
      }
      props.onTap();
    },
    (x, y) => void viewRef.current?.toggleColumnZoom(x, y),
  );

  // 拖分隔線：譯本這一格占兩格合起來的幾分之幾。
  // 分隔線上接一個原生的點擊監聽（什麼都不做）：手指按在分隔線上時，瀏覽器的觸控校正會把落點挪到旁邊
  // 看起來接得住點按的元素（頁面的文字層）；只有自己帶著原生點擊監聽的元素，才被當成按得到的、留得住落點
  const handleRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const el = handleRef.current;
    if (!el) return;
    const keep = () => {};
    el.addEventListener("click", keep);
    return () => el.removeEventListener("click", keep);
  }, []);
  const drag = useRef(false);
  const onHandleMove = (e: React.PointerEvent) => {
    if (!drag.current) return;
    const main = props.mainScrollRef.current?.getBoundingClientRect();
    const pane = scrollRef.current?.parentElement?.getBoundingClientRect();
    if (!main || !pane) return;
    props.onSplit(
      props.landscape ? splitAt(e.clientX, main.left, pane.right - main.left) : splitAt(e.clientY, main.top, pane.bottom - main.top),
    );
  };

  return (
    <section className={`bi-pane ${props.landscape ? "landscape" : "portrait"}`} aria-label={t("reader.bilingual.pane")}>
      <div className="bi-scroll" ref={scrollRef} onClick={onTap}>
        <div className="reader-wrap" ref={wrapRef} />
      </div>
      <button
        ref={handleRef}
        type="button"
        className="np-handle"
        aria-label={t("reader.bilingual.split")}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          drag.current = true;
        }}
        onPointerMove={onHandleMove}
        onPointerUp={() => (drag.current = false)}
        onPointerCancel={() => (drag.current = false)}
      >
        <span />
      </button>
    </section>
  );
}
