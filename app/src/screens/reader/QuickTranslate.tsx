// 選取文字的快速翻譯：選字之後按「翻譯」，選取上方浮出一張卡片（像程式編輯器的函式說明），
// 放譯文、用的服務（角落的小下拉可以當場換，換了就寫回「快速翻譯用的服務」這一個設定）、「複製」、「劃線並寫進短評」。
//
// - 送出：只送選取的文字與前後各一句（前後文由 `PdfView.selectionContext` 取）；外殼查過的不再送出。
// - 同意：會送出裝置的服務，第一次用時在卡片上說明送到哪裡、送什麼，按「送出」才送；同意記在這台裝置上
//   （服務與主機），設定裡可以重新詢問。設定勾了「送出前不再詢問」的不問。
// - 位置：打開時算一次（`quickText.placeCard`），之後捲動、縮放、點卡片外就收起（導覽堆疊的一層 `quickPop`），
//   不跟著每一格重算（逐格重算會拖慢捲動）。沒有改變任何東西，所以收起不給復原。
// - 離線的 Firefox 翻譯還沒下載模型：卡片上說明並給「下載模型」（下載前同樣先問），下載完卡片還開著就接著翻這一段。

import { useCallback, useEffect, useRef, useState } from "react";
import { api, kindOf, type QuickResult, type TranslateServiceView, type TranslateSettingsView } from "../../api";
import { useViewportSize } from "../../hooks";
import type { Nav } from "../../nav";
import { errorMessage, t, type StringKey } from "../../strings";
import { Button, Spinner } from "../../ui/components";
import { m as Motion, useMotion } from "../../ui/motion";
import { engineOf, serviceName, useEngines } from "../generate/engines";
import { EngineModel, engineState, useOnDevice } from "../generate/localEngine";
import { migrateLegacyConsent } from "../generate/quickConsent";
import { menuServices, missingSetting, quickServiceId, supportsTarget } from "../generate/translateConfig";
import type { SelectionCapture } from "./pdfView";
import { consentKey, joinHyphens, MAX_QUICK_CHARS, placeCard, type Rect } from "./quickText";

/** 打開卡片時帶著的：選取（劃線要用）、它在畫面上的範圍、前後各一句、畫面上方被頂列占掉的高度。 */
export interface QuickTarget {
  cap: SelectionCapture;
  rect: Rect;
  before: string | null;
  after: string | null;
  topLimit: number;
}

/** 卡片開著沒有、開的是哪一段：導覽堆疊的一層，返回鍵、捲動、縮放、點卡片外都收起。 */
export function useQuickTranslate(nav: Nav, sheets: readonly string[]) {
  const [target, setTarget] = useState<QuickTarget | null>(null);
  const layer = sheets.includes("quickPop");
  useEffect(() => {
    if (!layer) setTarget(null);
  }, [layer]);
  const closing = useRef(false);
  const open = useCallback(
    (tg: QuickTarget) => {
      setTarget(tg);
      if (!layer) nav.push({ name: "sheet", kind: "quickPop" });
    },
    [nav, layer],
  );
  /** 收起（捲動時每一格都會來：只退一層）。 */
  const close = useCallback(() => {
    if (!layer || closing.current) return;
    closing.current = true;
    void nav.back().finally(() => {
      closing.current = false;
    });
  }, [nav, layer]);
  return { target: layer ? target : null, open, close, isOpen: layer && target !== null };
}

type Phase =
  | { k: "load" }
  | { k: "consent"; host: string }
  | { k: "busy" }
  | { k: "done"; r: QuickResult }
  | { k: "fail"; why: string; others: TranslateServiceView[] }
  | { k: "long" }
  | { k: "model"; s: TranslateServiceView }
  | { k: "blocked"; why: string };

/** 卡片上的模型下載：同設定裡的下載（先問、進度、停止）；下載好了呼叫 `onReady` 一次。 */
function QuickModel(props: { wifiOnly: boolean; onReady: () => void }) {
  const od = useOnDevice();
  const ready = od.status ? engineState(od.status) === "ready" : false;
  const fired = useRef(false);
  const { onReady } = props;
  useEffect(() => {
    if (!ready || fired.current) return;
    fired.current = true;
    onReady();
  }, [ready, onReady]);
  return (
    <div className="od-model">
      <EngineModel od={od} wifiOnly={props.wifiOnly} />
      {od.note && !od.note.ok ? (
        <p className="field-error" role="alert">
          {od.note.text}
        </p>
      ) : null}
    </div>
  );
}

export function QuickCard(props: {
  paperId: string;
  target: QuickTarget;
  /** 能不能劃線（產生的譯本 PDF 也可以；原頁疊譯文上不會打開卡片）。 */
  canMark: boolean;
  onComment: (text: string) => void;
  onFull: () => void;
  onToast: (message: string) => void;
}) {
  const { target } = props;
  const engines = useEngines();
  const motion = useMotion("pop");
  const vp = useViewportSize();
  // 打開時算一次位置，之後不重算（捲動、縮放就收起）
  const [spot] = useState(() => placeCard(target.rect, vp.w, vp.h, target.topLimit, target.cap.length));
  const [cfg, setCfg] = useState<TranslateSettingsView | null>(null);
  // 已經同意送出的服務與主機：記在設定檔，讀到設定時放進來
  const consentsRef = useRef<string[]>([]);
  const [phase, setPhase] = useState<Phase>({ k: "load" });
  // 換了服務、再試一次：只認最後一次的回覆
  const req = useRef(0);

  useEffect(() => {
    let alive = true;
    api
      .translateSettings()
      .then(migrateLegacyConsent)
      .then((v) => {
        if (!alive) return;
        consentsRef.current = v.quickConsent;
        setCfg(v);
      })
      .catch((e) => alive && setPhase({ k: "blocked", why: errorMessage(kindOf(e)) }));
    return () => {
      alive = false;
      req.current += 1;
    };
  }, []);

  const menu =
    cfg && engines ? menuServices(cfg).filter((s) => supportsTarget(engineOf(engines, s.provider), cfg.targetLang)) : [];
  const chosenId = cfg ? quickServiceId(cfg) : null;
  const svc = cfg?.services.find((s) => s.id === chosenId) ?? null;

  /** 用 `s` 翻；`agreed` 是剛在卡片上按了「送出」。 */
  const run = useCallback(
    async (s: TranslateServiceView, v: TranslateSettingsView, agreed: boolean) => {
      const my = ++req.current;
      if (target.cap.length > MAX_QUICK_CHARS) {
        setPhase({ k: "long" });
        return;
      }
      const info = engineOf(engines, s.provider);
      if (!supportsTarget(info, v.targetLang)) {
        setPhase({ k: "blocked", why: t("quick.unsupported", { name: serviceName(s) }) });
        return;
      }
      const missing = missingSetting(s, info, v.secretsSupported);
      if (missing !== null) {
        setPhase({
          k: "blocked",
          why: t("quick.missing", { name: serviceName(s), why: t(`generate.confirm.why.${missing}` as StringKey) }),
        });
        return;
      }
      // 離線引擎還沒下載模型：先在卡片上給下載（讀不到狀態就照常翻，外殼會說原因）
      if (info?.kind === "on_device") {
        const st = await api.onDeviceStatus().catch(() => null);
        if (my !== req.current) return;
        if (st && engineState(st) === "missing") {
          setPhase({ k: "model", s });
          return;
        }
      }
      const key = consentKey(s.id, s.host);
      if (info?.leavesDevice && !v.skipConfirm && !consentsRef.current.includes(key)) {
        if (!agreed) {
          setPhase({ k: "consent", host: s.host });
          return;
        }
        consentsRef.current = [...consentsRef.current, key];
        // 記不進去只是下次再問一次，不擋這次翻譯
        void api.patchTranslateSettings({ quickConsent: consentsRef.current }).catch(() => {});
      }
      setPhase({ k: "busy" });
      try {
        const r = await api.translateQuick(
          props.paperId,
          joinHyphens(target.cap.exact),
          target.before && joinHyphens(target.before),
          target.after && joinHyphens(target.after),
          s.id,
        );
        if (my !== req.current) return;
        setPhase({ k: "done", r });
      } catch (e) {
        if (my !== req.current) return;
        const others = menuServices(v)
          .filter((x) => x.id !== s.id && supportsTarget(engineOf(engines, x.provider), v.targetLang))
          .slice(0, 2);
        setPhase({ k: "fail", why: errorMessage(kindOf(e)), others });
      }
    },
    [engines, props.paperId, target],
  );

  // 讀好設定與引擎：照「快速翻譯用的服務」翻；換了服務再翻一次
  useEffect(() => {
    if (!cfg || !engines) return;
    if (!svc) {
      setPhase({ k: "blocked", why: t("quick.noService") });
      return;
    }
    void run(svc, cfg, false);
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [cfg, engines, chosenId]);

  /** 換服務（小下拉、「改用…」）：寫回設定（下次照用），再照新的設定翻。`""` 是跟全文翻譯相同。 */
  const pick = async (id: string) => {
    try {
      await api.patchTranslateSettings({ quickService: id });
      const v = await api.translateSettings();
      const same = quickServiceId(v) === chosenId;
      setCfg(v);
      // 換成的實際上是同一個服務（例如「跟全文翻譯相同」就是它）：效果不會再跑，這裡直接再翻一次
      const s = v.services.find((x) => x.id === quickServiceId(v));
      if (same && s && phase.k !== "done") void run(s, v, false);
    } catch (e) {
      setPhase({ k: "fail", why: errorMessage(kindOf(e)), others: [] });
    }
  };

  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      props.onToast(t("quick.copied"));
    } catch {
      props.onToast(t("quick.copyFailed"));
    }
  };

  const served = phase.k === "done" ? (cfg?.services.find((s) => s.id === phase.r.service) ?? null) : null;
  const servedName = phase.k === "done" ? (served ? serviceName(served) : serviceName({ name: "", provider: phase.r.provider })) : "";
  const fullName = (() => {
    if (!cfg) return "—";
    const id = quickServiceId({ ...cfg, quickService: null });
    const s = cfg.services.find((x) => x.id === id);
    return s ? serviceName(s) : "—";
  })();
  const selectValue = cfg?.quickService && menu.some((s) => s.id === cfg.quickService) ? cfg.quickService : "";

  return (
    <Motion.div
      className="seg-pop quick-card"
      role="dialog"
      aria-label={t("quick.card")}
      data-phase={phase.k}
      data-above={spot.above ? "1" : "0"}
      style={{
        left: spot.left,
        width: spot.width,
        maxHeight: spot.maxHeight,
        ...(spot.above ? { bottom: spot.bottom } : { top: spot.top }),
      }}
      onClick={(e) => e.stopPropagation()}
      {...motion}
    >
      <div className="quick-head">
        <p className="quick-src" lang="en">
          {target.cap.exact}
        </p>
        {cfg && menu.length > 0 ? (
          <select
            className="quick-svc"
            aria-label={t("quick.service")}
            value={selectValue}
            onChange={(e) => void pick(e.target.value)}
          >
            <option value="">{t("quick.sameAsFull", { name: fullName })}</option>
            {menu.map((s) => (
              <option key={s.id} value={s.id}>
                {serviceName(s)}
              </option>
            ))}
          </select>
        ) : null}
      </div>

      {phase.k === "load" || phase.k === "busy" ? (
        <p className="muted small quick-status" role="status">
          <Spinner /> {t("quick.loading")}
        </p>
      ) : null}

      {phase.k === "consent" ? (
        <div className="quick-body">
          <p className="quick-note">{t("quick.consent", { host: phase.host })}</p>
          <p className="muted small">{t("quick.consentNote")}</p>
          <div className="seg-pop-actions">
            <Button primary onClick={() => svc && cfg && void run(svc, cfg, true)}>
              {t("quick.send")}
            </Button>
          </div>
        </div>
      ) : null}

      {phase.k === "long" ? (
        <div className="quick-body">
          <p className="quick-note">{t("quick.tooLong", { n: target.cap.length })}</p>
          <div className="seg-pop-actions">
            <Button onClick={props.onFull}>{t("quick.toFull")}</Button>
          </div>
        </div>
      ) : null}

      {phase.k === "model" && cfg ? (
        <div className="quick-body">
          <p className="quick-note">{t("quick.needModel", { name: serviceName(phase.s) })}</p>
          <QuickModel wifiOnly={cfg.wifiOnly} onReady={() => void run(phase.s, cfg, false)} />
        </div>
      ) : null}

      {phase.k === "blocked" ? (
        <p className="field-error quick-note" role="alert">
          {phase.why}
        </p>
      ) : null}

      {phase.k === "fail" ? (
        <div className="quick-body">
          <p className="field-error quick-note" role="alert">
            {t("quick.failed", { reason: phase.why })}
          </p>
          <div className="seg-pop-actions">
            <Button onClick={() => svc && cfg && void run(svc, cfg, false)}>{t("quick.retry")}</Button>
            {phase.others.map((o) => (
              <Button key={o.id} onClick={() => void pick(o.id)}>
                {t("quick.useOther", { name: serviceName(o) })}
              </Button>
            ))}
          </div>
        </div>
      ) : null}

      {phase.k === "done" ? (
        <>
          <p className="seg-pop-text quick-text" data-cached={phase.r.cached ? "1" : "0"} data-service={phase.r.service}>
            {phase.r.text}
          </p>
          {phase.r.offline ? <p className="muted small">{t("quick.offline", { name: servedName })}</p> : null}
          {phase.r.cached ? <p className="muted small">{t("quick.cached")}</p> : null}
          <div className="seg-pop-actions">
            <Button onClick={() => void copy(phase.r.text)}>{t("quick.copy")}</Button>
            {props.canMark ? <Button onClick={() => props.onComment(phase.r.text)}>{t("quick.toComment")}</Button> : null}
          </div>
        </>
      ) : null}
    </Motion.div>
  );
}
