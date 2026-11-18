//! 工作 8：從備份檔還原。

mod common;

use common::*;
use paperreader_core::backup::restore_backup;
use paperreader_core::error::LibError;
use paperreader_core::jsonio::sha256_hex;
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{EventFilter, ProblemKind};
use std::fs;
use std::path::{Path, PathBuf};

// ---- 測試自己做的 zip（完全控制名稱、壓縮方式、旗標、檔案屬性）----

struct Ent {
    name: Vec<u8>,
    data: Vec<u8>,
    method: u16,
    flags: u16,
    external_attr: u32,
}

impl Ent {
    fn file(name: &str, data: &[u8]) -> Ent {
        Ent {
            name: name.as_bytes().to_vec(),
            data: data.to_vec(),
            method: 0,
            flags: 0x0800,
            external_attr: 0o100644 << 16,
        }
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn build_zip(ents: &[Ent]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for e in ents {
        let offset = out.len() as u32;
        let crc = crc32(&e.data);
        out.extend_from_slice(b"PK\x03\x04");
        le16(&mut out, 20);
        le16(&mut out, e.flags);
        le16(&mut out, e.method);
        le16(&mut out, 0);
        le16(&mut out, 33);
        le32(&mut out, crc);
        le32(&mut out, e.data.len() as u32);
        le32(&mut out, e.data.len() as u32);
        le16(&mut out, e.name.len() as u16);
        le16(&mut out, 0);
        out.extend_from_slice(&e.name);
        out.extend_from_slice(&e.data);

        central.extend_from_slice(b"PK\x01\x02");
        le16(&mut central, (3 << 8) | 20);
        le16(&mut central, 20);
        le16(&mut central, e.flags);
        le16(&mut central, e.method);
        le16(&mut central, 0);
        le16(&mut central, 33);
        le32(&mut central, crc);
        le32(&mut central, e.data.len() as u32);
        le32(&mut central, e.data.len() as u32);
        le16(&mut central, e.name.len() as u16);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le32(&mut central, e.external_attr);
        le32(&mut central, offset);
        central.extend_from_slice(&e.name);
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(b"PK\x05\x06");
    le16(&mut out, 0);
    le16(&mut out, 0);
    le16(&mut out, ents.len() as u16);
    le16(&mut out, ents.len() as u16);
    le32(&mut out, central.len() as u32);
    le32(&mut out, cd_offset);
    le16(&mut out, 0);
    out
}

const LIB_JSON: &str = r#"{"format":1,"id":"01JCK3V7Q8Z9X5M2N4P6R8T0AB","created":"2026-11-13T08:25:02.123Z","notes_dir":"notes"}"#;

fn manifest_for(files: &[(&str, &[u8])]) -> Vec<u8> {
    let list: Vec<serde_json::Value> = files
        .iter()
        .map(|(p, d)| serde_json::json!({"path": p, "bytes": d.len(), "sha256": sha256_hex(d)}))
        .collect();
    serde_json::to_vec_pretty(&serde_json::json!({
        "format": 1,
        "kind": "paperreader-backup",
        "created": "2026-11-15T15:00:00.000Z",
        "library_id": "01JCK3V7Q8Z9X5M2N4P6R8T0AB",
        "files": list,
    }))
    .unwrap()
}

/// 一份合法的備份檔：`library.json` 加上 `extra` 裡的檔。
fn good_zip(extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut files: Vec<(&str, &[u8])> = vec![("library.json", LIB_JSON.as_bytes())];
    files.extend_from_slice(extra);
    let mut ents: Vec<Ent> = files
        .iter()
        .map(|(p, d)| Ent::file(&format!("library/{p}"), d))
        .collect();
    ents.push(Ent::file("manifest.json", &manifest_for(&files)));
    build_zip(&ents)
}

struct Place {
    _dir: tempfile::TempDir,
    parent: PathBuf,
    archive: PathBuf,
    target: PathBuf,
}

fn place(zip_bytes: &[u8]) -> Place {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("restore-parent");
    fs::create_dir_all(&parent).unwrap();
    let archive = dir.path().join("b.zip");
    fs::write(&archive, zip_bytes).unwrap();
    let target = parent.join("lib");
    Place {
        _dir: dir,
        parent,
        archive,
        target,
    }
}

fn restore(p: &Place) -> Result<paperreader_core::store::RestoreReport, LibError> {
    restore_backup(&p.archive, &p.target, DEVICE)
}

fn assert_unreadable_and_clean(p: &Place, what: &str) {
    let r = restore(p);
    eprintln!(
        "[{what}] {}",
        r.as_ref().err().map(|e| e.to_string()).unwrap_or_default()
    );
    assert!(
        matches!(r, Err(LibError::Unreadable { .. })),
        "{what}: {r:?}"
    );
    assert!(!p.target.exists(), "{what}: target was created");
    let leftover: Vec<String> = fs::read_dir(&p.parent)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(leftover.is_empty(), "{what}: {leftover:?}");
}

fn export_to(lib: &LocalLibrary, dir: &Path) -> PathBuf {
    let out = dir.join("backup.zip");
    lib.export_backup(&out, &mut |_, _| {}).unwrap();
    out
}

// ---- 測試 ----

#[test]
fn roundtrip_is_identical() {
    let (dir, lib) = build_library();
    let work = tempfile::tempdir().unwrap();
    let archive = export_to(&lib, work.path());
    let target = work.path().join("a/b/restored");
    let report = restore_backup(&archive, &target, DEVICE).unwrap();
    assert!(
        report.manifest_mismatches.is_empty(),
        "{:?}",
        report.manifest_mismatches
    );
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(report.library_id, lib.library_file().id);

    assert_eq!(
        backup_worthy_files(dir.path()),
        backup_worthy_files(&target)
    );
    assert_eq!(report.files, backup_worthy_files(&target).len());

    let restored = LocalLibrary::open(&target, DEVICE).unwrap();
    assert_eq!(restored.library_file().id, lib.library_file().id);
    assert_eq!(restored.list_papers(), lib.list_papers());
    assert_eq!(
        restored.read_events(&EventFilter::default()).unwrap(),
        lib.read_events(&EventFilter::default()).unwrap()
    );
    assert_eq!(
        restored.list_groups().unwrap().0,
        lib.list_groups().unwrap().0
    );
    for s in lib.list_papers() {
        assert_eq!(
            restored.read_notes(&s.id).unwrap(),
            lib.read_notes(&s.id).unwrap(),
            "{}",
            s.id
        );
    }
    assert_eq!(restored.list_trash().unwrap(), lib.list_trash().unwrap());
    assert!(!lib.list_trash().unwrap().is_empty());
}

#[test]
fn restore_into_empty_existing_dir() {
    let p = place(&good_zip(&[("notes/x.md", b"hello")]));
    fs::create_dir_all(&p.target).unwrap();
    let r = restore(&p).unwrap();
    assert!(r.manifest_mismatches.is_empty());
    assert_eq!(fs::read(p.target.join("notes/x.md")).unwrap(), b"hello");
    assert!(p.target.join("library.json").is_file());
    // 暫存資料夾沒有留下
    let names: Vec<String> = fs::read_dir(&p.parent)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["lib"]);
}

#[test]
fn non_empty_target_is_refused() {
    let p = place(&good_zip(&[]));
    fs::create_dir_all(&p.target).unwrap();
    fs::write(p.target.join(".hidden"), "x").unwrap();
    let before = snapshot(&p.target);
    let r = restore(&p);
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    assert_eq!(snapshot(&p.target), before);
    // 目標是檔案
    let q = place(&good_zip(&[]));
    fs::write(&q.target, "i am a file").unwrap();
    let r = restore(&q);
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    assert_eq!(fs::read(&q.target).unwrap(), b"i am a file");
}

#[test]
fn invalid_device_id_is_refused_before_writing() {
    let p = place(&good_zip(&[]));
    for bad in ["", "short", "../../etc", "01jck3t1a2b3c4d5e6f7g8h9jk"] {
        let r = restore_backup(&p.archive, &p.target, bad);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{bad}: {r:?}"
        );
    }
    assert_eq!(fs::read_dir(&p.parent).unwrap().count(), 0);
    assert!(!p.target.exists());
}

#[test]
fn unsafe_entries_are_rejected() {
    let good: Vec<(&str, &[u8])> = vec![("library.json", LIB_JSON.as_bytes())];
    let lib_ent = || Ent::file("library/library.json", LIB_JSON.as_bytes());

    // 每個案例：不安全的名稱同時列在備份清單裡（`listed` 為 Some 時用它當清單路徑），
    // 讓拒絕的原因只可能是那個名稱本身。
    let name_cases: Vec<(&str, &str, Option<&str>)> = vec![
        ("dotdot inside library", "library/../evil", Some("../evil")),
        ("dotdot at top", "../evil", Some("../evil")),
        ("absolute", "/abs", Some("/abs")),
        ("empty segment", "library//x", Some("/x")),
        ("dot segment", "library/./x", Some("./x")),
        ("backslash", "library\\..\\x", Some("..\\x")),
        ("drive letter", "C:/x", Some("C:/x")),
        ("drive letter inside", "library/C:/x", Some("C:/x")),
        ("colon in name", "library/papers/a:b", Some("papers/a:b")),
        ("temp prefix", "library/.pr-x", Some(".pr-x")),
        // 反斜線規則本身（上面以 `library\` 開頭或不在 `library/` 底下的案例，是被「不是 library/ 底下」擋下的）
        (
            "backslash inside library",
            "library/papers/a\\b",
            Some("papers/a\\b"),
        ),
    ];
    for (what, entry_name, listed) in name_cases {
        let mut files = good.clone();
        let content: &[u8] = b"evil";
        if let Some(l) = listed {
            files.push((l, content));
        }
        let ents = vec![
            lib_ent(),
            Ent::file(entry_name, content),
            Ent::file("manifest.json", &manifest_for(&files)),
        ];
        assert_unreadable_and_clean(&place(&build_zip(&ents)), what);
    }

    // 符號連結項目
    let mut link = Ent::file("library/link", b"/etc/passwd");
    link.external_attr = 0o120777 << 16;
    let files: Vec<(&str, &[u8])> = vec![
        ("library.json", LIB_JSON.as_bytes()),
        ("link", b"/etc/passwd"),
    ];
    let zip = build_zip(&[
        lib_ent(),
        link,
        Ent::file("manifest.json", &manifest_for(&files)),
    ]);
    assert_unreadable_and_clean(&place(&zip), "symlink entry");

    // 只差大小寫的兩個項目
    let files: Vec<(&str, &[u8])> = vec![
        ("library.json", LIB_JSON.as_bytes()),
        ("a.txt", b"1"),
        ("A.txt", b"2"),
    ];
    let zip = build_zip(&[
        lib_ent(),
        Ent::file("library/a.txt", b"1"),
        Ent::file("library/A.txt", b"2"),
        Ent::file("manifest.json", &manifest_for(&files)),
    ]);
    assert_unreadable_and_clean(&place(&zip), "case duplicate entries");

    // 備份清單沒列的項目
    let zip = build_zip(&[
        lib_ent(),
        Ent::file("library/unlisted.txt", b"x"),
        Ent::file("manifest.json", &manifest_for(&good)),
    ]);
    assert_unreadable_and_clean(&place(&zip), "entry not in manifest");

    // 備份清單列了但 zip 裡沒有的檔
    let files: Vec<(&str, &[u8])> =
        vec![("library.json", LIB_JSON.as_bytes()), ("missing.txt", b"x")];
    let zip = build_zip(&[lib_ent(), Ent::file("manifest.json", &manifest_for(&files))]);
    assert_unreadable_and_clean(&place(&zip), "manifest lists a missing file");

    // 備份清單的路徑含 `..`（zip 裡也有同名項目已在上面涵蓋；這裡只有清單）
    let files: Vec<(&str, &[u8])> = vec![("library.json", LIB_JSON.as_bytes()), ("a/../b", b"x")];
    let zip = build_zip(&[lib_ent(), Ent::file("manifest.json", &manifest_for(&files))]);
    assert_unreadable_and_clean(&place(&zip), "manifest path with ..");

    // 備份清單裡路徑重複（不分大小寫）
    let files: Vec<(&str, &[u8])> = vec![
        ("library.json", LIB_JSON.as_bytes()),
        ("x.txt", b"1"),
        ("X.TXT", b"1"),
    ];
    let zip = build_zip(&[
        lib_ent(),
        Ent::file("library/x.txt", b"1"),
        Ent::file("manifest.json", &manifest_for(&files)),
    ]);
    assert_unreadable_and_clean(&place(&zip), "manifest duplicate paths");

    // 項目的名稱不是 manifest.json 也不在 library/ 底下
    let zip = build_zip(&[
        lib_ent(),
        Ent::file("other/x", b"x"),
        Ent::file("manifest.json", &manifest_for(&good)),
    ]);
    assert_unreadable_and_clean(&place(&zip), "entry outside library/");

    // Deflate 壓縮的項目（本地檔頭與中央目錄兩處的壓縮方式都是 8）
    let mut deflated = Ent::file("library/papers/x.txt", b"data");
    deflated.method = 8;
    let files: Vec<(&str, &[u8])> = vec![
        ("library.json", LIB_JSON.as_bytes()),
        ("papers/x.txt", b"data"),
    ];
    let zip = build_zip(&[
        lib_ent(),
        deflated,
        Ent::file("manifest.json", &manifest_for(&files)),
    ]);
    assert_unreadable_and_clean(&place(&zip), "deflate entry");

    // 加密的項目（通用旗標第 0 位元）
    let mut enc = Ent::file("library/papers/x.txt", b"data");
    enc.flags |= 1;
    let zip = build_zip(&[
        lib_ent(),
        enc,
        Ent::file("manifest.json", &manifest_for(&files)),
    ]);
    assert_unreadable_and_clean(&place(&zip), "encrypted entry");
}

#[test]
fn huge_manifest_size_is_unreadable_not_a_panic() {
    // 備份清單的 bytes 是 u64 最大值時，不可以溢位 panic
    let files: Vec<(&str, &[u8])> =
        vec![("library.json", LIB_JSON.as_bytes()), ("a.txt", b"hello")];
    let mut manifest: serde_json::Value = serde_json::from_slice(&manifest_for(&files)).unwrap();
    for big in [u64::MAX, u64::MAX - 1, 1u64 << 63] {
        manifest["files"][1]["bytes"] = serde_json::json!(big);
        let zip = build_zip(&[
            Ent::file("library/library.json", LIB_JSON.as_bytes()),
            Ent::file("library/a.txt", b"hello"),
            Ent::file("manifest.json", &serde_json::to_vec(&manifest).unwrap()),
        ]);
        assert_unreadable_and_clean(&place(&zip), &format!("bytes = {big}"));
    }
}

#[test]
fn manifest_library_id_must_match_library_json() {
    let files: Vec<(&str, &[u8])> = vec![("library.json", LIB_JSON.as_bytes())];
    let mut m: serde_json::Value = serde_json::from_slice(&manifest_for(&files)).unwrap();
    m["library_id"] = serde_json::json!("01ZZZZZZZZZZZZZZZZZZZZZZZZ");
    let zip = build_zip(&[
        Ent::file("library/library.json", LIB_JSON.as_bytes()),
        Ent::file("manifest.json", &serde_json::to_vec(&m).unwrap()),
    ]);
    assert_unreadable_and_clean(&place(&zip), "library id mismatch");
}

#[test]
fn files_that_cannot_form_a_library_are_rejected_before_writing() {
    // 低 3
    let lib = ("library.json", LIB_JSON.as_bytes());
    type FileList<'a> = Vec<(&'a str, &'a [u8])>;
    let cases: Vec<(&str, FileList)> = vec![
        ("papers is a file", vec![lib, ("papers", b"x")]),
        ("notes is a file", vec![lib, ("notes", b"x")]),
        (
            "file and folder share a name",
            vec![lib, ("a", b"x"), ("a/b", b"y")],
        ),
        (
            "deep file and folder",
            vec![lib, ("p/q", b"x"), ("p/q/r.txt", b"y")],
        ),
    ];
    for (what, files) in cases {
        let mut ents: Vec<Ent> = files
            .iter()
            .map(|(p, d)| Ent::file(&format!("library/{p}"), d))
            .collect();
        ents.push(Ent::file("manifest.json", &manifest_for(&files)));
        assert_unreadable_and_clean(&place(&build_zip(&ents)), what);
    }
}

#[test]
fn failed_restore_removes_the_parent_it_created() {
    // 解出途中失敗，不留下自己建的上層資料夾
    let files: Vec<(&str, &[u8])> =
        vec![("library.json", LIB_JSON.as_bytes()), ("a.txt", b"hello")];
    let mut m: serde_json::Value = serde_json::from_slice(&manifest_for(&files)).unwrap();
    m["files"][1]["sha256"] = serde_json::json!("0".repeat(64));
    let p = place(&build_zip(&[
        Ent::file("library/library.json", LIB_JSON.as_bytes()),
        Ent::file("library/a.txt", b"hello"),
        Ent::file("manifest.json", &serde_json::to_vec(&m).unwrap()),
    ]));
    let new_parent = p.parent.join("not-yet");
    let target = new_parent.join("lib");
    let r = restore_backup(&p.archive, &target, DEVICE);
    assert!(matches!(r, Err(LibError::Unreadable { .. })), "{r:?}");
    assert!(
        !new_parent.exists(),
        "the parent created by the restore must be removed"
    );
    // 原本就存在的上層資料夾不動
    assert!(p.parent.exists());
}

#[test]
fn hash_or_size_mismatch_is_rejected_and_cleaned() {
    // 備份清單的指紋被改
    let files: Vec<(&str, &[u8])> =
        vec![("library.json", LIB_JSON.as_bytes()), ("a.txt", b"hello")];
    let mut manifest: serde_json::Value = serde_json::from_slice(&manifest_for(&files)).unwrap();
    manifest["files"][1]["sha256"] = serde_json::json!("0".repeat(64));
    let zip = build_zip(&[
        Ent::file("library/library.json", LIB_JSON.as_bytes()),
        Ent::file("library/a.txt", b"hello"),
        Ent::file("manifest.json", &serde_json::to_vec(&manifest).unwrap()),
    ]);
    assert_unreadable_and_clean(&place(&zip), "hash mismatch");

    // zip 裡某一項比備份清單多 1 個位元組
    let zip = build_zip(&[
        Ent::file("library/library.json", LIB_JSON.as_bytes()),
        Ent::file("library/a.txt", b"hello!"),
        Ent::file("manifest.json", &manifest_for(&files)),
    ]);
    assert_unreadable_and_clean(&place(&zip), "size mismatch");
}

#[test]
fn missing_or_bad_manifest() {
    let lib_ent = Ent::file("library/library.json", LIB_JSON.as_bytes());
    let zip = build_zip(&[Ent::file("library/library.json", LIB_JSON.as_bytes())]);
    assert_unreadable_and_clean(&place(&zip), "no manifest");

    let zip = build_zip(&[lib_ent, Ent::file("manifest.json", b"{")]);
    assert_unreadable_and_clean(&place(&zip), "manifest is not json");

    let files: Vec<(&str, &[u8])> = vec![("library.json", LIB_JSON.as_bytes())];
    let mut m: serde_json::Value = serde_json::from_slice(&manifest_for(&files)).unwrap();
    m["kind"] = serde_json::json!("something-else");
    let zip = build_zip(&[
        Ent::file("library/library.json", LIB_JSON.as_bytes()),
        Ent::file("manifest.json", &serde_json::to_vec(&m).unwrap()),
    ]);
    assert_unreadable_and_clean(&place(&zip), "wrong kind");

    // 不是 zip
    assert_unreadable_and_clean(&place(b"this is not a zip file at all"), "not a zip");

    // format 比這一版新
    let mut m: serde_json::Value = serde_json::from_slice(&manifest_for(&files)).unwrap();
    m["format"] = serde_json::json!(2);
    let p = place(&build_zip(&[
        Ent::file("library/library.json", LIB_JSON.as_bytes()),
        Ent::file("manifest.json", &serde_json::to_vec(&m).unwrap()),
    ]));
    let r = restore(&p);
    assert!(matches!(r, Err(LibError::FormatTooNew { .. })), "{r:?}");
    assert!(!p.target.exists());
}

#[test]
fn library_json_required_and_readable() {
    // 備份清單沒有 library.json
    let files: Vec<(&str, &[u8])> = vec![("notes/x.md", b"x")];
    let zip = build_zip(&[
        Ent::file("library/notes/x.md", b"x"),
        Ent::file("manifest.json", &manifest_for(&files)),
    ]);
    assert_unreadable_and_clean(&place(&zip), "no library.json");

    // library.json 的內容是 `{`
    let files: Vec<(&str, &[u8])> = vec![("library.json", b"{")];
    let zip = build_zip(&[
        Ent::file("library/library.json", b"{"),
        Ent::file("manifest.json", &manifest_for(&files)),
    ]);
    assert_unreadable_and_clean(&place(&zip), "library.json is not json");
}

#[test]
fn leftover_restore_temp_is_cleaned() {
    let p = place(&good_zip(&[]));
    let good = format!(".pr-restoring-{}", paperreader_core::ids::new_id());
    fs::create_dir_all(p.parent.join(&good).join("junk")).unwrap();
    fs::write(p.parent.join(&good).join("junk/f"), "x").unwrap();
    fs::create_dir_all(p.parent.join(".pr-restoring-foo")).unwrap();
    restore(&p).unwrap();
    assert!(!p.parent.join(&good).exists());
    assert!(p.parent.join(".pr-restoring-foo").exists());
}

#[test]
fn report_lists_existing_problems() {
    let (dir, lib) = build_library();
    fs::write(
        dir.path().join("papers/Alpha Paper/extra.txt"),
        "not listed",
    )
    .unwrap();
    let work = tempfile::tempdir().unwrap();
    let archive = export_to(&lib, work.path());
    let target = work.path().join("restored");
    let report = restore_backup(&archive, &target, DEVICE).unwrap();
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.kind == ProblemKind::UnlistedFile && p.path.ends_with("extra.txt")),
        "{:?}",
        report.problems
    );
}

#[test]
fn every_export_is_restorable() {
    let (dir, lib) = build_library();
    let root = dir.path();
    // 超長的路徑（各平台都試；Windows 上建不出來時照實略過並印出原因）
    let mut long = root.join("notes");
    for _ in 0..8 {
        long = long.join("d".repeat(140));
    }
    let long_made = fs::create_dir_all(&long)
        .and_then(|_| fs::write(long.join("deep.md"), "deep"))
        .is_ok();
    if !long_made {
        eprintln!("SKIPPED part: this platform cannot create a path longer than 1024 bytes");
    }
    #[cfg(unix)]
    {
        fs::write(root.join("notes/Meeting 10:30.md"), "colon").unwrap();
        fs::write(root.join("notes/a\\b.txt"), "backslash").unwrap();
        fs::write(root.join("papers/Alpha Paper/Case.txt"), "1").unwrap();
        fs::write(root.join("papers/Alpha Paper/case.txt"), "2").unwrap();
    }

    let work = tempfile::tempdir().unwrap();
    let out = work.path().join("b.zip");
    let report = lib.export_backup(&out, &mut |_, _| {}).unwrap();
    if long_made {
        assert!(
            report
                .skipped
                .iter()
                .any(|s| s.reason == "unsafe-path" && s.path.ends_with("deep.md")),
            "{:?}",
            report.skipped
        );
    }
    #[cfg(unix)]
    {
        let has = |p: &str, r: &str| report.skipped.iter().any(|s| s.path == p && s.reason == r);
        assert!(
            has("notes/Meeting 10:30.md", "unsafe-path"),
            "{:?}",
            report.skipped
        );
        assert!(has("notes/a\\b.txt", "unsafe-path"));
        assert!(has("papers/Alpha Paper/case.txt", "case-duplicate"));
    }
    let target = work.path().join("restored");
    let r = restore_backup(&out, &target, DEVICE).unwrap();
    assert!(r.manifest_mismatches.is_empty());
}

#[test]
#[ignore = "4.1 GiB: run by hand on a computer when needed"]
fn export_over_4gib_roundtrip() {
    use std::io::Write;
    let (dir, lib) = temp_library();
    // 許多個 100 MB 的檔湊到 4.1 GiB（內容都不同，放在 notes/ 底下）
    let notes = dir.path().join("notes");
    fs::create_dir_all(&notes).unwrap();
    let chunk = fake_bytes(7, 1 << 20);
    for i in 0..42 {
        let mut f = fs::File::create(notes.join(format!("big-{i:02}.bin"))).unwrap();
        for j in 0..100u32 {
            let mut c = chunk.clone();
            c[..8].copy_from_slice(&((i as u64) << 32 | j as u64).to_le_bytes());
            f.write_all(&c).unwrap();
        }
    }
    let work = tempfile::tempdir().unwrap();
    let out = work.path().join("big.zip");
    lib.export_backup(&out, &mut |_, _| {}).unwrap();
    let target = work.path().join("restored");
    let r = restore_backup(&out, &target, DEVICE).unwrap();
    assert!(r.manifest_mismatches.is_empty());
    for i in 0..42 {
        let p = target.join(format!("notes/big-{i:02}.bin"));
        assert_eq!(
            fs::metadata(&p).unwrap().len(),
            100 << 20,
            "{}",
            p.display()
        );
    }
}
