//! 對某一篇文獻做修改、移除之前，先讀它資料夾裡的 `paper.json`，確認代號就是要動的那一篇。
//! 使用者可能在應用開著的時候，用檔案管理員把資料夾改了名或對調了。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::{BibDraft, PaperFile};
use paperreader_core::jsonio::read_json;
use paperreader_core::local::LocalLibrary;
use std::fs;
use std::path::Path;

const ID_A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const ID_B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";

fn paper(id: &str, title: &str) -> PaperFile {
    let mut p = sample_paper(id);
    p.bib.title = Some(title.into());
    p.materials = vec![source_material("source.pdf", None, None)];
    p
}

fn id_in(root: &Path, folder: &str) -> String {
    read_json::<PaperFile>(&root.join("papers").join(folder).join("paper.json"))
        .unwrap()
        .0
        .id
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<_> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => vec![],
    };
    v.sort();
    v
}

/// 開啟之後（不重掃）：A 資料夾改名成 X，B 資料夾改名成 A 原本的名稱。
fn swapped() -> (tempfile::TempDir, LocalLibrary) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let a = write_paper(root, "A", &paper(ID_A, "Paper A"));
    fs::write(a.join("source.pdf"), b"aaa").unwrap();
    let b = write_paper(root, "B", &paper(ID_B, "Paper B"));
    fs::write(b.join("source.pdf"), b"bbb").unwrap();
    let lib = LocalLibrary::open(root, DEVICE).unwrap();
    fs::rename(root.join("papers/A"), root.join("papers/X")).unwrap();
    fs::rename(root.join("papers/B"), root.join("papers/A")).unwrap();
    (dir, lib)
}

#[test]
fn remove_moves_the_folder_that_holds_the_requested_id() {
    let (dir, mut lib) = swapped();
    let root = dir.path();
    let b_before = fs::read(root.join("papers/A/paper.json")).unwrap();

    lib.remove_paper(ID_A).unwrap();

    // 移走的是 X（代號 A 的那一個）；原本叫 A 的資料夾（現在裝的是 B）原封不動
    let trash = names_in(&root.join(".trash"));
    assert_eq!(trash.len(), 1);
    assert!(trash[0].ends_with("-X"), "{trash:?}");
    assert_eq!(names_in(&root.join("papers")), ["A"]);
    assert_eq!(id_in(root, "A"), ID_B);
    assert_eq!(
        fs::read(root.join("papers/A/paper.json")).unwrap(),
        b_before
    );
    let list = lib.list_papers();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, ID_B);
    assert_eq!(list[0].folder, "A");
}

#[test]
fn get_paper_returns_the_content_of_the_requested_id() {
    let (_dir, lib) = swapped();
    let (p, _) = lib.get_paper(ID_A).unwrap();
    assert_eq!(p.id, ID_A);
    assert_eq!(p.bib.title.as_deref(), Some("Paper A"));
    let (p, _) = lib.get_paper(ID_B).unwrap();
    assert_eq!(p.bib.title.as_deref(), Some("Paper B"));
}

#[test]
fn update_bib_writes_the_file_of_the_requested_id() {
    let (dir, mut lib) = swapped();
    let root = dir.path();
    let (_, token) = lib.get_paper(ID_A).unwrap();
    let b_before = fs::read(root.join("papers/A/paper.json")).unwrap();

    lib.update_bib(
        ID_A,
        BibDraft {
            title: Some("Paper A edited".into()),
            ..Default::default()
        },
        &token,
    )
    .unwrap();

    let a = read_json::<PaperFile>(&root.join("papers/X/paper.json"))
        .unwrap()
        .0;
    assert_eq!(a.id, ID_A);
    assert_eq!(a.bib.title.as_deref(), Some("Paper A edited"));
    assert_eq!(
        fs::read(root.join("papers/A/paper.json")).unwrap(),
        b_before
    );
}

#[test]
fn material_path_points_into_the_folder_of_the_requested_id() {
    let (dir, lib) = swapped();
    let (p, _) = lib.get_paper(ID_A).unwrap();
    let path = lib.material_path(ID_A, &p.materials[0].id).unwrap();
    assert_eq!(path, dir.path().join("papers/X/source.pdf"));
    assert_eq!(fs::read(path).unwrap(), b"aaa");
}

#[test]
fn a_user_folder_without_paper_json_is_never_touched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_paper(root, "A", &paper(ID_A, "Paper A"));
    let user = root.join("papers/U");
    fs::create_dir_all(&user).unwrap();
    fs::write(user.join("my-notes.txt"), "mine").unwrap();
    let mut lib = LocalLibrary::open(root, DEVICE).unwrap();

    // 使用者把自己的資料夾改名成 A 原本的名稱，A 改成 X
    fs::rename(root.join("papers/A"), root.join("papers/X")).unwrap();
    fs::rename(root.join("papers/U"), root.join("papers/A")).unwrap();

    lib.remove_paper(ID_A).unwrap();

    assert_eq!(
        fs::read_to_string(root.join("papers/A/my-notes.txt")).unwrap(),
        "mine"
    );
    assert_eq!(names_in(&root.join("papers")), ["A"]);
    let trash = names_in(&root.join(".trash"));
    assert_eq!(trash.len(), 1);
    assert!(trash[0].ends_with("-X"), "{trash:?}");
}

#[test]
fn not_found_after_the_folder_is_gone_and_nothing_is_touched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_paper(root, "A", &paper(ID_A, "Paper A"));
    let other = write_paper(root, "B", &paper(ID_B, "Paper B"));
    let mut lib = LocalLibrary::open(root, DEVICE).unwrap();
    let (_, token) = lib.get_paper(ID_A).unwrap();
    fs::remove_dir_all(root.join("papers/A")).unwrap();
    let before = snapshot(root);

    assert!(matches!(
        lib.remove_paper(ID_A),
        Err(LibError::NotFound { .. })
    ));
    assert!(matches!(
        lib.get_paper(ID_A),
        Err(LibError::NotFound { .. })
    ));
    assert!(matches!(
        lib.update_bib(ID_A, BibDraft::default(), &token),
        Err(LibError::NotFound { .. })
    ));
    assert!(matches!(
        lib.material_path(ID_A, "x"),
        Err(LibError::NotFound { .. })
    ));
    assert_eq!(snapshot(root), before);
    assert!(other.exists());
    // 重新掃描一次之後，目錄也不再有 A
    assert_eq!(lib.list_papers().len(), 1);
}

#[test]
fn same_size_files_with_the_same_timestamp_are_not_mistaken_for_each_other() {
    // 兩個資料夾對調後，paper.json 大小相同；就算修改時間也一樣，仍要以檔案內容裡的代號為準
    let (dir, mut lib) = swapped();
    let root = dir.path();
    let size_a = fs::metadata(root.join("papers/X/paper.json"))
        .unwrap()
        .len();
    let size_b = fs::metadata(root.join("papers/A/paper.json"))
        .unwrap()
        .len();
    assert_eq!(size_a, size_b, "測試前提：兩份描述檔一樣大");
    let m = fs::metadata(root.join("papers/X/paper.json"))
        .unwrap()
        .modified()
        .unwrap();
    fs::File::options()
        .write(true)
        .open(root.join("papers/A/paper.json"))
        .unwrap()
        .set_modified(m)
        .unwrap();
    lib.remove_paper(ID_A).unwrap();
    assert_eq!(id_in(root, "A"), ID_B);
}
