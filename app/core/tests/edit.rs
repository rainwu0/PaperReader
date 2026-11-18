mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::*;
use paperreader_core::jsonio::read_json;
use paperreader_core::local::LocalLibrary;
use serde_json::{Value, json};
use std::fs;

const ID: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

fn new_draft(title: &str) -> BibDraft {
    BibDraft {
        title: Some(title.into()),
        authors: vec![Author {
            literal: Some("RT-DETR Team".into()),
            ..Default::default()
        }],
        year: Some(2025),
        venue: None,
        doi: Some("https://doi.org/10.1/ABC".into()),
        url: None,
    }
}

fn setup() -> (tempfile::TempDir, LocalLibrary) {
    let dir = tempfile::tempdir().unwrap();
    write_paper(dir.path(), "A", &sample_paper(ID));
    let lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    (dir, lib)
}

fn disk(dir: &tempfile::TempDir) -> PaperFile {
    read_json::<PaperFile>(&dir.path().join("papers/A/paper.json"))
        .unwrap()
        .0
}

#[test]
fn update_changes_bib_and_stamps() {
    let (dir, mut lib) = setup();
    let (_, token) = lib.get_paper(ID).unwrap();
    lib.update_bib(ID, new_draft("New title"), &token).unwrap();
    let p = disk(&dir);
    assert_eq!(p.bib.title.as_deref(), Some("New title"));
    assert_eq!(p.bib.year, Some(2025));
    assert_eq!(p.bib.authors[0].literal.as_deref(), Some("RT-DETR Team"));
    assert_eq!(p.bib.authors.len(), 1);
    assert_eq!(p.bib.venue, None);
    assert_eq!(p.bib.doi.as_deref(), Some("10.1/abc"));
    assert!(p.bib.modified.as_deref().unwrap() > "2026-11-13T08:30:11.045Z");
    assert_eq!(p.bib.modified_by.as_deref(), Some(DEVICE));
    // 目錄也更新了
    assert_eq!(lib.list_papers()[0].bib.title.as_deref(), Some("New title"));
}

#[test]
fn update_keeps_folder_name() {
    let (dir, mut lib) = setup();
    let (_, token) = lib.get_paper(ID).unwrap();
    lib.update_bib(ID, new_draft("Completely different"), &token)
        .unwrap();
    assert!(dir.path().join("papers/A/paper.json").exists());
    assert_eq!(lib.list_papers()[0].folder, "A");
}

#[test]
fn update_keeps_unknown_fields_and_materials() {
    let dir = tempfile::tempdir().unwrap();
    let mut v: Value = serde_json::to_value(sample_paper(ID)).unwrap();
    v["x_future"] = json!({"k": [1, 2]});
    v["bib"]["x_future"] = json!("b");
    v["materials"][0]["x_future"] = json!(7);
    v["short_name"] = json!("Zhao24");
    v["aliases"] = json!(["L001"]);
    fs::create_dir_all(dir.path().join("papers/A")).unwrap();
    fs::write(
        dir.path().join("papers/A/paper.json"),
        serde_json::to_string_pretty(&v).unwrap() + "\n",
    )
    .unwrap();
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let (_, token) = lib.get_paper(ID).unwrap();
    lib.update_bib(ID, new_draft("T"), &token).unwrap();

    let out: Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("papers/A/paper.json")).unwrap())
            .unwrap();
    assert_eq!(out["x_future"], v["x_future"]);
    assert_eq!(out["bib"]["x_future"], json!("b"));
    assert_eq!(out["materials"], v["materials"]);
    assert_eq!(out["short_name"], json!("Zhao24"));
    assert_eq!(out["aliases"], json!(["L001"]));
    assert_eq!(out["id"], json!(ID));
    assert_eq!(out["added"], v["added"]);
}

#[test]
fn stale_token_is_conflict() {
    let (dir, mut lib) = setup();
    let (_, token) = lib.get_paper(ID).unwrap();
    // 別的工具只改篇名，不動 bib.modified
    let mut external = sample_paper(ID);
    external.bib.title = Some("Edited by another tool".into());
    write_paper(dir.path(), "A", &external);
    let before = fs::read(dir.path().join("papers/A/paper.json")).unwrap();

    let r = lib.update_bib(ID, new_draft("Mine"), &token);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert_eq!(
        fs::read(dir.path().join("papers/A/paper.json")).unwrap(),
        before
    );
    assert_eq!(
        disk(&dir).bib.title.as_deref(),
        Some("Edited by another tool")
    );
}

#[test]
fn returned_token_allows_next_update() {
    let (dir, mut lib) = setup();
    let (_, t0) = lib.get_paper(ID).unwrap();
    let t1 = lib.update_bib(ID, new_draft("one"), &t0).unwrap();
    let t2 = lib.update_bib(ID, new_draft("two"), &t1).unwrap();
    assert_ne!(t1, t2);
    assert_eq!(disk(&dir).bib.title.as_deref(), Some("two"));
    let (_, now) = lib.get_paper(ID).unwrap();
    assert_eq!(now, t2);
    // 用已經過期的記號
    assert!(matches!(
        lib.update_bib(ID, new_draft("three"), &t1),
        Err(LibError::Conflict)
    ));
}

#[test]
fn update_in_read_only_library_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("library.json"),
        "{\"format\":2,\"id\":\"01JCK3V7Q8Z9X5M2N4P6R8T0AB\"}",
    )
    .unwrap();
    write_paper(dir.path(), "A", &sample_paper(ID));
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let (_, token) = lib.get_paper(ID).unwrap();
    let before = snapshot(dir.path());
    let r = lib.update_bib(ID, new_draft("x"), &token);
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn update_unknown_id_is_not_found() {
    let (_dir, mut lib) = setup();
    let (_, token) = lib.get_paper(ID).unwrap();
    let r = lib.update_bib("01ZZZZZZZZZZZZZZZZZZZZZZZZ", new_draft("x"), &token);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
}

#[test]
fn newer_paper_format_is_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = sample_paper(ID);
    p.format = 2;
    write_paper(dir.path(), "A", &p);
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let (_, token) = lib.get_paper(ID).unwrap();
    let before = snapshot(dir.path());
    let r = lib.update_bib(ID, new_draft("x"), &token);
    assert!(
        matches!(r, Err(LibError::FormatTooNew { found: 2 })),
        "{r:?}"
    );
    assert_eq!(snapshot(dir.path()), before);
}
