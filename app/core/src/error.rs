//! 文獻庫操作的錯誤。：交回可分辨的原因，不吞掉、不自己重試。

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum LibError {
    #[error("no permission: {path}")]
    PermissionDenied { path: String },
    #[error("no space left on device")]
    NoSpace,
    #[error("not found: {what}")]
    NotFound { what: String },
    /// 檔案在讀取之後被別處改過。
    #[error("file was changed elsewhere")]
    Conflict,
    #[error("format version {found} is newer than this app understands")]
    FormatTooNew { found: u32 },
    #[error("library is read-only")]
    ReadOnly,
    #[error("unreadable content in {path}: {reason}")]
    Unreadable { path: String, reason: String },
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    #[error("i/o error at {path}: {reason}")]
    Io { path: String, reason: String },
}

pub type Result<T> = std::result::Result<T, LibError>;

impl LibError {
    /// 項目名，供命令列工具與日後的介面層辨認。
    pub fn kind_name(&self) -> &'static str {
        match self {
            LibError::PermissionDenied { .. } => "PermissionDenied",
            LibError::NoSpace => "NoSpace",
            LibError::NotFound { .. } => "NotFound",
            LibError::Conflict => "Conflict",
            LibError::FormatTooNew { .. } => "FormatTooNew",
            LibError::ReadOnly => "ReadOnly",
            LibError::Unreadable { .. } => "Unreadable",
            LibError::InvalidInput { .. } => "InvalidInput",
            LibError::Io { .. } => "Io",
        }
    }
}

/// 把標準的 I/O 錯誤依種類對應到 `LibError`。
pub fn io_error(path: &Path, e: std::io::Error) -> LibError {
    use std::io::ErrorKind;
    let p = path.display().to_string();
    match e.kind() {
        ErrorKind::NotFound => LibError::NotFound { what: p },
        ErrorKind::PermissionDenied => LibError::PermissionDenied { path: p },
        ErrorKind::StorageFull => LibError::NoSpace,
        _ => LibError::Io {
            path: p,
            reason: e.to_string(),
        },
    }
}
