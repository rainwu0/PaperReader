//! 匯出備份檔與同步包時打不開的檔：一般的檔記為略過（`unreadable`）、
//! 不放進 zip 與清單，匯出照樣完成；`library.json` 打不開則整個失敗、不留下任何檔。
//!
//! 在 Windows 上以獨占方式開著那個檔來模擬「打不開」（列檔時讀得到大小，打開來讀時被拒）。
#![cfg(windows)]

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::store::SyncMode;
use std::fs;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

/// 獨占開著（不讓別人讀、寫、刪）；放開之前別人都打不開它。
fn lock(path: &Path) -> fs::File {
    let f = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap();
    // 列檔那一步仍讀得到大小與修改時間：略過是在打開來讀的時候發生的
    assert!(fs::metadata(path).unwrap().is_file());
    assert!(fs::File::open(path).is_err());
    f
}

const LOCKED: &str = "papers/Beta Paper/source.pdf";

/// 打不開 `library.json` 的錯誤：`Io`，路徑是那個 `library.json`（不是寫完之後「清單裡沒有它」的 `Unreadable`）。
fn assert_library_json_io<T: std::fmt::Debug>(r: &Result<T, LibError>) {
    match r {
        Err(LibError::Io { path, .. }) if path.ends_with("library.json") => {}
        other => panic!("expected Io on library.json, got {other:?}"),
    }
}

fn names(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn backup_skips_a_file_it_cannot_open() {
    let (dir, lib) = build_library();
    let _hold = lock(&dir.path().join(LOCKED));
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    let report = lib.export_backup(&out, &mut |_, _| {}).unwrap();
    let skipped: Vec<(String, String)> = report
        .skipped
        .iter()
        .map(|s| (s.path.clone(), s.reason.clone()))
        .collect();
    assert_eq!(skipped, [(LOCKED.to_string(), "unreadable".to_string())]);
    let entries = zip_entries(&out);
    assert!(
        entries.iter().all(|(n, _, _)| !n.ends_with(LOCKED)),
        "{:?}",
        entries.iter().map(|e| &e.0).collect::<Vec<_>>()
    );
    let (name, manifest, _) = entries.last().unwrap();
    assert_eq!(name, "manifest.json");
    let m: serde_json::Value = serde_json::from_slice(manifest).unwrap();
    let files = m["files"].as_array().unwrap();
    assert_eq!(files.len(), report.files);
    assert!(files.iter().all(|f| f["path"] != LOCKED));
    assert_eq!(names(out_dir.path()), ["b.zip"]);
}

#[test]
fn backup_fails_when_library_json_cannot_be_opened() {
    let (dir, lib) = build_library();
    let _hold = lock(&dir.path().join("library.json"));
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    let r = lib.export_backup(&out, &mut |_, _| {});
    assert_library_json_io(&r);
    assert!(names(out_dir.path()).is_empty());
}

#[test]
fn sync_package_skips_a_file_it_cannot_open() {
    let (dir, mut lib) = build_library();
    let _hold = lock(&dir.path().join(LOCKED));
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("s.zip");
    let report = lib
        .export_sync_package(&out, SyncMode::Full, None, &mut |_, _| {})
        .unwrap();
    let skipped: Vec<(String, String)> = report
        .skipped
        .iter()
        .map(|s| (s.path.clone(), s.reason.clone()))
        .collect();
    assert_eq!(skipped, [(LOCKED.to_string(), "unreadable".to_string())]);
    let entries = zip_entries(&out);
    assert!(entries.iter().all(|(n, _, _)| !n.ends_with(LOCKED)));
    let (name, manifest, _) = entries.last().unwrap();
    assert_eq!(name, "manifest.json");
    let m: serde_json::Value = serde_json::from_slice(manifest).unwrap();
    let files = m["files"].as_array().unwrap();
    assert_eq!(files.len(), report.listed);
    assert!(files.iter().all(|f| f["path"] != LOCKED));
    assert_eq!(names(out_dir.path()), ["s.zip"]);
}

#[test]
fn sync_package_fails_when_library_json_cannot_be_opened() {
    let (dir, mut lib) = build_library();
    let _hold = lock(&dir.path().join("library.json"));
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("s.zip");
    let r = lib.export_sync_package(&out, SyncMode::Full, None, &mut |_, _| {});
    assert_library_json_io(&r);
    assert!(names(out_dir.path()).is_empty());
}
