//! 工作 7：匯出備份檔。

mod common;

use common::*;
use paperreader_core::backup::{
    BackupManifest, LIBRARY_PREFIX, MANIFEST_NAME, classify_backup_paths, is_safe_backup_path,
};
use paperreader_core::error::LibError;
use paperreader_core::jsonio::sha256_hex;
use paperreader_core::local::LocalLibrary;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use zip::CompressionMethod;

fn export(lib: &LocalLibrary, out: &Path) -> paperreader_core::store::ExportReport {
    lib.export_backup(out, &mut |_, _| {}).unwrap()
}

#[test]
fn export_contains_every_file_and_manifest() {
    let (dir, lib) = build_library();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    let report = export(&lib, &out);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert!(report.changed_during_export.is_empty());

    let entries = zip_entries(&out);
    let want = backup_worthy_files(dir.path());
    // manifest.json 是最後一項
    assert_eq!(entries.last().unwrap().0, MANIFEST_NAME);
    let files: Vec<&(String, Vec<u8>, CompressionMethod)> =
        entries[..entries.len() - 1].iter().collect();
    let got: BTreeMap<String, Vec<u8>> = files
        .iter()
        .map(|(n, b, _)| {
            (
                n.strip_prefix(LIBRARY_PREFIX).unwrap().to_string(),
                b.clone(),
            )
        })
        .collect();
    assert_eq!(got, want);
    assert_eq!(report.files, want.len());
    assert_eq!(
        report.bytes,
        want.values().map(|v| v.len() as u64).sum::<u64>()
    );

    let manifest: BackupManifest = serde_json::from_slice(&entries.last().unwrap().1).unwrap();
    assert_eq!(manifest.format, 1);
    assert_eq!(manifest.kind, "paperreader-backup");
    assert_eq!(manifest.library_id, lib.library_file().id);
    assert!(manifest.created.ends_with('Z'));
    // files 依路徑排序，zip 裡的順序與備份清單相同
    let paths: Vec<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(paths, sorted);
    let zip_order: Vec<&str> = files
        .iter()
        .map(|(n, _, _)| n.strip_prefix(LIBRARY_PREFIX).unwrap())
        .collect();
    assert_eq!(paths, zip_order);
    for f in &manifest.files {
        let content = &got[&f.path];
        assert_eq!(f.bytes, content.len() as u64, "{}", f.path);
        assert_eq!(f.sha256, sha256_hex(content), "{}", f.path);
    }
    // 回收區、筆記、紀錄檔都在
    assert!(paths.iter().any(|p| p.starts_with(".trash/")));
    assert!(paths.iter().any(|p| p.starts_with("notes/")));
    assert!(paths.iter().any(|p| p.starts_with("logs/")));
    assert!(paths.contains(&"library.json"));
}

#[test]
fn export_uses_stored_only() {
    let (_dir, lib) = build_library();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    export(&lib, &out);
    for (name, _, method) in zip_entries(&out) {
        assert_eq!(method, CompressionMethod::Stored, "{name}");
    }
}

#[test]
fn export_excludes_temp_and_foreign_hidden() {
    let (dir, lib) = build_library();
    let a = dir.path().join("papers/Alpha Paper");
    fs::write(a.join(".pr-tmp-X"), "tmp").unwrap();
    fs::write(a.join(".DS_Store"), "ds").unwrap();
    fs::create_dir_all(dir.path().join(".obsidian")).unwrap();
    fs::write(dir.path().join(".obsidian/app.json"), "{}").unwrap();
    fs::write(dir.path().join(".stfolder"), "").unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    let report = export(&lib, &out);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let names: Vec<String> = zip_entries(&out).into_iter().map(|(n, _, _)| n).collect();
    for n in &names {
        assert!(!n.contains(".pr-tmp-X"), "{n}");
        assert!(!n.contains(".DS_Store"), "{n}");
        assert!(!n.contains(".obsidian"), "{n}");
        assert!(!n.contains(".stfolder"), "{n}");
    }
    assert!(names.iter().any(|n| n.starts_with("library/.trash/")));
}

#[test]
fn export_cjk_names_marked_utf8() {
    let (dir, mut lib) = temp_library();
    add_paper(&mut lib, 1, "論文 測試", None);
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    export(&lib, &out);
    let want = "library/papers/論文 測試/source.pdf";
    let entries = zip_entries(&out);
    assert!(entries.iter().any(|(n, _, _)| n == want), "{entries:?}");
    // 通用旗標第 11 位元（UTF-8 檔名）：在中央目錄裡找這個名稱的標頭
    let bytes = fs::read(&out).unwrap();
    let name = want.as_bytes();
    let mut found_central = false;
    let mut pos = 0;
    while let Some(i) = bytes[pos..].windows(name.len()).position(|w| w == name) {
        let at = pos + i;
        if at >= 46 && &bytes[at - 46..at - 42] == b"PK\x01\x02" {
            let flags = u16::from_le_bytes([bytes[at - 46 + 8], bytes[at - 46 + 9]]);
            assert!(flags & 0x0800 != 0, "UTF-8 flag not set");
            found_central = true;
        }
        pos = at + 1;
    }
    assert!(found_central);
    let _ = dir;
}

#[test]
fn classify_backup_paths_cases() {
    let mut paths: Vec<String> = [
        "notes/Meeting 10:30.md",
        "papers/A/a\\b.txt",
        "papers/A/A.pdf",
        "papers/A/a.pdf",
        "papers/B/ok.pdf",
        "library.json",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    paths.push(format!("notes/{}", "x".repeat(1100)));
    paths.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    let (keep, skipped) = classify_backup_paths(&paths);
    assert_eq!(keep, ["library.json", "papers/A/A.pdf", "papers/B/ok.pdf"]);
    let reason = |p: &str| {
        skipped
            .iter()
            .find(|s| s.path == p)
            .map(|s| s.reason.clone())
            .unwrap_or_default()
    };
    assert_eq!(reason("notes/Meeting 10:30.md"), "unsafe-path");
    assert_eq!(reason("papers/A/a\\b.txt"), "unsafe-path");
    assert_eq!(reason("papers/A/a.pdf"), "case-duplicate");
    assert_eq!(skipped.len(), 4);
    assert!(
        skipped
            .iter()
            .any(|s| s.reason == "unsafe-path" && s.path.len() > 1024)
    );

    for ok in [
        "a",
        "a/b",
        "papers/中文 資料夾/source.pdf",
        ".trash/20261113T083011Z-X/paper.json",
    ] {
        assert!(is_safe_backup_path(ok), "{ok}");
    }
    for bad in [
        "",
        "/abs",
        "a//b",
        "a/./b",
        "a/../b",
        "..",
        "a\\b",
        "C:/x",
        "a/b:c",
        "a/.pr-x/b",
        "a\0b",
        "a/",
    ] {
        assert!(!is_safe_backup_path(bad), "{bad:?}");
    }
}

#[cfg(unix)]
#[test]
fn symlink_is_skipped_and_reported() {
    let (dir, lib) = build_library();
    std::os::unix::fs::symlink("/etc/passwd", dir.path().join("notes/link.md")).unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    let report = export(&lib, &out);
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.path == "notes/link.md" && s.reason == "symlink"),
        "{:?}",
        report.skipped
    );
    assert!(
        !zip_entries(&out)
            .iter()
            .any(|(n, _, _)| n.contains("link.md"))
    );
}

#[test]
fn export_refuses_existing_target_and_target_inside_library() {
    let (dir, lib) = build_library();
    let before = snapshot(dir.path());
    let out_dir = tempfile::tempdir().unwrap();
    let existing = out_dir.path().join("exists.zip");
    fs::write(&existing, "x").unwrap();
    let r = lib.export_backup(&existing, &mut |_, _| {});
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    assert_eq!(fs::read(&existing).unwrap(), b"x");

    for inside in [
        dir.path().join("b.zip"),
        dir.path().join("papers").join("b.zip"),
    ] {
        let r = lib.export_backup(&inside, &mut |_, _| {});
        assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
        assert!(!inside.exists());
    }
    assert_eq!(snapshot(dir.path()), before);
    assert_eq!(
        fs::read_dir(out_dir.path()).unwrap().count(),
        1,
        "nothing else was written"
    );
}

#[test]
fn export_missing_parent_is_not_found() {
    let (_dir, lib) = build_library();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("no-such-dir").join("b.zip");
    let r = lib.export_backup(&out, &mut |_, _| {});
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
}

fn names_with_prefix(dir: &Path, prefix: &str) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(prefix))
        .collect()
}

/// 依路徑排序之後最後一個要放進備份的檔案。
fn last_backup_file(root: &Path) -> PathBuf {
    let files = backup_worthy_files(root);
    let (rel, _) = files
        .iter()
        .max_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()))
        .unwrap();
    root.join(rel)
}

#[test]
fn vanished_file_fails_and_leaves_nothing() {
    let (dir, lib) = build_library();
    let victim = last_backup_file(dir.path());
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    let mut first = true;
    let r = lib.export_backup(&out, &mut |_, _| {
        if first {
            first = false;
            fs::remove_file(&victim).unwrap();
        }
    });
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert!(!out.exists());
    assert!(names_with_prefix(out_dir.path(), ".pr-export-").is_empty());
}

#[test]
fn changed_file_is_reported_and_consistent() {
    let (dir, lib) = build_library();
    // 在第一次 progress 被呼叫時，附加一行到依路徑排序最後的那個檔（它還沒被讀到）
    let victim = last_backup_file(dir.path());
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    let mut first = true;
    let victim2 = victim.clone();
    let report = lib
        .export_backup(&out, &mut |_, _| {
            if first {
                first = false;
                use std::io::Write;
                let mut f = fs::OpenOptions::new().append(true).open(&victim2).unwrap();
                f.write_all(b"\nappended during export\n").unwrap();
            }
        })
        .unwrap();
    let rel = victim
        .strip_prefix(dir.path())
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    assert!(
        report.changed_during_export.contains(&rel),
        "{rel} not in {:?}",
        report.changed_during_export
    );
    let entries = zip_entries(&out);
    let manifest: BackupManifest = serde_json::from_slice(&entries.last().unwrap().1).unwrap();
    let f = manifest.files.iter().find(|f| f.path == rel).unwrap();
    let content = &entries
        .iter()
        .find(|(n, _, _)| n == &format!("library/{rel}"))
        .unwrap()
        .1;
    assert_eq!(f.sha256, sha256_hex(content));
    assert_eq!(f.bytes, content.len() as u64);
    assert!(content.ends_with(b"appended during export\n"));
}

#[test]
fn leftover_export_temp_is_cleaned() {
    let (_dir, lib) = build_library();
    let out_dir = tempfile::tempdir().unwrap();
    let good = format!(".pr-export-{}.zip", paperreader_core::ids::new_id());
    fs::write(out_dir.path().join(&good), "old").unwrap();
    fs::write(out_dir.path().join(".pr-export-foo.zip"), "keep").unwrap();
    fs::write(out_dir.path().join("other.txt"), "keep").unwrap();
    export(&lib, &out_dir.path().join("b.zip"));
    assert!(!out_dir.path().join(&good).exists());
    assert!(out_dir.path().join(".pr-export-foo.zip").exists());
    assert!(out_dir.path().join("other.txt").exists());
    assert!(out_dir.path().join("b.zip").exists());
}

#[test]
fn export_fails_when_library_json_is_missing() {
    // 沒有 library.json 的備份檔還原不了，整個匯出失敗、不產生備份檔
    let (dir, lib) = build_library();
    fs::remove_file(dir.path().join("library.json")).unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    let r = lib.export_backup(&out, &mut |_, _| {});
    assert!(matches!(r, Err(LibError::Unreadable { .. })), "{r:?}");
    assert!(!out.exists());
    assert_eq!(fs::read_dir(out_dir.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn export_fails_when_library_json_cannot_be_opened() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, lib) = build_library();
    let lj = dir.path().join("library.json");
    fs::set_permissions(&lj, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::File::open(&lj).is_ok() {
        eprintln!("SKIPPED: running as a user who can read mode 000 files");
        return;
    }
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    let r = lib.export_backup(&out, &mut |_, _| {});
    fs::set_permissions(&lj, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(r.is_err(), "{r:?}");
    assert!(!out.exists());
    assert_eq!(fs::read_dir(out_dir.path()).unwrap().count(), 0);
}

#[test]
fn read_only_library_can_export() {
    let (dir, lib) = build_library();
    drop(lib);
    let lp = dir.path().join("library.json");
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    v["format"] = serde_json::json!(2);
    fs::write(&lp, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    assert!(lib.is_read_only());
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    export(&lib, &out);
    assert!(out.is_file());
}

#[test]
fn export_does_not_modify_library() {
    let (dir, lib) = build_library();
    let before = snapshot(dir.path());
    let out_dir = tempfile::tempdir().unwrap();
    export(&lib, &out_dir.path().join("b.zip"));
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn progress_reports_every_file() {
    let (_dir, lib) = build_library();
    let out_dir = tempfile::tempdir().unwrap();
    let mut calls: Vec<(u64, u64)> = Vec::new();
    let report = lib
        .export_backup(&out_dir.path().join("b.zip"), &mut |d, t| {
            calls.push((d, t))
        })
        .unwrap();
    assert_eq!(calls.len(), report.files);
    assert_eq!(calls.last().unwrap().0, report.bytes);
    assert!(calls.windows(2).all(|w| w[0].0 <= w[1].0));
    assert!(calls.iter().all(|c| c.1 == calls[0].1));
}
