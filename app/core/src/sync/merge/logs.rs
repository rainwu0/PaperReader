//! 閱讀紀錄與變更紀錄檔的合併：每台裝置只寫自己的檔、只往後附加，一行都不刪。

use crate::changes::ChangeRecord;
use std::collections::{HashMap, HashSet};

/// 一份紀錄檔要對本機做的事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogMerge {
    Unchanged,
    /// 把這些位元組附加到本機的檔（本機沒有這個檔時就是整份內容）。
    Append(Vec<u8>),
}

/// 一行的 `id`（讀不懂的行是 `None`）。
fn line_id(line: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(line).ok()?;
    v.get("id")?.as_str().map(String::from)
}

fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes
        .split(|b| *b == b'\n')
        .filter(|l| !l.iter().all(|b| b.is_ascii_whitespace()))
}

/// 依檔名逐份：只在匯入那邊的複製過來；一份是另一份開頭的一段時，本機較短就接上多出的那段；
/// 互不為開頭時依每一行的 `id` 取聯集（本機的行全部保留，再接上匯入那邊本機沒有的行；讀不懂的行只保留本機的）。
pub fn merge_log(local: Option<&[u8]>, remote: &[u8]) -> LogMerge {
    let Some(local) = local else {
        return if remote.is_empty() {
            LogMerge::Unchanged
        } else {
            LogMerge::Append(remote.to_vec())
        };
    };
    if remote.starts_with(local) {
        return if remote.len() > local.len() {
            LogMerge::Append(remote[local.len()..].to_vec())
        } else {
            LogMerge::Unchanged
        };
    }
    if local.starts_with(remote) {
        return LogMerge::Unchanged;
    }
    let have: HashSet<String> = lines(local).filter_map(line_id).collect();
    let mut out = Vec::new();
    let mut added: HashSet<String> = HashSet::new();
    for l in lines(remote) {
        let Some(id) = line_id(l) else {
            continue;
        };
        if have.contains(&id) || !added.insert(id) {
            continue;
        }
        out.extend_from_slice(l);
        out.push(b'\n');
    }
    if out.is_empty() {
        return LogMerge::Unchanged;
    }
    if !local.is_empty() && !local.ends_with(b"\n") {
        out.insert(0, b'\n');
    }
    LogMerge::Append(out)
}

/// 兩邊的變更記錄合起來（依代號去重），依（`t`, `device`, `id`）排序。
pub fn union_records(a: &[ChangeRecord], b: &[ChangeRecord]) -> Vec<ChangeRecord> {
    let mut seen: HashMap<&str, ()> = HashMap::new();
    let mut out = Vec::new();
    for r in a.iter().chain(b.iter()) {
        if seen.insert(r.id.as_str(), ()).is_none() {
            out.push(r.clone());
        }
    }
    crate::changes::sort_records(&mut out);
    out
}
