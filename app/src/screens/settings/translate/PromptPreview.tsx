// 看完整提示詞：把實際送出的內容分段原樣列出，使用者改不到的標「固定」。
// 內容由外殼用和翻譯、重點筆記實際送出同一組函式組出（`prompt_preview`），逐字相同；設定的編輯畫面與產生面板共用。
// 一次翻譯可能是兩次請求（開了第二輪潤飾），每一次各列一段。放在「詳情」裡（第二層），打開時才組。

import { useEffect, useState } from "react";
import { api, kindOf, type PromptPart, type PromptPreset, type PromptPreview } from "../../../api";
import { errorMessage, hasText, t, type StringKey } from "../../../strings";
import { Spinner } from "../../../ui/components";

function Part(props: { part: PromptPart }) {
  const { part } = props;
  const empty = part.text.trim() === "";
  return (
    <div className="prompt-part">
      <p className="field-label">
        {t(`prompts.part.${part.key}` as StringKey)}
        <span className="svc-badge">{part.fixed ? t("prompts.fixed") : t("prompts.editable")}</span>
      </p>
      {/* 固定的部分這一次是空的（範例原文沒有術語、只有一部分）：照樣列出，說明什麼時候才有 */}
      {empty ? (
        <p className="muted small">
          {t(hasText(`prompts.empty.${part.key}`) ? (`prompts.empty.${part.key}` as StringKey) : "prompts.empty")}
        </p>
      ) : (
        <pre className="prompt-text">{part.text}</pre>
      )}
    </div>
  );
}

/** 可改的部分是空的不列（存檔時已檢查，不會發生）；固定的部分空的也列出。 */
const shown = (parts: PromptPart[]) => parts.filter((p) => p.fixed || p.text.trim() !== "");

function Request(props: { request: PromptPreview; many: boolean }) {
  const { request } = props;
  return (
    <>
      {props.many ? <h3 className="gen-heading">{t(`prompts.request.${request.kind}` as StringKey)}</h3> : null}
      <p className="gen-heading">{t("prompts.system")}</p>
      {shown(request.system).map((p, i) => (
        <Part key={`s${i}`} part={p} />
      ))}
      <p className="gen-heading">{t("prompts.user")}</p>
      {shown(request.user).map((p, i) => (
        <Part key={`u${i}`} part={p} />
      ))}
    </>
  );
}

/**
 * `preset` 可以是還沒存的草稿；`paperId` 給了就代入這篇的篇名與術語；`materialId` 是接著翻的那份譯本（用它的語言與術語）；
 * `refine` 開著時多列第二輪潤飾那一次。
 */
export function PromptPreviewBox(props: {
  preset: PromptPreset;
  paperId: string | null;
  materialId?: string | null;
  refine?: boolean;
}) {
  const [state, setState] = useState<
    { phase: "loading" } | { phase: "ready"; data: PromptPreview[] } | { phase: "failed"; why: string }
  >({ phase: "loading" });
  const key = JSON.stringify([props.preset, props.paperId, props.materialId ?? null, !!props.refine]);
  useEffect(() => {
    let alive = true;
    const [preset, paperId, materialId, refine] = JSON.parse(key) as [PromptPreset, string | null, string | null, boolean];
    api
      .previewPrompt(preset, paperId, materialId, refine)
      .then((data) => alive && setState({ phase: "ready", data }))
      .catch((e) => alive && setState({ phase: "failed", why: errorMessage(kindOf(e)) }));
    return () => {
      alive = false;
    };
  }, [key]);

  if (state.phase === "loading") {
    return (
      <p className="muted small" role="status">
        <Spinner /> {t("prompts.fullLoading")}
      </p>
    );
  }
  if (state.phase === "failed") {
    return (
      <p className="field-error">
        {t("prompts.fullFailed")} {state.why}
      </p>
    );
  }
  const many = state.data.length > 1;
  return (
    <div className="prompt-preview">
      <p className="muted small">{t("prompts.fullAbout")}</p>
      {state.data.map((r, i) => (
        <Request key={i} request={r} many={many} />
      ))}
    </div>
  );
}
