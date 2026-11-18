//! 代號（ULID）與時間（世界協調時間）。

use time::{OffsetDateTime, format_description::FormatItem, macros::format_description};

const UTC_FMT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");
const COMPACT_FMT: &[FormatItem<'static>] =
    format_description!("[year][month][day]T[hour][minute][second]Z");

/// 26 個字元的 ULID，在裝置上產生，不需要連線。
pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}

/// `2026-11-13T08:25:02.123Z`
pub fn now_utc() -> String {
    OffsetDateTime::now_utc()
        .format(UTC_FMT)
        .expect("fixed format cannot fail")
}

/// `20261113T083011Z`，不含 `:`，可當資料夾名稱。
pub fn now_compact() -> String {
    OffsetDateTime::now_utc()
        .format(COMPACT_FMT)
        .expect("fixed format cannot fail")
}

/// 是不是合法的代號：26 個 Crockford Base32 字元（`0123456789ABCDEFGHJKMNPQRSTVWXYZ`，大寫）。
/// 裝置代號會變成紀錄檔的檔名，所以拿來組檔名之前一定要先過這一關。
pub fn is_valid_id(s: &str) -> bool {
    s.len() == 26
        && s.bytes()
            .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b))
}

const UTC_PARSE_FMT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]");

/// 嚴格的 `2026-11-13T08:25:02.123Z`（到毫秒、結尾 `Z`、真實存在的日期與時間）→ 自 Unix 紀元起的毫秒數。
/// 格式不符或不是真實的時間交回 `None`。
pub fn parse_utc_millis(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 24 {
        return None;
    }
    let digit_at = |i: usize| b[i].is_ascii_digit();
    let shape_ok = (0..4).all(digit_at)
        && b[4] == b'-'
        && (5..7).all(digit_at)
        && b[7] == b'-'
        && (8..10).all(digit_at)
        && b[10] == b'T'
        && (11..13).all(digit_at)
        && b[13] == b':'
        && (14..16).all(digit_at)
        && b[16] == b':'
        && (17..19).all(digit_at)
        && b[19] == b'.'
        && (20..23).all(digit_at)
        && b[23] == b'Z';
    if !shape_ok {
        return None;
    }
    let dt = time::PrimitiveDateTime::parse(&s[..23], UTC_PARSE_FMT).ok()?;
    let nanos = dt.assume_utc().unix_timestamp_nanos();
    i64::try_from(nanos / 1_000_000).ok()
}

/// 事件的 `t` 能到的最晚時間（毫秒）：`9999-12-31T23:59:59.998Z`。再晚的事件算無效，
/// 這樣「上一筆加 1 毫秒」一定還在可表示的範圍內。
pub const MAX_EVENT_MS: i64 = 253_402_300_799_998;

/// 事件的 `t` → 毫秒。格式不符、不是真實的時間、或晚於 `MAX_EVENT_MS` 交回 `None`。
pub fn parse_event_millis(s: &str) -> Option<i64> {
    parse_utc_millis(s).filter(|ms| *ms <= MAX_EVENT_MS)
}

/// 自 Unix 紀元起的毫秒數 → `2026-11-13T08:25:02.123Z`。超出可表示的範圍交回 `None`，不會崩潰。
pub fn format_utc_millis(ms: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
        .ok()?
        .format(UTC_FMT)
        .ok()
}

/// 現在的時間，自 Unix 紀元起的毫秒數。
pub fn now_utc_millis() -> i64 {
    i64::try_from(OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
        .expect("current time in range")
}
