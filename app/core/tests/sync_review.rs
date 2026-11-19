//! 同步包的檢查與防呆：各條的測試。
//! 依據：同步包的格式與合併規則。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::{BibDraft, PaperFile};
use paperreader_core::jsonio::{sha256_hex, to_canonical_json};
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{CheckLevel, ConflictKind, NoticeKind, ProblemKind, SyncMode};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

fn folder_of(lib: &LocalLibrary, id: &str) -> String {
    lib.list_papers()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap()
        .folder
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

/// 一個資料夾裡所有 `.md` 的內容接起來。
fn all_notes(root: &Path) -> String {
    let mut s = String::new();
    for e in fs::read_dir(root.join("notes")).unwrap().flatten() {
        s.push_str(&fs::read_to_string(e.path()).unwrap_or_default());
    }
    s
}

fn plan_dir(root: &Path) -> PathBuf {
    fs::read_dir(root)
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with(".pr-sync-"))
        .unwrap()
        .path()
}

// ---- H1：使用者寫過的文字，合併後在筆記檔或某一份衝突副本裡找得到 ----

/// A 刪了劃線；B 在那之後改了它的短評；另外兩邊的筆記檔標記以外的文字不同。
fn deleted_but_edited_with_outside_text(p: &mut Pair) -> String {
    let alpha = find(&p.a, "Alpha Paper");
    let (notes, nt) = p.a.read_notes(&alpha).unwrap();
    let hid = notes.highlights[0].id.clone();
    p.a.delete_highlight(&alpha, &hid, nt.as_ref().unwrap())
        .unwrap();
    // A 的筆記檔多一段自己的文字
    let path = p.a_root().join("notes/Alpha Paper.md");
    let t = fs::read_to_string(&path).unwrap() + "\nA's own paragraph\n";
    fs::write(&path, t).unwrap();
    // B 在刪除之後改短評
    let nt = p.b.read_notes(&alpha).unwrap().1.unwrap();
    p.b.set_highlight_comment(&alpha, &hid, "B wrote this after the delete", &nt)
        .unwrap();
    alpha
}

#[test]
fn h1_probe_local_edited_highlight_kept_with_outside_text_only() {
    let mut p = pair();
    deleted_but_edited_with_outside_text(&mut p);
    let r = sync_once(&mut p.a, &mut p.b, p.out.path());
    assert!(
        all_notes(&p.b_root()).contains("B wrote this after the delete"),
        "B 寫的短評要在筆記檔或某一份衝突副本裡"
    );
    assert_eq!(r.conflict_copies.len(), 2, "{:?}", r.conflict_copies);
}

#[test]
fn h1_probe_local_edited_highlight_kept_when_two_copies_needed() {
    let mut p = pair();
    let alpha = deleted_but_edited_with_outside_text(&mut p);
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "A thinks", nt.as_ref()).unwrap();
    let nt = p.b.read_notes(&alpha).unwrap().1;
    p.b.set_thoughts(&alpha, "B thinks", nt.as_ref()).unwrap();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let kinds: Vec<ConflictKind> = preview.conflicts.iter().map(|c| c.kind).collect();
    assert!(kinds.contains(&ConflictKind::Thoughts));
    assert!(kinds.contains(&ConflictKind::HighlightDeletedButEdited));
    let choices: Vec<_> = preview
        .conflicts
        .iter()
        .map(|c| (c.id.clone(), c.default))
        .collect();
    let r =
        p.b.apply_sync_import(&preview.preview_id, &choices)
            .unwrap();
    let all = all_notes(&p.b_root());
    assert!(all.contains("B wrote this after the delete"));
    assert!(all.contains("A thinks"));
    assert!(all.contains("B thinks"));
    // 兩份副本的檔名各帶自己的裝置代號前 6 碼
    let names: Vec<&str> = r.conflict_copies.iter().map(String::as_str).collect();
    assert_eq!(names.len(), 2);
    assert!(
        names
            .iter()
            .any(|n| n.contains(&format!("conflict {}", &DEVICE[..6])))
    );
    assert!(
        names
            .iter()
            .any(|n| n.contains(&format!("conflict {}", &DEVICE_B[..6])))
    );
}

// ---- H2：暫存檔落盤、執行前驗指紋 ----

#[test]
fn h2_probe_truncated_staged_file_not_renamed_on_resume() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "From A");
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    p.b.set_sync_apply_hook(Some(Box::new(|k| k != 1)));
    assert!(p.b.apply_sync_import(&preview.preview_id, &[]).is_err());
    let root = p.b_root();
    let folder = folder_of(&p.b, &alpha);
    let before = fs::read(root.join("papers").join(&folder).join("paper.json")).unwrap();
    drop(p.b);
    // 模擬斷電：暫存的 paper.json 是空的
    let dir = plan_dir(&root);
    let plan: Value = serde_json::from_slice(&fs::read(dir.join("plan.json")).unwrap()).unwrap();
    let src = plan["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["op"] == json!("write") && i["target"] == json!("paper.json"))
        .unwrap()["source"]
        .as_str()
        .unwrap()
        .to_string();
    fs::write(dir.join(&src), b"").unwrap();
    let b = LocalLibrary::open(&root, DEVICE_B).unwrap();
    assert_eq!(
        fs::read(root.join("papers").join(&folder).join("paper.json")).unwrap(),
        before,
        "空的暫存檔不可以蓋過目標"
    );
    assert!(b.is_read_only());
    assert!(
        b.check(CheckLevel::Quick)
            .unwrap()
            .iter()
            .any(|x| x.kind == ProblemKind::UnfinishedSyncImport)
    );
    assert!(b.list_papers().iter().any(|x| x.id == alpha));
}

#[test]
fn h2_staged_files_are_synced_before_plan() {
    // 計畫寫成的那一刻，每一個暫存檔的指紋都等於計畫記的 `after`
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "From A");
    add_paper(&mut p.a, 951, "New One", None);
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let root = p.b_root();
    let checked = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let c2 = checked.clone();
    p.b.set_sync_apply_hook(Some(Box::new(move |k| {
        if k == 1 {
            let dir = plan_dir(&root);
            let plan: Value =
                serde_json::from_slice(&fs::read(dir.join("plan.json")).unwrap()).unwrap();
            for i in plan["items"].as_array().unwrap() {
                if i["op"] == json!("write") {
                    let b = fs::read(dir.join(i["source"].as_str().unwrap())).unwrap();
                    assert_eq!(json!(sha256_hex(&b)), i["after"]);
                    *c2.lock().unwrap() += 1;
                }
                if i["op"] == json!("new-paper") {
                    for f in i["files"].as_array().unwrap() {
                        let b = fs::read(
                            dir.join(i["source"].as_str().unwrap())
                                .join(f["path"].as_str().unwrap()),
                        )
                        .unwrap();
                        assert_eq!(json!(sha256_hex(&b)), f["sha256"]);
                        *c2.lock().unwrap() += 1;
                    }
                }
            }
        }
        true
    })));
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert!(*checked.lock().unwrap() >= 3);
}

// ---- M1：寫入時重算的衝突與預覽不同 → Conflict ----

#[test]
fn m1_probe_conflict_appearing_after_preview_is_refused() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "same", nt.as_ref()).unwrap();
    sync_once(&mut p.a, &mut p.b, p.out.path());
    set_title(&mut p.a, &alpha, "From A");
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(preview.conflicts.is_empty());
    // 預覽之後在筆記工具裡改了想法（筆記檔不在計畫要寫的檔裡）
    let path = p.b_root().join("notes/Alpha Paper.md");
    let t = fs::read_to_string(&path)
        .unwrap()
        .replace("same", "changed after preview");
    fs::write(&path, t).unwrap();
    let r = p.b.apply_sync_import(&preview.preview_id, &[]);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert!(matches!(
        p.b.apply_sync_import(&preview.preview_id, &[]),
        Err(LibError::NotFound { .. })
    ));
}

// ---- M2：清空想法不會被撤銷 ----

#[test]
fn m2_probe_cleared_thoughts_within_throttle_stay_cleared() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    let t = p.a.set_thoughts(&alpha, "X", nt.as_ref()).unwrap();
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    assert_eq!(p.b.read_notes(&alpha).unwrap().0.thoughts, "X");
    // 10 分鐘內在應用裡清空
    p.a.set_thoughts(&alpha, "", Some(&t)).unwrap();
    sync_once(&mut p.b, &mut p.a, &out);
    sync_once(&mut p.a, &mut p.b, &out);
    assert_eq!(p.a.read_notes(&alpha).unwrap().0.thoughts, "");
    assert_eq!(p.b.read_notes(&alpha).unwrap().0.thoughts, "");
}

#[test]
fn m2_cleared_in_notes_tool_stays_cleared() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "X", nt.as_ref()).unwrap();
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    // 在筆記工具裡把想法刪掉（直接改檔）。只拿掉想法標記之間那一行：文獻代號（隨機的 ULID）最後一個字
    // 也可能是 X，單純拿掉第一個「X 換行」會改到屬性區的 `paperreader_paper`（約 1/32 的機率）
    let path = p.a_root().join("notes/Alpha Paper.md");
    let text = fs::read_to_string(&path).unwrap();
    let thoughts = "<!-- paperreader:thoughts -->\nX\n";
    assert!(text.contains(thoughts), "{text}");
    let t = text.replace(thoughts, "<!-- paperreader:thoughts -->\n");
    fs::write(&path, t).unwrap();
    assert_eq!(p.a.read_notes(&alpha).unwrap().0.thoughts, "");
    sync_once(&mut p.b, &mut p.a, &out);
    sync_once(&mut p.a, &mut p.b, &out);
    assert_eq!(p.a.read_notes(&alpha).unwrap().0.thoughts, "");
    assert_eq!(p.b.read_notes(&alpha).unwrap().0.thoughts, "");
}

#[test]
fn m2_clearing_is_never_throttled() {
    let (dir, mut lib) = temp_library();
    let (p, _) = add_paper(&mut lib, 1, "Alpha", None);
    let t = lib.set_thoughts(&p, "X", None).unwrap();
    lib.set_thoughts(&p, "", Some(&t)).unwrap();
    let lines = change_lines(dir.path(), DEVICE);
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[1]["data"]["sha256"],
        json!(paperreader_core::changes::thoughts_hash(""))
    );
    assert_eq!(lines[1]["data"]["prev"], json!([lines[0]["id"]]));
}

// ---- M3：清單的 device、package、created 先驗 ----

#[test]
fn m3_bad_manifest_ids_rejected() {
    let mut p = pair();
    let pkg = export_pkg(&mut p.a, p.out.path());
    for (key, bad) in [
        ("device", json!("ab:cd*?/x")),
        ("package", json!("not-an-id")),
        ("created", json!("yesterday")),
    ] {
        let dst = p.out.path().join(format!("bad-{key}.zip"));
        rewrite(&pkg, &dst, |m, _| m[key] = bad);
        let r = p.b.prepare_sync_import(&dst, &mut |_, _| {});
        assert!(
            matches!(r, Err(LibError::Unreadable { .. })),
            "{key}: {r:?}"
        );
    }
}

// ---- M4：在應用以外被改過的材料不補回；只帶變更時只略過那一項材料 ----

#[test]
fn m4_material_changed_outside_not_overwritten() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "From A");
    let src = p
        .b_root()
        .join("papers")
        .join(folder_of(&p.b, &alpha))
        .join("source.pdf");
    let mut changed = fs::read(&src).unwrap();
    changed.extend_from_slice(b"annotated elsewhere");
    fs::write(&src, &changed).unwrap();
    let pkg = p.out.path().join("full.zip");
    p.a.export_sync_package(&pkg, SyncMode::Full, None, &mut |_, _| {})
        .unwrap();
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        preview
            .notices
            .iter()
            .any(|n| n.kind == NoticeKind::MaterialChangedOutside),
        "{:?}",
        preview.notices
    );
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert_eq!(fs::read(&src).unwrap(), changed, "不覆寫");
    assert_eq!(
        p.b.get_paper(&alpha).unwrap().0.bib.title.as_deref(),
        Some("From A"),
        "其他的照常合併"
    );
}

#[test]
fn m4_changes_package_missing_candidate_skips_only_that_material() {
    let mut p = pair();
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    sync_once(&mut p.b, &mut p.a, &out);
    let alpha = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "Alpha edited");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "new thought", nt.as_ref())
        .unwrap();
    // B 的原文不見了；同步包只帶變更，省略了它
    let src = p
        .b_root()
        .join("papers")
        .join(folder_of(&p.b, &alpha))
        .join("source.pdf");
    fs::remove_file(&src).unwrap();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        preview.skipped_papers.is_empty(),
        "{:?}",
        preview.skipped_papers
    );
    assert!(
        preview.notices.iter().any(
            |n| n.kind == NoticeKind::MissingFile && n.paper.as_deref() == Some(alpha.as_str())
        )
    );
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert_eq!(
        p.b.get_paper(&alpha).unwrap().0.bib.title.as_deref(),
        Some("Alpha edited")
    );
    assert_eq!(p.b.read_notes(&alpha).unwrap().0.thoughts, "new thought");
    assert!(!src.exists());
}

// ---- L1：logs/ 只收兩種紀錄檔 ----

#[test]
fn l1_only_log_files_are_merged() {
    let mut p = pair();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let dst = p.out.path().join("evil.zip");
    rewrite(&pkg, &dst, |m, e| {
        e.push(("library/logs/evil.txt".into(), b"payload".to_vec()));
        m["files"].as_array_mut().unwrap().push(json!({
            "path": "logs/evil.txt", "bytes": 7, "sha256": sha256_hex(b"payload"), "included": true
        }));
    });
    import_defaults(&mut p.b, &dst);
    assert!(!p.b_root().join("logs/evil.txt").exists());
}

// ---- L2：譯文疊加檔的匯入端路徑要在同步包裡 ----

#[test]
fn l2_overlay_path_outside_package_not_read() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let mid = p.a.get_paper(&alpha).unwrap().0.materials[0].id.clone();
    let tok = p.a.get_paper(&alpha).unwrap().1;
    let ov =
        p.a.create_overlay(
            &alpha,
            paperreader_core::overlay::OverlayDraft {
                layout_model_pages: Vec::new(),
                source_material: mid,
                provider: "x".into(),
                model: "y".into(),
                source_lang: None,
                target_lang: "zh-TW".into(),
                segments: vec![paperreader_core::overlay::SegmentDraft {
                    page: 0,
                    bbox: [1.0, 1.0, 2.0, 2.0],
                    kind: "body".into(),
                    size: None,
                    source: "s".into(),
                }],
            },
            &tok,
        )
        .unwrap();
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    // A 的 paper.json 把這份譯文疊加檔的路徑改成指到同步包外
    let path = p.a_root().join("papers/Alpha Paper/paper.json");
    let mut paper: PaperFile = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for m in &mut paper.materials {
        if m.id == ov.material_id {
            m.path = "../../library.json".into();
        }
    }
    paper.bib.title = Some("From A".into());
    paper.bib.modified = Some(minutes_ago(-1));
    fs::write(&path, to_canonical_json(&paper).unwrap()).unwrap();
    p.a.rescan().unwrap();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        !preview
            .notices
            .iter()
            .any(|n| n.path.as_deref().is_some_and(|x| x.contains(".."))),
        "{:?}",
        preview.notices
    );
}

// ---- L3：因記錄要放進回收區的新文獻，資料夾名帶時間 ----

#[test]
fn l3_incoming_paper_sent_to_trash_gets_time_prefix() {
    let mut p = pair();
    let out = p.out.path().to_path_buf();
    let (q, _) = add_paper(&mut p.a, 960, "Q Paper", None);
    sync_once(&mut p.a, &mut p.b, &out);
    p.b.remove_paper(&q).unwrap();
    // B 的回收區那一份被手動刪掉（記錄還在）
    let folder =
        p.b.list_trash()
            .unwrap()
            .into_iter()
            .find(|t| t.paper_id == q)
            .unwrap()
            .folder;
    fs::remove_dir_all(p.b_root().join(".trash").join(folder)).unwrap();
    set_title(&mut p.a, &q, "Q edited");
    sync_once(&mut p.a, &mut p.b, &out);
    let t =
        p.b.list_trash()
            .unwrap()
            .into_iter()
            .find(|t| t.paper_id == q)
            .unwrap();
    assert!(t.removed_at.is_some(), "{}", t.folder);
}

// ---- L6：準備匯入逐檔回報進度 ----

#[test]
fn l6_prepare_reports_progress_per_file() {
    let mut p = pair();
    let pkg = p.out.path().join("full.zip");
    p.a.export_sync_package(&pkg, SyncMode::Full, None, &mut |_, _| {})
        .unwrap();
    let mut calls: Vec<(u64, u64)> = Vec::new();
    p.b.prepare_sync_import(&pkg, &mut |d, t| calls.push((d, t)))
        .unwrap();
    assert!(calls.len() > 3, "{calls:?}");
    assert!(calls.windows(2).all(|w| w[0].0 <= w[1].0));
    let (d, t) = *calls.last().unwrap();
    assert_eq!(d, t);
}

// ---- L8：補的測試 ----

#[test]
fn l8_unreadable_groups_keep_local_membership() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let out = p.out.path().to_path_buf();
    // A 把 Alpha 移出群組（所屬較新）；B 的 library.json 群組寫壞
    let (groups, _) = p.a.list_groups().unwrap();
    let tok = p.a.get_paper(&alpha).unwrap().1;
    p.a.remove_from_group(&alpha, &groups[0].id, &tok).unwrap();
    let (q, _) = add_paper(&mut p.a, 961, "Grouped New", None);
    let tok = p.a.get_paper(&q).unwrap().1;
    p.a.add_to_group(&q, &groups[0].id, &tok).unwrap();
    let lp = p.b_root().join("library.json");
    let mut lf: Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    lf["groups"] = json!({"broken": true});
    fs::write(&lp, lf.to_string()).unwrap();
    let r = sync_once(&mut p.a, &mut p.b, &out);
    assert_eq!(
        p.b.get_paper(&alpha).unwrap().0.groups,
        vec![groups[0].id.clone()],
        "兩邊都有的文獻留本機的所屬"
    );
    assert_eq!(
        p.b.get_paper(&q).unwrap().0.groups,
        vec![groups[0].id.clone()]
    );
    let _ = r;
    assert!(
        p.b.check(CheckLevel::Quick)
            .unwrap()
            .iter()
            .any(|x| x.kind == ProblemKind::UnknownGroupRef
                || x.kind == ProblemKind::UnreadableGroups)
    );
}

#[test]
fn l8_resolution_records_precede_file_writes() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "A version", nt.as_ref()).unwrap();
    let nt = p.b.read_notes(&alpha).unwrap().1;
    p.b.set_thoughts(&alpha, "B version", nt.as_ref()).unwrap();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let c = &preview.conflicts[0];
    assert_eq!(c.kind, ConflictKind::Thoughts);
    let root = p.b_root();
    let notes_before = fs::read(root.join("notes/Alpha Paper.md")).unwrap();
    // 停在第 2 項（附加記錄之後、其他寫入之前）
    p.b.set_sync_apply_hook(Some(Box::new(|k| k != 2)));
    let r = p.b.apply_sync_import(
        &preview.preview_id,
        &[(
            c.id.clone(),
            paperreader_core::store::ConflictChoice::Incoming,
        )],
    );
    assert!(r.is_err());
    let lines = change_lines(&root, DEVICE_B);
    assert!(lines.iter().any(|l| l["type"] == json!("thoughts-written")
        && l["data"]["prev"].as_array().unwrap().len() == 2));
    assert_eq!(
        fs::read(root.join("notes/Alpha Paper.md")).unwrap(),
        notes_before
    );
}

#[test]
fn l8_compressed_entry_rejected() {
    let mut p = pair();
    let pkg = export_pkg(&mut p.a, p.out.path());
    // 把第一項的壓縮方式改成 8（deflate）：本地檔頭與中央目錄各一處
    let mut b = fs::read(&pkg).unwrap();
    let local = b
        .windows(4)
        .position(|w| w == [0x50, 0x4b, 0x03, 0x04])
        .unwrap();
    b[local + 8] = 8;
    let central = b
        .windows(4)
        .position(|w| w == [0x50, 0x4b, 0x01, 0x02])
        .unwrap();
    b[central + 10] = 8;
    let bad = p.out.path().join("deflated.zip");
    fs::write(&bad, b).unwrap();
    let r = p.b.prepare_sync_import(&bad, &mut |_, _| {});
    match r {
        Err(LibError::Unreadable { reason, .. }) => {
            assert!(reason.contains("compression"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
}
