// 設定 → 術語庫：使用者自己的「原文詞 → 譯法」對照。翻譯時只把這一批原文裡出現的術語交給對話模型；
// DeepL 用它的術語表功能。可以匯入、匯出三欄 TSV（原文、譯法、備註；OmegaT 與 DeepL 都讀得懂）。
// 存在應用專屬空間，不進文獻庫。

import { useCallback, useEffect, useMemo, useState } from "react";
import { api, kindOf, type GlossaryTerm } from "../../../api";
import type { Nav } from "../../../nav";
import { errorMessage, t } from "../../../strings";
import { Button, Details, InfoTip, Screen } from "../../../ui/components";
import { Section } from "../parts";

const EMPTY: GlossaryTerm = { source: "", target: "", note: "" };

export function GlossaryScreen(props: { nav: Nav; toast: (message: string) => void }) {
  const { nav, toast: setToast } = props;
  const [terms, setTerms] = useState<GlossaryTerm[] | null>(null);
  const [form, setForm] = useState<GlossaryTerm>(EMPTY);
  const [editing, setEditing] = useState<number | null>(null);
  const [edit, setEdit] = useState<GlossaryTerm>(EMPTY);
  const [filter, setFilter] = useState("");
  const [note, setNote] = useState<{ text: string; detail?: string } | null>(null);

  const load = useCallback(async () => {
    try {
      setTerms(await api.glossaryGet());
    } catch (e) {
      setNote({ text: t("glossary.failed"), detail: errorMessage(kindOf(e)) });
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  const save = async (next: GlossaryTerm[]) => {
    try {
      await api.glossarySet(next);
      setTerms(next);
      setNote(null);
      return true;
    } catch (e) {
      setNote({ text: t("glossary.failed"), detail: errorMessage(kindOf(e)) });
      return false;
    }
  };

  const clean = (g: GlossaryTerm): GlossaryTerm => ({ source: g.source.trim(), target: g.target.trim(), note: g.note.trim() });

  const addTerm = async () => {
    if (!terms) return;
    const g = clean(form);
    if (!g.source || !g.target) return;
    // 原文詞相同的取代
    const i = terms.findIndex((x) => x.source.toLowerCase() === g.source.toLowerCase());
    const next = i >= 0 ? terms.map((x, j) => (j === i ? g : x)) : [g, ...terms];
    if (await save(next)) setForm(EMPTY);
  };

  const importTsv = async () => {
    try {
      const r = await api.glossaryImport();
      if (r.cancelled) return;
      await load();
      setToast(t("glossary.imported", { added: r.added, updated: r.updated, skipped: r.skipped }));
    } catch (e) {
      setNote({ text: t("glossary.importFailed"), detail: errorMessage(kindOf(e)) });
    }
  };

  const exportTsv = async () => {
    try {
      const text = await api.glossaryTsv();
      const file = await api.exportPutBytes(`${t("glossary.fileName")}.tsv`, new TextEncoder().encode(text));
      const r = await api.deliverExport(file.path, file.name, "save");
      if (r === "saved") setToast(t("glossary.exported"));
    } catch (e) {
      setNote({ text: t("glossary.exportFailed"), detail: errorMessage(kindOf(e)) });
    }
  };

  const shown = useMemo(() => {
    const f = filter.trim().toLowerCase();
    return (terms ?? [])
      .map((g, i) => ({ g, i }))
      .filter(({ g }) => !f || g.source.toLowerCase().includes(f) || g.target.toLowerCase().includes(f));
  }, [terms, filter]);

  return (
    <Screen title={t("glossary.title")} onBack={() => void nav.back()} className="settings">
      <div className="set-inner">
        <p className="set-hint set-about">
          {t("glossary.about")}
          <InfoTip label={t("glossary.title")}>{t("glossary.aboutMore")}</InfoTip>
        </p>
        {note ? (
          <div className="field-error" role="alert">
            {note.text}
            {note.detail ? <Details>{note.detail}</Details> : null}
          </div>
        ) : null}

        <Section title={t("glossary.add")}>
          <div className="set-pad glossary-form">
            <input
              className="input"
              placeholder={t("glossary.source")}
              aria-label={t("glossary.source")}
              value={form.source}
              autoCapitalize="none"
              onChange={(e) => setForm((f) => ({ ...f, source: e.target.value }))}
            />
            <input
              className="input"
              placeholder={t("glossary.target")}
              aria-label={t("glossary.target")}
              value={form.target}
              onChange={(e) => setForm((f) => ({ ...f, target: e.target.value }))}
            />
            <input
              className="input"
              placeholder={t("glossary.note")}
              aria-label={t("glossary.note")}
              value={form.note}
              onChange={(e) => setForm((f) => ({ ...f, note: e.target.value }))}
            />
            <Button primary disabled={!form.source.trim() || !form.target.trim()} onClick={() => void addTerm()}>
              {t("glossary.addButton")}
            </Button>
          </div>
        </Section>

        <div className="set-pad btn-row">
          <Button onClick={() => void importTsv()}>{t("glossary.import")}</Button>
          <Button disabled={!terms?.length} onClick={() => void exportTsv()}>
            {t("glossary.export")}
          </Button>
        </div>

        <Section title={t("glossary.count", { n: terms?.length ?? 0 })}>
          {(terms?.length ?? 0) > 8 ? (
            <div className="set-pad">
              <input
                className="input"
                type="search"
                placeholder={t("glossary.filter")}
                aria-label={t("glossary.filter")}
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
              />
            </div>
          ) : null}
          {terms && terms.length === 0 ? <p className="set-pad muted small">{t("glossary.empty")}</p> : null}
          {shown.map(({ g, i }) =>
            editing === i ? (
              <div key={i} className="set-pad glossary-form">
                <input className="input" aria-label={t("glossary.source")} value={edit.source} onChange={(e) => setEdit((x) => ({ ...x, source: e.target.value }))} />
                <input className="input" aria-label={t("glossary.target")} value={edit.target} onChange={(e) => setEdit((x) => ({ ...x, target: e.target.value }))} />
                <input className="input" aria-label={t("glossary.note")} placeholder={t("glossary.note")} value={edit.note} onChange={(e) => setEdit((x) => ({ ...x, note: e.target.value }))} />
                <div className="btn-row">
                  <Button
                    primary
                    disabled={!edit.source.trim() || !edit.target.trim()}
                    onClick={async () => {
                      if (terms && (await save(terms.map((x, j) => (j === i ? clean(edit) : x))))) setEditing(null);
                    }}
                  >
                    {t("glossary.save")}
                  </Button>
                  <Button
                    onClick={async () => {
                      if (terms && (await save(terms.filter((_, j) => j !== i)))) setEditing(null);
                    }}
                  >
                    {t("glossary.delete")}
                  </Button>
                  <Button onClick={() => setEditing(null)}>{t("common.cancel")}</Button>
                </div>
              </div>
            ) : (
              <button
                key={i}
                type="button"
                className="set-row set-row-btn"
                onClick={() => {
                  setEditing(i);
                  setEdit(g);
                }}
              >
                <span className="set-text">
                  <span className="set-title">
                    {g.source} → {g.target}
                  </span>
                  {g.note ? <span className="set-hint">{g.note}</span> : null}
                </span>
              </button>
            ),
          )}
        </Section>
      </div>
    </Screen>
  );
}
