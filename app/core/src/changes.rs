//! 變更紀錄檔：每台裝置一份 `logs/changes-<裝置代號>.jsonl`，只往後附加。
//!
//! 記刪除、移到回收區與放回、劃線的放回、整篇的想法的寫入（節流），以及匯入同步包的記錄；
//! 新增與一般修改不記（檔案裡已經帶著自己的時間與裝置）。這個模組只有格式與純計算，
//! 讀寫檔案在 `local/changes.rs`。

use crate::format::Map;
use crate::ids::{is_valid_id, parse_utc_millis};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CHANGES_FILE_PREFIX: &str = "changes-";
pub const CHANGES_FILE_SUFFIX: &str = ".jsonl";

pub const PAPER_REMOVED: &str = "paper-removed";
pub const PAPER_RESTORED: &str = "paper-restored";
pub const HIGHLIGHT_DELETED: &str = "highlight-deleted";
pub const HIGHLIGHT_RESTORED: &str = "highlight-restored";
pub const STROKES_DELETED: &str = "strokes-deleted";
pub const THOUGHTS_WRITTEN: &str = "thoughts-written";
/// 刪掉重點筆記的一點（`delete_digest_point`）：`{"material": …, "point": …}`。
/// 合併重點筆記時，這一點在兩邊都拿掉（逐點合併）。
pub const DIGEST_POINT_DELETED: &str = "digest-point-deleted";
/// 移除一項材料（`remove_material`）：`{"material": …}`。同一項材料的移除與放回取時間較新的一筆：
/// 是移除時，合併後那一項在兩邊都拿掉，本機的檔搬進回收區。
pub const MATERIAL_REMOVED: &str = "material-removed";
/// 把回收區裡移除的材料放回那篇（`restore_material`）：`{"material": …}`。
pub const MATERIAL_RESTORED: &str = "material-restored";
/// 永久刪除回收區裡的一篇（`purge_trash_entry`、`empty_trash`）：`{}`。只在這台再也沒有那篇（目錄與回收區
/// 都沒有）時寫。合併後有它、而且之後沒有 `paper-restored` 時：回收區裡的那篇從兩邊拿掉、不帶回；在任一邊
/// 目錄裡的不刪，提醒 `PurgedButRestored`。之後的 `paper-removed` 不抵銷它。
pub const PAPER_PURGED: &str = "paper-purged";
/// 刪參考文獻清單的判斷（`remove_link_judgments`）：`{"judgments": ["j-…", …]}`。合併參考文獻清單時，
/// 這幾項在兩邊都拿掉。
pub const LINK_JUDGMENT_DELETED: &str = "link-judgment-deleted";
pub const SYNC_IMPORTED: &str = "sync-imported";

/// 這台這篇的上一筆 `thoughts-written` 距現在不到這麼久（毫秒）就不記（節流）。
pub const THOUGHTS_RECORD_INTERVAL_MS: i64 = 10 * 60 * 1000;

/// 測試掛勾：每附加一筆變更記錄之後呼叫一次（`LocalLibrary::set_change_hook`）。
pub type ChangeHookFn = Box<dyn FnMut(&ChangeRecord) -> bool + Send>;

/// 測試掛勾：匯入同步包寫入時每做計畫的一項之前呼叫一次（`LocalLibrary::set_sync_apply_hook`）。
pub type ApplyHookFn = Box<dyn FnMut(usize) -> bool + Send>;

/// 變更紀錄檔的一行。外框與閱讀紀錄相同；不認得的欄位放 `extra`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeRecord {
    pub id: String,
    pub t: String,
    pub device: String,
    #[serde(default)]
    pub paper: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub data: serde_json::Value,
    #[serde(flatten)]
    pub extra: Map,
}

impl ChangeRecord {
    /// （時間, 裝置, 代號）：比「較新」與排序用。時間讀不懂的算最舊。
    pub fn order_key(&self) -> (i64, &str, &str) {
        (
            parse_utc_millis(&self.t).unwrap_or(i64::MIN),
            self.device.as_str(),
            self.id.as_str(),
        )
    }

    /// `data` 裡一個字串欄位。
    pub fn data_str(&self, key: &str) -> Option<&str> {
        self.data.get(key).and_then(|v| v.as_str())
    }

    /// `thoughts-written` 的 `prev`（記錄的代號）。
    pub fn prev(&self) -> Vec<String> {
        self.data
            .get("prev")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// 一行 → 記錄。讀不懂（JSON 錯、`id` 或 `device` 不是合法的代號、`t` 不是合法的時間）交回 `None`。
pub fn parse_change_line(line: &[u8]) -> Option<ChangeRecord> {
    let r: ChangeRecord = serde_json::from_slice(line).ok()?;
    if !is_valid_id(&r.id) || !is_valid_id(&r.device) || parse_utc_millis(&r.t).is_none() {
        return None;
    }
    Some(r)
}

/// `changes-<裝置代號>.jsonl`。
pub fn changes_file_name(device: &str) -> String {
    format!("{CHANGES_FILE_PREFIX}{device}{CHANGES_FILE_SUFFIX}")
}

/// 是不是變更紀錄檔的檔名（`changes-` 開頭、`.jsonl` 結尾）。
pub fn is_changes_file(name: &str) -> bool {
    name.starts_with(CHANGES_FILE_PREFIX) && name.ends_with(CHANGES_FILE_SUFFIX)
}

/// 整篇的想法的指紋：`NotesDoc.thoughts` 那個字串（換行已轉成 `\n`、去掉前後空行）的 SHA-256。
pub fn thoughts_hash(text: &str) -> String {
    crate::jsonio::hex(&Sha256::digest(text.as_bytes()))
}

/// 依（`t`, `device`, `id`）排序。
pub fn sort_records(v: &mut [ChangeRecord]) {
    v.sort_by(|a, b| (&a.t, &a.device, &a.id).cmp(&(&b.t, &b.device, &b.id)));
}

// ---- 整篇的想法的節點 ----

/// 這台這篇的「上一筆」：`device` 對 `paper` 寫過的 `thoughts-written` 裡（`t`, `id`）最晚的一筆。
pub fn last_own_thoughts<'a>(
    records: &'a [ChangeRecord],
    device: &str,
    paper: &str,
) -> Option<&'a ChangeRecord> {
    records
        .iter()
        .filter(|r| {
            r.kind == THOUGHTS_WRITTEN && r.device == device && r.paper.as_deref() == Some(paper)
        })
        .max_by(|a, b| (parse_utc_millis(&a.t), &a.id).cmp(&(parse_utc_millis(&b.t), &b.id)))
}

/// `N(裝置, 內容)`：那台這篇的上一筆若指紋等於這份內容就是它；否則合起來的記錄裡這一篇指紋等於
/// 這份內容、（`t`, `id`）最晚的一筆；都沒有是 `None`。
pub fn thoughts_node<'a>(
    records: &'a [ChangeRecord],
    device: &str,
    paper: &str,
    hash: &str,
) -> Option<&'a ChangeRecord> {
    if let Some(last) = last_own_thoughts(records, device, paper)
        && last.data_str("sha256") == Some(hash)
    {
        return Some(last);
    }
    records
        .iter()
        .filter(|r| {
            r.kind == THOUGHTS_WRITTEN
                && r.paper.as_deref() == Some(paper)
                && r.data_str("sha256") == Some(hash)
        })
        .max_by(|a, b| (parse_utc_millis(&a.t), &a.id).cmp(&(parse_utc_millis(&b.t), &b.id)))
}

/// 下一筆「接在哪裡」：寫入前的內容有節點就接那個節點；沒有就接這台這篇的上一筆；都沒有是 `[]`。
pub fn thoughts_prev(
    records: &[ChangeRecord],
    device: &str,
    paper: &str,
    before_hash: &str,
) -> Vec<String> {
    if let Some(n) = thoughts_node(records, device, paper, before_hash) {
        return vec![n.id.clone()];
    }
    last_own_thoughts(records, device, paper)
        .map(|r| vec![r.id.clone()])
        .unwrap_or_default()
}

/// `set_thoughts` 要不要記、記的話 `prev` 是什麼。
/// `now_ms` 是現在的毫秒數。交回 `None` 表示不記。
pub fn thoughts_record_for_write(
    records: &[ChangeRecord],
    device: &str,
    paper: &str,
    before_hash: &str,
    new_hash: &str,
    now_ms: i64,
) -> Option<Vec<String>> {
    if before_hash == new_hash {
        return None;
    }
    let last = last_own_thoughts(records, device, paper);
    let before_node = thoughts_node(records, device, paper, before_hash);
    // 把想法清空時不節流，一定記一筆
    let clearing = new_hash == thoughts_hash("");
    if !clearing
        && let Some(last) = last
        && let Some(t) = parse_utc_millis(&last.t)
        && now_ms - t < THOUGHTS_RECORD_INTERVAL_MS
        && before_node.is_none_or(|n| n.id == last.id)
    {
        return None;
    }
    Some(thoughts_prev(records, device, paper, before_hash))
}

/// 補記（匯出前、準備匯入時）。交回 `prev`；不記是 `None`。
///
/// - 目前的內容不是空的：有節點就不記；沒有節點就記，接在這台這篇的上一筆之後。
/// - 目前的內容是空的：這台這篇的上一筆不是空的，就記一筆空的、接在那一筆之後（在筆記工具裡清空）；
///   這台沒有上一筆、空的內容也沒有節點，而別台對這篇記過想法時，也記一筆空的（`prev` 是 `[]`），
///   免得合併時被當成起點、清掉的文字回來。
pub fn thoughts_catch_up(
    records: &[ChangeRecord],
    device: &str,
    paper: &str,
    current: &str,
) -> Option<Vec<String>> {
    let hash = thoughts_hash(current);
    if current.is_empty() {
        let last = last_own_thoughts(records, device, paper);
        if let Some(l) = last {
            return (l.data_str("sha256") != Some(hash.as_str())).then(|| vec![l.id.clone()]);
        }
        let any = records
            .iter()
            .any(|r| r.kind == THOUGHTS_WRITTEN && r.paper.as_deref() == Some(paper));
        if any && thoughts_node(records, device, paper, &hash).is_none() {
            return Some(Vec::new());
        }
        return None;
    }
    if thoughts_node(records, device, paper, &hash).is_some() {
        return None;
    }
    Some(thoughts_prev(records, device, paper, &hash))
}
