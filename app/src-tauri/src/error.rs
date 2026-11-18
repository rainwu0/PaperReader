//! 指令交回畫面的錯誤：「種類」加「細節」。
//!
//! 畫面依種類從文字表取出白話的訊息；細節只給除錯用，不含篇名、路徑這類個人資料
//! 所以日誌只記種類。

use paperreader_core::error::LibError;
use serde::Serialize;

/// 錯誤的種類。前七項是核心的 `LibError`（一對一），其餘是應用這一層才有的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorKind {
    PermissionDenied,
    NoSpace,
    NotFound,
    Conflict,
    FormatTooNew,
    ReadOnly,
    Unreadable,
    InvalidInput,
    Io,
    /// 文獻庫還沒建立或還打不開。
    NoLibrary,
    /// 選到的檔案不是 PDF。
    NotPdf,
    /// 這個平台做不到（不是 Android）。
    Unsupported,
    /// Android 外掛回報失敗。
    Platform,
    /// 翻譯：金鑰不對、沒有權限或餘額。
    TranslateAuth,
    /// 翻譯：設定不完整（沒有金鑰、模型名稱、網址不合法）。
    TranslateConfig,
    /// 翻譯：太頻繁或額度用完。
    TranslateRateLimited,
    /// 翻譯：連不上服務。
    TranslateNetwork,
    /// 翻譯：服務端出錯。
    TranslateServer,
    /// 翻譯：服務回的內容讀不懂。
    TranslateBadResponse,
    /// 已經有一份翻譯工作在進行。
    TranslateBusy,
    /// 翻譯：額度用完（和金鑰不對分開）。
    TranslateQuota,
    /// 離線翻譯：語言模型還沒下載。
    TranslateNoModel,
    /// 離線翻譯：這個引擎沒有這個語言組合的模型。
    TranslateLanguage,
    /// 「只在 Wi-Fi 下載」打開，但現在不是 Wi-Fi。
    NotOnWifi,
    /// 找不到系統內建的中日韓字型（產生譯本 PDF 要用）。
    NoFont,
    /// 同步包屬於另一個文獻庫（核心 `library id mismatch`）。
    SyncOtherLibrary,
    /// 這台裝置自己匯出的同步包（核心 `package from this device`）。
    SyncOwnPackage,
    /// 選到的是備份檔，不是同步包（核心 `backup file, not a sync package`）。
    SyncBackupFile,
    /// 加入要用完整的同步包（核心 `package is not complete`）。
    SyncIncomplete,
    /// 從備份檔還原時選到的不是備份檔，也不是同步包。
    NotBackupFile,
    /// 從備份檔還原時選到的是同步包（要到「同步」匯入）。
    RestoreGotSyncPackage,
    /// 整批匯入的 zip 項目名稱用了反斜線（Windows PowerShell 5.1 做的）。
    ImportZipBackslash,
    /// 搬到共用空間：那裡（`Documents/PaperReader`）已經有文獻庫或別的東西。
    SharedNotEmpty,
    /// 搬到共用空間：搬過去的與原本的核對不符，或有檔案沒能放進暫存的備份檔。
    MoveMismatch,
    /// 檢查更新：連不上 GitHub。
    UpdateNetwork,
    /// 檢查更新：GitHub 暫時拒絕（不帶金鑰的請求每小時有上限）。
    UpdateRateLimited,
    /// 檢查更新：GitHub 交回的內容讀不懂，或那一版缺這台裝置要的安裝檔、核對碼清單。
    UpdateBadRelease,
    /// 下載的安裝檔和公布的 SHA-256 不符：已刪掉，沒有安裝。
    UpdateChecksum,
    /// 下載的安裝檔不是同一個應用、版本不比現在新，或簽章憑證和已安裝的不同：已刪掉，沒有安裝。
    UpdateSignature,
    /// 這台手機讀不出下載的安裝檔或它的簽章憑證，無從核對：已刪掉，沒有安裝。
    /// 和 [`ErrorKind::UpdateSignature`] 分開：讀不到不代表是假的。
    UpdateUnverifiable,
    /// 那一版已從 GitHub 撤下（下載時 404）：查到的版本與下載的檔都清掉。
    UpdateGone,
    /// 系統沒有裝成（使用者在確認畫面按了取消、空間不夠、系統拒絕）。
    UpdateInstall,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    pub kind: ErrorKind,
    pub detail: String,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> ApiError {
        ApiError {
            kind,
            detail: detail.into(),
        }
    }

    pub fn no_library() -> ApiError {
        ApiError::new(ErrorKind::NoLibrary, "library is not open")
    }

    /// 只記種類。
    pub fn log(self, command: &str) -> ApiError {
        eprintln!("paperreader: {command} failed: {:?}", self.kind);
        self
    }
}

/// 核心的錯誤種類對應。沒有萬用分支：核心新增一種錯誤時，這裡編譯不過，逼人補上對應。
pub fn kind_of(e: &LibError) -> ErrorKind {
    match e {
        LibError::PermissionDenied { .. } => ErrorKind::PermissionDenied,
        LibError::NoSpace => ErrorKind::NoSpace,
        LibError::NotFound { .. } => ErrorKind::NotFound,
        LibError::Conflict => ErrorKind::Conflict,
        LibError::FormatTooNew { .. } => ErrorKind::FormatTooNew,
        LibError::ReadOnly => ErrorKind::ReadOnly,
        LibError::Unreadable { .. } => ErrorKind::Unreadable,
        LibError::InvalidInput { .. } => ErrorKind::InvalidInput,
        LibError::Io { .. } => ErrorKind::Io,
    }
}

impl From<LibError> for ApiError {
    fn from(e: LibError) -> ApiError {
        // 細節只帶種類名稱：核心的訊息含路徑與篇名，不交給畫面也不進日誌。
        ApiError::new(kind_of(&e), e.kind_name())
    }
}

/// 同步包、加入、從備份檔還原的錯誤：核心以 `reason` 的固定開頭區分幾種情況，
/// 這裡在轉換之前比對開頭，轉成畫面認得的種類；其餘照一般的對應。`reason` 本身不交給畫面。
/// 選到的是同步包、備份檔還是都不是，不靠這裡：呼叫的一方先用核心的 `peek_kind` 看清單。
pub fn sync_error(e: LibError) -> ApiError {
    let reason = match &e {
        LibError::InvalidInput { reason } | LibError::Unreadable { reason, .. } => reason.as_str(),
        _ => "",
    };
    let kind = if reason.starts_with("library id mismatch") {
        ErrorKind::SyncOtherLibrary
    } else if reason.starts_with("package from this device") {
        ErrorKind::SyncOwnPackage
    } else if reason.starts_with("package is not complete") {
        ErrorKind::SyncIncomplete
    } else {
        return e.into();
    };
    ApiError::new(kind, format!("{kind:?}"))
}

/// 匯入同步包之前先看清單：備份檔說「這是備份檔，請用從備份檔還原」，都不是說讀不懂（不必等核心解開才知道）。
pub fn expect_sync_package(archive: &std::path::Path) -> ApiResult<()> {
    use paperreader_core::store::ArchiveKind;
    match paperreader_core::sync::package::peek_kind(archive)? {
        ArchiveKind::SyncPackage => Ok(()),
        ArchiveKind::Backup => Err(ApiError::new(ErrorKind::SyncBackupFile, "SyncBackupFile")),
        ArchiveKind::Other => Err(ApiError::new(ErrorKind::Unreadable, "Unreadable")),
    }
}

impl From<tauri_plugin_paperreader::Error> for ApiError {
    fn from(e: tauri_plugin_paperreader::Error) -> ApiError {
        match e {
            tauri_plugin_paperreader::Error::Unsupported => {
                ApiError::new(ErrorKind::Unsupported, "unsupported platform")
            }
            tauri_plugin_paperreader::Error::Platform(_) => {
                ApiError::new(ErrorKind::Platform, "platform call failed")
            }
            tauri_plugin_paperreader::Error::NoSpace => {
                ApiError::new(ErrorKind::NoSpace, "no space left on device")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 核心的每一種 `LibError` 都要有對應的種類，而且種類的名稱與核心的 `kind_name` 相同
    /// （畫面的文字表依這個名稱取訊息）。
    #[test]
    fn every_lib_error_has_a_kind_with_the_same_name() {
        let all = [
            LibError::PermissionDenied { path: "p".into() },
            LibError::NoSpace,
            LibError::NotFound { what: "w".into() },
            LibError::Conflict,
            LibError::FormatTooNew { found: 9 },
            LibError::ReadOnly,
            LibError::Unreadable {
                path: "p".into(),
                reason: "r".into(),
            },
            LibError::InvalidInput { reason: "r".into() },
            LibError::Io {
                path: "p".into(),
                reason: "r".into(),
            },
        ];
        let mut seen = std::collections::HashSet::new();
        for e in &all {
            let kind = kind_of(e);
            let name = serde_json::to_value(kind).unwrap();
            assert_eq!(
                name,
                e.kind_name(),
                "kind name must match LibError::kind_name"
            );
            assert!(seen.insert(e.kind_name()), "two errors share one kind");
        }
        assert_eq!(seen.len(), 9);
    }

    #[test]
    fn plugin_errors_map_to_their_own_kinds() {
        use tauri_plugin_paperreader::Error as P;
        assert_eq!(ApiError::from(P::NoSpace).kind, ErrorKind::NoSpace);
        assert_eq!(ApiError::from(P::Unsupported).kind, ErrorKind::Unsupported);
        let e = ApiError::from(P::Platform("/storage/emulated/0/secret".into()));
        assert_eq!(e.kind, ErrorKind::Platform);
        assert!(
            !e.detail.contains("secret"),
            "platform detail must not echo paths"
        );
    }

    #[test]
    fn sync_reasons_map_to_their_own_kinds_without_leaking_the_reason() {
        let cases = [
            (
                LibError::InvalidInput {
                    reason: "library id mismatch: package A vs library B".into(),
                },
                ErrorKind::SyncOtherLibrary,
            ),
            (
                LibError::InvalidInput {
                    reason: "package from this device".into(),
                },
                ErrorKind::SyncOwnPackage,
            ),
            (
                LibError::InvalidInput {
                    reason: "package is not complete: join needs a full sync package".into(),
                },
                ErrorKind::SyncIncomplete,
            ),
            // 檔的種類不再比對英文（呼叫的一方先 `peek_kind`）：照一般的對應
            (
                LibError::Unreadable {
                    path: "/sdcard/Download/secret.zip".into(),
                    reason: "backup file, not a sync package".into(),
                },
                ErrorKind::Unreadable,
            ),
            (
                LibError::Unreadable {
                    path: "x".into(),
                    reason: "manifest.json is missing".into(),
                },
                ErrorKind::Unreadable,
            ),
            (LibError::NoSpace, ErrorKind::NoSpace),
        ];
        for (e, kind) in cases {
            let api = sync_error(e);
            assert_eq!(api.kind, kind);
            assert!(!api.detail.contains("secret") && !api.detail.contains('/'));
        }
    }

    #[test]
    fn converted_error_detail_carries_no_path_or_title() {
        let e = LibError::NotFound {
            what: "/storage/emulated/0/Documents/PaperReader/papers/Secret Title".into(),
        };
        let api: ApiError = e.into();
        assert_eq!(api.kind, ErrorKind::NotFound);
        assert!(!api.detail.contains("Secret"));
        assert!(!api.detail.contains('/'));
    }
}
