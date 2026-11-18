import { useEffect, useState } from "react";
import { api, kindOf, type ErrorKind, type AppInfo } from "../../api";
import type { Nav } from "../../nav";
import { t, type StringKey } from "../../strings";
import { Button, Details, KindNotice, Screen, Spinner } from "../../ui/components";
import { Section, SettingRow } from "./parts";
import { loadLicenses, visibleGroups, type LicenseData, type LicenseGroupId, type LicenseItem } from "./licenses";

const GROUP_TITLE: Record<LicenseGroupId, StringKey> = {
  ui: "about.group.ui",
  rust: "about.group.rust",
  android: "about.group.android",
  windows: "about.group.windows",
  files: "about.group.files",
  download: "about.group.download",
};

const CSL_URL = "https://citationstyles.org/";

/** 用系統的瀏覽器打開網址；打不開就交回 `false`。 */
export function openWeb(url: string): Promise<boolean> {
  return api
    .openUrl(url)
    .then(() => true)
    .catch(() => false);
}

/**
 * 關於：版本、應用本身的授權、用到的全部組件與授權、CSL 專案。
 * 組件清單是建置時從 Cargo.lock 與 package-lock.json 產生的（`scripts/licenses.mjs`）；點一項看授權全文。
 */
export function About(props: { nav: Nav }) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<ErrorKind | null>(null);
  const [data, setData] = useState<LicenseData | null>(null);
  const [listFailed, setListFailed] = useState(false);
  const [openFailed, setOpenFailed] = useState(false);

  useEffect(() => {
    api
      .appInfo()
      .then(setInfo)
      .catch((e: unknown) => setError(kindOf(e)));
    loadLicenses()
      .then(setData)
      .catch(() => setListFailed(true));
  }, []);

  // 只列這個安裝檔內含的（Android 與 Windows 共用一份清單）
  const groups = data ? visibleGroups(data, info?.platform ?? null) : [];
  const total = groups.reduce((n, g) => n + g.rows.length, 0);
  const open = (item: string) => props.nav.push({ name: "license", item });

  return (
    <Screen title={t("about.title")} onBack={props.nav.back}>
      <div className="content">
        {error ? <KindNotice kind={error} /> : null}
        {!info && !error ? <Spinner dark /> : null}
        {info ? (
          <>
            <p>PaperReader　{t("about.version", { version: info.version })}</p>
            <p className="muted small">{t("about.copyright")}</p>
            <p className="muted">{t("about.noNetwork")}</p>
            <Details>{t("about.network.detail")}</Details>
          </>
        ) : null}

        <Section title={t("about.license.heading")}>
          <SettingRow title={t("about.license.app")} value="Apache-2.0" chevron onClick={() => open("app")} />
        </Section>

        <p className="muted small" style={{ marginTop: 16 }}>
          {t("about.csl")}
        </p>
        <Button
          onClick={() => {
            void openWeb(CSL_URL).then((ok) => setOpenFailed(!ok));
          }}
        >
          {t("about.csl.link")}
        </Button>
        {openFailed ? <p className="muted small">{t("about.openFailed")}</p> : null}

        <h2 className="section">{t("about.components.heading")}</h2>
        {listFailed ? <p className="muted">{t("about.components.loadFailed")}</p> : null}
        {!data && !listFailed ? <Spinner dark /> : null}
        {data ? (
          <>
            <p className="muted small">{t("about.components.count", { n: total })}</p>
            <p className="muted small">{t("about.components.mpl")}</p>
            {groups.map((g) => (
              <Section key={g.id} title={`${t(GROUP_TITLE[g.id])}（${g.rows.length}）`}>
                {g.rows.map(({ item: it, index }) => (
                  <ComponentRow key={`${it.name}-${it.version}`} item={it} onClick={() => open(`${g.id}:${index}`)} />
                ))}
              </Section>
            ))}
          </>
        ) : null}
      </div>
    </Screen>
  );
}

/** 清單的一列：名稱與版本、授權、原始碼的位置。整列可以按，打開授權全文。 */
function ComponentRow(props: { item: LicenseItem; onClick: () => void }) {
  const it = props.item;
  return (
    <button type="button" className="set-row set-row-btn" onClick={props.onClick}>
      <span className="set-text">
        <span className="set-title">
          {it.name} {it.version}
        </span>
        <span className="set-hint">
          {it.license}
          {it.note ? `・${it.note}` : ""}
        </span>
        {it.source ? <span className="set-hint">{it.source}</span> : null}
      </span>
      <span className="set-chev" aria-hidden="true">
        ›
      </span>
    </button>
  );
}
