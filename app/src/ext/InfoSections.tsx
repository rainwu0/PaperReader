// 文獻資訊裡外掛的幾節：依登記處逐一放、套用應用的節標題樣式，排在書目之後、匯出之前。
// `PaperInfo.tsx` 只放這一個元件。

import { useEffect, useRef, useState, type ReactNode } from "react";
import type { Nav } from "../nav";
import { plugins } from "./index";
import type { Box, InfoSection } from "./types";

function Heading(props: { children: ReactNode }) {
  return <h2 className="section">{props.children}</h2>;
}

const sections: InfoSection[] = plugins.flatMap((p) => p.infoSections ?? []).sort((a, b) => a.order - b.order);

export function ExtInfoSections(props: { paperId: string; nav: Nav }) {
  const { paperId, nav } = props;
  // 文獻資訊又回到最上面（從閱讀畫面、別篇回來）：各節重讀自己的內容
  const onTop = [...nav.stack].reverse().find((r) => r.name !== "sheet")?.name === "info";
  const [refresh, setRefresh] = useState(0);
  const was = useRef(onTop);
  useEffect(() => {
    if (onTop && !was.current) setRefresh((n) => n + 1);
    was.current = onTop;
  }, [onTop]);
  const openPaper = (id: string) => nav.push({ name: "reader", paperId: id });
  const showSource = (page: number, bbox: Box | null) =>
    nav.push({ name: "reader", paperId, page: page + 1, ...(bbox ? { box: bbox } : {}) });
  return (
    <>
      {sections.map((s) => (
        <s.Component key={s.id} paperId={paperId} openPaper={openPaper} showSource={showSource} Heading={Heading} refresh={refresh} />
      ))}
    </>
  );
}
