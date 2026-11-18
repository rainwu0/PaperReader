// 設定 → 翻譯服務：服務清單（每個服務一個開關，點進去改設定）、主要與備用服務、新增服務。
// 做法參考沉浸式翻譯的服務清單與 LiteLLM 的具名實例。

import { useEffect } from "react";
import { api, type TranslateProvider, type TranslateServiceView } from "../../../api";
import type { Nav, SheetKind } from "../../../nav";
import { errorMessage, t, type StringKey } from "../../../strings";
import { Button, ErrorNotice, InfoTip, Sheet, TopBar } from "../../../ui/components";
import { engineOf, serviceName, useEngines } from "../../generate/engines";
import { missingSetting } from "../../generate/translateConfig";
import { Section } from "../parts";
import { useTranslateSettings } from "./useTranslateSettings";

/** 列上的第二行：引擎、送到哪裡、設定好了沒。 */
function useStatusLine() {
  const engines = useEngines();
  return (s: TranslateServiceView, secretsSupported: boolean) => {
    const info = engineOf(engines, s.provider);
    const engine = t(`settings.generate.provider.${s.provider}` as StringKey);
    const parts = s.name.trim() !== "" ? [engine] : [];
    if (info?.kind === "on_device") parts.push(t("services.offline"));
    else if (s.host) parts.push(s.host);
    const miss = missingSetting(s, info, secretsSupported);
    if (miss) parts.push(t(`generate.confirm.why.${miss}` as StringKey));
    return parts.join(" · ");
  };
}

export function TranslateServices(props: { nav: Nav; sheet: SheetKind | null }) {
  const { nav } = props;
  const st = useTranslateSettings();
  const engines = useEngines();
  const statusLine = useStatusLine();
  const v = st.view;
  // 從一個服務的頁面回來時重新讀（那裡改了名稱、網址、金鑰）
  const onTop = nav.top.name === "translateServices";
  const { reload } = st;
  useEffect(() => {
    if (onTop) void reload();
  }, [onTop, reload]);

  const toggle = (s: TranslateServiceView, enabled: boolean) =>
    void st.run(() => api.saveTranslateService({ ...strip(s), enabled }));

  const add = async (provider: TranslateProvider) => {
    let id = "";
    const ok = await st.run(async () => {
      id = await api.addTranslateService(provider);
    });
    await nav.back();
    if (ok) nav.push({ name: "translateService", id });
  };

  const enabled = v?.services.filter((s) => s.enabled) ?? [];
  const moveFallback = (i: number, d: -1 | 1) => {
    if (!v) return;
    const f = [...v.fallbacks];
    const j = i + d;
    if (j < 0 || j >= f.length) return;
    [f[i], f[j]] = [f[j] as string, f[i] as string];
    void st.patch({ fallbacks: f });
  };
  const byId = (id: string) => v?.services.find((s) => s.id === id);

  return (
    <div className="screen settings">
      <TopBar title={t("services.title")} onBack={() => void nav.back()} />
      <div className="scroll">
        <div className="set-inner">
          {st.error ? <ErrorNotice message={t("settings.generate.saveFailed")} detail={errorMessage(st.error)} /> : null}
          {v ? (
            <>
              <Section title={t("services.list")}>
                {v.services.map((s) => (
                  <div key={s.id} className="set-row svc-row">
                    <button
                      type="button"
                      className="svc-main"
                      onClick={() => nav.push({ name: "translateService", id: s.id })}
                    >
                      <span className="set-text">
                        <span className="set-title">
                          {serviceName(s)}
                          {s.id === v.primary ? <span className="svc-badge">{t("services.primary")}</span> : null}
                          {v.fallbacks.includes(s.id) ? (
                            <span className="svc-badge">
                              {t("services.fallbackN", { n: v.fallbacks.indexOf(s.id) + 1 })}
                            </span>
                          ) : null}
                        </span>
                        <span className="set-hint">{statusLine(s, v.secretsSupported)}</span>
                      </span>
                      <span className="set-chev" aria-hidden="true">
                        ›
                      </span>
                    </button>
                    <input
                      type="checkbox"
                      role="switch"
                      className="set-switch"
                      aria-label={t("services.enabled", { name: serviceName(s) })}
                      checked={s.enabled}
                      onChange={(e) => toggle(s, e.target.checked)}
                    />
                  </div>
                ))}
              </Section>
              <div className="set-pad">
                <Button block onClick={() => nav.push({ name: "sheet", kind: "serviceAdd" })}>
                  {t("services.add")}
                </Button>
              </div>

              <Section title={t("services.order")}>
                <div className="set-pad">
                  <label className="field">
                    <span>
                      {t("services.primary")}
                      <InfoTip label={t("services.order")}>{t("services.orderAbout")}</InfoTip>
                    </span>
                    <select
                      className="input"
                      value={v.primary}
                      onChange={(e) => void st.patch({ primary: e.target.value })}
                    >
                      {enabled.map((s) => (
                        <option key={s.id} value={s.id}>
                          {serviceName(s)}
                        </option>
                      ))}
                      {enabled.some((s) => s.id === v.primary) ? null : (
                        <option value={v.primary}>{byId(v.primary) ? serviceName(byId(v.primary)!) : v.primary}</option>
                      )}
                    </select>
                  </label>
                  <p className="field-label">{t("services.fallbacks")}</p>
                  {v.fallbacks.length === 0 ? <p className="muted small">{t("services.noFallback")}</p> : null}
                  <ol className="svc-fallbacks">
                    {v.fallbacks.map((id, i) => {
                      const s = byId(id);
                      return (
                        <li key={id}>
                          <span className="svc-fb-name">{s ? serviceName(s) : id}</span>
                          <button type="button" className="btn small" disabled={i === 0} onClick={() => moveFallback(i, -1)}>
                            {t("services.up")}
                          </button>
                          <button
                            type="button"
                            className="btn small"
                            disabled={i === v.fallbacks.length - 1}
                            onClick={() => moveFallback(i, 1)}
                          >
                            {t("services.down")}
                          </button>
                          <button
                            type="button"
                            className="btn small"
                            onClick={() => void st.patch({ fallbacks: v.fallbacks.filter((f) => f !== id) })}
                          >
                            {t("services.remove")}
                          </button>
                        </li>
                      );
                    })}
                  </ol>
                  {enabled.some((s) => s.id !== v.primary && !v.fallbacks.includes(s.id)) && v.fallbacks.length < 5 ? (
                    <label className="field">
                      <span>{t("services.addFallback")}</span>
                      <select
                        className="input"
                        value=""
                        onChange={(e) => e.target.value && void st.patch({ fallbacks: [...v.fallbacks, e.target.value] })}
                      >
                        <option value="">{t("services.pick")}</option>
                        {enabled
                          .filter((s) => s.id !== v.primary && !v.fallbacks.includes(s.id))
                          .map((s) => (
                            <option key={s.id} value={s.id}>
                              {serviceName(s)}
                            </option>
                          ))}
                      </select>
                    </label>
                  ) : null}
                </div>
              </Section>
            </>
          ) : null}
        </div>
      </div>

      <Sheet open={props.sheet === "serviceAdd"} title={t("services.addTitle")} onDismiss={() => void nav.back()}>
        <div role="list">
          {(engines ?? [])
            .filter((e) => e.kind !== "on_device" || !v?.services.some((s) => s.provider === e.provider))
            .map((e) => (
              <button
                key={e.provider}
                type="button"
                role="listitem"
                className="pick-card"
                onClick={() => void add(e.provider as TranslateProvider)}
              >
                <span className="pick-card-text">
                  <span className="pick-card-name">{t(`settings.generate.provider.${e.provider}` as StringKey)}</span>
                  <span className="pick-card-desc">{t(`services.kind.${e.kind}` as StringKey)}</span>
                </span>
              </button>
            ))}
        </div>
      </Sheet>
    </div>
  );
}

/** 畫面上的服務去掉只供顯示的欄位，存回去用。 */
export function strip(s: TranslateServiceView) {
  return {
    id: s.id,
    name: s.name,
    provider: s.provider,
    model: s.model,
    endpoint: s.endpoint,
    enabled: s.enabled,
    rpm: s.rpm,
    prices: s.prices,
  };
}
