//! 抽出的文字 `text.json`：每篇文獻的資料夾裡最多一份，與 `paper.json`、
//! `ink.json`、`references.json` 同一層；不列在材料清單。原文逐頁的純文字，由逐篇處理抽、經 `write_text` 寫入；
//! 原文換了由逐篇處理重抽、整份換掉，沒有單獨的移除。
//!
//! 這個模組只有格式、檢查與讀檔。讀寫的操作在 `local/text.rs`，同步的合併在 `sync/merge/text.rs`。

use crate::error::{LibError, Result, io_error};
use crate::format::Map;
use crate::jsonio::{VersionToken, read_json, read_json_untokened};
use crate::references::Extraction;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const TEXT_FILE: &str = "text.json";

/// 抽法的代號（這一版）與它在 `extractor` 的上限。
pub const MAX_EXTRACTOR: usize = 40;
/// 一頁最多幾個字（Unicode 字元）。
pub const MAX_PAGE_CHARS: usize = 100_000;
/// 一份最多幾個字。
pub const MAX_TOTAL_CHARS: usize = 5_000_000;
/// 寫出的 JSON 最多幾個位元組；不合的不收（`InvalidInput`），合併的結果超過就不寫。
pub const MAX_WRITE_BYTES: usize = 24 * 1024 * 1024;
/// 讀到超過這個大小的當成讀不懂。
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextFile {
    #[serde(
        default = "crate::format::one",
        deserialize_with = "crate::format::format_or_current"
    )]
    pub format: u32,
    /// 檔頭，重抽時連同 `pages` 整個換；型別和 `references.json` 的 `extraction` 共用。
    pub extraction: Extraction,
    /// 每頁一項，依頁序；第 i 項的 `page` 就是 i。
    pub pages: Vec<TextPage>,
    /// 到了上限沒抽完；`false` 時不寫出。
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
    #[serde(flatten)]
    pub extra: Map,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// 一頁的文字。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPage {
    /// 頁序，從 0 起。
    pub page: u32,
    /// PDF.js 文字片段依序接起來，標了行尾的片段後接 `\n`；沒有文字的頁是空字串。
    pub text: String,
    #[serde(flatten)]
    pub extra: Map,
}

/// 寫入時交來的草稿（`write_text`）。`extraction.created` 這類由核心蓋的欄位不在這裡。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TextDraft {
    pub extractor: String,
    pub version: u32,
    pub source_material: String,
    pub source_sha256: String,
    /// 每頁的文字，第 i 項是第 i 頁。
    pub pages: Vec<String>,
    #[serde(default)]
    pub truncated: bool,
}

/// 逐篇處理判斷做過沒有用的檔頭（`text_states`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextState {
    pub extractor: String,
    pub version: u32,
    pub source_sha256: String,
}

fn bad(reason: impl Into<String>) -> LibError {
    LibError::InvalidInput {
        reason: reason.into(),
    }
}

/// 檢查草稿：抽法的代號 1 到 40 字、版本 1 以上；頁數不多於原文的 `source_pages`，沒截斷時相等；
/// 一頁最多 10 萬字、一份最多 500 萬字（Unicode 字元）。位元組的上限寫成 JSON 之後才看（`check_size`）。
pub(crate) fn check_draft(d: &TextDraft, source_pages: u32) -> Result<()> {
    let n = d.extractor.chars().count();
    if n == 0 || n > MAX_EXTRACTOR {
        return Err(bad(format!(
            "extractor must be 1 to {MAX_EXTRACTOR} characters: {:?}",
            d.extractor
        )));
    }
    if d.version < 1 {
        return Err(bad("extraction version must be at least 1"));
    }
    let pages = d.pages.len();
    if pages > source_pages as usize || (!d.truncated && pages != source_pages as usize) {
        return Err(bad(format!(
            "{pages} pages for a source of {source_pages} pages (truncated: {})",
            d.truncated
        )));
    }
    let mut total = 0usize;
    for (i, p) in d.pages.iter().enumerate() {
        let n = p.chars().count();
        if n > MAX_PAGE_CHARS {
            return Err(bad(format!(
                "page {i} has {n} characters, more than {MAX_PAGE_CHARS} characters"
            )));
        }
        total += n;
    }
    if total > MAX_TOTAL_CHARS {
        return Err(bad(format!(
            "{total} characters in all, more than {MAX_TOTAL_CHARS} characters"
        )));
    }
    Ok(())
}

/// 寫成 JSON（`write_json_atomic` 的寫法）的位元組數；寫不出來當成無限大。
pub(crate) fn written_bytes(doc: &TextFile) -> usize {
    crate::jsonio::to_canonical_json(doc).map_or(usize::MAX, |s| s.len())
}

/// 頁上不認得的欄位從另一份搬過來：依頁序，`to` 的那一頁沒有同名的鍵才加；超過 `to` 頁數的丟掉。
/// 呼叫的一方先確認兩份抽自同一份原文。
pub(crate) fn carry_page_extras(to: &mut [TextPage], from: &[TextPage]) {
    for (t, f) in to.iter_mut().zip(from) {
        for (k, v) in &f.extra {
            if !t.extra.contains_key(k) {
                t.extra.insert(k.clone(), v.clone());
            }
        }
    }
}

/// 讀一份 `text.json`：不存在是 `None`；超過 32 MB、不是一般檔案、讀不懂是 `Unreadable`。讀抽出的文字只用這一個
/// （讀寫、檢查、同步都是），「超過上限」到哪裡都是讀不懂。
pub fn read_text_file(path: &Path) -> Result<Option<(TextFile, VersionToken)>> {
    if !check_file(path)? {
        return Ok(None);
    }
    let read = read_json::<TextFile>(path).and_then(|(doc, token)| {
        check_sequence(path, &doc)?;
        Ok((doc, token))
    });
    match read {
        Ok(v) => Ok(Some(v)),
        Err(LibError::NotFound { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// 同 `read_text_file`，但不算版本記號（給全文搜尋的索引讀檔，`read_for_index`）。
pub(crate) fn read_text_file_untokened(path: &Path) -> Result<Option<TextFile>> {
    if !check_file(path)? {
        return Ok(None);
    }
    let read = read_json_untokened::<TextFile>(path).and_then(|doc| {
        check_sequence(path, &doc)?;
        Ok(doc)
    });
    match read {
        Ok(v) => Ok(Some(v)),
        Err(LibError::NotFound { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// 頁序從 0 起、連續（第 i 項的 `page` 就是 i）；不合當成讀不懂。
fn check_sequence(path: &Path, doc: &TextFile) -> Result<()> {
    match doc
        .pages
        .iter()
        .enumerate()
        .find(|(i, p)| p.page as usize != *i)
    {
        None => Ok(()),
        Some((i, p)) => Err(LibError::Unreadable {
            path: path.display().to_string(),
            reason: format!("page {i} is numbered {}", p.page),
        }),
    }
}

/// 只看檔案屬性（不開檔）：不存在交回 `false`；不是一般檔案、超過 32 MB 是 `Unreadable`。快速檢查用它，讀檔的幾個函式先經過它。
pub(crate) fn check_file(path: &Path) -> Result<bool> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(io_error(path, e)),
    };
    let unreadable = |reason: &str| LibError::Unreadable {
        path: path.display().to_string(),
        reason: reason.to_string(),
    };
    if !meta.is_file() {
        return Err(unreadable("not a file"));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(unreadable("file is larger than 32 MB"));
    }
    Ok(true)
}

/// 只讀檔頭（`text_states`）：`pages` 不建（`IgnoredAny`）。不存在、讀不懂、超過 32 MB 都是 `None`；`format` 較新
/// 照樣交回。
pub(crate) fn read_text_state(path: &Path) -> Option<TextState> {
    #[derive(Deserialize)]
    struct Head {
        extraction: Extraction,
        #[allow(dead_code)]
        pages: serde::de::IgnoredAny,
    }
    if !matches!(check_file(path), Ok(true)) {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let head: Head = serde_json::from_slice(&bytes).ok()?;
    Some(TextState {
        extractor: head.extraction.extractor,
        version: head.extraction.version,
        source_sha256: head.extraction.source_sha256,
    })
}
