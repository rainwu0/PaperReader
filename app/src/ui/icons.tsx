// 線條圖示。裝飾用，所以 `aria-hidden`；按鈕本身有說明文字。

const common = { viewBox: "0 0 24 24", "aria-hidden": true, focusable: false } as const;

export const BackIcon = () => (
  <svg {...common}>
    <path d="M15 5l-7 7 7 7" />
  </svg>
);

export const PlusIcon = () => (
  <svg {...common}>
    <path d="M12 5v14M5 12h14" />
  </svg>
);

export const MenuIcon = () => (
  <svg {...common}>
    <path d="M4 7h16M4 12h16M4 17h16" />
  </svg>
);

export const SearchIcon = () => (
  <svg {...common}>
    <circle cx="11" cy="11" r="6.5" />
    <path d="M16 16l4.5 4.5" />
  </svg>
);

export const CloseIcon = () => (
  <svg {...common}>
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
);

/** 大綱：一條一條的目錄。 */
export const OutlineIcon = () => (
  <svg {...common}>
    <path d="M9 6h11M9 12h11M9 18h11" />
    <path d="M4 6h.01M4 12h.01M4 18h.01" />
  </svg>
);

export const InfoIcon = () => (
  <svg {...common}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 11v5M12 8v.01" />
  </svg>
);

/** 分享、匯出：一個節點連到另外兩個。 */
export const ShareIcon = () => (
  <svg {...common}>
    <circle cx="6" cy="12" r="2.6" />
    <circle cx="17.5" cy="6" r="2.6" />
    <circle cx="17.5" cy="18" r="2.6" />
    <path d="M8.3 10.8l6.9-3.6M8.3 13.2l6.9 3.6" />
  </svg>
);

/**
 * 閱讀狀態的圓點：三種狀態形狀不同，不靠顏色就分得出。
 * 未讀＝實心；讀到一半＝圓圈裡左半邊實心；已讀＝圓圈裡一個勾。
 * 只是裝飾（狀態另外用文字寫在篇名下面），所以 `aria-hidden`。
 */
export const StatusDot = (props: { status: "Unread" | "Reading" | "Read" }) => (
  <svg
    className={`status-dot ${props.status.toLowerCase()}`}
    data-status={props.status}
    viewBox="0 0 24 24"
    aria-hidden="true"
    focusable="false"
  >
    {props.status === "Unread" ? <circle cx="12" cy="12" r="7" className="fill" /> : null}
    {props.status === "Reading" ? (
      <>
        <circle cx="12" cy="12" r="7" className="ring" />
        <path d="M12 5a7 7 0 0 0 0 14z" className="fill" />
      </>
    ) : null}
    {props.status === "Read" ? (
      <>
        <circle cx="12" cy="12" r="8" className="ring" />
        <path d="M8.5 12.5l2.5 2.5 4.5-5" className="tick" />
      </>
    ) : null}
  </svg>
);
