import { useSyncExternalStore } from "react";
import type { LibraryStatus, OpenTo, PaperSummary, Theme } from "../../api";
import { perPaper } from "../../perPaperApp";
import type { Nav, SheetKind } from "../../nav";
import type { PrefsState } from "../../prefs";
import { t, type StringKey } from "../../strings";
import { TopBar } from "../../ui/components";
import { GenerateSettings } from "./GenerateSettings";
import { Section, SegmentRow, SettingRow, SwitchRow } from "./parts";
import {
  AboutIcon,
  ExportIcon,
  FolderIcon,
  HelpIcon,
  NoteIcon,
  OpenIcon,
  PagesIcon,
  PinIcon,
  RestoreIcon,
  SunIcon,
  SyncIcon,
  ThemeIcon,
  TrashIcon,
  UpdateIcon,
} from "./settingsIcons";
import { useUpdates } from "../../updates";
import { useBackup } from "./useBackup";
import { useMove } from "./useMove";
import type { NoticeMore } from "../add/useAdd";
import { useToolboxKeepOpen } from "../reader/Toolbox";

function locationText(status: LibraryStatus | null): string {
  if (!status) return t("settings.library.none");
  if (status.state === "access_lost") return t("settings.library.lost");
  if (status.state === "missing") return t("settings.library.missing");
  if (status.storage === "shared") return t("settings.library.shared");
  if (status.storage === "private") return t("settings.library.private");
  return t("settings.library.none");
}

/** 設定分區：外觀、閱讀、筆記、文獻庫（位置、備份與還原、同步、回收區）、產生、關於與說明。 */
export function Settings(props: {
  nav: Nav;
  status: LibraryStatus | null;
  papers: PaperSummary[];
  /** 回收區裡有幾篇。 */
  trashCount: number;
  theme: Theme;
  onTheme: (t: Theme) => void;
  prefs: PrefsState;
  /** 從備份檔還原、搬到共用空間之後，文獻庫已經換了：重新讀目錄。 */
  refresh: () => Promise<void>;
  /** 目前疊在設定畫面上的面板（匯出與還原備份用）。 */
  sheet: SheetKind | null;
  /** 文獻庫是空的（沒有文獻、回收區與群組也空）：從備份檔還原會放在原位。 */
  libraryEmpty: boolean;
  /** 出錯時的訊息面板（App 的那一個）。 */
  notify: (title: string, message: string, more?: NoticeMore) => void;
}) {
  const { nav, status, prefs } = props;
  const backup = useBackup(nav, props.sheet, props.refresh, props.libraryEmpty, props.notify);
  const move = useMove(nav, props.sheet, props.refresh, props.notify);
  const [keepToolbox, setKeepToolbox] = useToolboxKeepOpen();
  // 整理文獻庫（逐篇處理）：已處理幾篇
  const organize = useSyncExternalStore(perPaper.subscribe, perPaper.getProgress);
  const updateState = useUpdates();

  const themes: Array<[Theme, StringKey]> = [
    ["system", "settings.theme.system"],
    ["light", "settings.theme.light"],
    ["dark", "settings.theme.dark"],
  ];
  const openTos: Array<[OpenTo, StringKey]> = [
    ["home", "settings.reading.openTo.home"],
    ["last", "settings.reading.openTo.last"],
  ];
  // 文獻庫在應用專屬空間：可以整個搬到共用空間
  const canMove = status?.state === "ready" && status.storage === "private";

  const shortLocation = status?.state === "ready" && status.storage !== null;

  return (
    <div className="screen settings">
      <TopBar title={t("settings.title")} onBack={() => void nav.back()} />
      <div className="scroll">
        <div className="set-inner">
          <Section title={t("settings.appearance.heading")}>
            <SegmentRow
              icon={<ThemeIcon />}
              title={t("settings.appearance.theme")}
              options={themes.map(([v, k]) => [v, t(k)])}
              value={props.theme}
              onChange={props.onTheme}
            />
          </Section>

          <Section title={t("settings.reading.heading")}>
            <SwitchRow
              icon={<PagesIcon />}
              title={t("settings.reading.darkPages")}
              hint={t("settings.reading.darkPagesHint")}
              checked={prefs.prefs.dark_pages}
              onChange={(v) => prefs.update({ dark_pages: v })}
            />
            <SwitchRow
              icon={<SunIcon />}
              title={t("settings.reading.keepAwake")}
              checked={prefs.prefs.keep_awake}
              onChange={(v) => prefs.update({ keep_awake: v })}
            />
            <SwitchRow
              icon={<PinIcon />}
              title={t("settings.reading.keepToolbox")}
              hint={t("settings.reading.keepToolboxHint")}
              checked={keepToolbox}
              onChange={setKeepToolbox}
            />
            <SegmentRow
              icon={<OpenIcon />}
              title={t("settings.reading.openTo")}
              options={openTos.map(([v, k]) => [v, t(k)])}
              value={prefs.prefs.open_to}
              onChange={(v) => prefs.update({ open_to: v })}
            />
          </Section>

          <Section title={t("reader.notes.title")}>
            <SettingRow icon={<NoteIcon />} title={t("settings.notes.where")} />
            <SettingRow
              title={t("settings.notes.obsidian")}
              chevron
              onClick={() => nav.push({ name: "help", topic: "obsidian" })}
            />
          </Section>

          <Section title={t("settings.library.heading")}>
            <SettingRow
              icon={<FolderIcon />}
              title={t("settings.library.location")}
              value={shortLocation ? locationText(status) : undefined}
              hint={
                shortLocation
                  ? status?.storage === "private"
                    ? t("settings.library.privateNote")
                    : undefined
                  : locationText(status)
              }
            />
            {status?.path ? <p className="set-path">{status.path}</p> : null}
            {/* 位置的路徑之後緊接著「搬到共用空間」：說的是同一件事 */}
            {canMove ? (
              <SettingRow
                title={t("settings.library.move")}
                chevron
                onClick={() => move.start(status?.allFilesAccess === true)}
              />
            ) : null}
            <SettingRow
              icon={<ExportIcon />}
              title={t("backup.export")}
              hint={t("backup.about")}
              chevron
              onClick={backup.exportNow}
            />
            <SettingRow
              icon={<RestoreIcon />}
              title={t("backup.restore")}
              chevron
              onClick={backup.startRestore}
            />
            <SettingRow
              icon={<SyncIcon />}
              title={t("settings.sync")}
              chevron
              onClick={() => nav.push({ name: "sync" })}
            />
            <SettingRow
              icon={<TrashIcon />}
              title={t("trash.title")}
              chevron
              onClick={() => nav.push({ name: "trash" })}
            />
            <SwitchRow
              icon={<PagesIcon />}
              title={t("settings.organize.title", { done: organize.done, total: organize.total })}
              hint={t("settings.organize.hint")}
              checked={!prefs.prefs.organize_paused}
              onChange={(v) => prefs.update({ organize_paused: !v })}
            />
          </Section>

          <Section title={t("reader.tool.generate")}>
            <GenerateSettings nav={nav} />
          </Section>


          <Section title={t("about.title")}>
            {/* 檢查更新只有 Android；電腦版之後用 Tauri 官方的更新外掛 */}
            {updateState?.supported ? (
              <SettingRow
                icon={<UpdateIcon />}
                title={t("update.settings.row")}
                value={
                  updateState.available
                    ? t("update.settings.available", { version: updateState.available.version })
                    : updateState.current
                }
                chevron
                onClick={() => nav.push({ name: "update" })}
              />
            ) : null}
            <SettingRow
              icon={<AboutIcon />}
              title={t("settings.about")}
              chevron
              onClick={() => nav.push({ name: "about" })}
            />
            <SettingRow
              icon={<HelpIcon />}
              title={t("help.title")}
              chevron
              onClick={() => nav.push({ name: "help" })}
            />
          </Section>
        </div>
      </div>
      {backup.overlays}
      {move.overlays}
    </div>
  );
}
