//! 傳輸接口與資料夾傳輸。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::store::SyncMode;
use paperreader_core::sync::transport::{FolderTransport, SyncTransport};
use std::fs;

#[test]
fn folder_list_only_zip_files() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("b.zip"), "b").unwrap();
    fs::write(d.path().join("a.zip"), "aa").unwrap();
    fs::write(d.path().join(".pr-put-x.zip"), "x").unwrap();
    fs::write(d.path().join(".hidden.zip"), "x").unwrap();
    fs::write(d.path().join("x.txt"), "x").unwrap();
    fs::create_dir(d.path().join("dir.zip")).unwrap();
    let mut t = FolderTransport::new(d.path());
    let list = t.list().unwrap();
    let names: Vec<&str> = list.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["a.zip", "b.zip"]);
    assert_eq!(list[0].bytes, Some(2));
    assert!(list[0].modified.is_some());
}

#[test]
fn put_then_fetch_roundtrip_and_replace_same_name() {
    let d = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let src = work.path().join("s.zip");
    fs::write(&src, "first").unwrap();
    let mut t = FolderTransport::new(d.path());
    let r = t.put(&src, "pkg.zip").unwrap();
    assert_eq!(r.name, "pkg.zip");
    fs::write(&src, "second").unwrap();
    t.put(&src, "pkg.zip").unwrap();
    // 成功之後沒有暫存檔
    let names: Vec<String> = fs::read_dir(d.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["pkg.zip"]);
    let dest = work.path().join("got.zip");
    let list = t.list().unwrap();
    t.fetch(&list[0], &dest).unwrap();
    assert_eq!(fs::read_to_string(&dest).unwrap(), "second");
}

#[test]
fn fetch_refuses_existing_dest_and_missing_is_not_found() {
    let d = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    fs::write(d.path().join("p.zip"), "x").unwrap();
    let mut t = FolderTransport::new(d.path());
    let p = t.list().unwrap().remove(0);
    let dest = work.path().join("exists.zip");
    fs::write(&dest, "keep").unwrap();
    assert!(matches!(
        t.fetch(&p, &dest),
        Err(LibError::InvalidInput { .. })
    ));
    assert_eq!(fs::read_to_string(&dest).unwrap(), "keep");
    fs::remove_file(d.path().join("p.zip")).unwrap();
    assert!(matches!(
        t.fetch(&p, &work.path().join("new.zip")),
        Err(LibError::NotFound { .. })
    ));
}

#[test]
fn end_to_end_via_folder() {
    let mut p = pair();
    add_paper(&mut p.a, 777, "Via folder", None);
    let shared = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let pkg = work.path().join("a.zip");
    p.a.export_sync_package(&pkg, SyncMode::Changes, None, &mut |_, _| {})
        .unwrap();
    let name = format!("paperreader-sync-{}-{DEVICE}.zip", p.a.library_file().id);
    let mut ta = FolderTransport::new(shared.path());
    ta.put(&pkg, &name).unwrap();
    let mut tb = FolderTransport::new(shared.path());
    let list = tb.list().unwrap();
    let got = work.path().join("b.zip");
    tb.fetch(&list[0], &got).unwrap();
    let preview = p.b.prepare_sync_import(&got, &mut |_, _| {}).unwrap();
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert!(
        p.b.list_papers()
            .iter()
            .any(|x| x.bib.title.as_deref() == Some("Via folder"))
    );
}
