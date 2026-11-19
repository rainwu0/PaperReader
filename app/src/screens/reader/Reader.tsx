import { useCallback, useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";
import { api, AppError, kindOf, type Highlight, type PaperDetail, type ToolboxDock, type ToolboxMode } from "../../api";
import { isGeneratedPdf, lastDigest, sourcePdf } from "../../api/types";
import { displayTitle } from "../../format";
import { isBool, useBackOnTop, useLocalPref, useViewportSize } from "../../hooks";
import type { Nav, SheetKind } from "../../nav";
import type { PrefsState } from "../../prefs";
import { errorDetail, errorMessage, t } from "../../strings";
import { MATERIALS_CHANGED } from "../../translate/autoBuild";
import { holdPerPaper } from "../../perPaperApp";
import { BusyOverlay, Button, Details, IconButton, Toast, type ToastData } from "../../ui/components";
import { hintPending, markHintSeen, type HintId } from "../../hints";
import { BackIcon, InfoIcon, ShareIcon } from "../../ui/icons";
import { ExportSheet } from "../export/exportUi";
import { AnimatePresence, fadeInElement, m as Motion, useMotion, useReducedMotion } from "../../ui/motion";
import { CommentPane } from "./CommentPane";
import { NotesPane } from "./NotesPane";
import { PdfView, type OutlineEntry, type SwitchPosition, type ViewPosition } from "./pdfView";
import { CompareView } from "./CompareView";
import { DigestView } from "./DigestView";
import { Toolbox, useToolboxKeepOpen, type ToolDef, type ToolboxLayout } from "./Toolbox";
import {
  BrushIcon,
  CommentIcon,
  GenerateIcon,
  HighlightIcon,
  JumpIcon,
  MoonIcon,
  NotesIcon,
  SourceIcon,
  TapIcon,
  TextIcon,
  TranslationIcon,
} from "./toolIcons";
import { OutlineIcon } from "../../ui/icons";
import { useNotes } from "./useNotes";
import { useInk } from "./useInk";
import { useNotesPane } from "./useNotesPane";
import { otherDevicePosition } from "./otherDevice";
import { CardPop, SegmentPop } from "./SegmentParts";
import { pageRegionHint } from "../../ext/pageRegions";
import { useReaderViews, type SegTarget } from "./useReaderViews";
import { useHighlighting } from "./useHighlighting";
import { QuickCard, useQuickTranslate } from "./QuickTranslate";
import { offersQuick } from "./quickText";
import type { SelectionCapture } from "./pdfView";
import {
  BrushBar,
  brushStyleOf,
  CommentsSheet,
  HighlightEdit,
  HighlightMenu,
  HintCard,
  JumpSheet,
  LoadingHint,
  OutlineSheet,
  ViewsSheet,
  type BrushPick,
  type OutlineState,
  type ViewOption,
} from "./ReaderSheets";

const SAVE_AFTER_MS = 2000; // 停下來約兩秒就記下位置
const FLASH_AFTER_MS = 500; // 跳到一條劃線之後等頁面畫好，再讓它閃一下（連結、筆記窗格、開啟時帶著的劃線都用這個）
const BAR_HEIGHT = 49; // 頂列在狀態列以下的高度：觸控高度 48（`--tap`）加下框線 1
const DOUBLE_TAP_MS = 300; // 兩下點在這麼短的時間內、這麼近的地方算雙擊（Android 的雙擊時限也是 300ms）
const DOUBLE_TAP_SLOP = 40; // 像素
const READ_ERROR_AGAIN_MS = 10_000; // 讀不出來的提示收掉之後，又有頁讀不出來時隔多久再說一次
const PEEK_H = 56; // 筆記窗格縮成一條時的高度
const LONG_PRESS_MS = 500; // 按住劃線這麼久開小選單
const LONG_PRESS_SLOP = 10; // 像素：手指移動超過這麼多就是在捲動，不算長按
// 這台裝置上記住的選擇：短評標記開著沒有（沒記過就是開）
const COMMENTS_KEY = "pr-comments-show";

type LoadState =
  | { phase: "loading" }
  | { phase: "ready"; total: number }
  | { phase: "failed"; message: string; detail?: string };

/** 一條劃線第一行的上緣（PDF 頁面座標，越大越靠上）、左緣：同一頁裡排先後。 */
const topOf = (h: Highlight): number => Math.max(...h.rects.map((r) => Math.max(r[1], r[3])), 0);
const leftOf = (h: Highlight): number => Math.min(...h.rects.map((r) => Math.min(r[0], r[2])), Infinity);

export function Reader(props: {
  paperId: string;
  /** 從連結來的目標（頁、材料、劃線）。 */
  target: {
    page?: number | undefined;
    material?: string | undefined;
    highlight?: string | undefined;
    view?: "compare" | "overlay" | "digest" | "notes" | undefined;
    /** 和 `page` 一起給：到那一頁、框出這一處（PDF 頁面座標；文獻資訊的「看原文」）。 */
    box?: [number, number, number, number] | undefined;
    /** 和 `page` 一起給：原文那一頁文字裡的 UTF-16 位置與原樣文字，框出它（首頁搜尋的「內文」）。 */
    range?: [number, number] | undefined;
    quote?: string | undefined;
    /** 和 `view: "digest"` 一起給：重點筆記的這一點。 */
    point?: string | undefined;
  };
  nav: Nav;
  /** 蓋在閱讀畫面上的面板。 */
  sheets: SheetKind[];
  prefs: PrefsState;
  /** 記下了新的總頁數或離開時的位置：首頁的「第 7／30 頁」要重新讀。 */
  onChanged: () => void;
}) {
  const { paperId, nav, prefs } = props;
  const scrollRef = useRef<HTMLDivElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<PdfView | null>(null);
  const materialRef = useRef<string | null>(null);
  const saveTimer = useRef(0);
  const carry = useRef<SwitchPosition | null>(null);
  const opened = useRef(false);
  const progressRef = useRef<Awaited<ReturnType<typeof api.readingProgress>> | null>(null);
  // 這台裝置的代號：閱讀位置是另一台記的時先問要不要跳過去（`otherDevice.ts`，同步包）
  const deviceRef = useRef<string | null>(null);
  // 打開材料之後要去的地方：連結的頁與劃線、從譯文「看版面」的那一段
  const targetRef = useRef<typeof props.target & { segment?: SegTarget }>(
    props.target.box && props.target.page ? { ...props.target, segment: { page: props.target.page - 1, bbox: props.target.box } } : props.target,
  );
  // 捲動或縮放了（PDF 上點一段的小框要收起）
  const onMovedRef = useRef(() => {});

  const [detail, setDetail] = useState<PaperDetail | null>(null);
  const [materialId, setMaterialId] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [load, setLoad] = useState<LoadState>({ phase: "loading" });
  // 讀不出來時按「再試一次」：重新開同一份材料
  const [reopen, setReopen] = useState(0);
  const readErrorAt = useRef(0);
  const [page, setPage] = useState(1);
  const [barVisible, setBarVisible] = useState(true);
  const barVisibleRef = useRef(true);
  useEffect(() => {
    barVisibleRef.current = barVisible;
  });
  const [saveNote, setSaveNote] = useState<string | null>(null);
  const failures = useRef(0);
  const noted = useRef(false);
  const [outline, setOutline] = useState<OutlineState>({ phase: "loading" });
  // 短暫提示：給一句話，或連同一顆按鈕（`ToastData`）
  const [toast, setToast] = useReducer(
    (_: ToastData | null, next: string | ToastData | null) => (typeof next === "string" ? { message: next } : next),
    null,
  );
  // 跳到別處之後回得去：跳頁、大綱、筆記窗格的劃線、連結、重點筆記的頁碼跳過去之後，
  // 頁面下方出現「回到第 n 頁」；按了回到跳之前的位置（含縮放），捲離落點兩頁以上就收
  const [back, setBack] = useState<{ pos: ViewPosition; landing: number } | null>(null);
  /** 在同一份材料裡跳過去：先記下現在的位置，跳完再記落點。 */
  const jumpWithin = useCallback((go: () => void) => {
    const view = viewRef.current;
    if (!view || view.numPages === 0) {
      go();
      return;
    }
    const from = view.getPosition();
    go();
    const landing = view.getPosition().page;
    setBack(landing === from.page ? null : { pos: from, landing });
  }, []);
  // 首次提示：一次一則
  const [hint, setHint] = useState<HintId | null>(null);
  const offerHint = useCallback((id: HintId) => {
    if (hintPending(id)) setHint((cur) => cur ?? id);
  }, []);
  const closeHint = () => {
    if (hint) markHintSeen(hint);
    setHint(null);
  };

  // 劃線、筆刷、筆記窗格
  const notes = useNotes(paperId);
  const ink = useInk(paperId, () => setToast(t("reader.brush.failed")));
  const { pane, update: updatePane, toggle: togglePane, leave } = useNotesPane(nav);
  const [brushOn, setBrushOn] = useState(false);
  const [brush, setBrush] = useState<BrushPick>({ color: 0, width: 0, eraser: false });
  const [busy, setBusy] = useState(false);
  const vp = useViewportSize();
  const [dockH, setDockH] = useState(0);

  const materials = detail?.paper.materials ?? [];
  const source = sourcePdf(materials);
  const current = materials.find((m) => m.id === materialId);
  // 重點筆記：最後做好的那一份，從「檢視切換」打開
  const digestMat = detail ? lastDigest(detail.paper.materials) : undefined;
  const digestId = digestMat?.id;
  const startView = useRef(props.target.view);

  // 閱讀畫面開著時，逐篇處理讓路（閱讀與它各開一份 PDF 會吃掉手機的記憶體）
  useEffect(() => holdPerPaper(), []);

  // 閱讀時螢幕不自動關閉：只在這個畫面開著、而且設定打開時；離開就恢復
  const keepAwake = prefs.prefs.keep_awake;
  useEffect(() => {
    if (!keepAwake) return;
    api.setKeepAwake(true).catch(() => {});
    return () => {
      api.setKeepAwake(false).catch(() => {});
    };
  }, [keepAwake]);

  const onSaveFailed = useCallback((e: unknown) => {
    const kind = e instanceof AppError ? e.kind : "Unknown";
    failures.current += 1;
    const hopeless = kind === "ReadOnly" || kind === "NoSpace" || kind === "PermissionDenied";
    if (!noted.current && (hopeless || failures.current >= 2)) {
      noted.current = true;
      setSaveNote(t("reader.saveFailed"));
    }
  }, []);

  /** 記下現在的位置（含放大後水平捲到哪）。交回的 promise 在寫完或失敗之後完成。 */
  // 打開、捲動或縮放了幾次，記下位置時是第幾次：記下之後沒有再動過就不再記。同一篇開著兩個閱讀畫面時（從產生畫面
  // 「看重點筆記」會再疊一個），底下那個沒動過，離開時不可以用它舊的位置蓋掉上面那個剛記下的
  const moves = useRef(0);
  const savedMoves = useRef(0);
  const savePosition = useCallback((): Promise<void> => {
    window.clearTimeout(saveTimer.current);
    const view = viewRef.current;
    const material = materialRef.current;
    if (!view || !material || view.numPages === 0 || moves.current === savedMoves.current) return Promise.resolve();
    const p = view.getPosition();
    const at = moves.current;
    return api
      .recordPosition(paperId, material, p.page, p.offset, p.zoom, p.left)
      .then(() => {
        failures.current = 0;
        savedMoves.current = at;
      })
      .catch(onSaveFailed);
  }, [paperId, onSaveFailed]);
  // 離開時記下的位置寫完之後，首頁的「讀到第幾頁」要重新讀（首頁一直留在底下，不會自己重讀）
  const onChangedRef = useRef(props.onChanged);
  useEffect(() => {
    onChangedRef.current = props.onChanged;
  });

  // 先讀這篇的描述與閱讀位置，決定打開哪一份材料
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [d, progress, me] = await Promise.all([
          api.getPaper(paperId),
          api.readingProgress(paperId),
          api.thisDevice().catch(() => null),
        ]);
        if (cancelled) return;
        progressRef.current = progress;
        deviceRef.current = me;
        setDetail(d);
        setTitle(displayTitle(d.paper.bib));
        const src = sourcePdf(d.paper.materials);
        const wanted = targetRef.current.material;
        const chosen = d.paper.materials.find((m) => m.id === wanted && m.form === "pdf") ?? src;
        if (!chosen) {
          setLoad({ phase: "failed", message: t("reader.noMaterial") });
          return;
        }
        setMaterialId(chosen.id);
      } catch (e) {
        if (cancelled) return;
        const kind = e instanceof AppError ? e.kind : "Unreadable";
        setLoad({
          phase: "failed",
          message: errorMessage(kind === "Unknown" ? "Unreadable" : kind),
          detail: errorDetail(kind === "Unknown" ? "Unreadable" : kind),
        });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [paperId]);

  // 材料清單可能變了：產生好（或更新了）譯本 PDF、從產生畫面或文獻資訊回來（開始翻譯、加入譯本）。
  // 重新讀這篇的描述，工具箱與檢視切換才看得到新的；正在看的就是更新的那一份時重新打開它，停在同一處
  const detailRef = useRef(detail);
  useEffect(() => {
    detailRef.current = detail;
  });
  const refreshDetail = useCallback(() => {
    api
      .getPaper(paperId)
      .then((next) => {
        const id = materialRef.current;
        const timeOf = (p: PaperDetail | null) => p?.paper.materials.find((m) => m.id === id)?.provenance?.time;
        const changed = id !== null && timeOf(detailRef.current) !== timeOf(next);
        setDetail(next);
        if (changed) setReopen((n) => n + 1);
      })
      .catch(() => {});
  }, [paperId]);
  useEffect(() => {
    const on = (e: Event) => {
      if ((e as CustomEvent<{ paperId: string }>).detail.paperId === paperId) refreshDetail();
    };
    window.addEventListener(MATERIALS_CHANGED, on);
    return () => window.removeEventListener(MATERIALS_CHANGED, on);
  }, [paperId, refreshDetail]);
  useBackOnTop(nav, "reader", refreshDetail);

  // 打開（或換到另一份）材料
  const inkRef = useRef(ink);
  useEffect(() => {
    inkRef.current = ink;
  });
  useEffect(() => {
    if (!materialId) return;
    let cancelled = false;
    const scroll = scrollRef.current;
    const wrap = wrapRef.current;
    if (!scroll || !wrap) return;
    setLoad({ phase: "loading" });
    hl.setSelection(null);

    const view = new PdfView({
      scroll,
      wrap,
      onPage: (p) => setPage(p),
      // 捲動與縮放時每一格（requestAnimationFrame 節流）都會來：只做便宜的事。頂列已經收起就不再設狀態
      // （每格設一次會讓 React 每格都排一次更新），存位置的計時器停下兩秒才真的寫
      onMove: () => {
        moves.current += 1;
        if (barVisibleRef.current) setBarVisible(false);
        // 捲動或縮放了：點一段的小框收起（正在改的不收）
        onMovedRef.current();
        window.clearTimeout(saveTimer.current);
        saveTimer.current = window.setTimeout(savePosition, SAVE_AFTER_MS);
      },
      onStroke: (st) => {
        const material = materialRef.current;
        return material ? inkRef.current.add(material, st) : undefined;
      },
      onErase: (id) => void inkRef.current.erase(id),
      // 開啟之後有一段讀不出來：說一聲，「再試一次」重新開這一份（停在同一處）。之後捲到別的頁也讀不出來時，
      // 隔一陣子再說一次（提示幾秒後會收掉，空白的頁不能沒有說明）
      onReadError: () => {
        if (cancelled || Date.now() - readErrorAt.current < READ_ERROR_AGAIN_MS) return;
        readErrorAt.current = Date.now();
        setToast({ message: t("reader.readFailed"), action: { label: t("common.retry"), run: () => setReopen((n) => n + 1) } });
      },
    });
    viewRef.current = view;
    materialRef.current = null;

    (async () => {
      try {
        const size = await api.materialSize(paperId, materialId);
        const total = await view.load(paperId, materialId, size);
        if (cancelled) return;
        materialRef.current = materialId;
        setLoad({ phase: "ready", total });
        // 記下總頁數（核心已經對的就不寫）
        api
          .setMaterialPages(paperId, materialId, total)
          .then((changed) => changed && props.onChanged())
          .catch(() => {});
        const tgt = targetRef.current;
        const pos = progressRef.current?.position;
        if (tgt.segment || tgt.page) {
          // 換材料時帶著指定的去處（某一段、某一頁）：縮放沿用換之前的，再去那裡
          if (carry.current) view.restoreSwitch(carry.current);
          if (tgt.segment) view.showSegment(tgt.segment.page, tgt.segment.bbox);
          else if (tgt.page && tgt.range && tgt.quote !== undefined) void view.showRange(tgt.page - 1, tgt.range, tgt.quote);
          else if (tgt.page) view.goToPage(tgt.page);
        } else if (carry.current) {
          // 在原文與譯本之間切換：停在同一頁
          if (!view.restoreSwitch(carry.current)) setToast(t("reader.view.shorter"));
          fadeInElement(scroll);
        } else if (pos && pos.material === materialId) {
          const elsewhere = otherDevicePosition(progressRef.current, deviceRef.current, materialId);
          if (elsewhere) {
            // 最新的位置是另一台記的：先停在這台自己讀到的地方，問一次要不要跳過去
            view.restore(elsewhere.own);
            const other = elsewhere.other;
            setToast({
              message: t("reader.otherDevice", { page: other.page }),
              action: { label: t("reader.jump.go"), run: () => viewRef.current?.restore(other) },
            });
          } else {
            view.restore(pos);
          }
        }
        setPage(view.getPosition().page);
        // 打開（含回到上次的位置）也算一次「動了」：沒捲動就離開也照樣記下這一頁（和原本一樣），
        // 兩秒後記下之後，這個畫面要再動過才會再記
        moves.current += 1;
        window.clearTimeout(saveTimer.current);
        saveTimer.current = window.setTimeout(savePosition, SAVE_AFTER_MS);
        if (tgt.highlight) {
          const id = tgt.highlight;
          window.setTimeout(() => view.flash(id), FLASH_AFTER_MS);
        }
        targetRef.current = {};
        view
          .outline()
          .then((items) => !cancelled && setOutline({ phase: "ready", items }))
          .catch(() => !cancelled && setOutline({ phase: "failed" }));
        if (!opened.current) {
          opened.current = true;
          await api.recordOpen(paperId, materialId).catch(onSaveFailed);
        }
      } catch (e) {
        if (cancelled) return;
        const kind = e instanceof AppError ? e.kind : "Unreadable";
        setLoad({
          phase: "failed",
          message: errorMessage(kind === "Unknown" ? "Unreadable" : kind),
          detail: errorDetail(kind === "Unknown" ? "Unreadable" : kind),
        });
      }
    })();

    const onHidden = () => {
      if (document.visibilityState === "hidden") savePosition();
    };
    document.addEventListener("visibilitychange", onHidden);
    window.addEventListener("pagehide", savePosition);

    return () => {
      cancelled = true;
      document.removeEventListener("visibilitychange", onHidden);
      window.removeEventListener("pagehide", savePosition);
      void savePosition().then(() => onChangedRef.current());
      carry.current = view.getSwitchPosition();
      view.destroy();
      viewRef.current = null;
    };
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [paperId, materialId, savePosition, onSaveFailed, reopen]);

  // 從連結來的新目標（已經開著這一篇時再點一次連結）
  useEffect(() => {
    const on = (e: Event) => {
      const d = (e as CustomEvent<{ paperId: string; page?: number; highlight?: string }>).detail;
      if (d.paperId !== paperId) return;
      if (source && materialId !== source.id) {
        carry.current = null;
        targetRef.current = { page: d.page, highlight: d.highlight };
        setMaterialId(source.id);
        return;
      }
      const goPage = d.page;
      if (goPage) jumpWithin(() => viewRef.current?.goToPage(goPage));
      const id = d.highlight;
      if (id) window.setTimeout(() => viewRef.current?.flash(id), FLASH_AFTER_MS);
    };
    window.addEventListener("pr-goto", on);
    return () => window.removeEventListener("pr-goto", on);
  }, [paperId, source, materialId, jumpWithin]);

  const total = load.phase === "ready" ? load.total : 0;
  const ready = load.phase === "ready";
  // 選字後浮出的劃線鈕（很頻繁，100ms）
  const popMotion = useMotion("pop");

  // 第一次讀到有字的頁面：說一次長按選字劃線
  useEffect(() => {
    if (ready) offerHint("select");
  }, [ready, offerHint]);

  // ---------- 看法與段落：原文、譯本、原頁疊譯文、譯文；PDF 上點一段（`useReaderViews.ts`） ----------

  const views = useReaderViews({
    paperId,
    nav,
    sheets: props.sheets,
    materials,
    materialId,
    ready,
    viewRef,
    switchTo: (id) => switchTo(id),
    openAt: (id, target) => openAt(id, target),
    setToast,
    offerHint,
    hasDigest: !!digestMat,
    openDigest: () => nav.push({ name: "sheet", kind: "digest" }),
    onParagraphComment: (box) => void hl.paragraphComment(box),
  });
  const { overlayActive, onSource } = views;
  const hlPopOpen = props.sheets.includes("hlPop");
  // 選取文字的快速翻譯：卡片是導覽堆疊的一層，捲動、縮放、點卡片外就收起
  const quick = useQuickTranslate(nav, props.sheets);
  // 捲動時每格都會來：收起只做一次（返回還沒完成之前不再退一層）
  const hlClosing = useRef(false);
  useEffect(() => {
    onMovedRef.current = () => {
      views.onMoved();
      if (quick.isOpen) quick.close();
      // 捲動或縮放了：劃線的小選單收起（它對齊的位置已經不對）
      if (hlPopOpen && !hlClosing.current) {
        hlClosing.current = true;
        void nav.back().finally(() => {
          hlClosing.current = false;
        });
      }
    };
  });

  // 從產生畫面來的：直接進疊加、譯文或重點筆記（只做一次）
  const overlayReady = !!views.textProps?.overlay.doc;
  useEffect(() => {
    if (!ready || !startView.current) return;
    const v = startView.current;
    if (v === "notes") {
      // 首頁搜尋的「內文」找到筆記檔的自由書寫：打開筆記窗格
      startView.current = undefined;
      if (!pane.open) togglePane();
      return;
    }
    if (v === "digest") {
      if (!digestId) return;
      startView.current = undefined;
      nav.push({ name: "sheet", kind: "digest" });
      return;
    }
    if (!overlayReady) return;
    startView.current = undefined;
    if (v === "overlay") views.showOverlay();
    else views.openText();
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, overlayReady, digestId, nav]);

  // ---------- 劃線（`useHighlighting.ts`） ----------

  const generatedHere = current ? isGeneratedPdf(current) : false;
  const hl = useHighlighting({
    viewRef,
    notes,
    materialId,
    ready,
    brushOn,
    canMark: !overlayActive,
    generated: generatedHere,
    nav,
    setToast,
  });
  const { selection, highlightMode } = hl;
  // 拖選取的把手時按鈕列淡出（移除動畫時直接隱藏），放開後再出現
  const reducedMotion = useReducedMotion() ?? false;
  const pillMotion = hl.dragging
    ? { ...popMotion, animate: { opacity: 0, transition: { duration: reducedMotion ? 0 : 0.12 } } }
    : popMotion;

  // ---------- 選取文字的快速翻譯 ----------
  // 目標語言：選的已經是譯文的語言（譯本 PDF 上的中文）時不給「翻譯」（`quickText.offersQuick`）
  const [targetLang, setTargetLang] = useState("zh-TW");
  useEffect(() => {
    api
      .translateSettings()
      .then((v) => setTargetLang(v.targetLang))
      .catch(() => {});
  }, []);
  const quickSeq = useRef(0);
  /** 選取上方浮出卡片：位置與前後各一句在這裡算一次；系統的選取收掉（卡片留著一份選取，劃線用它）。 */
  const openQuick = (cap: SelectionCapture) => {
    const view = viewRef.current;
    if (!view) return;
    const rect = view.captureRect(cap) ?? cap.anchor;
    const ctx = view.selectionContext(cap);
    const insetTop = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--inset-top")) || 0;
    window.getSelection()?.removeAllRanges();
    hl.setSelection(null);
    quickSeq.current += 1;
    quick.open({ cap, rect, before: ctx.before, after: ctx.after, topLimit: insetTop + (barVisible ? BAR_HEIGHT : 0) });
  };
  // 換了材料：卡片收起
  useEffect(() => {
    quick.close();
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [materialId]);

  // ---------- 短評標記與短評窗格 ----------
  const [commentsShown, setCommentsShown] = useLocalPref(COMMENTS_KEY, true, isBool);
  // 筆刷開著（標記會擋住畫筆）、原頁疊譯文開著（劃線是蓋住的）時不顯示
  const commentsVisible = commentsShown && !brushOn && !overlayActive;
  useEffect(() => {
    viewRef.current?.setCommentDisplay({ show: commentsVisible });
  }, [commentsVisible, ready, materialId]);

  // 這份材料上有短評的劃線，照頁序、同頁由上到下、由左到右（窗格的「上一則」「下一則」）
  const commented = useMemo(
    () =>
      (notes.view?.doc.highlights ?? [])
        .filter((h) => h.material === materialId && h.complete && h.comment.trim() !== "" && !hl.pending.includes(h.id))
        .sort((a, b) => a.page.index - b.page.index || topOf(b) - topOf(a) || leftOf(a) - leftOf(b)),
    [notes.view, materialId, hl.pending],
  );
  const [cpId, setCpId] = useState<string | null>(null);
  const cpLayer = props.sheets.includes("commentPane");
  const cpIndex = cpId === null ? -1 : commented.findIndex((h) => h.id === cpId);
  const cpHighlight = cpIndex >= 0 ? (commented[cpIndex] ?? null) : null;
  const cpOpen = cpLayer && cpHighlight !== null && commentsVisible;
  // 返回鍵退掉那一層：窗格收起
  const cpWasLayer = useRef(false);
  useEffect(() => {
    if (!cpLayer && cpWasLayer.current) setCpId(null);
    cpWasLayer.current = cpLayer;
  }, [cpLayer]);
  // 那一條不見了（刪了、短評清空、換了材料）或標記關了：窗格收起
  useEffect(() => {
    if (cpLayer && cpId !== null && !cpOpen) void nav.back();
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [cpLayer, cpId, cpOpen]);

  // 筆記窗格開著（拉高了）時，點有短評的劃線不開短評窗格：筆記窗格的清單捲到那一項並標出
  const notesUp = pane.open && pane.size !== "peek";
  const [listFocus, setListFocus] = useState<{ id: string; n: number } | null>(null);
  useEffect(() => {
    if (!notesUp) setListFocus(null);
  }, [notesUp]);
  const [onlyComments, setOnlyComments] = useState(false);

  const openCommentPane = (id: string) => {
    if (notesUp) {
      setListFocus((cur) => ({ id, n: (cur?.n ?? 0) + 1 }));
      return;
    }
    setCpId(id);
    if (!cpLayer) nav.push({ name: "sheet", kind: "commentPane" });
  };
  /** 點頁面：點在有短評的劃線上（短評標記開著時）打開短評窗格；處理了交回 `true`。 */
  const commentAt = (x: number, y: number): boolean => {
    if (!commentsVisible) return false;
    const id = hl.highlightIdAt(x, y);
    if (!id || !commented.some((h) => h.id === id)) return false;
    openCommentPane(id);
    return true;
  };

  // 強調框：窗格開著的那一條；筆記窗格開著時是清單標出的那一條
  const focusId = cpOpen ? cpId : notesUp ? (listFocus?.id ?? null) : null;
  useEffect(() => {
    viewRef.current?.setFocus(focusId);
  }, [focusId, ready, materialId]);
  /** 把強調框那一條捲到窗格的上方（窗格的大小穩定之後）。 */
  const revealFocus = (top: number) => {
    const h = commented.find((x) => x.id === cpId);
    if (h) viewRef.current?.revealAbove(h.id, h.page.index + 1, top);
  };
  const cpStep = (delta: number) => {
    const next = commented[cpIndex + delta];
    if (next) setCpId(next.id);
  };

  // ---------- 長按劃線：一律開小選單，不受「點內容」開關影響 ----------
  // 按下去時不讀版面（點在哪一條劃線上要量頁面的位置，手勢剛開始時會逼瀏覽器先排一次版面，雙指縮放因此變慢）：
  // 只記下位置，按滿時限、手指沒動才判斷是不是在劃線上。移動與放開的監聽只在按著的這段時間掛
  const press = useRef<{ x: number; y: number; timer: number; off: () => void } | null>(null);
  const longPressAt = useRef(0);
  const endPress = () => {
    const p = press.current;
    press.current = null;
    if (!p) return;
    window.clearTimeout(p.timer);
    p.off();
  };
  useEffect(() => () => endPress(), []);
  const onPressStart = (e: React.PointerEvent) => {
    // 第二根手指放上來（雙指縮放）、筆刷開著、不是主要的按鍵：不算長按
    if (press.current || brushOn || !e.isPrimary || e.button !== 0) {
      endPress();
      return;
    }
    const { clientX: x, clientY: y } = e;
    const move = (ev: PointerEvent) => {
      if (Math.hypot(ev.clientX - x, ev.clientY - y) > LONG_PRESS_SLOP) endPress();
    };
    const extra = () => endPress();
    window.addEventListener("pointermove", move, { passive: true });
    window.addEventListener("pointerup", extra, { passive: true });
    window.addEventListener("pointercancel", extra, { passive: true });
    window.addEventListener("pointerdown", extra, { passive: true, capture: true });
    press.current = {
      x,
      y,
      off: () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", extra);
        window.removeEventListener("pointercancel", extra);
        window.removeEventListener("pointerdown", extra, { capture: true });
      },
      timer: window.setTimeout(() => {
        endPress();
        if (Date.now() - longPressAt.current < 1000 || props.sheets.includes("hlPop")) return;
        const id = hl.highlightIdAt(x, y);
        if (!id) return;
        longPressAt.current = Date.now();
        hl.longPressHighlight(id, x, y);
      }, LONG_PRESS_MS),
    };
  };

  // 點頁面：單擊要先等一下，確定不是雙擊；雙擊是放大到那一欄的寬度，再雙擊縮回（`PdfView.toggleColumnZoom`）。
  // 單擊：小框開著就收起；「點內容」開著時，點劃線開短評、點疊譯文暫看原文、點一段出小框；其餘顯示或隱藏頂列
  const lastTap = useRef<{ t: number; x: number; y: number; timer: number } | null>(null);
  useEffect(() => () => window.clearTimeout(lastTap.current?.timer), []);
  const singleTap = (x: number, y: number) => {
    // 劃線的小選單、快速翻譯的卡片開著：點別處只是收起它
    if (props.sheets.includes("hlPop") || quick.isOpen) {
      void nav.back();
      return;
    }
    if (!views.tap(x, y, () => hl.tapHighlight(x, y), () => commentAt(x, y))) setBarVisible((v) => !v);
  };
  const onPageTap = (e: React.MouseEvent) => {
    if (brushOn) return;
    // 長按劃線剛開了小選單：放開手指的那一下不算點（不然馬上又收起）
    if (Date.now() - longPressAt.current < 700) return;
    if ((window.getSelection()?.toString() ?? "") !== "") return;
    const { clientX: x, clientY: y } = e;
    const prev = lastTap.current;
    if (prev && e.timeStamp - prev.t < DOUBLE_TAP_MS && Math.hypot(x - prev.x, y - prev.y) < DOUBLE_TAP_SLOP) {
      window.clearTimeout(prev.timer);
      lastTap.current = null;
      void viewRef.current?.toggleColumnZoom(x, y);
      return;
    }
    window.clearTimeout(prev?.timer);
    const timer = window.setTimeout(() => {
      lastTap.current = null;
      singleTap(x, y);
    }, DOUBLE_TAP_MS);
    lastTap.current = { t: e.timeStamp, x, y, timer };
  };

  // ---------- 筆刷 ----------

  useEffect(() => {
    viewRef.current?.setBrush(brushOn ? brushStyleOf(brush.color, brush.width) : null);
  }, [brushOn, brush.color, brush.width, ready, materialId]);
  useEffect(() => {
    viewRef.current?.setEraser(brushOn && brush.eraser);
  }, [brushOn, brush.eraser, ready, materialId]);

  // 筆刷的「復原」只撤銷這次打開筆刷以來的動作：關掉時清空
  const resetInkHistory = ink.resetHistory;
  useEffect(() => {
    if (!brushOn) resetInkHistory();
  }, [brushOn, resetInkHistory]);

  const brushTool = () => {
    setBrushOn((v) => !v);
    hl.setSelection(null);
  };

  /** 筆刷列的「復原」：撤銷畫面上看得到的頁上最後一個動作（畫的刪掉、擦的與清掉的畫回去）。 */
  const undoInk = async () => {
    const view = viewRef.current;
    if (!view || !materialId) return;
    const done = await ink.undo(materialId, view.visiblePages());
    if (!done) setToast(t("reader.brush.nothingToUndo"));
  };

  /** 清除這一頁的筆跡：直接清，提示給「復原」（整頁畫回去）。 */
  const clearPage = async () => {
    const view = viewRef.current;
    if (!view || !materialId) return;
    const step = await ink.clearPage(materialId, view.getPosition().page - 1);
    if (!step) return;
    setToast({
      message: t("reader.brush.cleared"),
      keep: true,
      action: { label: t("common.undo"), run: () => void inkRef.current.revert(step) },
    });
  };

  // 這份材料的筆跡畫在頁面上
  useEffect(() => {
    const view = viewRef.current;
    if (!view || !ready) return;
    view.setInk(ink.strokes.filter((s) => s.material === materialId));
  }, [ink.strokes, ready, materialId]);

  const flipPage = (delta: number) => {
    const v = viewRef.current;
    if (!v) return;
    v.goToPage(Math.min(Math.max(1, v.getPosition().page + delta), v.numPages));
  };

  // ---------- 切換、跳頁、大綱 ----------

  /** 換到另一份材料，停在同一處（原文與譯本 PDF 版面相同）。 */
  const switchTo = (m: string | undefined) => {
    if (!m || m === materialId) return;
    savePosition();
    carry.current = viewRef.current?.getSwitchPosition() ?? null;
    setMaterialId(m);
  };

  /** 換到另一份材料（同一份就直接去），去某一段或某一頁；縮放沿用換之前的。 */
  const openAt = (m: string, target: { segment?: SegTarget; page?: number }) => {
    if (m === materialId) {
      const seg = target.segment;
      const pageNo = target.page;
      if (seg) jumpWithin(() => viewRef.current?.showSegment(seg.page, seg.bbox));
      else if (pageNo) jumpWithin(() => viewRef.current?.goToPage(pageNo));
      return;
    }
    savePosition();
    targetRef.current = target;
    setMaterialId(m);
  };

  /** 回到原文的某一頁（重點筆記的頁碼）。 */
  const gotoSource = (pageNo: number) => {
    views.setOverlayOn(false);
    if (source) openAt(source.id, { page: pageNo });
  };

  const openJump = () => nav.push({ name: "sheet", kind: "jump" });
  /** 短評的顯示面板的「短評一覽」：筆記窗格開到半開，劃線清單只列有短評的。 */
  const [overviewWanted, setOverviewWanted] = useState(false);
  const openOverview = () => {
    setOverviewWanted(true);
    void nav.back();
  };
  // 面板退掉之後才開筆記窗格（窗格自己在返回堆疊上推一層，要等堆疊穩定）
  const menuLayer = props.sheets.includes("commentMenu");
  useEffect(() => {
    if (!overviewWanted || menuLayer) return;
    setOverviewWanted(false);
    setOnlyComments(true);
    if (!pane.open || pane.size === "peek") updatePane({ open: true, size: "half" });
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [overviewWanted, menuLayer]);
  const openOutline = () => nav.push({ name: "sheet", kind: "outline" });

  const goToOutlineEntry = async (entry: OutlineEntry) => {
    const target = await viewRef.current?.pageOfDest(entry.dest);
    if (target == null) return false;
    await nav.back();
    jumpWithin(() => viewRef.current?.goToPage(target));
    return true;
  };

  const jumpToHighlight = async (h: Highlight) => {
    // 全開時整個畫面都是筆記，看不到論文：縮回半開
    if (pane.size === "full") updatePane({ size: "half" });
    // 劃線在哪一份材料上就切到那一份（原文或譯本 PDF）
    const home = detail?.paper.materials.some((m) => m.id === h.material) ? h.material : source?.id;
    if (home && materialId !== home) {
      carry.current = null;
      targetRef.current = { page: h.page.index + 1, highlight: h.id };
      setMaterialId(home);
      return;
    }
    jumpWithin(() => viewRef.current?.goToPage(h.page.index + 1));
    window.setTimeout(() => viewRef.current?.flash(h.id), FLASH_AFTER_MS);
  };

  // 「回到第 n 頁」：捲離落點兩頁以上、換了材料就收
  useEffect(() => {
    if (back && Math.abs(page - back.landing) >= 2) setBack(null);
  }, [page, back]);
  useEffect(() => {
    setBack(null);
  }, [materialId]);
  const goBack = () => {
    const b = back;
    setBack(null);
    if (b) viewRef.current?.restore(b.pos);
  };

  // ---------- 移除一份材料（譯本選單、重點筆記） ----------

  /**
   * 移到回收區，不先問，給「復原」。正在看的就是它（或疊譯文開著、要移除的是逐段譯文）時先回到原文。
   * 復原：回收區裡這篇、這一項最晚移除的那一份放回來（核心的移除不交回回收區的資料夾名稱，和文獻資訊的做法相同）。
   */
  const removeMaterialHere = async (id: string) => {
    const d = detailRef.current;
    if (!d) return;
    if (id === materialId || (id === views.overlayMat?.id && views.overlayActive)) {
      views.showSource();
      // 等這一份的檢視關掉，再把檔案移走（不讓它還在讀的那一段報讀不出來）
      await new Promise((r) => window.setTimeout(r, 150));
    }
    try {
      await api.removeMaterial(paperId, id, d.token);
    } catch (e) {
      refreshDetail();
      setToast(errorMessage(kindOf(e)));
      return;
    }
    refreshDetail();
    onChangedRef.current();
    setToast({
      message: t("info.removed"),
      keep: true,
      action: {
        label: t("common.undo"),
        run: () =>
          void (async () => {
            const latest = (await api.listRemovedMaterials())
              .filter((e) => e.paper_id === paperId && e.material.id === id)
              .sort((a, b) => (b.removed_at ?? "").localeCompare(a.removed_at ?? ""))[0];
            if (!latest) throw new AppError("NotFound", "removed material");
            await api.restoreMaterial(latest.folder);
            refreshDetail();
            onChangedRef.current();
          })().catch((e) => setToast(`${t("undo.failed.title")}：${errorMessage(kindOf(e))}`)),
      },
    });
  };

  // ---------- 筆記窗格：交給別的應用、回到前景時重讀 ----------

  const openExternal = async (obsidian: boolean) => {
    try {
      const r = await api.openNotesExternal(paperId, obsidian);
      if (r === "no_app") {
        if (obsidian) {
          setToast({
            message: t("reader.notes.noObsidian"),
            action: { label: t("common.learnMore"), run: () => nav.push({ name: "help", topic: "obsidian" }) },
          });
        } else setToast(t("reader.notes.noApp"));
      }
      if (r === "not_shareable") setToast(t("reader.notes.notShareable"));
    } catch {
      setToast(t("reader.notes.openFailed"));
    }
  };

  // 從外部程式（Obsidian 等）改完回來：重新讀筆記檔
  const reloadNotes = notes.reload;
  useEffect(() => {
    const on = () => {
      if (document.visibilityState === "visible") void reloadNotes();
    };
    document.addEventListener("visibilitychange", on);
    window.addEventListener("focus", on);
    return () => {
      document.removeEventListener("visibilitychange", on);
      window.removeEventListener("focus", on);
    };
  }, [reloadNotes]);

  // ---------- 工具箱 ----------

  const layout: ToolboxLayout = {
    mode: prefs.prefs.toolbox_mode,
    dock: prefs.prefs.toolbox_dock,
    x: prefs.prefs.toolbox_x,
    y: prefs.prefs.toolbox_y,
  };
  const onLayout = (l: ToolboxLayout) =>
    prefs.update({ toolbox_mode: l.mode as ToolboxMode, toolbox_dock: l.dock as ToolboxDock, toolbox_x: l.x, toolbox_y: l.y });
  // 第一次把工具箱從圓鈕展開：說一次怎麼拖、怎麼停靠（看存下的模式變化；工具箱的拖動處理拿的是舊的回呼）
  const lastMode = useRef(layout.mode);
  useEffect(() => {
    if (lastMode.current === "mini" && layout.mode !== "mini") offerHint("toolbox");
    lastMode.current = layout.mode;
  }, [layout.mode, offerHint]);

  // 浮動的工具箱沒有釘住時，打開一篇從圓鈕開始（不一直蓋在頁面上）
  const [keepOpen, setKeepOpen] = useToolboxKeepOpen();
  const settled = useRef(false);
  useEffect(() => {
    if (settled.current || !prefs.loaded) return;
    settled.current = true;
    if (!keepOpen && layout.mode === "floating") onLayout({ ...layout, mode: "mini" });
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [prefs.loaded]);

  const tools: ToolDef[] = [
    {
      id: "source",
      label: t("reader.tool.source"),
      icon: <SourceIcon />,
      active: onSource && !overlayActive,
      view: true,
      onClick: views.showSource,
    },
    {
      id: "translation",
      label: t("reader.tool.translation"),
      icon: <TranslationIcon />,
      active: views.onTranslation,
      unavailable: !views.preferred && !digestMat,
      view: true,
      onClick: views.openTranslation,
      onMore: () => nav.push({ name: "sheet", kind: "views" }),
      moreLabel: t("reader.tool.translation.more"),
    },
    {
      id: "compare",
      label: t("reader.tool.compare"),
      icon: <TextIcon />,
      unavailable: !views.overlayMat,
      view: true,
      onClick: views.openText,
    },
    { id: "notes", label: t("reader.notes.title"), icon: <NotesIcon />, active: pane.open, view: true, onClick: togglePane },
    {
      id: "highlight",
      label: t("reader.tool.highlight"),
      icon: <HighlightIcon />,
      active: highlightMode,
      unavailable: overlayActive,
      onClick: hl.highlightTool,
    },
    { id: "brush", label: t("reader.tool.brush"), icon: <BrushIcon />, active: brushOn, onClick: brushTool },
    {
      id: "comment",
      label: t("reader.tool.comment"),
      icon: <CommentIcon />,
      unavailable: overlayActive,
      onClick: hl.commentTool,
      // 小箭頭（或長按）：短評的顯示（顯示短評、短評一覽）
      onMore: () => nav.push({ name: "sheet", kind: "commentMenu" }),
      moreLabel: t("reader.tool.comment.more"),
    },
    { id: "jump", label: t("reader.tool.jump"), icon: <JumpIcon />, onClick: openJump },
    { id: "outline", label: t("reader.tool.outline"), icon: <OutlineIcon />, onClick: openOutline },
    {
      id: "tap",
      label: t("reader.tool.tapContent"),
      icon: <TapIcon />,
      active: views.tapContent,
      onClick: () => {
        views.setTapContent(!views.tapContent);
        setToast(t(views.tapContent ? "reader.tapContent.off" : "reader.tapContent.on"));
      },
    },
    {
      id: "generate",
      label: t("reader.tool.generate"),
      icon: <GenerateIcon />,
      onClick: () => nav.push({ name: "generate", paperId }),
    },
    {
      id: "dark",
      label: t("settings.theme.dark"),
      icon: <MoonIcon />,
      active: prefs.prefs.dark_pages,
      onClick: () => {
        const next = !prefs.prefs.dark_pages;
        prefs.update({ dark_pages: next });
        const dark = document.documentElement.dataset["theme"] === "dark";
        if (next && !dark) {
          setToast({
            message: t("reader.darkPages.lightNote"),
            action: { label: t("reader.darkPages.toAppearance"), run: () => nav.push({ name: "settings" }) },
          });
        } else setToast(next ? t("reader.darkPages.on") : t("reader.darkPages.off"));
      },
    },
  ];

  // 換一份的選單：原文、各份譯本、疊譯文、譯文，加上重點筆記
  const pickView = (run: () => void) => () => void nav.back().then(run);
  const viewOptions: ViewOption[] = views.viewOptions(pickView);
  if (digestMat) {
    viewOptions.push({
      id: "digest",
      label: t("reader.view.digest"),
      current: false,
      run: pickView(() => nav.push({ name: "sheet", kind: "digest" })),
    });
  }

  const editing = hl.editing;
  const dockedBottom = layout.mode === "docked" && layout.dock === "bottom";
  useLayoutEffect(() => {
    const el = document.querySelector<HTMLElement>(".tb-docked.bottom");
    setDockH(dockedBottom && el ? Math.round(el.getBoundingClientRect().height) : 0);
  }, [dockedBottom, ready, vp.w, vp.h]);
  const pill = ready ? t("reader.page", { page, total }) : null;

  // 選取之後的「劃線」鈕放在選取第一行的下面（系統自己的複製選單出現在上面，不要疊在一起）；下面放不下就放上面
  const pillTop = selection
    ? selection.anchor.bottom + 80 < window.innerHeight
      ? selection.anchor.bottom + 44
      : selection.anchor.top - 130
    : 0;

  // 窗格的位置：直式在下方、橫式在右側；縮小時只剩下方一條。論文的捲動區讓出窗格占的地方
  const landscape = vp.w > vp.h;
  const paneHeight = !pane.open ? 0 : pane.size === "peek" ? PEEK_H : !landscape && pane.size === "half" ? Math.round(pane.split * vp.h) : 0;
  const paneWidth = pane.open && pane.size === "half" && landscape ? Math.round(pane.split * vp.w) : 0;
  const paneStyle: React.CSSProperties =
    pane.size === "full" ? {} : landscape && pane.size === "half" ? { width: paneWidth } : { height: paneHeight };
  const readerClass = ["screen reader", prefs.prefs.dark_pages ? "dark-pages" : "", pane.open ? "pane-open" : ""]
    .filter(Boolean)
    .join(" ");
  const rootStyle = {
    "--pane-lift": dockedBottom && dockH > 0 ? `${dockH}px` : "var(--inset-bottom)",
    "--pane-h": `${pane.size === "full" ? 0 : paneHeight}px`,
    "--pane-w": `${pane.size === "full" ? 0 : paneWidth}px`,
  } as React.CSSProperties;

  return (
    <div className={readerClass} style={rootStyle}>
      <div
        className="reader-scroll"
        ref={scrollRef}
        onClick={onPageTap}
        onPointerDown={onPressStart}
        onContextMenu={(e) => {
          // 長按在劃線上：不出系統的選單，開劃線的小選單。系統認出長按時可能先取消了手指（`pointercancel`），
          // 計時就不會到，所以這裡也開（兩邊誰先到都只開一次）
          if (brushOn) return;
          const id = hl.highlightIdAt(e.clientX, e.clientY);
          if (!id) return;
          e.preventDefault();
          if (Date.now() - longPressAt.current < 1000 || props.sheets.includes("hlPop")) return;
          endPress();
          longPressAt.current = Date.now();
          hl.longPressHighlight(id, e.clientX, e.clientY);
        }}
        role="document"
        aria-label={title}
      >
        <div className="reader-wrap" ref={wrapRef} />
      </div>

      {saveNote ? (
        <div className="reader-note" role="status" onClick={() => setSaveNote(null)}>
          {saveNote}
        </div>
      ) : null}
      <AnimatePresence>
        {hint && !saveNote ? (
          <HintCard key={hint} id={hint} text={hint === "card" ? pageRegionHint() : undefined} onClose={closeHint} />
        ) : null}
      </AnimatePresence>

      {barVisible ? (
        <header className="reader-bar">
          <IconButton label={t("common.back")} onClick={() => void leave()}>
            <BackIcon />
          </IconButton>
          <h1>{title}</h1>
          {views.preferred ? (
            <IconButton label={t(views.onTranslation ? "reader.swap.toSource" : "reader.swap.toTranslation")} onClick={views.swap}>
              <TranslationIcon />
            </IconButton>
          ) : null}
          <IconButton label={t("export.title")} onClick={() => nav.push({ name: "sheet", kind: "exportMenu" })}>
            <ShareIcon />
          </IconButton>
          <IconButton label={t("info.title")} onClick={() => nav.push({ name: "info", paperId })}>
            <InfoIcon />
          </IconButton>
        </header>
      ) : null}

      {ready && !dockedBottom ? (
        <button type="button" className="page-pill" aria-label={t("reader.jump.open", { page })} onClick={openJump}>
          <span className="pill-face">{pill}</span>
        </button>
      ) : null}
      {ready && back && !brushOn ? (
        <button type="button" className="back-pill" onClick={goBack}>
          {t("reader.back", { page: back.pos.page })}
        </button>
      ) : null}

      <AnimatePresence>
        {selection && !overlayActive && !brushOn && !highlightMode ? (
          <Motion.div
            key="pill"
            className="sel-pill"
            style={{ top: pillTop, pointerEvents: hl.dragging ? "none" : undefined }}
            role="toolbar"
            aria-label={t("reader.tool.highlight")}
            aria-hidden={hl.dragging || undefined}
            {...pillMotion}
          >
            <button type="button" onClick={() => void hl.create(selection, false)}>
              {t("reader.tool.highlight")}
            </button>
            <button type="button" onClick={() => void hl.create(selection, true)}>
              {t("reader.highlight.withComment")}
            </button>
            <button
              type="button"
              onClick={() =>
                void hl.paragraphComment({
                  x: (selection.anchor.left + selection.anchor.right) / 2,
                  y: (selection.anchor.top + selection.anchor.bottom) / 2,
                })
              }
            >
              {t("reader.highlight.paragraph")}
            </button>
            {offersQuick(selection.exact, targetLang) ? (
              <button type="button" onClick={() => openQuick(selection)}>
                {t("quick.action")}
              </button>
            ) : null}
          </Motion.div>
        ) : null}
      </AnimatePresence>

      <AnimatePresence>
        {quick.target ? (
          <QuickCard
            key={`quick-${quickSeq.current}`}
            paperId={paperId}
            target={quick.target}
            canMark={!overlayActive}
            onComment={(text) => {
              const cap = quick.target?.cap;
              quick.close();
              if (cap) void hl.create(cap, false, text);
            }}
            onFull={() => void nav.back().then(() => nav.push({ name: "generate", paperId }))}
            onToast={setToast}
          />
        ) : null}
      </AnimatePresence>

      <AnimatePresence>
        {brushOn ? (
          <BrushBar
            key="brush"
            pick={brush}
            onPick={setBrush}
            aboveDock={dockedBottom}
            onUndo={() => void undoInk()}
            onClear={() => void clearPage()}
            onFlip={flipPage}
          />
        ) : null}
      </AnimatePresence>

      <AnimatePresence>
        {hlPopOpen && hl.menu ? (
          <HighlightMenu
            key="hl-menu"
            highlight={hl.menu.highlight}
            anchor={hl.menu.anchor}
            onDelete={() => void hl.deleteFromMenu(hl.menu?.highlight.id ?? "")}
            onComment={() => void hl.openComment(hl.menu?.highlight.id ?? "")}
          />
        ) : null}
      </AnimatePresence>

      <AnimatePresence>
        {views.popProps ? (
          <SegmentPop key="pop" {...views.popProps} />
        ) : views.cardProps ? (
          <CardPop key="card" {...views.cardProps} onOpen={(id) => void nav.back().then(() => nav.push({ name: "reader", paperId: id }))} />
        ) : null}
      </AnimatePresence>

      {ready ? (
        <Toolbox
          layout={layout}
          onLayout={onLayout}
          tools={tools}
          pageText={pill}
          onPage={openJump}
          topOffset={barVisible ? BAR_HEIGHT : 0}
          keepOpen={keepOpen}
          onKeepOpen={setKeepOpen}
        />
      ) : null}

      <ViewsSheet
        open={props.sheets.includes("views")}
        options={viewOptions}
        onDismiss={() => void nav.back()}
        onRemove={(id) => void nav.back().then(() => removeMaterialHere(id))}
      />
      <CommentsSheet
        open={props.sheets.includes("commentMenu")}
        show={commentsShown}
        onShow={setCommentsShown}
        onOverview={openOverview}
        onDismiss={() => void nav.back()}
      />

      <AnimatePresence>
        {views.textOpen && views.textProps ? (
          <CompareView
            key="compare"
            paperId={paperId}
            {...views.textProps}
            totalPages={source?.pages ?? total}
            nav={nav}
            sheets={props.sheets}
            onClose={() => void nav.back()}
          />
        ) : null}
        {props.sheets.includes("digest") && digestMat ? (
          <DigestView
            key="digest"
            paperId={paperId}
            materialId={digestMat.id}
            method={digestMat.provenance?.method ?? null}
            nav={nav}
            sheets={props.sheets}
            onClose={() => void nav.back()}
            onJump={(pageNo) => void nav.back().then(() => gotoSource(pageNo))}
            onRemove={() => {
              const id = digestMat.id;
              void nav.back().then(() => removeMaterialHere(id));
            }}
            focusPoint={props.target.point}
          />
        ) : null}
      </AnimatePresence>

      <JumpSheet
        open={props.sheets.includes("jump")}
        total={total}
        onGo={(n) => void nav.back().then(() => jumpWithin(() => viewRef.current?.goToPage(n)))}
        onCancel={() => void nav.back()}
      />
      <OutlineSheet
        open={props.sheets.includes("outline")}
        outline={outline}
        onPick={goToOutlineEntry}
        onJumpInstead={() => void nav.back().then(openJump)}
        onDismiss={() => void nav.back()}
      />

      <AnimatePresence>
        {cpOpen && cpHighlight ? (
          <CommentPane
            key="comment-pane"
            highlight={cpHighlight}
            index={cpIndex}
            total={commented.length}
            onPrev={() => cpStep(-1)}
            onNext={() => cpStep(1)}
            onClose={() => void nav.back()}
            onEdit={() => void nav.back().then(() => hl.editComment(cpHighlight.id))}
            onDelete={() => void nav.back().then(() => hl.removeHighlight(cpHighlight.id))}
            onJump={() => revealFocus(document.querySelector(".cm-pane")?.getBoundingClientRect().top ?? window.innerHeight)}
            onSettled={revealFocus}
          />
        ) : null}
      </AnimatePresence>

      <AnimatePresence>
        {pane.open ? (
          <NotesPane
            key="pane"
            notes={notes}
            title={title}
            size={pane.size}
            landscape={landscape}
            style={paneStyle}
            onSize={(size) => updatePane({ size })}
            onSplit={(f) => updatePane({ split: Math.min(landscape ? 0.75 : 0.85, Math.max(landscape ? 0.25 : 0.2, f)) })}
            onClose={() => updatePane({ open: false })}
            onJump={(h) => {
              setListFocus((cur) => ({ id: h.id, n: (cur?.n ?? 0) + 1 }));
              void jumpToHighlight(h);
            }}
            onDelete={hl.removeHighlight}
            onlyComments={onlyComments}
            onOnlyComments={setOnlyComments}
            focus={listFocus}
            hiddenIds={hl.pending}
            onMessage={setToast}
            onOpenExternal={(obsidian) => void openExternal(obsidian)}
          />
        ) : null}
      </AnimatePresence>
      <AnimatePresence>
        {props.sheets.includes("highlightEdit") && editing ? (
          <HighlightEdit
            key={editing.id}
            highlight={editing}
            onSave={async (text) => {
              setBusy(true);
              const m = await notes.setComment(editing.id, text);
              setBusy(false);
              if (m) setToast(m);
              await nav.back();
            }}
            onDelete={async () => {
              const id = editing.id;
              await nav.back();
              hl.removeHighlight(id);
            }}
            onCancel={() => void nav.back()}
          />
        ) : null}
        {props.sheets.includes("exportMenu") ? (
          <ExportSheet
            key="export"
            paperId={paperId}
            materialId={materialId ?? undefined}
            overlayId={views.overlayMat?.id}
            onClose={() => void nav.back()}
          />
        ) : null}
      </AnimatePresence>
      <BusyOverlay active={load.phase === "loading" || busy} label={t("reader.loading")} delayMs={1000} />
      {load.phase === "loading" ? <LoadingHint /> : null}
      <Toast toast={toast} onDone={() => setToast(null)} />

      {load.phase === "failed" ? (
        <div className="center-fill" style={{ position: "absolute", inset: 0, background: "var(--bg)" }}>
          <h2>{t("reader.failed.title")}</h2>
          <p className="muted">{load.message}</p>
          {load.detail ? <Details>{load.detail}</Details> : null}
          <Button primary onClick={nav.back}>
            {t("common.back")}
          </Button>
        </div>
      ) : null}
    </div>
  );
}
