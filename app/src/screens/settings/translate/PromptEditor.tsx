// 編輯一組提示詞預設組：只放使用者改得到的——名稱（自訂的）、要做什麼與怎麼寫、重點筆記的區段。
// 回覆格式、段落代號、臺灣用語與全形標點、術語這些固定的部分不在這裡，在「看完整提示詞」原樣列出。
// 內建的也可以改，改過的可以「還原預設」；自訂的可以刪。存檔由外殼再檢查一次（`translate.rs` 的 `validate_preset`）。

import { useEffect, useState } from "react";
import { api, type PromptPreset, type PromptPresetView, type PromptSection } from "../../../api";
import { hasText, t, type StringKey } from "../../../strings";
import { Button, Details } from "../../../ui/components";
import { PromptPreviewBox } from "./PromptPreview";

/** 提示詞裡不認得的變數（只用來在輸入框下面點名是哪一個；擋不擋由外殼決定）。 */
export function unknownVars(text: string): string[] {
  const known = new Set(["from", "to", "title"]);
  return [...text.matchAll(/\{\{\s*([^}]*?)\s*\}\}/g)].map((m) => m[1] ?? "").filter((v) => !known.has(v));
}

/** 內建區段的名稱（名稱欄空白時的提示；畫面的文字表有這個代號才有）。 */
function builtinLabel(id: string): string {
  return hasText(`digest.section.${id}`) && id !== "other" ? t(`digest.section.${id}` as StringKey) : "";
}

/** 一組最多幾個區段（只用來關掉「加一個區段」；擋不擋由外殼決定）。 */
const MAX_SECTIONS = 12;

/** 外殼交回的原因代號（`validate_preset`）→ 畫面的說法。規則只有外殼那一份。 */
const PROBLEM: Record<string, StringKey> = {
  "prompt name": "prompts.problem.name",
  "prompt text": "prompts.problem.text",
  "prompt variable": "prompts.problem.variable",
  "prompt sections": "prompts.problem.sections",
  "section id": "prompts.section.badId",
  "section label": "prompts.problem.label",
  "section about": "prompts.section.needAbout",
  "section max": "prompts.section.badMax",
};

/** 草稿合不合規則：問外殼。檢查中是 `undefined`；合規則是 `null`。 */
function useProblem(draft: PromptPreset): StringKey | null | undefined {
  const [result, setResult] = useState<{ key: string; problem: StringKey | null } | null>(null);
  const key = JSON.stringify(draft);
  useEffect(() => {
    let alive = true;
    api
      .checkPrompt(JSON.parse(key) as PromptPreset)
      .then((why) => alive && setResult({ key, problem: why === null ? null : (PROBLEM[why] ?? "prompts.problem.other") }))
      .catch(() => alive && setResult({ key, problem: "prompts.problem.other" }));
    return () => {
      alive = false;
    };
  }, [key]);
  return result && result.key === key ? result.problem : undefined;
}

function SectionsEditor(props: { sections: PromptSection[]; onChange: (s: PromptSection[]) => void }) {
  const { sections, onChange } = props;
  const set = (i: number, patch: Partial<PromptSection>) =>
    onChange(sections.map((s, k) => (k === i ? { ...s, ...patch } : s)));
  const move = (i: number, d: number) => {
    const j = i + d;
    if (j < 0 || j >= sections.length) return;
    const next = [...sections];
    [next[i], next[j]] = [next[j] as PromptSection, next[i] as PromptSection];
    onChange(next);
  };
  return (
    <div className="prompt-sections">
      <p className="field-label">{t("prompts.sections")}</p>
      <small className="muted block-hint">{t("prompts.sectionsAbout")}</small>
      <div>
        {sections.map((s, i) => (
          <div key={i} className="cmp-seg">
            <div className="glossary-form">
              <label className="field">
                <span>{t("prompts.section.label")}</span>
                <input
                  className="input"
                  value={s.label}
                  placeholder={builtinLabel(s.id)}
                  onChange={(e) => set(i, { label: e.target.value })}
                />
              </label>
              <label className="field">
                <span>{t("prompts.section.id")}</span>
                <input
                  className="input"
                  value={s.id}
                  autoCapitalize="none"
                  spellCheck={false}
                  onChange={(e) => set(i, { id: e.target.value.trim() })}
                />
                <small className="muted">{t("prompts.section.idHint")}</small>
              </label>
              <label className="field">
                <span>{t("prompts.section.about")}</span>
                <textarea
                  className="input"
                  rows={2}
                  value={s.about}
                  spellCheck={false}
                  onChange={(e) => set(i, { about: e.target.value })}
                />
              </label>
              <label className="field">
                <span>{t("prompts.section.max")}</span>
                <input
                  className="input"
                  inputMode="numeric"
                  value={String(s.max)}
                  onChange={(e) => set(i, { max: Math.round(Number(e.target.value) || 0) })}
                />
              </label>
              <div className="btn-row">
                <Button disabled={i === 0} onClick={() => move(i, -1)}>
                  {t("prompts.section.up")}
                </Button>
                <Button disabled={i === sections.length - 1} onClick={() => move(i, 1)}>
                  {t("prompts.section.down")}
                </Button>
                <Button disabled={sections.length <= 1} onClick={() => onChange(sections.filter((_, k) => k !== i))}>
                  {t("prompts.section.remove")}
                </Button>
              </div>
            </div>
          </div>
        ))}
      </div>
      <Button
        block
        disabled={sections.length >= MAX_SECTIONS}
        onClick={() => onChange([...sections, { id: "", label: "", about: "", max: 3 }])}
      >
        {t("prompts.section.add")}
      </Button>
    </div>
  );
}

export function PromptEditor(props: {
  preset: PromptPresetView;
  /** 名稱（內建的照文字表）。 */
  name: string;
  busy: boolean;
  onSave: (p: PromptPreset) => void;
  onCopy: (p: PromptPreset) => void;
  /** 自訂的：刪掉；改過的內建組：還原預設。 */
  onRemove: () => void;
}) {
  const { preset } = props;
  const [draft, setDraft] = useState<PromptPreset>(() => ({
    id: preset.id,
    name: preset.name,
    kind: preset.kind,
    text: preset.text,
    sections: preset.sections,
  }));
  const problem = useProblem(draft);
  const bad = unknownVars(draft.text);
  const changed =
    draft.name !== preset.name || draft.text !== preset.text || JSON.stringify(draft.sections) !== JSON.stringify(preset.sections);

  return (
    <div className="set-pad prompt-edit">
      {!preset.builtin ? (
        <label className="field">
          <span>{t("prompts.name")}</span>
          <input className="input" value={draft.name} onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))} />
        </label>
      ) : null}
      <label className="field">
        <span>{t("prompts.text")}</span>
        <textarea
          className="input prompt-area"
          rows={8}
          value={draft.text}
          spellCheck={false}
          onChange={(e) => setDraft((d) => ({ ...d, text: e.target.value }))}
        />
        <small className="muted">{t("prompts.vars")}</small>
        {bad.length > 0 ? (
          <small className="field-error" role="alert">
            {t("prompts.badVar", { name: `{{${bad[0]}}}` })}
          </small>
        ) : null}
      </label>
      {draft.kind === "digest" ? (
        <SectionsEditor sections={draft.sections} onChange={(sections) => setDraft((d) => ({ ...d, sections }))} />
      ) : null}
      {problem && changed ? (
        <p className="field-error" role="alert">
          {t(problem)}
        </p>
      ) : null}
      <Details label={t("prompts.full")}>
        {problem === null ? (
          <PromptPreviewBox preset={draft} paperId={null} />
        ) : problem ? (
          <p className="field-error">{t(problem)}</p>
        ) : null}
      </Details>
      <div className="btn-row">
        <Button primary disabled={problem !== null || !changed || props.busy} onClick={() => props.onSave(draft)}>
          {t("prompts.save")}
        </Button>
        <Button disabled={props.busy} onClick={() => props.onCopy({ ...draft, name: t("prompts.copyOf", { name: props.name }) })}>
          {t("prompts.copy")}
        </Button>
        {!preset.builtin ? (
          <Button disabled={props.busy} onClick={props.onRemove}>
            {t("prompts.delete")}
          </Button>
        ) : preset.modified ? (
          <Button disabled={props.busy} onClick={props.onRemove}>
            {t("prompts.reset")}
          </Button>
        ) : null}
      </div>
    </div>
  );
}
