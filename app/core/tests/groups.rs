//! 工作 2：群組的定義。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::jsonio::{VersionToken, sha256_hex};
use paperreader_core::local::LocalLibrary;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

fn lib_path(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("library.json")
}

fn read_lib(dir: &tempfile::TempDir) -> Value {
    serde_json::from_slice(&fs::read(lib_path(dir)).unwrap()).unwrap()
}

fn write_lib(dir: &tempfile::TempDir, v: &Value) {
    fs::write(
        lib_path(dir),
        serde_json::to_string_pretty(v).unwrap() + "\n",
    )
    .unwrap();
}

fn token_of(path: &Path) -> VersionToken {
    VersionToken(sha256_hex(&fs::read(path).unwrap()))
}

fn setup() -> (tempfile::TempDir, LocalLibrary, VersionToken) {
    let (dir, lib) = temp_library();
    let (_, t) = lib.list_groups().unwrap();
    (dir, lib, t)
}

#[test]
fn create_group_writes_spec_layout() {
    let (dir, mut lib, t) = setup();
    let before = read_lib(&dir);
    let (g, t2) = lib.create_group("物件偵測", &t).unwrap();
    let after = read_lib(&dir);
    let def = &after["groups"][0];
    assert_eq!(def.as_object().unwrap().len(), 5);
    assert_eq!(def["id"], g.id.as_str());
    assert_eq!(def["name"], "物件偵測");
    assert_eq!(def["created"], def["modified"]);
    assert_eq!(def["modified_by"], DEVICE);
    for k in ["id", "created", "notes_dir", "format"] {
        assert_eq!(before[k], after[k], "{k}");
    }
    assert_eq!(t2, token_of(&lib_path(&dir)));
    let (list, t3) = lib.list_groups().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, g.id);
    assert_eq!(t3, t2);
    // 記憶體裡的 library_file 也更新了
    assert_eq!(lib.library_file().groups.as_array().unwrap().len(), 1);
}

#[test]
fn stale_token_is_conflict() {
    let (dir, mut lib, t) = setup();
    let mut v = read_lib(&dir);
    v["x_other_tool"] = json!(true);
    write_lib(&dir, &v);
    let on_disk = fs::read(lib_path(&dir)).unwrap();
    let r = lib.create_group("A", &t);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert_eq!(fs::read(lib_path(&dir)).unwrap(), on_disk);
}

#[test]
fn returned_token_allows_next_write() {
    let (_dir, mut lib, t) = setup();
    let (a, t) = lib.create_group("A", &t).unwrap();
    let (_b, t) = lib.create_group("B", &t).unwrap();
    let t = lib.rename_group(&a.id, "A2", &t).unwrap();
    let (list, t2) = lib.list_groups().unwrap();
    assert_eq!(t, t2);
    assert_eq!(
        list.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
        ["A2", "B"]
    );
}

#[test]
fn group_name_rules() {
    let (dir, mut lib, t) = setup();
    let before = fs::read(lib_path(&dir)).unwrap();
    let too_long = "x".repeat(101);
    for bad in ["", "   ", too_long.as_str(), "a\u{7}b"] {
        let r = lib.create_group(bad, &t);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{bad:?}: {r:?}"
        );
    }
    assert_eq!(fs::read(lib_path(&dir)).unwrap(), before);

    let (g, t) = lib.create_group("  物件偵測 ", &t).unwrap();
    assert_eq!(g.name, "物件偵測");
    // 與既有的只差大小寫
    let (_, t) = lib.create_group("Vision", &t).unwrap();
    let r = lib.create_group("vision", &t);
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    // 100 個字元可以（以 Unicode 字元計）
    let hundred = "論".repeat(100);
    lib.create_group(&hundred, &t).unwrap();
}

#[test]
fn rename_group_to_own_name_with_case_change_is_ok() {
    let (_dir, mut lib, t) = setup();
    let (g, t) = lib.create_group("vision", &t).unwrap();
    lib.rename_group(&g.id, "Vision", &t).unwrap();
    let (list, _) = lib.list_groups().unwrap();
    assert_eq!(list[0].name, "Vision");
}

#[test]
fn rename_group_stamps_time_and_device() {
    let (dir, mut lib, t) = setup();
    let (g, t) = lib.create_group("A", &t).unwrap();
    // 讓兩次的時間不同：時間以毫秒計，確保至少過 2 毫秒
    std::thread::sleep(std::time::Duration::from_millis(5));
    lib.rename_group(&g.id, "B", &t).unwrap();
    let def = read_lib(&dir)["groups"][0].clone();
    assert_eq!(def["name"], "B");
    assert_eq!(def["modified_by"], DEVICE);
    assert!(def["modified"].as_str() > def["created"].as_str());
    assert_eq!(def["created"], json!(g.created));
}

#[test]
fn rename_group_touches_only_library_json() {
    let (dir, mut lib, t) = setup();
    add_paper(&mut lib, 1, "P", None);
    let (g, t) = lib.create_group("A", &t).unwrap();
    let before = snapshot(&dir.path().join("papers"));
    lib.rename_group(&g.id, "B", &t).unwrap();
    assert_eq!(snapshot(&dir.path().join("papers")), before);
}

#[test]
fn rename_unknown_group_is_not_found() {
    let (_dir, mut lib, t) = setup();
    let r = lib.rename_group("01JCKA1B2C3D4E5F6G7H8J9K0M", "X", &t);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
}

#[test]
fn unknown_fields_survive_group_writes() {
    let (dir, mut lib, t) = setup();
    lib.create_group("A", &t).unwrap();
    let mut v = read_lib(&dir);
    v["x_future"] = json!(1);
    v["groups"][0]["x_future"] = json!(1);
    write_lib(&dir, &v);
    let (_, t) = lib.list_groups().unwrap();
    let (b, t) = lib.create_group("B", &t).unwrap();
    let after = read_lib(&dir);
    assert_eq!(after["x_future"], 1);
    assert_eq!(after["groups"][0]["x_future"], 1);
    lib.rename_group(&b.id, "B2", &t).unwrap();
    let after = read_lib(&dir);
    assert_eq!(after["x_future"], 1);
    assert_eq!(after["groups"][0]["x_future"], 1);
    // 交給畫面的定義也帶 extra
    let (list, _) = lib.list_groups().unwrap();
    assert_eq!(list[0].extra.get("x_future"), Some(&json!(1)));
}

#[test]
fn untouched_definitions_are_written_back_verbatim() {
    let (dir, mut lib, _t) = setup();
    let mut v = read_lib(&dir);
    v["groups"] = json!([{"id": "01JCKA1B2C3D4E5F6G7H8J9K0M", "name": "舊的"}]);
    write_lib(&dir, &v);
    let (_, t) = lib.list_groups().unwrap();
    lib.create_group("新的", &t).unwrap();
    let after = read_lib(&dir);
    let old = after["groups"][0].as_object().unwrap();
    assert!(!old.contains_key("created"));
    assert!(!old.contains_key("modified"));
    assert_eq!(old.len(), 2);
    assert_eq!(after["groups"][1]["name"], "新的");
}

#[test]
fn duplicate_definition_id_first_wins_and_is_kept() {
    let (dir, mut lib, _t) = setup();
    let id = "01JCKA1B2C3D4E5F6G7H8J9K0M";
    let mut v = read_lib(&dir);
    v["groups"] = json!([
        {"id": id, "name": "First"},
        {"id": id, "name": "Second"}
    ]);
    write_lib(&dir, &v);
    let (list, t) = lib.list_groups().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "First");
    lib.rename_group(id, "Renamed", &t).unwrap();
    let after = read_lib(&dir);
    assert_eq!(after["groups"].as_array().unwrap().len(), 2);
    assert_eq!(after["groups"][0]["name"], "Renamed");
    assert_eq!(after["groups"][1]["name"], "Second");
}

#[test]
fn unreadable_groups_do_not_block_open() {
    let bad_values = [
        json!("x"),
        json!([1]),
        json!([{"name": "a"}]),
        json!([{"id": "A", "name": 5}]),
    ];
    for bad in bad_values {
        let (dir, mut lib, _t) = setup();
        add_paper(&mut lib, 1, "P", None);
        drop(lib);
        let mut v = read_lib(&dir);
        v["groups"] = bad.clone();
        write_lib(&dir, &v);
        let bytes = fs::read(lib_path(&dir)).unwrap();

        let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
        assert_eq!(lib.list_papers().len(), 1, "{bad}");
        let r = lib.list_groups();
        assert!(
            matches!(r, Err(LibError::Unreadable { .. })),
            "{bad}: {r:?}"
        );
        let r = lib.create_group("A", &token_of(&lib_path(&dir)));
        assert!(
            matches!(r, Err(LibError::Unreadable { .. })),
            "{bad}: {r:?}"
        );
        assert_eq!(fs::read(lib_path(&dir)).unwrap(), bytes, "{bad}");
    }
}

#[test]
fn unreadable_deleted_groups_blocks_writes_only() {
    let (dir, lib, _t) = setup();
    drop(lib);
    let mut v = read_lib(&dir);
    v["deleted_groups"] = json!("x");
    write_lib(&dir, &v);
    let bytes = fs::read(lib_path(&dir)).unwrap();
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let (list, t) = lib.list_groups().unwrap();
    assert!(list.is_empty());
    let r = lib.create_group("A", &t);
    assert!(matches!(r, Err(LibError::Unreadable { .. })), "{r:?}");
    assert_eq!(fs::read(lib_path(&dir)).unwrap(), bytes);
}

#[test]
fn read_only_library_refuses_group_writes() {
    let (dir, mut lib, t) = setup();
    let (g, _) = lib.create_group("A", &t).unwrap();
    drop(lib);
    let mut v = read_lib(&dir);
    v["format"] = json!(2);
    write_lib(&dir, &v);
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    assert!(lib.is_read_only());
    let (list, t) = lib.list_groups().unwrap();
    assert_eq!(list.len(), 1);
    let r = lib.create_group("B", &t);
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
    let r = lib.rename_group(&g.id, "C", &t);
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
}
