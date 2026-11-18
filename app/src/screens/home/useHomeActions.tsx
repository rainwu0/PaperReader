// 首頁的群組與文獻列選單、多選的動作：狀態、動作，以及它們的面板。
// 面板都是導覽堆疊的一層（返回鍵先關它）；要從一個面板換到另一個時，先退一層再推新的，
// 返回鍵才不會留著已經用完的那一層。
//
// 可以反做的動作（移除、移出群組、標示為已讀、刪群組）直接做，提示給「復原」，不先問。
//
// 面板的種類（`nav.ts` 的 `SheetKind`）：
//   groupMenu       文獻列的選單（文獻資訊、放進群組、移出群組、加入譯本、標示為已讀、移除），
//                   或側邊欄裡一個群組的選單（改名、匯出書目、刪除）
//   groupPick       一篇文獻的群組勾選清單
//   groupName       輸入群組名稱（新增、改名）
//   translationResult  加入譯本的結果
//   bibExport       書目匯出（一個群組的全部，或多選勾的幾篇）
//   selectMenu      多選的「更多動作」（移出群組、標示為已讀、匯出書目）
//   bulkGroup       多選：把勾的幾篇放進一個群組
//   bulkLeave       多選：把勾的幾篇移出一個群組

import { useState, type ReactNode } from "react";
import { api, AppError, type Group, type TranslationResult } from "../../api";
import { ALL_VIEW, type View } from "../../catalog";
import { displayTitle } from "../../format";
import { createGroupWith, type Library } from "../../library";
import type { Nav, SheetKind } from "../../nav";
import { errorDetail, errorMessage, t } from "../../strings";
import { BusyOverlay, Button, Sheet, type ShowUndo } from "../../ui/components";
import { BibExportSheet } from "../export/exportUi";
import { AnimatePresence } from "../../ui/motion";
import { GroupNameSheet, GroupPickList, TranslationResultSheet } from "./groupUi";

type MenuTarget = { kind: "paper"; id: string } | { kind: "group"; id: string };
type NameDraft =
  | { mode: "create"; paperId: string | null }
  | { mode: "createBulk"; paperIds: string[] }
  | { mode: "rename"; id: string };
interface BibTarget {
  ids: string[];
  /** 檔名的前半（群組的名稱）。 */
  name: string | null;
}
type TranslationOutcome = Pick<TranslationResult, "outcome" | "name" | "error">;

/** 對每一篇做 `f`，一篇失敗不影響其他篇。交回做成的幾篇，與最後一個錯誤（全部成功是 `null`）。 */
async function eachPaper(ids: string[], f: (id: string) => Promise<unknown>): Promise<{ done: string[]; error: unknown }> {
  const done: string[] = [];
  let error: unknown = null;
  for (const id of ids) {
    try {
      await f(id);
      done.push(id);
    } catch (e) {
      error = e;
    }
  }
  return { done, error };
}

/** 復原用：對每一篇做 `f`，做完才丟出錯誤（做得到的照做，提示說「沒有復原」）。 */
async function revertEach(ids: string[], f: (id: string) => Promise<unknown>): Promise<void> {
  const r = await eachPaper(ids, f);
  if (r.error !== null) throw r.error;
}

export interface HomeActions {
  openPaperMenu: (paperId: string) => void;
  openGroupMenu: (group: Group) => void;
  newGroup: () => void;
  /** 把這幾篇的書目一次匯出（開格式與交出檔案的面板）。 */
  openBibExport: (ids: string[], name: string | null) => void;
  /** 多選：選一個群組，把這幾篇都放進去。 */
  openBulkGroup: (ids: string[]) => void;
  /** 多選：把這幾篇移到回收區（給「復原」）。 */
  bulkRemove: (ids: string[]) => void;
  /** 多選的「更多動作」（移出群組、標示為已讀、匯出書目）。 */
  openSelectMenu: (ids: string[]) => void;
  /** 目前該畫出來的面板與進行中的提示。 */
  overlays: (sheets: SheetKind[]) => ReactNode;
}

export function useHomeActions(o: {
  lib: Library;
  nav: Nav;
  view: View;
  onView: (v: View) => void;
  /** 顯示一則要使用者按「確定」的訊息（出錯時用）。 */
  notify: (title: string, message: string, more?: { detail?: string }) => void;
  /** 一則短暫的訊息（成功時用）。 */
  toast: (message: string) => void;
  /** 做完可以反做的動作：提示給「復原」。 */
  undo: ShowUndo;
  /** 多選的動作做完了：結束多選。 */
  onBulkDone: () => void;
}): HomeActions {
  const { lib, nav, view, onView, notify, toast, undo, onBulkDone } = o;
  const [menu, setMenu] = useState<MenuTarget | null>(null);
  const [pickId, setPickId] = useState<string | null>(null);
  const [nameDraft, setNameDraft] = useState<NameDraft | null>(null);
  const [bibTarget, setBibTarget] = useState<BibTarget | null>(null);
  const [bulkIds, setBulkIds] = useState<string[]>([]);
  const [translation, setTranslation] = useState<TranslationOutcome | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const fail = (title: string, e: unknown) => {
    const kind = e instanceof AppError ? e.kind : "Unknown";
    notify(title, errorMessage(kind), { detail: errorDetail(kind) });
  };
  const push = (kind: SheetKind) => nav.push({ name: "sheet", kind });
  const back = () => void nav.back();
  /** 正在看的群組（看全部文獻、未分組時是 `undefined`）。 */
  const viewGroup = view.kind === "group" ? lib.groups.find((g) => g.id === view.id) : undefined;

  const openPaperMenu = (paperId: string) => {
    setMenu({ kind: "paper", id: paperId });
    push("groupMenu");
  };
  const openGroupMenu = (group: Group) => {
    setMenu({ kind: "group", id: group.id });
    push("groupMenu");
  };
  const newGroup = () => {
    setNameDraft({ mode: "create", paperId: null });
    push("groupName");
  };

  const openBibExport = (ids: string[], name: string | null) => {
    setBibTarget({ ids, name });
    push("bibExport");
  };
  const openBulkGroup = (ids: string[]) => {
    setBulkIds(ids);
    push("bulkGroup");
  };
  const openSelectMenu = (ids: string[]) => {
    setBulkIds(ids);
    push("selectMenu");
  };

  /** 選單裡的一項：先關掉選單，再做事。 */
  const fromMenu = async (next: () => void) => {
    await nav.back();
    next();
  };

  const toggleMember = async (paperId: string, g: Group, member: boolean) => {
    setPending(g.id);
    try {
      await api.setGroupMember(paperId, g.id, member);
    } catch (e) {
      fail(t("group.pick.failed.title"), e);
    } finally {
      // 不管成不成，畫面都以實際的結果為準
      await lib.refresh();
      setPending(null);
    }
  };

  const addTranslation = async (paperId: string) => {
    setBusy(t("translation.busy"));
    try {
      const r = await api.addTranslationFromPicker(paperId);
      if (r.cancelled) return;
      // 加入了或原本就在：一則短暫提示；失敗才開面板說原因
      if (r.outcome === "failed") {
        setTranslation({ outcome: r.outcome, name: r.name, error: r.error });
        push("translationResult");
      } else {
        toast(t(r.outcome === "added" ? "translation.added" : "translation.present"));
      }
      await lib.refresh();
    } catch (e) {
      fail(t("translation.failed.title"), e);
    } finally {
      setBusy(null);
    }
  };

  /**
   * 一個做在幾篇上的動作（移除、移出群組、標示為已讀）：每篇做 `f`，做完結束多選（`bulk`）、重讀目錄。
   * 全部沒做成就說明原因；做成了的給「復原」（`revert` 只反做做成的那幾篇），有幾篇沒做成時提示裡說。
   */
  const runOnPapers = async (p: {
    ids: string[];
    bulk: boolean;
    f: (id: string) => Promise<unknown>;
    failTitle: string;
    doneText: (n: number) => string;
    partialText: (ok: number, fail: number) => string;
    revert: (id: string) => Promise<unknown>;
  }) => {
    setBusy(t("common.working"));
    const r = await eachPaper(p.ids, p.f);
    await lib.refresh();
    setBusy(null);
    if (p.bulk) onBulkDone();
    if (r.done.length === 0) {
      if (r.error !== null) fail(p.failTitle, r.error);
      return;
    }
    const message = r.done.length === p.ids.length ? p.doneText(r.done.length) : p.partialText(r.done.length, p.ids.length - r.done.length);
    undo(message, () => revertEach(r.done, p.revert));
  };

  /** 移到回收區（一篇或多選的幾篇）。 */
  const removePapers = (ids: string[], bulk: boolean) =>
    runOnPapers({
      ids,
      bulk,
      f: (id) => api.removePaper(id),
      failTitle: t("paper.remove.failed.title"),
      doneText: (n) => (bulk ? t("bulk.removed", { n }) : t("info.removed")),
      partialText: (ok, f) => t("bulk.remove.partial", { ok, fail: f }),
      revert: (id) => api.restorePaper(id),
    });

  /** 標示為已讀：已經是已讀的不動（復原時才不會把它們改掉）。 */
  const markRead = (ids: string[], bulk: boolean) => {
    const target = ids.filter((id) => lib.papers.find((p) => p.id === id)?.progress.status !== "Read");
    if (target.length === 0) {
      if (bulk) onBulkDone();
      toast(bulk ? t("bulk.read.done", { n: ids.length }) : t("read.marked"));
      return;
    }
    return runOnPapers({
      ids: target,
      bulk,
      f: (id) => api.setRead(id, true),
      failTitle: t("read.failed.title"),
      doneText: (n) => (bulk ? t("bulk.read.done", { n }) : t("read.marked")),
      partialText: (ok, f) => t("bulk.read.partial", { ok, fail: f }),
      // 取消標示：回到標示之前的未讀或讀到一半（閱讀狀態照紀錄算）
      revert: (id) => api.setRead(id, false),
    });
  };

  /** 移出一個群組：只動在這個群組裡的那幾篇。 */
  const leaveGroup = (ids: string[], g: Group, bulk: boolean) => {
    const members = ids.filter((id) => lib.papers.find((p) => p.id === id)?.groups.includes(g.id));
    return runOnPapers({
      ids: members,
      bulk,
      f: (id) => api.setGroupMember(id, g.id, false),
      failTitle: t("bulk.leave.failed.title"),
      doneText: () => t("group.left", { name: g.name }),
      partialText: (ok, f) => t("bulk.leave.partial", { ok, fail: f }),
      revert: (id) => api.setGroupMember(id, g.id, true),
    });
  };

  const bulkRemove = (ids: string[]) => void removePapers(ids, true);

  /** 多選：把這幾篇都放進一個群組。全部做完才收面板；有幾篇沒能放進去就說明。 */
  const addBulk = async (ids: string[], g: Group) => {
    setBusy(t("common.working"));
    let ok = 0;
    for (const id of ids) {
      try {
        await api.setGroupMember(id, g.id, true);
        ok++;
      } catch {
        // 其餘照做；沒放進去的在最後一起說明
      }
    }
    await lib.refresh();
    setBusy(null);
    await nav.back();
    onBulkDone();
    if (ok === ids.length) toast(t("bulk.group.done", { n: ok, name: g.name }));
    else notify(t("bulk.group.failed.title"), t("bulk.group.partial", { ok, name: g.name, fail: ids.length - ok }));
  };

  const submitName = async (draft: NameDraft, name: string) => {
    if (draft.mode === "rename") {
      const current = lib.groups.find((g) => g.id === draft.id);
      if (current && current.name !== name) await api.renameGroup(draft.id, name);
      await lib.refresh();
      await nav.back();
      return;
    }
    if (draft.mode === "createBulk") {
      const g = await api.createGroup(name);
      await lib.refresh();
      await nav.back(); // 關掉名稱面板
      await addBulk(draft.paperIds, g);
      return;
    }
    const g = await api.createGroup(name);
    let memberFailed = false;
    if (draft.paperId) {
      try {
        await api.setGroupMember(draft.paperId, g.id, true);
      } catch {
        memberFailed = true;
      }
    }
    await lib.refresh();
    await nav.back();
    if (memberFailed) notify(t("group.pick.failed.title"), t("group.name.memberFailed"));
  };

  /**
   * 刪除群組（不先問，給「復原」）。復原是建一個同名的群組、把原本的成員放回去：代號是新的，
   * 舊代號留著刪除記錄，同步到另一台時舊的照樣刪掉、新的帶過去，不會變成兩個。
   */
  const deleteGroup = async (g: Group) => {
    const members = lib.papers.filter((p) => p.groups.includes(g.id)).map((p) => p.id);
    const viewing = view.kind === "group" && view.id === g.id;
    setBusy(t("common.working"));
    try {
      const r = await api.deleteGroup(g.id);
      if (viewing) onView(ALL_VIEW);
      await lib.refresh();
      if (r.skipped.length > 0) {
        // 有幾篇的群組記錄沒改到：照實說明（這時不給復原，那幾篇還帶著舊的代號）
        notify(t("group.delete.skipped.title"), t("group.delete.skipped", { n: r.skipped.length }));
        return;
      }
      undo(t("group.deleted", { name: g.name }), async () => {
        const again = await createGroupWith(g.name, members);
        if (viewing) onView({ kind: "group", id: again.group.id });
        if (again.memberFailed) throw new AppError("Unknown", "group member");
      });
    } catch (e) {
      await lib.refresh();
      fail(t("group.delete.failed.title"), e);
    } finally {
      setBusy(null);
    }
  };

  const overlays = (sheets: SheetKind[]): ReactNode => {
    const menuPaper = menu?.kind === "paper" ? lib.papers.find((p) => p.id === menu.id) : undefined;
    const menuGroup = menu?.kind === "group" ? lib.groups.find((g) => g.id === menu.id) : undefined;
    const pickPaper = pickId ? lib.papers.find((p) => p.id === pickId) : undefined;
    // 多選勾的幾篇所在的群組（「移出群組」列出這些）
    const bulkPapers = lib.papers.filter((p) => bulkIds.includes(p.id));
    const leaveCandidates = lib.groups.filter((g) => bulkPapers.some((p) => p.groups.includes(g.id)));

    return (
      <>
        <AnimatePresence>
        {sheets.includes("groupMenu") && menuPaper ? (
          <Sheet key="paperMenu" open title={displayTitle(menuPaper.bib, menuPaper.folder)} onDismiss={back}>
            <div className="choice-list menu-list">
              <button
                type="button"
                className="choice"
                onClick={() => void fromMenu(() => nav.push({ name: "info", paperId: menuPaper.id }))}
              >
                {t("info.title")}
              </button>
              <button
                type="button"
                className="choice"
                disabled={menuPaper.needs_newer_app}
                onClick={() =>
                  void fromMenu(() => {
                    setPickId(menuPaper.id);
                    push("groupPick");
                  })
                }
              >
                {t("group.pick.title")}
              </button>
              {viewGroup && menuPaper.groups.includes(viewGroup.id) ? (
                <button
                  type="button"
                  className="choice"
                  disabled={menuPaper.needs_newer_app}
                  onClick={() => void fromMenu(() => void leaveGroup([menuPaper.id], viewGroup, false))}
                >
                  {t("group.leave")}
                </button>
              ) : null}
              <button
                type="button"
                className="choice"
                disabled={menuPaper.needs_newer_app}
                onClick={() => void fromMenu(() => void addTranslation(menuPaper.id))}
              >
                {t("translation.add")}
              </button>
              {menuPaper.progress.status !== "Read" ? (
                <button
                  type="button"
                  className="choice"
                  onClick={() => void fromMenu(() => void markRead([menuPaper.id], false))}
                >
                  {t("info.markRead")}
                </button>
              ) : null}
              <button
                type="button"
                className="choice"
                disabled={menuPaper.needs_newer_app}
                onClick={() => void fromMenu(() => void removePapers([menuPaper.id], false))}
              >
                {t("paper.remove")}
              </button>
            </div>
          </Sheet>
        ) : null}

        {sheets.includes("groupMenu") && menuGroup ? (
          <Sheet key="groupMenu" open title={menuGroup.name} onDismiss={back}>
            <div className="choice-list menu-list">
              <button
                type="button"
                className="choice"
                onClick={() =>
                  void fromMenu(() => {
                    setNameDraft({ mode: "rename", id: menuGroup.id });
                    push("groupName");
                  })
                }
              >
                {t("group.rename")}
              </button>
              <button
                type="button"
                className="choice"
                onClick={() =>
                  void fromMenu(() =>
                    openBibExport(
                      lib.papers.filter((p) => p.groups.includes(menuGroup.id)).map((p) => p.id),
                      menuGroup.name,
                    ),
                  )
                }
              >
                {t("bib.title")}
              </button>
              <button type="button" className="choice" onClick={() => void fromMenu(() => void deleteGroup(menuGroup))}>
                {t("group.delete")}
              </button>
            </div>
          </Sheet>
        ) : null}

        {sheets.includes("groupPick") && pickPaper ? (
          <Sheet key="groupPick" open title={t("group.pick.title")} onDismiss={back}>
            <p className="muted small">{displayTitle(pickPaper.bib, pickPaper.folder)}</p>
            {lib.groups.length === 0 ? (
              <p className="muted">{t("group.none")}</p>
            ) : (
              <>
                <GroupPickList
                  groups={lib.groups}
                  memberOf={pickPaper.groups}
                  pending={pending}
                  onToggle={(g, member) => void toggleMember(pickPaper.id, g, member)}
                />
              </>
            )}
            <div className="actions">
              <Button
                block
                onClick={() => {
                  setNameDraft({ mode: "create", paperId: pickPaper.id });
                  push("groupName");
                }}
              >
                {t("sidebar.newGroup")}
              </Button>
              <Button primary block onClick={back}>
                {t("export.done")}
              </Button>
            </div>
          </Sheet>
        ) : null}

        {sheets.includes("bibExport") && bibTarget ? (
          <BibExportSheet key="bib" paperIds={bibTarget.ids} name={bibTarget.name} onClose={back} />
        ) : null}

        {sheets.includes("selectMenu") ? (
          <Sheet key="selectMenu" open title={t("select.count", { n: bulkIds.length })} onDismiss={back}>
            <div className="choice-list menu-list">
              <button
                type="button"
                className="choice"
                onClick={() =>
                  void fromMenu(() => {
                    // 看著某個群組時，就是移出這個群組；否則選要移出哪一個
                    if (viewGroup) void leaveGroup(bulkIds, viewGroup, true);
                    else push("bulkLeave");
                  })
                }
              >
                {t("group.leave")}
              </button>
              <button type="button" className="choice" onClick={() => void fromMenu(() => void markRead(bulkIds, true))}>
                {t("info.markRead")}
              </button>
              <button
                type="button"
                className="choice"
                onClick={() => void fromMenu(() => openBibExport(bulkIds, viewGroup ? viewGroup.name : null))}
              >
                {t("bib.title")}
              </button>
            </div>
          </Sheet>
        ) : null}

        {sheets.includes("bulkLeave") ? (
          <Sheet key="bulkLeave" open title={t("group.leave")} onDismiss={back}>
            {leaveCandidates.length === 0 ? (
              <p className="muted">{t("bulk.leave.none")}</p>
            ) : (
              <>
                <p className="muted small">{t("bulk.leave.hint", { n: bulkIds.length })}</p>
                <div className="choice-list">
                  {leaveCandidates.map((g) => (
                    <button
                      key={g.id}
                      type="button"
                      className="choice"
                      onClick={() => void fromMenu(() => void leaveGroup(bulkIds, g, true))}
                    >
                      <span className="gname">{g.name}</span>
                    </button>
                  ))}
                </div>
              </>
            )}
            <div className="actions">
              <Button block onClick={back}>
                {t("common.cancel")}
              </Button>
            </div>
          </Sheet>
        ) : null}

        {sheets.includes("bulkGroup") ? (
          <Sheet key="bulkGroup" open title={t("group.pick.title")} onDismiss={back}>
            <p className="muted small">{t("bulk.group.hint", { n: bulkIds.length })}</p>
            {lib.groups.length === 0 ? (
              <p className="muted">{t("group.none")}</p>
            ) : (
              <div className="choice-list">
                {lib.groups.map((g) => (
                  <button key={g.id} type="button" className="choice" onClick={() => void addBulk(bulkIds, g)}>
                    <span className="gname">{g.name}</span>
                  </button>
                ))}
              </div>
            )}
            <div className="actions">
              <Button
                block
                onClick={() => {
                  setNameDraft({ mode: "createBulk", paperIds: bulkIds });
                  push("groupName");
                }}
              >
                {t("sidebar.newGroup")}
              </Button>
              <Button block onClick={back}>
                {t("common.cancel")}
              </Button>
            </div>
          </Sheet>
        ) : null}

        {/* 名稱面板放在會開它的面板（勾選清單、多選的放進群組）後面：同一層疊放時後畫的在上面，才不會被蓋住 */}
        {sheets.includes("groupName") && nameDraft ? (
          <GroupNameSheet
            key={nameDraft.mode === "rename" ? `name-${nameDraft.id}` : "name-new"}
            title={nameDraft.mode === "rename" ? t("group.name.renameTitle") : t("sidebar.newGroup")}
            initial={
              nameDraft.mode === "rename" ? (lib.groups.find((g) => g.id === nameDraft.id)?.name ?? "") : ""
            }
            submitLabel={nameDraft.mode === "rename" ? t("common.save") : t("group.name.create")}
            otherNames={lib.groups
              .filter((g) => nameDraft.mode !== "rename" || g.id !== nameDraft.id)
              .map((g) => g.name)}
            onCancel={back}
            onSubmit={(name) => submitName(nameDraft, name)}
          />
        ) : null}

        {sheets.includes("translationResult") && translation ? (
          <TranslationResultSheet key="translation" result={translation} onDone={back} />
        ) : null}

        </AnimatePresence>
        <BusyOverlay active={busy !== null} label={busy ?? ""} />
      </>
    );
  };

  return { openPaperMenu, openGroupMenu, newGroup, openBibExport, openBulkGroup, bulkRemove, openSelectMenu, overlays };
}
