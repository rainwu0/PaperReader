// 外掛在畫面一側的接入點的型別：文獻資訊的一節、頁面上的可點區域、
// 逐篇處理。外掛只經由 `src/ext/index.ts` 登記；應用不引用任何一個外掛。

import type { ComponentType, ReactNode } from "react";
import type { LayoutSegment, RawItem } from "../translate/layout";

/** PDF 頁面座標的方框 `[x0, y0, x1, y1]`，原點在左下。 */
export type Box = [number, number, number, number];

/** 一個外掛在畫面一側登記的東西（`src/ext/index.ts` 的 `plugins`）。 */
export interface ExtPlugin {
  /** 和 `app/ext/<名稱>/`、`src/ext/<名稱>/` 相同。 */
  name: string;
  /** 會連到哪裡、送出什麼。 */
  network: { host: string; sends: string }[];
  infoSections?: InfoSection[];
  pageRegions?: PageRegionProvider;
  perPaper?: PerPaperTask[];
  /** 首頁搜尋的一節：放在書目的結果之後。 */
  searchSections?: SearchSection[];
}

/** 打開某篇的某一處。沒給的就照閱讀畫面平常的做法。 */
export interface HitTarget {
  paperId: string;
  /** 原文的材料；譯文是哪一份疊加檔；重點筆記是哪一份；劃線是劃在哪一份。 */
  material?: string;
  /** 頁序，從 0 起。 */
  page?: number;
  /** 原文那一頁文字（`text.json` 的一頁）裡的 UTF-16 位置 `[起, 訖)`，和 `quote` 一起給。 */
  range?: [number, number];
  /** `range` 那一段的原樣文字：閱讀畫面對不上 `range` 時改在那一頁找它。 */
  quote?: string;
  /** 和 `page` 一起：框出那一頁的這一處（PDF 頁面座標）。 */
  box?: Box;
  /** 打開之後進哪種檢視：原頁疊譯文、重點筆記、筆記窗格。 */
  view?: "overlay" | "digest" | "notes";
  /** 跳到這條劃線。 */
  highlight?: string;
  /** 重點筆記的這一點（和 `view: "digest"` 一起）。 */
  point?: string;
}

/** 首頁搜尋的一節交給外掛的東西。 */
export interface SearchSectionProps {
  /** 搜尋字串，應用停頓 300 ms 後才換。 */
  query: string;
  /** 目前的群組與篩選下看得到的文獻代號（不套搜尋字串本身）；沒有群組也沒有篩選是 `null`。 */
  scope: string[] | null;
  /** 打開那一處（閱讀畫面，返回鍵回到搜尋）。 */
  openHit: (target: HitTarget) => void;
  /** 節標題：應用的樣式。 */
  Heading: ComponentType<{ children: ReactNode }>;
  /** 一篇文獻：首頁現有的文獻列（封面、篇名、作者年份；點了打開），`children` 放在列的下面。 */
  Paper: ComponentType<{ paperId: string; children?: ReactNode }>;
  /** 逐篇處理還沒抽完文字的篇數。 */
  unextracted: number;
}

/** 首頁搜尋的一節。 */
export interface SearchSection {
  /** 例如「fulltext.body」。 */
  id: string;
  /** 小的在前。 */
  order: number;
  Component: ComponentType<SearchSectionProps>;
}

/** 文獻資訊的一節交給外掛的東西。 */
export interface InfoSectionProps {
  paperId: string;
  /** 打開某篇（閱讀畫面，返回鍵回到這裡）。 */
  openPaper: (paperId: string) => void;
  /** 跳到這篇原文的某一頁，框出某一處（`bbox` 沒有時只到那一頁）。`page` 從 0 起。 */
  showSource: (page: number, bbox: Box | null) => void;
  /** 節標題：應用的樣式（`h2.section`）。 */
  Heading: ComponentType<{ children: ReactNode }>;
  /** 文獻資訊重讀了（例如從閱讀畫面回來）：數字變了就重讀自己的內容。 */
  refresh: number;
}

/** 文獻資訊的一節。 */
export interface InfoSection {
  /** 例如「links.references」。 */
  id: string;
  /** 小的在前。 */
  order: number;
  /** 沒有內容時交回 `null`，應用就不顯示這一節。 */
  Component: ComponentType<InfoSectionProps>;
}

/** 頁面上的可點區域。 */
export interface PageRegionProvider {
  /** 閱讀畫面畫好原文的一頁之後呼叫；沒有就交回 []。換頁、離開時 `signal` 中止。 */
  regionsFor(input: PageRegionInput, signal: AbortSignal): Promise<PageRegion[]>;
  /** 第一次出卡片時的首次提示，外掛自己的文字。 */
  hint?: string;
}

export interface PageRegionInput {
  paperId: string;
  /** 原文的材料代號。 */
  materialId: string;
  /** 頁序，從 0 起。 */
  page: number;
  /** 頁面可見範圍。 */
  view: Box;
  /** 那一頁的文字片段（`toRawItems`）。 */
  items: RawItem[];
}

export interface PageRegion {
  bbox: Box;
  card: RegionCard;
}

/** 卡片是資料；閱讀畫面用自己的小框顯示。 */
export interface RegionCard {
  /** 標記本身，例如「[3, 5–7]」。 */
  title: string;
  /** `open`：可以打開的文獻代號；`note`：條目下面的一行小字（例如「可能是：…」）。 */
  items: { text: string; note?: string; open?: string }[];
}

/** 逐篇處理的一件抽取。結果由它自己經核心的指令寫入，核心檢查過才寫。 */
export interface PerPaperTask {
  /** 代號，例如「links.references」；也是應用空間紀錄裡的鍵。 */
  id: string;
  /** 抽取規則的版本。規則改了加一，做過的篇會重做。 */
  version: number;
  /** 要不要版面分析（`layoutDocument`）；同一篇的抽取共用一次。 */
  needsLayout: boolean;
  /** 要不要每頁的文字（`PerPaperInput.pages`）；沒寫是 `false`。和版面分析共用一次讀頁。 */
  needsPages?: boolean;
  /**
   * 一批裡哪些做過了（交回做過的文獻代號）；沒給就看應用空間的紀錄。
   * 交回 `null` 表示現在判斷不了（例如索引還在整理中）：這件抽取這一輪先不做。
   */
  doneFor?(papers: { paperId: string; sourceSha256: string }[]): Promise<Set<string> | null>;
  /** 對一篇做；自己經核心的指令寫入。丟出的錯誤由排程分成暫時與永久。`signal` 中止時盡快停下。 */
  run(input: PerPaperInput, signal: AbortSignal): Promise<void>;
}

/** 交給一件抽取的一篇。 */
export interface PerPaperInput {
  paperId: string;
  source: { materialId: string; sha256: string; pages: number };
  /** PDF 自帶資訊（`getMetadata().info`）。 */
  info: Record<string, unknown>;
  /** XMP 原文。 */
  xmp: string | null;
  /** 第一頁的文字（片段標了行尾的接換行）。 */
  firstPageText: string;
  /** `needsLayout` 時才有；同一篇的抽取共用一份。 */
  layout: { segments: LayoutSegment[]; references: LayoutSegment[] } | null;
  /** 有一件 `needsPages` 時才有：每頁一項，是那一頁的 `pageText(items)`（片段標了行尾的接換行），還沒換掉單獨的代理碼。 */
  pages: string[] | null;
}
