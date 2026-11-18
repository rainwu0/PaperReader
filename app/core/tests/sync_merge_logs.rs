//! 紀錄檔與回收區位置的合併。純計算。

mod common;

use common::*;
use paperreader_core::changes::{PAPER_PURGED, PAPER_REMOVED, PAPER_RESTORED};
use paperreader_core::store::{ConflictChoice, ConflictKind};
use paperreader_core::sync::merge::logs::{LogMerge, merge_log};
use paperreader_core::sync::merge::trash::{Place, merge_place};
use serde_json::json;

fn line(id: &str) -> String {
    format!("{{\"id\":\"{id}\",\"t\":\"2026-11-01T00:00:00.000Z\"}}\n")
}

#[test]
fn absent_file_copied() {
    let r = line("a") + &line("b");
    assert_eq!(
        merge_log(None, r.as_bytes()),
        LogMerge::Append(r.into_bytes())
    );
}

#[test]
fn prefix_appends_suffix() {
    let l = line("a");
    let r = line("a") + &line("b");
    assert_eq!(
        merge_log(Some(l.as_bytes()), r.as_bytes()),
        LogMerge::Append(line("b").into_bytes())
    );
}

#[test]
fn local_longer_untouched_and_own_file_never_shrinks() {
    let l = line("a") + &line("b");
    let r = line("a");
    assert_eq!(
        merge_log(Some(l.as_bytes()), r.as_bytes()),
        LogMerge::Unchanged
    );
    assert_eq!(merge_log(Some(l.as_bytes()), b""), LogMerge::Unchanged);
}

#[test]
fn divergent_union_by_id_keeps_local_order() {
    let l = line("a") + &line("c");
    let r = line("a") + &line("b") + &line("c") + &line("d");
    assert_eq!(
        merge_log(Some(l.as_bytes()), r.as_bytes()),
        LogMerge::Append((line("b") + &line("d")).into_bytes())
    );
}

#[test]
fn unreadable_incoming_lines_dropped_local_kept() {
    let l = line("a") + "{broken local\n";
    let r = line("a") + "{broken remote\n" + &line("z");
    assert_eq!(
        merge_log(Some(l.as_bytes()), r.as_bytes()),
        LogMerge::Append(line("z").into_bytes())
    );
}

fn place(
    local: Place,
    remote: Place,
    recs: &[paperreader_core::changes::ChangeRecord],
    choice: Option<ConflictChoice>,
) -> (Place, paperreader_core::sync::merge::MergeCtx) {
    let choices: Vec<_> = choice
        .map(|c| vec![(ConflictKind::TrashState, Some("P"), None, c)])
        .unwrap_or_default();
    let mut ctx = mctx(&choices);
    let p = merge_place(
        "P",
        None,
        local,
        remote,
        recs,
        "20261201T000000Z-P",
        &mut ctx,
    );
    (p, ctx)
}

#[test]
fn latest_record_wins_remove_vs_restore() {
    let recs = [
        rec(
            "01A00000000000000000000001",
            "2026-11-01T00:00:00.000Z",
            DEVICE,
            Some("P"),
            PAPER_REMOVED,
            json!({"folder": "x"}),
        ),
        rec(
            "01A00000000000000000000002",
            "2026-11-02T00:00:00.000Z",
            DEVICE_B,
            Some("P"),
            PAPER_RESTORED,
            json!({}),
        ),
    ];
    assert_eq!(
        place(Place::Trash, Place::Trash, &recs, None).0,
        Place::Papers
    );
    assert_eq!(
        place(Place::Papers, Place::Trash, &recs[..1], None).0,
        Place::Trash
    );
}

#[test]
fn no_record_same_place_and_one_side_absent() {
    assert_eq!(place(Place::Trash, Place::Trash, &[], None).0, Place::Trash);
    assert_eq!(
        place(Place::Absent, Place::Trash, &[], None).0,
        Place::Trash
    );
    assert_eq!(
        place(Place::Papers, Place::Absent, &[], None).0,
        Place::Papers
    );
}

#[test]
fn no_record_trash_vs_active_conflict_and_resolution_writes_record() {
    let (p, ctx) = place(Place::Trash, Place::Papers, &[], None);
    assert_eq!(ctx.conflicts.len(), 1);
    assert_eq!(
        ctx.conflicts[0].1.default,
        ConflictChoice::Incoming,
        "預設是不在回收區的那一邊"
    );
    assert_eq!(p, Place::Papers);
    assert_eq!(ctx.records.len(), 1);
    assert_eq!(ctx.records[0].kind, PAPER_RESTORED);
    let (p, ctx) = place(
        Place::Trash,
        Place::Papers,
        &[],
        Some(ConflictChoice::Local),
    );
    assert_eq!(p, Place::Trash);
    assert_eq!(ctx.records[0].kind, PAPER_REMOVED);
    assert_eq!(ctx.records[0].data["folder"], json!("20261201T000000Z-P"));
    // 寫了記錄之後，反向合併不再衝突
    let (p2, ctx2) = place(Place::Papers, Place::Trash, &ctx.records, None);
    assert_eq!(p2, Place::Trash);
    assert!(ctx2.conflicts.is_empty());
}

#[test]
fn purged_record_ignored() {
    let recs = [rec(
        "01A00000000000000000000003",
        "2026-11-03T00:00:00.000Z",
        DEVICE,
        Some("P"),
        PAPER_PURGED,
        json!({}),
    )];
    assert_eq!(
        place(Place::Papers, Place::Papers, &recs, None).0,
        Place::Papers
    );
}
