// 產生（譯本、重點筆記）共用的流程：讀這篇與翻譯設定、從設定回來時重讀、接手進行中的工作、
// 讀原文分段、輪詢進度、取消、出錯的說法與「前往設定」。工作本身在外殼的背景進行，離開畫面也會做完。
// 兩邊不同的規則（預設選哪個服務、同意的條件、做完之後做什麼）由各自的畫面給。

import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  AppError,
  kindOf,
  type ErrorKind,
  type FallbackOffer,
  type JobState,
  type Material,
  type PaperDetail,
  type SegmentInput,
  type TranslateSettingsView,
} from "../../api";
import { sourcePdf } from "../../api/types";
import { displayTitle } from "../../format";
import { useAlive, usePoll } from "../../hooks";
import type { Nav } from "../../nav";
import { withPdf } from "../../pdf/pdfSource";
import { enqueuePdf } from "../../pdfQueue";
import { holdPerPaper } from "../../perPaperApp";
import { errorReason, t, type StringKey } from "../../strings";
import { extractSegments } from "../../translate/extract";

export interface GenError {
  message: string;
  detail?: string;
  kind?: ErrorKind;
}

export interface Analysis {
  n: number;
  total: number;
  /** `text`：讀文字層；`model`：用版面模型分段（只有翻譯、模型已下載時）。 */
  step: "text" | "model";
  model: boolean;
}

interface JobLike {
  paperId: string;
  state: JobState;
  fallback: FallbackOffer | null;
}

/** 要到設定裡改才能解決的錯誤（給一顆直接帶過去的按鈕）。 */
const FIX_IN_SETTINGS: ReadonlySet<ErrorKind> = new Set<ErrorKind>([
  "TranslateAuth",
  "TranslateConfig",
  "TranslateNoModel",
  "TranslateLanguage",
  "NotOnWifi",
]);

export function useGenerateJob<S extends JobLike>(opts: {
  nav: Nav;
  paperId: string;
  /** 外殼的工作狀態與取消。 */
  status: () => Promise<S | null>;
  cancel: () => Promise<unknown>;
  /** 讀到（或重讀）設定之後。 */
  onConfig?: (c: TranslateSettingsView) => void;
  /** 第一次讀到這篇之後。 */
  onLoaded?: (d: PaperDetail) => Promise<void>;
  /** 輪詢到工作不在跑了（做完、停下、取消；`null` 是外殼沒有工作）。 */
  onSettled?: (s: S | null) => Promise<void> | void;
}) {
  const { nav, paperId } = opts;
  const alive = useAlive();
  const cb = useRef(opts);
  useEffect(() => {
    cb.current = opts;
  });
  const stopAnalysis = useRef(false);
  const busy = useRef(false);
  const [detail, setDetail] = useState<PaperDetail | null>(null);
  const [cfg, setCfg] = useState<TranslateSettingsView | null>(null);
  const [phase, setPhase] = useState<"panel" | "analyzing" | "progress">("panel");
  /** 讀原文、分段的進度。`model`：這次會用版面模型（讀完文字後多一步 `step: "model"`）。 */
  const [analysis, setAnalysis] = useState<Analysis>({ n: 0, total: 0, step: "text", model: false });
  const [job, setJob] = useState<S | null>(null);
  /** 使用者對這次停下來時的備用服務說了「不要」。 */
  const [declined, setDeclined] = useState(false);
  const [error, setError] = useState<GenError | null>(null);

  useEffect(
    () => () => {
      stopAnalysis.current = true;
    },
    [],
  );

  const loadCfg = useCallback(async () => {
    const c = await api.translateSettings();
    if (alive.current) {
      setCfg(c);
      cb.current.onConfig?.(c);
    }
    return c;
  }, [alive]);

  useEffect(() => {
    (async () => {
      try {
        const [d, , s] = await Promise.all([api.getPaper(paperId), loadCfg(), cb.current.status()]);
        if (!alive.current) return;
        setDetail(d);
        // 正在做，或停下來等「要不要換備用服務」的回答（離開畫面再回來也要看得到那個問題）
        if (s && s.paperId === paperId && (s.state === "running" || (s.state === "stopped" && s.fallback))) {
          setJob(s);
          setPhase("progress");
        }
        await cb.current.onLoaded?.(d);
      } catch {
        // 讀不到就維持畫面現狀；開始時會再碰到同樣的錯並說明
      }
    })();
  }, [paperId, loadCfg, alive]);

  // 從設定回來（改了服務或金鑰）時重新讀設定
  const onTop = nav.top.name === "generate";
  useEffect(() => {
    if (onTop) void loadCfg().catch(() => {});
  }, [onTop, loadCfg]);

  // 進行中：每隔一下問一次進度
  usePoll(
    phase === "progress" && job?.state === "running",
    async () => {
      const s = await cb.current.status();
      if (!alive.current) return false;
      if (s) setJob(s);
      if (s && s.state === "running") return true;
      await cb.current.onSettled?.(s);
      return false;
    },
    700,
    300,
  );

  /** 一次只做一件（連按不會開始兩次）。 */
  const once = useCallback(async (f: () => Promise<void>) => {
    if (busy.current) return;
    busy.current = true;
    try {
      await f();
    } finally {
      busy.current = false;
    }
  }, []);

  /**
   * 讀這篇的原文 PDF、分段（PDF.js 讀文字層，`layout.ts` 分段）。按了取消、離開畫面交回 `null`；
   * 讀不出任何字時說明 `noText`、交回 `null`。`recordPages`：先把頁數記進核心（核心檢查頁碼要在頁數之內）。
   * `layoutModel`：版面模型已下載時用它分段（只有翻譯；重點筆記照規則）。
   */
  const analyze = async (
    noText: StringKey,
    recordPages = false,
    layoutModel = false,
  ): Promise<{ source: Material; segments: SegmentInput[]; modelPages: number[] } | null> => {
    const source = detail ? sourcePdf(detail.paper.materials) : undefined;
    if (!source) throw new AppError("NotFound", "no source");
    stopAnalysis.current = false;
    // 模型沒下載（或這個建置沒有）就不多那一步
    const lm = layoutModel ? await api.layoutStatus().catch(() => null) : null;
    const model = !!lm?.supported && lm.downloaded;
    setPhase("analyzing");
    setAnalysis({ n: 0, total: 0, step: "text", model });
    const stopped = () => stopAnalysis.current;
    const progress = (step: Analysis["step"]) => (n: number, total: number) =>
      alive.current && setAnalysis({ n, total, step, model });
    // 大檔分段讀取：開檔途中按停止也能停（精讀與首頁的修正）。開 PDF 排進共用的佇列（優先），抽段落的期間
    // 逐篇處理讓路（產生畫面開著時不同時開另一份 PDF）
    const releasePerPaper = holdPerPaper();
    const extracted = await enqueuePdf("urgent", () =>
      withPdf(
        paperId,
        source.id,
        async (pdf, src) => {
          if (recordPages && source.pages !== pdf.numPages) await api.setMaterialPages(paperId, source.id, pdf.numPages);
          return extractSegments(src, pdf, progress("text"), stopped, model ? { onPage: progress("model") } : undefined);
        },
        stopped,
      ),
    ).finally(releasePerPaper);
    if (extracted === null || !alive.current) {
      setPhase("panel");
      return null;
    }
    const { segments, modelPages } = extracted;
    if (segments.length === 0) {
      setPhase("panel");
      setError({ message: t(noText) });
      return null;
    }
    return { source, segments, modelPages };
  };

  const cancel = () => {
    stopAnalysis.current = true;
    void cb.current.cancel().catch(() => {});
  };

  /** 出錯：表面一句 `message`，原因放詳情；要到設定裡改的另給按鈕（`fixAction`）。 */
  const fail = (message: StringKey, e: unknown) =>
    setError({ message: t(message), detail: errorReason(kindOf(e)), kind: kindOf(e) });

  /** 「前往設定」：直接到那個服務；不知道是哪個就到服務清單。 */
  const toService = (id: string | null) =>
    id ? nav.push({ name: "translateService", id }) : nav.push({ name: "translateServices" });
  const fixAction = (kind: ErrorKind | null | undefined, id: string | null) =>
    kind && FIX_IN_SETTINGS.has(kind) ? { onAction: () => toService(id), actionLabel: t("common.toSettings") } : {};

  const title = detail ? displayTitle(detail.paper.bib, detail.paper.short_name ?? undefined) : null;

  return {
    alive,
    detail,
    setDetail,
    cfg,
    loadCfg,
    title,
    phase,
    setPhase,
    analysis,
    job,
    setJob,
    declined,
    setDeclined,
    error,
    setError,
    once,
    analyze,
    cancel,
    fail,
    toService,
    fixAction,
  };
}
