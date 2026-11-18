//! 匯出同步包。

mod common;

use common::*;
use paperreader_core::changes::SYNC_IMPORTED;
use paperreader_core::error::LibError;
use paperreader_core::ids::new_id;
use paperreader_core::jsonio::{sha256_hex, to_canonical_json};
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{SyncExportReport, SyncMode};
use paperreader_core::sync::package::SyncManifest;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use zip::CompressionMethod;

fn export(lib: &mut LocalLibrary, out: &Path, mode: SyncMode) -> SyncExportReport {
    lib.export_sync_package(out, mode, Some("Pixel 8"), &mut |_, _| {})
        .unwrap()
}

fn manifest_of(zip: &Path) -> SyncManifest {
    let entries = zip_entries(zip);
    let (name, bytes, _) = entries.last().unwrap();
    assert_eq!(name, "manifest.json");
    serde_json::from_slice(bytes).unwrap()
}

fn listed(m: &SyncManifest) -> BTreeMap<String, bool> {
    m.files
        .iter()
        .map(|f| (f.path.clone(), f.included))
        .collect()
}

/// B 匯入了 A 的某一份同步包、並把這筆記錄帶回 A（直接寫進 A 的 `logs/changes-<B>.jsonl`）。
fn imported_by_b(root: &Path, a_device: &str, created: &str) {
    let mut lines = change_lines(root, DEVICE_B);
    lines.push(json!({
        "id": new_id(), "t": minutes_ago(0), "device": DEVICE_B, "paper": null,
        "type": SYNC_IMPORTED,
        "data": {"package": new_id(), "from_device": a_device, "package_created": created}
    }));
    write_change_lines(root, DEVICE_B, &lines);
}

#[test]
fn first_export_is_full() {
    let (dir, mut lib) = build_library();
    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("s.zip");
    let r = export(&mut lib, &zip, SyncMode::Changes);
    assert_eq!(r.mode, SyncMode::Full);
    assert_eq!(r.since, None);
    let m = manifest_of(&zip);
    assert_eq!(m.kind, "paperreader-sync");
    assert_eq!(m.format, 1);
    assert_eq!(m.package, r.package);
    assert_eq!(m.library_id, lib.library_file().id);
    assert_eq!(m.device, DEVICE);
    assert_eq!(m.device_label.as_deref(), Some("Pixel 8"));
    assert_eq!(m.mode, SyncMode::Full);
    assert!(m.files.iter().all(|f| f.included));
    assert_eq!(r.listed, m.files.len());
    assert_eq!(r.included, m.files.len());
    let entries = zip_entries(&zip);
    let names: Vec<String> = entries.iter().map(|e| e.0.clone()).collect();
    let mut want: Vec<String> = m
        .files
        .iter()
        .map(|f| format!("library/{}", f.path))
        .collect();
    want.push("manifest.json".into());
    assert_eq!(names, want);
    for (name, bytes, method) in &entries {
        assert_eq!(*method, CompressionMethod::Stored);
        if let Some(rel) = name.strip_prefix("library/") {
            let f = m.files.iter().find(|f| f.path == rel).unwrap();
            assert_eq!(f.bytes, Some(bytes.len() as u64));
            assert_eq!(f.sha256.as_deref(), Some(sha256_hex(bytes).as_str()));
        }
    }
    drop(dir);
}

#[test]
fn syncable_files_only() {
    let (dir, mut lib) = build_library();
    let root = dir.path();
    // 加筆跡與譯文疊加檔
    let p1 = lib
        .list_papers()
        .into_iter()
        .find(|p| p.bib.title.as_deref() == Some("Alpha Paper"))
        .unwrap();
    let m1 = p1.materials[0].id.clone();
    lib.add_stroke(&p1.id, stroke_draft(&m1, 0), None).unwrap();
    let tok = lib.get_paper(&p1.id).unwrap().1;
    lib.create_overlay(
        &p1.id,
        paperreader_core::overlay::OverlayDraft {
            source_material: m1,
            provider: "x".into(),
            model: "y".into(),
            source_lang: None,
            target_lang: "zh-TW".into(),
            segments: vec![],
        },
        &tok,
    )
    .unwrap();
    // 不該同步的東西
    fs::write(root.join("papers/Alpha Paper/mine.txt"), "x").unwrap();
    fs::create_dir_all(root.join(".obsidian")).unwrap();
    fs::write(root.join(".obsidian/x"), "x").unwrap();
    fs::write(root.join("papers/Alpha Paper/.pr-tmp-x"), "x").unwrap();
    fs::create_dir_all(root.join("jobs")).unwrap();
    fs::write(root.join("jobs/x"), "x").unwrap();
    fs::write(root.join("stray.txt"), "x").unwrap();
    fs::write(root.join("notes/plain.md"), "# my own note\n").unwrap();
    // 別台的紀錄檔
    write_change_lines(root, DEVICE_B, &[]);
    fs::write(changes_path(root, DEVICE_B), "").unwrap();

    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("s.zip");
    export(&mut lib, &zip, SyncMode::Full);
    let files = listed(&manifest_of(&zip));
    for bad in [
        "papers/Alpha Paper/mine.txt",
        ".obsidian/x",
        "papers/Alpha Paper/.pr-tmp-x",
        "jobs/x",
        "stray.txt",
        "notes/plain.md",
    ] {
        assert!(!files.contains_key(bad), "{bad}");
    }
    let trash_folder = lib.list_trash().unwrap()[0].folder.clone();
    for good in [
        "library.json".to_string(),
        "papers/Alpha Paper/paper.json".into(),
        "papers/Alpha Paper/source.pdf".into(),
        "papers/Alpha Paper/translation.pdf".into(),
        "papers/Alpha Paper/ink.json".into(),
        "papers/Alpha Paper/translation-overlay.json".into(),
        format!(".trash/{trash_folder}/paper.json"),
        format!(".trash/{trash_folder}/source.pdf"),
        "notes/Alpha Paper.md".into(),
        format!("logs/changes-{DEVICE}.jsonl"),
        format!("logs/changes-{DEVICE_B}.jsonl"),
    ] {
        assert!(files.contains_key(&good), "{good} in {files:?}");
    }
    assert!(files.keys().any(|k| k.starts_with("logs/reading-")));
}

/// 把一篇文獻每項材料的 `provenance.time` 改成很早以前（指紋不變）。
fn age_materials(root: &Path, folder: &str) {
    let path = root.join("papers").join(folder).join("paper.json");
    let mut p: paperreader_core::format::PaperFile =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for m in &mut p.materials {
        m.provenance.time = Some("2026-01-01T00:00:00.000Z".into());
    }
    fs::write(&path, to_canonical_json(&p).unwrap()).unwrap();
}

#[test]
fn changes_mode_omits_old_materials() {
    let (dir, mut lib) = build_library();
    let root = dir.path().to_path_buf();
    age_materials(&root, "Alpha Paper");
    age_materials(&root, "Beta Paper");
    lib.rescan().unwrap();
    let created = minutes_ago(1);
    imported_by_b(&root, DEVICE, &created);
    // 之後：加一篇，對舊的一篇加譯本
    add_paper(&mut lib, 50, "New Paper", None);
    let beta = lib
        .list_papers()
        .into_iter()
        .find(|p| p.bib.title.as_deref() == Some("Beta Paper"))
        .unwrap();
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf_seeded(src.path(), "t.pdf", 400, 77);
    let tok = lib.get_paper(&beta.id).unwrap().1;
    lib.add_translation(&beta.id, &pdf, None, &tok).unwrap();

    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("s.zip");
    let r = export(&mut lib, &zip, SyncMode::Changes);
    assert_eq!(r.mode, SyncMode::Changes);
    assert_eq!(r.since.as_deref(), Some(created.as_str()));
    let m = manifest_of(&zip);
    let files = listed(&m);
    assert!(files["papers/New Paper/source.pdf"]);
    assert!(files["papers/Beta Paper/translation.pdf"]);
    assert!(!files["papers/Alpha Paper/source.pdf"]);
    assert!(!files["papers/Beta Paper/source.pdf"]);
    let alpha: paperreader_core::format::PaperFile =
        serde_json::from_slice(&fs::read(root.join("papers/Alpha Paper/paper.json")).unwrap())
            .unwrap();
    let omitted = m
        .files
        .iter()
        .find(|f| f.path == "papers/Alpha Paper/source.pdf")
        .unwrap();
    assert_eq!(omitted.bytes, alpha.materials[0].bytes);
    assert_eq!(omitted.sha256, alpha.materials[0].sha256);
    for f in &m.files {
        if f.path.ends_with(".json") || f.path.ends_with(".md") || f.path.ends_with(".jsonl") {
            assert!(f.included, "{}", f.path);
        }
    }
    let names: Vec<String> = zip_entries(&zip).into_iter().map(|e| e.0).collect();
    assert!(!names.contains(&"library/papers/Alpha Paper/source.pdf".to_string()));
}

#[test]
fn since_ignores_undelivered_exports() {
    let (dir, mut lib) = build_library();
    let out = tempfile::tempdir().unwrap();
    let r1 = export(&mut lib, &out.path().join("1.zip"), SyncMode::Changes);
    let r2 = export(&mut lib, &out.path().join("2.zip"), SyncMode::Changes);
    assert_eq!(r1.mode, SyncMode::Full);
    assert_eq!(r2.mode, SyncMode::Full, "B 沒匯入過：仍是完整的");
    let first_created = manifest_of(&out.path().join("1.zip")).created;
    imported_by_b(dir.path(), DEVICE, &first_created);
    let r3 = export(&mut lib, &out.path().join("3.zip"), SyncMode::Changes);
    assert_eq!(r3.mode, SyncMode::Changes);
    assert_eq!(r3.since.as_deref(), Some(first_created.as_str()));
    // 別台裝置自己的記錄（from_device 不是這台）不算
    imported_by_b(dir.path(), DEVICE_B, &minutes_ago(-5));
    let r4 = export(&mut lib, &out.path().join("4.zip"), SyncMode::Changes);
    assert_eq!(r4.since.as_deref(), Some(first_created.as_str()));
}

#[test]
fn export_writes_no_export_record_and_changes_only_changes_file() {
    let (dir, mut lib) = build_library();
    let p1 = lib.list_papers()[0].id.clone();
    let nt = lib.read_notes(&p1).unwrap().1;
    lib.set_thoughts(&p1, "an idea", nt.as_ref()).unwrap();
    // 讓這份內容沒有節點：拿掉這台的想法記錄（像是在筆記工具裡改過）
    write_change_lines(dir.path(), DEVICE, &[]);
    let before = snapshot(dir.path());
    let out = tempfile::tempdir().unwrap();
    export(&mut lib, &out.path().join("s.zip"), SyncMode::Full);
    let after = snapshot(dir.path());
    let own = format!("logs/changes-{DEVICE}.jsonl");
    for (k, v) in &after {
        if *k != own {
            assert_eq!(before.get(k), Some(v), "{k}");
        }
    }
    assert_eq!(before.len(), after.len());
    let lines = change_lines(dir.path(), DEVICE);
    assert_eq!(lines.len(), 1, "只有補記的想法");
    assert_eq!(lines[0]["type"], json!("thoughts-written"));
    // 再匯出一次：已有節點，不再補記
    export(&mut lib, &out.path().join("t.zip"), SyncMode::Full);
    assert_eq!(change_lines(dir.path(), DEVICE).len(), 1);
}

#[test]
fn thoughts_catch_up_on_export() {
    let (dir, mut lib) = temp_library();
    let (p, _) = add_paper(&mut lib, 1, "Alpha", None);
    let mut t = lib.set_thoughts(&p, "a", None).unwrap();
    t = lib.set_thoughts(&p, "ab", Some(&t)).unwrap(); // 節流：不記
    let _ = t;
    let out = tempfile::tempdir().unwrap();
    export(&mut lib, &out.path().join("s.zip"), SyncMode::Full);
    let lines = change_lines(dir.path(), DEVICE);
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[1]["data"]["sha256"],
        json!(paperreader_core::changes::thoughts_hash("ab"))
    );
    assert_eq!(lines[1]["data"]["prev"], json!([lines[0]["id"]]));
}

#[test]
fn export_refuses_existing_target_and_inside_library() {
    let (dir, mut lib) = build_library();
    let out = tempfile::tempdir().unwrap();
    let existing = out.path().join("x.zip");
    fs::write(&existing, "x").unwrap();
    assert!(matches!(
        lib.export_sync_package(&existing, SyncMode::Full, None, &mut |_, _| {}),
        Err(LibError::InvalidInput { .. })
    ));
    let inside = dir.path().join("s.zip");
    assert!(matches!(
        lib.export_sync_package(&inside, SyncMode::Full, None, &mut |_, _| {}),
        Err(LibError::InvalidInput { .. })
    ));
    assert!(!inside.exists());
}

#[test]
fn export_missing_parent_is_not_found() {
    let (_dir, mut lib) = build_library();
    let out = tempfile::tempdir().unwrap();
    let r = lib.export_sync_package(
        &out.path().join("nope").join("s.zip"),
        SyncMode::Full,
        None,
        &mut |_, _| {},
    );
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
}

#[test]
fn read_only_library_refused() {
    let (dir, lib) = build_library();
    drop(lib);
    let path = dir.path().join("library.json");
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    v["format"] = json!(2);
    fs::write(&path, v.to_string()).unwrap();
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let out = tempfile::tempdir().unwrap();
    let r = lib.export_sync_package(
        &out.path().join("s.zip"),
        SyncMode::Full,
        None,
        &mut |_, _| {},
    );
    assert!(matches!(r, Err(LibError::ReadOnly)));
}

fn last_syncable(root: &Path) -> PathBuf {
    // 依路徑排序最後的檔：papers/ 底下的某個材料
    let mut v: Vec<String> = snapshot(&root.join("papers"))
        .into_iter()
        .filter(|(_, v)| v.is_some())
        .map(|(k, _)| k)
        .collect();
    v.sort();
    root.join("papers").join(v.last().unwrap())
}

#[test]
fn vanished_file_fails_and_leaves_nothing() {
    let (dir, mut lib) = build_library();
    let victim = last_syncable(dir.path());
    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("s.zip");
    let mut first = true;
    let r = lib.export_sync_package(&zip, SyncMode::Full, None, &mut |_, _| {
        if first {
            first = false;
            fs::remove_file(&victim).unwrap();
        }
    });
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert!(!zip.exists());
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn leftover_syncout_temp_is_cleaned() {
    let (_dir, mut lib) = build_library();
    let out = tempfile::tempdir().unwrap();
    let good = format!(".pr-syncout-{}.zip", new_id());
    fs::write(out.path().join(&good), "old").unwrap();
    fs::write(out.path().join(".pr-syncout-foo.zip"), "keep").unwrap();
    export(&mut lib, &out.path().join("s.zip"), SyncMode::Full);
    assert!(!out.path().join(&good).exists());
    assert!(out.path().join(".pr-syncout-foo.zip").exists());
    assert!(out.path().join("s.zip").exists());
}

#[cfg(unix)]
#[test]
fn classify_blocks_unsafe_paths() {
    let (dir, mut lib) = build_library();
    // 名稱含 `:` 的材料：路徑不安全，被擋下、列在 skipped
    let p = lib.list_papers()[0].clone();
    let folder = dir.path().join("papers").join(&p.folder);
    fs::write(folder.join("a:b.pdf"), "x").unwrap();
    let path = folder.join("paper.json");
    let mut paper: paperreader_core::format::PaperFile =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut m = paper.materials[0].clone();
    m.id = new_id();
    m.path = "a:b.pdf".into();
    paper.materials.push(m);
    fs::write(&path, to_canonical_json(&paper).unwrap()).unwrap();
    lib.rescan().unwrap();
    let out = tempfile::tempdir().unwrap();
    let r = export(&mut lib, &out.path().join("s.zip"), SyncMode::Full);
    assert!(r.skipped.iter().any(|s| s.reason == "unsafe-path"));
}
