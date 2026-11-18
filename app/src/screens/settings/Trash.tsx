import { useCallback, useEffect, useState } from "react";
import {
  api,
  kindOf,
  type ErrorKind,
  type PaperSummary,
  type RemovedMaterialEntry,
  type TrashEntry,
} from "../../api";
import { displayTitle, relativeDay } from "../../format";
import type { Nav, SheetKind } from "../../nav";
import { t } from "../../strings";
import { BusyOverlay, Button, ConfirmSheet, ErrorNotice, KindNotice, Screen, Spinner } from "../../ui/components";
import { materialLabel } from "../home/paperMeta";

/** 要永久刪除的那一項（確認面板開著時）。 */
type Purging = { folder: string; name: string; paper: boolean };

function sizeText(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

/** 回收區：移除的文獻與移除的材料，都可以還原、永久刪除；全部清空。 */
export function Trash(props: {
  nav: Nav;
  sheet: SheetKind | null;
  /** 目錄裡的文獻（移除的材料用那篇的篇名稱呼）。 */
  papers: PaperSummary[];
  onChanged: () => void;
  /** 短暫的提示（App 的那一個）。 */
  toast: (message: string) => void;
}) {
  const { nav, toast: setToast } = props;
  const [entries, setEntries] = useState<TrashEntry[] | null>(null);
  const [materials, setMaterials] = useState<RemovedMaterialEntry[]>([]);
  const [error, setError] = useState<ErrorKind | null>(null);
  const [purging, setPurging] = useState<Purging | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const [e, m] = await Promise.all([api.listTrash(), api.listRemovedMaterials()]);
      setEntries(e);
      setMaterials(m);
      setError(null);
    } catch (e) {
      setError(kindOf(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const restore = async (id: string) => {
    try {
      await api.restorePaper(id);
      props.onChanged();
      setToast(t("trash.restored"));
      await load();
    } catch (e) {
      setError(kindOf(e));
    }
  };

  /** 移除的材料放回那篇。那篇不在目錄裡、或那一項已經回到那篇時，說明原因。 */
  const [restoreNote, setRestoreNote] = useState<string | null>(null);
  const restoreMaterial = async (m: RemovedMaterialEntry) => {
    setRestoreNote(null);
    try {
      await api.restoreMaterial(m.folder);
      props.onChanged();
      setToast(t("trash.restoredTo", { paper: paperName(m) }));
      await load();
    } catch (e) {
      const kind = kindOf(e);
      if (kind === "NotFound") setRestoreNote(t("trash.restoreMaterial.noPaper", { paper: paperName(m) }));
      else if (kind === "InvalidInput") setRestoreNote(t("trash.restoreMaterial.already"));
      else setError(kind);
    }
  };

  const askPurge = (p: Purging) => {
    setPurging(p);
    nav.push({ name: "sheet", kind: "purgeConfirm" });
  };

  const purge = async () => {
    if (!purging) return;
    setBusy(true);
    try {
      await api.purgeTrashEntry(purging.folder);
      await nav.back();
      setToast(t("trash.purged"));
    } catch (e) {
      await nav.back();
      setError(kindOf(e));
    } finally {
      setPurging(null);
      setBusy(false);
      props.onChanged();
      await load();
    }
  };

  const emptyAll = async () => {
    setBusy(true);
    try {
      await api.emptyTrash();
      await nav.back();
      setToast(t("trash.emptied"));
    } catch (e) {
      await nav.back();
      setError(kindOf(e));
    } finally {
      setBusy(false);
      props.onChanged();
      await load();
    }
  };

  const total = (entries?.length ?? 0) + materials.length;
  /** 移除的材料屬於哪一篇：目錄或回收區裡那篇的篇名，都找不到時用移除時的資料夾名稱。 */
  const paperName = (m: RemovedMaterialEntry): string => {
    const inCatalog = props.papers.find((p) => p.id === m.paper_id);
    if (inCatalog) return displayTitle(inCatalog.bib, m.paper_folder);
    const inTrash = entries?.find((e) => e.paper_id === m.paper_id);
    return inTrash ? displayTitle(inTrash.bib, inTrash.name) : m.paper_folder;
  };

  return (
    <>
      <Screen title={t("trash.title")} onBack={() => void nav.back()}>
        {error ? <KindNotice kind={error} onAction={() => void load()} /> : null}
        {restoreNote ? <ErrorNotice message={restoreNote} /> : null}
        {entries === null && !error ? (
          <div className="center-fill">
            <Spinner dark />
          </div>
        ) : null}
        {entries && total === 0 ? (
          <div className="empty">
            <h2>{t("trash.empty.title")}</h2>
            <p className="muted">{t("trash.empty.body")}</p>
          </div>
        ) : null}
        {entries && entries.length > 0 ? (
          <>
            {materials.length > 0 ? <h2 className="section trash-heading">{t("trash.papers")}</h2> : null}
            <ul className="list">
              {entries.map((e) => {
                const name = displayTitle(e.bib, e.name);
                return (
                  <li key={e.folder} className="row">
                    <div className="row-main" style={{ cursor: "default" }}>
                      <span className="row-text">
                        <span className="row-title">{name}</span>
                        <span className="row-sub">
                          {e.removed_at ? t("trash.removedAt", { when: relativeDay(e.removed_at) }) : " "}
                        </span>
                      </span>
                    </div>
                    <div className="trash-actions">
                      <Button onClick={() => void restore(e.paper_id)}>{t("trash.restore")}</Button>
                      <Button onClick={() => askPurge({ folder: e.folder, name, paper: true })}>
                        {t("trash.purge")}
                      </Button>
                    </div>
                  </li>
                );
              })}
            </ul>
          </>
        ) : null}
        {materials.length > 0 ? (
          <>
            <h2 className="section trash-heading">{t("info.materials.heading")}</h2>
            <ul className="list">
              {materials.map((m) => {
                const name = t("trash.materialOf", { paper: paperName(m), name: materialLabel(m.material) });
                return (
                  <li key={m.folder} className="row">
                    <div className="row-main" style={{ cursor: "default" }}>
                      <span className="row-text">
                        <span className="row-title">{materialLabel(m.material)}</span>
                        <span className="row-sub">{paperName(m)}</span>
                        <span className="row-sub">
                          {m.removed_at ? t("trash.removedAt", { when: relativeDay(m.removed_at) }) : ""}
                          {m.removed_at ? "・" : ""}
                          {sizeText(m.bytes)}
                        </span>
                      </span>
                    </div>
                    <div className="trash-actions">
                      <Button onClick={() => void restoreMaterial(m)}>{t("trash.restore")}</Button>
                      <Button onClick={() => askPurge({ folder: m.folder, name, paper: false })}>
                        {t("trash.purge")}
                      </Button>
                    </div>
                  </li>
                );
              })}
            </ul>
          </>
        ) : null}
        {total > 0 ? (
          <div className="content">
            <Button block onClick={() => nav.push({ name: "sheet", kind: "emptyTrash" })}>
              {t("trash.emptyAll")}
            </Button>
          </div>
        ) : null}
      </Screen>

      <ConfirmSheet
        open={props.sheet === "purgeConfirm" && purging !== null}
        title={t("trash.purge.title", { name: purging?.name ?? "" })}
        confirm={t("trash.purge")}
        onConfirm={() => void purge()}
        onCancel={() => void nav.back()}
      >
        <p>{purging?.paper ? t("trash.purge.bodyPaper") : t("trash.purge.body")}</p>
      </ConfirmSheet>
      <ConfirmSheet
        open={props.sheet === "emptyTrash"}
        title={t("trash.emptyAll.title")}
        confirm={t("trash.emptyAll")}
        onConfirm={() => void emptyAll()}
        onCancel={() => void nav.back()}
      >
        <p>{t("trash.emptyAll.body", { n: total })}</p>
      </ConfirmSheet>
      <BusyOverlay active={busy} label={t("trash.title")} />
    </>
  );
}
