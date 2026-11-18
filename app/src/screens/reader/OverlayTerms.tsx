// 「檢查譯文」裡的「這篇的術語」：翻譯時學到的原文詞與譯法看得到、改得了、刪得掉。
// 術語記在那份譯文疊加檔裡：改了之後，還沒翻的段落、接著翻與同一篇的重點筆記照新的用；
// 已翻好的段落不會自動重翻（畫面上寫明）。使用者的術語庫照舊優先。

import { useState } from "react";
import { api, kindOf, type OverlayTerm } from "../../api";
import { errorMessage, t } from "../../strings";
import { Button, Details } from "../../ui/components";

const inUse = (s: string) => s === "learned" || s === "user";

export function OverlayTerms(props: {
  paperId: string;
  overlayId: string;
  terms: OverlayTerm[];
  /** 存好之後重讀這份譯本。 */
  onSaved: () => Promise<void> | void;
}) {
  const shown = props.terms.filter((x) => inUse(x.state));
  // 刪掉的：收在下面，可以放回
  const removed = props.terms.filter((x) => x.state === "removed");
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);

  const write = async (next: OverlayTerm[], done = t("reader.check.terms.saved")) => {
    if (busy) return;
    setBusy(true);
    setNote(null);
    try {
      await api.setOverlayTerms(props.paperId, props.overlayId, next);
      setDrafts({});
      setNote({ ok: true, text: done });
      await props.onSaved();
    } catch (e) {
      setNote({ ok: false, text: `${t("reader.check.terms.failed")} ${errorMessage(kindOf(e))}` });
    } finally {
      setBusy(false);
    }
  };

  /** 改譯法：成了使用者定的，學到的不再蓋掉它。 */
  const save = (source: string) =>
    void write(
      props.terms.map((x) =>
        x.source === source ? { ...x, target: (drafts[source] ?? x.target).trim(), state: "user", batches: 0 } : x,
      ),
    );
  /** 刪掉：不用，之後也不再學這個詞。 */
  const remove = (source: string) =>
    void write(props.terms.map((x) => (x.source === source ? { ...x, state: "removed", batches: 0 } : x)));
  /** 放回：照刪掉之前的譯法，當成使用者定的（學到的不再蓋掉它；原本是學到的還是你改的，檔裡沒有記）。 */
  const restore = (source: string) =>
    void write(
      props.terms.map((x) => (x.source === source ? { ...x, state: "user", batches: 0 } : x)),
      t("reader.check.terms.restored"),
    );

  return (
    <Details label={t("reader.check.terms.title", { n: shown.length })}>
      <p className="muted small">{t("reader.check.terms.about")}</p>
      {shown.length === 0 ? <p className="muted small">{t("reader.check.terms.empty")}</p> : null}
      {shown.map((x) => {
        const value = drafts[x.source] ?? x.target;
        const changed = value.trim() !== x.target && value.trim() !== "";
        return (
          <div key={x.source} className="cmp-seg">
            <label className="field">
              <span>
                {x.source}
                {x.state === "user" ? <span className="svc-badge">{t("reader.check.terms.mine")}</span> : null}
              </span>
              <input
                className="input"
                aria-label={t("reader.check.terms.target", { source: x.source })}
                value={value}
                onChange={(e) => setDrafts((d) => ({ ...d, [x.source]: e.target.value }))}
              />
            </label>
            <div className="btn-row">
              <Button primary disabled={busy || !changed} onClick={() => save(x.source)}>
                {t("reader.check.terms.save")}
              </Button>
              <Button disabled={busy} onClick={() => remove(x.source)}>
                {t("reader.check.terms.delete")}
              </Button>
            </div>
          </div>
        );
      })}
      {removed.length > 0 ? (
        <Details label={t("reader.check.terms.removed", { n: removed.length })}>
          <p className="muted small">{t("reader.check.terms.removedAbout")}</p>
          {removed.map((x) => (
            <div key={x.source} className="cmp-seg btn-row">
              <span>
                {x.source} → {x.target}
              </span>
              <Button disabled={busy} onClick={() => restore(x.source)}>
                {t("reader.check.terms.restore")}
              </Button>
            </div>
          ))}
        </Details>
      ) : null}
      {note ? (
        <p className={note.ok ? "gen-status" : "field-error"} role={note.ok ? "status" : "alert"}>
          {note.text}
        </p>
      ) : null}
    </Details>
  );
}
