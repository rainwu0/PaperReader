// 「檢查譯文」的並排比較：選一段或那一頁，讓兩三個服務各翻一次並排看，挑一個套用到那一段（做法參考
// zotero-pdf-translate 的並列比較）。文字會送出裝置時，在面板上說送到哪裡、要你同意。
// 匯出給翻譯軟體（XLIFF、TMX）是匯出面板的一項（`export/exportUi.tsx`）。

import { useEffect, useState } from "react";
import { api, kindOf, type CompareColumn, type OverlaySegment, type TranslateSettingsView } from "../../api";
import type { Nav } from "../../nav";
import { errorMessage, t } from "../../strings";
import { Button, CheckRow, Details, Sheet, Spinner } from "../../ui/components";
import { engineOf, serviceName, useEngines } from "../generate/engines";
import { menuServices, missingSetting } from "../generate/translateConfig";

export interface CompareTarget {
  /** 要比較的段落（一段，或那一頁有原文的全部段落）。 */
  segments: OverlaySegment[];
  page: number;
  scope: "segment" | "page";
}

export function CompareServicesSheet(props: {
  open: boolean;
  nav: Nav;
  paperId: string;
  materialId: string;
  target: CompareTarget | null;
  /** 套用了一段（譯文要重新讀）。 */
  onApplied: () => void;
}) {
  const { nav, target } = props;
  const engines = useEngines();
  const [cfg, setCfg] = useState<TranslateSettingsView | null>(null);
  const [picked, setPicked] = useState<string[]>([]);
  const [consent, setConsent] = useState(false);
  const [running, setRunning] = useState(false);
  const [cols, setCols] = useState<CompareColumn[] | null>(null);
  const [applied, setApplied] = useState<Record<string, string>>({});
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    if (!props.open) return;
    setCols(null);
    setApplied({});
    setFailure(null);
    setConsent(false);
    api
      .translateSettings()
      .then((c) => {
        setCfg(c);
        // 預設：主要服務加上第一個備用（或下一個啟用的），只挑設定好的雲端服務（離線引擎的模型可能還沒下載）
        const ready = menuServices(c).filter((s) => {
          const info = engineOf(engines, s.provider);
          return info?.kind !== "on_device" && missingSetting(s, info, c.secretsSupported) === null;
        });
        setPicked(ready.slice(0, 2).map((s) => s.id));
      })
      .catch((e) => setFailure(errorMessage(kindOf(e))));
  }, [props.open, engines]);

  const menu = cfg ? menuServices(cfg) : [];
  const chosen = menu.filter((s) => picked.includes(s.id));
  const hosts = [...new Set(chosen.map((s) => s.host).filter(Boolean))];
  const needConsent = hosts.length > 0 && !!cfg && !cfg.skipConfirm && !consent;

  const toggle = (id: string, on: boolean) =>
    setPicked((p) => (on ? (p.length >= 3 ? p : [...p, id]) : p.filter((x) => x !== id)));

  const run = async () => {
    if (!target || running) return;
    setRunning(true);
    setFailure(null);
    try {
      setCols(
        await api.translateCompare(
          props.paperId,
          props.materialId,
          target.segments.map((s) => s.id),
          picked,
        ),
      );
    } catch (e) {
      setFailure(errorMessage(kindOf(e)));
    } finally {
      setRunning(false);
    }
  };

  const apply = async (segId: string, text: string, service: string) => {
    try {
      await api.overlayApply(props.paperId, props.materialId, segId, text, service);
      setApplied((a) => ({ ...a, [segId]: text }));
      props.onApplied();
    } catch (e) {
      setFailure(errorMessage(kindOf(e)));
    }
  };

  const svcOf = (id: string) => cfg?.services.find((s) => s.id === id);

  return (
    <Sheet
      open={props.open}
      title={target?.scope === "page" ? t("compare.titlePage", { n: (target?.page ?? 0) + 1 }) : t("compare.title")}
      onDismiss={() => void nav.back()}
    >
      <div className="cmp-sheet">
        {cols === null ? (
          <>
            <p className="muted small">{t("compare.pick")}</p>
            {menu.map((s) => {
              const miss = missingSetting(s, engineOf(engines, s.provider), cfg?.secretsSupported ?? false);
              return (
                <CheckRow
                  key={s.id}
                  checked={picked.includes(s.id)}
                  disabled={miss !== null || (!picked.includes(s.id) && picked.length >= 3)}
                  onChange={(on) => toggle(s.id, on)}
                  hint={miss ? t(`generate.confirm.why.${miss}`) : undefined}
                >
                  {serviceName(s)}
                </CheckRow>
              );
            })}
            {hosts.length > 0 ? (
              <p className="muted small">{t("compare.sends", { n: target?.segments.length ?? 0, hosts: hosts.join("、") })}</p>
            ) : null}
            {hosts.length > 0 && cfg && !cfg.skipConfirm ? (
              <CheckRow checked={consent} onChange={setConsent}>
                {t("compare.consent")}
              </CheckRow>
            ) : null}
            <div className="actions">
              <Button primary block disabled={picked.length < 2 || needConsent || running} onClick={() => void run()}>
                {running ? (
                  <>
                    <Spinner /> {t("compare.running")}
                  </>
                ) : (
                  t("compare.run", { n: picked.length })
                )}
              </Button>
            </div>
          </>
        ) : (
          <div className="cmp-results">
            {target?.segments.map((seg, i) => (
              <section key={seg.id} className="cmp-seg">
                <p className="compare-src" lang="en">
                  {seg.source}
                </p>
                {cols.map((c) => {
                  const text = c.texts[i];
                  const svc = svcOf(c.service);
                  const isApplied = applied[seg.id] !== undefined && applied[seg.id] === text;
                  return (
                    <div key={c.service} className="cmp-col">
                      <span className="cmp-name">{svc ? serviceName(svc) : c.service}</span>
                      {text ? (
                        <>
                          <p className="cmp-text">{text}</p>
                          <Button primary={!isApplied} disabled={isApplied} onClick={() => void apply(seg.id, text, c.service)}>
                            {isApplied ? t("compare.applied") : t("compare.apply")}
                          </Button>
                        </>
                      ) : (
                        <p className="field-error">{c.error ? errorMessage(c.error) : t("reader.compare.failed")}</p>
                      )}
                    </div>
                  );
                })}
              </section>
            ))}
            <div className="actions">
              <Button block onClick={() => void nav.back()}>
                {t("export.done")}
              </Button>
            </div>
          </div>
        )}
        {failure ? (
          <div className="field-error" role="alert">
            {t("compare.failed")}
            <Details>{failure}</Details>
          </div>
        ) : null}
      </div>
    </Sheet>
  );
}
