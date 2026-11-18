mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::*;
use paperreader_core::jsonio::{read_json, to_canonical_json};
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::MaterialStatus;
use std::fs;

fn open(dir: &tempfile::TempDir) -> LocalLibrary {
    LocalLibrary::open(dir.path(), DEVICE).unwrap()
}

#[test]
fn open_creates_library_json() {
    let (dir, lib) = temp_library();
    let (f, _): (LibraryFile, _) = read_json(&dir.path().join("library.json")).unwrap();
    assert_eq!(f.format, 1);
    assert_eq!(f.notes_dir, "notes");
    assert_eq!(f.id.len(), 26);
    assert_eq!(lib.library_file().id, f.id);
    assert!(!lib.is_read_only());
    // 新建的文獻庫寫兩個空陣列
    assert_eq!(f.groups, serde_json::json!([]));
    assert_eq!(f.deleted_groups, serde_json::json!([]));
}

#[test]
fn old_library_json_without_groups_opens() {
    let dir = tempfile::tempdir().unwrap();
    let text = "{\n  \"format\": 1,\n  \"id\": \"01JCK3V7Q8Z9X5M2N4P6R8T0AB\",\n  \"created\": \"2026-11-13T08:25:02.123Z\",\n  \"notes_dir\": \"notes\"\n}\n";
    fs::write(dir.path().join("library.json"), text).unwrap();
    let lib = open(&dir);
    // 開啟時不寫回
    assert_eq!(
        fs::read_to_string(dir.path().join("library.json")).unwrap(),
        text
    );
    let (groups, _) = lib.list_groups().unwrap();
    assert!(groups.is_empty());
}

#[test]
fn reopen_keeps_library_id() {
    let (dir, lib) = temp_library();
    let id = lib.library_file().id.clone();
    drop(lib);
    assert_eq!(open(&dir).library_file().id, id);
}

#[test]
fn open_creates_missing_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("a").join("b").join("PaperReader");
    let lib = LocalLibrary::open(&root, DEVICE).unwrap();
    assert!(root.join("library.json").is_file());
    assert!(lib.list_papers().is_empty());
}

#[test]
fn newer_library_format_opens_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let text = "{\n  \"format\": 2,\n  \"id\": \"01JCK3V7Q8Z9X5M2N4P6R8T0AB\",\n  \"created\": \"2026-11-13T08:25:02.123Z\",\n  \"notes_dir\": \"notes\",\n  \"x_future\": true\n}\n";
    fs::write(dir.path().join("library.json"), text).unwrap();
    let lib = open(&dir);
    assert!(lib.is_read_only());
    assert_eq!(
        fs::read_to_string(dir.path().join("library.json")).unwrap(),
        text
    );
}

#[test]
fn unreadable_library_json_is_error_and_untouched() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("library.json"), "not json at all").unwrap();
    let r = LocalLibrary::open(dir.path(), DEVICE);
    assert!(matches!(r, Err(LibError::Unreadable { .. })));
    assert_eq!(
        fs::read_to_string(dir.path().join("library.json")).unwrap(),
        "not json at all"
    );
}

#[test]
fn open_cleans_own_temp_items() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("papers/.pr-adding-X")).unwrap();
    fs::write(root.join("papers/.pr-adding-X/source.pdf"), b"half").unwrap();
    write_paper(root, "A", &sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA"));
    fs::write(root.join("papers/A/.pr-tmp-Y"), b"x").unwrap();
    fs::write(root.join(".pr-tmp-Z"), b"x").unwrap();
    let lib = open(&dir);
    assert!(!root.join("papers/.pr-adding-X").exists());
    assert!(!root.join("papers/A/.pr-tmp-Y").exists());
    assert!(!root.join(".pr-tmp-Z").exists());
    assert!(root.join("papers/A/paper.json").exists());
    assert_eq!(lib.list_papers().len(), 1);
}

#[test]
fn open_cleans_each_interrupted_add_state() {
    for state in 0..4 {
        let dir = tempfile::tempdir().unwrap();
        let adding = dir.path().join("papers/.pr-adding-X");
        fs::create_dir_all(&adding).unwrap();
        match state {
            0 => {}
            1 => fs::write(adding.join("source.pdf"), fake_bytes(1, 100)).unwrap(),
            2 => fs::write(adding.join("source.pdf"), fake_bytes(1, 1000)).unwrap(),
            _ => {
                fs::write(adding.join("source.pdf"), fake_bytes(1, 1000)).unwrap();
                fs::write(
                    adding.join("paper.json"),
                    to_canonical_json(&sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA")).unwrap(),
                )
                .unwrap();
            }
        }
        let lib = open(&dir);
        assert!(!adding.exists(), "state {state}");
        assert!(lib.list_papers().is_empty(), "state {state}");
    }
}

#[test]
fn read_only_open_does_not_clean() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("library.json"),
        "{\"format\":2,\"id\":\"01JCK3V7Q8Z9X5M2N4P6R8T0AB\"}",
    )
    .unwrap();
    fs::write(dir.path().join(".pr-tmp-Z"), b"x").unwrap();
    fs::create_dir_all(dir.path().join("papers/.pr-adding-X")).unwrap();
    let lib = open(&dir);
    assert!(lib.is_read_only());
    assert!(dir.path().join(".pr-tmp-Z").exists());
    assert!(dir.path().join("papers/.pr-adding-X").exists());
}

#[test]
fn foreign_hidden_items_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".stfolder")).unwrap();
    fs::create_dir_all(root.join(".obsidian")).unwrap();
    fs::write(root.join(".obsidian/app.json"), b"{\"a\":1}").unwrap();
    let a = write_paper(root, "A", &sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA"));
    fs::write(a.join(".DS_Store"), b"\x00\x01binary").unwrap();
    let before = hidden_part(&snapshot(root));
    assert!(before.len() >= 4);

    let mut lib = open(&dir);
    lib.rescan().unwrap();
    assert_eq!(hidden_part(&snapshot(root)), before);
    assert_eq!(lib.list_papers().len(), 1);
}

#[test]
fn folder_without_paper_json_is_skipped() {
    let (dir, mut lib) = temp_library();
    fs::create_dir_all(dir.path().join("papers/Empty")).unwrap();
    fs::write(dir.path().join("papers/Empty/source.pdf"), b"x").unwrap();
    lib.rescan().unwrap();
    assert!(lib.list_papers().is_empty());
}

#[test]
fn unreadable_paper_json_not_listed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let good = write_paper(root, "Good", &sample_paper("01GGGGGGGGGGGGGGGGGGGGGGGG"));
    assert!(good.exists());
    for (name, text) in [
        ("BadJson", "{ nope".to_string()),
        (
            "NoId",
            "{\"format\":1,\"bib\":{\"title\":\"x\"}}".to_string(),
        ),
        (
            "BadYear",
            to_canonical_json(&sample_paper("01BBBBBBBBBBBBBBBBBBBBBBBB"))
                .unwrap()
                .replace("\"year\": 2024", "\"year\": \"2024\""),
        ),
    ] {
        let d = root.join("papers").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("paper.json"), text).unwrap();
    }
    let lib = open(&dir);
    let list = lib.list_papers();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].folder, "Good");
}

#[test]
fn newer_paper_format_listed_flagged() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA");
    p.format = 2;
    write_paper(dir.path(), "A", &p);
    write_paper(dir.path(), "B", &sample_paper("01BBBBBBBBBBBBBBBBBBBBBBBB"));
    let lib = open(&dir);
    let list = lib.list_papers();
    assert_eq!(list.len(), 2);
    assert!(list[0].needs_newer_app);
    assert!(!list[1].needs_newer_app);
}

#[test]
fn duplicate_id_first_folder_wins() {
    let dir = tempfile::tempdir().unwrap();
    let p = sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA");
    write_paper(dir.path(), "B", &p);
    write_paper(dir.path(), "A", &p);
    let lib = open(&dir);
    let list = lib.list_papers();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].folder, "A");
}

#[test]
fn nested_folders_not_scanned() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("papers/A/sub");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        nested.join("paper.json"),
        to_canonical_json(&sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA")).unwrap(),
    )
    .unwrap();
    let lib = open(&dir);
    assert!(lib.list_papers().is_empty());
}

#[test]
fn list_is_sorted_by_folder_name() {
    let dir = tempfile::tempdir().unwrap();
    write_paper(
        dir.path(),
        "Zed",
        &sample_paper("01ZZZZZZZZZZZZZZZZZZZZZZZZ"),
    );
    write_paper(
        dir.path(),
        "Alpha",
        &sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA"),
    );
    let lib = open(&dir);
    let names: Vec<_> = lib.list_papers().into_iter().map(|p| p.folder).collect();
    assert_eq!(names, ["Alpha", "Zed"]);
}

fn paper_with_materials() -> PaperFile {
    let mut p = sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA");
    p.materials = vec![
        source_material("ok.pdf", Some(10), None),
        source_material("gone.pdf", None, None),
        source_material("short.pdf", Some(999), None),
        source_material("../x.pdf", None, None),
        source_material("/etc/passwd", None, None),
        source_material("C:\\x.pdf", None, None),
        source_material("@src/x.pdf", None, None),
    ];
    p
}

#[test]
fn material_statuses() {
    let dir = tempfile::tempdir().unwrap();
    let folder = write_paper(dir.path(), "A", &paper_with_materials());
    fs::write(folder.join("ok.pdf"), fake_bytes(1, 10)).unwrap();
    fs::write(folder.join("short.pdf"), fake_bytes(2, 5)).unwrap();
    let lib = open(&dir);
    let st: Vec<_> = lib.list_papers()[0]
        .materials
        .iter()
        .map(|m| m.status)
        .collect();
    use MaterialStatus::*;
    assert_eq!(
        st,
        [
            Ok,
            Missing,
            SizeMismatch,
            InvalidPath,
            InvalidPath,
            InvalidPath,
            InvalidPath
        ]
    );
}

#[test]
fn material_path_refuses_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let p = paper_with_materials();
    let folder = write_paper(dir.path(), "A", &p);
    fs::write(folder.join("ok.pdf"), fake_bytes(1, 10)).unwrap();
    let lib = open(&dir);
    let id = &p.id;
    assert_eq!(
        lib.material_path(id, &p.materials[0].id).unwrap(),
        folder.join("ok.pdf")
    );
    assert!(matches!(
        lib.material_path(id, &p.materials[1].id),
        Err(LibError::NotFound { .. })
    ));
    for i in 3..7 {
        assert!(matches!(
            lib.material_path(id, &p.materials[i].id),
            Err(LibError::InvalidInput { .. })
        ));
    }
    assert!(matches!(
        lib.material_path(id, "nope"),
        Err(LibError::NotFound { .. })
    ));
    assert!(matches!(
        lib.material_path("nope", "nope"),
        Err(LibError::NotFound { .. })
    ));
}

#[test]
fn get_paper_reads_disk_and_returns_token() {
    let dir = tempfile::tempdir().unwrap();
    let p = sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA");
    write_paper(dir.path(), "A", &p);
    let lib = open(&dir);
    let (got, token) = lib.get_paper(&p.id).unwrap();
    assert_eq!(got, p);
    let (_, t2): (PaperFile, _) = read_json(&dir.path().join("papers/A/paper.json")).unwrap();
    assert_eq!(token, t2);
    assert!(matches!(
        lib.get_paper("nope"),
        Err(LibError::NotFound { .. })
    ));
}

#[test]
fn rescan_unchanged_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    write_paper(dir.path(), "A", &sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA"));
    let mut lib = open(&dir);
    let d = lib.rescan().unwrap();
    assert!(d.added.is_empty() && d.changed.is_empty() && d.removed.is_empty());
}

#[test]
fn rescan_sees_external_title_edit() {
    let dir = tempfile::tempdir().unwrap();
    let p = sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA");
    write_paper(dir.path(), "A", &p);
    let mut lib = open(&dir);
    let mut edited = p.clone();
    edited.bib.title = Some("A much longer externally edited title".into());
    write_paper(dir.path(), "A", &edited);
    let diff = lib.rescan().unwrap();
    assert_eq!(diff.changed, vec![p.id.clone()]);
    assert!(diff.added.is_empty() && diff.removed.is_empty());
    assert_eq!(
        lib.list_papers()[0].bib.title.as_deref(),
        Some("A much longer externally edited title")
    );
}

#[test]
fn rescan_sees_folder_rename() {
    let dir = tempfile::tempdir().unwrap();
    let p = sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA");
    write_paper(dir.path(), "A", &p);
    let mut lib = open(&dir);
    fs::rename(
        dir.path().join("papers/A"),
        dir.path().join("papers/Renamed"),
    )
    .unwrap();
    let diff = lib.rescan().unwrap();
    assert_eq!(diff.changed, vec![p.id.clone()]);
    assert!(diff.added.is_empty() && diff.removed.is_empty());
    let list = lib.list_papers();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, p.id);
    assert_eq!(list[0].folder, "Renamed");
}

#[test]
fn rescan_sees_added_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    write_paper(dir.path(), "A", &sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA"));
    let mut lib = open(&dir);
    write_paper(dir.path(), "B", &sample_paper("01BBBBBBBBBBBBBBBBBBBBBBBB"));
    fs::remove_dir_all(dir.path().join("papers/A")).unwrap();
    let diff = lib.rescan().unwrap();
    assert_eq!(diff.added, ["01BBBBBBBBBBBBBBBBBBBBBBBB"]);
    assert_eq!(diff.removed, ["01AAAAAAAAAAAAAAAAAAAAAAAA"]);
    assert!(diff.changed.is_empty());
    assert_eq!(lib.list_papers().len(), 1);
}

#[test]
fn rescan_sees_same_size_edit() {
    // 篇名換成等長的字串：檔案大小不變，只靠修改時間看出來
    let dir = tempfile::tempdir().unwrap();
    let mut p = sample_paper("01AAAAAAAAAAAAAAAAAAAAAAAA");
    p.bib.title = Some("Title AAA".into());
    write_paper(dir.path(), "A", &p);
    let path = dir.path().join("papers/A/paper.json");
    let mut lib = open(&dir);
    let size_before = fs::metadata(&path).unwrap().len();

    // 等到修改時間必然不同（不依賴檔案系統的時間解析度）
    std::thread::sleep(std::time::Duration::from_millis(2100));
    p.bib.title = Some("Title BBB".into());
    write_paper(dir.path(), "A", &p);
    assert_eq!(
        fs::metadata(&path).unwrap().len(),
        size_before,
        "測試前提：大小不變"
    );

    let diff = lib.rescan().unwrap();
    assert_eq!(diff.changed, vec![p.id.clone()]);
    assert_eq!(lib.list_papers()[0].bib.title.as_deref(), Some("Title BBB"));
}
