//! 工作 3：所屬關係與刪除群組。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::jsonio::VersionToken;
use paperreader_core::local::LocalLibrary;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

fn paper_path(dir: &tempfile::TempDir, folder: &str) -> PathBuf {
    dir.path().join("papers").join(folder).join("paper.json")
}

fn read_value(p: &PathBuf) -> Value {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}

fn write_value(p: &PathBuf, v: &Value) {
    fs::write(p, serde_json::to_string_pretty(v).unwrap() + "\n").unwrap();
}

fn group_token(lib: &LocalLibrary) -> VersionToken {
    lib.list_groups().unwrap().1
}

fn new_group(lib: &mut LocalLibrary, name: &str) -> String {
    let t = group_token(lib);
    lib.create_group(name, &t).unwrap().0.id
}

fn ptoken(lib: &LocalLibrary, id: &str) -> VersionToken {
    lib.get_paper(id).unwrap().1
}

/// 一個文獻庫、一個群組 G、三篇文獻 P1..P3（資料夾 P1..P3）。
fn setup() -> (tempfile::TempDir, LocalLibrary, String, Vec<String>) {
    let (dir, mut lib) = temp_library();
    let g = new_group(&mut lib, "G");
    let ids = (1..=3)
        .map(|i| add_paper(&mut lib, i, &format!("P{i}"), None).0)
        .collect();
    (dir, lib, g, ids)
}

#[test]
fn add_to_group_writes_only_groups() {
    let (dir, mut lib, g, ids) = setup();
    let path = paper_path(&dir, "P1");
    let mut v = read_value(&path);
    v["x_future"] = json!(1);
    write_value(&path, &v);
    lib.rescan().unwrap();
    let before = read_value(&path);
    lib.add_to_group(&ids[0], &g, &ptoken(&lib, &ids[0]))
        .unwrap();
    let after = read_value(&path);
    assert_eq!(after["groups"], json!([g]));
    assert_eq!(after["groups_modified_by"], DEVICE);
    assert!(after["groups_modified"].as_str().unwrap().ends_with('Z'));
    for k in ["bib", "materials", "x_future", "id", "added", "format"] {
        assert_eq!(before[k], after[k], "{k}");
    }
}

#[test]
fn add_to_group_twice_is_noop() {
    let (dir, mut lib, g, ids) = setup();
    let t = lib
        .add_to_group(&ids[0], &g, &ptoken(&lib, &ids[0]))
        .unwrap();
    let path = paper_path(&dir, "P1");
    let bytes = fs::read(&path).unwrap();
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    let t2 = lib.add_to_group(&ids[0], &g, &t).unwrap();
    assert_eq!(t, t2);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), mtime);
}

#[test]
fn paper_can_be_in_many_groups() {
    let (_dir, mut lib, g, ids) = setup();
    let g2 = new_group(&mut lib, "G2");
    let g3 = new_group(&mut lib, "G3");
    let mut t = ptoken(&lib, &ids[0]);
    for x in [&g, &g3, &g2] {
        t = lib.add_to_group(&ids[0], x, &t).unwrap();
    }
    let (p, _) = lib.get_paper(&ids[0]).unwrap();
    assert_eq!(p.groups, vec![g.clone(), g3, g2]);
}

#[test]
fn add_to_unknown_or_deleted_group_is_not_found() {
    let (_dir, mut lib, g, ids) = setup();
    let t = ptoken(&lib, &ids[0]);
    let r = lib.add_to_group(&ids[0], "01JCKA1B2C3D4E5F6G7H8J9K0M", &t);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    let gt = group_token(&lib);
    lib.delete_group(&g, &gt).unwrap();
    let r = lib.add_to_group(&ids[0], &g, &t);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
}

#[test]
fn unreadable_groups_blocks_add_but_not_remove() {
    let (dir, mut lib, g, ids) = setup();
    let t = lib
        .add_to_group(&ids[0], &g, &ptoken(&lib, &ids[0]))
        .unwrap();
    let lp = dir.path().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    v["groups"] = json!("x");
    write_value(&lp, &v);
    let t2 = ptoken(&lib, &ids[1]);
    let r = lib.add_to_group(&ids[1], &g, &t2);
    assert!(matches!(r, Err(LibError::Unreadable { .. })), "{r:?}");
    lib.remove_from_group(&ids[0], &g, &t).unwrap();
    assert!(lib.get_paper(&ids[0]).unwrap().0.groups.is_empty());
}

#[test]
fn remove_and_delete_also_drop_duplicate_ids() {
    // 這篇的群組被改時順便去掉重複的代號
    let (dir, mut lib, g, ids) = setup();
    let g2 = new_group(&mut lib, "G2");
    let path = paper_path(&dir, "P1");
    let mut v = read_value(&path);
    v["groups"] = json!([g, g2, g, g2]);
    write_value(&path, &v);
    lib.rescan().unwrap();
    lib.remove_from_group(&ids[0], &g, &ptoken(&lib, &ids[0]))
        .unwrap();
    assert_eq!(read_value(&path)["groups"], json!([g2]));

    let mut v = read_value(&path);
    v["groups"] = json!([g2, g, g2, g]);
    write_value(&path, &v);
    lib.rescan().unwrap();
    lib.delete_group(&g, &group_token(&lib)).unwrap();
    assert_eq!(read_value(&path)["groups"], json!([g2]));
}

#[test]
fn unreadable_deleted_groups_blocks_only_delete() {
    // deleted_groups 讀不懂時，加入、移出照常（library.json 不動）；刪除拒絕
    let (dir, mut lib, g, ids) = setup();
    let lp = dir.path().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    v["deleted_groups"] = json!("x");
    write_value(&lp, &v);
    let lib_bytes = fs::read(&lp).unwrap();
    let t = lib
        .add_to_group(&ids[0], &g, &ptoken(&lib, &ids[0]))
        .unwrap();
    lib.remove_from_group(&ids[0], &g, &t).unwrap();
    let t = lib
        .add_to_group(&ids[0], &g, &ptoken(&lib, &ids[0]))
        .unwrap();
    let _ = t;
    assert_eq!(
        fs::read(&lp).unwrap(),
        lib_bytes,
        "library.json must not be touched"
    );
    let before = snapshot(&dir.path().join("papers"));
    let r = lib.delete_group(&g, &group_token(&lib));
    assert!(matches!(r, Err(LibError::Unreadable { .. })), "{r:?}");
    assert_eq!(snapshot(&dir.path().join("papers")), before);
    assert_eq!(fs::read(&lp).unwrap(), lib_bytes);
}

#[test]
fn membership_stale_token_is_conflict() {
    let (dir, mut lib, g, ids) = setup();
    let t = ptoken(&lib, &ids[0]);
    let path = paper_path(&dir, "P1");
    let mut v = read_value(&path);
    v["bib"]["title"] = json!("Changed Outside");
    write_value(&path, &v);
    let bytes = fs::read(&path).unwrap();
    let r = lib.add_to_group(&ids[0], &g, &t);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    let r = lib.remove_from_group(&ids[0], &g, &t);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn remove_dangling_group_id() {
    let (dir, mut lib, _g, ids) = setup();
    let path = paper_path(&dir, "P1");
    let mut v = read_value(&path);
    v["groups"] = json!(["01JCKA1B2C3D4E5F6G7H8J9K0M"]);
    write_value(&path, &v);
    lib.rescan().unwrap();
    lib.remove_from_group(
        &ids[0],
        "01JCKA1B2C3D4E5F6G7H8J9K0M",
        &ptoken(&lib, &ids[0]),
    )
    .unwrap();
    let after = read_value(&path);
    assert_eq!(after["groups"], json!([]));
    assert_eq!(after["groups_modified_by"], DEVICE);
    assert!(after["groups_modified"].is_string());
}

#[test]
fn remove_when_not_member_is_noop() {
    let (dir, mut lib, g, ids) = setup();
    let t = ptoken(&lib, &ids[0]);
    let bytes = fs::read(paper_path(&dir, "P1")).unwrap();
    let t2 = lib.remove_from_group(&ids[0], &g, &t).unwrap();
    assert_eq!(t, t2);
    assert_eq!(fs::read(paper_path(&dir, "P1")).unwrap(), bytes);
}

#[test]
fn membership_newer_format_refused() {
    let (dir, mut lib, g, ids) = setup();
    let path = paper_path(&dir, "P1");
    let mut v = read_value(&path);
    v["format"] = json!(2);
    write_value(&path, &v);
    lib.rescan().unwrap();
    let t = ptoken(&lib, &ids[0]);
    let bytes = fs::read(&path).unwrap();
    let r = lib.add_to_group(&ids[0], &g, &t);
    assert!(matches!(r, Err(LibError::FormatTooNew { .. })), "{r:?}");
    let r = lib.remove_from_group(&ids[0], &g, &t);
    assert!(matches!(r, Err(LibError::FormatTooNew { .. })), "{r:?}");
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn membership_paper_in_trash_not_found() {
    let (_dir, mut lib, g, ids) = setup();
    let t = ptoken(&lib, &ids[0]);
    lib.remove_paper(&ids[0]).unwrap();
    let r = lib.add_to_group(&ids[0], &g, &t);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    let r = lib.remove_from_group(&ids[0], &g, &t);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
}

#[test]
fn membership_read_only_refused() {
    let (dir, lib, g, ids) = setup();
    let t = ptoken(&lib, &ids[0]);
    drop(lib);
    let lp = dir.path().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    v["format"] = json!(2);
    write_value(&lp, &v);
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let r = lib.add_to_group(&ids[0], &g, &t);
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
    let r = lib.remove_from_group(&ids[0], &g, &t);
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
}

#[test]
fn summary_lists_groups() {
    let (_dir, mut lib, g, ids) = setup();
    lib.add_to_group(&ids[0], &g, &ptoken(&lib, &ids[0]))
        .unwrap();
    let s = lib
        .list_papers()
        .into_iter()
        .find(|s| s.id == ids[0])
        .unwrap();
    assert_eq!(s.groups, vec![g]);
}

#[test]
fn delete_group_removes_members_then_definition() {
    let (dir, mut lib, g, ids) = setup();
    for id in &ids {
        lib.add_to_group(id, &g, &ptoken(&lib, id)).unwrap();
    }
    let (_, other) = {
        let (id4, _) = add_paper(&mut lib, 9, "P4", None);
        (0, id4)
    };
    let p4 = paper_path(&dir, "P4");
    let p4_before = fs::read(&p4).unwrap();
    let out = lib.delete_group(&g, &group_token(&lib)).unwrap();
    assert!(out.skipped.is_empty());
    for (i, id) in ids.iter().enumerate() {
        let v = read_value(&paper_path(&dir, &format!("P{}", i + 1)));
        assert_eq!(v["groups"], json!([]), "{id}");
        assert_eq!(v["groups_modified_by"], DEVICE);
    }
    assert_eq!(fs::read(&p4).unwrap(), p4_before);
    let _ = other;
    let (defs, t) = lib.list_groups().unwrap();
    assert!(defs.is_empty());
    assert_eq!(out.token, t);
    let lib_v: Value =
        serde_json::from_slice(&fs::read(dir.path().join("library.json")).unwrap()).unwrap();
    let del = lib_v["deleted_groups"].as_array().unwrap();
    assert_eq!(del.len(), 1);
    assert_eq!(del[0]["id"], g.as_str());
    assert_eq!(del[0]["deleted_by"], DEVICE);
    assert!(del[0]["deleted"].as_str().unwrap().ends_with('Z'));
}

#[test]
fn delete_group_updates_memory_without_rescan() {
    let (_dir, mut lib, g, ids) = setup();
    for id in &ids {
        lib.add_to_group(id, &g, &ptoken(&lib, id)).unwrap();
    }
    lib.delete_group(&g, &group_token(&lib)).unwrap();
    for s in lib.list_papers() {
        assert!(!s.groups.contains(&g), "{s:?}");
    }
}

#[test]
fn delete_group_removes_all_duplicate_definitions() {
    let (dir, mut lib, _g, _ids) = setup();
    let lp = dir.path().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    let id = "01JCKA1B2C3D4E5F6G7H8J9K0M";
    v["groups"] = json!([{"id": id, "name": "A"}, {"id": id, "name": "B"}]);
    write_value(&lp, &v);
    let r = lib.delete_group(id, &group_token(&lib)).unwrap();
    let _ = r;
    let after: Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    assert_eq!(after["groups"], json!([]));
    assert!(lib.list_groups().unwrap().0.is_empty());
}

#[test]
fn delete_group_retry_after_partial() {
    let (dir, mut lib, g, ids) = setup();
    for id in &ids {
        lib.add_to_group(id, &g, &ptoken(&lib, id)).unwrap();
    }
    // 做到一半：只有 P3 還帶著代號
    for i in [1, 2] {
        let p = paper_path(&dir, &format!("P{i}"));
        let mut v = read_value(&p);
        v["groups"] = json!([]);
        write_value(&p, &v);
    }
    lib.rescan().unwrap();
    let out = lib.delete_group(&g, &group_token(&lib)).unwrap();
    assert!(out.skipped.is_empty());
    for i in 1..=3 {
        assert_eq!(
            read_value(&paper_path(&dir, &format!("P{i}")))["groups"],
            json!([])
        );
    }
    let lib_v: Value =
        serde_json::from_slice(&fs::read(dir.path().join("library.json")).unwrap()).unwrap();
    assert_eq!(lib_v["groups"], json!([]));
    assert_eq!(lib_v["deleted_groups"].as_array().unwrap().len(), 1);
}

#[test]
fn delete_group_skips_newer_format_member() {
    let (dir, mut lib, g, ids) = setup();
    for id in &ids {
        lib.add_to_group(id, &g, &ptoken(&lib, id)).unwrap();
    }
    let p2 = paper_path(&dir, "P2");
    let mut v = read_value(&p2);
    v["format"] = json!(2);
    write_value(&p2, &v);
    let bytes = fs::read(&p2).unwrap();
    let out = lib.delete_group(&g, &group_token(&lib)).unwrap();
    assert_eq!(out.skipped, vec![ids[1].clone()]);
    assert_eq!(fs::read(&p2).unwrap(), bytes);
    assert_eq!(read_value(&paper_path(&dir, "P1"))["groups"], json!([]));
    assert_eq!(read_value(&paper_path(&dir, "P3"))["groups"], json!([]));
    assert!(lib.list_groups().unwrap().0.is_empty());
}

#[test]
fn delete_group_leaves_trash_untouched() {
    let (dir, mut lib, g, ids) = setup();
    lib.add_to_group(&ids[0], &g, &ptoken(&lib, &ids[0]))
        .unwrap();
    lib.remove_paper(&ids[0]).unwrap();
    let trash_json = fs::read_dir(dir.path().join(".trash"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("paper.json");
    let before = fs::read(&trash_json).unwrap();
    lib.delete_group(&g, &group_token(&lib)).unwrap();
    assert_eq!(fs::read(&trash_json).unwrap(), before);
}

#[test]
fn delete_group_stale_token_changes_nothing() {
    let (dir, mut lib, g, ids) = setup();
    lib.add_to_group(&ids[0], &g, &ptoken(&lib, &ids[0]))
        .unwrap();
    let stale = group_token(&lib);
    // 外部改了 library.json
    let lp = dir.path().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    v["x_future"] = json!(true);
    write_value(&lp, &v);
    let papers_before = snapshot(&dir.path().join("papers"));
    let lib_before = fs::read(&lp).unwrap();
    let r = lib.delete_group(&g, &stale);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert_eq!(snapshot(&dir.path().join("papers")), papers_before);
    assert_eq!(fs::read(&lp).unwrap(), lib_before);
}

#[test]
fn delete_unknown_group_is_not_found() {
    let (_dir, mut lib, _g, _ids) = setup();
    let r = lib.delete_group("01JCKA1B2C3D4E5F6G7H8J9K0M", &group_token(&lib));
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
}
