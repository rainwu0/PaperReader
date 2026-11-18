// 文獻資訊的一節「庫裡引用這篇的（M 篇）」：沒有就不顯示；每篇一列，點了打開，列尾寫用哪幾條引的。

import "./links.css";
import { useEffect, useState } from "react";
import type { InfoSectionProps } from "../types";
import { linksApi, type CitingPaper } from "./api";
import { s } from "./strings";

export function CitedBySection(props: InfoSectionProps) {
  const { paperId, refresh, Heading } = props;
  const [list, setList] = useState<CitingPaper[] | null>(null);
  // `refresh` 變了（文獻資訊又回到最上面）就重讀
  useEffect(() => {
    let alive = true;
    linksApi
      .citedBy(paperId)
      .then((l) => alive && setList(l))
      .catch(() => alive && setList([]));
    return () => {
      alive = false;
    };
    // oxlint-disable-next-line react/exhaustive-effect-dependencies
  }, [paperId, refresh]);
  if (!list || list.length === 0) return null;
  return (
    <>
      <Heading>{s("citedBy.heading", { m: list.length })}</Heading>
      <ul className="list lk-cited">
        {list.map((c) => {
          const nums = c.numbers.filter((n): n is number => n !== null);
          return (
            <li key={c.paper} className="row">
              <button type="button" className="row-main" onClick={() => props.openPaper(c.paper)}>
                <span className="row-text">
                  <span className="row-title">{c.title.trim() || s("refs.untitled")}</span>
                  <span className="row-sub">
                    {[c.year ?? "", nums.length > 0 ? s("citedBy.numbers", { numbers: nums.join("、") }) : ""]
                      .filter((x) => x !== "")
                      .join("・") || " "}
                  </span>
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </>
  );
}
