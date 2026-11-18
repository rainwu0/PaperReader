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
  const [editId, setEditId] = useState<string | null>(null);

  // 這份材料（原文或譯本 PDF）上的劃線畫在頁面上。
  // 產生的譯本 PDF 重新產生過、譯文改了的地方，舊劃線的引句對不上：列為找不回（不畫、不刪，引句與短評仍在筆記裡）
  const lostToast = useRef<string | null>(null);
  useEffect(() => {
    const view = viewRef.current;
    if (!view || !ready) return;
    const mine = (notes.view?.doc.highlights ?? []).filter((h) => h.material === materialId);
    const draw = (lost: Set<string>) =>
      view.setHighlights(
        mine
          .filter((h) => !lost.has(h.id))
          .map((h) => ({ id: h.id, n: h.page.index + 1, rects: h.rects, hasComment: h.comment.trim() !== "" })),
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
  }, [notes.view, ready, materialId, o.generated]);

  const create = useCallback(
    async (cap: SelectionCapture, withComment: boolean) => {
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
        comment: "",
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
      } else {
        setToast(t("reader.highlight.saved"));
      }
    },
    [viewRef, notes, materialId, nav, setToast],
  );
  const createRef = useRef(create);
  const modeRef = useRef({ highlightMode, canMark: o.canMark });
  useEffect(() => {
    createRef.current = create;
    modeRef.current = { highlightMode, canMark: o.canMark };
  });

  // 選字：瀏覽器原生的選取；選好之後（停下一會兒）出現「劃線」，劃線模式開著就直接劃
  useEffect(() => {
    if (!ready) return;
    let timer = 0;
    const onSel = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(
        () => {
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

  /** 點到一條劃線：開它的短評。點到的不是劃線交回 `false`。 */
  const tapHighlight = (x: number, y: number): boolean => {
    const id = o.canMark ? viewRef.current?.highlightAt(x, y) : null;
    if (!id) return false;
    setEditId(id);
    nav.push({ name: "sheet", kind: "highlightEdit" });
    return true;
  };

  /** 「短評」工具：有選字就劃線並寫短評；沒有選字就用這一頁頁首附近的第一行當依據。 */
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

  const editing = editId ? (notes.view?.doc.highlights.find((h) => h.id === editId) ?? null) : null;

  return { highlightMode, selection, setSelection, create, tapHighlight, commentTool, highlightTool, editing };
}
