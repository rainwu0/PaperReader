// 設定 → 翻譯服務 → 一個服務：名稱、模型、網址、速率、金鑰、啟用、測試、還原預設、刪除。
// 要填哪些欄位照引擎申報；離線引擎改成顯示語言模型的下載狀態。

import { useState } from "react";
import { api, kindOf, type TranslateServiceView, type TranslateTestResult } from "../../../api";
import type { Nav, SheetKind } from "../../../nav";
import { errorMessage, t, type StringKey } from "../../../strings";
import {
  Button,
  ConfirmSheet,
  Details,
  ErrorNotice,
  InfoTip,
  Screen,
  Sheet,
  TopBar,
  type ShowUndo,
} from "../../../ui/components";
import { engineOf, serviceName, useEngines } from "../../generate/engines";
import { EngineModel, useOnDevice } from "../../generate/localEngine";
import { isInsecureRemote } from "../../generate/translateConfig";
import { Section, SwitchRow } from "../parts";
import { strip } from "./Services";
import { useTranslateSettings } from "./useTranslateSettings";

/** 已設定金鑰時欄位裡顯示的遮罩：固定長度，看得出填過，不透露金鑰的長度與內容（畫面讀不回金鑰）。 */
const KEY_MASK = "•".repeat(12);

/** 測試成功：送出的句子、收到的回應、模型（或引擎）與花了多久。 */
function TestOk(props: { r: TranslateTestResult; engine: string }) {
  const { r } = props;
  return (
    <>
      <span className="block-hint">{t("settings.generate.testSent", { text: r.sent })}</span>
      <span className="block-hint">{t("settings.generate.testReceived", { text: r.received })}</span>
      <small className="muted block-hint">
        {t("settings.generate.testMeta", { model: r.model || props.engine, seconds: (r.ms / 1000).toFixed(1) })}
      </small>
    </>
  );
}

export function TranslateServiceScreen(props: { nav: Nav; id: string; sheet: SheetKind | null; undo: ShowUndo }) {
  const { nav, id } = props;
  const st = useTranslateSettings();
  const engines = useEngines();
  const od = useOnDevice();
  const svc = st.view?.services.find((s) => s.id === id) ?? null;
  const info = svc ? engineOf(engines, svc.provider) : null;

  // 文字欄的草稿：離開欄位時才存
  const [draft, setDraft] = useState({ name: "", model: "", endpoint: "", rpm: "", priceIn: "", priceOut: "" });
  const [keyText, setKeyText] = useState("");
  /** 正在輸入新的金鑰（已設定時，點了欄位才換成空白讓你貼新的）。 */
  const [keyEditing, setKeyEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string; detail?: string } | null>(null);
  const [tested, setTested] = useState<TranslateTestResult | null>(null);
  // 服務的內容從外殼重新讀回來（或換了服務）時，草稿跟著更新
  const svcKey = svc ? JSON.stringify([svc.id, svc.name, svc.model, svc.endpoint, svc.rpm, svc.prices]) : "";
  const [seen, setSeen] = useState("");
  if (svc && svcKey !== seen) {
    setSeen(svcKey);
    setDraft({
      name: svc.name,
      model: svc.model,
      endpoint: svc.endpoint,
      rpm: svc.rpm ? String(svc.rpm) : "",
      priceIn: svc.prices ? String(svc.prices.input) : "",
      priceOut: svc.prices ? String(svc.prices.output) : "",
    });
  }

  if (!st.view || !svc) {
    return (
      <Screen title={t("services.title")} onBack={() => void nav.back()} className="settings" />
    );
  }

  const v = st.view;
  const showModel = !!info && info.model !== "no";
  const showEndpoint = !!info && info.endpoint !== "no";
  const showKey = !!info && info.key !== "no";
  const isCompat = info?.endpoint === "required";
  const local = info?.kind === "on_device";

  const save = (next: Partial<TranslateServiceView>) => st.run(() => api.saveTranslateService({ ...strip(svc), ...next }));

  const saveDraft = async () => {
    const rpm = Math.max(0, Math.min(6000, Math.round(Number(draft.rpm) || 0)));
    // 單價：兩欄都空白是沒填；填了一欄，另一欄當 0
    const price = (s: string) => Math.max(0, Math.min(10000, Number(s.trim()) || 0));
    const prices =
      draft.priceIn.trim() === "" && draft.priceOut.trim() === ""
        ? null
        : { input: price(draft.priceIn), output: price(draft.priceOut) };
    if (
      draft.name === svc.name &&
      draft.model === svc.model &&
      draft.endpoint === svc.endpoint &&
      rpm === svc.rpm &&
      JSON.stringify(prices) === JSON.stringify(svc.prices)
    )
      return;
    await save({ name: draft.name.trim(), model: draft.model.trim(), endpoint: draft.endpoint.trim(), rpm, prices });
  };

  const act = async (f: () => Promise<unknown>, ok: StringKey) => {
    if (busy) return;
    setBusy(true);
    setResult(null);
    setTested(null);
    try {
      await f();
      setResult({ ok: true, text: t(ok) });
    } catch (e) {
      setResult({ ok: false, text: t("settings.generate.saveFailed"), detail: errorMessage(kindOf(e)) });
    } finally {
      setBusy(false);
      await st.reload();
    }
  };

  const runTest = async () => {
    if (busy) return;
    setBusy(true);
    setResult(null);
    setTested(null);
    try {
      await saveDraft();
      setTested(await api.testTranslate(id));
    } catch (e) {
      setResult({ ok: false, text: t("services.testFailed"), detail: errorMessage(kindOf(e)) });
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    await nav.back(); // 關掉確認框
    const ok = await st.run(() => api.deleteTranslateService(id));
    if (ok) await nav.back();
  };

  /** 還原預設：直接做，提示給「復原」（存回原本的設定；金鑰另外存，不受影響）。 */
  const reset = async () => {
    if (busy) return;
    const before = strip(svc);
    setBusy(true);
    setResult(null);
    setTested(null);
    const ok = await st.run(() => api.resetTranslateService(id));
    setBusy(false);
    if (ok) {
      props.undo(t("prompts.resetDone"), async () => {
        await api.saveTranslateService(before);
        await st.reload();
      });
    }
  };

  const clearKey = async () => {
    await nav.back(); // 關掉確認框
    await act(() => api.clearTranslateKey(id), "settings.generate.keyCleared");
  };

  const field = (k: keyof typeof draft) => ({
    value: draft[k],
    autoComplete: "off",
    autoCapitalize: "none",
    spellCheck: false,
    onChange: (e: { target: { value: string } }) => setDraft((d) => ({ ...d, [k]: e.target.value })),
    onBlur: () => void saveDraft(),
  });

  return (
    <div className="screen settings">
      <TopBar title={serviceName(svc)} onBack={() => void nav.back()} />
      <div className="scroll">
        <div className="set-inner">
          {st.error ? <ErrorNotice message={t("settings.generate.saveFailed")} detail={errorMessage(st.error)} /> : null}
          <Section title={t(`settings.generate.provider.${svc.provider}` as StringKey)}>
            <SwitchRow
              title={t("services.enabledRow")}
              hint={t("services.enabledHint")}
              checked={svc.enabled}
              onChange={(enabled) => void save({ enabled })}
            />
            <div className="set-pad">
              <label className="field">
                <span>{t("services.name")}</span>
                <input className="input" placeholder={t(`settings.generate.provider.${svc.provider}` as StringKey)} {...field("name")} />
              </label>

              {showModel ? (
                <label className="field">
                  <span>{t("settings.generate.model")}</span>
                  <input className="input" placeholder={info?.defaultModel ?? ""} {...field("model")} />
                  {isCompat ? (
                    draft.model.trim() === "" ? <small className="muted">{t("settings.generate.modelRequired")}</small> : null
                  ) : (
                    <small className="muted">{t("settings.generate.modelHint", { name: info?.defaultModel ?? "" })}</small>
                  )}
                </label>
              ) : null}

              {showEndpoint ? (
                <label className="field">
                  <span>{t("settings.generate.endpoint")}</span>
                  <input
                    className="input"
                    inputMode="url"
                    placeholder={isCompat ? t("settings.generate.endpointPlaceholder") : "https://"}
                    {...field("endpoint")}
                  />
                  {isCompat ? (
                    draft.endpoint.trim() === "" ? <small className="muted">{t("settings.generate.endpointRequired")}</small> : null
                  ) : (
                    <small className="muted">{t("settings.generate.endpointHint")}</small>
                  )}
                  {isInsecureRemote(draft.endpoint) ? (
                    <small className="field-error" role="alert">
                      {t("settings.generate.insecure")}
                    </small>
                  ) : null}
                </label>
              ) : null}

              {svc.host ? <p className="muted small">{t("services.sendsTo", { host: svc.host })}</p> : null}

              {showKey ? (
                <>
                  <label className="field">
                    <span>
                      {t("settings.generate.key")}
                      <InfoTip label={t("settings.generate.key")}>{t("settings.generate.keyAbout")}</InfoTip>
                    </span>
                    <input
                      className="input"
                      type="password"
                      value={svc.hasKey && !keyEditing ? KEY_MASK : keyText}
                      placeholder={t("settings.generate.keyPlaceholder")}
                      autoComplete="off"
                      autoCapitalize="none"
                      spellCheck={false}
                      disabled={!v.secretsSupported}
                      onFocus={() => setKeyEditing(true)}
                      onBlur={() => keyText === "" && setKeyEditing(false)}
                      onChange={(e) => setKeyText(e.target.value)}
                    />
                    <small className="muted">
                      {!v.secretsSupported
                        ? t("settings.generate.unsupported")
                        : svc.hasKey
                          ? t("settings.generate.keySet")
                          : info?.key === "optional"
                            ? t("settings.generate.keyOptional")
                            : t("settings.generate.keyUnset")}
                    </small>
                  </label>
                  <div className="btn-row">
                    <Button
                      primary
                      disabled={keyText.trim() === "" || busy}
                      onClick={() =>
                        void act(async () => {
                          await api.setTranslateKey(id, keyText);
                          setKeyText("");
                          setKeyEditing(false);
                        }, "settings.generate.keySaved")
                      }
                    >
                      {t("settings.generate.keySave")}
                    </Button>
                    {/* 金鑰讀不回來，清了就沒辦法復原：先確認一次 */}
                    <Button disabled={!svc.hasKey || busy} onClick={() => nav.push({ name: "sheet", kind: "keyClear" })}>
                      {t("settings.generate.keyClear")}
                    </Button>
                  </div>
                </>
              ) : null}

              {local ? (
                <>
                  <p className="field-label">{t("services.model")}</p>
                  <EngineModel od={od} wifiOnly={v.wifiOnly} canDelete />
                  {od.note ? (
                    <p className={od.note.ok ? "gen-status" : "field-error"} role={od.note.ok ? "status" : "alert"}>
                      {od.note.text}
                    </p>
                  ) : null}
                </>
              ) : null}

              {!local ? (
                <label className="field">
                  <span>
                    {t("services.rpm")}
                    <InfoTip label={t("services.rpm")}>{t("services.rpmAbout")}</InfoTip>
                  </span>
                  <input className="input" inputMode="numeric" placeholder={t("services.rpmNone")} {...field("rpm")} />
                </label>
              ) : null}

              {info?.kind === "chat" ? (
                <>
                  <p className="field-label">
                    {t("services.prices")}
                    <InfoTip label={t("services.prices")}>{t("services.pricesAbout")}</InfoTip>
                  </p>
                  <div className="gen-langs-row">
                    <label className="field">
                      <span>{t("services.priceIn")}</span>
                      <input className="input" inputMode="decimal" {...field("priceIn")} />
                    </label>
                    <label className="field">
                      <span>{t("services.priceOut")}</span>
                      <input className="input" inputMode="decimal" {...field("priceOut")} />
                    </label>
                  </div>
                </>
              ) : null}
            </div>
            {local ? (
              <SwitchRow
                title={t("ondevice.wifiOnly")}
                checked={v.wifiOnly}
                onChange={(wifiOnly) => void st.patch({ wifiOnly }).then(() => od.refresh())}
              />
            ) : null}
          </Section>

          <div className="set-pad">
            <div className="gen-actions">
              <Button primary block disabled={busy} onClick={() => void runTest()}>
                {busy ? t("settings.generate.testing") : t("services.test")}
              </Button>
              <Button block disabled={busy} onClick={() => void reset()}>
                {t("services.reset")}
              </Button>
              <Button block disabled={busy || v.services.length <= 1} onClick={() => nav.push({ name: "sheet", kind: "serviceDelete" })}>
                {t("services.delete")}
              </Button>
            </div>
            {tested ? (
              <div className="gen-status" role="status">
                <TestOk r={tested} engine={t(`settings.generate.provider.${svc.provider}` as StringKey)} />
              </div>
            ) : null}
            {result ? (
              <div className={result.ok ? "gen-status" : "field-error"} role={result.ok ? "status" : "alert"}>
                {result.text}
                {result.detail ? <Details>{result.detail}</Details> : null}
              </div>
            ) : null}
          </div>
        </div>
      </div>

      <Sheet
        open={props.sheet === "serviceDelete"}
        title={t("services.deleteTitle", { name: serviceName(svc) })}
        center
        onDismiss={() => void nav.back()}
      >
        <p>{t("services.deleteBody")}</p>
        <div className="actions">
          <Button primary block onClick={() => void remove()}>
            {t("services.delete")}
          </Button>
          <Button block onClick={() => void nav.back()}>
            {t("common.cancel")}
          </Button>
        </div>
      </Sheet>
      <ConfirmSheet
        open={props.sheet === "keyClear"}
        title={t("key.clear.title")}
        confirm={t("settings.generate.keyClear")}
        onConfirm={() => void clearKey()}
        onCancel={() => void nav.back()}
      >
        <p>{t("key.clear.body")}</p>
      </ConfirmSheet>
    </div>
  );
}
