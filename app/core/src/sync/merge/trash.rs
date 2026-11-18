//! 回收區的位置與永久刪除。

use super::{MergeCtx, side};
use crate::changes::{ChangeRecord, PAPER_PURGED, PAPER_REMOVED, PAPER_RESTORED};
use crate::store::{ConflictChoice, ConflictKind};
use serde::{Deserialize, Serialize};

/// 一篇文獻在一邊的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    Papers,
    Trash,
    Absent,
}

/// 這篇最晚的一筆 `kind` 記錄。
fn latest_of<'a>(records: &'a [ChangeRecord], paper: &str, kind: &str) -> Option<&'a ChangeRecord> {
    records
        .iter()
        .filter(|r| r.kind == kind && r.paper.as_deref() == Some(paper))
        .max_by(|a, b| a.order_key().cmp(&b.order_key()))
}

/// 合併後的紀錄裡這篇有 `paper-purged`，而且最晚的那一筆之後沒有 `paper-restored`：
/// 回收區裡的那篇從兩邊拿掉、不帶回；在任一邊目錄裡的不刪（由呼叫端處理）。只有明確的放回抵銷永久刪除；
/// 之後的移除（另一台還沒同步就把它移到回收區）不抵銷。
pub fn purged_last(records: &[ChangeRecord], paper: &str) -> bool {
    latest_of(records, paper, PAPER_PURGED).is_some()
        && restored_after_purge(records, paper).is_none()
}

/// 永久刪除之後又有人放回：交回（最晚的那筆永久刪除, 它之後最晚的放回）。
pub fn restored_after_purge<'a>(
    records: &'a [ChangeRecord],
    paper: &str,
) -> Option<(&'a ChangeRecord, &'a ChangeRecord)> {
    let purge = latest_of(records, paper, PAPER_PURGED)?;
    let restore = latest_of(records, paper, PAPER_RESTORED)?;
    (restore.order_key() > purge.order_key()).then_some((purge, restore))
}

/// 合併後的紀錄檔裡這篇最晚的移除或放回（永久刪除另外看 `purged_last`）。
pub fn latest_place_record<'a>(
    records: &'a [ChangeRecord],
    paper: &str,
) -> Option<&'a ChangeRecord> {
    records
        .iter()
        .filter(|r| {
            (r.kind == PAPER_REMOVED || r.kind == PAPER_RESTORED)
                && r.paper.as_deref() == Some(paper)
        })
        .max_by(|a, b| a.order_key().cmp(&b.order_key()))
}

/// 合併後的位置。`trash_folder` 是解決 `TrashState` 衝突而選「回收區」時寫進記錄的資料夾名稱。
pub fn merge_place(
    paper: &str,
    title: Option<&str>,
    local: Place,
    remote: Place,
    records: &[ChangeRecord],
    trash_folder: &str,
    ctx: &mut MergeCtx,
) -> Place {
    if let Some(r) = latest_place_record(records, paper) {
        return if r.kind == PAPER_REMOVED {
            Place::Trash
        } else {
            Place::Papers
        };
    }
    match (local, remote) {
        (a, b) if a == b => a,
        (Place::Absent, b) => b,
        (a, Place::Absent) => a,
        (a, b) => {
            let default = if a == Place::Papers {
                ConflictChoice::Local
            } else {
                ConflictChoice::Incoming
            };
            let choice = ctx.conflict(
                ConflictKind::TrashState,
                Some(paper),
                title,
                None,
                side(true, None, None, Some(place_word(a))),
                side(true, None, None, Some(place_word(b))),
                vec![ConflictChoice::Local, ConflictChoice::Incoming],
                default,
                1,
            );
            let chosen = if choice == ConflictChoice::Incoming {
                b
            } else {
                a
            };
            if chosen == Place::Trash {
                ctx.record(
                    Some(paper),
                    PAPER_REMOVED,
                    serde_json::json!({ "folder": trash_folder }),
                );
            } else {
                ctx.record(Some(paper), PAPER_RESTORED, serde_json::json!({}));
            }
            chosen
        }
    }
}

fn place_word(p: Place) -> &'static str {
    match p {
        Place::Papers => "papers",
        Place::Trash => "trash",
        Place::Absent => "absent",
    }
}
