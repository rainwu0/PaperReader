//! 加入時核心自己驗檔頭，頁數的時間戳，目錄的 `added`。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::*;
use paperreader_core::jsonio::to_canonical_json;
use paperreader_core::local::AddOutcome;
use std::fs;
use std::io::Cursor;
use std::path::Path;

fn title(t: &str) -> BibDraft {
    BibDraft {
        title: Some(t.into()),
        ..Default::default()
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn add_rejects_non_pdf() {
    let (dir, mut lib) = temp_library();
    let src = tempfile::tempdir().unwrap();
    let p = src.path().join("a.pdf");
    fs::write(&p, b"this is plain text, not a pdf at all").unwrap();
    let res = lib.add_paper(&p, title("x"), None);
    match res {
        Err(LibError::InvalidInput { reason }) => {
            assert!(reason.starts_with("not a PDF"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
    assert!(names_in(&dir.path().join("papers")).is_empty());
    assert!(names_in(dir.path()).iter().all(|n| !n.starts_with(".pr-")));
}

#[test]
fn add_from_reader_rejects_non_pdf() {
    let (dir, mut lib) = temp_library();
    let res = lib.add_paper_from_reader(
        &mut Cursor::new(b"PK\x03\x04 not a pdf".to_vec()),
        Some("a.pdf"),
        title("x"),
        None,
    );
    match res {
        Err(LibError::InvalidInput { reason }) => {
            assert!(reason.starts_with("not a PDF"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
    assert!(names_in(&dir.path().join("papers")).is_empty());
    assert!(names_in(dir.path()).iter().all(|n| !n.starts_with(".pr-")));
}

#[test]
fn add_accepts_header_after_junk() {
    let (_dir, mut lib) = temp_library();
    let src = tempfile::tempdir().unwrap();
    let p = src.path().join("a.pdf");
    let mut bytes = vec![b'j'; 200];
    bytes.extend_from_slice(b"%PDF-1.5\nrest");
    fs::write(&p, bytes).unwrap();
    let out = lib.add_paper(&p, title("x"), None).unwrap();
    assert!(matches!(out, AddOutcome::Added(_)), "{out:?}");
}

#[test]
fn add_with_pages_stamps_pages_time() {
    let (_dir, mut lib) = temp_library();
    let (id, _) = add_paper(&mut lib, 1, "With pages", Some(14));
    let (paper, _) = lib.get_paper(&id).unwrap();
    let m = &paper.materials[0];
    assert_eq!(m.pages, Some(14));
    assert_eq!(m.pages_modified, paper.added);
    assert_eq!(m.pages_modified_by.as_deref(), Some(DEVICE));

    let (id2, _) = add_paper(&mut lib, 2, "No pages", None);
    let (paper2, _) = lib.get_paper(&id2).unwrap();
    assert_eq!(paper2.materials[0].pages, None);
    assert_eq!(paper2.materials[0].pages_modified, None);
    assert_eq!(paper2.materials[0].pages_modified_by, None);
}

#[test]
fn summary_has_added_time() {
    let (_dir, mut lib) = temp_library();
    let (id, _) = add_paper(&mut lib, 1, "A", None);
    let (paper, _) = lib.get_paper(&id).unwrap();
    assert!(paper.added.is_some());
    let s = lib.list_papers().into_iter().find(|s| s.id == id).unwrap();
    assert_eq!(s.added, paper.added);
    assert!(s.groups.is_empty());
}

#[test]
fn summary_added_is_null_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let mut paper = sample_paper("01JCK3W2E4H6J8K0M2N4P6Q8RS");
    paper.added = None;
    write_paper(dir.path(), "Old", &paper);
    let lib = paperreader_core::local::LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let list = lib.list_papers();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].added, None);
    // 確認寫出去的檔案真的沒有 added 的值
    let text = to_canonical_json(&paper).unwrap();
    assert!(text.contains("\"added\": null"));
}
