// 首頁上方的一行（不跳全螢幕）：查到新版時說一行；還沒決定要不要檢查更新時問一次。
// 都可以關掉：新版提示「略過這一版」只略過那一版；邀請答「不用」就不再問（設定裡仍可以打開）。

import { api } from "../../api";
import type { Nav } from "../../nav";
import { t } from "../../strings";
import { Button } from "../../ui/components";
import { updates, useUpdates } from "../../updates";

export function UpdateNotice(props: { nav: Nav; hasPapers: boolean }) {
  const s = useUpdates();
  if (!s?.supported) return null;
  // 外殼在沒同意時不交回 available；這裡再看一次，收回同意後首頁不再提示
  const a = s.consented ? s.available : null;
  if (a && s.dismissed !== a.version) {
    return (
      <div className="notice" role="status" data-testid="update-home">
        {t("update.home.available", { version: a.version })}
        <div className="notice-actions">
          <Button onClick={() => props.nav.push({ name: "update" })}>{t("update.home.view")}</Button>
          <Button
            onClick={() => {
              void api
                .updateConfigure({ dismissed: a.version })
                .catch(() => {})
                .then(() => updates.refresh());
            }}
          >
            {t("update.home.skip")}
          </Button>
        </div>
      </div>
    );
  }
  // 邀請：有文獻之後才問（第一次打開時先讓人加入文獻，不同時塞兩件事）
  if (!s.consented && !s.invited && props.hasPapers) {
    return (
      <div className="notice" role="status" data-testid="update-invite">
        {t("update.home.invite")}
        <div className="notice-actions">
          <Button onClick={() => props.nav.push({ name: "update" })}>{t("update.home.invite.yes")}</Button>
          <Button
            onClick={() => {
              void api
                .updateConfigure({ invited: true })
                .catch(() => {})
                .then(() => updates.refresh());
            }}
          >
            {t("update.home.invite.no")}
          </Button>
        </div>
      </div>
    );
  }
  return null;
}
