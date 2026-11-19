// 設定 → 提示詞預設組：翻譯與重點筆記共用一套，上面切換用途。內建的（翻譯「通用」「學術論文」、
// 重點筆記「標準」）可以直接改、改過的可以還原預設；自訂的可以新增、改、刪。選中的是產生時預設用的那一組。
// 只改那一組（`translate_prompt_save`、`translate_prompt_delete`），不拿畫面上的整份清單蓋回去。

import { useState } from "react";
import { api, type PresetKind, type PromptPreset, type PromptPresetView } from "../../../api";
import type { Nav } from "../../../nav";
import { errorMessage, t, type StringKey } from "../../../strings";
import { Button, ErrorNotice, InfoTip, Screen, Segmented, type ShowUndo } from "../../../ui/components";
import { Section } from "../parts";
import { PromptEditor } from "./PromptEditor";
import { useTranslateSettings } from "./useTranslateSettings";

/** 預設組顯示的名稱：內建的照文字表。 */
export function presetName(p: Pick<PromptPresetView, "id" | "name" | "builtin">): string {
  return p.builtin ? t(`prompts.builtin.${p.id}` as StringKey) : p.name;
}

/** 從產生面板打開時要先看哪一種、展開哪一組（導覽的頁面不帶參數，打開時讀一次）。 */
let focus: { kind: PresetKind; id: string | null } | null = null;

/** 打開提示詞預設組的設定頁，先看 `kind` 這一種、展開 `id` 那一組（產生面板的「編輯這一組」）。 */
export function openPromptSettings(nav: Nav, kind: PresetKind, id: string | null) {
  focus = { kind, id };
  nav.push({ name: "translatePrompts" });
}

/** 每種用途選用的是設定裡的哪一個欄位、新增時從哪一組複製。 */
const KIND: Record<PresetKind, { field: "prompt" | "digestPrompt" | "quickPrompt"; base: string }> = {
  translation: { field: "prompt", base: "academic" },
  digest: { field: "digestPrompt", base: "standard" },
  quick: { field: "quickPrompt", base: "quick" },
};

function newId(): string {
  return `custom_${Math.random().toString(36).slice(2, 10).replace(/[^a-z0-9]/g, "")}`;
}

export function TranslatePrompts(props: { nav: Nav; undo: ShowUndo }) {
  const { nav } = props;
  const st = useTranslateSettings();
  const [start] = useState(() => {
    const f = focus;
    focus = null;
    return f;
  });
  const [kind, setKind] = useState<PresetKind>(start?.kind ?? "translation");
  const [open, setOpen] = useState<string | null>(start?.id ?? null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const v = st.view;
  if (!v) {
    return <Screen title={t("prompts.title")} onBack={() => void nav.back()} className="settings" />;
  }
  const list = v.prompts.filter((p) => p.kind === kind);
  const selected = v[KIND[kind].field];

  const act = async (f: () => Promise<unknown>, done: StringKey, then?: () => void) => {
    if (busy) return;
    setBusy(true);
    setNote(null);
    const ok = await st.run(f);
    setBusy(false);
    if (ok) {
      setNote(t(done));
      then?.();
    }
  };

  const select = (id: string) => void st.patch({ [KIND[kind].field]: id });

  /** 另存一份自訂的（複製、新增）並展開它。 */
  const copy = (p: PromptPreset) => {
    const id = newId();
    void act(() => api.savePrompt({ ...p, id }), "prompts.saved", () => setOpen(id));
  };

  /**
   * 刪一組自訂的，或把改過的內建組還原預設：直接做，提示給「復原」。
   * 復原把原本那一組存回去；刪掉的是產生時預設用的那一組時（外殼改回了內建的），選擇也改回來。
   */
  const remove = async (p: PromptPresetView) => {
    if (busy) return;
    setBusy(true);
    setNote(null);
    const wasSelected = selected === p.id;
    const ok = await st.run(() => api.deletePrompt(p.id));
    setBusy(false);
    if (!ok) return;
    if (!p.builtin) setOpen(null);
    const original: PromptPreset = { id: p.id, name: p.name, kind: p.kind, text: p.text, sections: p.sections };
    props.undo(p.builtin ? t("prompts.resetDone") : t("prompts.deletedOne", { name: presetName(p) }), async () => {
      await api.savePrompt(original);
      if (wasSelected) await api.patchTranslateSettings({ [KIND[p.kind].field]: p.id });
      await st.reload();
    });
  };

  const addNew = () => {
    const base = list.find((p) => p.id === KIND[kind].base) ?? list[0];
    if (!base) return;
    copy({ id: "", name: t("prompts.newName"), kind, text: base.text, sections: base.sections });
  };

  return (
    <Screen title={t("prompts.title")} onBack={() => void nav.back()} className="settings">
      <div className="set-inner">
        {st.error ? <ErrorNotice message={t("prompts.saveFailed")} detail={errorMessage(st.error)} /> : null}
        <div className="set-pad">
          <Segmented
            label={t("prompts.kind.label")}
            options={[
              ["translation", t("prompts.kind.translation")],
              ["digest", t("prompts.kind.digest")],
              ["quick", t("prompts.kind.quick")],
            ]}
            value={kind}
            onChange={(k) => {
              setKind(k);
              setOpen(null);
              setNote(null);
            }}
          />
        </div>
        <p className="set-hint set-about">
          {t(`prompts.about.${kind}`)}
          <InfoTip label={t("prompts.title")}>{t("prompts.rules")}</InfoTip>
        </p>
        {note ? (
          <p className="gen-status set-pad" role="status">
            {note}
          </p>
        ) : null}
        <Section title={t("prompts.list")}>
          {list.map((p) => (
            <div key={p.id} className="prompt-item">
              <div className="set-row svc-row">
                <input
                  type="radio"
                  name={`prompt-${kind}`}
                  className="prompt-radio"
                  aria-label={t("prompts.useThis", { name: presetName(p) })}
                  checked={selected === p.id}
                  onChange={() => select(p.id)}
                />
                <button
                  type="button"
                  className="svc-main"
                  aria-expanded={open === p.id}
                  onClick={() => setOpen((o) => (o === p.id ? null : p.id))}
                >
                  <span className="set-text">
                    <span className="set-title">
                      {presetName(p)}
                      {p.builtin ? <span className="svc-badge">{t("prompts.builtinBadge")}</span> : null}
                      {p.modified ? <span className="svc-badge">{t("prompts.modifiedBadge")}</span> : null}
                    </span>
                    <span className="set-hint">{p.text.split("\n")[0]?.slice(0, 60)}</span>
                  </span>
                  <span className="set-chev" aria-hidden="true">
                    {open === p.id ? "⌃" : "⌄"}
                  </span>
                </button>
              </div>
              {open === p.id ? (
                <PromptEditor
                  key={JSON.stringify(p)}
                  preset={p}
                  name={presetName(p)}
                  busy={busy}
                  onSave={(d) => void act(() => api.savePrompt(d), "prompts.saved")}
                  onCopy={copy}
                  onRemove={() => void remove(p)}
                />
              ) : null}
            </div>
          ))}
        </Section>
        <div className="set-pad">
          <Button block disabled={busy} onClick={addNew}>
            {t("prompts.add")}
          </Button>
        </div>
      </div>
    </Screen>
  );
}
