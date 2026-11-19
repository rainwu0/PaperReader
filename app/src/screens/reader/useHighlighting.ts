// 在 PDF 上劃線：選字之後的「劃線」、劃線模式（選字即劃）、「短評」工具、點劃線開短評、把這份材料的劃線畫在頁面上。
// 筆記的資料在 `useNotes.ts`；這裡是 PDF 與筆記之間的那一段（與 `useInk.ts` 之於筆跡相同）。

import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import type { Nav } from "../../nav";
import { t } from "../../strings";
import type { ToastData } from "../../ui/components";
import type { PdfView, SelectionCapture } from "./pdfView";
import type { NotesState } from "./useNotes";

const SELECTION_QUIET_MS = 650; // 選取停下這麼久才算選好（拖把手時不斷在變）
const HIGHLIGHT_MODE_QUIET_MS = 1500; // 劃線模式開著時選好就直接劃，多等一下，讓人拖把手把一個字延伸成一句

export function useHighlighting(o: {
  viewRef: RefObject<PdfView | null>;
  notes: NotesState;
  materialId: string | null;
  ready: boolean;
  /** 筆刷開著時不取選字。 */
  brushOn: boolean;
  /** 能不能劃線（原頁疊譯文上不能）。 */
  canMark: boolean;
  /** 這份是產生的譯本 PDF：重新產生過時，找不回原位置的劃線不畫。 */
  generated: boolean;
  nav: Nav;
  setToast: (t: string | ToastData) => void;
}) {
  const { viewRef, notes, materialId, ready, nav, setToast } = o;
  const [highlightMode, setHighlightMode] = useState(false);
  const [selection, setSelection] = useState<SelectionCapture | null>(null);
  // 選取還在變（拖開頭或結尾的把手）：選字的按鈕列先藏起來，停下來之後再出現在新的位置；
  // 避免手指放開在按鈕上、把選取延伸到頁尾
  const [dragging, setDragging] = useState(false);
  const shownRef = useRef(false);
  const [editId, setEditId] = useState<string | null>(null);
  /** 點一條劃線出的小選單：哪一條、對齊畫面上的哪裡。 */
  const [menu, setMenu] = useState<{ id: string; anchor: { left: number; top: number; right: number; bottom: number } } | null>(null);

  // ---------- 刪除與復原一次 ----------
  // 按「刪除」時先從畫面拿掉、出現「已刪除劃線 · 復原」（不自動消失）；提示「定了」（滑掉、點了別處、換成別的提示、
  // 離開這一篇、應用退到背景，共用提示元件的 `onSettle`）才真的從筆記檔刪。按「復原」就什麼都沒改。
  // 筆記檔沒有新的格式（刪除本來就有）；復原也不必重新加回去（重新加會換掉劃線的代號，連結就斷了）。
  // 每一條各自定：連刪兩條時，前一則提示被換掉、前一條馬上刪，後一條仍可以復原。
  const [pending, setPending] = useState<readonly string[]>([]);
  const notesRef = useRef(notes);
  useEffect(() => {
    notesRef.current = notes;
  });
  const settled = useRef(new Set<string>());
  /** 刪一條劃線（頁面上的小選單、短評面板、筆記窗格、短評窗格都用這個）：可以復原一次。 */
  const removeHighlight = useCallback(
    (id: string) => {
      settled.current.delete(id);
      setPending((cur) => (cur.includes(id) ? cur : [...cur, id]));
      const drop = () => setPending((cur) => cur.filter((x) => x !== id));
      setToast({
        message: t("reader.highlight.deleted"),
        keep: true,
        action: { label: t("common.undo"), run: () => {} },
        onSettle: (undone) => {
          // 每一條只定一次（提示元件保證一則只呼叫一次；這裡再擋一次重複的刪除）
          if (settled.current.has(id)) return;
          settled.current.add(id);
          if (undone) {
            drop();
            return;
          }
          void notesRef.current.remove(id).then((m) => {
            drop();
            if (m) setToast(t("reader.highlight.deleteFailed", { reason: m }));
          });
        },
      });
    },
    [setToast],
  );

  // 這份材料（原文或譯本 PDF）上的劃線畫在頁面上（等著刪的不畫）。
  // 產生的譯本 PDF 重新產生過、譯文改了的地方，舊劃線的引句對不上：列為找不回（不畫、不刪，引句與短評仍在筆記裡）
  const lostToast = useRef<string | null>(null);
  useEffect(() => {
    const view = viewRef.current;
    if (!view || !ready) return;
    const mine = (notes.view?.doc.highlights ?? []).filter((h) => h.material === materialId && !pending.includes(h.id));
    const draw = (lost: Set<string>) =>
      view.setHighlights(
        mine
          .filter((h) => !lost.has(h.id))
          .map((h) => ({
            id: h.id,
            n: h.page.index + 1,
            rects: h.rects,
            hasComment: h.comment.trim() !== "",
            comment: h.comment,
            quote: h.quote.exact,
          })),
      );
    if (!o.generated || mine.length === 0) {
      draw(new Set());
      return;
    }
    let cancelled = false;
    (async () => {
      const lost = new Set<string>();
      for (const h of mine) {
        const ok = await view.quoteStillAt(h.page.index, h.rects, h.quote.exact).catch(() => true);
        if (!ok) lost.add(h.id);
      }
      if (cancelled) return;
      draw(lost);
      const key = `${materialId}:${[...lost].sort().join(",")}`;
      if (lost.size > 0 && lostToast.current !== key) {
        lostToast.current = key;
        setToast(t("reader.highlight.lost", { n: lost.size }));
      }
    })();
    return () => {
      cancelled = true;
    };
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [notes.view, ready, materialId, o.generated, pending]);

  /**
   * 劃一條線。`withComment` 劃好後打開短評面板；`comment` 給了就連同短評一起存（快速翻譯的「劃線並寫進短評」），
   * 提示說已寫進短評、可以復原。
   */
  const create = useCallback(
    async (cap: SelectionCapture, withComment: boolean, comment = "") => {
      const view = viewRef.current;
      if (!view || !materialId) return;
      const r = await notes.addHighlight({
        material: materialId,
        page_index: cap.pageIndex,
        page_label: null,
        rects: cap.rects,
        position: { start: cap.start, end: cap.end, basis: "textlayer-chars-v1" },
        quote: { exact: cap.exact, prefix: cap.prefix, suffix: cap.suffix },
        fingerprint: view.fingerprint,
        comment,
      });
      window.getSelection()?.removeAllRanges();
      setSelection(null);
      if (r.message) {
        setToast(r.message);
        return;
      }
      if (withComment && r.added) {
        setEditId(r.added.highlight.id);
        nav.push({ name: "sheet", kind: "highlightEdit" });
      } else if (r.added) {
        // 劃錯了（尤其劃線模式選字即劃）可以馬上復原：剛加的這條直接刪掉
        const id = r.added.highlight.id;
        setToast({
          message: t(comment.trim() !== "" ? "quick.savedAsComment" : "reader.highlight.saved"),
          keep: true,
          action: {
            label: t("common.undo"),
            run: () =>
              void notesRef.current.remove(id).then((m) => m && setToast(t("reader.highlight.deleteFailed", { reason: m }))),
          },
        });
      }
    },
    [viewRef, notes, materialId, nav, setToast],
  );

  /**
   * 整段短評：畫面上一點（選取的第一行、或點的那一段）所在的整段選起來，存成一條劃線、打開短評面板。
   * 段落的範圍用現有的分段（譯文疊加檔的段落，沒有就用這一頁的版面分析）；不加新的資料格式。
   */
  const paragraphComment = useCallback(
    async (at: { x: number; y: number } | { page: number; bbox: [number, number, number, number] }) => {
      const view = viewRef.current;
      if (!view) return;
      const box = "bbox" in at ? at : await view.paragraphAt(at.x, at.y);
      const cap = box ? view.captureBox(box.page, box.bbox) : null;
      if (!cap) {
        setToast(t("reader.highlight.noParagraph"));
        return;
      }
      await create(cap, true);
    },
    [viewRef, create, setToast],
  );
  const createRef = useRef(create);
  const modeRef = useRef({ highlightMode, canMark: o.canMark });
  useEffect(() => {
    createRef.current = create;
    modeRef.current = { highlightMode, canMark: o.canMark };
  });

  // 長按劃線開小選單時，瀏覽器同時開始選字：那一下的選取不算（不出「劃線」鈕，劃線模式也不再劃一條）
  useEffect(() => {
    shownRef.current = selection !== null;
  });
  const suppressUntil = useRef(0);
  const suppressSelection = (ms: number) => {
    suppressUntil.current = Date.now() + ms;
    window.getSelection()?.removeAllRanges();
    setSelection(null);
  };

  // 選字：瀏覽器原生的選取；選好之後（停下一會兒）出現「劃線」，劃線模式開著就直接劃
  useEffect(() => {
    if (!ready) return;
    let timer = 0;
    const onSel = () => {
      window.clearTimeout(timer);
      if (Date.now() < suppressUntil.current) {
        const sel = window.getSelection();
        if (sel && !sel.isCollapsed) sel.removeAllRanges();
        return;
      }
      // 按鈕列正顯示著、選取又變了：多半是在拖把手，先藏起來
      if (shownRef.current) setDragging(true);
      timer = window.setTimeout(
        () => {
          setDragging(false);
          const cap = o.brushOn ? null : (viewRef.current?.captureSelection() ?? null);
          setSelection(cap);
          if (cap && modeRef.current.highlightMode && modeRef.current.canMark) void createRef.current(cap, false);
        },
        modeRef.current.highlightMode ? HIGHLIGHT_MODE_QUIET_MS : SELECTION_QUIET_MS,
      );
    };
    document.addEventListener("selectionchange", onSel);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("selectionchange", onSel);
    };
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, o.brushOn]);

  /**
   * 點到一條劃線：在它旁邊出小選單（刪除、短評），同類閱讀器的慣例。點到的不是劃線交回 `false`。
   * 原頁疊譯文開著時劃線是蓋住的，不理。
   */
  const tapHighlight = (x: number, y: number): boolean => {
    const id = highlightIdAt(x, y);
    if (!id) return false;
    openMenu(id, x, y);
    return true;
  };
  /** 畫面上這一點落在哪一條劃線上（原頁疊譯文開著時劃線是蓋住的，不算）。 */
  const highlightIdAt = (x: number, y: number): string | null => (o.canMark ? (viewRef.current?.highlightAt(x, y) ?? null) : null);
  const openMenu = (id: string, x: number, y: number) => {
    const anchor = viewRef.current?.highlightRect(id) ?? { left: x, top: y, right: x, bottom: y };
    setMenu({ id, anchor });
    nav.push({ name: "sheet", kind: "hlPop" });
  };
  /**
   * 長按一條劃線：一律開小選單，不受「點內容」開關影響（劃線是使用者自己做的東西，要找得到移除）。
   * 瀏覽器同時開始的選字取消掉。
   */
  const longPressHighlight = (id: string, x: number, y: number) => {
    suppressSelection(1500);
    window.setTimeout(() => window.getSelection()?.removeAllRanges(), 120);
    openMenu(id, x, y);
  };

  /** 小選單的「短評」：換成短評面板（小選單那一層先退掉）。 */
  const openComment = async (id: string) => {
    await nav.back();
    setEditId(id);
    nav.push({ name: "sheet", kind: "highlightEdit" });
  };
  /** 小選單的「刪除」。 */
  const deleteFromMenu = async (id: string) => {
    await nav.back();
    removeHighlight(id);
  };

  /** 「短評」工具：有選字就劃線並寫短評（選字工具列的「短評」同）；沒有選字就用這一頁頁首附近的第一行當依據。 */
  const commentTool = () => {
    if (!o.canMark) {
      setToast(t("reader.highlight.translationNote"));
      return;
    }
    const view = viewRef.current;
    const cap = selection ?? view?.captureSelection() ?? view?.firstVisibleLine() ?? null;
    if (!cap) {
      setToast(t("reader.highlight.noText"));
      return;
    }
    void create(cap, true);
  };

  const highlightTool = () => {
    if (!o.canMark) {
      setToast(t("reader.highlight.translationNote"));
      return;
    }
    setToast(highlightMode ? t("reader.highlightMode.off") : t("reader.highlightMode.on"));
    setHighlightMode(!highlightMode);
  };

  /** 短評窗格的「編輯」：打開短評面板。 */
  const editComment = (id: string) => {
    setEditId(id);
    nav.push({ name: "sheet", kind: "highlightEdit" });
  };

  const editing = editId && !pending.includes(editId) ? (notes.view?.doc.highlights.find((h) => h.id === editId) ?? null) : null;
  const menuHighlight =
    menu && !pending.includes(menu.id) ? (notes.view?.doc.highlights.find((h) => h.id === menu.id) ?? null) : null;

  return {
    highlightMode,
    selection,
    /** 選取還在變（拖把手中）：選字的按鈕列藏起來。 */
    dragging,
    setSelection,
    create,
    paragraphComment,
    tapHighlight,
    highlightIdAt,
    longPressHighlight,
    commentTool,
    highlightTool,
    editing,
    editComment,
    menu: menuHighlight && menu ? { highlight: menuHighlight, anchor: menu.anchor } : null,
    openComment,
    deleteFromMenu,
    removeHighlight,
    /** 等著刪（還能復原）的：筆記窗格不列。 */
    pending,
  };
}
