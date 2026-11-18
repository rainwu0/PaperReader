// 設定頁的線條圖示（自己畫的，裝飾用，`aria-hidden`）。
const common = {
  viewBox: "0 0 24 24",
  "aria-hidden": true,
  focusable: false,
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round",
  strokeLinejoin: "round",
} as const;

export const ThemeIcon = () => (
  <svg {...common}>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 3.5v17a8.5 8.5 0 0 0 0-17z" fill="currentColor" stroke="none" />
  </svg>
);

export const PagesIcon = () => (
  <svg {...common}>
    <path d="M5 4.5h9l5 5v10a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1v-14a1 1 0 0 1 1-1z" />
    <path d="M14 4.5v5h5" />
  </svg>
);

export const SunIcon = () => (
  <svg {...common}>
    <circle cx="12" cy="12" r="3.5" />
    <path d="M12 3.5v2M12 18.5v2M3.5 12h2M18.5 12h2M6 6l1.4 1.4M16.6 16.6L18 18M6 18l1.4-1.4M16.6 7.4L18 6" />
  </svg>
);

/** 釘住工具箱：和工具箱上的圖釘同一個圖形（那一個的線條樣式由閱讀畫面的 CSS 給，這裡用設定頁的線條） */
export const PinIcon = () => (
  <svg {...common}>
    <path d="M9 4h6l-1 5 3 3H7l3-3z" />
    <path d="M12 12v8" />
  </svg>
);

export const OpenIcon = () => (
  <svg {...common}>
    <path d="M5 5.5a1.5 1.5 0 0 1 1.5-1.5H12v16H6.5A1.5 1.5 0 0 1 5 18.5z" />
    <path d="M12 4h5.5A1.5 1.5 0 0 1 19 5.5v13a1.5 1.5 0 0 1-1.5 1.5H12" />
  </svg>
);

export const NoteIcon = () => (
  <svg {...common}>
    <path d="M5 20l1-4L17 5a2 2 0 0 1 3 3L9 19z" />
    <path d="M15 7l2 2" />
  </svg>
);

export const FolderIcon = () => (
  <svg {...common}>
    <path d="M4 7a1.5 1.5 0 0 1 1.5-1.5H10l2 2.5h6.5A1.5 1.5 0 0 1 20 9.5v8a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 17.5z" />
  </svg>
);

export const ExportIcon = () => (
  <svg {...common}>
    <path d="M12 4v11M7.5 10.5L12 15l4.5-4.5M5 19.5h14" />
  </svg>
);

export const RestoreIcon = () => (
  <svg {...common}>
    <path d="M12 15V4M7.5 8.5L12 4l4.5 4.5M5 19.5h14" />
  </svg>
);

/** 同步：上下兩支方向相反的箭頭。 */
export const SyncIcon = () => (
  <svg {...common}>
    <path d="M5 9h12.5M14 5.5L17.5 9 14 12.5M19 15H6.5M10 11.5L6.5 15l3.5 3.5" />
  </svg>
);

export const TrashIcon = () => (
  <svg {...common}>
    <path d="M5 7h14M10 7V5h4v2M7 7l1 12.5h8L17 7M10.5 11v5M13.5 11v5" />
  </svg>
);

export const AboutIcon = () => (
  <svg {...common}>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 11v5.5M12 7.8v.2" />
  </svg>
);

/** 說明：圓圈裡一個問號。 */
export const HelpIcon = () => (
  <svg {...common}>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M9.6 9.4a2.5 2.5 0 0 1 4.8.9c0 1.7-2.4 2.1-2.4 3.6M12 16.8v.2" />
  </svg>
);
