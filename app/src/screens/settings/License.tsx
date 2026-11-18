import { useEffect, useState } from "react";
import type { Nav } from "../../nav";
import { t } from "../../strings";
import { Screen, Spinner } from "../../ui/components";
import { openWeb } from "./About";
import { findItem, loadLicenses, type LicenseData } from "./licenses";
import { Section, SettingRow } from "./parts";

/** 一項組件（或應用本身）的授權全文：名稱、版本、授權、原始碼的位置（按了用瀏覽器打開），再來是附的每一份授權檔。 */
export function License(props: { nav: Nav; item: string }) {
  const [data, setData] = useState<LicenseData | null>(null);
  const [failed, setFailed] = useState(false);
  const [openFailed, setOpenFailed] = useState(false);

  useEffect(() => {
    loadLicenses()
      .then(setData)
      .catch(() => setFailed(true));
  }, []);

  const it = data ? findItem(data, props.item) : null;

  return (
    <Screen title={t("license.title")} onBack={props.nav.back}>
      <div className="content">
        {failed || (data && !it) ? <p className="muted">{t("about.components.loadFailed")}</p> : null}
        {!data && !failed ? <Spinner dark /> : null}
        {data && it ? (
          <>
            <h2 className="section">
              {it.name}
              {it.version ? ` ${it.version}` : ""}
            </h2>
            <p className="muted">
              {it.license}
              {it.note ? `・${it.note}` : ""}
            </p>
            {it.source ? (
              <Section title={t("license.source")}>
                <SettingRow
                  title={it.source}
                  chevron
                  onClick={() => {
                    const url = it.source;
                    if (url) void openWeb(url).then((ok) => setOpenFailed(!ok));
                  }}
                />
              </Section>
            ) : null}
            {openFailed ? <p className="muted small">{t("about.openFailed")}</p> : null}
            {it.texts.map((id) => {
              const text = data.texts[id];
              if (!text) return null;
              return (
                <section key={id} className="license-block">
                  <h3 className="license-name">{text.name}</h3>
                  <pre className="license-text">{text.body}</pre>
                </section>
              );
            })}
          </>
        ) : null}
      </div>
    </Screen>
  );
}
