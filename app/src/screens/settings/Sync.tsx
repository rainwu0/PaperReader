// 設定的「同步」頁：匯出同步包（預設只帶變更，可切成完整）、匯入同步包、
// 合併沒做完時的「重試」，以及「詳情」裡的三句說明。流程與面板在 `useSync.tsx`。

import type { LibraryStatus } from "../../api";
import type { Nav } from "../../nav";
import { t } from "../../strings";
import { Button, Details, Screen } from "../../ui/components";
import { Section, SettingRow, SwitchRow } from "./parts";
import { ExportIcon, RestoreIcon } from "./settingsIcons";
import type { SyncUi } from "./useSync";

export function Sync(props: { nav: Nav; status: LibraryStatus | null; sync: SyncUi }) {
  const { nav, sync } = props;
  return (
    <Screen title={t("sync.title")} onBack={() => void nav.back()} className="settings">
      <div className="set-inner">
        <p className="sync-lead">{t("sync.lead")}</p>
        {props.status?.unfinishedSync ? (
          <div className="notice sync-unfinished" role="status">
            {t("sync.unfinished")}
            <div>
              <Button onClick={sync.resume}>{t("sync.unfinished.retry")}</Button>
            </div>
          </div>
        ) : null}
        <Section title={t("sync.title")}>
          <SwitchRow
            title={t("sync.full")}
            hint={t("sync.full.hint")}
            checked={sync.full}
            onChange={sync.setFull}
          />
          <SettingRow icon={<ExportIcon />} title={t("sync.export")} chevron onClick={sync.exportNow} />
          <SettingRow icon={<RestoreIcon />} title={t("sync.import")} chevron onClick={sync.startImport} />
        </Section>
        <Details>
          <ul className="sync-points">
            <li>{t("sync.details.clock")}</li>
            <li>{t("sync.details.delete")}</li>
            <li>{t("sync.details.samePdf")}</li>
          </ul>
        </Details>
      </div>
    </Screen>
  );
}
