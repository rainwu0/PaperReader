// 首頁用的線條圖示（自己畫的）。裝飾用，所以 `aria-hidden`；按鈕本身有說明文字。

const common = { viewBox: "0 0 24 24", "aria-hidden": true, focusable: false } as const;

/** 「⋯」：更多選項。 */
export const MoreIcon = () => (
  <svg {...common}>
    <path d="M5 12h.01M12 12h.01M19 12h.01" strokeWidth="3.2" />
  </svg>
);

/** 空的文獻庫：幾份疊著的紙。 */
export const StackIcon = () => (
  <svg {...common}>
    <rect x="6" y="3.5" width="12" height="15" rx="2" />
    <path d="M4 7.5v11a2 2 0 0 0 2 2h9" />
    <path d="M9.5 8.5h5M9.5 12h5" />
  </svg>
);
