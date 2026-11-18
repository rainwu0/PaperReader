// 工具箱用的線條圖示（自己畫的）。裝飾用，所以 `aria-hidden`；按鈕本身有說明文字。

const common = { viewBox: "0 0 24 24", "aria-hidden": true, focusable: false } as const;

/** 工具箱縮小時那顆圓鈕：一把螺絲起子與一支筆交叉的意思，畫成扳手。 */
export const ToolboxIcon = () => (
  <svg {...common}>
    <path d="M14.5 6.5a4 4 0 0 0 4.9 4.9l-9 9a2.1 2.1 0 0 1-3-3l9-9a4 4 0 0 0-1.9-1.9z" />
  </svg>
);

/** 原文：一張有字的紙。 */
export const SourceIcon = () => (
  <svg {...common}>
    <path d="M7 3h7l4 4v14H7z" />
    <path d="M14 3v4h4M10 12h5M10 16h5" />
  </svg>
);

/** 譯本：兩個方向的箭頭，表示轉成另一種文字。 */
export const TranslationIcon = () => (
  <svg {...common}>
    <path d="M4 8h13M14 5l3 3-3 3" />
    <path d="M20 16H7M10 13l-3 3 3 3" />
  </svg>
);

/** 譯文：排好的幾行字，第一行是標題。 */
export const TextIcon = () => (
  <svg {...common}>
    <path d="M5 5h10" />
    <path d="M5 10h14M5 14h14M5 18h9" />
  </svg>
);

/** 看版面：一頁上有一塊圖與幾行字。 */
export const LayoutIcon = () => (
  <svg {...common}>
    <path d="M5 3h14v18H5z" />
    <path d="M8 7h8v5H8zM8 15h8M8 18h5" />
  </svg>
);

/** 更多：三個點。 */
export const MoreIcon = () => (
  <svg {...common}>
    <path d="M12 5h.01M12 12h.01M12 19h.01" />
  </svg>
);

/** 點內容：一根手指點下去。 */
export const TapIcon = () => (
  <svg {...common}>
    <path d="M9 11V5.5a1.5 1.5 0 0 1 3 0V10l4.5 1a2 2 0 0 1 1.5 2.2L17 20h-7l-3.5-5a1.5 1.5 0 0 1 2.4-1.8L9 13" />
    <path d="M5.5 6.5a5 5 0 0 1 2-3.4M15.5 6.5a5 5 0 0 0-2-3.4" />
  </svg>
);

/** 釘住：一根圖釘（工具箱選了檢視也不收起）。 */
export const PinIcon = () => (
  <svg {...common}>
    <path d="M9 4h6l-1 5 3 3H7l3-3z" />
    <path d="M12 12v8" />
  </svg>
);

export const NotesIcon = () => (
  <svg {...common}>
    <path d="M5 4h14v16H5z" />
    <path d="M9 9h6M9 13h6M9 17h3" />
  </svg>
);

/** 劃線：一支螢光筆。 */
export const HighlightIcon = () => (
  <svg {...common}>
    <path d="M15 4l5 5-8 8H7v-5z" />
    <path d="M4 21h9" />
  </svg>
);

/** 筆刷：一支畫筆與一道筆跡。 */
export const BrushIcon = () => (
  <svg {...common}>
    <path d="M19 4c-4 1-7 4-8 8l1.5 1.5c4-1 7-4 8-8z" />
    <path d="M10.5 13.5C8 13 6 14.5 6 17c0 1.5-1 2.5-2 3 3 .5 6 0 7.5-2.5" />
  </svg>
);

/** 短評：一個對話框。 */
export const CommentIcon = () => (
  <svg {...common}>
    <path d="M4 5h16v11H11l-4 4v-4H4z" />
    <path d="M8 9h8M8 12h5" />
  </svg>
);

/** 跳頁：一個箭頭指向一條線。 */
export const JumpIcon = () => (
  <svg {...common}>
    <path d="M12 4v11M7 11l5 5 5-5" />
    <path d="M5 20h14" />
  </svg>
);

/** 產生：一顆星，表示做出新的東西。 */
export const GenerateIcon = () => (
  <svg {...common}>
    <path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z" />
    <path d="M18 17v4M16 19h4" />
  </svg>
);

/** 頁面深色：一彎月亮。 */
export const MoonIcon = () => (
  <svg {...common}>
    <path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z" />
  </svg>
);

/** 拖動的把手：兩排點。 */
export const GripIcon = () => (
  <svg {...common}>
    <path d="M9 6h.01M15 6h.01M9 12h.01M15 12h.01M9 18h.01M15 18h.01" />
  </svg>
);

/** 縮小：往邊緣收起。 */
export const MinimizeIcon = () => (
  <svg {...common}>
    <path d="M6 12h12" />
  </svg>
);

export const UndoIcon = () => (
  <svg {...common}>
    <path d="M9 14L4 9l5-5" />
    <path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" />
  </svg>
);

/** 橡皮擦：一塊斜放的橡皮。 */
export const EraserIcon = () => (
  <svg {...common}>
    <path d="M8 20h11M5.5 14.5l8-8a2 2 0 0 1 2.8 0l2.2 2.2a2 2 0 0 1 0 2.8L12 18H8l-2.5-2.5a1 1 0 0 1 0-1z" />
    <path d="M10 10l5 5" />
  </svg>
);

export const TrashIcon = () => (
  <svg {...common}>
    <path d="M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13" />
  </svg>
);

export const ChevronUpIcon = () => (
  <svg {...common}>
    <path d="M6 15l6-6 6 6" />
  </svg>
);

export const ChevronDownIcon = () => (
  <svg {...common}>
    <path d="M6 9l6 6 6-6" />
  </svg>
);
