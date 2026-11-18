//! `references.json`（參考文獻清單）的合併：依檔合併，同 `ink.json`。

use super::{Change, MergeCtx, cmp_stamp};
use crate::references::{ReferencesFile, carry_entry_extras, within_limits};
use crate::store::{ItemKind, NoticeKind};
use std::cmp::Ordering;
use std::collections::HashSet;

/// 抽出的部分取哪一邊：交回 `true` 表示匯入那邊勝。依序比：① 原文指紋是不是合併後那篇原文的；
/// ② `extractor` 相同時 `version` 高的；③（`extraction.created`, `extraction.created_by`）較新的。都相同留本機的。
fn remote_extraction_wins(l: &ReferencesFile, r: &ReferencesFile, source: Option<&str>) -> bool {
    let current = |f: &ReferencesFile| source == Some(f.extraction.source_sha256.as_str());
    match (current(l), current(r)) {
        (false, true) => return true,
        (true, false) => return false,
        _ => {}
    }
    if l.extraction.extractor == r.extraction.extractor
        && l.extraction.version != r.extraction.version
    {
        return r.extraction.version > l.extraction.version;
    }
    cmp_stamp(
        Some(&r.extraction.created),
        Some(&r.extraction.created_by),
        Some(&l.extraction.created),
        Some(&l.extraction.created_by),
    ) == Ordering::Greater
}

/// 合併一篇的參考文獻清單。`deleted` 是合併後 `link-judgment-deleted` 記錄裡這篇的判斷代號；`source` 是合併後
/// 那篇原文的指紋。交回要寫的內容；不必寫是 `None`。讀不懂、`format` 較新的由呼叫的一方先擋下。
pub fn merge_references(
    local: Option<&ReferencesFile>,
    remote: Option<&ReferencesFile>,
    deleted: &HashSet<String>,
    source: Option<&str>,
    pid: &str,
    title: Option<&str>,
    ctx: &mut MergeCtx,
) -> Option<ReferencesFile> {
    let l = match (local, remote) {
        (None, None) => return None,
        (None, Some(r)) => {
            // 只在匯入那邊：照原樣複製（拿掉已刪除的判斷後）
            let mut doc = r.clone();
            doc.judgments.retain(|j| !deleted.contains(&j.id));
            if !within_limits(&doc) {
                ctx.notice(NoticeKind::TooLargeToMerge, Some(pid), None, None);
                return None;
            }
            ctx.note(Change::Added, ItemKind::References, Some(pid), title, 1);
            return Some(doc);
        }
        (Some(l), _) => l,
    };
    let mut doc = l.clone();
    if let Some(r) = remote {
        // 抽出的部分整份取一邊；輸的那份條目上的不認得欄位依找回鍵搬到勝的那份
        if remote_extraction_wins(l, r, source) {
            doc.extraction = r.extraction.clone();
            doc.entries = r.entries.clone();
            carry_entry_extras(&mut doc.entries, &l.entries);
        } else {
            carry_entry_extras(&mut doc.entries, &r.entries);
        }
        // 判斷依代號取聯集：本機的順序，接上匯入那邊多出的；同一個代號留本機的
        let have: HashSet<&str> = l.judgments.iter().map(|j| j.id.as_str()).collect();
        for j in &r.judgments {
            if !have.contains(j.id.as_str()) {
                doc.judgments.push(j.clone());
            }
        }
        for (k, v) in &r.extra {
            if !doc.extra.contains_key(k) {
                doc.extra.insert(k.clone(), v.clone());
            }
        }
    }
    doc.judgments.retain(|j| !deleted.contains(&j.id));
    if doc.extraction == l.extraction
        && doc.entries == l.entries
        && doc.judgments == l.judgments
        && doc.extra == l.extra
    {
        return None;
    }
    // 合併的結果超過上限：不寫，本機的不動（不寫出會被當成讀不懂的檔）
    if !within_limits(&doc) {
        ctx.notice(NoticeKind::TooLargeToMerge, Some(pid), None, None);
        return None;
    }
    ctx.note(Change::Updated, ItemKind::References, Some(pid), title, 1);
    doc.modified = Some(ctx.now.clone());
    doc.modified_by = Some(ctx.device.clone());
    Some(doc)
}
