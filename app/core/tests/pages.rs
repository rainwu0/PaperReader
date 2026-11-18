//! 記下總頁數 `set_material_pages`。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::local::LocalLibrary;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn paper_json_path(dir: &tempfile::TempDir, folder: &str) -> PathBuf {
    dir.path().join("papers").join(folder).join("paper.json")
}

fn read_value(p: &PathBuf) -> Value {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}

/// 把 `paper.json` 讀成 JSON 值，排除那項材料的 pages 三個欄位。
fn without_pages_fields(mut v: Value) -> Value {
    for m in v["materials"].as_array_mut().unwrap() {
        let o = m.as_object_mut().unwrap();
        o.remove("pages");
        o.remove("pages_modified");
        o.remove("pages_modified_by");
    }
    v
}

fn setup() -> (tempfile::TempDir, LocalLibrary, String, String, PathBuf) {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Paper", None);
    let path = paper_json_path(&dir, "Paper");
    // 手動加一個不認得的欄位，驗證原樣保留
    let mut v = read_value(&path);
    v["x_future"] = serde_json::json!(1);
    fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    lib.rescan().unwrap();
    (dir, lib, p, m, path)
}

#[test]
fn set_pages_writes_when_unknown() {
    let (_dir, mut lib, p, m, path) = setup();
    let before = read_value(&path);
    assert!(lib.set_material_pages(&p, &m, 14).unwrap());
    let after = read_value(&path);
    let mat = &after["materials"][0];
    assert_eq!(mat["pages"], 14);
    assert_eq!(mat["pages_modified_by"], DEVICE);
    assert!(mat["pages_modified"].as_str().unwrap().ends_with('Z'));
    assert_eq!(before["bib"]["modified"], after["bib"]["modified"]);
    assert_eq!(after["x_future"], 1);
    assert_eq!(without_pages_fields(before), without_pages_fields(after));
}

#[test]
fn set_pages_same_value_does_not_write() {
    let (_dir, mut lib, p, m, path) = setup();
    assert!(lib.set_material_pages(&p, &m, 14).unwrap());
    let bytes = fs::read(&path).unwrap();
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(!lib.set_material_pages(&p, &m, 14).unwrap());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), mtime);
}

#[test]
fn set_pages_overwrites_different_value() {
    let (_dir, mut lib, p, m, path) = setup();
    lib.set_material_pages(&p, &m, 14).unwrap();
    let first = read_value(&path)["materials"][0]["pages_modified"].clone();
    assert!(lib.set_material_pages(&p, &m, 20).unwrap());
    let v = read_value(&path);
    assert_eq!(v["materials"][0]["pages"], 20);
    assert!(v["materials"][0]["pages_modified"].as_str() >= first.as_str());
}

#[test]
fn set_pages_range() {
    let (_dir, mut lib, p, m, path) = setup();
    let bytes = fs::read(&path).unwrap();
    for bad in [0u32, 100_001] {
        let r = lib.set_material_pages(&p, &m, bad);
        assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    }
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(lib.set_material_pages(&p, &m, 100_000).unwrap());
    assert!(lib.set_material_pages(&p, &m, 1).unwrap());
}

#[test]
fn set_pages_unknown_material_or_paper_is_not_found() {
    let (_dir, mut lib, p, m, path) = setup();
    let bytes = fs::read(&path).unwrap();
    let r = lib.set_material_pages(&p, "NOSUCHMATERIAL", 5);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    let r = lib.set_material_pages("01JCK3W2E4H6J8K0M2N4P6Q8RS", &m, 5);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn set_pages_paper_in_trash_is_not_found() {
    let (_dir, mut lib, p, m, _path) = setup();
    lib.remove_paper(&p).unwrap();
    let r = lib.set_material_pages(&p, &m, 5);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
}

#[test]
fn set_pages_newer_format_is_refused() {
    let (_dir, mut lib, p, m, path) = setup();
    let mut v = read_value(&path);
    v["format"] = serde_json::json!(2);
    fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    lib.rescan().unwrap();
    let bytes = fs::read(&path).unwrap();
    let r = lib.set_material_pages(&p, &m, 5);
    assert!(
        matches!(r, Err(LibError::FormatTooNew { found: 2 })),
        "{r:?}"
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn set_pages_read_only_is_refused() {
    let (dir, mut lib, p, m, _path) = setup();
    lib.set_material_pages(&p, &m, 14).unwrap();
    drop(lib);
    // 把 library.json 的 format 改成 2：整個文獻庫唯讀
    let lp = dir.path().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    v["format"] = serde_json::json!(2);
    fs::write(&lp, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    assert!(lib.is_read_only());
    // 值相同也先回報唯讀
    let r = lib.set_material_pages(&p, &m, 14);
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
}

#[test]
fn set_pages_visible_without_rescan() {
    let (_dir, mut lib, p, m, _path) = setup();
    lib.set_material_pages(&p, &m, 33).unwrap();
    let s = lib.list_papers().into_iter().find(|s| s.id == p).unwrap();
    assert_eq!(s.materials[0].pages, Some(33));
}

#[test]
fn set_pages_reads_fresh_file() {
    let (_dir, mut lib, p, m, path) = setup();
    // 由測試直接改寫篇名（不經核心）
    let mut v = read_value(&path);
    v["bib"]["title"] = serde_json::json!("Changed Outside");
    fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    assert!(lib.set_material_pages(&p, &m, 14).unwrap());
    let after = read_value(&path);
    assert_eq!(after["bib"]["title"], "Changed Outside");
    assert_eq!(after["materials"][0]["pages"], 14);
}
