// 要下載才有的模型檔的共用部分：狀態、下載（先問）、停止與刪除（`useModelFile`、`ModelFileControls`）。
// 離線翻譯（Firefox 翻譯）的語言模型：設定（「產生」區的「離線翻譯模型」與服務頁）、翻譯面板、選字的快速翻譯用；
// 版面模型：設定的「產生」區用。

import { useCallback, useEffect, useRef, useState } from "react";
import { useAlive, usePoll } from "../../hooks";
import { api, kindOf, type EngineInfo, type LocalEngine, type OnDeviceStatus } from "../../api";
import { errorMessage, t, type StringKey } from "../../strings";
import { Button } from "../../ui/components";
import { formatBytes } from "../settings/useBackup";
import { engineOf, useEngines } from "./engines";
import { ProgressBar } from "./JobProgress";
import { supportsTarget } from "./translateConfig";

/** 離線引擎（目前只有 Firefox 翻譯）。 */
export const LOCAL_ENGINE: LocalEngine = "bergamot";

/** 目標語言的模型下載了沒有、有沒有這個語言組合。 */
export function engineState(s: OnDeviceStatus): "ready" | "missing" | "noLang" {
  if (s.bergamotReady === null) return "noLang";
  return s.bergamotReady ? "ready" : "missing";
}

/** 目標語言對到的 Firefox 翻譯模型（照引擎申報的語言對照表：`zh-TW` → `zh-Hant`）。 */
function bergamotModel(s: OnDeviceStatus, info: EngineInfo | null) {
  const target = info?.targets.find((l) => l.code.toLowerCase() === s.targetLang.toLowerCase())?.engineCode;
  return s.bergamot.find((m) => m.target === target) ?? null;
}

/**
 * 目前目標語言的模型：狀態（支援哪些目標語言照引擎申報）、狀態那一句、要下載的大小、模型本身。
 * 設定的「離線翻譯模型」那一列、翻譯面板、選字的快速翻譯共用同一套說法。
 */
export function engineSummary(s: OnDeviceStatus, info: EngineInfo | null) {
  const state = info && !supportsTarget(info, s.targetLang) ? "noLang" : engineState(s);
  const bm = bergamotModel(s, info);
  const size = formatBytes(bm?.downloadBytes ?? 0);
  const line =
    state === "noLang"
      ? t("ondevice.noLang", { lang: t(`settings.generate.lang.${s.targetLang}` as StringKey) })
      : state === "ready" && bm
        ? t("ondevice.ready", { size: formatBytes(bm.bytes) })
        : t("ondevice.notReady", { size });
  return { state, line, size, bm };
}

/**
 * 一個要下載才有的模型檔的操作：離線翻譯的語言模型與版面模型共用同一套狀態、下載（先問）、停止、刪除。
 * 要是模組層的常數，hook 才不會每次重做。
 */
export interface ModelFileOps<S> {
  status: () => Promise<S>;
  download: (wifiOnly: boolean) => Promise<unknown>;
  cancel: () => Promise<unknown>;
  /** 刪掉一個模型；只有一個模型的不看 `id`。 */
  remove: (id: string) => Promise<unknown>;
}

const ONDEVICE: ModelFileOps<OnDeviceStatus> = {
  status: () => api.onDeviceStatus(),
  download: (wifiOnly) => api.onDeviceDownload(LOCAL_ENGINE, wifiOnly),
  cancel: () => api.onDeviceDownloadCancel(),
  remove: (model) => api.onDeviceDelete(LOCAL_ENGINE, model),
};

/** 離線翻譯（Firefox 翻譯）的語言模型。 */
export const useOnDevice = () => useModelFile(ONDEVICE);

/** 狀態與下載、刪除。下載中每隔一下更新進度。 */
export function useModelFile<S>(ops: ModelFileOps<S>) {
  const [status, setStatus] = useState<S | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);
  const alive = useAlive();
  /** 使用者按了「停止下載」：下載交回的錯誤不當成失敗說明。 */
  const stopped = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const s = await ops.status();
      if (alive.current) setStatus(s);
    } catch {
      // 讀不到就維持原狀
    }
  }, [alive, ops]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // 下載中每隔一下更新進度
  usePoll(busy, refresh, 700);

  /** 下載；交回有沒有下載完（停止、失敗是 `false`）。交回時狀態已經重讀過。 */
  const download = async (wifiOnly: boolean): Promise<boolean> => {
    if (busy) return false;
    setBusy(true);
    setNote(null);
    stopped.current = false;
    let ok = false;
    try {
      await ops.download(wifiOnly);
      ok = true;
      if (alive.current) setNote({ ok: true, text: t("ondevice.downloaded") });
    } catch (e) {
      if (alive.current && !stopped.current) setNote({ ok: false, text: t("ondevice.failed", { reason: errorMessage(kindOf(e)) }) });
    } finally {
      if (alive.current) setBusy(false);
      await refresh();
    }
    return ok;
  };

  const remove = async (model: string) => {
    if (busy) return;
    setNote(null);
    try {
      await ops.remove(model);
      if (alive.current) setNote({ ok: true, text: t("ondevice.deleted") });
    } catch (e) {
      if (alive.current) setNote({ ok: false, text: t("ondevice.failed", { reason: errorMessage(kindOf(e)) }) });
    }
    await refresh();
  };

  const stop = () => {
    stopped.current = true;
    void ops.cancel();
  };

  return { status, busy, note, refresh, download, remove, stop };
}

type OnDevice = ReturnType<typeof useOnDevice>;

/** 下載前的確認：連到哪裡、多大、不送出什麼；按「下載」才下載。 */
export function ModelAsk(props: { title: string; body: string; onConfirm: () => void; onCancel: () => void }) {
  return (
    <div className="od-ask" role="group" aria-label={props.title}>
      <p className="small">{props.body}</p>
      <div className="btn-row">
        <Button primary onClick={props.onConfirm}>
          {t("ondevice.ask.confirm")}
        </Button>
        <Button onClick={props.onCancel}>{t("switch.intro.decline")}</Button>
      </div>
    </div>
  );
}

/** 下載中：已下載多少、進度條、「停止下載」。 */
export function ModelProgress(props: { progress: { done: number; total: number }; onStop: () => void }) {
  const dl = props.progress;
  return (
    <>
      <p className="muted small" role="status">
        {t("ondevice.downloading", { done: formatBytes(dl.done), total: formatBytes(dl.total) })}
      </p>
      <ProgressBar n={dl.done} total={dl.total} />
      <div className="btn-row">
        <Button onClick={props.onStop}>{t("ondevice.cancelDownload")}</Button>
      </div>
    </>
  );
}

/**
 * 一個模型檔的控制項：狀態那一句、下載（先說明連到哪裡、多大，按「下載」才下載）、進度與停止、刪除。
 * 離線翻譯的語言模型與版面模型共用。
 */
export function ModelFileControls(props: {
  /** 狀態那一句（已下載多大、還沒下載約多大）。 */
  line: string;
  /** 還沒下載、可以下載。 */
  missing: boolean;
  /** 下載前的確認：讀屏標籤與內容（連到哪裡、多大、不送出什麼）。 */
  askTitle: string;
  askBody: string;
  busy: boolean;
  progress: { done: number; total: number };
  onDownload: () => void;
  onStop: () => void;
  /** 有東西可以刪時給。刪了不先問：要用時可以再下載，刪掉後的那一句會這樣說。 */
  onDelete?: () => void;
}) {
  const [asking, setAsking] = useState<"download" | null>(null);
  return (
    <>
      <p className="muted small" role="status">
        {props.line}
      </p>
      {props.busy ? <ModelProgress progress={props.progress} onStop={props.onStop} /> : null}
      {props.missing && !props.busy ? (
        asking === "download" ? (
          <ModelAsk
            title={props.askTitle}
            body={props.askBody}
            onConfirm={() => {
              setAsking(null);
              props.onDownload();
            }}
            onCancel={() => setAsking(null)}
          />
        ) : (
          <div className="btn-row">
            <Button primary onClick={() => setAsking("download")}>
              {t("ondevice.download")}
            </Button>
          </div>
        )
      ) : null}
      {props.onDelete && !props.busy ? (
        <div className="btn-row">
          <Button onClick={props.onDelete}>{t("ondevice.delete")}</Button>
        </div>
      ) : null}
    </>
  );
}

/** Firefox 翻譯的模型：目前目標語言的那一個，加上其他已下載、可以刪的。 */
export function EngineModel(props: { od: OnDevice; wifiOnly: boolean; canDelete?: boolean }) {
  const { od } = props;
  const s = od.status;
  const info = engineOf(useEngines(), LOCAL_ENGINE);
  if (!s) return null;
  const { state, line, size, bm } = engineSummary(s, info);
  // 已下載、但不是目前目標語言的其他模型（可以刪）
  const others = s.bergamot.filter((m) => m.downloaded && m.id !== bm?.id).map((m) => m.id);

  return (
    <div className="od-model">
      <ModelFileControls
        line={line}
        missing={state === "missing"}
        askTitle={t("ondevice.ask.title")}
        askBody={t("ondevice.ask.body", { host: s.bergamotHost, size })}
        busy={od.busy}
        progress={s.download}
        onDownload={() => void od.download(props.wifiOnly)}
        onStop={od.stop}
        {...(props.canDelete && state === "ready" && bm ? { onDelete: () => void od.remove(bm.id) } : {})}
      />
      {props.canDelete && others.length > 0 ? (
        <>
          <p className="muted small">{t("ondevice.otherModels", { list: others.join("、") })}</p>
          <div className="btn-row">
            {others.map((m) => (
              <Button key={m} onClick={() => void od.remove(m)}>
                {t("ondevice.deleteOther", { name: m })}
              </Button>
            ))}
          </div>
        </>
      ) : null}
    </div>
  );
}
