import { useEffect, useState } from "react";
import { api, type LayoutModelStatus, type TranslateSettingsView } from "../../api";
import type { Nav } from "../../nav";
import { errorMessage, t } from "../../strings";
import { Button, ErrorNotice } from "../../ui/components";
import { serviceName } from "../generate/engines";
import { migrateLegacyConsent } from "../generate/quickConsent";
import { fullTranslationService, menuServices } from "../generate/translateConfig";
import { ModelFileControls, useModelFile, type ModelFileOps } from "../generate/localEngine";
import { formatBytes } from "./useBackup";
import { SettingRow, SwitchRow } from "./parts";
import { presetName } from "./translate/Prompts";
import { useTranslateSettings } from "./translate/useTranslateSettings";

/** 設定裡的「產生」區：翻譯服務、提示詞預設組、術語庫（各自一頁），送出前要不要詢問，版面模型。 */
export function GenerateSettings(props: { nav: Nav }) {
  const { nav } = props;
  const st = useTranslateSettings();
  const [terms, setTerms] = useState<number | null>(null);
  // 從子頁回來時重新讀（子頁改了服務、預設組或術語）
  const onTop = nav.top.name === "settings";
  const { reload } = st;
  useEffect(() => {
    if (!onTop) return;
    // 舊版記在本機儲存的快速翻譯同意，有的話先搬進設定檔再讀
    void api
      .translateSettings()
      .then(migrateLegacyConsent)
      .catch(() => {})
      .then(() => reload());
    api
      .glossaryGet()
      .then((g) => setTerms(g.length))
      .catch(() => {});
  }, [onTop, reload]);

  const v = st.view;
  if (!v) {
    return st.error ? <ErrorNotice message={t("settings.generate.saveFailed")} detail={errorMessage(st.error)} /> : null;
  }
  const primary = v.services.find((s) => s.id === v.primary);
  const preset = v.prompts.find((p) => p.id === v.prompt);
  const digestPreset = v.prompts.find((p) => p.id === v.digestPrompt);
  const quickPreset = v.prompts.find((p) => p.id === v.quickPrompt);
  const on = v.services.filter((s) => s.enabled).length;

  return (
    <>
      <SettingRow
        title={t("services.title")}
        hint={t("services.summary", { n: on, name: primary ? serviceName(primary) : "—" })}
        chevron
        onClick={() => nav.push({ name: "translateServices" })}
      />
      <SettingRow
        title={t("prompts.title")}
        hint={t("prompts.summary", {
          translation: preset ? presetName(preset) : "—",
          digest: digestPreset ? presetName(digestPreset) : "—",
          quick: quickPreset ? presetName(quickPreset) : "—",
        })}
        chevron
        onClick={() => nav.push({ name: "translatePrompts" })}
      />
      <QuickServiceRow
        v={v}
        onPick={(id) => void st.patch({ quickService: id })}
        onResetConsents={() => st.patch({ quickConsent: [] })}
      />
      <SettingRow
        title={t("glossary.title")}
        value={terms !== null ? t("glossary.countShort", { n: terms }) : undefined}
        chevron
        onClick={() => nav.push({ name: "glossary" })}
      />
      <SwitchRow
        title={t("settings.generate.skipConfirm")}
        hint={t("settings.generate.skipConfirmHint")}
        checked={v.skipConfirm}
        onChange={(skipConfirm) => void st.patch({ skipConfirm })}
      />
      <LayoutModel wifiOnly={v.wifiOnly} />
    </>
  );
}

/**
 * 快速翻譯用的服務：一個設定，預設「跟全文翻譯相同」；閱讀畫面卡片角落的小下拉改的是同一個。
 * 下面列這台裝置上已經同意送出的服務數，可以全部重新詢問。
 */
function QuickServiceRow(props: {
  v: TranslateSettingsView;
  onPick: (id: string) => void;
  onResetConsents: () => Promise<boolean>;
}) {
  const { v } = props;
  const consents = v.quickConsent;
  const [reset, setReset] = useState(false);
  const menu = menuServices(v);
  const full = menu.find((s) => s.id === fullTranslationService(v));
  const value = v.quickService && menu.some((s) => s.id === v.quickService) ? v.quickService : "";
  return (
    <div className="set-pad quick-setting">
      <label className="field">
        <span>{t("quick.settings.title")}</span>
        <select className="input" value={value} onChange={(e) => props.onPick(e.target.value)}>
          <option value="">{t("quick.sameAsFull", { name: full ? serviceName(full) : "—" })}</option>
          {menu.map((s) => (
            <option key={s.id} value={s.id}>
              {serviceName(s)}
            </option>
          ))}
        </select>
        <small className="muted">{t("quick.settings.hint")}</small>
      </label>
      {consents.length > 0 ? (
        <div className="btn-row">
          <span className="muted small">{t("quick.settings.consents", { n: consents.length })}</span>
          <Button
            onClick={() => {
              void props.onResetConsents().then((ok) => setReset(ok));
            }}
          >
            {t("quick.settings.resetConsents")}
          </Button>
        </div>
      ) : null}
      {reset ? (
        <p className="gen-status" role="status">
          {t("quick.settings.consentsReset")}
        </p>
      ) : null}
    </div>
  );
}

const LAYOUT: ModelFileOps<LayoutModelStatus> = {
  status: () => api.layoutStatus(),
  download: (wifiOnly) => api.layoutDownload(wifiOnly),
  cancel: () => api.layoutDownloadCancel(),
  remove: () => api.layoutDelete(),
};

/** 版面模型（只用在翻譯的分段）：和離線翻譯的語言模型同一套下載與刪除。這個建置沒有版面模型時不顯示。 */
function LayoutModel(props: { wifiOnly: boolean }) {
  const lm = useModelFile(LAYOUT);
  const s = lm.status;
  if (!s?.supported) return null;
  const size = formatBytes(s.downloadBytes);
  return (
    <>
      <SettingRow title={t("layout.title")} hint={t("layout.about")} />
      <div className="set-pad od-model">
        <ModelFileControls
          line={s.downloaded ? t("ondevice.ready", { size: formatBytes(s.bytes) }) : t("ondevice.notReady", { size })}
          missing={!s.downloaded}
          askTitle={t("layout.ask.title")}
          askBody={t("ondevice.ask.body", { host: s.host, size })}
          busy={lm.busy}
          progress={s.download}
          onDownload={() => void lm.download(props.wifiOnly)}
          onStop={lm.stop}
          {...(s.downloaded ? { onDelete: () => void lm.remove("") } : {})}
        />
        {lm.note ? (
          <p className={lm.note.ok ? "gen-status" : "field-error"} role={lm.note.ok ? "status" : "alert"}>
            {lm.note.text}
          </p>
        ) : null}
      </div>
    </>
  );
}
