//! 同步頁要的核心方法：
//! 只看一份檔是哪一種、「合併沒做完」的唯讀、提醒標出屬於哪個衝突、想法衝突兩邊的時間、
//! 開啟時自動做完的合併讓畫面知道一次。

mod common;

use common::*;
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{ArchiveKind, ConflictChoice, ConflictKind, NoticeKind, SyncMode};
use paperreader_core::sync::package::peek_kind;
use std::fs;
use std::io::Write;
use zip::write::SimpleFileOptions;

fn find(lib: &LocalLibrary, title: &str) -> String {
    lib.list_papers()
        .into_iter()
        .find(|p| p.bib.title.as_deref() == Some(title))
        .unwrap()
        .id
}

fn zip_with(path: &std::path::Path, entries: &[(&str, &[u8])]) {
    let f = fs::File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    for (n, b) in entries {
        z.start_file(
            *n,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        z.write_all(b).unwrap();
    }
    z.finish().unwrap();
}

#[test]
fn peek_kind_tells_sync_packages_backups_and_other_files_apart() {
    let (_dir, mut lib) = build_library();
    let out = tempfile::tempdir().unwrap();
    let pkg = out.path().join("s.zip");
    lib.export_sync_package(&pkg, SyncMode::Full, None, &mut |_, _| {})
        .unwrap();
    let backup = out.path().join("b.zip");
    lib.export_backup(&backup, &mut |_, _| {}).unwrap();
    assert_eq!(peek_kind(&pkg).unwrap(), ArchiveKind::SyncPackage);
    assert_eq!(peek_kind(&backup).unwrap(), ArchiveKind::Backup);

    let other_kind = out.path().join("o.zip");
    zip_with(
        &other_kind,
        &[("manifest.json", br#"{"kind":"something-else"}"#)],
    );
    let no_manifest = out.path().join("n.zip");
    zip_with(&no_manifest, &[("a.txt", b"x")]);
    let bad_json = out.path().join("j.zip");
    zip_with(&bad_json, &[("manifest.json", b"{nope")]);
    let not_zip = out.path().join("p.pdf");
    fs::write(&not_zip, fake_bytes(1, 200)).unwrap();
    // 匯入清單包在一層資料夾裡：不在根目錄，不算整批匯入用的 zip
    let nested_import = out.path().join("i2.zip");
    zip_with(
        &nested_import,
        &[("papers/paperreader-import.json", b"{}"), ("a.pdf", b"x")],
    );
    for f in [
        &other_kind,
        &no_manifest,
        &bad_json,
        &not_zip,
        &nested_import,
    ] {
        assert_eq!(peek_kind(f).unwrap(), ArchiveKind::Other, "{f:?}");
    }
    assert!(peek_kind(&out.path().join("missing.zip")).is_err());
}

/// 根目錄有匯入清單、沒有同步包或備份檔的清單：整批匯入用的 zip。清單的內容不看。
/// 同步包或備份檔的清單在時，照那一種。
#[test]
fn peek_kind_tells_import_zips_apart() {
    let out = tempfile::tempdir().unwrap();
    let import = out.path().join("i.zip");
    zip_with(
        &import,
        &[
            (
                "paperreader-import.json",
                br#"{"kind":"paperreader-import","format":1,"papers":[]}"#,
            ),
            ("pdf/a.pdf", b"%PDF-1.4"),
        ],
    );
    let unreadable_list = out.path().join("u.zip");
    zip_with(&unreadable_list, &[("paperreader-import.json", b"{nope")]);
    // `manifest.json` 的 `kind` 不是同步包也不是備份檔，但匯入清單在
    let with_other_manifest = out.path().join("m.zip");
    zip_with(
        &with_other_manifest,
        &[
            ("manifest.json", br#"{"kind":"something-else"}"#),
            ("paperreader-import.json", b"{}"),
        ],
    );
    for f in [&import, &unreadable_list, &with_other_manifest] {
        assert_eq!(peek_kind(f).unwrap(), ArchiveKind::ImportZip, "{f:?}");
    }
    let sync_with_list = out.path().join("s.zip");
    zip_with(
        &sync_with_list,
        &[
            ("manifest.json", br#"{"kind":"paperreader-sync"}"#),
            ("paperreader-import.json", b"{}"),
        ],
    );
    assert_eq!(
        peek_kind(&sync_with_list).unwrap(),
        ArchiveKind::SyncPackage
    );
    let backup_with_list = out.path().join("b.zip");
    zip_with(
        &backup_with_list,
        &[
            ("manifest.json", br#"{"kind":"paperreader-backup"}"#),
            ("paperreader-import.json", b"{}"),
        ],
    );
    assert_eq!(peek_kind(&backup_with_list).unwrap(), ArchiveKind::Backup);
}

/// 唯讀有兩個原因：格式較新、合併沒做完。只有後者是「合併沒做完」。
#[test]
fn unfinished_sync_is_told_apart_from_a_newer_format() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let (paper, tok) = p.a.get_paper(&alpha).unwrap();
    let d = paperreader_core::format::BibDraft {
        title: Some("From A".into()),
        authors: paper.bib.authors.clone(),
        ..Default::default()
    };
    p.a.update_bib(&alpha, d, &tok).unwrap();
    let pkg = export_pkg(&mut p.a, p.out.path());
    assert!(!p.b.has_unfinished_sync_import());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    p.b.set_sync_apply_hook(Some(Box::new(|k| k != 1)));
    assert!(p.b.apply_sync_import(&preview.preview_id, &[]).is_err());
    assert!(p.b.is_read_only());
    assert!(p.b.has_unfinished_sync_import());

    // 格式較新的唯讀不是
    let (dir, lib) = temp_library();
    drop(lib);
    let lp = dir.path().join("library.json");
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    v["format"] = 2.into();
    fs::write(&lp, v.to_string()).unwrap();
    let ro = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    assert!(ro.is_read_only());
    assert!(!ro.has_unfinished_sync_import());
}

/// 開啟時自動做完的合併：回報留到畫面說「收到了」才清掉（問狀況的那一次被丟掉也不會不見）。
#[test]
fn a_merge_finished_on_open_is_reported_until_acknowledged() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "from A", nt.as_ref()).unwrap();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    p.b.set_sync_apply_hook(Some(Box::new(|k| k != 1)));
    assert!(p.b.apply_sync_import(&preview.preview_id, &[]).is_err());
    let root = p.b_root();
    drop(p.b);
    let mut b = LocalLibrary::open(&root, DEVICE_B).unwrap();
    assert!(!b.is_read_only());
    let r = b.resumed_sync_report();
    assert_eq!(
        r.map(|r| r.package.clone()),
        Some(preview.package.package.clone())
    );
    assert!(
        b.resumed_sync_report().is_some(),
        "asking again still has it"
    );
    b.clear_resumed_sync_report();
    assert!(b.resumed_sync_report().is_none());

    // 平常開啟：沒有
    let b2 = LocalLibrary::open(&root, DEVICE_B).unwrap();
    assert!(b2.resumed_sync_report().is_none());
}

/// 同一則提醒（種類、文獻、路徑、其他都相同）只留一則，衝突的鍵併在一起；其中一次和選擇無關
/// （沒有鍵）時，合起來就一定出現。
#[test]
fn the_same_notice_twice_is_kept_once_with_the_keys_merged() {
    use paperreader_core::sync::merge::MergeCtx;
    let ctx = || {
        MergeCtx::new(
            "2026-01-01T00:00:00.000Z".into(),
            "20260101T000000Z".into(),
            DEVICE.into(),
            DEVICE_B.into(),
            Default::default(),
            Box::new(|| "x".into()),
        )
    };
    let mut c = ctx();
    c.notice_for(
        NoticeKind::ConflictCopy,
        Some("p"),
        Some("n.md"),
        None,
        vec!["k1".into()],
    );
    c.notice_for(
        NoticeKind::ConflictCopy,
        Some("p"),
        Some("n.md"),
        None,
        vec!["k2".into()],
    );
    c.notice_for(
        NoticeKind::ConflictCopy,
        Some("p"),
        Some("n.md"),
        None,
        vec!["k1".into()],
    );
    assert_eq!(c.notices.len(), 1, "{:?}", c.notices);
    assert_eq!(
        c.notices[0].conflicts,
        vec!["k1".to_string(), "k2".to_string()]
    );
    // 從子項目併進來也照這個規則
    let mut sub = ctx();
    sub.notice_for(
        NoticeKind::ConflictCopy,
        Some("p"),
        Some("n.md"),
        None,
        vec!["k3".into()],
    );
    c.absorb(sub);
    assert_eq!(c.notices.len(), 1);
    assert_eq!(c.notices[0].conflicts.len(), 3);
    c.notice(NoticeKind::ConflictCopy, Some("p"), Some("n.md"), None);
    assert_eq!(c.notices.len(), 1);
    assert!(c.notices[0].conflicts.is_empty(), "unconditional now");
    c.notice_for(
        NoticeKind::ConflictCopy,
        Some("p"),
        Some("n.md"),
        None,
        vec!["k4".into()],
    );
    assert!(c.notices[0].conflicts.is_empty());
}

/// 那一邊的時間取「那一邊內容的節點」那一筆，不是那台最後一筆。B 最後一筆想法記錄是別的內容
/// （之後在筆記工具裡改回來、沒有再記），B 這一邊的時間是 "B version" 那一筆的時間。
#[test]
fn thoughts_side_time_is_the_node_not_the_devices_last_record() {
    use paperreader_core::changes::thoughts_hash;
    use paperreader_core::ids::{format_utc_millis, new_id, now_utc_millis};
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "A version", nt.as_ref()).unwrap();
    let nt = p.b.read_notes(&alpha).unwrap().1;
    p.b.set_thoughts(&alpha, "B version", nt.as_ref()).unwrap();
    let mut lines = change_lines(&p.b_root(), DEVICE_B);
    let node = lines
        .iter()
        .rev()
        .find(|l| l["type"] == "thoughts-written" && l["paper"] == alpha.as_str())
        .unwrap()
        .clone();
    let mut later = node.clone();
    later["id"] = new_id().into();
    later["t"] = format_utc_millis(now_utc_millis() + 60_000).unwrap().into();
    later["data"] = serde_json::json!({
        "sha256": thoughts_hash("something else"),
        "prev": [node["id"].clone()],
    });
    lines.push(later.clone());
    write_change_lines(&p.b_root(), DEVICE_B, &lines);
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let c = preview
        .conflicts
        .iter()
        .find(|c| c.kind == ConflictKind::Thoughts)
        .unwrap();
    assert_eq!(c.local.modified.as_deref(), node["t"].as_str());
    assert_ne!(c.local.modified.as_deref(), later["t"].as_str());
}

/// 想法衝突兩邊的修改時間：那一邊內容的那一筆 `thoughts-written` 的時間。
#[test]
fn thoughts_conflict_sides_carry_the_time_of_their_record() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "A version", nt.as_ref()).unwrap();
    let nt = p.b.read_notes(&alpha).unwrap().1;
    p.b.set_thoughts(&alpha, "B version", nt.as_ref()).unwrap();
    let t_of = |root: &std::path::Path, dev: &str| -> String {
        change_lines(root, dev)
            .into_iter()
            .rev()
            .find(|l| l["type"] == "thoughts-written" && l["paper"] == alpha.as_str())
            .unwrap()["t"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let ta = t_of(&p.a_root(), DEVICE);
    let tb = t_of(&p.b_root(), DEVICE_B);
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let c = preview
        .conflicts
        .iter()
        .find(|c| c.kind == ConflictKind::Thoughts)
        .unwrap();
    assert_eq!(c.local.modified.as_deref(), Some(tb.as_str()));
    assert_eq!(c.incoming.modified.as_deref(), Some(ta.as_str()));
}

/// 衝突副本的提醒標出它是因為哪幾個衝突選了「兩個都留」；和選擇無關的不標。
#[test]
fn conflict_copy_notices_name_the_conflicts_they_depend_on() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "A version", nt.as_ref()).unwrap();
    let nt = p.b.read_notes(&alpha).unwrap().1;
    p.b.set_thoughts(&alpha, "B version", nt.as_ref()).unwrap();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let c = preview
        .conflicts
        .iter()
        .find(|c| c.kind == ConflictKind::Thoughts)
        .unwrap();
    assert_eq!(c.default, ConflictChoice::Both);
    let copies: Vec<_> = preview
        .notices
        .iter()
        .filter(|n| n.kind == NoticeKind::ConflictCopy)
        .collect();
    assert_eq!(copies.len(), 1, "{:?}", preview.notices);
    assert_eq!(copies[0].conflicts, vec![c.id.clone()]);
    // 選了「留這台的」：寫入時沒有副本
    let r =
        p.b.apply_sync_import(
            &preview.preview_id,
            &[(c.id.clone(), ConflictChoice::Local)],
        )
        .unwrap();
    assert!(r.conflict_copies.is_empty(), "{r:?}");
    assert!(
        !r.notices.iter().any(|n| n.kind == NoticeKind::ConflictCopy),
        "{:?}",
        r.notices
    );
}

/// 標記以外的文字不同（和選擇無關）也需要副本：同一份副本的提醒不標衝突，一定會出現。
#[test]
fn a_copy_needed_regardless_of_choices_is_not_tied_to_conflicts() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "A version", nt.as_ref()).unwrap();
    let nt = p.b.read_notes(&alpha).unwrap().1;
    p.b.set_thoughts(&alpha, "B version", nt.as_ref()).unwrap();
    let path = p.a_root().join("notes/Alpha Paper.md");
    let t = fs::read_to_string(&path).unwrap() + "\nA's own paragraph\n";
    fs::write(&path, t).unwrap();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let copies: Vec<_> = preview
        .notices
        .iter()
        .filter(|n| n.kind == NoticeKind::ConflictCopy)
        .collect();
    assert_eq!(copies.len(), 1, "{:?}", preview.notices);
    assert!(copies[0].conflicts.is_empty(), "{:?}", copies[0]);
}
