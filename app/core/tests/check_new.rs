//! 「檢查文獻庫」的新問題種類。

mod common;

use common::*;
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{CheckLevel, Problem, ProblemKind};
use serde_json::{Value, json};
use std::fs;

fn quick(lib: &LocalLibrary) -> Vec<Problem> {
    lib.check(CheckLevel::Quick).unwrap()
}

fn of_kind(ps: &[Problem], kind: ProblemKind) -> Vec<&Problem> {
    ps.iter().filter(|p| p.kind == kind).collect()
}

fn set_groups_on_paper(dir: &tempfile::TempDir, folder: &str, groups: Value) {
    let p = dir.path().join("papers").join(folder).join("paper.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
    v["groups"] = groups;
    fs::write(&p, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
}

fn edit_library_json(dir: &tempfile::TempDir, f: impl FnOnce(&mut Value)) {
    let p = dir.path().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
    f(&mut v);
    fs::write(&p, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
}

#[test]
fn library_with_groups_has_no_unknown_entry() {
    let (dir, mut lib) = temp_library();
    let (id, _) = add_paper(&mut lib, 1, "P", None);
    let (_, t) = lib.list_groups().unwrap();
    let (g, _) = lib.create_group("G", &t).unwrap();
    let pt = lib.get_paper(&id).unwrap().1;
    lib.add_to_group(&id, &g.id, &pt).unwrap();
    let ps = quick(&lib);
    assert!(ps.is_empty(), "{ps:?}");
    // 再開一次也一樣
    drop(lib);
    let lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    assert!(quick(&lib).is_empty());
}

#[test]
fn check_reports_unknown_group_ref() {
    let (dir, mut lib) = temp_library();
    let (id, _) = add_paper(&mut lib, 1, "P", None);
    let (_, t) = lib.list_groups().unwrap();
    lib.create_group("G", &t).unwrap();
    // 兩個懸空的代號仍只一項
    set_groups_on_paper(
        &dir,
        "P",
        json!(["01JCKA1B2C3D4E5F6G7H8J9K0M", "01JCKA1B2C3D4E5F6G7H8J9K0N"]),
    );
    let ps = quick(&lib);
    let refs = of_kind(&ps, ProblemKind::UnknownGroupRef);
    assert_eq!(refs.len(), 1, "{ps:?}");
    assert_eq!(refs[0].path, "papers/P/paper.json");
    assert_eq!(refs[0].paper.as_deref(), Some(id.as_str()));
}

#[test]
fn ref_to_deleted_group_is_unknown() {
    let (dir, mut lib) = temp_library();
    let (id, _) = add_paper(&mut lib, 1, "P", None);
    let (_, t) = lib.list_groups().unwrap();
    let (g, t) = lib.create_group("G", &t).unwrap();
    let pt = lib.get_paper(&id).unwrap().1;
    lib.add_to_group(&id, &g.id, &pt).unwrap();
    lib.delete_group(&g.id, &t).unwrap();
    // 刪除時已經把成員移出；手動再放回去，模擬別處帶著舊代號回來
    set_groups_on_paper(&dir, "P", json!([g.id]));
    let ps = quick(&lib);
    assert_eq!(
        of_kind(&ps, ProblemKind::UnknownGroupRef).len(),
        1,
        "{ps:?}"
    );
}

#[test]
fn no_groups_defined_makes_every_ref_unknown() {
    let (dir, mut lib) = temp_library();
    add_paper(&mut lib, 1, "P", None);
    add_paper(&mut lib, 2, "Q", None);
    set_groups_on_paper(&dir, "P", json!(["01JCKA1B2C3D4E5F6G7H8J9K0M"]));
    set_groups_on_paper(&dir, "Q", json!(["01JCKA1B2C3D4E5F6G7H8J9K0M"]));
    let ps = quick(&lib);
    assert_eq!(
        of_kind(&ps, ProblemKind::UnknownGroupRef).len(),
        2,
        "{ps:?}"
    );
}

#[test]
fn check_reports_unreadable_groups_and_skips_refs() {
    let (dir, mut lib) = temp_library();
    add_paper(&mut lib, 1, "P", None);
    set_groups_on_paper(&dir, "P", json!(["01JCKA1B2C3D4E5F6G7H8J9K0M"]));
    edit_library_json(&dir, |v| v["groups"] = json!("x"));
    let ps = quick(&lib);
    let bad = of_kind(&ps, ProblemKind::UnreadableGroups);
    assert_eq!(bad.len(), 1, "{ps:?}");
    assert_eq!(bad[0].path, "library.json");
    assert!(of_kind(&ps, ProblemKind::UnknownGroupRef).is_empty());
}

// ---- 筆記檔相關（工作 6）----

fn write_note(dir: &tempfile::TempDir, name: &str, text: &str) {
    let d = dir.path().join("notes");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join(name), text).unwrap();
}

fn note_with_paper(paper: &str) -> String {
    format!("---\npaperreader_paper: {paper}\n---\n\n# T\n")
}

#[test]
fn notes_dir_items_are_not_problems() {
    let (dir, lib) = temp_library();
    write_note(&dir, "mine.md", "我自己的筆記，沒有屬性區\n");
    fs::create_dir_all(dir.path().join("notes/sub")).unwrap();
    fs::create_dir_all(dir.path().join("notes/.obsidian")).unwrap();
    assert!(quick(&lib).is_empty(), "{:?}", quick(&lib));
}

#[test]
fn check_reports_invalid_notes_dir() {
    let (dir, lib) = temp_library();
    drop(lib);
    edit_library_json(&dir, |v| v["notes_dir"] = json!("../x"));
    let lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let ps = quick(&lib);
    let bad = of_kind(&ps, ProblemKind::InvalidNotesDir);
    assert_eq!(bad.len(), 1, "{ps:?}");
    assert_eq!(bad[0].path, "library.json");
    assert_eq!(bad[0].paper, None);
}

#[test]
fn check_reports_unreadable_notes_file() {
    let (dir, lib) = temp_library();
    let d = dir.path().join("notes");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("bad.md"), [0xFFu8, 0xFE, 0x80]).unwrap();
    let ps = quick(&lib);
    let bad = of_kind(&ps, ProblemKind::UnreadableNotesFile);
    assert_eq!(bad.len(), 1, "{ps:?}");
    assert_eq!(bad[0].path, "notes/bad.md");
}

#[test]
fn check_reports_duplicate_notes_file() {
    let (dir, mut lib) = temp_library();
    let (id, _) = add_paper(&mut lib, 1, "P", None);
    write_note(&dir, "a.md", &note_with_paper(&id));
    write_note(&dir, "b.md", &note_with_paper(&id));
    let ps = quick(&lib);
    let dup = of_kind(&ps, ProblemKind::DuplicateNotesFile);
    assert_eq!(dup.len(), 1, "{ps:?}");
    assert_eq!(dup[0].path, "notes/b.md");
    assert_eq!(dup[0].paper.as_deref(), Some(id.as_str()));
}

#[test]
fn check_reports_unreadable_note_block_once_per_file() {
    let (dir, mut lib) = temp_library();
    let (id, _) = add_paper(&mut lib, 1, "P", None);
    let broken = format!(
        "{}\n<!-- paperreader:highlight {{broken1 -->\nx\n<!-- paperreader:highlight-end -->\n\n<!-- paperreader:highlight {{broken2 -->\ny\n<!-- paperreader:highlight-end -->\n\n<!-- paperreader:highlight {{\"v\":1}} -->\nno end marker\n",
        note_with_paper(&id)
    );
    write_note(&dir, "P.md", &broken);
    let ps = quick(&lib);
    let bad = of_kind(&ps, ProblemKind::UnreadableNoteBlock);
    assert_eq!(bad.len(), 1, "{ps:?}");
    assert_eq!(bad[0].path, "notes/P.md");
    assert_eq!(bad[0].paper.as_deref(), Some(id.as_str()));
}

#[test]
fn check_reports_leftover_temp_in_notes_dir() {
    let (dir, lib) = temp_library();
    drop(lib);
    edit_library_json(&dir, |v| v["format"] = json!(2));
    write_note(&dir, ".pr-tmp-LEFT", "x");
    let lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    assert!(lib.is_read_only());
    let ps = quick(&lib);
    let left = of_kind(&ps, ProblemKind::LeftoverTemp);
    assert_eq!(left.len(), 1, "{ps:?}");
    assert_eq!(left[0].path, "notes/.pr-tmp-LEFT");
}
