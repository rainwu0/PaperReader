//! `text.json`（抽出的文字）的合併：依檔合併，整份取一邊。不新增衝突種類：內容全由原文算出，
//! 沒有要問使用者的事。

use super::{Change, MergeCtx, cmp_stamp};
use crate::store::{ItemKind, NoticeKind};
use crate::text::{MAX_WRITE_BYTES, TextFile, carry_page_extras, written_bytes};
use std::cmp::Ordering;

/// 取哪一邊：交回 `true` 表示匯入那邊勝。依序比：① `source_sha256` 是合併後那篇原文的指紋的勝；② `extractor`
/// 相同時 `version` 高的勝；③（`created`, `created_by`）較新的勝。三步都比不出來留本機的。
fn remote_wins(l: &TextFile, r: &TextFile, source: Option<&str>) -> bool {
    let current = |f: &TextFile| source == Some(f.extraction.source_sha256.as_str());
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

/// 合併一篇的抽出的文字。`source` 是合併後那篇原文的指紋；`path` 是那份 `text.json` 的路徑（`TooLargeToMerge`
/// 提醒指向它）。交回要寫的內容；不必寫是 `None`。讀不懂、`format` 較新的由呼叫的一方先擋下。
///
/// 只在本機有：不動。只在匯入那邊有：照複製。兩邊都有：整份取一邊（`remote_wins`）；`extraction` 整個取勝的
/// 一邊；兩邊抽自同一份原文時，輸的一邊每頁上不認得的欄位依頁序搬到勝的一邊（那頁沒有同名的鍵才加）；頂層不認得
/// 的欄位本機的留著、匯入那邊多的加進來。寫出會超過 24 MB 就不寫，本機的不動，提醒 `TooLargeToMerge`。
pub fn merge_text(
    local: Option<&TextFile>,
    remote: Option<&TextFile>,
    source: Option<&str>,
    pid: &str,
    path: &str,
    title: Option<&str>,
    ctx: &mut MergeCtx,
) -> Option<TextFile> {
    let (l, r) = match (local, remote) {
        (_, None) => return None,
        (None, Some(r)) => {
            if written_bytes(r) > MAX_WRITE_BYTES {
                ctx.notice(NoticeKind::TooLargeToMerge, Some(pid), Some(path), None);
                return None;
            }
            ctx.note(Change::Added, ItemKind::Text, Some(pid), title, 1);
            return Some(r.clone());
        }
        (Some(l), Some(r)) => (l, r),
    };
    let same_source = l.extraction.source_sha256 == r.extraction.source_sha256;
    let mut doc = if remote_wins(l, r, source) {
        let mut doc = r.clone();
        if same_source {
            carry_page_extras(&mut doc.pages, &l.pages);
        }
        doc.extra = l.extra.clone();
        doc
    } else {
        let mut doc = l.clone();
        if same_source {
            carry_page_extras(&mut doc.pages, &r.pages);
        }
        doc
    };
    for (k, v) in &r.extra {
        if !doc.extra.contains_key(k) {
            doc.extra.insert(k.clone(), v.clone());
        }
    }
    if doc == *l {
        return None;
    }
    if written_bytes(&doc) > MAX_WRITE_BYTES {
        ctx.notice(NoticeKind::TooLargeToMerge, Some(pid), Some(path), None);
        return None;
    }
    ctx.note(Change::Updated, ItemKind::Text, Some(pid), title, 1);
    Some(doc)
}
