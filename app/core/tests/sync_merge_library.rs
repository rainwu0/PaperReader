//! `library.json` 的合併。純計算。

mod common;

use common::*;
use paperreader_core::format::LibraryFile;
use paperreader_core::store::NoticeKind;
use paperreader_core::sync::merge::library::merge_library;
use serde_json::{Value, json};

fn lib(groups: Value, deleted: Value) -> LibraryFile {
    LibraryFile {
        format: 1,
        id: "01JCK3V7Q8Z9X5M2N4P6R8T0AB".into(),
        created: "2026-11-01T00:00:00.000Z".into(),
        notes_dir: "notes".into(),
        groups,
        deleted_groups: deleted,
        extra: Default::default(),
    }
}

fn g(id: &str, name: &str, t: &str, by: &str) -> Value {
    json!({"id": id, "name": name, "created": "2026-11-01T00:00:00.000Z", "modified": t, "modified_by": by})
}

#[test]
fn group_newer_rename_wins() {
    let l = lib(
        json!([g("G1", "Old", "2026-11-02T00:00:00.000Z", DEVICE_B)]),
        json!([]),
    );
    let r = lib(
        json!([g("G1", "New", "2026-11-03T00:00:00.000Z", DEVICE)]),
        json!([]),
    );
    let mut ctx = mctx(&[]);
    let m = merge_library(&l, &r, &mut ctx);
    assert!(m.changed);
    assert_eq!(m.file.groups[0]["name"], json!("New"));
    // 反過來：本機較新
    let mut ctx = mctx(&[]);
    let m = merge_library(&r, &l, &mut ctx);
    assert!(!m.changed);
}

#[test]
fn group_same_time_keeps_local() {
    let t = "2026-11-02T00:00:00.000Z";
    let l = lib(json!([g("G1", "Local", t, DEVICE)]), json!([]));
    let r = lib(json!([g("G1", "Remote", t, DEVICE)]), json!([]));
    let mut ctx = mctx(&[]);
    let m = merge_library(&l, &r, &mut ctx);
    assert!(!m.changed);
    assert!(ctx.conflicts.is_empty());
}

#[test]
fn deleted_groups_union_and_deletion_beats_rename() {
    let l = lib(
        json!([
            g("G1", "One", "2026-11-05T00:00:00.000Z", DEVICE_B),
            g("G2", "Two", "2026-11-01T00:00:00.000Z", DEVICE_B)
        ]),
        json!([{"id": "G9", "deleted": "2026-11-01T00:00:00.000Z", "deleted_by": DEVICE_B}]),
    );
    let r = lib(
        json!([g("G2", "Two", "2026-11-01T00:00:00.000Z", DEVICE_B)]),
        json!([{"id": "G1", "deleted": "2026-11-04T00:00:00.000Z", "deleted_by": DEVICE}]),
    );
    let mut ctx = mctx(&[]);
    let m = merge_library(&l, &r, &mut ctx);
    let ids: Vec<&str> = m
        .file
        .groups
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["G2"], "刪除勝過改名（即使改名較晚）");
    let del: Vec<&str> = m
        .file
        .deleted_groups
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap())
        .collect();
    assert_eq!(del, ["G9", "G1"]);
    assert!(m.deleted.unwrap().contains("G1"));
}

#[test]
fn same_name_groups_both_kept_with_notice() {
    let l = lib(
        json!([g("G1", "Vision", "2026-11-01T00:00:00.000Z", DEVICE_B)]),
        json!([]),
    );
    let r = lib(
        json!([g("G2", "vision", "2026-11-01T00:00:00.000Z", DEVICE)]),
        json!([]),
    );
    let mut ctx = mctx(&[]);
    let m = merge_library(&l, &r, &mut ctx);
    assert_eq!(m.file.groups.as_array().unwrap().len(), 2);
    assert!(
        ctx.notices
            .iter()
            .any(|n| n.kind == NoticeKind::SameGroupName)
    );
}

#[test]
fn unreadable_incoming_groups_not_merged() {
    let l = lib(
        json!([g("G1", "One", "2026-11-01T00:00:00.000Z", DEVICE_B)]),
        json!([]),
    );
    let r = lib(json!({"not": "an array"}), json!([]));
    let mut ctx = mctx(&[]);
    let m = merge_library(&l, &r, &mut ctx);
    assert!(!m.changed);
    assert!(m.deleted.is_none());
    assert!(
        ctx.notices
            .iter()
            .any(|n| n.kind == NoticeKind::IncomingUnreadable)
    );
}

#[test]
fn unreadable_local_groups_never_overwritten() {
    let l = lib(json!([{"name": "no id"}]), json!([]));
    let r = lib(
        json!([g("G1", "One", "2026-11-01T00:00:00.000Z", DEVICE)]),
        json!([]),
    );
    let mut ctx = mctx(&[]);
    let m = merge_library(&l, &r, &mut ctx);
    assert_eq!(m.file.groups, l.groups);
    assert!(
        ctx.notices
            .iter()
            .any(|n| n.kind == NoticeKind::LocalUnreadable)
    );
}

#[test]
fn unknown_fields_local_kept_incoming_added() {
    let mut l = lib(json!([]), json!([]));
    l.extra.insert("x".into(), json!("local"));
    let mut r = lib(json!([]), json!([]));
    r.extra.insert("x".into(), json!("remote"));
    r.extra.insert("y".into(), json!(1));
    let mut ctx = mctx(&[]);
    let m = merge_library(&l, &r, &mut ctx);
    assert_eq!(m.file.extra["x"], json!("local"));
    assert_eq!(m.file.extra["y"], json!(1));
    assert!(m.changed);
}

#[test]
fn unchanged_library_not_rewritten() {
    let l = lib(
        json!([g("G1", "One", "2026-11-01T00:00:00.000Z", DEVICE_B)]),
        json!([]),
    );
    let mut ctx = mctx(&[]);
    let m = merge_library(&l, &l.clone(), &mut ctx);
    assert!(!m.changed);
}
