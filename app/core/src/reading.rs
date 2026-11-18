//! 閱讀事件的種類與驗證。
//!
//! 三種事件：`open`、`position`、`status`。這裡只管「一筆事件是不是有效、裡面是什麼」，
//! 不管檔案怎麼讀寫（那在 `local/events.rs`），也不管狀況怎麼算（那在 `progress.rs`）。

use crate::format::Event;
use crate::ids::parse_event_millis;
use serde_json::Value;

pub const TYPE_OPEN: &str = "open";
pub const TYPE_POSITION: &str = "position";
pub const TYPE_STATUS: &str = "status";

/// 頁碼的上限：材料沒有記頁數時仍要有個界線。
pub const MAX_PAGE: i64 = 100_000;

/// 同一個位置的判斷。
pub const SAME_OFFSET_BELOW: f64 = 0.02;
pub const SAME_ZOOM_BELOW: f64 = 0.001;
pub const SAME_LEFT_BELOW: f64 = 0.02;

/// 位置事件 `data` 裡選填的水平位置欄位。
pub const LEFT_FIELD: &str = "left";

/// 一筆有效事件的內容。
#[derive(Debug, Clone, PartialEq)]
pub enum ReadingKind {
    Open {
        material: String,
    },
    Position {
        material: String,
        page: u32,
        offset: f64,
        zoom: Option<f64>,
        /// 畫面左緣在這一頁寬度的幾分之幾；沒有記或讀不懂是 `None`。
        left: Option<f64>,
    },
    Status {
        read: bool,
    },
}

/// 通過驗證的事件。
#[derive(Debug, Clone, PartialEq)]
pub struct ValidEvent {
    pub id: String,
    pub t: String,
    pub device: String,
    pub paper: String,
    pub kind: ReadingKind,
}

/// 驗證一筆讀進來的事件。無效交回 `None`：算狀況時略過，不報錯，也不從檔案裡刪掉。
///
/// 有效要同時符合：`t` 是時間的標準格式（而且是真實的時間，不晚於 `9999-12-31T23:59:59.998Z`）；`id`、`device`、`paper` 是非空的字串；
/// `type` 是三種之一；`data` 的欄位齊全、型別與範圍照事件格式。
pub fn parse_event(e: &Event) -> Option<ValidEvent> {
    if e.id.is_empty() || e.device.is_empty() || e.paper.is_empty() {
        return None;
    }
    parse_event_millis(&e.t)?;
    let data = e.data.as_object()?;
    let kind = match e.kind.as_str() {
        TYPE_OPEN => ReadingKind::Open {
            material: non_empty_str(data.get("material")?)?,
        },
        TYPE_POSITION => {
            let page = data.get("page")?.as_i64()?;
            let offset = data.get("offset")?.as_f64()?;
            let zoom = match data.get("zoom")? {
                Value::Null => None,
                v => Some(v.as_f64()?),
            };
            let (page, offset, zoom) = validate_position(page, offset, zoom).ok()?;
            // 水平位置是選填的：缺少、`null`、不是數字、超出範圍都當成不知道，事件照樣有效
            // （舊版應用不看這一欄，兩版算出的有效與否才一致）
            let left = data
                .get(LEFT_FIELD)
                .and_then(Value::as_f64)
                .and_then(|l| validate_left(Some(l)).ok().flatten());
            ReadingKind::Position {
                material: non_empty_str(data.get("material")?)?,
                page,
                offset,
                zoom,
                left,
            }
        }
        TYPE_STATUS => ReadingKind::Status {
            read: data.get("read")?.as_bool()?,
        },
        _ => return None,
    };
    Some(ValidEvent {
        id: e.id.clone(),
        t: e.t.clone(),
        device: e.device.clone(),
        paper: e.paper.clone(),
        kind,
    })
}

fn non_empty_str(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(String::from)
}

/// 位置的欄位範圍：頁碼 1 到 100000；頁內位置 0 到 1（含兩端）；縮放大於 0 或不知道。
/// 數字必須是有限的（不是 NaN 與無限大）。範圍之內交回（頁碼, 頁內位置, 縮放），否則交回原因。
pub fn validate_position(
    page: i64,
    offset: f64,
    zoom: Option<f64>,
) -> Result<(u32, f64, Option<f64>), String> {
    if !(1..=MAX_PAGE).contains(&page) {
        return Err(format!("page must be 1..={MAX_PAGE}, got {page}"));
    }
    if !offset.is_finite() || !(0.0..=1.0).contains(&offset) {
        return Err(format!("offset must be within 0..=1, got {offset}"));
    }
    if let Some(z) = zoom
        && (!z.is_finite() || z <= 0.0)
    {
        return Err(format!("zoom must be greater than 0 or null, got {z}"));
    }
    let page = u32::try_from(page).expect("page is within 1..=100000");
    Ok((page, offset, zoom))
}

/// 兩個位置算不算同一個：頁碼相同；頁內位置相差小於 0.02；
/// 縮放相同（兩邊都是 `None`，或相差小於 0.001）。
pub fn same_position(a: (u32, f64, Option<f64>), b: (u32, f64, Option<f64>)) -> bool {
    a.0 == b.0 && (a.1 - b.1).abs() < SAME_OFFSET_BELOW && same_optional(a.2, b.2, SAME_ZOOM_BELOW)
}

/// 水平位置的範圍：沒給是 `None`；給了要是 0 到 1（含兩端）的有限數字，否則交回原因。
pub fn validate_left(left: Option<f64>) -> Result<Option<f64>, String> {
    match left {
        Some(l) if !l.is_finite() || !(0.0..=1.0).contains(&l) => {
            Err(format!("left must be within 0..=1 or null, got {l}"))
        }
        other => Ok(other),
    }
}

/// 水平位置算不算相同：兩邊都不知道，或相差小於 0.02；一邊知道、一邊不知道算不同。
pub fn same_left(a: Option<f64>, b: Option<f64>) -> bool {
    same_optional(a, b, SAME_LEFT_BELOW)
}

fn same_optional(a: Option<f64>, b: Option<f64>, below: f64) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => (x - y).abs() < below,
        _ => false,
    }
}
