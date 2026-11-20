// 產生譯本：一個翻譯面板（原文與目標語言、翻譯服務、提示詞預設組、第二輪潤飾、開始），文字要送出裝置時
// 在同一個面板上說送到哪裡、要你同意→ 進度。做法參考沉浸式翻譯的翻譯面板。
// 最上面切換「譯本」與「重點筆記」；重點筆記的面板在 `DigestGenerate.tsx`。
// 翻譯本身在外殼的背景工作裡進行（可以離開這個畫面）；這個畫面只負責開始、顯示進度、取消、接著翻。
// 主要服務停擺時，進度畫面先問要不要改用依序的下一個服務；「送出前不再詢問」不涵蓋這一步。
// 說明與結果是導覽堆疊裡的面板（`nav.ts`），返回鍵先關掉它。
// 選了離線的 Firefox 翻譯而模型還沒下載時，主按鈕是「下載模型並翻譯」：先問（同設定裡的下載），下載完自動開始；
// 下載途中離開面板、換了服務或停止下載，就不自動開始。
// 和產生重點筆記共用的流程在 `useGenerateJob.ts`、`ServicePanel.tsx`、`JobProgress.tsx`。

import { type ReactNode, useEffect, useState, useSyncExternalStore } from "react";
import { FORM_OVERLAY, isGeneratedPdf, ROLE_TRANSLATION } from "../../api/types";
import { api, AppError, type JobStatus, type OverlayFile, type PaperDetail, type TranslateServiceView } from "../../api";
import type { Nav, SheetKind } from "../../nav";
import { errorReason, t, type StringKey } from "../../strings";
import { cancelBuild, expectTranslation, getBuild, startBuild, subscribeBuild } from "../../translate/autoBuild";
import { Button, CheckRow, Details, ErrorNotice, InfoTip, KindNotice, Screen, Segmented } from "../../ui/components";
import { useAddTranslation } from "../add/useAddTranslation";
import { engineSummary, ModelAsk, ModelProgress, useOnDevice } from "./localEngine";
import { DigestGenerate } from "./DigestGenerate";
import { engineOf, segmentServiceName, serviceName, servicesUsed, useEngines } from "./engines";
import { AnalyzingScreen, FallbackAsk, ProgressBar, UsageDetails } from "./JobProgress";
import { PresetPicker, ServicePanel } from "./ServicePanel";
import {
  defaultService,
  isInsecureRemote,
  menuServices,
  missingSetting,
  resumeNoteKey,
  serviceModel,
  startButton,
  supportsTarget,
} from "./translateConfig";
import { useGenerateJob } from "./useGenerateJob";

/** 目標語言的選單（實際列出的再依服務的引擎申報過濾）。 */
const TARGETS = ["zh-TW", "zh-CN", "en", "ja", "ko", "de", "fr", "es", "ru"] as const;
/** 原文語言：空白是讓服務自己判斷。 */
const SOURCES = ["", "en", "zh-TW", "zh-CN", "ja", "ko", "de", "fr", "es", "ru"] as const;

interface Existing {
  materialId: string;
  doc: OverlayFile;
}

function counts(doc: OverlayFile): { done: number; total: number } {
  return { done: doc.segments.filter((s) => s.status === "done").length, total: doc.segments.length };
}

/** 最近一次翻某一段的服務（各段記的 `service`）；這一版之前的譯本沒有，是 `null`。 */
function lastSegmentService(doc: OverlayFile) {
  let best: { t: string; s: NonNullable<OverlayFile["segments"][number]["service"]> } | null = null;
  for (const g of doc.segments) {
    if (g.service && g.modified && (!best || g.modified > best.t)) best = { t: g.modified, s: g.service };
  }
  return best?.s ?? null;
}

function langName(code: string): string {
  return code === "" ? t("translate.panel.auto") : t(`settings.generate.lang.${code}` as StringKey);
}

/** 產生譯本 PDF 的進度與結果（工作在 `translate/autoBuild.ts`，離開這個畫面也會繼續）。 */
function BuildStatus(props: { paperId: string; onOpen: (materialId: string) => void }) {
  const b = useSyncExternalStore(subscribeBuild, getBuild);
  if (b.phase === "idle" || b.paperId !== props.paperId) return null;
  if (b.phase === "working") {
    return (
      <div role="status">
        <p>{t(`generate.build.working.${b.stage}` as StringKey, { n: b.n, total: b.total })}</p>
        <ProgressBar n={b.n} total={b.total} />
        <div className="gen-actions">
          <Button block onClick={cancelBuild}>
            {t("common.cancel")}
          </Button>
        </div>
      </div>
    );
  }
  if (b.phase === "failed") {
    return <ErrorNotice message={t("generate.build.failed")} detail={errorReason(b.kind)} />;
  }
  const { out } = b;
  return (
    <div role="status">
      <p>{t(`generate.build.done.${out.outcome}` as StringKey)}</p>
      <Details>
        <p>{t("generate.build.stats", { placed: out.placed, kept: out.kept })}</p>
        {out.keptMath > 0 ? <p>{t("generate.build.math", { n: out.keptMath })}</p> : null}
        {out.shrunk > 0 ? <p>{t("generate.build.shrunk", { n: out.shrunk })}</p> : null}
        {out.truncated > 0 ? <p>{t("generate.build.truncated", { n: out.truncated })}</p> : null}
      </Details>
      <div className="gen-actions">
        <Button primary block onClick={() => props.onOpen(out.materialId)}>
          {t("generate.build.open")}
        </Button>
      </div>
    </div>
  );
}

type GenKind = "translation" | "digest";

export function Generate(props: {
  nav: Nav;
  paperId: string;
  /** 一打開就在哪一種（沒給是譯本；重點筆記正在做時自動切過去）。 */
  kind?: GenKind;
  sheet: SheetKind | null;
  /** 加入譯本或建立譯本之後呼叫（文獻的材料變了，目錄要重新讀）。 */
  onChanged?: () => void;
  /** 短暫的提示（App 的那一個）。 */
  toast: (message: string) => void;
}) {
  const [kind, setKind] = useState<GenKind>(props.kind ?? "translation");
  // 這篇的重點筆記正在做，或停下來等「要不要換備用服務」的回答：直接到那一邊
  useEffect(() => {
    let alive = true;
    void api
      .digestStatus()
      .then((s) => {
        if (alive && s && s.paperId === props.paperId && (s.state === "running" || (s.state === "stopped" && s.fallback))) setKind("digest");
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [props.paperId]);
  const kindSwitch = (
    <Segmented
      className="gen-kind"
      label={t("generate.kind.label")}
      options={[
        ["translation", t("generate.kind.translation")],
        ["digest", t("generate.kind.digest")],
      ]}
      value={kind}
      onChange={setKind}
    />
  );
  if (kind === "digest") {
    return <DigestGenerate nav={props.nav} paperId={props.paperId} kindSwitch={kindSwitch} />;
  }
  return <TranslateGenerate {...props} kindSwitch={kindSwitch} />;
}

function TranslateGenerate(props: {
  nav: Nav;
  paperId: string;
  sheet: SheetKind | null;
  onChanged?: () => void;
  toast: (message: string) => void;
  kindSwitch: ReactNode;
}) {
  const { nav, paperId } = props;
  const od = useOnDevice();
  const engines = useEngines();
  const [consent, setConsent] = useState(false);
  const [skipAsk, setSkipAsk] = useState(false);
  /** 要接著翻的那份譯本；接著翻也要經過面板（不再詢問沒勾就一律詢問）。 */
  const [resumeId, setResumeId] = useState<string | null>(null);
  /** 面板上這次選的服務（代號）、提示詞預設組、第二輪潤飾；沒動過就照設定。 */
  const [serviceId, setServiceId] = useState<string | null>(null);
  const [promptPick, setPromptPick] = useState<string | null>(null);
  const [refinePick, setRefinePick] = useState<boolean | null>(null);
  const [finalPick, setFinalPick] = useState<boolean | null>(null);
  const [existing, setExisting] = useState<Existing | null>(null);
  /** 按了「下載模型並翻譯」，正在問要不要下載。 */
  const [askDownload, setAskDownload] = useState(false);
  /** 下載完要自動開始的那一次（哪個服務、接著翻哪一份）；`downloaded` 是模型已經下載完。 */
  const [pending, setPending] = useState<{ service: string; resume: string | null; downloaded: boolean } | null>(null);

  const loadExisting = async (d: PaperDetail) => {
    const overlays = d.paper.materials.filter((m) => m.role === ROLE_TRANSLATION && m.form === FORM_OVERLAY);
    const last = overlays[overlays.length - 1];
    if (!last) {
      setExisting(null);
      return;
    }
    try {
      const doc = await api.readOverlay(paperId, last.id);
      if (g.alive.current) setExisting({ materialId: last.id, doc });
    } catch {
      if (g.alive.current) setExisting(null);
    }
  };

  const g = useGenerateJob<JobStatus>({
    nav,
    paperId,
    status: api.translateStatus,
    cancel: api.translateCancel,
    // 照上次用的服務選好
    onConfig: (c) => setServiceId((cur) => defaultService(menuServices(c), c.lastService, cur)),
    onLoaded: loadExisting,
    // 翻完（或停下）：重讀這份譯本的進度
    onSettled: async () => {
      if (g.detail) await loadExisting(g.detail);
    },
  });
  const { cfg, detail, job, error } = g;
  const addTranslation = useAddTranslation({
    paperId,
    nav,
    sheet: props.sheet,
    toast: props.toast,
    onChanged: () => props.onChanged?.(),
  });

  const menu = cfg ? menuServices(cfg) : [];
  const svc: TranslateServiceView | null = cfg?.services.find((s) => s.id === serviceId) ?? null;
  const info = svc ? engineOf(engines, svc.provider) : null;
  const local = info?.kind === "on_device";
  const chat = info?.kind === "chat";
  // 接著翻時語言照原本那份譯本；新的照面板上選的
  const resumeDoc = resumeId && existing && existing.materialId === resumeId ? existing.doc : null;
  const targetLang = resumeDoc?.target_lang ?? cfg?.targetLang ?? "zh-TW";
  const targetOk = supportsTarget(info, targetLang);
  const leaves = info?.leavesDevice ?? true;
  const missing = svc && cfg ? missingSetting(svc, info, cfg.secretsSupported) : "key";
  const host = svc?.host ?? "";
  const asks = cfg ? !cfg.skipConfirm : true;
  const needConsent = leaves && asks && !consent;
  const offline = local && od.status ? engineSummary(od.status, info) : null;
  const resumeCounts = resumeDoc ? counts(resumeDoc) : null;
  const providerLabel = (provider: string, model: string) =>
    t(`settings.generate.provider.${provider}` as StringKey) + (model ? `（${model}）` : "");
  const resumeSwitched = (() => {
    if (!resumeDoc || !svc) return null;
    // 原本用的：最近一次翻某一段的服務；這一版之前的譯本看檔頭
    const lastSeg = lastSegmentService(resumeDoc);
    const old = lastSeg
      ? providerLabel(lastSeg.provider, lastSeg.model)
      : providerLabel(resumeDoc.provider, resumeDoc.model);
    const now = providerLabel(svc.provider, chat ? serviceModel(svc, info) : "");
    return old !== now ? { old, now } : null;
  })();
  const button = startButton({
    local,
    model: offline?.state ?? null,
    downloading: od.busy,
    // 模型狀態照設定的目標語言；接著翻時照那份譯本的語言
    sameLang: !od.status || od.status.targetLang === targetLang,
    ready: !!svc && missing === null && !needConsent && targetOk,
  });
  const canStart = button.kind === "start" && button.enabled;
  // 這個面板還在最上面（上面另外疊了畫面，例如到設定改服務，就算離開了）
  const topScreen = [...nav.stack].reverse().find((r) => r.name !== "sheet");
  const onPanel = g.phase === "panel" && topScreen?.name === "generate";

  /** 面板上改的語言與選項、這次用的服務記進設定（下次打開面板沿用）。 */
  const remember = async (service: string) => {
    if (!cfg) return;
    await api.patchTranslateSettings({
      prompt: promptPick ?? cfg.prompt,
      refine: refinePick ?? cfg.refine,
      finalPass: finalPick ?? cfg.finalPass,
      lastService: service,
      ...(skipAsk && !cfg.skipConfirm ? { skipConfirm: true } : {}),
    });
  };

  const setLang = async (patch: { sourceLang?: string; targetLang?: string }) => {
    try {
      await api.patchTranslateSettings(patch);
      await g.loadCfg();
      await od.refresh();
    } catch (e) {
      g.fail("settings.generate.saveFailed", e);
    }
  };

  /** 接著翻：沒勾「不再詢問」就先到面板（顯示這次實際會送到的服務）；勾了而且原本的服務還能用就直接開始。 */
  const askResume = (materialId: string) => {
    const doc = existing && existing.materialId === materialId ? existing.doc : null;
    // 預設用上次翻這一份的服務；不知道就用同一種引擎的服務，再不然用主要服務
    const last = (job && job.materialId === materialId ? job.service : null) ?? (doc ? lastSegmentService(doc)?.id : null);
    const pick =
      menu.find((s) => s.id === last) ?? (doc ? menu.find((s) => s.provider === doc.provider) : undefined) ?? menu[0];
    if (pick) setServiceId(pick.id);
    const pickInfo = pick ? engineOf(engines, pick.provider) : null;
    // 不再詢問只適用於「和上次同一個服務」：換了服務（含使用者剛說不要的備用）一律先到面板
    if (
      pick &&
      pick.id === last &&
      cfg?.skipConfirm &&
      pickInfo?.leavesDevice &&
      missingSetting(pick, pickInfo, cfg.secretsSupported) === null
    ) {
      void start(materialId, pick.id);
      return;
    }
    setResumeId(materialId);
    setConsent(false);
    setSkipAsk(false);
    g.setError(null);
    g.setPhase("panel");
  };

  /**
   * 開始翻譯。`resume` 有值就接著翻那份譯本（只翻還沒翻好的段落，原地更新），否則分析版面、另做一份新的
   * （重新翻譯也是這樣，舊的與上面的劃線、筆跡都留著）。
   */
  const start = (resume: string | null, service: string) =>
    g.once(async () => {
      if (!detail) return;
      g.setError(null);
      g.setDeclined(false);
      try {
        await remember(service);
        // 別篇正在翻時，不要先建好譯本才被拒絕
        const busyNow = await api.translateStatus();
        if (busyNow && busyNow.state === "running") throw new AppError("TranslateBusy", "busy");
        let materialId = resume;
        let initial = { total: 0, done: 0 };
        if (resume && existing && existing.materialId === resume) initial = counts(existing.doc);
        if (resume && job && job.materialId === resume) initial = { total: job.total, done: job.done };
        if (!materialId) {
          const read = await g.analyze("generate.noText", false, true);
          if (!read) return;
          const created = await api.createOverlay(paperId, read.source.id, read.segments, read.modelPages, service);
          materialId = created.materialId;
          initial = { total: read.segments.length, done: 0 };
          props.onChanged?.();
          const d = await api.getPaper(paperId);
          if (g.alive.current) g.setDetail(d);
        }
        await api.translateRun(paperId, materialId, service);
        expectTranslation(); // 翻完（全部成功）就自動產生譯本 PDF
        if (!g.alive.current) return;
        setResumeId(null);
        g.setJob({
          paperId,
          materialId,
          service,
          total: initial.total,
          done: initial.done,
          failed: 0,
          state: "running",
          reason: null,
          fallback: null,
          inputTokens: 0,
          outputTokens: 0,
        });
        g.setPhase("progress");
      } catch (e) {
        if (!g.alive.current) return;
        g.setPhase(resume && g.phase === "progress" ? "progress" : "panel");
        g.fail("generate.start.failed", e);
      }
    });

  /** 「下載模型並翻譯」問過之後：下載，下載完（面板沒離開、服務沒換）自動開始。 */
  const downloadAndStart = () => {
    if (!svc || !cfg) return;
    const want = { service: svc.id, resume: resumeId, downloaded: false };
    setPending(want);
    void od.download(cfg.wifiOnly).then((ok) =>
      setPending((cur) => (cur !== want ? cur : ok ? { ...want, downloaded: true } : null)),
    );
  };

  useEffect(() => {
    if (!pending) return;
    if (!onPanel || pending.service !== serviceId || pending.resume !== resumeId) {
      setPending(null);
      return;
    }
    if (!pending.downloaded) return;
    setPending(null);
    if (canStart) void start(pending.resume, pending.service);
    // 只在這幾項變了時判斷；`start` 每次重畫都是新的
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [pending, onPanel, serviceId, resumeId, canStart]);

  const back = () => void nav.back();
  const openReader = (view: "compare" | "overlay") => nav.push({ name: "reader", paperId, view });
  const openTranslationPdf = (materialId: string) => nav.push({ name: "reader", paperId, material: materialId });
  // 這份譯文疊加檔已經產生過的譯本 PDF（有的話「重新產生」就是原地更新它）
  const generatedPdf =
    existing && detail
      ? detail.paper.materials.find((m) => isGeneratedPdf(m) && m.provenance?.generated_from === existing.materialId)
      : undefined;
  const build = useSyncExternalStore(subscribeBuild, getBuild);
  const buildNow = build.phase === "working";
  // 剛產生完的結果上方已經有「開啟譯本」，下面的按鈕就不重複出現
  const justBuilt =
    build.phase !== "idle" && build.phase !== "working" && build.phase !== "failed" && build.paperId === paperId;
  const errorBox = error ? (
    <ErrorNotice message={error.message} detail={error.detail} {...g.fixAction(error.kind, serviceId)} />
  ) : null;

  // ---------- 進度 ----------
  const done = job?.done ?? 0;
  const total = job?.total ?? 0;
  const remaining = total - done;

  if (g.phase === "analyzing") {
    return (
      <AnalyzingScreen
        title={t("info.generate")}
        status={t(
          !g.analysis.model ? "generate.analyzing" : g.analysis.step === "model" ? "generate.analyzingModel" : "generate.readingText",
          { n: g.analysis.n, total: g.analysis.total },
        )}
        n={g.analysis.n}
        total={g.analysis.total}
        onBack={back}
        onCancel={g.cancel}
      />
    );
  }

  if (g.phase === "progress") {
    const state = job?.state ?? "running";
    const failed = job?.failed ?? 0;
    const usedSvc = cfg?.services.find((s) => s.id === job?.service);
    const offer = state === "stopped" && !g.declined ? (job?.fallback ?? null) : null;
    return (
      <Screen title={t("generate.progress.heading")} onBack={back}>
        <div className="content">
          {g.title ? <p className="muted small">{t("generate.paper", { title: g.title })}</p> : null}
          {usedSvc ? <p className="muted small">{t("translate.progress.service", { name: serviceName(usedSvc) })}</p> : null}
          <p className="gen-count" role="status">
            {t("reader.compare.count", { done, total })}
          </p>
          <ProgressBar n={done} total={total} />
          {state === "running" ? <p className="muted small">{t("generate.progress.running")}</p> : null}
          {state === "finished" && remaining <= 0 ? <p>{t("generate.progress.finished")}</p> : null}
          {state === "cancelled" ? <p>{t("generate.progress.cancelled", { done })}</p> : null}
          {state === "stopped" ? (
            <ErrorNotice
              message={t("generate.progress.stopped", { done })}
              detail={job?.reason ? errorReason(job.reason) : undefined}
              {...g.fixAction(job?.reason, job?.service ?? null)}
            />
          ) : null}
          {offer && job ? (
            <FallbackAsk
              ask={t("translate.fallback.ask", { name: serviceName(offer) })}
              sends={
                offer.host
                  ? t("translate.fallback.sends", { n: remaining, host: offer.host })
                  : t("translate.fallback.local", { n: remaining })
              }
              yes={t("translate.fallback.yes", { name: serviceName(offer) })}
              no={t("translate.fallback.no")}
              onYes={() => void start(job.materialId, offer.id)}
              onNo={() => g.setDeclined(true)}
            />
          ) : null}
          {failed > 0 || (state === "finished" && remaining > 0) ? (
            <p className="muted small">{t("generate.progress.failed", { n: Math.max(failed, remaining) })}</p>
          ) : null}
          <UsageDetails
            paperId={paperId}
            input={job?.inputTokens ?? 0}
            output={job?.outputTokens ?? 0}
            service={job?.service}
            services={cfg?.services ?? []}
          />
          {errorBox}
          {state !== "running" ? <BuildStatus paperId={paperId} onOpen={openTranslationPdf} /> : null}
          <div className="gen-actions">
            {state === "running" ? (
              <Button block onClick={g.cancel}>
                {t("common.cancel")}
              </Button>
            ) : (
              <>
                {remaining > 0 && job && !offer ? (
                  <Button primary block onClick={() => askResume(job.materialId)}>
                    {t("generate.existing.resume")}
                  </Button>
                ) : null}
                <Button primary={remaining <= 0} block onClick={() => openReader("compare")}>
                  {t("reader.compare.title")}
                </Button>
                <Button block onClick={() => openReader("overlay")}>
                  {t("reader.view.overlay")}
                </Button>
                <Button block onClick={back}>
                  {t("export.done")}
                </Button>
              </>
            )}
          </div>
        </div>
      </Screen>
    );
  }

  const targets = TARGETS.filter((l) => supportsTarget(info, l));
  const existingCounts = existing ? counts(existing.doc) : null;

  return (
    <>
      <Screen title={t("info.generate")} onBack={back}>
        <div className="content">
          {props.kindSwitch}
          {g.title ? <p className="muted small">{t("generate.paper", { title: g.title })}</p> : null}
          {errorBox}

          {existing && existingCounts && !resumeId ? (
            <section className="gen-existing">
              <h2 className="gen-heading">
                {t("generate.existing.heading")}
                <InfoTip label={t("generate.build.now")}>{t("generate.build.about")}</InfoTip>
              </h2>
              <p>
                {t("generate.existing.summary", {
                  ...existingCounts,
                  // 各段記的服務；這一版之前的譯本看檔頭建立時的引擎
                  provider: (() => {
                    const used = servicesUsed(existing.doc.segments);
                    return used.length > 0
                      ? used.map((s) => segmentServiceName(s, cfg?.services ?? null)).join("、")
                      : t(`settings.generate.provider.${existing.doc.provider}` as StringKey);
                  })(),
                  lang: langName(existing.doc.target_lang),
                })}
              </p>
              {existingCounts.done >= existingCounts.total ? (
                <p className="muted small">{t("generate.existing.complete")}</p>
              ) : null}
              {generatedPdf ? <p className="muted small">{t("generate.build.has")}</p> : null}
              <BuildStatus paperId={paperId} onOpen={openTranslationPdf} />
              <div className="gen-actions">
                {existingCounts.done < existingCounts.total ? (
                  <Button primary block onClick={() => askResume(existing.materialId)}>
                    {t("generate.existing.resume")}
                  </Button>
                ) : null}
                {existingCounts.done > 0 ? (
                  <Button
                    primary={existingCounts.done >= existingCounts.total && !generatedPdf}
                    block
                    disabled={buildNow}
                    onClick={() => void startBuild(paperId, existing.materialId)}
                  >
                    {generatedPdf
                      ? t("generate.build.redo")
                      : existingCounts.done >= existingCounts.total
                        ? t("generate.build.now")
                        : t("generate.build.partial")}
                  </Button>
                ) : null}
                {generatedPdf && !justBuilt ? (
                  <Button block onClick={() => openTranslationPdf(generatedPdf.id)}>
                    {t("generate.build.open")}
                  </Button>
                ) : null}
                <Button block onClick={() => openReader("compare")}>
                  {t("reader.compare.title")}
                </Button>
              </div>
            </section>
          ) : null}

          <section className="gen-panel" aria-label={t("translate.panel.title")}>
            <h2 className="gen-heading">
              {resumeId ? t("generate.existing.resume") : existing ? t("translate.panel.again") : t("translate.panel.title")}
            </h2>
            {resumeCounts ? (
              <p className="muted small">
                {t(resumeNoteKey(info), { n: resumeCounts.total - resumeCounts.done, total: resumeCounts.total })}
              </p>
            ) : null}
            {/* 整篇重翻另做一份，舊的留著（畫面上寫明會做什麼） */}
            {existing && !resumeId ? <p className="muted small">{t("translate.panel.againNote")}</p> : null}

            {cfg ? (
              resumeDoc ? (
                <p className="gen-langs">
                  {langName(resumeDoc.source_lang ?? "")} → {langName(resumeDoc.target_lang)}
                </p>
              ) : (
                <div className="gen-langs-row">
                  <label className="field">
                    <span>{t("translate.panel.from")}</span>
                    <select className="input" value={cfg.sourceLang} onChange={(e) => void setLang({ sourceLang: e.target.value })}>
                      {SOURCES.map((l) => (
                        <option key={l} value={l}>
                          {langName(l)}
                        </option>
                      ))}
                    </select>
                  </label>
                  <span className="gen-arrow" aria-hidden="true">
                    →
                  </span>
                  <label className="field">
                    <span>{t("translate.panel.to")}</span>
                    <select className="input" value={cfg.targetLang} onChange={(e) => void setLang({ targetLang: e.target.value })}>
                      {targets.map((l) => (
                        <option key={l} value={l}>
                          {langName(l)}
                        </option>
                      ))}
                      {targets.some((l) => l === cfg.targetLang) ? null : (
                        <option value={cfg.targetLang}>{langName(cfg.targetLang)}</option>
                      )}
                    </select>
                  </label>
                </div>
              )
            ) : null}

            <ServicePanel
              label={t("translate.panel.service")}
              menu={menu}
              value={serviceId}
              onPick={(id) => {
                setServiceId(id);
                setConsent(false);
                setAskDownload(false);
              }}
              empty={cfg ? t("translate.panel.noService") : null}
              svc={svc}
              sends={
                svc
                  ? local
                    ? t("generate.confirm.sends.none")
                    : missing === null
                      ? t("generate.confirm.sends.account", { host })
                      : null
                  : null
              }
              missing={svc ? missing : null}
              insecure={svc && !local ? isInsecureRemote(svc.endpoint) : false}
              consent={
                leaves && missing === null && asks
                  ? { label: t("generate.confirm.consent", { host }), checked: consent, onChange: setConsent }
                  : null
              }
              after={
                leaves && missing === null && asks ? (
                  <CheckRow checked={skipAsk} onChange={setSkipAsk}>
                    {t("generate.confirm.skipAsk")}
                  </CheckRow>
                ) : null
              }
              warnings={
                <>
                  {resumeSwitched ? (
                    <p className="field-error" role="alert">
                      {t("generate.confirm.switched", { old: resumeSwitched.old, now: resumeSwitched.now })}
                    </p>
                  ) : null}
                  {svc && !targetOk ? <KindNotice kind="TranslateLanguage" {...g.fixAction("TranslateLanguage", serviceId)} /> : null}
                </>
              }
              onFix={g.toService}
            >
              {chat && cfg ? (
                <>
                  <PresetPicker
                    nav={nav}
                    kind="translation"
                    label={t("prompts.title")}
                    presets={cfg.prompts}
                    value={promptPick ?? cfg.prompt}
                    onPick={setPromptPick}
                    paperId={paperId}
                    materialId={resumeId}
                    refine={refinePick ?? cfg.refine}
                  />
                  <CheckRow checked={refinePick ?? cfg.refine} onChange={setRefinePick} hint={t("translate.panel.refineCost")}>
                    {t("translate.panel.refine")}
                  </CheckRow>
                  <CheckRow checked={finalPick ?? cfg.finalPass} onChange={setFinalPick} hint={t("translate.panel.finalPassAbout")}>
                    {t("translate.panel.finalPass")}
                  </CheckRow>
                </>
              ) : null}
              {svc?.provider === "deepl" && cfg && !resumeDoc && cfg.sourceLang === "" ? (
                <p className="muted small">{t("translate.panel.deeplGlossary")}</p>
              ) : null}
              {/* 模型的狀態一句；要下載就按下面的主按鈕 */}
              {offline ? (
                <p className="muted small" role="status">
                  {offline.line}
                </p>
              ) : null}
            </ServicePanel>

            {button.kind === "downloading" && od.status ? (
              <div className="od-model">
                <ModelProgress progress={od.status.download} onStop={od.stop} />
                {pending ? <p className="muted small">{t("generate.local.autoStart")}</p> : null}
              </div>
            ) : null}
            {button.kind === "download" && askDownload && od.status && offline ? (
              <ModelAsk
                title={t("ondevice.ask.title")}
                body={t("ondevice.ask.body", { host: od.status.bergamotHost, size: offline.size })}
                onConfirm={() => {
                  setAskDownload(false);
                  downloadAndStart();
                }}
                onCancel={() => setAskDownload(false)}
              />
            ) : null}
            {local && od.note && !od.note.ok ? (
              <p className="field-error" role="alert">
                {od.note.text}
              </p>
            ) : null}
            <div className="gen-actions">
              {button.kind === "download" && !askDownload ? (
                <Button primary block disabled={!button.enabled || addTranslation.busy} onClick={() => setAskDownload(true)}>
                  {t("generate.local.downloadStart", { size: offline?.size ?? "" })}
                </Button>
              ) : null}
              {button.kind === "start" ? (
                <Button
                  primary
                  block
                  disabled={!canStart || addTranslation.busy}
                  onClick={() => svc && void start(resumeId, svc.id)}
                >
                  {resumeId ? t("generate.existing.resume") : t("translate.panel.start")}
                </Button>
              ) : null}
              {resumeId ? (
                <Button block onClick={() => setResumeId(null)}>
                  {t("common.cancel")}
                </Button>
              ) : (
                <Button block disabled={addTranslation.busy} onClick={() => void addTranslation.start()}>
                  {t("translate.panel.external")}
                </Button>
              )}
            </div>
          </section>
        </div>
      </Screen>
      {addTranslation.overlays}
    </>
  );
}
