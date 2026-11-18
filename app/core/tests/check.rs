mod common;

use common::*;
use paperreader_core::format::*;
use paperreader_core::jsonio::sha256_hex;
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{CheckLevel, Problem, ProblemKind};
use std::fs;
use std::path::Path;

const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";

/// 一篇完整、沒有問題的文獻：`source.pdf` 在，大小與指紋都對。
fn good_paper(root: &Path, folder: &str, id: &str, seed: u64) -> PaperFile {
    let content = fake_bytes(seed, 400);
    let mut p = sample_paper(id);
    p.materials = vec![source_material(
        "source.pdf",
        Some(400),
        Some(&sha256_hex(&content)),
    )];
    let dir = write_paper(root, folder, &p);
    fs::write(dir.join("source.pdf"), content).unwrap();
    p
}

fn open(dir: &tempfile::TempDir) -> LocalLibrary {
    LocalLibrary::open(dir.path(), DEVICE).unwrap()
}

fn kinds(ps: &[Problem]) -> Vec<(ProblemKind, String)> {
    ps.iter().map(|p| (p.kind, p.path.clone())).collect()
}

#[test]
fn clean_library_has_no_problems() {
    let (dir, mut lib) = temp_library();
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf(src.path(), "a.pdf", 1000);
    let (p, m) = add_paper(&mut lib, 7, "T", None);
    let _ = pdf;
    lib.record_open(&p, &m).unwrap();
    fs::create_dir_all(dir.path().join("notes")).unwrap();
    fs::create_dir_all(dir.path().join("jobs")).unwrap();
    assert!(lib.check(CheckLevel::Quick).unwrap().is_empty());
    assert!(lib.check(CheckLevel::Full).unwrap().is_empty());
}

#[test]
fn unreadable_log_file_is_reported() {
    // 讀不了的紀錄檔（這裡是名稱符合的資料夾）列成一個問題，不擋住開啟
    let (dir, lib) = temp_library();
    drop(lib);
    fs::create_dir_all(dir.path().join("logs/reading-foo.jsonl")).unwrap();
    let lib = open(&dir);
    for level in [CheckLevel::Quick, CheckLevel::Full] {
        let ps = lib.check(level).unwrap();
        assert_eq!(
            kinds(&ps),
            [(
                ProblemKind::UnreadableLogFile,
                "logs/reading-foo.jsonl".into()
            )]
        );
        assert_eq!(ps[0].paper, None);
    }
    // 讀得了的紀錄檔不是問題
    fs::write(dir.path().join("logs/reading-bar.jsonl"), "").unwrap();
    assert_eq!(lib.check(CheckLevel::Quick).unwrap().len(), 1);
}

#[test]
fn unreadable_paper_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("papers/Bad")).unwrap();
    fs::write(dir.path().join("papers/Bad/paper.json"), "{ nope").unwrap();
    let lib = open(&dir);
    let ps = lib.check(CheckLevel::Quick).unwrap();
    assert_eq!(
        kinds(&ps),
        [(
            ProblemKind::UnreadablePaperFile,
            "papers/Bad/paper.json".into()
        )]
    );
    assert_eq!(ps[0].paper, None);
}

#[test]
fn duplicate_id() {
    let dir = tempfile::tempdir().unwrap();
    good_paper(dir.path(), "A", A, 1);
    good_paper(dir.path(), "B", A, 1);
    let lib = open(&dir);
    let ps = lib.check(CheckLevel::Quick).unwrap();
    assert_eq!(kinds(&ps), [(ProblemKind::DuplicateId, "papers/B".into())]);
    assert_eq!(ps[0].paper.as_deref(), Some(A));
}

#[test]
fn material_missing() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = sample_paper(A);
    p.materials = vec![source_material("source.pdf", Some(10), None)];
    write_paper(dir.path(), "A", &p);
    let lib = open(&dir);
    let ps = lib.check(CheckLevel::Quick).unwrap();
    assert_eq!(
        kinds(&ps),
        [(ProblemKind::MaterialMissing, "papers/A/source.pdf".into())]
    );
    assert_eq!(ps[0].paper.as_deref(), Some(A));
}

#[test]
fn material_size_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = sample_paper(A);
    p.materials = vec![source_material("source.pdf", Some(999), None)];
    let d = write_paper(dir.path(), "A", &p);
    fs::write(d.join("source.pdf"), fake_bytes(1, 10)).unwrap();
    let lib = open(&dir);
    let ps = lib.check(CheckLevel::Quick).unwrap();
    assert_eq!(
        kinds(&ps),
        [(
            ProblemKind::MaterialSizeMismatch,
            "papers/A/source.pdf".into()
        )]
    );
}

#[test]
fn material_invalid_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = sample_paper(A);
    p.materials = vec![source_material("../outside.pdf", None, None)];
    write_paper(dir.path(), "A", &p);
    let lib = open(&dir);
    let ps = lib.check(CheckLevel::Quick).unwrap();
    assert_eq!(
        kinds(&ps),
        [(
            ProblemKind::MaterialInvalidPath,
            "papers/A/paper.json".into()
        )]
    );
    assert_eq!(ps[0].paper.as_deref(), Some(A));
}

#[test]
fn hash_mismatch_only_in_full() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = sample_paper(A);
    let recorded = sha256_hex(&fake_bytes(1, 400));
    p.materials = vec![source_material("source.pdf", Some(400), Some(&recorded))];
    let d = write_paper(dir.path(), "A", &p);
    fs::write(d.join("source.pdf"), fake_bytes(2, 400)).unwrap(); // 大小相同、內容不同
    let lib = open(&dir);
    assert!(lib.check(CheckLevel::Quick).unwrap().is_empty());
    let ps = lib.check(CheckLevel::Full).unwrap();
    assert_eq!(
        kinds(&ps),
        [(
            ProblemKind::MaterialHashMismatch,
            "papers/A/source.pdf".into()
        )]
    );
}

#[test]
fn unknown_entries() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    good_paper(root, "A", A, 1);
    fs::write(root.join("stray.txt"), "x").unwrap();
    fs::create_dir_all(root.join("Other")).unwrap();
    fs::write(root.join("papers/readme.txt"), "x").unwrap();
    fs::create_dir_all(root.join("papers/NoJson")).unwrap();
    // 這些是已知的或隱藏的，不算
    for d in ["notes", "jobs", "logs", ".trash", ".stfolder"] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    let lib = open(&dir);
    let ps = lib.check(CheckLevel::Quick).unwrap();
    assert_eq!(
        kinds(&ps),
        [
            (ProblemKind::UnknownEntry, "Other".into()),
            (ProblemKind::UnknownEntry, "papers/NoJson".into()),
            (ProblemKind::UnknownEntry, "papers/readme.txt".into()),
            (ProblemKind::UnknownEntry, "stray.txt".into()),
        ]
    );
}

#[test]
fn unlisted_file() {
    let dir = tempfile::tempdir().unwrap();
    good_paper(dir.path(), "A", A, 1);
    fs::write(dir.path().join("papers/A/my-notes.txt"), "mine").unwrap();
    let lib = open(&dir);
    let ps = lib.check(CheckLevel::Quick).unwrap();
    assert_eq!(
        kinds(&ps),
        [(ProblemKind::UnlistedFile, "papers/A/my-notes.txt".into())]
    );
    assert_eq!(ps[0].paper.as_deref(), Some(A));
}

#[test]
fn hidden_items_are_not_problems() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    good_paper(root, "A", A, 1);
    fs::create_dir_all(root.join(".obsidian")).unwrap();
    fs::write(root.join(".obsidian/app.json"), "{}").unwrap();
    fs::write(root.join("papers/A/.DS_Store"), "x").unwrap();
    fs::write(root.join("papers/.hidden-file"), "x").unwrap();
    fs::create_dir_all(root.join("papers/.stfolder")).unwrap();
    let lib = open(&dir);
    assert!(lib.check(CheckLevel::Full).unwrap().is_empty());
}

#[test]
fn check_does_not_modify_anything() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    good_paper(root, "A", A, 1);
    good_paper(root, "B", B, 2);
    fs::write(root.join("papers/A/my-notes.txt"), "mine").unwrap();
    fs::create_dir_all(root.join("papers/Bad")).unwrap();
    fs::write(root.join("papers/Bad/paper.json"), "{").unwrap();
    fs::write(root.join("stray.txt"), "x").unwrap();
    let lib = open(&dir);
    let before = snapshot(root);
    assert!(!lib.check(CheckLevel::Full).unwrap().is_empty());
    assert_eq!(snapshot(root), before);
}

#[test]
fn check_works_on_read_only_library_and_sees_disk_not_memory() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("library.json"),
        "{\"format\":2,\"id\":\"01JCK3V7Q8Z9X5M2N4P6R8T0AB\"}",
    )
    .unwrap();
    good_paper(dir.path(), "A", A, 1);
    let lib = open(&dir);
    assert!(lib.is_read_only());
    assert!(lib.check(CheckLevel::Full).unwrap().is_empty());
    // 開啟之後磁碟上才出現的問題，檢查要看得到
    fs::remove_file(dir.path().join("papers/A/source.pdf")).unwrap();
    let ps = lib.check(CheckLevel::Quick).unwrap();
    assert_eq!(
        kinds(&ps),
        [(ProblemKind::MaterialMissing, "papers/A/source.pdf".into())]
    );
}
