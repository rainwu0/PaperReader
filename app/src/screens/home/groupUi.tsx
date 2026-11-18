// 群組與譯本共用的面板元件：首頁與文獻資訊都用。面板是導覽堆疊的一層，由擁有它的畫面決定何時畫出來。

import { useState } from "react";
import { AppError, type Group, type TranslationResult } from "../../api";
import { errorDetail, errorMessage, t } from "../../strings";
import { Button, Details, Sheet } from "../../ui/components";

const MAX_NAME = 100;

/** 輸入群組名稱的面板（新增、改名都用）。空白、太長、與別的群組同名會當場說明，不送出。 */
export function GroupNameSheet(props: {
  title: string;
  initial: string;
  submitLabel: string;
  /** 其他群組的名稱（同名不行，不分大小寫）。 */
  otherNames: string[];
  /** 送出：成功就由呼叫的人關面板；失敗丟出 `AppError`，這裡會顯示原因。 */
  onSubmit: (name: string) => Promise<void>;
  onCancel: () => void;
}) {
  const [name, setName] = useState(props.initial);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    const clean = name.trim();
    if (clean === "") return setProblem(t("group.name.empty"));
    if (Array.from(clean).length > MAX_NAME) return setProblem(t("group.name.tooLong"));
    const lower = clean.toLowerCase();
    if (props.otherNames.some((n) => n.trim().toLowerCase() === lower)) {
      return setProblem(t("group.name.duplicate"));
    }
    setProblem(null);
    setBusy(true);
    try {
      await props.onSubmit(clean);
    } catch (e) {
      setProblem(errorMessage(e instanceof AppError ? e.kind : "Unknown"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet open title={props.title} onDismiss={props.onCancel}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <label className="field">
          <span>{t("group.name.label")}</span>
          <input
            className="input"
            autoFocus
            enterKeyHint="done"
            value={name}
            placeholder={t("group.name.placeholder")}
            onChange={(e) => {
              setName(e.target.value);
              setProblem(null);
            }}
          />
        </label>
        {problem ? (
          <p className="field-error" role="alert">
            {problem}
          </p>
        ) : null}
        <div className="actions">
          <Button primary block disabled={busy} onClick={() => void submit()}>
            {props.submitLabel}
          </Button>
          <Button block onClick={props.onCancel}>
            {t("common.cancel")}
          </Button>
        </div>
      </form>
    </Sheet>
  );
}

/** 一篇文獻的群組勾選清單：每個群組一列，勾選就是在群組裡。勾選用方框加勾，不只靠顏色。 */
export function GroupPickList(props: {
  groups: Group[];
  memberOf: string[];
  /** 正在改的那個群組代號（那一列暫時不能再按）。 */
  pending: string | null;
  disabled?: boolean;
  onToggle: (group: Group, member: boolean) => void;
}) {
  return (
    <div className="choice-list" role="group" aria-label={t("group.pick.title")}>
      {props.groups.map((g) => {
        const member = props.memberOf.includes(g.id);
        return (
          <button
            key={g.id}
            type="button"
            role="checkbox"
            aria-checked={member}
            className="choice"
            disabled={props.disabled || props.pending !== null}
            onClick={() => props.onToggle(g, !member)}
          >
            <span className="gcheck" aria-hidden="true">
              {member ? "✓" : ""}
            </span>
            <span className="gname">{g.name}</span>
          </button>
        );
      })}
    </div>
  );
}

/** 加入譯本失敗的說明（加入了、原本就在的，畫面用短暫提示說）。 */
export function TranslationResultSheet(props: {
  result: Pick<TranslationResult, "outcome" | "name" | "error">;
  onDone: () => void;
}) {
  const kind = props.result.error ?? "Unknown";
  const detail = errorDetail(kind);
  return (
    <Sheet open title={t("translation.failed.title")} center onDismiss={props.onDone}>
      <p>{errorMessage(kind)}</p>
      <Details>
        <p>{props.result.name}</p>
        {detail ? <p>{detail}</p> : null}
      </Details>
      <div className="actions">
        <Button primary block onClick={props.onDone}>
          {t("common.confirm")}
        </Button>
      </div>
    </Sheet>
  );
}
