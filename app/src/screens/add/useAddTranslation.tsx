// 加入譯本（別處做好的 PDF）：開系統選檔畫面選一份，加成這篇的譯本。加入了或原本就在給短暫提示，
// 失敗開面板說原因（`TranslationResultSheet`）。文獻資訊與產生畫面共用；首頁那一份留給首頁那一批改。
// 放在「加入」這一組：和加入文獻同一條路（外殼先確認是 PDF，再交給核心）。

import { useRef, useState } from "react";
import { api, kindOf, type TranslationResult } from "../../api";
import { useAlive } from "../../hooks";
import type { Nav, SheetKind } from "../../nav";
import { t } from "../../strings";
import { BusyOverlay } from "../../ui/components";
import { AnimatePresence } from "../../ui/motion";
import { TranslationResultSheet } from "../home/groupUi";

export function useAddTranslation(opts: {
  paperId: string;
  nav: Nav;
  /** 這個畫面上面開著的面板。 */
  sheet: SheetKind | null;
  toast: (message: string) => void;
  /** 加入了（或原本就在、或失敗）之後：這篇的材料可能變了。 */
  onChanged: () => void | Promise<void>;
}) {
  const alive = useAlive();
  const busyRef = useRef(false);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState<Pick<TranslationResult, "outcome" | "name" | "error"> | null>(null);

  const showFailed = (r: Pick<TranslationResult, "outcome" | "name" | "error">) => {
    setFailed(r);
    opts.nav.push({ name: "sheet", kind: "translationResult" });
  };

  const start = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      const r = await api.addTranslationFromPicker(opts.paperId);
      if (r.cancelled) return;
      if (alive.current) {
        if (r.outcome === "failed") showFailed(r);
        else opts.toast(t(r.outcome === "added" ? "translation.added" : "translation.present"));
      }
      await opts.onChanged();
    } catch (e) {
      if (alive.current) showFailed({ outcome: "failed", name: "", error: kindOf(e) });
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const overlays = (
    <>
      <AnimatePresence>
        {opts.sheet === "translationResult" && failed ? (
          <TranslationResultSheet key="translation" result={failed} onDone={() => void opts.nav.back()} />
        ) : null}
      </AnimatePresence>
      <BusyOverlay active={busy} label={t("translation.busy")} />
    </>
  );

  return { start, busy, overlays };
}
