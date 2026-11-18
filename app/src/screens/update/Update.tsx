// 檢查更新（只有 Android）：同意 → 檢查 → 看變更摘要與大小 → 更新（下載、核對、交給系統安裝）。
//
// 沒同意之前這一頁只有說明與兩個按鈕，一個請求都不送（連到哪裡、送出什麼留在做決定的畫面上）。
// 行動數據下先說大小再下載。第一次更新要允許「安裝不明應用程式」：說明、帶到系統頁，回到前景接著裝。
// 交給系統之後應用會被系統結束（裝好要再打開），所以按「更新」之前就說。

import { useRef, useState } from "react";
import { api, kindOf, type ErrorKind } from "../../api";
import { useAlive, useForeground, usePoll } from "../../hooks";
import type { Nav, SheetKind } from "../../nav";
import { t } from "../../strings";
import { BusyOverlay, Button, ConfirmSheet, KindNotice, Screen } from "../../ui/components";
import { AnimatePresence } from "../../ui/motion";
import { updates, useUpdates } from "../../updates";
import { ProgressBar } from "../generate/JobProgress";
import { Section, SettingRow, SwitchRow } from "../settings/parts";
import { formatBytes } from "../settings/useBackup";

type Phase = "idle" | "checking" | "downloading" | "installing" | "waiting";

function lastCheckText(secs: number | null): string | null {
  if (!secs) return null;
  const d = new Date(secs * 1000);
  return t("update.lastCheck", { when: d.toLocaleString() });
}

export function Update(props: { nav: Nav; sheet: SheetKind | null }) {
  const { nav, sheet } = props;
  const s = useUpdates();
  const alive = useAlive();
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState<ErrorKind | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const cancelled = useRef(false);
  // 去系統頁允許「安裝不明應用程式」了：回到前景接著裝
  const waitingGrant = useRef(false);

  const fresh = () => {
    setError(null);
    setInfo(null);
  };

  usePoll(phase === "downloading", () => updates.refresh().then(() => {}), 400, 0);
  // 交給系統之後：使用者在確認畫面按了取消、或系統沒裝成，才會回到這裡（裝成功時應用已被結束）
  usePoll(
    phase === "waiting",
    async () => {
      const st = await api.updateInstallStatus().catch(() => "waiting" as const);
      if (!alive.current || st === "waiting") return;
      setPhase("idle");
      if (st === "cancelled") setInfo(t("update.installCancelled"));
      else setError("UpdateInstall");
      return true;
    },
    1000,
  );

  const check = async () => {
    fresh();
    setPhase("checking");
    try {
      const found = await api.updateCheck(true);
      await updates.refresh();
      if (alive.current && !found) setInfo(t("update.upToDate"));
    } catch (e) {
      if (alive.current) setError(kindOf(e));
    } finally {
      if (alive.current) setPhase("idle");
    }
  };

  const agree = async () => {
    try {
      await api.updateConfigure({ consented: true, autoCheck: true, invited: true });
    } catch (e) {
      setError(kindOf(e));
      return;
    }
    await updates.refresh();
    await check();
  };

  const decline = async () => {
    await api.updateConfigure({ invited: true }).catch(() => {});
    await updates.refresh();
    await nav.back();
  };

  /** 收回同意：外殼把設定回到沒同意的狀態、刪掉下載的檔；之後一個請求都不送。 */
  const revoke = async () => {
    fresh();
    try {
      await api.updateConfigure({ consented: false });
    } catch (e) {
      setError(kindOf(e));
      return;
    }
    await updates.refresh();
    if (alive.current) setInfo(t("update.revoked"));
  };

  const setAuto = async (on: boolean) => {
    await api.updateConfigure({ autoCheck: on }).catch((e: unknown) => setError(kindOf(e)));
    await updates.refresh();
  };

  /** 交給系統。`afterGrant`：剛從系統的授權頁回來，還是沒允許就說明，不再開說明框。 */
  const install = async (afterGrant = false) => {
    fresh();
    setPhase("installing");
    try {
      const r = await api.updateInstall();
      if (!alive.current) return;
      if (r === "needs_permission") {
        setPhase("idle");
        if (afterGrant) setInfo(t("update.permission.denied"));
        else nav.push({ name: "sheet", kind: "updatePermission" });
        return;
      }
      setPhase("waiting");
    } catch (e) {
      if (!alive.current) return;
      setPhase("idle");
      setError(kindOf(e));
      await updates.refresh();
    }
  };

  const download = async () => {
    fresh();
    cancelled.current = false;
    setPhase("downloading");
    try {
      await api.updateDownload();
      await updates.refresh();
      if (!alive.current) return;
      await install();
    } catch (e) {
      if (!alive.current) return;
      setPhase("idle");
      if (cancelled.current) setInfo(t("update.cancelled"));
      else setError(kindOf(e));
      await updates.refresh();
    }
  };

  const startUpdate = () => {
    if (!s?.available) return;
    if (s.downloaded === s.available.version) {
      void install();
      return;
    }
    // 行動數據：先說大小再下載
    if (s.unmetered === false) nav.push({ name: "sheet", kind: "updateMetered" });
    else void download();
  };

  const cancelDownload = () => {
    cancelled.current = true;
    void api.updateDownloadCancel().catch(() => {});
  };

  const grant = async () => {
    await nav.back(); // 關掉說明
    waitingGrant.current = true;
    await api.updateOpenInstallSettings().catch((e: unknown) => {
      waitingGrant.current = false;
      setError(kindOf(e));
    });
  };

  useForeground(() => {
    if (!waitingGrant.current) return;
    waitingGrant.current = false;
    void install(true);
  });

  const discard = async () => {
    await api.updateDiscard().catch(() => {});
    await updates.refresh();
  };

  const close = () => void nav.back();
  const a = s?.available ?? null;
  const dl = s?.download;
  const pct = dl && dl.total > 0 ? Math.min(100, Math.floor((dl.done * 100) / dl.total)) : null;
  const busy = phase === "checking" || phase === "installing";

  return (
    <Screen title={t("update.title")} onBack={() => void nav.back()} className="settings">
      <div className="set-inner">
        {s && !s.consented ? (
          <div className="set-pad" data-testid="update-consent">
            <p>{t("update.consent.lead")}</p>
            <p>{t("update.consent.where", { host: s.host })}</p>
            <p>{t("update.consent.download")}</p>
            <p className="muted small">{t("update.consent.off")}</p>
            <div className="actions">
              <Button primary block onClick={() => void agree()}>
                {t("update.consent.agree")}
              </Button>
              <Button block onClick={() => void decline()}>
                {t("update.consent.decline")}
              </Button>
            </div>
          </div>
        ) : null}

        {s && s.consented ? (
          <>
            <Section title={t("update.title")}>
              <SettingRow title={t("update.current")} value={s.current} />
              <SwitchRow
                title={t("update.auto")}
                hint={t("update.auto.hint")}
                checked={s.autoCheck}
                onChange={(v) => void setAuto(v)}
              />
              <SettingRow
                title={t("update.checkNow")}
                hint={lastCheckText(s.lastCheck) ?? undefined}
                chevron
                onClick={() => {
                  if (phase === "idle") void check();
                }}
              />
              <SettingRow
                title={t("update.revoke")}
                hint={t("update.revoke.hint")}
                onClick={() => {
                  // 下載或安裝進行中不收回（先取消下載）
                  if (phase === "idle") void revoke();
                }}
              />
            </Section>

            {a ? (
              <section className="set-section" data-testid="update-available">
                <h2 className="set-heading">{t("update.new", { version: a.version })}</h2>
                <div className="set-card set-pad">
                  <p className="muted small">{t("update.size", { size: formatBytes(a.size) })}</p>
                  <p className="license-text">{a.notes || t("update.notes.empty")}</p>
                  <p className="muted small">{t("update.closes")}</p>
                  {phase === "downloading" ? (
                    <div role="status" data-testid="update-progress">
                      <p>
                        {pct === 100
                          ? t("update.verifying")
                          : pct !== null
                            ? t("update.downloadingPct", { pct })
                            : t("update.downloading")}
                      </p>
                      <ProgressBar n={dl?.done ?? 0} total={dl?.total ?? 0} />
                      <div className="actions">
                        <Button block onClick={cancelDownload}>
                          {t("common.cancel")}
                        </Button>
                      </div>
                    </div>
                  ) : phase === "waiting" ? (
                    <p role="status">{t("update.installing.wait")}</p>
                  ) : (
                    <div className="actions">
                      <Button primary block disabled={phase !== "idle"} onClick={startUpdate}>
                        {s.downloaded === a.version ? t("update.install") : t("update.go")}
                      </Button>
                      {s.downloaded === a.version ? (
                        <Button block onClick={() => void discard()}>
                          {t("update.discard")}
                        </Button>
                      ) : null}
                    </div>
                  )}
                </div>
              </section>
            ) : null}
          </>
        ) : null}

        {info ? (
          <div className="notice" role="status" data-testid="update-info">
            {info}
          </div>
        ) : null}
        {error ? (
          <div data-testid="update-error" data-kind={error}>
            <KindNotice kind={error} />
          </div>
        ) : null}
      </div>

      <AnimatePresence>
        {sheet === "updateMetered" && a ? (
          <ConfirmSheet
            key="updateMetered"
            open
            title={t("update.metered.title")}
            confirm={t("update.metered.go")}
            onConfirm={() => {
              void nav.back().then(() => download());
            }}
            onCancel={close}
          >
            <p>{t("update.metered.body", { size: formatBytes(a.size) })}</p>
          </ConfirmSheet>
        ) : null}
        {sheet === "updatePermission" ? (
          <ConfirmSheet
            key="updatePermission"
            open
            title={t("update.permission.title")}
            confirm={t("update.permission.go")}
            onConfirm={() => void grant()}
            onCancel={close}
          >
            <p>{t("update.permission.body")}</p>
            <p className="muted small">{t("update.permission.later")}</p>
          </ConfirmSheet>
        ) : null}
      </AnimatePresence>
      <BusyOverlay
        active={busy}
        label={phase === "checking" ? t("update.checking") : t("update.installing")}
      />
    </Screen>
  );
}
