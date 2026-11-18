//! 筆跡檔 `ink.json` 的格式：每篇文獻資料夾裡一份，與 `paper.json` 同一層。
//!
//! 筆跡的點用 PDF 頁面座標（單位是點，原點在頁面左下），與螢幕大小和縮放無關。
//! 每個結構都以 `extra` 保留不認得的欄位，讀進來再寫回去不會丟掉。

use crate::error::{LibError, Result};
use crate::format::{Map, format_or_current, null_as_default, one};
use serde::{Deserialize, Serialize};

pub const INK_FILE: &str = "ink.json";

/// 一筆最多的點數。
pub const MAX_POINTS: usize = 20_000;
/// 線寬的上限（點）。
pub const MAX_WIDTH: f64 = 200.0;
/// 點座標的絕對值上限（PDF 的點）：太大的值取整時會變成無限大、寫成 `null`，讓整份檔案讀不懂。
pub const MAX_COORD: f64 = 1_000_000.0;
/// 頁序的上限（不含）。
const MAX_PAGE_INDEX: u32 = 100_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InkFile {
    #[serde(default = "one", deserialize_with = "format_or_current")]
    pub format: u32,
    #[serde(default)]
    pub modified: Option<String>,
    #[serde(default)]
    pub modified_by: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub strokes: Vec<Stroke>,
    #[serde(flatten)]
    pub extra: Map,
}

impl Default for InkFile {
    fn default() -> InkFile {
        InkFile {
            format: crate::format::FORMAT_VERSION,
            modified: None,
            modified_by: None,
            strokes: Vec::new(),
            extra: Map::new(),
        }
    }
}

fn full_opacity() -> f64 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub id: String,
    /// 畫在哪一份材料上（頁序是那份材料的頁序）。
    pub material: String,
    /// 頁序，從 0 起算。
    pub page: u32,
    /// `#rrggbb`。
    pub color: String,
    /// 線寬，單位是 PDF 的點。
    pub width: f64,
    /// 不透明度，大於 0、最多 1（螢光筆小於 1）。
    #[serde(default = "full_opacity")]
    pub opacity: f64,
    pub created: String,
    /// `[x, y]`，PDF 頁面座標。
    pub points: Vec<[f64; 2]>,
    #[serde(flatten)]
    pub extra: Map,
}

/// 座標取到小數兩位。
pub(crate) fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn bad(reason: impl Into<String>) -> LibError {
    LibError::InvalidInput {
        reason: reason.into(),
    }
}

/// 檢查畫面交來的一筆：不合規則時不寫任何東西。`pages` 是那份材料已知的總頁數。
pub(crate) fn validate_draft(d: &crate::store::StrokeDraft, pages: Option<u32>) -> Result<()> {
    if d.page_index >= MAX_PAGE_INDEX {
        return Err(bad(format!("page index out of range: {}", d.page_index)));
    }
    if let Some(n) = pages
        && d.page_index >= n
    {
        return Err(bad(format!(
            "page index {} is beyond the {n} pages of the material",
            d.page_index
        )));
    }
    if d.points.is_empty() || d.points.len() > MAX_POINTS {
        return Err(bad(format!(
            "a stroke needs 1 to {MAX_POINTS} points: {}",
            d.points.len()
        )));
    }
    if d.points
        .iter()
        .any(|p| !(p[0].abs() <= MAX_COORD && p[1].abs() <= MAX_COORD))
    {
        return Err(bad(format!(
            "point coordinates must be finite numbers within ±{MAX_COORD}"
        )));
    }
    if !is_hex_color(&d.color) {
        return Err(bad(format!("color must be #rrggbb: {:?}", d.color)));
    }
    if !(d.width.is_finite() && d.width > 0.0 && d.width <= MAX_WIDTH) {
        return Err(bad(format!(
            "width must be in (0, {MAX_WIDTH}]: {}",
            d.width
        )));
    }
    if !(d.opacity.is_finite() && d.opacity > 0.0 && d.opacity <= 1.0) {
        return Err(bad(format!("opacity must be in (0, 1]: {}", d.opacity)));
    }
    Ok(())
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}
