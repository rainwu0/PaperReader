// 引擎申報（外殼 `translate_engines`，`app/translate/src/engines.rs`）：讀一次就記住，申報是程式裡固定的。

import { useEffect, useState } from "react";
import { api, type EngineInfo, type SegmentService, type TranslateServiceView } from "../../api";
import { t, type StringKey } from "../../strings";

let cache: EngineInfo[] | null = null;
let pending: Promise<EngineInfo[]> | null = null;

/** 全部引擎的申報（讀一次就記住；申報是程式裡固定的，不會在執行中變）。 */
export function loadEngines(): Promise<EngineInfo[]> {
  if (cache) return Promise.resolve(cache);
  pending ??= api.translateEngines().then((list) => (cache = list));
  return pending;
}

export function useEngines(): EngineInfo[] | null {
  const [list, setList] = useState<EngineInfo[] | null>(cache);
  useEffect(() => {
    if (list) return;
    let alive = true;
    loadEngines()
      .then((l) => alive && setList(l))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [list]);
  return list;
}

export function engineOf(list: EngineInfo[] | null, provider: string): EngineInfo | null {
  return list?.find((e) => e.provider === provider) ?? null;
}


/** 服務顯示的名稱：使用者取的，沒有就用引擎的名稱。 */
export function serviceName(s: Pick<TranslateServiceView, "name" | "provider">): string {
  return s.name.trim() !== "" ? s.name.trim() : t(`settings.generate.provider.${s.provider}` as StringKey);
}

/** 翻一段的服務的名稱：這台的設定裡有那一筆就用它的名稱，沒有（別台翻的、已刪掉）就用引擎與模型。 */
export function segmentServiceName(
  s: SegmentService,
  services: ReadonlyArray<Pick<TranslateServiceView, "id" | "name" | "provider">> | null,
): string {
  const svc = services?.find((x) => x.id === s.id);
  if (svc) return serviceName(svc);
  const base = t(`settings.generate.provider.${s.provider}` as StringKey);
  return s.model ? `${base}（${s.model}）` : base;
}

/** 一份譯本已翻的段落用過的服務（依第一次出現的順序，不重複）。 */
export function servicesUsed(segments: ReadonlyArray<{ status: string; service?: SegmentService | null }>): SegmentService[] {
  const seen = new Map<string, SegmentService>();
  for (const s of segments) {
    if (s.status === "done" && s.service) seen.set(`${s.service.id}|${s.service.provider}|${s.service.model}`, s.service);
  }
  return [...seen.values()];
}
