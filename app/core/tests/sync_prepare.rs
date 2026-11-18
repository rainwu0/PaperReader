//! 匯入同步包的準備：拒絕的情況、整份檢查、省略的檔依指紋找、預覽只寫暫存。

mod common;

use common::*;
use paperreader_core::changes::thoughts_hash;
use paperreader_core::error::LibError;
use paperreader_core::format::BibDraft;
use paperreader_core::jsonio::{sha256_hex, to_canonical_json};
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{ConflictKind, ItemKind, SyncMode};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

type Entries = Vec<(String, Vec<u8>)>;

fn find(lib: &LocalLibrary, title: &str) -> String {
    lib.list_papers()
        .into_iter()
        .find(|p| p.bib.title.as_deref() == Some(title))
        .unwrap()
        .id
}

fn set_title(lib: &mut LocalLibrary, id: &str, title: &str) {
    let (paper, tok) = lib.get_paper(id).unwrap();
    let d = BibDraft {
        title: Some(title.into()),
        authors: paper.bib.authors.clone(),
        ..Default::default()
    };
    lib.update_bib(id, d, &tok).unwrap();
}

fn no_staging(root: &Path) -> bool {
    !fs::read_dir(root)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with(".pr-sync-"))
}

/// 照原本的同步包重做一份 zip：`f` 可以改清單與項目（名稱 → 內容）。
fn rewrite(
    src: &Path,
    dst: &Path,
    f: impl FnOnce(&mut Value, &mut Entries),
    method: zip::CompressionMethod,
) {
    let mut entries: Entries = zip_entries(src)
        .into_iter()
        .map(|(n, b, _)| (n, b))
        .collect();
    let mi = entries.iter().position(|e| e.0 == "manifest.json").unwrap();
    let (_, mb) = entries.remove(mi);
    let mut manifest: Value = serde_json::from_slice(&mb).unwrap();
    f(&mut manifest, &mut entries);
    let file = fs::File::create(dst).unwrap();
    let mut z = zip::ZipWriter::new(file);
    for (n, b) in &entries {
        z.start_file(
            n.as_str(),
            SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        z.write_all(b).unwrap();
    }
    z.start_file(
        "manifest.json",
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .unwrap();
    z.write_all(manifest.to_string().as_bytes()).unwrap();
    z.finish().unwrap();
}

fn expect_unreadable(lib: &mut LocalLibrary, pkg: &Path, root: &Path) {
    let r = lib.prepare_sync_import(pkg, &mut |_, _| {});
    assert!(matches!(r, Err(LibError::Unreadable { .. })), "{r:?}");
    assert!(no_staging(root));
}

#[test]
fn backup_file_rejected_with_reason() {
    let mut p = pair();
    let zip = p.out.path().join("b.zip");
    p.a.export_backup(&zip, &mut |_, _| {}).unwrap();
    match p.b.prepare_sync_import(&zip, &mut |_, _| {}) {
        Err(LibError::Unreadable { reason, .. }) => {
            assert!(reason.starts_with("backup file"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn other_library_rejected() {
    let mut p = pair();
    let (_d, mut other) = build_library();
    let pkg = export_pkg(&mut other, p.out.path());
    match p.b.prepare_sync_import(&pkg, &mut |_, _| {}) {
        Err(LibError::InvalidInput { reason }) => {
            assert!(reason.starts_with("library id mismatch"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
    assert!(no_staging(&p.b_root()));
}

#[test]
fn own_package_rejected() {
    let mut p = pair();
    let pkg = export_pkg(&mut p.b, p.out.path());
    match p.b.prepare_sync_import(&pkg, &mut |_, _| {}) {
        Err(LibError::InvalidInput { reason }) => assert_eq!(reason, "package from this device"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn format_too_new() {
    let mut p = pair();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let bad = p.out.path().join("new.zip");
    rewrite(
        &pkg,
        &bad,
        |m, _| m["format"] = json!(2),
        zip::CompressionMethod::Stored,
    );
    assert!(matches!(
        p.b.prepare_sync_import(&bad, &mut |_, _| {}),
        Err(LibError::FormatTooNew { found: 2 })
    ));
    // library.json 的 format 較新
    let bad2 = p.out.path().join("new2.zip");
    rewrite(
        &pkg,
        &bad2,
        |m, entries| {
            let e = entries
                .iter_mut()
                .find(|e| e.0 == "library/library.json")
                .unwrap();
            let mut v: Value = serde_json::from_slice(&e.1).unwrap();
            v["format"] = json!(3);
            e.1 = to_canonical_json(&v).unwrap().into_bytes();
            let f = m["files"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|f| f["path"] == json!("library.json"))
                .unwrap();
            f["bytes"] = json!(e.1.len());
            f["sha256"] = json!(sha256_hex(&e.1));
        },
        zip::CompressionMethod::Stored,
    );
    assert!(matches!(
        p.b.prepare_sync_import(&bad2, &mut |_, _| {}),
        Err(LibError::FormatTooNew { found: 3 })
    ));
}

#[test]
fn unsafe_entries_rejected() {
    let mut p = pair();
    let root = p.b_root();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let out = p.out.path().to_path_buf();
    let mut n = 0;
    let mut case = |f: &mut dyn FnMut(&mut Value, &mut Entries), method: zip::CompressionMethod| {
        n += 1;
        let bad = out.join(format!("bad{n}.zip"));
        rewrite(&pkg, &bad, |m, e| f(m, e), method);
        bad
    };
    let cases: Vec<PathBuf> = vec![
        // 不安全的路徑
        case(
            &mut |m, e| {
                e.push(("library/../evil".into(), b"x".to_vec()));
                m["files"].as_array_mut().unwrap().push(
                    json!({"path": "../evil", "bytes": 1, "sha256": sha256_hex(b"x"), "included": true}),
                );
            },
            zip::CompressionMethod::Stored,
        ),
        // included: true 卻不在 zip
        case(
            &mut |m, _| {
                m["files"].as_array_mut().unwrap().push(
                    json!({"path": "logs/ghost.jsonl", "bytes": 1, "sha256": sha256_hex(b"x"), "included": true}),
                );
            },
            zip::CompressionMethod::Stored,
        ),
        // included: false 卻在 zip
        case(
            &mut |m, _| {
                let f = m["files"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|f| f["path"].as_str().unwrap().ends_with(".pdf"))
                    .unwrap();
                f["included"] = json!(false);
            },
            zip::CompressionMethod::Stored,
        ),
        // zip 裡有清單沒列的
        case(
            &mut |_, e| e.push(("library/logs/extra.jsonl".into(), b"{}".to_vec())),
            zip::CompressionMethod::Stored,
        ),
        // 只差大小寫
        case(
            &mut |m, e| {
                e.push(("library/LIBRARY.json".into(), b"{}".to_vec()));
                m["files"].as_array_mut().unwrap().push(
                    json!({"path": "LIBRARY.json", "bytes": 2, "sha256": sha256_hex(b"{}"), "included": true}),
                );
            },
            zip::CompressionMethod::Stored,
        ),
        // 不是同步包
        case(
            &mut |m, _| m["kind"] = json!("something-else"),
            zip::CompressionMethod::Stored,
        ),
    ];
    for bad in cases {
        expect_unreadable(&mut p.b, &bad, &root);
    }
}

#[test]
fn hash_mismatch_rejected_and_staging_removed() {
    let mut p = pair();
    let root = p.b_root();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let bad = p.out.path().join("bad.zip");
    rewrite(
        &pkg,
        &bad,
        |m, _| {
            let f = m["files"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|f| f["path"].as_str().unwrap().ends_with(".pdf"))
                .unwrap();
            f["sha256"] = json!("0".repeat(64));
        },
        zip::CompressionMethod::Stored,
    );
    expect_unreadable(&mut p.b, &bad, &root);
}

/// 讓 A 的下一份同步包是「只帶變更」：A→B、B→A 各一次。
fn warm(p: &mut Pair) {
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    sync_once(&mut p.b, &mut p.a, &out);
}

/// B 的回收區放一篇代號不同、原文與 Alpha 相同的文獻（`content` 是那個檔的實際內容）。
fn trash_twin(p: &Pair, alpha: &str, content: Option<Vec<u8>>) {
    let (mut paper, _) = p.a.get_paper(alpha).unwrap();
    paper.id = paperreader_core::ids::new_id();
    paper.materials.truncate(1);
    let dir = p.b_root().join(".trash/20260101T000000Z-Twin");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("paper.json"), to_canonical_json(&paper).unwrap()).unwrap();
    let src = fs::read(p.a_root().join("papers/Alpha Paper/source.pdf")).unwrap();
    fs::write(dir.join("source.pdf"), content.unwrap_or(src)).unwrap();
}

#[test]
fn omitted_file_resolved_by_fingerprint() {
    let mut p = pair();
    warm(&mut p);
    let alpha = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "Alpha edited");
    // B 的 Alpha 原文不見了；回收區有一份同指紋的
    fs::remove_file(p.b_root().join("papers/Alpha Paper/source.pdf")).unwrap();
    trash_twin(&p, &alpha, None);
    let pkg = export_pkg(&mut p.a, p.out.path());
    let m: Value = serde_json::from_slice(&zip_entries(&pkg).last().unwrap().1).unwrap();
    let f = m["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == json!("papers/Alpha Paper/source.pdf"))
        .unwrap();
    assert_eq!(f["included"], json!(false));
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        preview.skipped_papers.is_empty(),
        "{:?}",
        preview.skipped_papers
    );
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert!(p.b_root().join("papers/Alpha Paper/source.pdf").is_file());
    assert_eq!(
        p.b.get_paper(&alpha).unwrap().0.bib.title.as_deref(),
        Some("Alpha edited")
    );
}

/// 找不到相符的候選檔時只略過那一項材料，不略過整篇。
#[test]
fn omitted_file_missing_skips_only_that_material() {
    let mut p = pair();
    warm(&mut p);
    let alpha = find(&p.a, "Alpha Paper");
    let beta = find(&p.a, "Beta Paper");
    set_title(&mut p.a, &alpha, "Alpha edited");
    set_title(&mut p.a, &beta, "Beta edited");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "new thought", nt.as_ref())
        .unwrap();
    let src = p.b_root().join("papers/Alpha Paper/source.pdf");
    fs::remove_file(&src).unwrap();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        preview.skipped_papers.is_empty(),
        "{:?}",
        preview.skipped_papers
    );
    assert!(preview.notices.iter().any(|n| {
        n.kind == paperreader_core::store::NoticeKind::MissingFile
            && n.paper.as_deref() == Some(alpha.as_str())
    }));
    assert!(
        preview
            .updated
            .iter()
            .any(|i| i.paper.as_deref() == Some(beta.as_str()) && i.kind == ItemKind::Bib)
    );
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert_eq!(p.b.read_notes(&alpha).unwrap().0.thoughts, "new thought");
    assert_eq!(
        p.b.get_paper(&alpha).unwrap().0.bib.title.as_deref(),
        Some("Alpha edited")
    );
    assert_eq!(
        p.b.get_paper(&beta).unwrap().0.bib.title.as_deref(),
        Some("Beta edited")
    );
    assert!(!src.exists());
}

#[test]
fn omitted_candidate_with_wrong_hash_treated_as_missing() {
    let mut p = pair();
    warm(&mut p);
    let alpha = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "Alpha edited");
    fs::remove_file(p.b_root().join("papers/Alpha Paper/source.pdf")).unwrap();
    trash_twin(&p, &alpha, Some(b"%PDF-not the same".to_vec()));
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(preview.skipped_papers.is_empty());
    assert!(
        preview
            .notices
            .iter()
            .any(|n| n.kind == paperreader_core::store::NoticeKind::MissingFile)
    );
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert!(!p.b_root().join("papers/Alpha Paper/source.pdf").exists());
}

#[test]
fn preview_writes_only_staging() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "Alpha edited");
    let root = p.b_root();
    let before = snapshot(&root);
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let after: BTreeMap<_, _> = snapshot(&root)
        .into_iter()
        .filter(|(k, _)| !k.starts_with(&format!(".pr-sync-{}", preview.preview_id)))
        .collect();
    let own = format!("logs/changes-{DEVICE_B}.jsonl");
    for (k, v) in &after {
        if *k != own {
            assert_eq!(before.get(k), Some(v), "{k}");
        }
    }
    assert_eq!(
        before.keys().filter(|k| **k != own).count(),
        after.keys().filter(|k| **k != own).count()
    );
}

#[test]
fn prepare_refused_while_unfinished() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "Alpha edited");
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    p.b.set_sync_apply_hook(Some(Box::new(|k| k != 2)));
    assert!(p.b.apply_sync_import(&preview.preview_id, &[]).is_err());
    assert!(matches!(
        p.b.prepare_sync_import(&pkg, &mut |_, _| {}),
        Err(LibError::ReadOnly)
    ));
}

#[test]
fn already_imported_flagged() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "Alpha edited");
    let pkg = export_pkg(&mut p.a, p.out.path());
    let first = import_defaults(&mut p.b, &pkg);
    assert!(!first.updated.is_empty());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(preview.package.already_imported);
    assert!(preview.added.is_empty());
    assert!(preview.updated.is_empty());
}

#[test]
fn second_prepare_discards_first_and_discard_removes_staging() {
    let mut p = pair();
    let root = p.b_root();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let one = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let two = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(!root.join(format!(".pr-sync-{}", one.preview_id)).exists());
    assert!(root.join(format!(".pr-sync-{}", two.preview_id)).exists());
    assert!(matches!(
        p.b.apply_sync_import(&one.preview_id, &[]),
        Err(LibError::NotFound { .. })
    ));
    p.b.discard_sync_import(&two.preview_id).unwrap();
    assert!(no_staging(&root));
    assert!(matches!(
        p.b.discard_sync_import(&two.preview_id),
        Err(LibError::NotFound { .. })
    ));
}

#[test]
fn reopen_cleans_unapplied_staging() {
    let mut p = pair();
    let root = p.b_root();
    let pkg = export_pkg(&mut p.a, p.out.path());
    p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(!no_staging(&root));
    drop(p.b);
    let b = LocalLibrary::open(&root, DEVICE_B).unwrap();
    assert!(no_staging(&root));
    assert!(!b.is_read_only());
}

#[test]
fn preview_counts() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let beta = find(&p.a, "Beta Paper");
    // 兩邊各加一篇、A 改一篇書目、A 刪一條劃線、兩邊各改想法（衝突）
    add_paper(&mut p.a, 501, "Only A", None);
    add_paper(&mut p.b, 502, "Only B", None);
    set_title(&mut p.a, &beta, "Beta edited");
    let (notes, nt) = p.a.read_notes(&alpha).unwrap();
    let t =
        p.a.delete_highlight(&alpha, &notes.highlights[0].id, nt.as_ref().unwrap())
            .unwrap();
    p.a.set_thoughts(&alpha, "A thinks", Some(&t)).unwrap();
    let nt = p.b.read_notes(&alpha).unwrap().1;
    p.b.set_thoughts(&alpha, "B thinks", nt.as_ref()).unwrap();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let kinds =
        |v: &Vec<paperreader_core::store::SyncItem>| -> Vec<(ItemKind, Option<String>, usize)> {
            v.iter()
                .map(|i| (i.kind, i.paper.clone(), i.count))
                .collect()
        };
    let only_a = find(&p.a, "Only A");
    assert_eq!(
        kinds(&preview.added),
        vec![(ItemKind::Paper, Some(only_a.clone()), 1)]
    );
    assert_eq!(
        kinds(&preview.updated),
        vec![(ItemKind::Bib, Some(beta.clone()), 1)]
    );
    assert_eq!(
        kinds(&preview.removed),
        vec![(ItemKind::Highlight, Some(alpha.clone()), 1)]
    );
    assert_eq!(preview.conflicts.len(), 1);
    assert_eq!(preview.conflicts[0].kind, ConflictKind::Thoughts);
    assert_eq!(preview.conflicts[0].id, "c1");
    assert_eq!(preview.conflicts[0].local.text.as_deref(), Some("B thinks"));
    assert_eq!(
        preview.conflicts[0].incoming.text.as_deref(),
        Some("A thinks")
    );
}

#[test]
fn thoughts_catch_up_after_join_reuses_other_device_node() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "from A", nt.as_ref()).unwrap();
    sync_once(&mut p.a, &mut p.b, p.out.path());
    // B 這台沒有自己的想法記錄；內容等於 A 那一筆的指紋 → 補記時不寫
    let before = change_lines(&p.b_root(), DEVICE_B)
        .into_iter()
        .filter(|l| l["type"] == json!("thoughts-written"))
        .count();
    export_pkg(&mut p.b, p.out.path());
    let after = change_lines(&p.b_root(), DEVICE_B)
        .into_iter()
        .filter(|l| l["type"] == json!("thoughts-written"))
        .count();
    assert_eq!(before, 0);
    assert_eq!(after, 0);
}

#[test]
fn thoughts_external_edit_caught_up() {
    let mut p = pair();
    let alpha = find(&p.b, "Alpha Paper");
    let nt = p.b.read_notes(&alpha).unwrap().1;
    p.b.set_thoughts(&alpha, "first", nt.as_ref()).unwrap();
    // 在筆記工具裡改（直接改檔）
    let path = p.b_root().join("notes/Alpha Paper.md");
    let t = fs::read_to_string(&path)
        .unwrap()
        .replace("first", "edited outside");
    fs::write(&path, t).unwrap();
    let pkg = export_pkg(&mut p.a, p.out.path());
    p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let lines: Vec<Value> = change_lines(&p.b_root(), DEVICE_B)
        .into_iter()
        .filter(|l| l["type"] == json!("thoughts-written"))
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[1]["data"]["sha256"],
        json!(thoughts_hash("edited outside"))
    );
    assert_eq!(lines[1]["data"]["prev"], json!([lines[0]["id"]]));
}

#[test]
fn full_mode_export_imports() {
    let mut p = pair();
    let pkg = p.out.path().join("full.zip");
    p.a.export_sync_package(&pkg, SyncMode::Full, None, &mut |_, _| {})
        .unwrap();
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(preview.conflicts.is_empty());
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
}
