//! 第二台裝置從同步包加入。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::SyncMode;
use paperreader_core::sync::join::join_from_sync_package;
use std::fs;

fn sorted_titles(lib: &LocalLibrary) -> Vec<(String, Option<String>)> {
    let mut v: Vec<_> = lib
        .list_papers()
        .into_iter()
        .map(|p| (p.id, p.bib.title))
        .collect();
    v.sort();
    v
}

#[test]
fn join_from_full_package() {
    let (dir, mut a) = build_library();
    fs::write(dir.path().join("papers/Alpha Paper/mine.txt"), "x").unwrap();
    let out = tempfile::tempdir().unwrap();
    let pkg = out.path().join("full.zip");
    a.export_sync_package(&pkg, SyncMode::Full, None, &mut |_, _| {})
        .unwrap();
    let target = out.path().join("joined");
    let report = join_from_sync_package(&pkg, &target, DEVICE_B).unwrap();
    assert_eq!(report.library_id, a.library_file().id);
    assert!(report.manifest_mismatches.is_empty());
    let b = LocalLibrary::open(&target, DEVICE_B).unwrap();
    assert_eq!(b.library_file().id, a.library_file().id);
    assert_eq!(sorted_titles(&a), sorted_titles(&b));
    assert_eq!(a.list_groups().unwrap().0, b.list_groups().unwrap().0);
    for p in a.list_papers() {
        let mut na = a.read_notes(&p.id).unwrap().0;
        let mut nb = b.read_notes(&p.id).unwrap().0;
        na.path = None;
        nb.path = None;
        assert_eq!(na, nb);
    }
    assert!(
        !target.join("papers/Alpha Paper/mine.txt").exists(),
        "使用者自己放的檔不在"
    );
}

#[test]
fn join_refuses_changes_package() {
    let mut p = pair();
    // 讓 A 的下一份是只帶變更
    sync_once(&mut p.a, &mut p.b, p.out.path());
    sync_once(&mut p.b, &mut p.a, p.out.path());
    let pkg = p.out.path().join("c.zip");
    let r =
        p.a.export_sync_package(&pkg, SyncMode::Changes, None, &mut |_, _| {})
            .unwrap();
    assert_eq!(r.mode, SyncMode::Changes);
    let target = p.out.path().join("joined");
    match join_from_sync_package(&pkg, &target, DEVICE_B) {
        Err(LibError::InvalidInput { reason }) => {
            assert!(reason.starts_with("package is not complete"))
        }
        other => panic!("{other:?}"),
    }
    assert!(!target.exists());
}

#[test]
fn join_refuses_non_empty_target_and_invalid_device() {
    let (_dir, mut a) = build_library();
    let out = tempfile::tempdir().unwrap();
    let pkg = out.path().join("full.zip");
    a.export_sync_package(&pkg, SyncMode::Full, None, &mut |_, _| {})
        .unwrap();
    let target = out.path().join("busy");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("x"), "x").unwrap();
    assert!(matches!(
        join_from_sync_package(&pkg, &target, DEVICE_B),
        Err(LibError::InvalidInput { .. })
    ));
    let fresh = out.path().join("fresh");
    assert!(matches!(
        join_from_sync_package(&pkg, &fresh, "bad id"),
        Err(LibError::InvalidInput { .. })
    ));
    assert!(!fresh.exists());
}

#[test]
fn join_rejects_backup_file() {
    let (_dir, a) = build_library();
    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("b.zip");
    a.export_backup(&zip, &mut |_, _| {}).unwrap();
    match join_from_sync_package(&zip, &out.path().join("t"), DEVICE_B) {
        Err(LibError::Unreadable { reason, .. }) => assert!(reason.starts_with("backup file")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn join_then_sync_both_ways() {
    let (_dir, mut a) = build_library();
    let out = tempfile::tempdir().unwrap();
    let pkg = out.path().join("full.zip");
    a.export_sync_package(&pkg, SyncMode::Full, None, &mut |_, _| {})
        .unwrap();
    let target = out.path().join("joined");
    join_from_sync_package(&pkg, &target, DEVICE_B).unwrap();
    let mut b = LocalLibrary::open(&target, DEVICE_B).unwrap();
    add_paper(&mut a, 61, "A after join", None);
    add_paper(&mut b, 62, "B after join", None);
    sync_once(&mut a, &mut b, out.path());
    sync_once(&mut b, &mut a, out.path());
    assert_eq!(sorted_titles(&a), sorted_titles(&b));
}

#[test]
fn restore_backup_then_sync_both_ways() {
    let mut p = pair();
    add_paper(&mut p.a, 63, "A side", None);
    add_paper(&mut p.b, 64, "B side", None);
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    sync_once(&mut p.b, &mut p.a, &out);
    assert_eq!(sorted_titles(&p.a), sorted_titles(&p.b));
}
