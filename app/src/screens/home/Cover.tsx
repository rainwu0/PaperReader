// 封面：原文第一頁的縮圖；還沒畫好（或畫不出來）時是篇名首字的色塊。
// 滑進（接近）畫面才去要圖，畫面關掉時取消還沒開始畫的，所以長清單不會一次把每一篇都畫一遍。

import { useEffect, useRef, useState } from "react";
import type { PaperSummary } from "../../api";
import { displayTitle } from "../../format";
import { coverMaterial, firstLetter } from "./paperMeta";
import { requestCover } from "./thumbs";

function useCover(paper: PaperSummary) {
  const ref = useRef<HTMLDivElement | null>(null);
  const [visible, setVisible] = useState(false);
  const [url, setUrl] = useState<string | null>(null);
  const materialId = coverMaterial(paper)?.id ?? null;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (typeof IntersectionObserver === "undefined") {
      setVisible(true);
      return;
    }
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisible(true);
          io.disconnect();
        }
      },
      { rootMargin: "240px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  useEffect(() => {
    if (!visible || materialId === null) return;
    let alive = true;
    let objectUrl: string | null = null;
    const req = requestCover(paper.id, materialId);
    void req.promise
      .then((blob) => {
        if (!alive || !blob) return;
        objectUrl = URL.createObjectURL(blob);
        setUrl(objectUrl);
      })
      .catch(() => {});
    return () => {
      alive = false;
      req.cancel();
      if (objectUrl) URL.revokeObjectURL(objectUrl);
      setUrl(null);
    };
  }, [visible, paper.id, materialId]);

  return { ref, url };
}

/** `fraction`：讀到全篇的幾分之幾，在封面下緣畫一條細的進度（只是看個大概，頁數寫在列上的字裡）。 */
export function Cover(props: { paper: PaperSummary; size: "sm" | "md" | "lg"; fraction?: number | null }) {
  const { ref, url } = useCover(props.paper);
  return (
    <div ref={ref} className={`cover cover-${props.size}`} aria-hidden="true">
      {url ? (
        <img src={url} alt="" draggable={false} />
      ) : (
        <span>{firstLetter(displayTitle(props.paper.bib, props.paper.folder))}</span>
      )}
      {props.fraction != null ? (
        <i className="cover-bar" style={{ width: `${Math.max(8, Math.round(props.fraction * 100))}%` }} />
      ) : null}
    </div>
  );
}
