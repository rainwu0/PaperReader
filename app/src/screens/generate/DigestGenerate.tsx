// 產生重點筆記：一個面板（服務、送到哪裡與同意、開始）→ 讀原文分段 → 進度 → 完成。
// 服務照「翻譯服務」的清單，只列對話模型類的；送出前在同一個面板寫明主機、要同意；
// 主要服務停擺時先問要不要改用依序的下一個。工作在外殼的背景進行，離開畫面也會做完並存進文獻庫。
// 和產生譯本共用的流程在 `useGenerateJob.ts`、`ServicePanel.tsx`、`JobProgress.tsx`。

import { type ReactNode, useState } from "react";
import { api, AppError, type DigestStatus, type TranslateServiceView } from "../../api";
import { lastDigest } from "../../api/types";
import { shortDate } from "../../format";
import type { Nav } from "../../nav";
import { errorReason, t, type StringKey } from "../../strings";
import { MATERIALS_CHANGED } from "../../translate/autoBuild";
import { Button, CheckRow, ErrorNotice, Screen } from "../../ui/components";
import { findAbstract } from "./digestInput";
import { engineOf, serviceName, useEngines } from "./engines";
import { AnalyzingScreen, FallbackAsk, ProgressBar, UsageDetails } from "./JobProgress";
import { PresetPicker, ServicePanel } from "./ServicePanel";
import { defaultService, isInsecureRemote, menuServices, missingSetting } from "./translateConfig";
import { useGenerateJob } from "./useGenerateJob";

export function DigestGenerate(props: {
  nav: Nav;
  paperId: string;
  /** 「要產生什麼」的切換（譯本／重點筆記），放在面板最上面。 */
  kindSwitch: ReactNode;
}) {
  const { nav, paperId } = props;
  const engines = useEngines();
  const g = useGenerateJob<DigestStatus>({
    nav,
    paperId,
    status: api.digestStatus,
    cancel: api.digestCancel,
    // 做完重新讀這篇（材料多了一份）；首頁與底下這篇的閱讀畫面也跟著重讀（「看重點筆記」回到它時已經是新的那份）
    onSettled: async (s) => {
      if (s?.state !== "finished") return;
      window.dispatchEvent(new CustomEvent(MATERIALS_CHANGED, { detail: { paperId } }));
      const d = await api.getPaper(paperId).catch(() => null);
      if (d && g.alive.current) g.setDetail(d);
    },
  });
  const { cfg, job, error } = g;
  const [serviceId, setServiceId] = useState<string | null>(null);
  const [consent, setConsent] = useState(false);
  /** 已有重點筆記時另外做一份（預設是新的取代舊的，舊的移到回收區）。 */
  const [keepOld, setKeepOld] = useState(false);
  /** 面板上這次選的預設組；沒動過就照設定（上次選的）。 */
  const [promptPick, setPromptPick] = useState<string | null>(null);

  // 對話模型類、啟用的服務（主要服務在前）
  const menu = cfg && engines ? menuServices(cfg).filter((s) => engineOf(engines, s.provider)?.kind === "chat") : [];
  // 這次選的；沒選過就照上次產生重點筆記用的；都不在選單裡（第一次、或剛被停用）用選單的第一個
  const picked = defaultService(menu, cfg?.lastDigestService, serviceId);

  const svc: TranslateServiceView | null = cfg?.services.find((s) => s.id === picked) ?? null;
  const info = svc ? engineOf(engines, svc.provider) : null;
  const missing = svc && cfg ? missingSetting(svc, info, cfg.secretsSupported) : "key";
  const host = svc?.host ?? "";
  const asks = cfg ? !cfg.skipConfirm : true;
  const canStart = !!svc && missing === null && !(asks && !consent);
  const existing = g.detail ? lastDigest(g.detail.paper.materials) : undefined;
  const lang = cfg ? t(`settings.generate.lang.${cfg.targetLang}` as StringKey) : "";
  /**
   * 看重點筆記。堆疊底下有這篇的閱讀畫面（從它進來的）：退回那個閱讀畫面、在它上面打開重點筆記，不再疊一個
   * 同一篇的閱讀畫面。底下沒有（從首頁或文獻資訊直接進來）才打開一個。
   */
  const openDigest = async () => {
    const at = nav.stack.map((r) => r.name === "reader" && r.paperId === paperId).lastIndexOf(true);
    if (at < 0) {
      nav.push({ name: "reader", paperId, view: "digest" });
      return;
    }
    for (let n = nav.stack.length - 1 - at; n > 0; n--) await nav.back();
    nav.push({ name: "sheet", kind: "digest" });
  };
  const back = () => void nav.back();

  const start = () =>
    g.once(async () => {
      if (!g.detail || !svc) return;
      g.setError(null);
      g.setDeclined(false);
      try {
        const busyNow = await api.digestStatus();
        if (busyNow && busyNow.state === "running") throw new AppError("TranslateBusy", "busy");
        // 選的預設組與服務記進設定：外殼照預設組產生，下次打開面板兩者都沿用
        await api.patchTranslateSettings({
          lastDigestService: svc.id,
          ...(promptPick && cfg && promptPick !== cfg.digestPrompt ? { digestPrompt: promptPick } : {}),
        });
        const read = await g.analyze("digest.noText", true);
        if (!read) return;
        await api.digestRun(
          paperId,
          read.source.id,
          read.segments.map((s) => ({ page: s.page, source: s.source })),
          findAbstract(read.segments),
          svc.id,
          existing && !keepOld ? existing.id : null,
        );
        if (!g.alive.current) return;
        g.setJob({
          paperId,
          service: svc.id,
          state: "running",
          part: 0,
          parts: 0,
          merging: false,
          reason: null,
          fallback: null,
          materialId: null,
          points: 0,
          dropped: 0,
          inputTokens: 0,
          outputTokens: 0,
        });
        setConsent(false);
        g.setPhase("progress");
      } catch (e) {
        if (!g.alive.current) return;
        g.setPhase("panel");
        g.fail("digest.start.failed", e);
      }
    });

  /** 停下來之後接著做（同一個服務，或同意改用的備用服務）。 */
  const continueWith = (service: string) =>
    g.once(async () => {
      g.setError(null);
      try {
        await api.digestContinue(service);
        if (!g.alive.current) return;
        g.setDeclined(false);
        setConsent(false);
        g.setJob((j) => (j ? { ...j, service, state: "running", reason: null, fallback: null } : j));
      } catch (e) {
        if (g.alive.current) g.fail("digest.start.failed", e);
      }
    });

  const errorBox = error ? (
    <ErrorNotice message={error.message} detail={error.detail} {...g.fixAction(error.kind, picked)} />
  ) : null;

  if (g.phase === "analyzing") {
    return (
      <AnalyzingScreen
        title={t("digest.title")}
        status={t("digest.progress.analyzing", { n: g.analysis.n, total: g.analysis.total })}
        n={g.analysis.n}
        total={g.analysis.total}
        onBack={back}
        onCancel={g.cancel}
      />
    );
  }

  if (g.phase === "progress" && job) {
    const state = job.state;
    const offer = state === "stopped" && !g.declined ? job.fallback : null;
    const used = cfg?.services.find((s) => s.id === job.service);
    const stage = job.merging
      ? t("digest.progress.merging")
      : job.parts > 1
        ? t("digest.progress.part", { n: job.part, total: job.parts })
        : t("digest.progress.one");
    const usedHost = used?.host ?? "";
    return (
      <Screen title={t("digest.progress.title")} onBack={back}>
        <div className="content">
          {g.title ? <p className="muted small">{t("generate.paper", { title: g.title })}</p> : null}
          {used ? <p className="muted small">{t("digest.progress.service", { name: serviceName(used) })}</p> : null}
          {state === "running" ? (
            <>
              <p className="gen-count" role="status">
                {stage}
              </p>
              <ProgressBar n={job.merging ? job.parts : Math.max(0, job.part - 1)} total={Math.max(1, job.parts)} />
              <p className="muted small">{t("digest.progress.leave")}</p>
              <div className="gen-actions">
                <Button block onClick={g.cancel}>
                  {t("common.cancel")}
                </Button>
              </div>
            </>
          ) : null}
          {state === "finished" ? (
            <>
              <p role="status">{t("digest.progress.done", { n: job.points })}</p>
              {job.dropped > 0 ? <p className="muted small">{t("digest.progress.dropped", { n: job.dropped })}</p> : null}
              <div className="gen-actions">
                <Button primary block onClick={() => void openDigest()}>
                  {t("digest.open")}
                </Button>
                <Button block onClick={back}>
                  {t("export.done")}
                </Button>
              </div>
            </>
          ) : null}
          {state === "cancelled" ? (
            <>
              <p role="status">{t("digest.progress.cancelled")}</p>
              <div className="gen-actions">
                <Button block onClick={() => g.setPhase("panel")}>
                  {t("digest.title")}
                </Button>
              </div>
            </>
          ) : null}
          {state === "stopped" ? (
            <>
              <ErrorNotice
                message={t("digest.progress.stopped")}
                detail={job.reason ? errorReason(job.reason) : undefined}
                {...g.fixAction(job.reason, job.service)}
              />
              {offer ? (
                <FallbackAsk
                  ask={t("digest.fallback.ask", { name: serviceName(offer) })}
                  sends={offer.host ? t("digest.fallback.sends", { host: offer.host }) : null}
                  yes={t("digest.fallback.yes", { name: serviceName(offer) })}
                  no={t("digest.fallback.no")}
                  onYes={() => void continueWith(offer.id)}
                  onNo={() => g.setDeclined(true)}
                />
              ) : (
                <>
                  {asks && usedHost ? (
                    <CheckRow checked={consent} onChange={setConsent}>
                      {t("digest.consent", { host: usedHost })}
                    </CheckRow>
                  ) : null}
                  <div className="gen-actions">
                    <Button
                      primary
                      block
                      disabled={asks && !consent && !!usedHost}
                      onClick={() => void continueWith(job.service)}
                    >
                      {t("digest.progress.continue")}
                    </Button>
                    <Button block onClick={() => g.setPhase("panel")}>
                      {t("digest.progress.restart")}
                    </Button>
                  </div>
                </>
              )}
            </>
          ) : null}
          <UsageDetails
            paperId={paperId}
            input={job.inputTokens}
            output={job.outputTokens}
            service={job.service}
            services={cfg?.services ?? []}
          />
          {errorBox}
        </div>
      </Screen>
    );
  }

  return (
    <Screen title={t("digest.title")} onBack={back}>
      <div className="content">
        {props.kindSwitch}
        {g.title ? <p className="muted small">{t("generate.paper", { title: g.title })}</p> : null}
        {errorBox}
        {existing ? (
          <section className="gen-existing">
            <p>{t("digest.existing", { date: shortDate(existing.provenance?.time) })}</p>
            <div className="gen-actions">
              <Button block onClick={() => void openDigest()}>
                {t("digest.open")}
              </Button>
            </div>
            <CheckRow
              checked={keepOld}
              onChange={setKeepOld}
              hint={keepOld ? t("digest.keepOld.on") : t("digest.keepOld.off")}
            >
              {t("digest.keepOld")}
            </CheckRow>
          </section>
        ) : null}
        <section className="gen-panel" aria-label={t("digest.title")}>
          <p className="muted small">{t("digest.about")}</p>
          {cfg ? (
            <p className="gen-langs">
              {t("digest.lang", { lang })}
              <small className="muted block-hint">{t("digest.lang.hint")}</small>
            </p>
          ) : null}
          <ServicePanel
            label={t("digest.service")}
            menu={menu}
            value={picked}
            onPick={(id) => {
              setServiceId(id);
              setConsent(false);
            }}
            empty={cfg && engines ? t("digest.noService") : null}
            svc={svc}
            sends={svc && missing === null ? t("digest.sends", { host }) : null}
            missing={svc ? missing : null}
            insecure={svc ? isInsecureRemote(svc.endpoint) : false}
            consent={
              svc && missing === null && asks
                ? { label: t("digest.consent", { host }), checked: consent, onChange: setConsent }
                : null
            }
            onFix={g.toService}
          >
            {cfg ? (
              <PresetPicker
                nav={nav}
                kind="digest"
                label={t("digest.prompt")}
                presets={cfg.prompts}
                value={promptPick ?? cfg.digestPrompt}
                onPick={setPromptPick}
                paperId={paperId}
              />
            ) : null}
          </ServicePanel>
          <div className="gen-actions">
            <Button primary block disabled={!canStart} onClick={() => void start()}>
              {t("digest.start")}
            </Button>
          </div>
        </section>
      </div>
    </Screen>
  );
}
