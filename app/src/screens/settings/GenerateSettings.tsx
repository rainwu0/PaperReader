import { useEffect, useState } from "react";
import { api, type LayoutModelStatus } from "../../api";
import type { Nav } from "../../nav";
import { errorMessage, t } from "../../strings";
import { ErrorNotice } from "../../ui/components";
import { serviceName } from "../generate/engines";
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
    void reload();
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
        })}
        chevron
        onClick={() => nav.push({ name: "translatePrompts" })}
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
          deleteAsk={{ title: t("layout.delete.title"), body: t("layout.delete.body") }}
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
