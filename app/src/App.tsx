import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, kindOf, AppError } from "./api";
import { ALL_VIEW, type View } from "./catalog";
import { useBackgroundCheck } from "./checks";
import { useForeground, useTheme } from "./hooks";
import { useLibrary } from "./library";
import { NavContext, useNav, type Route, type SheetKind } from "./nav";
import { usePrefs } from "./prefs";
import { BIB_FILLED, perPaper, pokePerPaper, startPerPaper } from "./perPaperApp";
import { MATERIALS_CHANGED } from "./translate/autoBuild";
import { startBergamotBridge } from "./translate/bergamot";
import { Home } from "./screens/home/Home";
import { History } from "./screens/history/History";
import { Problems } from "./screens/problems/Problems";
import { useAdd, type NoticeMore } from "./screens/add/useAdd";
import { PaperInfo } from "./screens/info/PaperInfo";
import { Reader } from "./screens/reader/Reader";
import { Generate } from "./screens/generate/Generate";
import { About } from "./screens/settings/About";
import { License } from "./screens/settings/License";
import { GlossaryScreen } from "./screens/settings/translate/Glossary";
import { TranslatePrompts } from "./screens/settings/translate/Prompts";
import { TranslateServiceScreen } from "./screens/settings/translate/ServiceDetail";
import { TranslateServices } from "./screens/settings/translate/Services";
import { Settings } from "./screens/settings/Settings";
import { Sync } from "./screens/settings/Sync";
import { SyncPreviewScreen } from "./screens/settings/SyncPreview";
import { useSync } from "./screens/settings/useSync";
import { useJoin } from "./screens/add/useJoin";
import { useImport } from "./screens/add/useImport";
import { ImportPreviewScreen } from "./screens/add/ImportPreview";
import { Trash } from "./screens/settings/Trash";
import { errorDetail, errorMessage, t } from "./strings";
import { Toast, type ToastData } from "./ui/components";
import { AnimatePresence, m, MotionRoot, useExitingStyle, useMotion } from "./ui/motion";
import { Help } from "./screens/settings/Help";

/** 把外殼回報的螢幕邊界（狀態列、瀏海、手勢列）寫成 CSS 變數，內容才不會被蓋住。 */
function applyInsets() {
  api
    .systemInsets()
    .then((i) => {
      const s = document.documentElement.style;
      s.setProperty("--inset-top", `${i.top}px`);
      s.setProperty("--inset-bottom", `${i.bottom}px`);
      s.setProperty("--inset-left", `${i.left}px`);
      s.setProperty("--inset-right", `${i.right}px`);
    })
    .catch(() => {});
}

export function App() {
  const nav = useNav();
  const lib = useLibrary();
  const [toast, setToast] = useState<ToastData | null>(null);
  const notifyRef = useRef<(title: string, message: string, more?: NoticeMore) => void>(() => {});
  const [theme, setTheme] = useTheme((e: AppError) => {
    // 存不進設定檔：選項已改回原值，說明原因
    notifyRef.current(t("settings.appearance.heading"), t("settings.theme.saveFailed"), {
      detail: errorMessage(e.kind),
    });
  });
  const prefs = usePrefs((e: AppError) => {
    notifyRef.current(t("settings.title"), t("settings.prefs.saveFailed"), { detail: errorMessage(e.kind) });
  });
  /** 短暫的提示：各畫面共用這一個。 */
  const showToast = (message: string) => setToast({ message });
  const add = useAdd(lib, nav, showToast);
  notifyRef.current = add.notify;
  // 首頁目前看的是哪一批文獻（全部文獻、未分組、某個群組）
  const [view, setView] = useState<View>(ALL_VIEW);
  // 打開文獻庫後在背景檢查；有問題才在首頁上方說一行
  const libReady = lib.status?.state === "ready" || lib.status?.state === "read_only";
  const checks = useBackgroundCheck(libReady);
  // 同步包與從另一台加入：匯入、加入完成後重讀目錄並重新檢查（首頁跟著更新）
  const sync = useSync(lib, nav, () => void checks.run(true), add.notify);
  const join = useJoin(lib, nav, add.notify, () => void checks.run(true));
  const imp = useImport(lib, nav, add.notify, add.start);

  // 打開應用時先到哪裡：設定選「上次讀的那一篇」就直接開那一篇（只在啟動時做一次）
  const startupDone = useRef(false);
  useEffect(() => {
    if (startupDone.current || !prefs.loaded || !libReady) return;
    startupDone.current = true;
    if (prefs.prefs.open_to === "last" && lib.continueId) {
      nav.push({ name: "reader", paperId: lib.continueId });
    }
  }, [prefs.loaded, prefs.prefs.open_to, libReady, lib.continueId, nav]);

  // 開啟文獻庫時把上一次沒做完的合併做完了：說一次，說了才告訴核心清掉（問狀況的那一次被丟掉也不會漏說）
  const resumedSync = lib.status?.resumedSync === true;
  useEffect(() => {
    if (!resumedSync) return;
    setToast({ message: t("sync.resumed") });
    void api.syncResumedSeen().catch(() => {});
  }, [resumedSync]);

  // 離線翻譯（Firefox 翻譯）在畫面一側跑：一直等外殼交來的工作
  useEffect(() => startBergamotBridge(), []);

  // 產生好譯本 PDF（材料清單變了）：目錄重新讀
  const refreshLib = lib.refresh;
  useEffect(() => {
    const on = () => void refreshLib();
    window.addEventListener(MATERIALS_CHANGED, on);
    return () => window.removeEventListener(MATERIALS_CHANGED, on);
  }, [refreshLib]);

  // 逐篇處理：文獻庫開著（可寫）就在背景跑，暫停照設定；換了文獻庫重讀它的紀錄；
  // 補了書目時目錄重新讀（同一陣子補了好幾篇只讀一次）
  const libWritable = lib.status?.state === "ready";
  const organizePaused = prefs.prefs.organize_paused;
  useEffect(() => {
    if (!prefs.loaded) return;
    perPaper.setPaused(organizePaused);
    if (libWritable) startPerPaper();
    pokePerPaper();
  }, [prefs.loaded, organizePaused, libWritable]);
  const libPath = lib.status?.path;
  useEffect(() => {
    perPaper.reset();
    pokePerPaper();
  }, [libPath]);
  useEffect(() => {
    let timer = 0;
    const on = () => {
      if (timer) return;
      timer = window.setTimeout(() => {
        timer = 0;
        void refreshLib();
      }, 3000);
    };
    window.addEventListener(BIB_FILLED, on);
    return () => {
      window.removeEventListener(BIB_FILLED, on);
      window.clearTimeout(timer);
    };
  }, [refreshLib]);

  useEffect(() => {
    void lib.refresh();
    applyInsets();
    // 設定檔壞了、已用預設值復原：告知一次
    api
      .startupNotice()
      .then((n) => {
        if (n.settingsRecovered) {
          notifyRef.current(t("startup.settingsRecovered.title"), t("startup.settingsRecovered"), {
            detail: t("startup.settingsRecovered.detail"),
            action: { label: t("common.toSettings"), run: () => nav.push({ name: "settings" }) },
          });
        }
      })
      .catch(() => {});
    window.addEventListener("resize", applyInsets);
    return () => window.removeEventListener("resize", applyInsets);
    // 只在啟動時做一次
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 別的應用（例如筆記工具）開了 paperreader:// 連結：打開那篇、那一頁
  const [pendingLink, setPendingLink] = useState<string | null>(null);
  const takeLink = () => {
    api
      .takeLink()
      .then((l) => l && setPendingLink(l))
      .catch(() => {});
  };
  useEffect(() => {
    if (libReady) takeLink();
  }, [libReady]);
  useEffect(() => {
    if (!pendingLink || !libReady) return;
    setPendingLink(null);
    let u: URL;
    try {
      u = new URL(pendingLink);
    } catch {
      return;
    }
    const paper = u.searchParams.get("paper");
    const page = Number(u.searchParams.get("page"));
    const material = u.searchParams.get("material") ?? undefined;
    const highlight = u.searchParams.get("highlight") ?? undefined;
    if (!paper || !lib.papers.some((p) => p.id === paper)) {
      add.notify(t("reader.link.missing.title"), t("reader.link.missing"), {
        detail: t("reader.link.missing.detail"),
      });
      return;
    }
    const pageNo = Number.isInteger(page) && page > 0 ? page : undefined;
    // 最上面的畫面（不算面板：筆記窗格開著時，最上面一層是窗格）
    const top = [...nav.stack].reverse().find((r) => r.name !== "sheet");
    if (top?.name === "reader" && top.paperId === paper) {
      window.dispatchEvent(new CustomEvent("pr-goto", { detail: { paperId: paper, page: pageNo, highlight } }));
      return;
    }
    nav.push({ name: "reader", paperId: paper, page: pageNo, material, highlight });
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingLink, libReady, lib.papers]);

  // 別的應用分享進來的 PDF：知道文獻庫的狀態後領取一次（冷啟動），之後每次回到前景再問
  const statusKnown = lib.status !== null;
  useEffect(() => {
    if (statusKnown) add.takeShared();
    // 只在第一次知道狀態時做
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [statusKnown]);

  useForeground(() => {
    if (add.resumeAfterPermission() || join.resumeAfterPermission()) return;
    takeLink();
    add.takeShared();
    void lib.foreground().then(() => checks.run());
  });

  // 進到新的一層（開閱讀、開設定…）就收掉提示，不讓它跟進去蓋住內容；退回上一層時留著
  const depth = nav.stack.length;
  const lastDepth = useRef(depth);
  useEffect(() => {
    if (depth > lastDepth.current) setToast(null);
    lastDepth.current = depth;
  }, [depth]);

  /** 移除之後回到首頁：顯示「已移到回收區」與「復原」。 */
  const onRemoved = (paperId: string) => {
    setToast({
      message: t("info.removed"),
      action: {
        label: t("common.undo"),
        run: () => {
          api
            .restorePaper(paperId)
            .then(() => lib.refresh())
            .catch((e: unknown) =>
              add.notify(t("add.restoreFailed.title"), errorMessage(kindOf(e)), {
                detail: errorDetail(kindOf(e)),
              }),
            );
        },
      },
    });
  };

  const render = (r: Route, sheets: SheetKind[]) => {
    // 說明框（tip）疊在面板上時，面板仍是開著的那一個
    const ownerSheet = sheets.filter((k) => k !== "tip").at(-1) ?? null;
    switch (r.name) {
      case "home":
        return (
          <Home
            lib={lib}
            nav={nav}
            sheets={sheets}
            prefs={prefs}
            view={view}
            onView={setView}
            problems={checks.problems}
            onAdd={add.start}
            onAddMenu={imp.openMenu}
            onCreateHere={add.createHere}
            onJoin={join.start}
            onResumeSync={sync.resume}
            highlightId={add.highlightId}
            notify={add.notify}
            toast={showToast}
          />
        );
      case "reader":
        return (
          <Reader
            paperId={r.paperId}
            target={{
              page: r.page,
              material: r.material,
              highlight: r.highlight,
              view: r.view,
              box: r.box,
              range: r.range,
              quote: r.quote,
              point: r.point,
            }}
            nav={nav}
            sheets={sheets}
            prefs={prefs}
            onChanged={() => void lib.refresh()}
          />
        );
      case "history":
        return <History nav={nav} papers={lib.papers} />;
      case "problems":
        return (
          <Problems
            nav={nav}
            problems={checks.problems}
            papers={lib.papers}
            recheck={() => checks.run(true)}
          />
        );
      case "info":
        return (
          <PaperInfo
            paperId={r.paperId}
            nav={nav}
            needsNewerApp={lib.papers.find((p) => p.id === r.paperId)?.needs_newer_app ?? false}
            sheet={ownerSheet}
            groups={lib.groups}
            memberOf={lib.papers.find((p) => p.id === r.paperId)?.groups ?? []}
            onChanged={() => lib.refresh()}
            onRemoved={onRemoved}
            toast={showToast}
          />
        );
      case "settings":
        return (
          <Settings
            nav={nav}
            status={lib.status}
            papers={lib.papers}
            trashCount={lib.trashCount}
            theme={theme}
            onTheme={setTheme}
            prefs={prefs}
            refresh={lib.refresh}
            sheet={ownerSheet}
            libraryEmpty={lib.papers.length === 0 && lib.trashCount === 0 && lib.groups.length === 0}
            notify={add.notify}
          />
        );
      case "sync":
        return <Sync nav={nav} status={lib.status} sync={sync} />;
      case "syncPreview":
        return <SyncPreviewScreen nav={nav} sync={sync} />;
      case "importPreview":
        return <ImportPreviewScreen nav={nav} imp={imp} papers={lib.papers} />;
      case "translateServices":
        return <TranslateServices nav={nav} sheet={ownerSheet} />;
      case "translateService":
        return <TranslateServiceScreen nav={nav} id={r.id} sheet={ownerSheet} />;
      case "translatePrompts":
        return <TranslatePrompts nav={nav} />;
      case "glossary":
        return <GlossaryScreen nav={nav} toast={showToast} />;
      case "trash":
        return (
          <Trash nav={nav} sheet={ownerSheet} papers={lib.papers} onChanged={() => void lib.refresh()} toast={showToast} />
        );
      case "about":
        return <About nav={nav} />;
      case "license":
        return <License nav={nav} item={r.item} />;
      case "generate":
        return (
          <Generate
            nav={nav}
            paperId={r.paperId}
            kind={r.kind}
            sheet={ownerSheet}
            onChanged={() => void lib.refresh()}
            toast={showToast}
          />
        );
      case "help":
        return <Help nav={nav} topic={r.topic} />;
      case "sheet":
        return null; // 面板由擁有它的畫面或 `useAdd` 畫在最上層
    }
  };

  // 所有疊著的畫面都留著（捲動位置、讀到哪都不會丟），只有最上面那一層能操作。
  // 面板那一層（`sheet`）由擁有它的畫面自己畫，這裡不給它一層，免得空的一層擋住觸控。
  return (
    <MotionRoot>
      <NavContext.Provider value={nav}>
        <AnimatePresence>
          {nav.stack.map((r, i) => {
            if (r.name === "sheet") return null;
            const covered = i < nav.stack.length - 1;
            // 這個畫面上面疊的全是面板（側邊欄、搜尋列、排序選單、跳頁…）：面板是這個畫面的一部分
            // （它自己畫面板），所以畫面不鎖住；上面還有別的畫面時才鎖住
            const above = nav.stack.slice(i + 1);
            const sheets = above.every((a) => a.name === "sheet")
              ? above.flatMap((a) => (a.name === "sheet" ? [a.kind] : []))
              : [];
            const locked = covered && !(above.length > 0 && above.every((a) => a.name === "sheet"));
            return (
              <ScreenLayer key={`${i}-${r.name}-${"paperId" in r ? r.paperId : ""}`} locked={locked} first={i === 0}>
                {render(r, sheets)}
              </ScreenLayer>
            );
          })}
        </AnimatePresence>
        {sync.overlays}
        {join.overlays}
        {imp.overlays}
        {add.overlays}
        <Toast toast={toast} onDone={() => setToast(null)} />
      </NavContext.Provider>
    </MotionRoot>
  );
}

/**
 * 一個畫面的一層。前進時從右側位移 30px 淡入，返回時反向淡出（共軸 X）；
 * 最底下的首頁不播進場（只在這一層給 `initial={false}`；給在 `AnimatePresence` 上會連帶讓首頁裡之後才出現的元素也不播）。
 * 這一層自己是滿版的固定定位：位移中畫面裡的固定定位元素仍以整個畫面為準。
 */
function ScreenLayer(props: { locked: boolean; first: boolean; children: ReactNode }) {
  const motion = useMotion("screen");
  const exiting = useExitingStyle();
  return (
    <m.div
      className="layer"
      style={exiting}
      // 底下被蓋住的畫面不讓讀屏軟體與鍵盤碰到
      inert={props.locked}
      aria-hidden={props.locked}
      {...motion}
      initial={props.first ? false : motion.initial}
    >
      {props.children}
    </m.div>
  );
}
