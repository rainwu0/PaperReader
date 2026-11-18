// 筆記窗格：閱讀畫面裡一個可以拉高、縮小、開全螢幕的窗格，邊讀邊寫。
//
// - 上面一排：標題、「已儲存」、三種大小（縮小／半開／全開）、選單（用其他應用打開、在 Obsidian 打開、關閉）。
// - 「整篇的想法」是一個 Markdown 編輯器（CodeMirror），有格式按鈕與編輯／預覽切換；停筆一秒自動儲存。
// - 下面列這篇的劃線（頁碼、引句、短評）：點一條跳到那一頁；短評可以就地改。
// 資料與格式都在核心；筆記檔裡讀不懂或不完整的部分只顯示、不提供編輯。窗格的位置與大小由閱讀畫面決定
// （`style`），這裡只管裡面的東西。

import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import type { Highlight } from "../../api";
import { t } from "../../strings";
import { Button } from "../../ui/components";
import { AnimatePresence, m as Motion, useMotion } from "../../ui/motion";
import { MarkdownEditor, type EditorHandle, type FormatCommand } from "./MarkdownEditor";
import { renderMarkdown } from "./markdown";
import { TrashIcon } from "./toolIcons";
import { appendPart, unsavedPart } from "./notesConflict";
import type { NotesState } from "./useNotes";

const SAVE_AFTER_MS = 1000;

export type PaneSize = "peek" | "half" | "full";

/** 「第 7 頁」：頁碼標籤優先（可能是 iv 這種），沒有就用頁序加一。 */
export function pageText(h: Highlight): string {
  return t("home.continue.page", { page: h.page.label ?? String(h.page.index + 1) });
}

const FORMATS: Array<{ cmd: FormatCommand; label: string; face: string; aria: Parameters<typeof t>[0] }> = [
  { cmd: "heading", face: "H", label: "標題", aria: "reader.notes.fmt.heading" },
  { cmd: "bold", face: "B", label: "粗體", aria: "reader.notes.fmt.bold" },
  { cmd: "italic", face: "I", label: "斜體", aria: "reader.notes.fmt.italic" },
  { cmd: "list", face: "•", label: "清單", aria: "reader.notes.fmt.list" },
  { cmd: "quote", face: "❝", label: "引用", aria: "reader.notes.fmt.quote" },
  { cmd: "link", face: "🔗", label: "連結", aria: "reader.notes.fmt.link" },
];

export function NotesPane(props: {
  notes: NotesState;
  title: string;
  size: PaneSize;
  landscape: boolean;
  style: CSSProperties;
  onSize: (s: PaneSize) => void;
  /** 拖動分隔線：交回新的比例（窗格占畫面高度或寬度的幾分之幾）。 */
  onSplit: (fraction: number) => void;
  onClose: () => void;
  onJump: (h: Highlight) => void;
  /** 刪一條劃線（可以從提示復原一次）。 */
  onDelete: (id: string) => void;
  /** 剛刪、還能復原的：先不列。 */
  hiddenIds: readonly string[];
  onMessage: (m: string) => void;
  onOpenExternal: (obsidian: boolean) => void;
}) {
  const { notes, size } = props;
  const doc = notes.view?.doc ?? null;
  const editor = useRef<EditorHandle>(null);
  const textRef = useRef("");
  const savedRef = useRef<string | null>(null); // 筆記檔裡現在的想法（最後讀到或寫入的）
  const applying = useRef(false);
  const timer = useRef(0);
  const [state, setState] = useState<"idle" | "dirty" | "saving" | "saved">("idle");
  const [mode, setMode] = useState<"edit" | "preview">("edit");
  const [previewHtml, setPreviewHtml] = useState("");
  const [menu, setMenu] = useState(false);
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);

  const complete = doc?.thoughts_complete ?? true;
  const ready = doc !== null;

  // 第一次讀到筆記：編輯器的起始文字
  const [initial, setInitial] = useState<string | null>(null);
  useEffect(() => {
    if (doc && initial === null) {
      setInitial(doc.thoughts);
      textRef.current = doc.thoughts;
      savedRef.current = doc.thoughts;
    }
  }, [doc, initial]);

  /** 編輯器換成這段文字（不算使用者的修改）。 */
  const putText = (text: string) => {
    applying.current = true;
    editor.current?.setText(text);
    applying.current = false;
    textRef.current = text;
  };

  /**
   * 筆記檔裡的想法在別處（例如 Obsidian）改過：編輯器換成檔案的新內容，不把手上的舊版本寫回去。
   * 使用者剛才打、還沒存進去的那段留在 `notes.stash`，窗格上方顯示，讓他複製或加到最後。
   */
  const takeTheirs = (theirs: string) => {
    window.clearTimeout(timer.current);
    again.current = false;
    const part = unsavedPart(savedRef.current ?? "", textRef.current);
    const lost = textRef.current !== theirs;
    putText(theirs);
    savedRef.current = theirs;
    setState("idle");
    if (!lost) return;
    if (part !== "") notes.setStash((prev) => (prev ? appendPart(prev, part) : part));
    props.onMessage(t("reader.notes.replaced"));
  };
  const takeTheirsRef = useRef(takeTheirs);
  useEffect(() => {
    takeTheirsRef.current = takeTheirs;
  });

  const saving = useRef(false); // 有一次寫入還沒回來
  const again = useRef(false); // 寫入途中又要存：回來之後再存一次
  const save = useCallback(
    async (retry = true): Promise<void> => {
      window.clearTimeout(timer.current);
      if (!complete) return;
      if (saving.current) {
        again.current = true;
        return;
      }
      const sent = textRef.current;
      if (sent === savedRef.current) {
        setState((s) => (s === "dirty" ? "idle" : s));
        return;
      }
      saving.current = true;
      setState("saving");
      const r = await notes.setThoughts(sent);
      saving.current = false;
      if (r.conflict) {
        const theirs = notes.latest()?.doc.thoughts;
        if (theirs === undefined) {
          setState("dirty");
          return;
        }
        // 別處只改了劃線或短評，想法還是我們手上的那一版：用新讀到的版本記號再存一次
        if (theirs === savedRef.current && retry) return saveRef.current(false);
        takeTheirsRef.current(theirs);
        return;
      }
      if (r.message) {
        props.onMessage(r.message);
        setState("dirty");
        return;
      }
      savedRef.current = sent;
      setState(textRef.current === sent ? "saved" : "dirty");
      if (again.current) {
        again.current = false;
        void saveRef.current();
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [complete, notes.setThoughts, notes.latest, props.onMessage],
  );

  const saveRef = useRef(save);
  useEffect(() => {
    saveRef.current = save;
  });
  // 關掉窗格時，還沒存的立刻存（撞上別處的修改就不寫，沒存的那段留在 `notes.stash`）
  useEffect(() => {
    return () => {
      window.clearTimeout(timer.current);
      void saveRef.current();
    };
  }, []);

  // 筆記檔重新讀回來（回到前景、別處改過）：想法變了就換成新的內容。
  // 寫入途中不管（撞上了由 `save` 處理）；手上有還沒存的修改，那段留下來告訴使用者
  useEffect(() => {
    if (!doc || initial === null) return;
    if (doc.thoughts === savedRef.current) return;
    if (saving.current) return;
    if (textRef.current !== savedRef.current) {
      takeTheirs(doc.thoughts);
      return;
    }
    putText(doc.thoughts);
    savedRef.current = doc.thoughts;
    setState("idle");
    // 只看筆記檔的內容有沒有變
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [doc?.thoughts]);

  /** 沒存進去的那段：加到想法的最後（照一般的修改存）。 */
  const appendStash = () => {
    const part = notes.stash;
    if (!part) return;
    notes.setStash(null);
    const next = appendPart(textRef.current, part);
    putText(next);
    onChange(next);
  };
  const copyStash = async () => {
    try {
      await navigator.clipboard.writeText(notes.stash ?? "");
      props.onMessage(t("reader.notes.stash.copied"));
    } catch {
      props.onMessage(t("reader.notes.stash.copyFailed"));
    }
  };

  const onChange = (text: string) => {
    if (applying.current) return;
    textRef.current = text;
    setState("dirty");
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void save(), SAVE_AFTER_MS);
  };

  const switchMode = (m: "edit" | "preview") => {
    if (m === "preview") setPreviewHtml(renderMarkdown(textRef.current));
    setMode(m);
  };

  // ---------- 拖動分隔線調整大小 ----------
  const drag = useRef<{ id: number } | null>(null);
  const onHandleDown = (e: React.PointerEvent) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { id: e.pointerId };
  };
  const onHandleMove = (e: React.PointerEvent) => {
    if (!drag.current) return;
    const f = props.landscape
      ? (window.innerWidth - e.clientX) / window.innerWidth
      : (window.innerHeight - e.clientY) / window.innerHeight;
    props.onSplit(f);
  };
  const onHandleUp = () => {
    drag.current = null;
  };

  const highlights = (doc?.highlights ?? []).filter((h) => !props.hiddenIds.includes(h.id));
  const stateText =
    state === "saving"
      ? t("reader.notes.saving")
      : state === "saved"
        ? t("info.saved")
        : state === "dirty"
          ? t("reader.notes.unsaved")
          : ready && complete
            ? t("info.saved")
            : "";

  const saveComment = async () => {
    if (!editing) return;
    const e = editing;
    setEditing(null);
    const h = highlights.find((x) => x.id === e.id);
    if (!h || h.comment === e.text) return;
    const m = await notes.setComment(e.id, e.text);
    if (m) props.onMessage(m);
  };

  const cls = `notes-pane ${size} ${props.landscape ? "landscape" : "portrait"}`;
  // 直式從底部滑入、橫式半開從右側滑入；換大小不補間（動高度會每格重新排版）
  const slide = useMotion(props.landscape && size === "half" ? "paneRight" : "paneUp");
  const pop = useMotion("dialog");

  return (
    <Motion.section className={cls} style={props.style} aria-label={t("reader.notes.title")} {...slide}>
      {size === "half" ? (
        <div
          className="np-handle"
          role="separator"
          aria-orientation={props.landscape ? "vertical" : "horizontal"}
          aria-label={t("reader.notes.resize")}
          onPointerDown={onHandleDown}
          onPointerMove={onHandleMove}
          onPointerUp={onHandleUp}
          onPointerCancel={onHandleUp}
        >
          <span />
        </div>
      ) : null}

      <header className="np-head">
        <button
          type="button"
          className="np-title"
          aria-expanded={size !== "peek"}
          onClick={() => props.onSize(size === "peek" ? "half" : "peek")}
        >
          <strong>{t("reader.notes.title")}</strong>
          <span className="np-doc-title">{props.title}</span>
        </button>
        <span className="np-state" role="status" aria-live="polite">
          {stateText}
        </span>
        <div className="np-sizes" role="group" aria-label={t("reader.notes.size")}>
          {(["peek", "half", "full"] as const).map((s) => (
            <button
              key={s}
              type="button"
              aria-pressed={size === s}
              aria-label={t(`reader.notes.size.${s}`)}
              onClick={() => props.onSize(s)}
            >
              <SizeGlyph kind={s} />
            </button>
          ))}
        </div>
        <button
          type="button"
          className="np-more"
          aria-label={t("reader.notes.menu")}
          aria-expanded={menu}
          onClick={() => setMenu((v) => !v)}
        >
          ⋯
        </button>
        <AnimatePresence>
        {menu ? (
          <Motion.div key="menu" className="np-menu" role="menu" {...pop}>
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                setMenu(false);
                props.onOpenExternal(false);
              }}
            >
              {t("reader.notes.openWith")}
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                setMenu(false);
                props.onOpenExternal(true);
              }}
            >
              {t("reader.notes.openObsidian")}
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                setMenu(false);
                props.onClose();
              }}
            >
              {t("reader.notes.closePane")}
            </button>
          </Motion.div>
        ) : null}
        </AnimatePresence>
      </header>

      <div className="np-body" hidden={size === "peek"}>
        {notes.loadError ? <p className="field-error">{notes.loadError}</p> : null}
        {!doc && !notes.loadError ? (
          <p className="muted" role="status">
            {t("common.loading")}
          </p>
        ) : null}

        {notes.stash ? (
          <div className="np-stash" role="alert">
            <p>{t("reader.notes.stash")}</p>
            <p className="np-stash-text">{notes.stash}</p>
            <div className="np-stash-actions">
              <Button onClick={() => void copyStash()}>{t("reader.notes.stash.copy")}</Button>
              {complete ? <Button onClick={appendStash}>{t("reader.notes.stash.append")}</Button> : null}
              <Button onClick={() => notes.setStash(null)}>{t("reader.notes.stash.drop")}</Button>
            </div>
          </div>
        ) : null}

        {doc && complete && initial !== null ? (
          <>
            <div className="np-toolbar" role="toolbar" aria-label={t("bib.format")}>
              {FORMATS.map((f) => (
                <button
                  key={f.cmd}
                  type="button"
                  className={`np-fmt fmt-${f.cmd}`}
                  aria-label={t(f.aria)}
                  disabled={mode === "preview"}
                  // 按下去時不要讓編輯器失去焦點（鍵盤才不會收起來）
                  onPointerDown={(e) => e.preventDefault()}
                  onClick={() => editor.current?.format(f.cmd)}
                >
                  {f.face}
                </button>
              ))}
              {/* 復原與重做：接編輯器自己的歷史，手機上沒有 Ctrl+Z */}
              {(["undo", "redo"] as const).map((cmd) => (
                <button
                  key={cmd}
                  type="button"
                  className={`np-fmt fmt-${cmd}`}
                  aria-label={t(`reader.notes.fmt.${cmd}`)}
                  disabled={mode === "preview"}
                  onPointerDown={(e) => e.preventDefault()}
                  onClick={() => editor.current?.history(cmd)}
                >
                  {cmd === "undo" ? "↶" : "↷"}
                </button>
              ))}
              <span className="np-spacer" />
              <div className="np-mode" role="group" aria-label={t("reader.notes.mode")}>
                <button type="button" aria-pressed={mode === "edit"} onClick={() => switchMode("edit")}>
                  {t("reader.notes.edit.mode")}
                </button>
                <button type="button" aria-pressed={mode === "preview"} onClick={() => switchMode("preview")}>
                  {t("reader.notes.preview.mode")}
                </button>
              </div>
            </div>
            <div className="np-editor" hidden={mode === "preview"}>
              <MarkdownEditor
                handle={editor}
                initial={initial}
                placeholderText={t("reader.notes.thoughtsHint")}
                label={t("reader.notes.thoughts")}
                onChange={onChange}
                onBlur={() => void save()}
              />
            </div>
            {mode === "preview" ? (
              <div
                className="np-preview md-preview"
                // 預覽不允許原始 HTML（markdown.ts）；連結不在這裡開啟
                onClick={(e) => {
                  if ((e.target as HTMLElement).closest("a")) e.preventDefault();
                }}
                // oxlint-disable-next-line react/no-danger
                dangerouslySetInnerHTML={{
                  __html: previewHtml === "" ? `<p class="muted">${t("reader.notes.previewEmpty")}</p>` : previewHtml,
                }}
              />
            ) : null}
          </>
        ) : null}
        {doc && !complete ? (
          <>
            <p className="notice">{t("reader.notes.incomplete")}</p>
            <p className="notes-readonly">{doc.thoughts}</p>
          </>
        ) : null}

        {doc ? (
          <>
            <h3 className="notes-section">{t("reader.notes.highlights", { n: highlights.length })}</h3>
            {highlights.length === 0 ? <p className="muted">{t("reader.notes.empty")}</p> : null}
            <ul className="notes-list">
              {highlights.map((h) => (
                <li key={h.id} className="notes-item">
                  <div className="notes-main">
                    <button type="button" className="notes-jump" onClick={() => props.onJump(h)}>
                      <span className="notes-page">{pageText(h)}</span>
                      <span className="notes-quote">{h.quote.exact}</span>
                    </button>
                    {editing?.id === h.id ? (
                      <div className="notes-inline">
                        <textarea
                          className="input"
                          rows={3}
                          autoFocus
                          value={editing.text}
                          aria-label={t("reader.tool.comment")}
                          placeholder={t("reader.edit.hint")}
                          onChange={(e) => setEditing({ id: h.id, text: e.target.value })}
                          onBlur={() => void saveComment()}
                        />
                      </div>
                    ) : h.complete ? (
                      <button
                        type="button"
                        className={h.comment.trim() === "" ? "notes-comment none" : "notes-comment"}
                        aria-label={t("reader.notes.edit", { page: pageText(h) })}
                        onClick={() => setEditing({ id: h.id, text: h.comment })}
                      >
                        {h.comment.trim() === "" ? t("reader.notes.noComment") : h.comment}
                      </button>
                    ) : (
                      <p className="muted small">{t("reader.notes.incomplete")}</p>
                    )}
                  </div>
                  {h.complete ? (
                    <button
                      type="button"
                      className="notes-delete"
                      aria-label={t("reader.notes.delete", { page: pageText(h) })}
                      onClick={() => props.onDelete(h.id)}
                    >
                      <TrashIcon />
                    </button>
                  ) : null}
                </li>
              ))}
            </ul>
            {doc.unreadable.length > 0 ? (
              <p className="muted small">{t("reader.notes.unreadable", { n: doc.unreadable.length })}</p>
            ) : null}
          </>
        ) : null}
      </div>
    </Motion.section>
  );
}

/** 三種大小的小圖：縮小（一條線）、半開（上下分開）、全開（滿格）。 */
function SizeGlyph(props: { kind: PaneSize }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <rect x="4" y="4" width="16" height="16" rx="2" />
      {props.kind === "peek" ? <path d="M4 17h16" /> : null}
      {props.kind === "half" ? <path d="M4 12h16" /> : null}
      {props.kind === "full" ? <path d="M8 8h8v8H8z" /> : null}
    </svg>
  );
}
