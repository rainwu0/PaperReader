import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  AppError,
  kindOf,
  type BibDraft,
  type Group,
  type Material,
  type PaperDetail,
  type ErrorKind,
  type ReadingProgress,
} from "../../api";
import { relativeDay, timeOfDay } from "../../format";
import { ExtInfoSections } from "../../ext/InfoSections";
import { BibFields, useBibForm } from "./BibFields";
import type { Nav, SheetKind } from "../../nav";
import { errorDetail, errorMessage, t } from "../../strings";
import { createGroupWith } from "../../library";
import {
  BusyOverlay,
  Button,
  CheckRow,
  Details,
  ErrorNotice,
  InfoTip,
  Screen,
  Sheet,
  type ShowUndo,
} from "../../ui/components";
import { useAddTranslation } from "../add/useAddTranslation";
import { GroupNameSheet, GroupPickList } from "../home/groupUi";
import { digestStateLabel, hasTranslation, materialLabel, opensElsewhere } from "../home/paperMeta";
import { lastDigest, ROLE_SOURCE } from "../../api/types";
import { ExportSheet } from "../export/exportUi";
import { AnimatePresence } from "../../ui/motion";

function statusLine(p: ReadingProgress | null): string {
  if (!p) return "";
  switch (p.status) {
    case "Read":
      return t("home.status.read");
    case "Reading":
      return p.position
        ? t("info.status.readingPage", { page: p.position.page })
        : t("home.status.reading");
    default:
      return t("home.status.unread");
  }
}

interface InfoError {
  message: string;
  detail?: string;
  conflict: boolean;
}

/** 出錯的提示：表面一句、原因放詳情；衝突時另給「重新載入」。 */
function noticeOf(kind: ErrorKind): InfoError {
  return { message: errorMessage(kind), detail: errorDetail(kind), conflict: kind === "Conflict" };
}

/** 文獻資訊：看與改篇名、作者、年份；標示已讀；移除這篇。 */
export function PaperInfo(props: {
  paperId: string;
  nav: Nav;
  /** 核心標的：這篇是較新版本的應用建立的，只能看不能改。 */
  needsNewerApp: boolean;
  /** 面板正蓋在這個畫面上時，是哪一種（返回鍵會先關掉它）。 */
  sheet: SheetKind | null;
  /** 全部的群組，以及這篇現在在哪些群組裡（目錄重新讀過就會更新）。 */
  groups: Group[];
  memberOf: string[];
  /** 資料有改動：請首頁重新整理目錄。回傳的承諾完成時，上面的 `groups`、`memberOf` 已是新的。 */
  onChanged: () => void | Promise<void>;
  /** 移除成功、回到首頁之後呼叫（首頁顯示帶「復原」的提示）。 */
  onRemoved: (paperId: string) => void;
  /** 短暫的提示（App 的那一個）。 */
  toast: (message: string) => void;
  /** 做完可以反做的動作（存書目、移除材料）：提示給「復原」。 */
  undo: ShowUndo;
}) {
  const { paperId, nav } = props;
  const { setGuard, push } = nav;
  const [detail, setDetail] = useState<PaperDetail | null>(null);
  const [progress, setProgress] = useState<ReadingProgress | null>(null);
  const form = useBibForm();
  const resetForm = form.reset;
  const [error, setError] = useState<InfoError | null>(null);
  const [busy, setBusy] = useState(false);
  const [pendingGroup, setPendingGroup] = useState<string | null>(null);
  const toast = props.toast;
  const undo = props.undo;

  const load = useCallback(async () => {
    try {
      const [d, p] = await Promise.all([api.getPaper(paperId), api.readingProgress(paperId)]);
      setDetail(d);
      setProgress(p);
      resetForm(d.paper.bib);
      setError(null);
    } catch (e) {
      setError(noticeOf(kindOf(e)));
    }
  }, [paperId, resetForm]);

  useEffect(() => {
    void load();
  }, [load]);

  // 從「產生」等畫面回來時，材料清單可能變了：重讀描述（不動正在改的欄位）
  const onTop = nav.top.name === "info";
  const loaded = detail !== null;
  useEffect(() => {
    if (!onTop || !loaded) return;
    let alive = true;
    api
      .getPaper(paperId)
      .then((d) => alive && setDetail(d))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [onTop, loaded, paperId]);

  // 群組、譯本的操作失敗時，說明放在畫面最上面；把它捲進畫面，免得使用者看不到
  const errorRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (error) errorRef.current?.scrollIntoView({ block: "nearest" });
  }, [error]);

  const bib = detail?.paper.bib;
  const yearValid = form.valid;
  const dirty = !!bib && form.dirty(bib);
  const conflict = error?.conflict === true;

  // 有還沒存的修改時，離開（返回鍵、返回手勢、頂端的返回）前先問一次
  useEffect(() => {
    if (!dirty) {
      setGuard(null);
      return;
    }
    setGuard(() => {
      push({ name: "sheet", kind: "discardConfirm" });
      return true;
    });
    return () => setGuard(null);
  }, [dirty, setGuard, push]);

  /**
   * 群組與譯本的變動會改到這篇的描述檔（版本記號跟著變），所以重新讀一次詳情；
   * 但不動使用者正在改的欄位，也不動閱讀狀態。
   */
  const refreshDetail = async () => {
    try {
      setDetail(await api.getPaper(paperId));
    } catch {
      // 讀不回來就維持現狀；真的要存書目時，版本衝突會當場說明
    }
  };

  const addTranslation = useAddTranslation({
    paperId,
    nav,
    sheet: props.sheet,
    toast,
    onChanged: async () => {
      await refreshDetail();
      await props.onChanged();
    },
  });

  if (!detail || !bib) {
    return (
      <Screen title={t("info.title")} onBack={() => void nav.back()}>
        {error ? <ErrorNotice message={error.message} detail={error.detail} onAction={() => void load()} /> : null}
      </Screen>
    );
  }

  const readOnlyPaper = props.needsNewerApp;

  // 閱讀畫面這一版開不了的 Markdown 材料（匯入的譯本、抽出文字）：交一份複本給其他應用
  const openElsewhere = async (materialId: string) => {
    try {
      const r = await api.materialOpenExternal(paperId, materialId);
      if (r === "no_app") toast(t("digest.view.noApp"));
    } catch (e) {
      toast(errorMessage(kindOf(e)));
    }
  };

  /** 存檔；成功交回 `true`。失敗時顯示原因，畫面上的欄位維持使用者輸入的，沒有存成的不會顯示成已存。 */
  const save = async (): Promise<boolean> => {
    if (!yearValid) return false;
    setBusy(true);
    // 存之前的書目：「復原」時存回去
    const before: BibDraft = {
      title: bib.title ?? null,
      authors: bib.authors,
      year: bib.year ?? null,
      venue: bib.venue ?? null,
      doi: bib.doi ?? null,
      url: bib.url ?? null,
    };
    try {
      await api.updateBib(paperId, form.draft(bib), detail.token);
      // 以核心存好的樣子為準，重新讀回來顯示
      await load();
      props.onChanged();
      undo(t("info.saved"), async () => {
        // 版本記號在按「復原」時重新讀（存好之後已經換了）
        const now = await api.getPaper(paperId);
        await api.updateBib(paperId, before, now.token);
        await load();
      });
      return true;
    } catch (e) {
      const kind = kindOf(e);
      setError(noticeOf(kind));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const toggleRead = async (read: boolean) => {
    try {
      await api.setRead(paperId, read);
      setProgress(await api.readingProgress(paperId));
      props.onChanged();
    } catch (e) {
      setError(noticeOf(kindOf(e)));
      // 開關要顯示實際的狀態，不是使用者想要的
      setProgress(await api.readingProgress(paperId).catch(() => progress));
    }
  };

  const toggleGroup = async (g: Group, member: boolean) => {
    setError(null);
    setPendingGroup(g.id);
    try {
      await api.setGroupMember(paperId, g.id, member);
    } catch (e) {
      setError(noticeOf(kindOf(e)));
    } finally {
      // 不管成不成，畫面都以實際的結果為準
      await refreshDetail();
      await props.onChanged();
      setPendingGroup(null);
    }
  };

  const createGroup = async (name: string) => {
    const { memberFailed } = await createGroupWith(name, [paperId]);
    await refreshDetail();
    await props.onChanged();
    await nav.back();
    if (memberFailed) setError({ message: t("group.name.memberFailed"), conflict: false });
  };

  /**
   * 移除一項材料：檔案移到回收區，不先問，給「復原」；之後以核心的結果重讀材料清單。
   * 復原：回收區裡這篇、這一項最晚移除的那一份放回來（核心的移除不交回回收區的資料夾名稱）。
   */
  const removeMaterial = async (m: Material) => {
    setBusy(true);
    setError(null);
    try {
      await api.removeMaterial(paperId, m.id, detail.token);
      undo(t("info.removed"), async () => {
        const latest = (await api.listRemovedMaterials())
          .filter((e) => e.paper_id === paperId && e.material.id === m.id)
          .sort((a, b) => (b.removed_at ?? "").localeCompare(a.removed_at ?? ""))[0];
        if (!latest) throw new AppError("NotFound", "removed material");
        await api.restoreMaterial(latest.folder);
        await refreshDetail();
      });
    } catch (e) {
      setError(noticeOf(kindOf(e)));
    } finally {
      await refreshDetail();
      await props.onChanged();
      setBusy(false);
    }
  };

  /** 移除這篇（不先問）：回到首頁，首頁的提示給「復原」。 */
  const remove = async () => {
    setBusy(true);
    try {
      await api.removePaper(paperId);
      props.onChanged();
      setGuard(null);
      await nav.home();
      props.onRemoved(paperId);
    } catch (e) {
      setError(noticeOf(kindOf(e)));
    } finally {
      setBusy(false);
    }
  };

  /** 離開前的三個選擇都先關掉那個確認面板。 */
  const leaveWithoutSaving = async () => {
    setGuard(null);
    await nav.back(); // 關掉面板
    await nav.back(); // 離開這個畫面
  };
  const saveAndLeave = async () => {
    if (await save()) {
      setGuard(null);
      await nav.back();
      await nav.back();
    } else {
      await nav.back(); // 存不成：關掉面板，留在這個畫面看原因
    }
  };

  return (
    <>
      <Screen title={t("info.title")} onBack={() => void nav.back()}>
        <div className="content">
          {error ? (
            <div className="notice" role="alert" ref={errorRef}>
              <div>{error.message}</div>
              {error.detail ? <Details>{error.detail}</Details> : null}
              {error.conflict ? (
                <>
                  <p className="muted small">{t("info.conflict.replaces")}</p>
                  <Button onClick={() => void load()}>{t("info.conflict.reload")}</Button>
                </>
              ) : null}
            </div>
          ) : null}
          {readOnlyPaper ? <div className="notice">{t("common.readOnly")}</div> : null}

          <BibFields form={form} disabled={readOnlyPaper} />
          {/* 衝突期間不能存：要先「重新載入」，不然一按就又被擋一次 */}
          <Button
            primary
            disabled={!dirty || !yearValid || readOnlyPaper || conflict}
            onClick={() => void save()}
          >
            {t("common.save")}
          </Button>

          <h2 className="section">
            {t("info.status.heading")}
            <InfoTip label={t("info.status.heading")}>{t("info.markReadHint")}</InfoTip>
          </h2>
          <p>{statusLine(progress)}</p>
          <CheckRow checked={progress?.status === "Read"} onChange={(read) => void toggleRead(read)}>
            {t("info.markRead")}
          </CheckRow>

          <h2 className="section">{t("info.groups.heading")}</h2>
          {props.groups.length === 0 ? (
            <p className="muted small">{t("group.none")}</p>
          ) : (
            <>
              <GroupPickList
                groups={props.groups}
                memberOf={props.memberOf}
                pending={pendingGroup}
                disabled={readOnlyPaper}
                onToggle={(g, member) => void toggleGroup(g, member)}
              />
            </>
          )}
          <div style={{ height: 8 }} />
          <Button block disabled={readOnlyPaper} onClick={() => push({ name: "sheet", kind: "groupName" })}>
            {t("sidebar.newGroup")}
          </Button>

          <h2 className="section">{t("info.materials.heading")}</h2>
          {hasTranslation(detail.paper.materials) ? null : <p>{t("reader.noTranslation")}</p>}
          <ul className="list info-materials">
            {detail.paper.materials.map((m) => (
              <li key={m.id} className="row">
                <div className="row-main" style={{ cursor: "default" }}>
                  <span className="row-text">
                    <span className="row-title">{materialLabel(m)}</span>
                    <span className="row-sub">
                      {/* 頁數；原文以外再寫做出來的時間，同一種材料有兩份時分得出哪一份 */}
                      {[
                        m.pages ? t("info.material.pages", { n: m.pages }) : "",
                        m.role !== ROLE_SOURCE && m.provenance?.time
                          ? t("info.material.made", {
                              when: relativeDay(m.provenance.time),
                              time: timeOfDay(m.provenance.time),
                            })
                          : "",
                      ]
                        .filter((x) => x !== "")
                        .join("・") || " "}
                    </span>
                  </span>
                </div>
                {opensElsewhere(m) ? (
                  <div style={{ paddingLeft: 12 }}>
                    <Button onClick={() => void openElsewhere(m.id)}>{t("digest.view.openWith")}</Button>
                  </div>
                ) : null}
                {m.role !== ROLE_SOURCE ? (
                  <div style={{ padding: "0 12px" }}>
                    <Button disabled={readOnlyPaper || busy} onClick={() => void removeMaterial(m)}>
                      {t("services.remove")}
                    </Button>
                  </div>
                ) : null}
              </li>
            ))}
          </ul>
          <Button block disabled={readOnlyPaper} onClick={() => void addTranslation.start()}>
            {t("translation.add")}
          </Button>

          <h2 className="section">{t("info.digest.heading")}</h2>
          {(() => {
            const d = lastDigest(detail.paper.materials);
            return d ? (
              <>
                <p>
                  {t("info.digest.has", {
                    state: digestStateLabel(d.provenance?.method, !!d.provenance?.proofread),
                  })}
                </p>
                <Button block onClick={() => push({ name: "reader", paperId, view: "digest" })}>
                  {t("digest.open")}
                </Button>
              </>
            ) : (
              <>
                <p>{t("info.digest.none")}</p>
                <Button block disabled={readOnlyPaper} onClick={() => push({ name: "generate", paperId, kind: "digest" })}>
                  {t("info.digest.make")}
                </Button>
              </>
            );
          })()}

          <ExtInfoSections paperId={paperId} nav={nav} />
          <h2 className="section">{t("export.title")}</h2>
          <Button block onClick={() => push({ name: "sheet", kind: "exportMenu" })}>
            {t("export.title")}
          </Button>

          <h2 className="section">&nbsp;</h2>
          <Button block onClick={() => push({ name: "generate", paperId })}>
            {t("info.generate")}
          </Button>
          <div style={{ height: 12 }} />
          <Button block onClick={() => void remove()} disabled={readOnlyPaper || busy}>
            {t("info.remove")}
          </Button>
        </div>
      </Screen>

      <Sheet open={props.sheet === "discardConfirm"} title={t("info.unsaved.title")} center onDismiss={() => void nav.back()}>
          <p>{t("info.unsaved.body")}</p>
          <div className="actions">
            <Button primary block disabled={!yearValid || conflict} onClick={() => void saveAndLeave()}>
              {t("info.unsaved.save")}
            </Button>
            <Button block onClick={() => void leaveWithoutSaving()}>
              {t("info.unsaved.discard")}
            </Button>
            <Button block onClick={() => void nav.back()}>
              {t("info.unsaved.stay")}
            </Button>
          </div>
        </Sheet>
      <AnimatePresence>
      {props.sheet === "groupName" ? (
        <GroupNameSheet
          key="groupName"
          title={t("sidebar.newGroup")}
          initial=""
          submitLabel={t("group.name.create")}
          otherNames={props.groups.map((g) => g.name)}
          onCancel={() => void nav.back()}
          onSubmit={createGroup}
        />
      ) : null}
      {props.sheet === "exportMenu" ? <ExportSheet key="export" paperId={paperId} onClose={() => void nav.back()} /> : null}
      </AnimatePresence>
      {addTranslation.overlays}
      <BusyOverlay active={busy} label={t("common.working")} />
    </>
  );
}
