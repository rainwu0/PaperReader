//! `ink.json` 與譯文疊加檔的合併。

use super::{Change, MergeCtx, cmp_stamp, side};
use crate::ink::InkFile;
use crate::overlay::{OverlayFile, OverlaySegment, STATUS_DONE, STATUS_FAILED};
use crate::store::{ConflictChoice, ConflictKind, ItemKind, NoticeKind};
use std::cmp::Ordering;
use std::collections::HashSet;

/// 筆跡：依代號取聯集，拿掉合併後 `strokes-deleted` 裡的每一個代號。交回要寫的內容；不必寫是 `None`。
pub fn merge_ink(
    local: Option<&InkFile>,
    remote: Option<&InkFile>,
    deleted: &HashSet<String>,
    pid: &str,
    title: Option<&str>,
    ctx: &mut MergeCtx,
) -> Option<InkFile> {
    match (local, remote) {
        (None, None) => None,
        (None, Some(r)) => {
            // 只在匯入那邊：照原樣複製（拿掉已刪除的筆跡後）
            let mut doc = r.clone();
            doc.strokes.retain(|s| !deleted.contains(&s.id));
            ctx.note(
                Change::Added,
                ItemKind::Stroke,
                Some(pid),
                title,
                doc.strokes.len(),
            );
            Some(doc)
        }
        (Some(l), r) => {
            let mut doc = l.clone();
            let mut added = 0;
            if let Some(r) = r {
                let have: HashSet<&str> = l.strokes.iter().map(|s| s.id.as_str()).collect();
                for s in &r.strokes {
                    if !have.contains(s.id.as_str()) && !deleted.contains(&s.id) {
                        doc.strokes.push(s.clone());
                        added += 1;
                    }
                }
                for (k, v) in &r.extra {
                    if !doc.extra.contains_key(k) {
                        doc.extra.insert(k.clone(), v.clone());
                    }
                }
            }
            let before = doc.strokes.len();
            doc.strokes.retain(|s| !deleted.contains(&s.id));
            let removed = before - doc.strokes.len();
            if doc.strokes == l.strokes && doc.extra == l.extra {
                return None;
            }
            ctx.note(Change::Added, ItemKind::Stroke, Some(pid), title, added);
            ctx.note(Change::Removed, ItemKind::Stroke, Some(pid), title, removed);
            doc.modified = Some(ctx.now.clone());
            doc.modified_by = Some(ctx.device.clone());
            Some(doc)
        }
    }
}

fn rank(status: &str) -> u8 {
    match status {
        STATUS_DONE => 2,
        STATUS_FAILED => 1,
        _ => 0,
    }
}

/// 一段的內容是否相同（不看版面與原文：建立後不變）。
fn same_segment(a: &OverlaySegment, b: &OverlaySegment) -> bool {
    a.target == b.target
        && a.status == b.status
        && a.error == b.error
        && a.issues == b.issues
        && a.extra == b.extra
        && a.modified == b.modified
        && a.modified_by == b.modified_by
        && a.service == b.service
        && a.by_user == b.by_user
}

/// 勝出的一段整段帶過來：譯文、狀態、錯誤、問題代號、翻它的服務、使用者決定的標記、
/// 不認得的欄位與時間。
fn take(dst: &mut OverlaySegment, src: &OverlaySegment) {
    dst.target = src.target.clone();
    dst.status = src.status.clone();
    dst.error = src.error.clone();
    dst.issues = src.issues.clone();
    dst.service = src.service.clone();
    dst.by_user = src.by_user;
    dst.extra = src.extra.clone();
    dst.modified = src.modified.clone();
    dst.modified_by = src.modified_by.clone();
}

/// 譯文疊加檔：頭的欄位留本機的；段落依代號逐段比。交回要寫的內容；不必寫是 `None`。
pub fn merge_overlay(
    local: &OverlayFile,
    remote: &OverlayFile,
    pid: &str,
    material: &str,
    path: &str,
    title: Option<&str>,
    ctx: &mut MergeCtx,
) -> Option<OverlayFile> {
    let lids: Vec<&str> = local.segments.iter().map(|s| s.id.as_str()).collect();
    let rids: Vec<&str> = remote.segments.iter().map(|s| s.id.as_str()).collect();
    if lids != rids {
        ctx.notice(NoticeKind::IncomingUnreadable, Some(pid), Some(path), None);
        return None;
    }
    let mut out = local.clone();
    let mut changed = 0usize;
    let mut contested: Vec<usize> = Vec::new();
    for (i, (l, r)) in local.segments.iter().zip(&remote.segments).enumerate() {
        if same_segment(l, r) {
            continue;
        }
        // 只有一邊是使用者挑的：挑的那一段勝出，不看時間；整批的翻譯不蓋掉它，同步帶來的也一樣
        let remote_wins = match (&l.modified, &r.modified) {
            _ if l.by_user != r.by_user => r.by_user,
            (Some(_), Some(_)) => {
                cmp_stamp(
                    r.modified.as_deref(),
                    r.modified_by.as_deref(),
                    l.modified.as_deref(),
                    l.modified_by.as_deref(),
                ) == Ordering::Greater
            }
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => {
                let (rl, rr) = (rank(&l.status), rank(&r.status));
                if rl == 2 && rr == 2 && l.target != r.target {
                    contested.push(i);
                    false
                } else {
                    rr > rl
                }
            }
        };
        if remote_wins {
            take(&mut out.segments[i], r);
            changed += 1;
        }
    }
    if !contested.is_empty() {
        let choice = ctx.conflict(
            ConflictKind::Overlay,
            Some(pid),
            title,
            Some(material),
            side(
                true,
                local.modified.as_deref(),
                local.modified_by.as_deref(),
                None,
            ),
            side(
                true,
                remote.modified.as_deref(),
                remote.modified_by.as_deref(),
                None,
            ),
            vec![ConflictChoice::Local, ConflictChoice::Incoming],
            ConflictChoice::Local,
            contested.len(),
        );
        for i in contested {
            if choice == ConflictChoice::Incoming {
                take(&mut out.segments[i], &remote.segments[i]);
            }
            out.segments[i].modified = Some(ctx.now.clone());
            out.segments[i].modified_by = Some(ctx.device.clone());
            changed += 1;
        }
    }
    // 這份譯本的術語：依原文詞合併，使用者的決定勝過學到的（`overlay::merge_terms`）
    let terms = crate::overlay::merge_terms(&local.terms, &remote.terms);
    if terms != local.terms {
        out.terms = terms;
        changed += 1;
    }
    if changed == 0 {
        return None;
    }
    ctx.note(Change::Updated, ItemKind::Overlay, Some(pid), title, 1);
    out.modified = Some(ctx.now.clone());
    out.modified_by = Some(ctx.device.clone());
    Some(out)
}
