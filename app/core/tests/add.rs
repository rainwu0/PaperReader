mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::*;
use paperreader_core::jsonio::read_json;
use paperreader_core::local::{AddOutcome, LocalLibrary};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

fn draft(family: &str, year: i32, title: &str) -> BibDraft {
    BibDraft {
        title: Some(title.into()),
        authors: vec![Author {
            family: Some(family.into()),
            ..Default::default()
        }],
        year: Some(year),
        ..Default::default()
    }
}

fn title_only(title: &str) -> BibDraft {
    BibDraft {
        title: Some(title.into()),
        ..Default::default()
    }
}

fn id_of(o: AddOutcome) -> String {
    match o {
        AddOutcome::Added(id) => id,
        other => panic!("expected Added, got {other:?}"),
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

fn src_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn add_writes_spec_layout() {
    let (dir, mut lib) = temp_library();
    let src = src_dir();
    let pdf = fake_pdf(src.path(), "2304.08069v3.pdf", 5000);
    let id = id_of(
        lib.add_paper(&pdf, draft("Zhao", 2024, "DETRs"), Some(14))
            .unwrap(),
    );

    let papers = dir.path().join("papers");
    let folders = names_in(&papers);
    assert_eq!(folders.len(), 1);
    let folder = papers.join(&folders[0]);
    assert_eq!(names_in(&folder), ["paper.json", "source.pdf"]);
    assert_eq!(
        fs::read(folder.join("source.pdf")).unwrap(),
        fs::read(&pdf).unwrap()
    );

    let (paper, _): (PaperFile, _) = read_json(&folder.join("paper.json")).unwrap();
    assert_eq!(paper.format, 1);
    assert_eq!(paper.id, id);
    assert_eq!(paper.short_name, None);
    assert!(paper.aliases.is_empty());
    assert_eq!(paper.bib.modified, paper.added);
    assert_eq!(paper.bib.modified_by.as_deref(), Some(DEVICE));
    assert_eq!(paper.materials.len(), 1);
    let m = &paper.materials[0];
    assert_eq!(m.id.len(), 26);
    assert_eq!(m.role, "source");
    assert_eq!(m.form, "pdf");
    assert_eq!(m.path, "source.pdf");
    assert_eq!(m.bytes, Some(5000));
    assert_eq!(m.pages, Some(14));
    let want: String = Sha256::digest(fs::read(&pdf).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(m.sha256.as_deref(), Some(want.as_str()));
    assert_eq!(m.provenance.method.as_deref(), Some("added-by-user"));
    assert_eq!(
        m.provenance.origin_name.as_deref(),
        Some("2304.08069v3.pdf")
    );
    assert!(m.provenance.time.is_some());
    assert_eq!(m.provenance.proofread, None);
    assert_eq!(m.provenance.page_map, None);
    assert_eq!(m.provenance.source_sha256, None);
}

#[test]
fn add_leaves_source_untouched() {
    let (_dir, mut lib) = temp_library();
    let src = src_dir();
    let pdf = fake_pdf(src.path(), "a.pdf", 777);
    let before = fs::read(&pdf).unwrap();
    lib.add_paper(&pdf, title_only("X"), None).unwrap();
    assert!(pdf.exists());
    assert_eq!(fs::read(&pdf).unwrap(), before);
}

#[test]
fn add_names_folder_from_bib() {
    let (dir, mut lib) = temp_library();
    let src = src_dir();
    let pdf = fake_pdf(src.path(), "a.pdf", 100);
    lib.add_paper(&pdf, draft("Zhao", 2024, "DETRs"), None)
        .unwrap();
    assert_eq!(names_in(&dir.path().join("papers")), ["Zhao 2024 - DETRs"]);
}

#[test]
fn add_same_name_gets_suffix() {
    let (dir, mut lib) = temp_library();
    let src = src_dir();
    let a = fake_pdf(src.path(), "a.pdf", 100);
    let b = fake_pdf(src.path(), "b.pdf", 200);
    lib.add_paper(&a, title_only("Paper"), None).unwrap();
    lib.add_paper(&b, title_only("Paper"), None).unwrap();
    assert_eq!(names_in(&dir.path().join("papers")), ["Paper", "Paper (2)"]);
}

#[test]
fn add_case_only_difference_gets_suffix() {
    let (dir, mut lib) = temp_library();
    let src = src_dir();
    let a = fake_pdf(src.path(), "a.pdf", 100);
    let b = fake_pdf(src.path(), "b.pdf", 200);
    lib.add_paper(&a, title_only("Paper"), None).unwrap();
    lib.add_paper(&b, title_only("paper"), None).unwrap();
    assert_eq!(names_in(&dir.path().join("papers")), ["Paper", "paper (2)"]);
}

#[test]
fn add_same_file_twice_is_already_present() {
    let (dir, mut lib) = temp_library();
    let src = src_dir();
    let pdf = fake_pdf(src.path(), "a.pdf", 300);
    let first = id_of(lib.add_paper(&pdf, title_only("Paper"), None).unwrap());
    let second = lib
        .add_paper(&pdf, title_only("Other title"), None)
        .unwrap();
    assert_eq!(second, AddOutcome::AlreadyPresent(first));
    assert_eq!(names_in(&dir.path().join("papers")), ["Paper"]);
    assert!(
        names_in(&dir.path().join("papers"))
            .iter()
            .all(|n| !n.starts_with(".pr-"))
    );
}

#[test]
fn add_normalizes_doi() {
    let (_dir, mut lib) = temp_library();
    let src = src_dir();
    let pdf = fake_pdf(src.path(), "a.pdf", 100);
    let mut d = title_only("P");
    d.doi = Some(" https://doi.org/10.48550/arXiv.2304.08069 ".into());
    let id = id_of(lib.add_paper(&pdf, d, None).unwrap());
    let (paper, _) = lib.get_paper(&id).unwrap();
    assert_eq!(paper.bib.doi.as_deref(), Some("10.48550/arxiv.2304.08069"));
}

#[test]
fn find_by_doi_matches_any_spelling() {
    let (_dir, mut lib) = temp_library();
    let src = src_dir();
    let a = fake_pdf(src.path(), "a.pdf", 100);
    let b = fake_pdf(src.path(), "b.pdf", 200);
    let mut d = title_only("P");
    d.doi = Some("10.48550/arxiv.2304.08069".into());
    let id = id_of(lib.add_paper(&a, d, None).unwrap());
    // 同一個 DOI、不同檔案：照樣加入（DOI 相同不擋下）
    let mut d2 = title_only("P again");
    d2.doi = Some("doi:10.48550/ARXIV.2304.08069".into());
    let id2 = id_of(lib.add_paper(&b, d2, None).unwrap());

    let mut found = lib.find_by_doi("HTTPS://DOI.ORG/10.48550/arXiv.2304.08069");
    found.sort();
    let mut want = vec![id, id2];
    want.sort();
    assert_eq!(found, want);
    assert!(lib.find_by_doi("10.1/none").is_empty());
    assert!(lib.find_by_doi("  ").is_empty());
}

#[test]
fn add_rejects_directory_missing_and_empty() {
    let (dir, mut lib) = temp_library();
    let src = src_dir();
    let empty = src.path().join("empty.pdf");
    fs::write(&empty, b"").unwrap();
    let r = lib.add_paper(src.path(), title_only("x"), None);
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    let r = lib.add_paper(&src.path().join("missing.pdf"), title_only("x"), None);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    let r = lib.add_paper(&empty, title_only("x"), None);
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    assert!(names_in(&dir.path().join("papers")).is_empty());
    assert!(lib.list_papers().is_empty());
}

#[test]
fn add_failure_leaves_nothing() {
    let (dir, mut lib) = temp_library();
    fs::remove_dir_all(dir.path().join("papers")).unwrap();
    fs::write(dir.path().join("papers"), b"i am a file").unwrap();
    let src = src_dir();
    let pdf = fake_pdf(src.path(), "a.pdf", 100);
    assert!(lib.add_paper(&pdf, title_only("x"), None).is_err());
    assert!(names_in(dir.path()).iter().all(|n| !n.starts_with(".pr-")));
    assert_eq!(fs::read(dir.path().join("papers")).unwrap(), b"i am a file");
}

#[test]
fn add_in_read_only_library_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("library.json"),
        "{\"format\":2,\"id\":\"01JCK3V7Q8Z9X5M2N4P6R8T0AB\"}",
    )
    .unwrap();
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let src = src_dir();
    let pdf = fake_pdf(src.path(), "a.pdf", 100);
    let before = snapshot(dir.path());
    let r = lib.add_paper(&pdf, title_only("x"), None);
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn added_paper_visible_without_rescan() {
    let (_dir, mut lib) = temp_library();
    let src = src_dir();
    let pdf = fake_pdf(src.path(), "a.pdf", 100);
    let id = id_of(
        lib.add_paper(&pdf, draft("Lema", 2025, "Survey"), None)
            .unwrap(),
    );
    let list = lib.list_papers();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
    assert_eq!(list[0].folder, "Lema 2025 - Survey");
    assert_eq!(list[0].materials.len(), 1);
    assert_eq!(
        list[0].materials[0].status,
        paperreader_core::store::MaterialStatus::Ok
    );
    let path = lib.material_path(&id, &list[0].materials[0].id).unwrap();
    assert!(path.ends_with("source.pdf"));
}

#[test]
fn add_without_bib_uses_origin_name_for_folder() {
    let (dir, mut lib) = temp_library();
    let src = src_dir();
    let pdf = fake_pdf(src.path(), "2304.08069v3.pdf", 100);
    lib.add_paper(&pdf, BibDraft::default(), None).unwrap();
    assert_eq!(names_in(&dir.path().join("papers")), ["2304.08069v3"]);
}
