// 產生（譯本、重點筆記）共用的進度元件：進度條、讀原文分段時的畫面、停擺時「要不要改用下一個服務」、
// 用量的詳情。

import { useEffect, useRef, useState } from "react";
import { api, type ServiceUsage, type TranslateServiceView } from "../../api";
import { t } from "../../strings";
import { Button, Details, Screen } from "../../ui/components";

/** 照單價（美元／百萬詞元）粗估的費用；單價沒填是 `null`。 */
function cost(input: number, output: number, svc: TranslateServiceView | undefined): number | null {
  const p = svc?.prices;
  return p ? (input * p.input + output * p.output) / 1_000_000 : null;
}

function money(v: number): string {
  return v >= 1 ? v.toFixed(2) : v.toFixed(v >= 0.01 ? 3 : 4);
}

/**
 * 用量的詳情：這次與這篇（在這支手機上）累計的詞元數，服務設定裡有填單價就附粗估的費用。
 * 服務沒有回報任何用量時不顯示。這篇的累計打開時讀一次：外殼一批一批記下的已經含這次到那時的用量，
 * 之後這次多用的直接加上去（不必每一批重讀）。
 */
export function UsageDetails(props: {
  paperId: string;
  input: number;
  output: number;
  /** 這次用的服務。 */
  service: string | undefined;
  services: TranslateServiceView[];
}) {
  const [stored, setStored] = useState<{ usage: ServiceUsage[]; input: number; output: number } | null>(null);
  const live = useRef({ input: props.input, output: props.output });
  useEffect(() => {
    live.current = { input: props.input, output: props.output };
  });
  useEffect(() => {
    let alive = true;
    api
      .usagePaper(props.paperId)
      .then((u) => alive && setStored({ usage: u, ...live.current }))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [props.paperId]);
  const svc = (id: string | undefined) => props.services.find((s) => s.id === id);
  // 讀的時候這次已經記了多少：之後多出來的加到這次的服務上
  const paper = (stored?.usage ?? []).map((u) => ({ ...u }));
  const more = { input: props.input - (stored?.input ?? props.input), output: props.output - (stored?.output ?? props.output) };
  if (more.input + more.output > 0 && props.service) {
    const row = paper.find((u) => u.service === props.service);
    if (row) {
      row.input += more.input;
      row.output += more.output;
    } else paper.push({ service: props.service, ...more });
  }
  const sum = paper.reduce((a, u) => ({ input: a.input + u.input, output: a.output + u.output }), { input: 0, output: 0 });
  if (props.input + props.output === 0 && sum.input + sum.output === 0) return null;
  const thisCost = cost(props.input, props.output, svc(props.service));
  const costs = paper.map((u) => cost(u.input, u.output, svc(u.service)));
  const paperCost = costs.every((c) => c !== null) ? costs.reduce<number>((a, c) => a + (c ?? 0), 0) : null;
  const n = (v: number) => v.toLocaleString("en-US");
  return (
    <Details label={t("generate.usage.label")}>
      {props.input + props.output > 0 ? (
        <p>
          {t("generate.usage.this", { input: n(props.input), output: n(props.output) })}
          {thisCost !== null ? t("generate.usage.cost", { cost: money(thisCost) }) : null}
        </p>
      ) : null}
      {sum.input + sum.output > 0 ? (
        <p>
          {t("generate.usage.paper", { input: n(sum.input), output: n(sum.output) })}
          {paperCost !== null ? t("generate.usage.cost", { cost: money(paperCost) }) : null}
        </p>
      ) : null}
      <p className="muted small">
        {thisCost !== null || paperCost !== null ? t("generate.usage.costNote") : t("generate.usage.noPrice")}
      </p>
    </Details>
  );
}

/** 進度條：只動 `transform`。 */
export function ProgressBar(props: { n: number; total: number }) {
  const f = props.total ? Math.min(1, props.n / props.total) : 0;
  return (
    <div className="gen-bar" role="progressbar" aria-valuemin={0} aria-valuemax={Math.max(1, props.total)} aria-valuenow={props.n}>
      <div style={{ transform: `scaleX(${f})` }} />
    </div>
  );
}

/** 讀原文、分段的那一段時間：進度與取消。 */
export function AnalyzingScreen(props: {
  title: string;
  status: string;
  n: number;
  total: number;
  onBack: () => void;
  onCancel: () => void;
}) {
  return (
    <Screen title={props.title} onBack={props.onBack}>
      <div className="content">
        <p role="status">{props.status}</p>
        <ProgressBar n={props.n} total={props.total} />
        <div className="gen-actions">
          <Button block onClick={props.onCancel}>
            {t("common.cancel")}
          </Button>
        </div>
      </div>
    </Screen>
  );
}

/** 主要服務停擺時先問要不要改用依序的下一個；送到哪裡寫在問題下面。 */
export function FallbackAsk(props: {
  ask: string;
  sends: string | null;
  yes: string;
  no: string;
  onYes: () => void;
  onNo: () => void;
}) {
  return (
    <section className="gen-fallback" role="alertdialog" aria-label={t("translate.fallback.title")}>
      <h2 className="gen-heading">{props.ask}</h2>
      {props.sends ? <p>{props.sends}</p> : null}
      <div className="gen-actions">
        <Button primary block onClick={props.onYes}>
          {props.yes}
        </Button>
        <Button block onClick={props.onNo}>
          {props.no}
        </Button>
      </div>
    </section>
  );
}
