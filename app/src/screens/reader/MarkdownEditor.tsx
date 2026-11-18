// 筆記的 Markdown 編輯器（CodeMirror 6，MIT）：只負責編輯文字與格式按鈕的動作；
// 儲存、預覽、窗格的大小都在 `NotesPane`。

import { defaultKeymap, history, historyKeymap, redo, undo } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorSelection, EditorState, Transaction, type ChangeSpec } from "@codemirror/state";
import { EditorView, keymap, placeholder } from "@codemirror/view";
import { useEffect, useImperativeHandle, useRef } from "react";

export type FormatCommand = "heading" | "bold" | "italic" | "list" | "quote" | "link";

export interface EditorHandle {
  format: (cmd: FormatCommand) => void;
  /** 復原、重做（編輯器自己的歷史；外部換掉的整份文字不算在歷史裡）。 */
  history: (cmd: "undo" | "redo") => void;
  getText: () => string;
  /** 換掉整份文字（外部程式改了檔案、這邊沒有未存的修改時用）。 */
  setText: (text: string) => void;
  focus: () => void;
}

const theme = EditorView.theme({
  "&": { color: "var(--text)", backgroundColor: "transparent", fontSize: "16px" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "inherit", lineHeight: "1.6" },
  ".cm-content": { padding: "8px 0", caretColor: "var(--accent)", fontFamily: "inherit", minHeight: "120px" },
  ".cm-line": { padding: "0 2px" },
  ".cm-cursor": { borderLeftColor: "var(--accent)", borderLeftWidth: "2px" },
  ".cm-selectionBackground, &.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground": {
    background: "var(--accent-soft)",
  },
  ".cm-placeholder": { color: "var(--text)", opacity: 0.6 },
});

/** 選取範圍涵蓋的每一行的行首位置。 */
function lineStarts(state: EditorState): number[] {
  const out: number[] = [];
  const seen = new Set<number>();
  for (const r of state.selection.ranges) {
    for (let pos = r.from; pos <= r.to; ) {
      const line = state.doc.lineAt(pos);
      if (!seen.has(line.from)) {
        seen.add(line.from);
        out.push(line.from);
      }
      pos = line.to + 1;
    }
  }
  return out;
}

function toggleLinePrefix(view: EditorView, prefix: string, anyOf?: RegExp) {
  const { state } = view;
  const starts = lineStarts(state);
  const lines = starts.map((from) => state.doc.lineAt(from));
  const has = (text: string) => (anyOf ? anyOf.test(text) : text.startsWith(prefix));
  const allHave = lines.every((l) => has(l.text));
  const changes: ChangeSpec[] = lines.map((l) => {
    if (allHave) {
      const m = anyOf ? (anyOf.exec(l.text)?.[0] ?? "") : prefix;
      return { from: l.from, to: l.from + m.length, insert: "" };
    }
    return { from: l.from, insert: prefix };
  });
  view.dispatch({ changes });
}

function wrap(view: EditorView, mark: string) {
  view.dispatch(
    view.state.changeByRange((r) => {
      if (r.empty) {
        return {
          changes: { from: r.from, insert: mark + mark },
          range: EditorSelection.range(r.from + mark.length, r.from + mark.length),
        };
      }
      const text = view.state.sliceDoc(r.from, r.to);
      if (text.startsWith(mark) && text.endsWith(mark) && text.length >= mark.length * 2) {
        const inner = text.slice(mark.length, text.length - mark.length);
        return {
          changes: { from: r.from, to: r.to, insert: inner },
          range: EditorSelection.range(r.from, r.from + inner.length),
        };
      }
      return {
        changes: [
          { from: r.from, insert: mark },
          { from: r.to, insert: mark },
        ],
        range: EditorSelection.range(r.from + mark.length, r.to + mark.length),
      };
    }),
  );
}

function insertLink(view: EditorView) {
  const r = view.state.selection.main;
  const text = view.state.sliceDoc(r.from, r.to);
  const label = text === "" ? "" : text;
  const insert = `[${label}](網址)`;
  // 選中「網址」兩個字，接著打就能取代
  const urlFrom = r.from + label.length + 3;
  view.dispatch({
    changes: { from: r.from, to: r.to, insert },
    selection: { anchor: urlFrom, head: urlFrom + 2 },
  });
}

export function MarkdownEditor(props: {
  initial: string;
  placeholderText: string;
  label: string;
  onChange: (text: string) => void;
  onBlur: () => void;
  handle: React.Ref<EditorHandle>;
}) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const cb = useRef(props);
  useEffect(() => {
    cb.current = props;
  });

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const v = new EditorView({
      parent: el,
      state: EditorState.create({
        doc: props.initial,
        extensions: [
          history(),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          markdown(),
          syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
          EditorView.lineWrapping,
          placeholder(props.placeholderText),
          EditorView.contentAttributes.of({ "aria-label": props.label, "aria-multiline": "true" }),
          theme,
          EditorView.updateListener.of((u) => {
            if (u.docChanged) cb.current.onChange(u.state.doc.toString());
          }),
          EditorView.domEventHandlers({ blur: () => cb.current.onBlur() }),
        ],
      }),
    });
    view.current = v;
    return () => {
      v.destroy();
      view.current = null;
    };
    // 只在掛上時建立一次；之後的文字由 handle.setText 換
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useImperativeHandle(props.handle, () => ({
    format: (cmd) => {
      const v = view.current;
      if (!v) return;
      v.focus();
      if (cmd === "heading") toggleLinePrefix(v, "## ", /^#{1,6} /);
      else if (cmd === "bold") wrap(v, "**");
      else if (cmd === "italic") wrap(v, "*");
      else if (cmd === "list") toggleLinePrefix(v, "- ", /^[-*] /);
      else if (cmd === "quote") toggleLinePrefix(v, "> ");
      else insertLink(v);
    },
    history: (cmd) => {
      const v = view.current;
      if (!v) return;
      v.focus();
      if (cmd === "undo") undo(v);
      else redo(v);
    },
    getText: () => view.current?.state.doc.toString() ?? "",
    setText: (text) => {
      const v = view.current;
      if (!v) return;
      // 外部程式改了檔案、換成新內容：不進復原的歷史（復原不會把別處改的內容退回去）
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: text }, annotations: Transaction.addToHistory.of(false) });
    },
    focus: () => view.current?.focus(),
  }));

  return <div className="md-editor" ref={host} />;
}
