mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::BibDraft;
use paperreader_core::local::{AddOutcome, LocalLibrary};
use std::fs;
use std::path::Path;

fn title(t: &str) -> BibDraft {
    BibDraft {
        title: Some(t.into()),
        ..Default::default()
    }
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

fn added(lib: &mut LocalLibrary, pdf: &Path, t: &str) -> String {
    match lib.add_paper(pdf, title(t), None).unwrap() {
        AddOutcome::Added(id) => id,
        o => panic!("{o:?}"),
    }
}

/// `^\d{8}T\d{6}Z-` 加原資料夾名稱
fn is_trash_name(name: &str, original: &str) -> bool {
    let b = name.as_bytes();
    name.len() == 17 + original.len()
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'T'
        && b[9..15].iter().all(u8::is_ascii_digit)
        && b[15] == b'Z'
        && b[16] == b'-'
        && &name[17..] == original
}

#[test]
fn remove_moves_whole_folder() {
    let (dir, mut lib) = temp_library();
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf(src.path(), "a.pdf", 500);
    let id = added(&mut lib, &pdf, "Paper");
    fs::write(dir.path().join("papers/Paper/my-notes.txt"), "mine").unwrap();

    lib.remove_paper(&id).unwrap();

    assert!(names_in(&dir.path().join("papers")).is_empty());
    assert!(lib.list_papers().is_empty());
    let trash = names_in(&dir.path().join(".trash"));
    assert_eq!(trash.len(), 1);
    assert!(is_trash_name(&trash[0], "Paper"), "{trash:?}");
    let moved = dir.path().join(".trash").join(&trash[0]);
    assert_eq!(
        names_in(&moved),
        ["my-notes.txt", "paper.json", "source.pdf"]
    );
    assert_eq!(
        fs::read_to_string(moved.join("my-notes.txt")).unwrap(),
        "mine"
    );
}

#[test]
fn restore_brings_back_same_id() {
    let (dir, mut lib) = temp_library();
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf(src.path(), "a.pdf", 500);
    let id = added(&mut lib, &pdf, "Paper");
    fs::write(dir.path().join("papers/Paper/my-notes.txt"), "mine").unwrap();
    lib.remove_paper(&id).unwrap();

    lib.restore_paper(&id).unwrap();

    let list = lib.list_papers();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
    assert_eq!(list[0].folder, "Paper");
    assert_eq!(
        fs::read_to_string(dir.path().join("papers/Paper/my-notes.txt")).unwrap(),
        "mine"
    );
    assert!(names_in(&dir.path().join(".trash")).is_empty());
}

#[test]
fn restore_with_name_taken_gets_suffix() {
    let (dir, mut lib) = temp_library();
    let src = tempfile::tempdir().unwrap();
    let a = fake_pdf(src.path(), "a.pdf", 500);
    let b = fake_pdf(src.path(), "b.pdf", 600);
    let id_a = added(&mut lib, &a, "Paper");
    lib.remove_paper(&id_a).unwrap();
    let id_b = added(&mut lib, &b, "Paper");
    lib.restore_paper(&id_a).unwrap();

    assert_eq!(names_in(&dir.path().join("papers")), ["Paper", "Paper (2)"]);
    let list = lib.list_papers();
    assert_eq!(list.iter().find(|p| p.id == id_b).unwrap().folder, "Paper");
    assert_eq!(
        list.iter().find(|p| p.id == id_a).unwrap().folder,
        "Paper (2)"
    );
}

#[test]
fn add_after_remove_reports_in_trash() {
    let (dir, mut lib) = temp_library();
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf(src.path(), "a.pdf", 500);
    let id = added(&mut lib, &pdf, "Paper");
    lib.remove_paper(&id).unwrap();

    let r = lib.add_paper(&pdf, title("Paper"), None).unwrap();
    assert_eq!(r, AddOutcome::InTrash(id));
    assert!(names_in(&dir.path().join("papers")).is_empty());
}

#[test]
fn remove_unknown_id_is_not_found() {
    let (_dir, mut lib) = temp_library();
    let r = lib.remove_paper("01ZZZZZZZZZZZZZZZZZZZZZZZZ");
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
}

#[test]
fn restore_unknown_id_is_not_found() {
    let (_dir, mut lib) = temp_library();
    let r = lib.restore_paper("01ZZZZZZZZZZZZZZZZZZZZZZZZ");
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
}

#[test]
fn remove_in_read_only_library_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("library.json"),
        "{\"format\":2,\"id\":\"01JCK3V7Q8Z9X5M2N4P6R8T0AB\"}",
    )
    .unwrap();
    write_paper(dir.path(), "A", &sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA"));
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let before = snapshot(dir.path());
    assert!(matches!(
        lib.remove_paper("01AAAAAAAAAAAAAAAAAAAAAAAA"),
        Err(LibError::ReadOnly)
    ));
    assert!(matches!(
        lib.restore_paper("01AAAAAAAAAAAAAAAAAAAAAAAA"),
        Err(LibError::ReadOnly)
    ));
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn removing_the_same_name_twice_in_a_second_does_not_clash() {
    let (dir, mut lib) = temp_library();
    let src = tempfile::tempdir().unwrap();
    let a = fake_pdf(src.path(), "a.pdf", 500);
    let b = fake_pdf(src.path(), "b.pdf", 600);
    let id_a = added(&mut lib, &a, "Paper");
    lib.remove_paper(&id_a).unwrap();
    let id_b = added(&mut lib, &b, "Paper");
    lib.remove_paper(&id_b).unwrap();
    assert_eq!(names_in(&dir.path().join(".trash")).len(), 2);
    lib.restore_paper(&id_a).unwrap();
    lib.restore_paper(&id_b).unwrap();
    assert_eq!(lib.list_papers().len(), 2);
}

#[test]
fn events_survive_remove() {
    let (_dir, mut lib) = temp_library();
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf(src.path(), "a.pdf", 500);
    let id = added(&mut lib, &pdf, "Paper");
    let mid = lib.get_paper(&id).unwrap().0.materials[0].id.clone();
    let e = lib.record_open(&id, &mid).unwrap();

    lib.remove_paper(&id).unwrap();
    let got = lib
        .read_events(&paperreader_core::local::EventFilter::default())
        .unwrap();
    assert_eq!(got, vec![e.clone()]);

    lib.restore_paper(&id).unwrap();
    let got = lib
        .read_events(&paperreader_core::local::EventFilter {
            paper: Some(id),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(got, [e]);
}
