//! 是不是 PDF、加入時的預設篇名。這兩條規則放在核心，換一個入口就不必再寫一份。
//! 核心不解析 PDF 的內容。

use crate::error::{LibError, Result, io_error};
use std::io::Read;
use std::path::Path;

/// PDF 規格允許檔頭前面有少量雜訊：標記 `%PDF-` 出現在前 1024 個位元組內就算。
pub const PDF_HEADER_WINDOW: usize = 1024;

const MARK: &[u8] = b"%PDF-";

/// `%PDF-` 出現在 `head` 的前 1024 個位元組內。
pub fn looks_like_pdf(head: &[u8]) -> bool {
    let window = &head[..head.len().min(PDF_HEADER_WINDOW)];
    window.windows(MARK.len()).any(|w| w == MARK)
}

/// 只讀檔案的前 1024 個位元組。檔案不存在 → `NotFound`；是資料夾 → `InvalidInput`。
pub fn is_pdf_file(path: &Path) -> Result<bool> {
    let meta = std::fs::metadata(path).map_err(|e| io_error(path, e))?;
    if meta.is_dir() {
        return Err(LibError::InvalidInput {
            reason: format!("not a file: {}", path.display()),
        });
    }
    let f = std::fs::File::open(path).map_err(|e| io_error(path, e))?;
    let mut head = Vec::with_capacity(PDF_HEADER_WINDOW);
    f.take(PDF_HEADER_WINDOW as u64)
        .read_to_end(&mut head)
        .map_err(|e| io_error(path, e))?;
    Ok(looks_like_pdf(&head))
}

/// 加入時的預設篇名：去掉前後空白，去掉結尾一個 `.pdf`（不分大小寫），再去掉前後空白；
/// 空的交回 `None`。
pub fn title_from_file_name(name: &str) -> Option<String> {
    let t = name.trim();
    let stem = match t.len().checked_sub(4).and_then(|i| t.get(i..)) {
        Some(tail) if tail.eq_ignore_ascii_case(".pdf") => &t[..t.len() - 4],
        _ => t,
    };
    let s = stem.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}
