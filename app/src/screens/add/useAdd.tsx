// 加入文獻的流程。
//
// 第一次加入才說明並要求「所有檔案存取」權限：
//   「去開啟權限」→ 系統授權頁 → 回來後查權限，有就放共用空間，沒有就當作選了「不要」；
//   「不要，放在應用專屬空間」→ 放應用專屬空間並提示解除安裝會一併刪除。之後不再詢問。
// 然後開系統選檔畫面（可以一次選多份）。流程裡的判斷（有沒有權限、放哪裡）都在外殼與核心，這裡只決定畫面怎麼走。
// 只選一份時：已加入／原本就在／在回收區／失敗，各有自己的提示與面板；選了多份時：逐份加入後，
// 用一個結果面板說「加入了幾篇、幾篇原本就在、幾篇失敗」，失敗的可以展開看原因。
// 每份加入成功之後，用 PDF 自帶的篇名與作者補上書目（`pdfInfo.ts`），讀不出來就維持檔名。
//
// 「原本的文獻庫不見了，在原位置建一個新的」也在這裡。搬到共用空間在設定（`settings/useMove.tsx`）。
//
// 說明、提示、確認都是導覽堆疊裡的一層（`nav.ts`），所以返回鍵會先關掉它們。
//
// 別的應用分享進來的 PDF 走同一條路（成功條件 1）：平台轉接器先收下、複製成暫存檔；畫面在啟動與回到前景時問
// 有沒有待加入的（`takeShared`），有就領取並逐一加入，結果和選檔一樣報（一份是提示、多份是結果面板）。
// 還沒有文獻庫時先走第一次加入的說明，建好之後領取分享進來的檔，不開選檔畫面。

import { useCallback, useRef, useState, type ReactNode } from "react";
import { api, kindOf, AppError, type AddBatch, type AddItem, type Storage } from "../../api";
import type { Library } from "../../library";
import { fillFromPdfInfo } from "../../pdfInfo";
import { holdPerPaper } from "../../perPaperApp";
import { topSheet, type Nav } from "../../nav";
import { errorDetail, errorMessage, t } from "../../strings";
import { BusyOverlay, Button, ConfirmSheet, Details, ResultSheet, Sheet } from "../../ui/components";
import { AnimatePresence } from "../../ui/motion";

interface AddUi {
  /** 按下「加入」。 */
  start: () => void;
  /** 原本的文獻庫不見了：使用者確認後，在原位置建一個新的。 */
  createHere: () => void;
  /** 回到前景時呼叫；如果正在等使用者從系統授權頁回來，這裡接著做，交回 `true`。 */
  resumeAfterPermission: () => boolean;
  /** 啟動與回到前景時呼叫：有別的應用分享進來的 PDF 就加入（還沒有文獻庫時先說明）。 */
  takeShared: () => void;
  /** 要顯示的對話框、進行中的提示。放在畫面最上層。 */
  overlays: ReactNode;
  /** 剛加入或帶到的那一篇（首頁會把它捲進畫面並標色）。 */
  highlightId: string | null;
  /** 顯示一則要使用者按「確定」的訊息（面板；返回鍵先關掉它）。原因放「詳情」，能做的事是一顆按鈕。 */
  notify: (title: string, message: string, more?: NoticeMore) => void;
}

type Waiting = "create" | "restore";

/** 訊息的「詳情」與按鈕。 */
export interface NoticeMore {
  detail?: string;
  action?: { label: string; run: () => void };
}

interface Notice extends NoticeMore {
  title: string;
  message: string;
}

/**
 * 去系統的授權頁開「所有檔案存取」，回來再接著做（加入與從另一台加入共用）。
 * `ask` 開授權頁；這個 Android 版本沒有授權頁就當作沒有權限，馬上做 `fallback`。回到前景時 `take` 交回在等的那一種。
 */
export function usePermissionGate<K>() {
  const waiting = useRef<K | null>(null);
  const ask = useCallback(async (kind: K, fallback: () => Promise<void> | void) => {
    waiting.current = kind;
    try {
      await api.openAllFilesSettings();
    } catch {
      waiting.current = null;
      await fallback();
    }
  }, []);
  const take = useCallback((): K | null => {
    const k = waiting.current;
    waiting.current = null;
    return k;
  }, []);
  return { ask, take };
}

export function useAdd(lib: Library, nav: Nav, onMessage: (m: string) => void): AddUi {
  // 進行中的提示文字；`null` 是沒有在進行
  const [busy, setBusy] = useState<string | null>(null);
  const [trashPromptId, setTrashPromptId] = useState<string | null>(null);
  const [error, setError] = useState<Notice | null>(null);
  // 一次加入多份的結果（結果面板用）
  const [batch, setBatch] = useState<AddItem[] | null>(null);
  const [highlightId, setHighlightId] = useState<string | null>(null);
  // 使用者去了系統授權頁，等他回來
  const { ask: askPermission, take: takePermission } = usePermissionGate<Waiting>();
  // 文獻庫建好之後要加入什麼：系統選檔畫面選的，或別的應用分享進來的
  const source = useRef<"picker" | "shared">("picker");
  // 正在處理分享進來的檔（啟動與回到前景可能接連觸發，只處理一次）
  const sharing = useRef(false);
  const sheet = topSheet(nav.stack);

  const showError = useCallback(
    (e: unknown, title: string = t("add.error.title")) => {
      const kind = kindOf(e);
      setError({ title, message: errorMessage(kind), detail: errorDetail(kind) });
      nav.push({ name: "sheet", kind: "message" });
    },
    [nav],
  );

  const notify = useCallback(
    (title: string, message: string, more?: NoticeMore) => {
      setError({ title, message, ...more });
      nav.push({ name: "sheet", kind: "message" });
    },
    [nav],
  );

  const highlight = useCallback((id: string | null) => {
    setHighlightId(id);
    if (id) window.setTimeout(() => setHighlightId((cur) => (cur === id ? null : cur)), 3000);
  }, []);

  /** 加入一批（選檔或分享進來的）：補上 PDF 自帶的書目，再報結果。一份是提示或面板，多份是結果面板。 */
  const runBatch = useCallback(
    async (load: () => Promise<AddBatch>) => {
      setBusy(t("add.busy"));
      // 加入與補篇名作者做完之前，逐篇處理不開始新的一篇：剛加入的一篇要等補完才排進去
      const releasePerPaper = holdPerPaper();
      try {
        const r = await load();
        if (r.cancelled || r.items.length === 0) return;
        // 加入成功的，補上 PDF 自帶的篇名與作者（做不成就維持檔名，不影響加入）
        for (const item of r.items) {
          if (item.outcome === "added" && item.paperId) await fillFromPdfInfo(item.paperId, item.name);
        }
        await lib.refresh();
        const only = r.items.length === 1 ? r.items[0] : undefined;
        if (only) {
          if (only.outcome === "added") {
            onMessage(t("add.added"));
            highlight(only.paperId);
          } else if (only.outcome === "already_present") {
            onMessage(t("add.alreadyPresent"));
            highlight(only.paperId);
          } else if (only.outcome === "in_trash") {
            setTrashPromptId(only.paperId);
            nav.push({ name: "sheet", kind: "inTrash" });
          } else {
            showError(new AppError(only.error ?? "Unknown", ""));
          }
          return;
        }
        setBatch(r.items);
        highlight(r.items.find((i) => i.outcome === "added")?.paperId ?? null);
        nav.push({ name: "sheet", kind: "addResult" });
      } catch (e) {
        showError(e);
      } finally {
        releasePerPaper();
        setBusy(null);
      }
    },
    [lib, nav, onMessage, highlight, showError],
  );

  const runPicker = useCallback(() => runBatch(api.addFromPicker), [runBatch]);

  const runShared = useCallback(async () => {
    sharing.current = true;
    try {
      await runBatch(api.addShared);
    } finally {
      sharing.current = false;
      source.current = "picker";
    }
  }, [runBatch]);

  /** 文獻庫準備好之後：開選檔畫面，或加入分享進來的檔。 */
  const runAdd = useCallback(
    () => (source.current === "shared" ? runShared() : runPicker()),
    [runShared, runPicker],
  );

  const createThenPick = useCallback(
    async (storage: Storage | null) => {
      setBusy(t("add.busy"));
      let privateNow = false;
      try {
        const st = await api.libraryCreate(storage);
        await lib.refresh();
        privateNow = st.storage === "private";
      } catch (e) {
        setBusy(null);
        showError(e);
        return;
      }
      setBusy(null);
      if (privateNow) nav.push({ name: "sheet", kind: "privateNotice" });
      else await runAdd();
    },
    [lib, nav, runAdd, showError],
  );

  const grant = useCallback(
    async (kind: Waiting) => {
      if (nav.top.name === "sheet") await nav.back();
      await askPermission(kind, async () => {
        if (kind === "create") await createThenPick(null);
      });
    },
    [nav, askPermission, createThenPick],
  );

  /**
   * 依文獻庫的狀態開始加入：準備好就直接加入；還沒有文獻庫就先說明並要求權限。
   * 電腦不需要權限：直接在共用空間（文件\PaperReader）建立，不出現說明。
   */
  const begin = useCallback(() => {
    const s = lib.status;
    if (!s) return;
    if (s.state === "ready") void runAdd();
    else if (s.state === "read_only") showError(new AppError("ReadOnly", ""));
    else if (s.state === "access_lost") void grant("restore");
    else if (s.state === "not_created" && s.allFilesAccess) {
      void api
        .platform()
        .catch(() => "android" as const)
        .then((p) => (p === "android" ? nav.push({ name: "sheet", kind: "addIntro" }) : createThenPick(null)));
    } else nav.push({ name: "sheet", kind: "addIntro" });
  }, [lib.status, runAdd, grant, nav, showError, createThenPick]);

  const start = useCallback(() => {
    source.current = "picker";
    begin();
  }, [begin]);

  // 還沒有文獻庫時，分享進來的檔只主動說明一次；使用者關掉說明，分享留著，下次啟動再問
  const askedForShared = useRef(false);
  const takeShared = useCallback(() => {
    if (sharing.current || busy !== null) return;
    sharing.current = true;
    void api
      .sharedPending()
      .then((n) => {
        sharing.current = false;
        if (n === 0) return;
        const ready = lib.status?.state === "ready";
        if (!ready && askedForShared.current) return;
        if (!ready) askedForShared.current = true;
        source.current = "shared";
        begin();
      })
      .catch(() => {
        sharing.current = false;
      });
  }, [busy, lib.status, begin]);

  const createHere = useCallback(() => {
    const storage = lib.status?.storage ?? null;
    setBusy(t("common.working"));
    api
      .libraryCreate(storage)
      .then(() => lib.refresh())
      .catch((e: unknown) => showError(e, t("home.missing.title")))
      .finally(() => setBusy(null));
  }, [lib, showError]);

  const resumeAfterPermission = useCallback(() => {
    const kind = takePermission();
    if (!kind) return false;
    if (kind === "create") void createThenPick(null);
    else void lib.refresh();
    return true;
  }, [takePermission, createThenPick, lib]);

  const restoreFromTrash = async (id: string) => {
    await nav.back();
    try {
      await api.restorePaper(id);
      await lib.refresh();
      onMessage(t("trash.restored"));
      highlight(id);
    } catch (e) {
      showError(e, t("add.restoreFailed.title"));
    }
  };

  const overlays = (
    <>
      <Sheet open={sheet === "addIntro"} title={t("add.intro.title")} onDismiss={() => void nav.back()}>
          <p>{t("add.intro.why")}</p>
          <div className="actions">
            <Button primary block onClick={() => void grant("create")}>
              {t("home.lostAccess.action")}
            </Button>
            <Button
              block
              onClick={() => {
                void (async () => {
                  await nav.back();
                  await createThenPick("private");
                })();
              }}
            >
              {t("add.intro.decline")}
            </Button>
          </div>
        </Sheet>
      <AnimatePresence>
        {sheet === "addResult" && batch ? (
          <BatchResult
            key="batch"
            items={batch}
            onDone={() => void nav.back()}
            onTrash={() => {
              void nav.back().then(() => nav.push({ name: "trash" }));
            }}
          />
        ) : null}
      </AnimatePresence>
      <ConfirmSheet
        open={sheet === "inTrash" && trashPromptId !== null}
        title={t("add.inTrash.title")}
        confirm={t("trash.restore")}
        cancel={t("add.inTrash.keep")}
        onConfirm={() => trashPromptId && void restoreFromTrash(trashPromptId)}
        onCancel={() => void nav.back()}
      >
        <p>{t("add.inTrash.body")}</p>
      </ConfirmSheet>
      <ResultSheet
        open={sheet === "privateNotice"}
        title={t("add.privateNotice.title")}
        onDismiss={() => void nav.back()}
        onDone={() => {
          void (async () => {
            await nav.back();
            await runAdd();
          })();
        }}
      >
        <p>{t("add.privateNotice")}</p>
      </ResultSheet>
      <ResultSheet
        open={sheet === "message" && error !== null}
        title={error?.title ?? ""}
        onDone={() => void nav.back()}
        action={
          error?.action
            ? {
                label: error.action.label,
                run: () => {
                  const run = error.action?.run;
                  void nav.back().then(() => run?.());
                },
              }
            : undefined
        }
      >
        <p>{error?.message}</p>
        {error?.detail ? <Details>{error.detail}</Details> : null}
      </ResultSheet>
      <BusyOverlay active={busy !== null} label={busy ?? ""} />
    </>
  );

  return { start, createHere, resumeAfterPermission, takeShared, overlays, highlightId, notify };
}

/** 一次加入多份之後的結果：幾篇加入、幾篇原本就在、幾篇失敗；失敗的可以展開看原因。 */
function BatchResult(props: { items: AddItem[]; onDone: () => void; onTrash: () => void }) {
  const count = (o: AddItem["outcome"]) => props.items.filter((i) => i.outcome === o).length;
  const failed = props.items.filter((i) => i.outcome === "failed");
  const inTrash = count("in_trash");
  return (
    <ResultSheet
      open
      title={t("add.batch.title")}
      onDone={props.onDone}
      action={inTrash > 0 ? { label: t("home.trashHint.action"), run: props.onTrash, primary: false } : undefined}
    >
      <p data-testid="batch-summary">
        {t("add.batch.summary", { added: count("added"), present: count("already_present"), failed: failed.length })}
      </p>
      {inTrash > 0 ? <p className="muted small">{t("add.batch.inTrash", { n: inTrash })}</p> : null}
      {failed.length > 0 ? (
        <Details label={t("add.batch.failedReasons")}>
          <ul className="problem-list">
            {failed.map((f, i) => (
              <li key={`${i}-${f.name}`}>
                {t("add.batch.failedLine", { name: f.name, reason: errorMessage(f.error ?? "Unknown") })}
              </li>
            ))}
          </ul>
        </Details>
      ) : null}
    </ResultSheet>
  );
}
