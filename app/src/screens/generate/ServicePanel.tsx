// 產生面板上「用哪個服務、送到哪裡、要你同意」那一段：譯本與重點筆記共用。
// 兩邊不同的地方由呼叫的一方給：選單列哪些服務、送出的說法、要不要問同意、選單下面多出的選項。

import type { ReactNode } from "react";
import type { PresetKind, PromptPresetView, TranslateServiceView } from "../../api";
import type { Nav } from "../../nav";
import { t, type StringKey } from "../../strings";
import { Button, CheckRow, Details } from "../../ui/components";
import { openPromptSettings, presetName } from "../settings/translate/Prompts";
import { PromptPreviewBox } from "../settings/translate/PromptPreview";
import { serviceName } from "./engines";
import type { missingSetting } from "./translateConfig";

/**
 * 選提示詞預設組：第一層只有選單（照上次選的）；第二層是「看完整提示詞」，
 * 裡面列出這篇實際會送出的內容，下面有「編輯這一組」到設定頁。譯本與重點筆記共用。
 */
export function PresetPicker(props: {
  nav: Nav;
  kind: PresetKind;
  label: string;
  presets: PromptPresetView[];
  value: string;
  onPick: (id: string) => void;
  paperId: string;
  /** 接著翻的那份譯本（預覽用它的語言與術語）。 */
  materialId?: string | null;
  /** 開了第二輪潤飾（預覽多列那一次請求）。 */
  refine?: boolean;
}) {
  const list = props.presets.filter((p) => p.kind === props.kind);
  const picked = list.find((p) => p.id === props.value) ?? list[0];
  return (
    <>
      <label className="field">
        <span>{props.label}</span>
        <select className="input" value={picked?.id ?? ""} onChange={(e) => props.onPick(e.target.value)}>
          {list.map((p) => (
            <option key={p.id} value={p.id}>
              {presetName(p)}
            </option>
          ))}
        </select>
      </label>
      {picked ? (
        <Details label={t("prompts.full")}>
          <PromptPreviewBox
            preset={picked}
            paperId={props.paperId}
            materialId={props.materialId ?? null}
            refine={props.refine ?? false}
          />
          <Button block onClick={() => openPromptSettings(props.nav, props.kind, picked.id)}>
            {t("prompts.edit")}
          </Button>
        </Details>
      ) : null}
    </>
  );
}

export function ServicePanel(props: {
  label: string;
  menu: TranslateServiceView[];
  value: string | null;
  onPick: (id: string) => void;
  /** 選單是空的時的說明（另給一顆「前往設定」）；還在讀設定時是 `null`。 */
  empty: string | null;
  svc: TranslateServiceView | null;
  /** 文字會送到哪裡（設定不完整時不說）。 */
  sends: string | null;
  missing: ReturnType<typeof missingSetting>;
  insecure: boolean;
  /** 同意送出：要問時給說明與勾選狀態。 */
  consent: { label: string; checked: boolean; onChange: (v: boolean) => void } | null;
  /** 「前往設定」：`null` 是服務清單。 */
  onFix: (id: string | null) => void;
  /** 選單下面的選項（提示詞、第二輪潤飾、離線模型…）。 */
  children?: ReactNode;
  /** 送出說明下面的提醒（換了服務、不支援目標語言）。 */
  warnings?: ReactNode;
  /** 同意下面的選項（之後不再詢問）。 */
  after?: ReactNode;
}) {
  const { svc, missing } = props;
  return (
    <>
      <label className="field">
        <span>{props.label}</span>
        <select className="input" value={props.value ?? ""} onChange={(e) => props.onPick(e.target.value)}>
          {props.menu.map((s) => (
            <option key={s.id} value={s.id}>
              {serviceName(s)}
            </option>
          ))}
        </select>
      </label>
      {props.empty !== null && props.menu.length === 0 ? (
        <>
          <p className="field-error">{props.empty}</p>
          <Button block onClick={() => props.onFix(null)}>
            {t("common.toSettings")}
          </Button>
        </>
      ) : null}
      {props.children}
      {props.sends ? <p className="muted small">{props.sends}</p> : null}
      {props.warnings}
      {props.insecure ? (
        <p className="field-error" role="alert">
          {t("generate.confirm.insecure")}
        </p>
      ) : null}
      {svc && missing !== null ? (
        <>
          <p className="field-error" role="alert">
            {t("generate.confirm.needSettings", { why: t(`generate.confirm.why.${missing}` as StringKey) })}
          </p>
          <Button block onClick={() => props.onFix(svc.id)}>
            {t("common.toSettings")}
          </Button>
        </>
      ) : null}
      {props.consent ? (
        <CheckRow checked={props.consent.checked} onChange={props.consent.onChange}>
          {props.consent.label}
        </CheckRow>
      ) : null}
      {props.after}
    </>
  );
}
