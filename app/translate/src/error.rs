//! 翻譯的錯誤。畫面依 `kind` 取白話說明，不顯示 `detail`（可能含服務回的長文字）。

use serde::Serialize;

/// 錯誤種類。名稱會原樣交給畫面，改名要同步改介面的文字表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorKind {
    /// 金鑰不對、沒有權限、帳號沒有餘額。重試沒有用。
    Auth,
    /// 設定不完整（沒有金鑰、沒有模型名稱、網址不合法、不支援的語言）。
    Config,
    /// 太頻繁或額度用完（HTTP 429）。等一下可以重試。
    RateLimited,
    /// 連不上服務（沒有網路、網址不對、逾時）。
    Network,
    /// 服務端出錯（5xx）。
    Server,
    /// 回覆讀不懂（格式不對、段數不符）。
    BadResponse,
    /// 使用者取消。
    Cancelled,
    /// 這個引擎不支援要譯成的語言（或語言組合）。
    UnsupportedLanguage,
    /// 離線引擎的語言模型還沒下載。
    ModelMissing,
    /// 額度用完（例如 DeepL 回 456）。和金鑰不對分開：要等額度重置或加購，不是改金鑰。
    QuotaExceeded,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{kind:?}: {detail}")]
pub struct TranslateError {
    pub kind: ErrorKind,
    /// 給記錄用的短說明（已去掉金鑰與網址，最長 300 字）。
    pub detail: String,
    /// 服務要求的等待秒數（`Retry-After`），沒有就是 `None`。
    pub retry_after_secs: Option<u64>,
}

impl TranslateError {
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> TranslateError {
        let mut d: String = detail.into();
        if d.chars().count() > 300 {
            d = d.chars().take(300).collect();
        }
        TranslateError {
            kind,
            detail: d,
            retry_after_secs: None,
        }
    }

    /// 重試有沒有可能成功。
    pub fn retryable(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::RateLimited | ErrorKind::Network | ErrorKind::Server
        )
    }

    /// 這種錯誤代表整個工作都不會成功，應該停下來而不是一段一段地失敗。
    pub fn is_fatal_for_job(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Auth
                | ErrorKind::Config
                | ErrorKind::UnsupportedLanguage
                | ErrorKind::ModelMissing
                | ErrorKind::QuotaExceeded
        )
    }

    pub(crate) fn from_status(status: u16, body: &str, retry_after_secs: Option<u64>) -> Self {
        let kind = match status {
            401..=403 => ErrorKind::Auth,
            404 => ErrorKind::Config,
            400 | 422 => ErrorKind::BadResponse,
            429 => ErrorKind::RateLimited,
            s if s >= 500 => ErrorKind::Server,
            _ => ErrorKind::BadResponse,
        };
        let mut e = TranslateError::new(kind, format!("HTTP {status}: {}", one_line(body)));
        e.retry_after_secs = retry_after_secs;
        e
    }

    pub(crate) fn network(e: &reqwest::Error) -> Self {
        let detail = if e.is_timeout() {
            "timeout".to_string()
        } else if e.is_connect() {
            "connect failed".to_string()
        } else {
            // 不用 `Display`：它會帶出網址
            "request failed".to_string()
        };
        TranslateError::new(ErrorKind::Network, detail)
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub type Result<T> = std::result::Result<T, TranslateError>;
