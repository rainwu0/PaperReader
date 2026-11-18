// 畫面之間的移動。用瀏覽器的歷史紀錄堆疊：每進一層（畫面或面板）就 pushState 一筆。
// Android 的返回鍵與返回手勢：不在首頁時由這裡接手、退一層；在首頁時交給系統。所以面板 → 關掉面板、
// 閱讀畫面 → 回首頁、首頁 → 離開。畫面不自製返回的提示。
//
// 面板（對話框、說明頁）也是堆疊裡的一層：返回鍵先關掉它，不會直接離開畫面。

import { onBackButtonPress } from "@tauri-apps/api/app";
import type { PluginListener } from "@tauri-apps/api/core";
import { createContext, useCallback, useEffect, useRef, useState } from "react";

export type SheetKind =
  /** 首頁的側邊欄（從左邊滑出，蓋在內容上）。 */
  | "sidebar"
  /** 首頁展開的搜尋列。 */
  | "search"
  | "sort"
  /** 閱讀畫面：跳頁、大綱。 */
  | "jump"
  | "outline"
  /**
   * 閱讀畫面：譯文（`compare`，蓋滿畫面）與它的檢查譯文、目錄、選項；正在改的一段；PDF 上點一段的小框；
   * 檢視切換（原文、譯本、原頁疊譯文、譯文、重點筆記）。
   */
  | "compare"
  | "compareCheck"
  | "compareToc"
  | "compareMenu"
  | "segEdit"
  | "segPop"
  /** 閱讀畫面：點一條劃線出的小選單（刪除、短評）。 */
  | "hlPop"
  | "views"
  /** 閱讀畫面：重點筆記（蓋滿畫面）、刪一點的確認。 */
  | "digest"
  | "digestDelete"
  /** 閱讀畫面：筆記窗格半開或全開（返回鍵先把它縮成一條）。 */
  | "notes"
  /** 一次加入多份之後的結果。 */
  | "addResult"
  /** 首頁「＋」的選單（選 PDF、整批匯入）；整批匯入的選資料夾或 zip、結果。 */
  | "addMenu"
  | "importSource"
  | "importResult"
  /** 首頁與文獻資訊的群組：勾選清單、名稱輸入、列的選單（側邊欄裡一個群組的選單也是這一種）。 */
  | "groupPick"
  | "groupName"
  | "groupMenu"
  /** 加入譯本的結果。 */
  | "translationResult"
  /** 設定的備份：匯出完成後怎麼交出、還原前的確認、結果。 */
  | "backupDeliver"
  | "restoreConfirm"
  | "backupResult"
  /** 設定：搬到共用空間的權限說明、確認（或改用共用空間已有的）、進度。 */
  | "moveIntro"
  | "moveConfirm"
  | "moveProgress"
  /** 閱讀畫面：一條劃線的短評。 */
  | "highlightEdit"
  | "addIntro"
  | "inTrash"
  | "privateNotice"
  | "message"
  /** 回收區：永久刪除一項、全部清空的確認。 */
  | "purgeConfirm"
  | "emptyTrash"
  /** 匯出與列印（文獻資訊、閱讀畫面）、書目匯出、首頁的多選、多選的「更多動作」、「放進群組」、「移出群組」。 */
  | "exportMenu"
  | "bibExport"
  | "select"
  | "selectMenu"
  | "bulkGroup"
  | "bulkLeave"
  | "discardConfirm"
  /** 翻譯設定：新增服務時選引擎、刪除服務的確認、清除金鑰的確認；檢查譯文：並排比較服務。 */
  | "serviceAdd"
  | "serviceDelete"
  | "keyClear"
  | "compareServices"
  /** 同步包：匯出完成後怎麼交出、合併的結果；從另一台加入：說明與選位置、結果。 */
  | "syncDeliver"
  | "syncResult"
  | "joinIntro"
  | "joinResult"
  /** 外掛在文獻資訊的一節裡開的面板（選單、挑一篇；`useStackLayer`）。 */
  | "ext"
  /** 檢查更新：用行動數據下載前的確認、第一次更新要允許「安裝不明應用程式」的說明。 */
  | "updateMetered"
  | "updatePermission"
  /** 點 ⓘ 打開的說明框（`ui/components.tsx` 的 `InfoTip`）。 */
  | "tip";

export type Route =
  | { name: "home" }
  /** `page`、`material`、`highlight`：從 `paperreader://` 連結來的，直接跳到那一頁、那一份材料、那一條劃線。 */
  | {
      name: "reader";
      paperId: string;
      page?: number;
      material?: string;
      highlight?: string;
      /** 打開之後直接進哪種檢視（產生完成後從進度畫面來；首頁搜尋的「內文」來的也可以是筆記窗格）。 */
      view?: "compare" | "overlay" | "digest" | "notes";
      /** 和 `page` 一起：框出那一頁的這一處（PDF 頁面座標；文獻資訊的「看原文」）。 */
      box?: [number, number, number, number];
      /** 和 `page` 一起：原文那一頁文字（`text.json`）裡的 UTF-16 位置與那一段的原樣文字，框出它（首頁搜尋的「內文」）。 */
      range?: [number, number];
      quote?: string;
      /** 和 `view: "digest"` 一起：捲到重點筆記的這一點。 */
      point?: string;
    }
  | { name: "info"; paperId: string }
  | { name: "history" }
  | { name: "problems" }
  | { name: "settings" }
  /** 設定的翻譯：服務清單、一個服務、提示詞預設組、術語庫。 */
  | { name: "translateServices" }
  | { name: "translateService"; id: string }
  | { name: "translatePrompts" }
  | { name: "glossary" }
  | { name: "trash" }
  /** 設定的「同步」頁；匯入同步包的預覽（寫入之前列出會合併什麼、選衝突）。 */
  | { name: "sync" }
  | { name: "syncPreview" }
  /** 整批匯入的預覽（寫入之前列出會加入、會略過的篇）。 */
  | { name: "importPreview" }
  | { name: "about" }
  /** 檢查更新（只有 Android）：同意、檢查、下載、安裝。 */
  | { name: "update" }
  /** 關於頁裡一項組件（或應用本身，`item` 是 `app`）的授權全文。`item` 是 `群組:序號`。 */
  | { name: "license"; item: string }
  /** 產生譯本與重點筆記（只有前端）。`kind` 是一打開就在哪一種（沒給是譯本）。 */
  | { name: "generate"; paperId: string; kind?: "translation" | "digest" }
  /** 說明頁：劃線、筆刷、工具箱、翻譯、和 Obsidian 一起用。`topic` 打開時捲到那一節。 */
  | { name: "help"; topic?: HelpTopic }
  | { name: "sheet"; kind: SheetKind };

export type HelpTopic = "highlight" | "brush" | "toolbox" | "translate" | "obsidian";

/**
 * 最上面那個面板的種類（不算說明框：說明框疊在面板上時，面板仍開著）。最上面是畫面時是 `null`。
 */
export function topSheet(stack: Route[]): SheetKind | null {
  for (let i = stack.length - 1; i >= 0; i--) {
    const r = stack[i];
    if (r?.name !== "sheet") return null;
    if (r.kind !== "tip") return r.kind;
  }
  return null;
}

/** 讓深處的元件（例如說明框）拿得到導覽，不必一層層傳下去。 */
export const NavContext = createContext<Nav | null>(null);

export interface Nav {
  /** 目前最上面的一層。 */
  top: Route;
  /** 堆疊（最底下是首頁）。 */
  stack: Route[];
  push: (r: Route) => void;
  /** 退一層。退完（歷史紀錄真的退了）才完成，所以接著再推新的一層是安全的。 */
  back: () => Promise<void>;
  /** 一路退回首頁。 */
  home: () => Promise<void>;
  /**
   * 離開前的攔截（例如「還沒儲存的修改」）。有攔截時，返回（含系統返回鍵與返回手勢）退一層之前會先問它：
   * 傳回 `true` 表示它接手了（導覽停在原處，由它決定要不要真的離開）。傳 `null` 取消攔截。
   */
  setGuard: (guard: (() => boolean) | null) => void;
}

interface HistoryState {
  depth: number;
}

export function useNav(): Nav {
  const [stack, setStack] = useState<Route[]>([{ name: "home" }]);
  const stackRef = useRef(stack);
  const waiting = useRef<Array<() => void>>([]);
  const guardRef = useRef<(() => boolean) | null>(null);
  useEffect(() => {
    stackRef.current = stack;
  }, [stack]);

  useEffect(() => {
    // 頁面重新載入時歷史紀錄可能留有舊的深度，從首頁重新開始
    history.replaceState({ depth: 0 } satisfies HistoryState, "");
    const onPop = (e: PopStateEvent) => {
      const depth = (e.state as HistoryState | null)?.depth ?? 0;
      const current = stackRef.current;
      const leaving = depth + 1 < current.length;
      // 離開前的攔截：只在最上面一層是畫面（不是面板）時問它；它接手就把歷史紀錄補回原來的位置
      if (leaving && current[current.length - 1]?.name !== "sheet" && guardRef.current?.()) {
        history.pushState({ depth: current.length - 1 } satisfies HistoryState, "");
        const done = waiting.current;
        waiting.current = [];
        for (const f of done) f();
        return;
      }
      if (leaving) {
        stackRef.current = stackRef.current.slice(0, depth + 1);
        setStack(stackRef.current);
      }
      const done = waiting.current;
      waiting.current = [];
      for (const f of done) f();
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  const push = useCallback((r: Route) => {
    const depth = stackRef.current.length;
    history.pushState({ depth } satisfies HistoryState, "");
    // 馬上更新參照：同一個動作裡連推兩層時，深度才不會算錯
    stackRef.current = [...stackRef.current, r];
    setStack(stackRef.current);
  }, []);

  const go = useCallback((n: number) => {
    return new Promise<void>((resolve) => {
      if (n <= 0) {
        resolve();
        return;
      }
      waiting.current.push(resolve);
      history.go(-n);
    });
  }, []);

  const back = useCallback(() => go(stackRef.current.length > 1 ? 1 : 0), [go]);
  const home = useCallback(() => go(stackRef.current.length - 1), [go]);

  // Android 的返回鍵：不在首頁時由這裡接手，照導覽堆疊退一層。
  // 交給系統預設的話，系統的網頁元件會跳過「使用者還沒碰過畫面就由程式加上去的歷史」——
  // 啟動直接打開上次那篇、從筆記工具點連結開進來時，一按返回就離開應用，不會先回首頁。
  // 回到首頁就放手，讓系統照常離開應用。
  const deep = stack.length > 1;
  useEffect(() => {
    if (!deep) return;
    let listener: PluginListener | null = null;
    let gone = false;
    onBackButtonPress(() => void back())
      .then((l) => {
        if (gone) void l.unregister();
        else listener = l;
      })
      .catch(() => {}); // 不在 Android（電腦版、瀏覽器）：沒有返回鍵可接
    return () => {
      gone = true;
      void listener?.unregister();
    };
  }, [deep, back]);

  const setGuard = useCallback((guard: (() => boolean) | null) => {
    guardRef.current = guard;
  }, []);

  return { top: stack[stack.length - 1] ?? { name: "home" }, stack, push, back, home, setGuard };
}
