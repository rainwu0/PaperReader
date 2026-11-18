// 匯入同步包的預覽：寫入之前列出會新增、更新、移除什麼（依種類彙總，可以逐篇看），
// 每個衝突一列、兩邊並排、預設已選好，提醒，這次不合併的文獻。按「合併」才寫入；離開這一頁就丟棄預覽（`useSync`）。

import type {
  ConflictChoice,
  ConflictSide,
  SyncConflict,
  SyncItem,
  SyncNotice,
  SkippedPaper,
} from "../../api";
import { dayHeading, timeOfDay } from "../../format";
import type { Nav } from "../../nav";
import { hasText, t, type StringKey } from "../../strings";
import { Button, Details, TopBar } from "../../ui/components";
import { groupNotices, isEmptyPreview, sideOptions, totals, visibleNotices, type KindTotal } from "./syncModel";
import type { SyncUi } from "./useSync";

/** 這個版本認得的鍵就用，不認得的用通用的一句（核心之後可能新增種類）。 */
function known(key: string, fallback: StringKey, params?: Record<string, string | number>): string {
  return t(hasText(key) ? key : fallback, params);
}

function when(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return `${dayHeading(iso)} ${timeOfDay(iso)}`;
}

function kindsText(list: KindTotal[]): string {
  return list.map((k) => known(`sync.item.${k.kind}`, "sync.item.other", { n: k.count })).join(t("sync.list.sep"));
}

function titleOf(title: string | null, paper: string | null): string {
  if (title) return title;
  return paper ? t("common.untitled") : t("settings.library.heading");
}

/** 新增、更新、移除的一段：一行彙總；「逐篇看」展開每一篇的細項。 */
function Summary(props: { label: string; items: SyncItem[] }) {
  if (props.items.length === 0) return null;
  const byPaper = new Map<string, { title: string; items: SyncItem[] }>();
  for (const it of props.items) {
    const key = it.paper ?? "";
    const g = byPaper.get(key) ?? { title: titleOf(it.title, it.paper), items: [] };
    g.items.push(it);
    byPaper.set(key, g);
  }
  return (
    <div className="sync-sum">
      <div className="sync-sum-line">
        <span className="sync-sum-label">{props.label}</span>
        <span>{kindsText(totals(props.items))}</span>
      </div>
      {byPaper.size > 1 ? (
        <Details label={t("sync.preview.byPaper")}>
          <ul className="problem-list">
            {[...byPaper.entries()].map(([key, g]) => (
              <li key={key}>{t("sync.preview.paperLine", { title: g.title, what: kindsText(totals(g.items)) })}</li>
            ))}
          </ul>
        </Details>
      ) : null}
    </div>
  );
}

function sideBody(c: SyncConflict, side: ConflictSide): string {
  if (!side.exists) return t("sync.side.deleted");
  if (c.kind === "TrashState") return t(side.text === "trash" ? "sync.side.trash" : "sync.side.papers");
  if (side.text === null) return "";
  return side.text.trim() === "" ? t("sync.side.empty") : side.text;
}

/** 書目衝突：這一邊和另一邊不同的那幾欄，一欄一行（篇名已經是卡片的內文，不重複）。 */
function sideFields(side: ConflictSide): string[] {
  return (side.fields ?? [])
    .filter((f) => f.name !== "title")
    .map((f) =>
      t("sync.side.field", {
        field: hasText(`sync.field.${f.name}`) ? t(`sync.field.${f.name}` as StringKey) : f.name,
        value: f.value ?? t("sync.side.empty"),
      }),
    );
}

/** 衝突的一邊：一張可以選的卡片（單選）。 */
function SideCard(props: {
  label: string;
  body: string;
  /** 不同的那幾欄（書目衝突），一欄一行。 */
  fields?: string[];
  modified: string | null;
  /** 時間是約略的（想法：記錄的時間，可能是匯出或準備匯入時補記的那一刻）。 */
  approx?: boolean;
  checked: boolean;
  enabled: boolean;
  onPick: () => void;
}) {
  const time = when(props.modified);
  return (
    <button
      type="button"
      role="radio"
      aria-checked={props.checked}
      disabled={!props.enabled}
      className="pick-card sync-side"
      onClick={props.onPick}
    >
      <span className="pick-card-mark" aria-hidden="true" />
      <span className="pick-card-text">
        <span className="pick-card-name">{props.label}</span>
        {props.body ? <span className="sync-side-body">{props.body}</span> : null}
        {props.fields?.map((line) => (
          <span key={line} className="sync-side-body">
            {line}
          </span>
        ))}
        {time ? (
          <span className="pick-card-desc">
            {t(props.approx ? "sync.side.whenApprox" : "sync.side.when", { when: time })}
          </span>
        ) : null}
      </span>
    </button>
  );
}

function ConflictRow(props: {
  c: SyncConflict;
  value: ConflictChoice;
  otherName: string;
  onPick: (choice: ConflictChoice) => void;
}) {
  const { c } = props;
  const opts = sideOptions(c);
  const what = known(`sync.conflict.${c.kind}`, "sync.conflict.other", { n: c.count });
  return (
    <div className="sync-conflict" data-conflict={c.id}>
      <div className="sync-conflict-title">{titleOf(c.title, c.paper)}</div>
      <div className="muted small">{what}</div>
      <div className="sync-sides" role="radiogroup" aria-label={what}>
        <SideCard
          label={t("sync.side.local")}
          body={sideBody(c, c.local)}
          fields={sideFields(c.local)}
          modified={c.local.modified}
          approx={c.kind === "Thoughts"}
          checked={props.value === "Local"}
          enabled={opts.local}
          onPick={() => props.onPick("Local")}
        />
        <SideCard
          label={props.otherName}
          body={sideBody(c, c.incoming)}
          fields={sideFields(c.incoming)}
          modified={c.incoming.modified}
          approx={c.kind === "Thoughts"}
          checked={props.value === "Incoming"}
          enabled={opts.incoming}
          onPick={() => props.onPick("Incoming")}
        />
      </div>
      {opts.both ? (
        <button
          type="button"
          role="radio"
          aria-checked={props.value === "Both"}
          className="pick-card"
          onClick={() => props.onPick("Both")}
        >
          <span className="pick-card-mark" aria-hidden="true" />
          <span className="pick-card-text">
            <span className="pick-card-name">{t("sync.choice.Both")}</span>
          </span>
        </button>
      ) : null}
    </div>
  );
}

function Notices(props: { notices: SyncNotice[] }) {
  const groups = groupNotices(props.notices);
  if (groups.length === 0) return null;
  return (
    <section className="sync-block">
      <h2 className="sync-h">{t("sync.preview.notices")}</h2>
      <ul className="sync-list">
        {groups.map((g) => (
          <li key={g.key}>
            {known(`sync.notice.${g.key}`, "sync.notice.other")}
            {g.count > 1 ? t("sync.notice.count", { n: g.count }) : ""}
            {g.paths.length > 0 ? (
              <Details>
                <ul className="problem-list">
                  {g.paths.map((p) => (
                    <li key={p} className="sheet-path">
                      {p}
                    </li>
                  ))}
                </ul>
              </Details>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}

function Skipped(props: { papers: SkippedPaper[] }) {
  if (props.papers.length === 0) return null;
  return (
    <section className="sync-block">
      <h2 className="sync-h">{t("sync.preview.skipped")}</h2>
      <ul className="sync-list">
        {props.papers.map((s) => (
          <li key={s.paper}>
            {t("sync.preview.paperLine", {
              title: titleOf(s.title, s.paper),
              what: known(`sync.skip.${s.reason}`, "sync.skip.other"),
            })}
          </li>
        ))}
      </ul>
    </section>
  );
}

export function SyncPreviewScreen(props: { nav: Nav; sync: SyncUi }) {
  const { nav, sync } = props;
  const p = sync.preview;
  const back = () => void nav.back();
  if (!p) {
    // 合併完、正要離開這一頁
    return (
      <div className="screen">
        <TopBar title={t("sync.preview.title")} onBack={back} />
      </div>
    );
  }
  const otherName = p.package.device_label ?? t("sync.preview.otherDevice");
  return (
    <div className="screen sync-preview">
      <TopBar title={t("sync.preview.title")} onBack={back} />
      <div className="scroll">
        <div className="content">
          <p className="muted small">
            {t("sync.preview.from", { device: otherName, when: when(p.package.created) })}
          </p>
          {p.package.already_imported ? (
            <div className="notice sync-already" role="status">
              {t("sync.preview.already")}
            </div>
          ) : null}
          {isEmptyPreview(p) ? <p>{t("sync.preview.nothing")}</p> : null}
          <Summary label={t("sync.preview.added")} items={p.added} />
          <Summary label={t("sync.preview.updated")} items={p.updated} />
          <Summary label={t("sync.preview.removed")} items={p.removed} />
          {p.conflicts.length > 0 ? (
            <section className="sync-block">
              <h2 className="sync-h">{t("sync.preview.conflicts", { n: p.conflicts.length })}</h2>
              {p.conflicts.map((c) => (
                <ConflictRow
                  key={c.id}
                  c={c}
                  value={sync.picked[c.id] ?? c.default}
                  otherName={otherName}
                  onPick={(choice) => sync.choose(c.id, choice)}
                />
              ))}
            </section>
          ) : null}
          <Notices notices={visibleNotices(p.notices, p.conflicts, sync.picked)} />
          <Skipped papers={p.skipped_papers} />
        </div>
      </div>
      <div className="sync-bar">
        <Button primary block onClick={sync.apply}>
          {t("sync.preview.merge")}
        </Button>
      </div>
    </div>
  );
}
