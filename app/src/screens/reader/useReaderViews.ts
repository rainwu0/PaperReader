// 閱讀畫面的幾種看法與段落：原文、譯本 PDF、原頁疊譯文、譯文之間怎麼切換、記住上次選的哪一份，
// 以及譯文疊加檔的段落在 PDF 上的方框（點一段的小框、疊譯文暫看原文、從譯文回到同一段）。
// 譯文疊加檔在這裡只讀一份（`useOverlayDoc`），譯文、疊譯文、小框、更新譯本 PDF 共用。
// 材料的開啟、捲動、縮放仍由閱讀畫面與 `PdfView` 管；這裡只決定去哪裡。

import { useEffect, useRef, useState, type RefObject } from "react";
import type { Material } from "../../api";
import { FORM_OVERLAY, isGeneratedPdf, ROLE_TRANSLATION, sourcePdf } from "../../api/types";
import { hasPageRegions, pageRegionsFor } from "../../ext/pageRegions";
import type { RegionCard } from "../../ext/types";
import type { HintId } from "../../hints";
import { hintPending } from "../../hints";
import { isBool, useBackOnTop, useLocalPref } from "../../hooks";
import type { Nav, SheetKind } from "../../nav";
import { t } from "../../strings";
import { MATERIALS_CHANGED } from "../../translate/autoBuild";
import type { ToastData } from "../../ui/components";
import type { PdfView, SegmentBox } from "./pdfView";
import type { ViewOption } from "./ReaderSheets";
import { useOverlayDoc, useRebuild } from "./SegmentParts";
import { nearestSegment, newestFirst, preferredTranslation, shownText, type Seg } from "./segments";
import { relativeDay, timeOfDay } from "../../format";

// 這台裝置上記住的選擇：點內容帶出功能、每篇上次看的譯本
const TAP_KEY = "pr-tap-content";
const PICK_KEY = "pr-last-translation";
const isPicks = (v: unknown): v is Record<string, string> =>
  !!v && typeof v === "object" && !Array.isArray(v) && Object.values(v).every((x) => typeof x === "string");

/** 要去的一段（頁序從 0 起、PDF 頁面座標）。 */
export type SegTarget = Pick<Seg, "page" | "bbox">;

export interface PopState {
  /** 點的那一段（譯文疊加檔的段落代號）；出的是引用卡片時是空字串。 */
  id: string;
  anchor: { left: number; top: number; right: number; bottom: number };
  editing: boolean;
  /** 頁面上的可點區域交出的卡片。 */
  card?: RegionCard;
}

export function useReaderViews(o: {
  paperId: string;
  nav: Nav;
  sheets: SheetKind[];
  materials: Material[];
  materialId: string | null;
  ready: boolean;
  viewRef: RefObject<PdfView | null>;
  /** 換到另一份材料，停在同一處（同版面的 PDF 之間）。 */
  switchTo: (id: string | undefined) => void;
  /** 換到另一份材料並去某一段或某一頁。 */
  openAt: (id: string, target: { segment?: SegTarget; page?: number }) => void;
  setToast: (t: string | ToastData) => void;
  offerHint: (id: HintId) => void;
  /** 有重點筆記：沒有譯本時「譯本」改開它。 */
  hasDigest: boolean;
  openDigest: () => void;
}) {
  const { paperId, nav, sheets, materials, materialId, ready, viewRef, setToast } = o;
  const source = sourcePdf(materials);
  // 譯本 PDF：產生的與自己加入的；整篇重翻會另做一份，最新的排前面
  const translationPdfs = newestFirst(materials.filter((m) => m.role === ROLE_TRANSLATION && m.form === "pdf"));
  const onSource = materialId !== null && materialId === source?.id;
  const onTranslationPdf = materialId !== null && translationPdfs.some((m) => m.id === materialId);
  const current = materials.find((m) => m.id === materialId);

  // 逐段翻譯的譯本（譯文疊加檔）：在看由它產生的譯本 PDF 時用那一份，否則用最後一份
  const overlays = materials.filter((m) => m.role === ROLE_TRANSLATION && m.form === FORM_OVERLAY);
  const fromOverlay = current && isGeneratedPdf(current) ? current.provenance?.generated_from : undefined;
  const overlayMat = overlays.find((m) => m.id === fromOverlay) ?? overlays.at(-1);
  const ov = useOverlayDoc(paperId, overlayMat?.id);
  const doc = ov.doc;
  // 從產生畫面回來（接著翻了一些段落）、翻完產生了譯本 PDF：重讀譯文
  useBackOnTop(nav, "reader", () => void ov.reload());
  const reloadOverlay = ov.reload;
  useEffect(() => {
    const on = (e: Event) => {
      if ((e as CustomEvent<{ paperId: string }>).detail.paperId === paperId) void reloadOverlay();
    };
    window.addEventListener(MATERIALS_CHANGED, on);
    return () => window.removeEventListener(MATERIALS_CHANGED, on);
  }, [paperId, reloadOverlay]);
  const rebuild = useRebuild(paperId, overlayMat?.id, doc, materials);
  const [rebuildDismissed, setRebuildDismissed] = useState(false);
  // 這份 PDF 的版面和譯文的原文相同（原文本身，或由這份譯文產生的譯本 PDF）：段落的方框對得上
  const sameLayout =
    !!doc && !!overlayMat && (materialId === doc.source_material || (!!fromOverlay && fromOverlay === overlayMat.id));
  const [overlayOn, setOverlayOn] = useState(false);
  // 「原頁疊譯文」只在原文上；疊加開著時劃線與短評停用
  const overlayActive = overlayOn && onSource;
  // 點內容帶出功能（點段落看對照、點劃線開短評、點疊譯文暫看原文）；關掉時點內容只做閱讀本來的事
  const [tapContent, setTapContent] = useLocalPref(TAP_KEY, true, isBool);
  // 每篇上次在選單裡選的譯本（材料代號或 `overlay`）：工具箱「譯本」按一下直接切過去
  const [picks, setPicks] = useLocalPref(PICK_KEY, {}, isPicks);
  const preferred = preferredTranslation(
    {
      generated: translationPdfs.filter(isGeneratedPdf).map((m) => m.id),
      added: translationPdfs.filter((m) => !isGeneratedPdf(m)).map((m) => m.id),
      overlay: !!overlayMat,
    },
    picks[paperId] ?? null,
  );
  const remember = (pick: string) => setPicks({ ...picks, [paperId]: pick });

  // ---------- 方框：點一段、疊譯文 ----------

  useEffect(() => {
    viewRef.current?.setSegments(ready && sameLayout && doc ? doc.segments : null);
  }, [viewRef, ready, sameLayout, doc, materialId]);
  useEffect(() => {
    viewRef.current?.setOverlay(ready && overlayActive);
  }, [viewRef, ready, overlayActive, materialId]);
  // 打開疊譯文時還沒有翻好的段落、或譯本讀不出來：說一聲
  useEffect(() => {
    if (!overlayActive) return;
    if (ov.error && !doc) {
      setOverlayOn(false);
      setToast(ov.error);
    } else if (doc && !doc.segments.some((s) => !shownText(s).untranslated)) setToast(t("reader.overlay.noneDone"));
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [overlayActive, doc, ov.error]);

  const segById = (id: string | null) => (id ? doc?.segments.find((s) => s.id === id) : undefined);
  /** 去某一段：同版面的 PDF 捲到那一段並框出來，其他的到那一頁。 */
  const showSeg = (s: SegTarget) => {
    if (sameLayout) viewRef.current?.showSegment(s.page, s.bbox);
    else viewRef.current?.goToPage(s.page + 1);
  };

  // ---------- PDF 上點一段的小框（A11） ----------
  // 導覽堆疊裡的一層：返回鍵、點框外、再點一次、捲動都收起（正在改的捲動不收）

  const [pop, setPop] = useState<PopState | null>(null);
  const popLayer = sheets.includes("segPop");
  useEffect(() => {
    if (!popLayer) setPop(null);
  }, [popLayer]);
  const popSeg = pop && popLayer && !pop.card ? segById(pop.id) : undefined;

  // ---------- 頁面上的可點區域（引用卡片）：只在原文 PDF 上 ----------

  useEffect(() => {
    const view = viewRef.current;
    if (!view || !ready) return;
    const material = materialId;
    view.setRegionSource(
      onSource && material && hasPageRegions()
        ? (page, input, signal) =>
            pageRegionsFor(
              { paperId, materialId: material, page, view: input.view, items: input.items },
              signal,
            )
        : null,
    );
  }, [viewRef, ready, onSource, materialId, paperId]);
  /** 點在引用標記的區域上：出卡片（原頁疊譯文開著時不作用）。處理了交回 `true`。 */
  const tapRegion = (x: number, y: number): boolean => {
    if (overlayActive || !onSource) return false;
    const hit = viewRef.current?.regionAt(x, y);
    if (!hit) return false;
    setPop({ id: "", anchor: hit.rect, editing: false, card: hit.region.card });
    nav.push({ name: "sheet", kind: "segPop" });
    o.offerHint("card");
    return true;
  };
  const cardProps = pop && popLayer && pop.card ? { card: pop.card, anchor: pop.anchor } : null;
  const openPop = (seg: SegmentBox) => {
    const anchor = viewRef.current?.segmentRect(seg.page, seg.bbox);
    if (!anchor) return;
    setPop({ id: seg.id, anchor, editing: false });
    nav.push({ name: "sheet", kind: "segPop" });
    o.offerHint("paragraph");
  };
  /** PDF 捲動或縮放了：小框收起（正在改的不收）。 */
  const onMoved = () => {
    if (pop && popLayer && !pop.editing) void nav.back();
  };
  /**
   * 點頁面（單擊），依序：小框開著就收起；「點內容」關著只顯示或隱藏頂列；點在劃線上
   * （`contentFirst`）開短評；點在引用標記上出卡片；點疊譯文暫看原文、點一段出小框。處理了交回 `true`。
   */
  const tap = (x: number, y: number, contentFirst: () => boolean): boolean => {
    if (popLayer) {
      void nav.back();
      return true;
    }
    if (!tapContent) return false;
    if (contentFirst()) return true;
    if (tapRegion(x, y)) return true;
    const seg = viewRef.current?.segmentAt(x, y);
    if (!seg) return false;
    if (overlayActive && !shownText(seg).untranslated) viewRef.current?.peek(seg.id);
    else openPop(seg);
    return true;
  };
  const savePop = async (text: string) => {
    if (!pop) return null;
    const message = await ov.edit(pop.id, text);
    if (message) return message;
    await nav.back();
    // 譯本 PDF 還是改之前的：說一次，可以直接更新（A5）
    setToast(
      rebuild.pdf
        ? { message: t("reader.text.behind"), action: { label: t("reader.text.rebuild"), run: rebuild.run } }
        : t("reader.text.saved"),
    );
    return null;
  };
  const popProps = popSeg && pop
    ? {
        seg: popSeg,
        show: (onSource ? "target" : "source") as "target" | "source",
        anchor: pop.anchor,
        editing: pop.editing,
        onEdit: () => setPop({ ...pop, editing: true }),
        onSave: savePop,
        onCancelEdit: () => setPop({ ...pop, editing: false }),
      }
    : null;

  // 更新譯本 PDF 做完了（不論從哪裡按的）：說一聲
  const buildPhase = rebuild.build?.phase;
  const lastBuild = useRef(buildPhase);
  useEffect(() => {
    if (lastBuild.current === "working" && buildPhase === "done") setToast(t("reader.text.rebuilt"));
    if (lastBuild.current === "working" && buildPhase === "failed")
      setToast({ message: t("reader.text.rebuildFailed"), action: { label: t("common.retry"), run: rebuild.run } });
    lastBuild.current = buildPhase;
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [buildPhase]);

  // ---------- 譯文（A1、A6、A15） ----------
  // 從 PDF 上正在讀的那一段開始；回來時停在譯文裡讀到的那一段。只是打開再關掉，PDF 不動

  const textStart = useRef<string | null>(null);
  const reading = useRef<string | null>(null);
  const leavingToLayout = useRef(false);
  const openText = () => {
    if (!overlayMat) {
      setToast({
        message: t("reader.noOverlay"),
        action: { label: t("reader.tool.generate"), run: () => nav.push({ name: "generate", paperId }) },
      });
      return;
    }
    const pt = viewRef.current?.readingPoint();
    const start = pt && doc ? nearestSegment(doc.segments, pt.page, pt.x, pt.y) : null;
    textStart.current = start?.id ?? null;
    reading.current = textStart.current;
    nav.push({ name: "sheet", kind: "compare" });
  };
  const textOpen = sheets.includes("compare");
  const wasText = useRef(false);
  useEffect(() => {
    if (wasText.current && !textOpen && !leavingToLayout.current && reading.current !== textStart.current) {
      const s = segById(reading.current);
      if (s) showSeg(s);
    }
    leavingToLayout.current = false;
    wasText.current = textOpen;
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [textOpen]);

  /** 譯文的「看版面」：關掉譯文（連同檢查、選單那幾層），到譯本（上次看的那一份）的這一段（J4）。 */
  const layoutFromText = async (id: string) => {
    const s = segById(id);
    leavingToLayout.current = true;
    const layers = sheets.length - sheets.indexOf("compare");
    for (let i = 0; i < layers; i++) await nav.back();
    if (!s) return;
    const want = preferred?.kind === "pdf" ? preferred.id : source?.id;
    setOverlayOn(preferred?.kind === "overlay");
    if (!want || want === materialId) {
      showSeg(s);
      return;
    }
    const target = materials.find((m) => m.id === want);
    const aligned = want === doc?.source_material || (!!target && target.provenance?.generated_from === overlayMat?.id);
    o.openAt(want, aligned ? { segment: s } : { page: s.page + 1 });
  };

  const textProps = overlayMat
    ? {
        overlayId: overlayMat.id,
        overlay: ov,
        startAt: textStart.current,
        onReading: (id: string) => (reading.current = id),
        onLayout: (id: string) => void layoutFromText(id),
        rebuild,
        rebuildDismissed,
        onDismissRebuild: () => setRebuildDismissed(true),
      }
    : null;

  // ---------- 原文、譯本、疊譯文之間 ----------

  function showOverlay() {
    if (!overlayMat) return;
    if (!onSource) o.switchTo(source?.id);
    setOverlayOn(true);
    if (hintPending("overlay")) o.offerHint("overlay");
    else setToast(t("reader.overlay.on"));
  }
  const showSource = () => {
    setOverlayOn(false);
    o.switchTo(source?.id);
  };
  const showPdf = (id: string) => {
    setOverlayOn(false);
    o.switchTo(id);
  };

  /**
   * 工具箱「譯本」按一下：直接切到上次選的那一份（第一次：產生的 → 自己加入的 → 原頁疊譯文，A14）。
   * 沒有譯本但有重點筆記就打開重點筆記；什麼都沒有就說怎麼產生。換一份由長按或小箭頭的選單。
   */
  function openTranslation() {
    if (preferred?.kind === "pdf") showPdf(preferred.id);
    else if (preferred?.kind === "overlay") showOverlay();
    else if (o.hasDigest) o.openDigest();
    else
      setToast({
        message: t("reader.noTranslation"),
        action: { label: t("reader.tool.generate"), run: () => nav.push({ name: "generate", paperId }) },
      });
  }
  /** 頂列的互換鈕：原文 ⇄ 譯本（上次選的那一份），停在同一處。 */
  const onTranslation = onTranslationPdf || overlayActive;
  const swap = () => (onTranslation ? showSource() : openTranslation());

  /** 換一份的選單（工具箱「譯本」長按或小箭頭）：選了哪一份譯本就記住，下次按一下直接到它。 */
  const viewOptions = (pick: (run: () => void) => () => void): ViewOption[] => {
    const out: ViewOption[] = [];
    if (source) out.push({ id: "source", label: t("reader.tool.source"), current: onSource && !overlayActive, run: pick(showSource) });
    // 每一份寫上做出來的時間（和文獻資訊的材料清單同一個說法），幾份產生的譯本分得出哪份是新的
    for (const m of translationPdfs) {
      const time = m.provenance?.time;
      const made = time ? t("info.material.made", { when: relativeDay(time), time: timeOfDay(time) }) : "";
      out.push({
        id: m.id,
        label: [t(isGeneratedPdf(m) ? "reader.view.generated" : "reader.view.added"), made].filter(Boolean).join("・"),
        current: materialId === m.id,
        run: pick(() => {
          remember(m.id);
          showPdf(m.id);
        }),
      });
    }
    if (overlayMat) {
      out.push({
        id: "overlay",
        label: t("reader.view.overlay"),
        current: overlayActive,
        run: pick(() => {
          remember("overlay");
          showOverlay();
        }),
      });
      out.push({ id: "text", label: t("reader.view.text"), current: false, run: pick(openText) });
    }
    return out;
  };

  return {
    onSource,
    onTranslation,
    overlayMat,
    overlayOn,
    setOverlayOn,
    overlayActive,
    preferred,
    tapContent,
    setTapContent,
    openTranslation,
    showSource,
    showOverlay,
    swap,
    viewOptions,
    openText,
    textOpen,
    textProps,
    popProps,
    cardProps,
    tap,
    onMoved,
  };
}
