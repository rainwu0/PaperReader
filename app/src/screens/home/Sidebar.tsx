// 側邊欄：從左邊滑出、蓋在首頁上，切換首頁要看哪些文獻，並通往閱讀紀錄、回收區、設定。
// 它是導覽堆疊裡的一層（返回鍵、點遮罩都先關掉它）；不是第三個主要畫面。
//
// 項目分兩種：
//   - 檢視項目：切換首頁清單的內容（全部文獻、未分組、各群組），旁邊顯示篇數；
//     群組那幾列右邊有「⋯」，開改名、刪除的選單；最後一列是「新增群組」
//   - 頁面項目：進另一個次頁（閱讀紀錄、回收區、設定）

import type { ReactNode } from "react";
import { ALL_VIEW, inView, sameView, UNGROUPED_VIEW, viewKey, type GroupIds, type View } from "../../catalog";
import type { Group } from "../../api";
import type { Library } from "../../library";
import type { Nav, Route } from "../../nav";
import { t } from "../../strings";
import { IconButton } from "../../ui/components";
import { m, useExitingStyle, useMotion } from "../../ui/motion";
import { CloseIcon, PlusIcon } from "../../ui/icons";
import { MoreIcon } from "./homeIcons";

interface ViewItem {
  view: View;
  label: string;
  count: number;
  /** 群組那幾列才有：開選單用。 */
  group?: Group;
}

export function Sidebar(props: {
  lib: Library;
  nav: Nav;
  view: View;
  onView: (v: View) => void;
  groupIds: GroupIds;
  onGroupMenu: (g: Group) => void;
  onNewGroup: () => void;
}) {
  const { lib, nav, groupIds } = props;

  const viewItems: ViewItem[] = [
    {
      view: ALL_VIEW,
      label: t("sidebar.all"),
      count: lib.papers.length,
    },
    {
      view: UNGROUPED_VIEW,
      label: t("sidebar.ungrouped"),
      count: lib.papers.filter((p) => inView(p, UNGROUPED_VIEW, groupIds)).length,
    },
    ...lib.groups.map((g): ViewItem => {
      const view: View = { kind: "group", id: g.id };
      return {
        view,
        label: g.name,
        count: lib.papers.filter((p) => inView(p, view, groupIds)).length,
        group: g,
      };
    }),
  ];
  const pageItems: Array<{ label: string; route: Route; extra?: ReactNode }> = [
    { label: t("history.title"), route: { name: "history" } },
    {
      label: t("trash.title"),
      route: { name: "trash" },
      extra: lib.trashCount > 0 ? <span className="count">{lib.trashCount}</span> : null,
    },
    { label: t("settings.title"), route: { name: "settings" } },
  ];

  // 抽屜：平移加遮罩淡入淡出
  const scrim = useMotion("scrim");
  const slide = useMotion("drawerLeft");
  const exiting = useExitingStyle();
  const close = () => void nav.back();
  const chooseView = (v: View) => {
    props.onView(v);
    close();
  };
  // 先關掉側邊欄（退回首頁那一層），再推新的一層，返回鍵才會回到首頁
  const goTo = async (route: Route) => {
    await nav.back();
    nav.push(route);
  };

  return (
    <m.div
      className="drawer-scrim"
      style={exiting}
      {...scrim}
      onClick={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <m.nav className="drawer" aria-label={t("home.menu")} {...slide}>
        <div className="drawer-head">
          <h2>{t("sidebar.title")}</h2>
          <IconButton label={t("sidebar.close")} onClick={close}>
            <CloseIcon />
          </IconButton>
        </div>
        <div className="drawer-scroll">
          <ul className="drawer-list">
            {viewItems.map((item) => (
              <li key={viewKey(item.view)} className="drawer-row">
                <button
                  type="button"
                  className="drawer-item"
                  aria-current={sameView(item.view, props.view) ? "page" : undefined}
                  onClick={() => chooseView(item.view)}
                >
                  <span className="drawer-name">{item.label}</span>
                  <span className="count">{item.count}</span>
                </button>
                {item.group ? (
                  <IconButton
                    label={t("sidebar.groupMenu", { name: item.label })}
                    onClick={() => props.onGroupMenu(item.group as Group)}
                  >
                    <MoreIcon />
                  </IconButton>
                ) : null}
              </li>
            ))}
            <li>
              <button type="button" className="drawer-item drawer-add" onClick={props.onNewGroup}>
                <PlusIcon />
                <span className="drawer-name">{t("sidebar.newGroup")}</span>
              </button>
            </li>
          </ul>
          <hr className="drawer-rule" />
          <ul className="drawer-list">
            {pageItems.map((item) => (
              <li key={item.route.name}>
                <button type="button" className="drawer-item" onClick={() => void goTo(item.route)}>
                  <span>{item.label}</span>
                  {item.extra}
                </button>
              </li>
            ))}
          </ul>
        </div>
      </m.nav>
    </m.div>
  );
}
