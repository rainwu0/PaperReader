//! `id` 以外的欄位缺少或寫成 `null`，不算讀不懂；當作「不知道」。

mod common;

use common::*;
use paperreader_core::format::PaperFile;
use paperreader_core::jsonio::{read_json, to_canonical_json};
use paperreader_core::local::LocalLibrary;
use serde_json::{Value, json};
use std::fs;

const ID: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

fn base() -> Value {
    serde_json::to_value(sample_paper(ID)).unwrap()
}

/// 把 `v` 寫成 `papers/A/paper.json`，開啟，回傳（暫存資料夾、文獻庫、磁碟上讀回的描述檔）。
fn load(v: &Value) -> (tempfile::TempDir, LocalLibrary, PaperFile) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("papers/A")).unwrap();
    let path = dir.path().join("papers/A/paper.json");
    fs::write(&path, serde_json::to_string_pretty(v).unwrap() + "\n").unwrap();
    let lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let (p, _): (PaperFile, _) = read_json(&path).unwrap();
    (dir, lib, p)
}

fn assert_listed_and_clean(v: &Value) -> PaperFile {
    let (_dir, lib, paper) = load(v);
    assert_eq!(lib.list_papers().len(), 1, "必須列入目錄");
    // 讀入再寫回後，沒有補成空字串的欄位
    let out = to_canonical_json(&paper).unwrap();
    assert!(!out.contains("\"\""), "{out}");
    paper
}

#[test]
fn added_null() {
    let mut v = base();
    v["added"] = Value::Null;
    assert_eq!(assert_listed_and_clean(&v).added, None);
}

#[test]
fn added_missing() {
    let mut v = base();
    v.as_object_mut().unwrap().remove("added");
    assert_eq!(assert_listed_and_clean(&v).added, None);
}

#[test]
fn aliases_null() {
    let mut v = base();
    v["aliases"] = Value::Null;
    assert!(assert_listed_and_clean(&v).aliases.is_empty());
}

#[test]
fn authors_null() {
    let mut v = base();
    v["bib"]["authors"] = Value::Null;
    assert!(assert_listed_and_clean(&v).bib.authors.is_empty());
}

#[test]
fn bib_missing() {
    let mut v = base();
    v.as_object_mut().unwrap().remove("bib");
    let p = assert_listed_and_clean(&v);
    assert_eq!(p.bib.title, None);
    assert_eq!(p.bib.modified, None);
    assert_eq!(p.bib.modified_by, None);
}

#[test]
fn bib_null() {
    let mut v = base();
    v["bib"] = Value::Null;
    let p = assert_listed_and_clean(&v);
    assert!(p.bib.authors.is_empty());
    assert_eq!(p.bib.year, None);
}

#[test]
fn bib_modified_fields_missing_or_null() {
    let mut v = base();
    v["bib"].as_object_mut().unwrap().remove("modified");
    v["bib"]["modified_by"] = Value::Null;
    let p = assert_listed_and_clean(&v);
    assert_eq!(p.bib.modified, None);
    assert_eq!(p.bib.modified_by, None);
}

#[test]
fn materials_missing() {
    let mut v = base();
    v.as_object_mut().unwrap().remove("materials");
    assert!(assert_listed_and_clean(&v).materials.is_empty());
}

#[test]
fn materials_null() {
    let mut v = base();
    v["materials"] = Value::Null;
    assert!(assert_listed_and_clean(&v).materials.is_empty());
}

#[test]
fn provenance_missing_and_null() {
    let mut v = base();
    v["materials"][0]
        .as_object_mut()
        .unwrap()
        .remove("provenance");
    let p = assert_listed_and_clean(&v);
    assert_eq!(p.materials[0].provenance.method, None);

    let mut v = base();
    v["materials"][0]["provenance"] = Value::Null;
    let p = assert_listed_and_clean(&v);
    assert_eq!(p.materials[0].provenance.time, None);
}

#[test]
fn format_null_is_current_format() {
    let mut v = base();
    v["format"] = Value::Null;
    let (_dir, lib, p) = load(&v);
    assert_eq!(p.format, 1);
    assert!(!lib.list_papers()[0].needs_newer_app);
}

#[test]
fn still_unreadable_cases() {
    // 缺 id、型別不對（年份是字串）、不是合法 JSON：仍然不列入目錄
    let mut no_id = base();
    no_id.as_object_mut().unwrap().remove("id");
    let mut bad_year = base();
    bad_year["bib"]["year"] = json!("2024");
    let mut null_id = base();
    null_id["id"] = Value::Null;
    for v in [no_id, bad_year, null_id] {
        let (_dir, lib, _) = {
            let dir = tempfile::tempdir().unwrap();
            fs::create_dir_all(dir.path().join("papers/A")).unwrap();
            fs::write(dir.path().join("papers/A/paper.json"), v.to_string()).unwrap();
            let lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
            (dir, lib, ())
        };
        assert!(lib.list_papers().is_empty(), "{v}");
    }
}
