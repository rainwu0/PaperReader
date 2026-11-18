//! 參考文獻清單的同步：合併層的純計算與兩台文獻庫走同步包（含中斷、「連錯了」）。

mod common;

use common::*;
use paperreader_core::changes::LINK_JUDGMENT_DELETED;
use paperreader_core::jsonio::sha256_hex;
use paperreader_core::local::LocalLibrary;
use paperreader_core::references::*;
use paperreader_core::store::{ItemKind, NoticeKind};
use paperreader_core::sync::merge::references::merge_references;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const SHA_NOW: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const SHA_OLD: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const T1: &str = "2026-11-17T08:00:00.000Z";
const T2: &str = "2026-11-17T09:00:00.000Z";

fn re(text: &str) -> RefEntry {
    RefEntry {
        number: None,
        label: None,
        text: text.into(),
        page: 0,
        bbox: None,
        doi: None,
        arxiv: None,
        year: None,
        extra: Default::default(),
    }
}

fn jd(id: &str, paper: &str, text: &str) -> Judgment {
    Judgment {
        id: id.into(),
        kind: "reject".into(),
        paper: paper.into(),
        entry_key: entry_key(text, 1).unwrap(),
        key_version: 1,
        entry_text: text.into(),
        entry_doi: None,
        entry_arxiv: None,
        created: T1.into(),
        created_by: DEVICE.into(),
        extra: Default::default(),
    }
}

fn rf(sha: &str, version: u32, created: &str, by: &str, texts: &[&str]) -> ReferencesFile {
    ReferencesFile {
        format: 1,
        modified: Some(created.into()),
        modified_by: Some(by.into()),
        extraction: Extraction {
            extractor: "links".into(),
            version,
            source_material: "M1".into(),
            source_sha256: sha.into(),
            created: created.into(),
            created_by: by.into(),
            extra: Default::default(),
        },
        entries: texts.iter().map(|t| re(t)).collect(),
        judgments: vec![],
        extra: Default::default(),
    }
}

fn texts(f: &ReferencesFile) -> Vec<&str> {
    f.entries.iter().map(|e| e.text.as_str()).collect()
}

fn ids(f: &ReferencesFile) -> Vec<&str> {
    f.judgments.iter().map(|j| j.id.as_str()).collect()
}

fn merge(
    l: Option<&ReferencesFile>,
    r: Option<&ReferencesFile>,
    deleted: &[&str],
) -> (
    Option<ReferencesFile>,
    paperreader_core::sync::merge::MergeCtx,
) {
    let mut ctx = mctx(&[]);
    let deleted: HashSet<String> = deleted.iter().map(|s| s.to_string()).collect();
    let out = merge_references(l, r, &deleted, Some(SHA_NOW), "P", Some("t"), &mut ctx);
    (out, ctx)
}

// ---------- 10. 合併層 ----------

#[test]
fn only_remote_is_copied_without_deleted_judgments() {
    let mut r = rf(SHA_NOW, 1, T1, DEVICE, &["a", "b"]);
    r.judgments = vec![jd("j-1", "X", "a"), jd("j-2", "Y", "b")];
    let (out, ctx) = merge(None, Some(&r), &["j-2"]);
    let out = out.unwrap();
    assert_eq!(ids(&out), ["j-1"]);
    assert_eq!(texts(&out), ["a", "b"]);
    assert!(
        ctx.summary
            .added
            .contains_key(&(ItemKind::References, Some("P".into())))
    );
}

#[test]
fn only_local_drops_deleted_judgments_and_otherwise_stays() {
    let mut l = rf(SHA_NOW, 1, T1, DEVICE_B, &["a"]);
    l.judgments = vec![jd("j-1", "X", "a"), jd("j-2", "Y", "a")];
    assert!(merge(Some(&l), None, &[]).0.is_none());
    let out = merge(Some(&l), None, &["j-1"]).0.unwrap();
    assert_eq!(ids(&out), ["j-2"]);
    assert_eq!(out.modified.as_deref(), Some(NOW));
    assert_eq!(out.modified_by.as_deref(), Some(DEVICE_B));
}

#[test]
fn same_content_is_not_rewritten() {
    let l = rf(SHA_NOW, 1, T1, DEVICE, &["a"]);
    let mut r = l.clone();
    r.modified = Some(T2.into());
    assert!(merge(Some(&l), Some(&r), &[]).0.is_none());
}

#[test]
fn extraction_from_the_current_source_wins() {
    // 本機的較新、版本較高，但抽自舊的原文
    let l = rf(SHA_OLD, 3, T2, DEVICE_B, &["old"]);
    let r = rf(SHA_NOW, 1, T1, DEVICE, &["new"]);
    let out = merge(Some(&l), Some(&r), &[]).0.unwrap();
    assert_eq!(texts(&out), ["new"]);
    assert_eq!(out.extraction.source_sha256, SHA_NOW);
    // 反過來：匯入的抽自舊原文，本機不動
    assert!(merge(Some(&r), Some(&l), &[]).0.is_none());
}

#[test]
fn higher_version_of_the_same_extractor_wins() {
    let l = rf(SHA_NOW, 1, T2, DEVICE_B, &["v1"]);
    let r = rf(SHA_NOW, 2, T1, DEVICE, &["v2"]);
    let out = merge(Some(&l), Some(&r), &[]).0.unwrap();
    assert_eq!(texts(&out), ["v2"]);
    assert!(merge(Some(&r), Some(&l), &[]).0.is_none());
    // 抽取者不同時不比版本，往下比時間
    let mut r2 = rf(SHA_NOW, 9, T1, DEVICE, &["other"]);
    r2.extraction.extractor = "other".into();
    assert!(merge(Some(&l), Some(&r2), &[]).0.is_none());
}

#[test]
fn same_version_newer_extraction_wins() {
    let l = rf(SHA_NOW, 1, T1, DEVICE_B, &["older"]);
    let r = rf(SHA_NOW, 1, T2, DEVICE, &["newer"]);
    let out = merge(Some(&l), Some(&r), &[]).0.unwrap();
    assert_eq!(texts(&out), ["newer"]);
    // 時間相同比裝置代號
    let r = rf(SHA_NOW, 1, T1, DEVICE, &["same time, smaller device"]);
    assert!(merge(Some(&l), Some(&r), &[]).0.is_none());
}

#[test]
fn unknown_fields_of_the_losing_entries_move_to_the_winner() {
    let mut l = rf(
        SHA_NOW,
        1,
        T1,
        DEVICE_B,
        &["[1] Same Title. 2020.", "Only local"],
    );
    l.entries[0].extra.insert("openalex".into(), json!("W1"));
    l.entries[0].extra.insert("both".into(), json!("local"));
    l.entries[1].extra.insert("lost".into(), json!(true));
    let mut r = rf(
        SHA_NOW,
        2,
        T1,
        DEVICE,
        &["Same Title. 2020.", "Only remote"],
    );
    r.entries[0].extra.insert("both".into(), json!("remote"));
    let out = merge(Some(&l), Some(&r), &[]).0.unwrap();
    assert_eq!(out.entries[0].extra.get("openalex"), Some(&json!("W1")));
    assert_eq!(out.entries[0].extra.get("both"), Some(&json!("remote")));
    assert!(out.entries[1].extra.is_empty());
}

#[test]
fn judgments_union_by_id_minus_deleted_local_wins_same_id() {
    let mut l = rf(SHA_NOW, 1, T1, DEVICE_B, &["a"]);
    l.judgments = vec![jd("j-1", "X", "a"), jd("j-2", "Y", "a")];
    let mut r = l.clone();
    let mut changed = jd("j-1", "X", "a");
    changed.kind = "manual".into();
    r.judgments = vec![changed, jd("j-3", "Z", "a"), jd("j-4", "W", "a")];
    let out = merge(Some(&l), Some(&r), &["j-4"]).0.unwrap();
    assert_eq!(ids(&out), ["j-1", "j-2", "j-3"]);
    assert_eq!(out.judgments[0].kind, "reject");
}

#[test]
fn top_level_unknown_fields_local_kept_remote_added() {
    let mut l = rf(SHA_NOW, 1, T1, DEVICE_B, &["a"]);
    l.extra.insert("k".into(), json!("local"));
    let mut r = l.clone();
    r.extra.insert("k".into(), json!("remote"));
    r.extra.insert("n".into(), json!(1));
    let out = merge(Some(&l), Some(&r), &[]).0.unwrap();
    assert_eq!(out.extra.get("k"), Some(&json!("local")));
    assert_eq!(out.extra.get("n"), Some(&json!(1)));
}

/// `n` 項判斷，代號從 `from` 起，原文 `len` 個字。
fn many(from: usize, n: usize, len: usize) -> Vec<Judgment> {
    (from..from + n)
        .map(|i| {
            let mut j = jd(&format!("j-{i:026}"), "X", "a");
            j.entry_text = "x".repeat(len);
            j
        })
        .collect()
}

fn too_large(ctx: &paperreader_core::sync::merge::MergeCtx) -> bool {
    ctx.notices
        .iter()
        .any(|n| n.kind == NoticeKind::TooLargeToMerge && n.paper.as_deref() == Some("P"))
}

/// 合併的結果超過上限（判斷 5000 項或 2 MB）：不寫，本機的不動，提醒。
#[test]
fn merged_result_over_a_limit_is_not_written() {
    // 條數：兩邊各 3000 項不同的判斷
    let mut l = rf(SHA_NOW, 1, T1, DEVICE_B, &["a"]);
    l.judgments = many(0, 3000, 1);
    let mut r = l.clone();
    r.judgments = many(3000, 3000, 1);
    let (out, ctx) = merge(Some(&l), Some(&r), &[]);
    assert!(out.is_none());
    assert!(too_large(&ctx));
    assert!(ctx.summary.updated.is_empty());
    // 位元組：兩邊各 1000 項、每項原文 1000 字，合起來超過 2 MB
    l.judgments = many(0, 1000, 1000);
    r.judgments = many(1000, 1000, 1000);
    let (out, ctx) = merge(Some(&l), Some(&r), &[]);
    assert!(out.is_none());
    assert!(too_large(&ctx));
    // 刪除記錄拿掉一些之後在上限內：照寫
    let deleted: Vec<String> = (0..1000).map(|i| format!("j-{i:026}")).collect();
    let deleted: Vec<&str> = deleted.iter().map(String::as_str).collect();
    let (out, ctx) = merge(Some(&l), Some(&r), &deleted);
    assert_eq!(out.unwrap().judgments.len(), 1000);
    assert!(!too_large(&ctx));
    // 只在匯入那邊、本身就超過上限（在應用以外改的）：不帶過來
    let mut big = rf(SHA_NOW, 1, T1, DEVICE, &["a"]);
    big.judgments = many(0, 5001, 1);
    let (out, ctx) = merge(None, Some(&big), &[]);
    assert!(out.is_none());
    assert!(too_large(&ctx));
    assert!(ctx.summary.added.is_empty());
}

// ---------- 兩台文獻庫 ----------

fn folder(lib: &LocalLibrary, id: &str) -> String {
    lib.list_papers()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap()
        .folder
}

fn refs_file(lib: &LocalLibrary, root: &Path, id: &str) -> PathBuf {
    root.join("papers")
        .join(folder(lib, id))
        .join(REFERENCES_FILE)
}

fn source_of(lib: &LocalLibrary, id: &str) -> (String, String) {
    let m = lib.get_paper(id).unwrap().0.materials[0].clone();
    (m.id, m.sha256.unwrap())
}

fn write(lib: &mut LocalLibrary, id: &str, version: u32, texts: &[&str]) {
    let (mid, sha) = source_of(lib, id);
    lib.write_references(
        id,
        ReferencesDraft {
            extractor: "links".into(),
            version,
            source_material: mid,
            source_sha256: sha,
            entries: texts
                .iter()
                .map(|t| EntryDraft {
                    number: None,
                    label: None,
                    text: t.to_string(),
                    page: 0,
                    bbox: None,
                    doi: None,
                    arxiv: None,
                    year: None,
                })
                .collect(),
        },
    )
    .unwrap();
}

fn read(lib: &LocalLibrary, id: &str) -> ReferencesFile {
    lib.read_references(id).unwrap().unwrap().0
}

const ENTRY: &str = "A. Vaswani et al. Attention is all you need. In NeurIPS, 2017.";

#[test]
fn two_devices_extract_the_same_paper_and_end_up_with_one_file() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let beta = find(&p.a, "Beta Paper");
    write(&mut p.a, &alpha, 1, &["from A"]);
    write(&mut p.b, &alpha, 1, &["from B", ENTRY]);
    let tok = p.b.read_references(&alpha).unwrap().unwrap().1;
    p.b.add_link_judgment(&alpha, 1, JudgmentKind::Reject, &beta, &tok)
        .unwrap();
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    sync_once(&mut p.b, &mut p.a, &out);
    // B 抽得較晚，抽出的部分取 B 的；判斷照樣在
    let a = read(&p.a, &alpha);
    assert_eq!(texts(&a), ["from B", ENTRY]);
    assert_eq!(a.judgments.len(), 1);
    assert_eq!(sync_state(&p.a, &p.a_root()), sync_state(&p.b, &p.b_root()));
    assert!(
        p.a.check(paperreader_core::store::CheckLevel::Quick)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn changes_package_carries_a_file_where_only_judgments_changed() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let beta = find(&p.a, "Beta Paper");
    write(&mut p.a, &alpha, 1, &[ENTRY]);
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    sync_once(&mut p.b, &mut p.a, &out);
    // 只加一項判斷，再匯出「只帶變更」
    let tok = p.a.read_references(&alpha).unwrap().unwrap().1;
    let (j, _) =
        p.a.add_link_judgment(&alpha, 0, JudgmentKind::Reject, &beta, &tok)
            .unwrap();
    let pkg = export_pkg(&mut p.a, &out);
    let names: Vec<String> = zip_entries(&pkg).into_iter().map(|e| e.0).collect();
    assert!(
        names
            .iter()
            .any(|n| n.ends_with(&format!("/{REFERENCES_FILE}")))
    );
    import_defaults(&mut p.b, &pkg);
    assert_eq!(ids(&read(&p.b, &alpha)), [j.id.as_str()]);
}

#[test]
fn paper_only_on_the_other_side_brings_its_references() {
    let mut p = pair();
    let (pid, _) = add_paper(&mut p.a, 50, "New On A", Some(3));
    write(&mut p.a, &pid, 1, &[ENTRY]);
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    assert_eq!(texts(&read(&p.b, &pid)), [ENTRY]);
}

#[test]
fn unreadable_or_newer_files_are_not_merged() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    write(&mut p.a, &alpha, 1, &["A"]);
    write(&mut p.b, &alpha, 1, &["B"]);
    let out = p.out.path().to_path_buf();
    let bfile = refs_file(&p.b, &p.b_root(), &alpha);
    let afile = refs_file(&p.a, &p.a_root(), &alpha);
    // 本機讀不懂
    fs::write(&bfile, b"garbage").unwrap();
    let pkg = export_pkg(&mut p.a, &out);
    let pre = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        pre.notices
            .iter()
            .any(|n| n.kind == NoticeKind::LocalUnreadable && n.paper.as_deref() == Some(&alpha))
    );
    p.b.discard_sync_import(&pre.preview_id).unwrap();
    // 匯入的讀不懂
    fs::remove_file(&bfile).unwrap();
    write(&mut p.b, &alpha, 1, &["B"]); // 寫回一份讀得懂的
    let good = fs::read(&afile).unwrap();
    fs::write(&afile, b"garbage").unwrap();
    let pkg = export_pkg(&mut p.a, &out);
    fs::write(&afile, &good).unwrap();
    let before = fs::read(&bfile).unwrap();
    let pre = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        pre.notices
            .iter()
            .any(|n| n.kind == NoticeKind::IncomingUnreadable && n.paper.as_deref() == Some(&alpha))
    );
    let choices: Vec<_> = pre
        .conflicts
        .iter()
        .map(|c| (c.id.clone(), c.default))
        .collect();
    p.b.apply_sync_import(&pre.preview_id, &choices).unwrap();
    assert_eq!(fs::read(&bfile).unwrap(), before);
    // 匯入的格式較新
    let mut v: Value = serde_json::from_slice(&good).unwrap();
    v["format"] = json!(2);
    fs::write(&afile, v.to_string()).unwrap();
    let pkg = export_pkg(&mut p.a, &out);
    let pre = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        pre.notices
            .iter()
            .any(|n| n.kind == NoticeKind::NewerFormat && n.paper.as_deref() == Some(&alpha))
    );
    p.b.discard_sync_import(&pre.preview_id).unwrap();
    assert_eq!(fs::read(&bfile).unwrap(), before);
}

/// 走兩台：兩邊各有 3000 項不同的判斷（各自在上限內），合起來超過 5000 項：本機的檔不動，提醒。
#[test]
fn sync_that_would_exceed_the_limits_leaves_the_local_file() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    write(&mut p.a, &alpha, 1, &["a"]);
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    let put = |lib: &LocalLibrary, root: &Path, from: usize| {
        let f = refs_file(lib, root, &alpha);
        let mut v: Value = serde_json::from_slice(&fs::read(&f).unwrap()).unwrap();
        v["judgments"] = serde_json::to_value(many(from, 3000, 1)).unwrap();
        fs::write(&f, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        f
    };
    put(&p.a, &p.a_root(), 0);
    let bfile = put(&p.b, &p.b_root(), 3000);
    let before = fs::read(&bfile).unwrap();
    let pkg = export_pkg(&mut p.a, &out);
    let pre = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        pre.notices
            .iter()
            .any(|n| n.kind == NoticeKind::TooLargeToMerge && n.paper.as_deref() == Some(&alpha))
    );
    let choices: Vec<_> = pre
        .conflicts
        .iter()
        .map(|c| (c.id.clone(), c.default))
        .collect();
    p.b.apply_sync_import(&pre.preview_id, &choices).unwrap();
    assert_eq!(fs::read(&bfile).unwrap(), before);
    assert_eq!(read(&p.b, &alpha).judgments.len(), 3000);
}

/// 只在匯入那邊的新文獻，它的 `references.json` 讀不懂或格式較新：提醒、不帶那份檔，文獻照樣加進來。
#[test]
fn new_paper_with_unreadable_or_newer_references_comes_without_it() {
    for (body, kind) in [
        (b"garbage".to_vec(), NoticeKind::IncomingUnreadable),
        (Vec::new(), NoticeKind::NewerFormat),
    ] {
        let mut p = pair();
        let (pid, _) = add_paper(&mut p.a, 70, "New With Refs", Some(3));
        write(&mut p.a, &pid, 1, &[ENTRY]);
        let afile = refs_file(&p.a, &p.a_root(), &pid);
        if body.is_empty() {
            let mut v: Value = serde_json::from_slice(&fs::read(&afile).unwrap()).unwrap();
            v["format"] = json!(2);
            fs::write(&afile, v.to_string()).unwrap();
        } else {
            fs::write(&afile, &body).unwrap();
        }
        let out = p.out.path().to_path_buf();
        let pkg = export_pkg(&mut p.a, &out);
        let pre = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
        assert!(
            pre.notices
                .iter()
                .any(|n| n.kind == kind && n.paper.as_deref() == Some(&pid)),
            "{kind:?}: {:?}",
            pre.notices
        );
        let choices: Vec<_> = pre
            .conflicts
            .iter()
            .map(|c| (c.id.clone(), c.default))
            .collect();
        p.b.apply_sync_import(&pre.preview_id, &choices).unwrap();
        assert!(p.b.get_paper(&pid).is_ok());
        assert!(!refs_file(&p.b, &p.b_root(), &pid).exists());
    }
}

#[test]
fn unknown_per_paper_file_in_a_package_is_ignored() {
    // 模擬舊版應用看到 references.json：同步包裡多一個這一版不認得的每篇檔
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let dst = out.join("with-future.zip");
    let rel = format!("library/papers/{}/future.json", folder(&p.a, &alpha));
    let body = br#"{"format": 1, "whatever": true}"#.to_vec();
    let sha = sha256_hex(&body);
    let rel2 = rel.clone();
    rewrite(&pkg, &dst, move |m, e| {
        m["files"].as_array_mut().unwrap().push(json!({
            "path": rel2.trim_start_matches("library/"),
            "bytes": body.len(),
            "sha256": sha,
            "included": true
        }));
        e.push((rel2, body));
    });
    import_defaults(&mut p.b, &dst);
    assert!(
        !p.b_root()
            .join("papers")
            .join(folder(&p.b, &alpha))
            .join("future.json")
            .exists()
    );
}

#[test]
fn references_file_is_never_a_candidate_for_a_missing_material() {
    // 匯入那邊一篇新文獻的原文沒放進同步包；它的指紋剛好等於本機某份 references.json 的內容：不可以拿它來用
    let mut p = pair();
    let alpha = find(&p.b, "Alpha Paper");
    write(&mut p.b, &alpha, 1, &[ENTRY]);
    let bytes = fs::read(refs_file(&p.b, &p.b_root(), &alpha)).unwrap();
    let sha = sha256_hex(&bytes);
    let (pid, _) = add_paper(&mut p.a, 60, "Only A", Some(2));
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let dst = out.join("omitted.zip");
    let src_rel = format!("library/papers/{}/source.pdf", folder(&p.a, &pid));
    let src_rel2 = src_rel.clone();
    rewrite(&pkg, &dst, move |m, e| {
        for f in m["files"].as_array_mut().unwrap() {
            if format!("library/{}", f["path"].as_str().unwrap()) == src_rel2 {
                f["included"] = json!(false);
                f["sha256"] = json!(sha);
                f["bytes"] = json!(bytes.len());
            }
        }
        e.retain(|(n, _)| *n != src_rel2);
    });
    let pre = p.b.prepare_sync_import(&dst, &mut |_, _| {}).unwrap();
    assert!(
        pre.notices
            .iter()
            .any(|n| n.kind == NoticeKind::MissingFile && n.paper.as_deref() == Some(&pid))
    );
    p.b.discard_sync_import(&pre.preview_id).unwrap();
}

#[test]
fn interrupted_removal_is_finished_by_the_next_merge() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let beta = find(&p.a, "Beta Paper");
    write(&mut p.a, &alpha, 1, &[ENTRY]);
    let tok = p.a.read_references(&alpha).unwrap().unwrap().1;
    let (j, _) =
        p.a.add_link_judgment(&alpha, 0, JudgmentKind::Reject, &beta, &tok)
            .unwrap();
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    assert_eq!(ids(&read(&p.b, &alpha)), [j.id.as_str()]);
    // A 刪這一項：記錄寫了、檔還沒寫就被中斷
    p.a.set_change_hook(Some(Box::new(|_| false)));
    let tok = p.a.read_references(&alpha).unwrap().unwrap().1;
    assert!(
        p.a.remove_link_judgments(&alpha, std::slice::from_ref(&j.id), &tok)
            .is_err()
    );
    p.a.set_change_hook(None);
    assert_eq!(read(&p.a, &alpha).judgments.len(), 1);
    assert!(
        change_lines(&p.a_root(), DEVICE)
            .iter()
            .any(|l| l["type"] == json!(LINK_JUDGMENT_DELETED))
    );
    // 下一次合併在兩邊都拿掉
    sync_once(&mut p.a, &mut p.b, &out);
    assert!(read(&p.b, &alpha).judgments.is_empty());
    sync_once(&mut p.b, &mut p.a, &out);
    assert!(read(&p.a, &alpha).judgments.is_empty());
}

/// 走兩台：A 的逐篇處理補了 DOI（書目的時間不變），B 改了篇名；兩邊同步之後都有，不問。
#[test]
fn auto_filled_bib_and_user_edit_both_survive_sync() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    assert!(
        p.a.fill_bib_blanks(
            &alpha,
            paperreader_core::format::BibFill {
                doi: Some("10.48550/arxiv.2304.08069".into()),
                url: Some("https://arxiv.org/abs/2304.08069".into()),
                ..Default::default()
            },
        )
        .unwrap()
    );
    let (paper, tok) = p.b.get_paper(&alpha).unwrap();
    let draft = paperreader_core::format::BibDraft {
        title: Some("Alpha Renamed".into()),
        authors: paper.bib.authors.clone(),
        year: paper.bib.year,
        ..Default::default()
    };
    p.b.update_bib(&alpha, draft, &tok).unwrap();
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let pre = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(pre.conflicts.is_empty());
    let r = p.b.apply_sync_import(&pre.preview_id, &[]).unwrap();
    let _ = r;
    sync_once(&mut p.b, &mut p.a, &out);
    for lib in [&p.a, &p.b] {
        let bib = lib.get_paper(&alpha).unwrap().0.bib;
        assert_eq!(bib.title.as_deref(), Some("Alpha Renamed"));
        assert_eq!(bib.doi.as_deref(), Some("10.48550/arxiv.2304.08069"));
    }
    assert_eq!(sync_state(&p.a, &p.a_root()), sync_state(&p.b, &p.b_root()));
}

/// 走兩台：A 補了年份、使用者在 A 清掉年份；B 沒補、使用者在 B 改篇名（較新）。
/// 互相同步之後，B 的逐篇處理不補年份；再同步回 A，A 的年份仍是空的、`cleared` 仍有 `year`。
#[test]
fn cleared_on_the_older_side_survives_a_newer_edit_elsewhere() {
    use paperreader_core::format::{BibDraft, BibFill};
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    // 1. A 的逐篇處理補了年份（兩台的這篇原本都沒有年份）
    assert_eq!(p.a.get_paper(&alpha).unwrap().0.bib.year, None);
    let out = p.out.path().to_path_buf();
    let fill = BibFill {
        year: Some(2023),
        ..Default::default()
    };
    assert!(p.a.fill_bib_blanks(&alpha, fill.clone()).unwrap());
    // 2. 使用者在 A 清掉年份（T1）
    let (paper, tok) = p.a.get_paper(&alpha).unwrap();
    let draft = BibDraft {
        title: paper.bib.title.clone(),
        authors: paper.bib.authors.clone(),
        year: None,
        ..Default::default()
    };
    p.a.update_bib(&alpha, draft, &tok).unwrap();
    assert_eq!(p.a.get_paper(&alpha).unwrap().0.bib.cleared, vec!["year"]);
    // 3. 使用者在 B 改篇名（T2 晚於 T1）
    std::thread::sleep(std::time::Duration::from_millis(5));
    let (paper, tok) = p.b.get_paper(&alpha).unwrap();
    let draft = BibDraft {
        title: Some("Alpha Renamed on B".into()),
        authors: paper.bib.authors.clone(),
        ..Default::default()
    };
    p.b.update_bib(&alpha, draft, &tok).unwrap();
    // 4. 互相同步
    sync_once(&mut p.a, &mut p.b, &out);
    sync_once(&mut p.b, &mut p.a, &out);
    for lib in [&p.a, &p.b] {
        let bib = lib.get_paper(&alpha).unwrap().0.bib;
        assert_eq!(bib.title.as_deref(), Some("Alpha Renamed on B"));
        assert_eq!(bib.year, None);
        assert_eq!(bib.cleared, vec!["year"]);
    }
    // 5. B 的逐篇處理想補年份：不補
    assert!(!p.b.fill_bib_blanks(&alpha, fill).unwrap());
    // 6. 再同步回 A
    sync_once(&mut p.b, &mut p.a, &out);
    let bib = p.a.get_paper(&alpha).unwrap().0.bib;
    assert_eq!(bib.year, None);
    assert_eq!(bib.cleared, vec!["year"]);
    assert_eq!(sync_state(&p.a, &p.a_root()), sync_state(&p.b, &p.b_root()));
}

/// 「連錯了」：按「不是這篇」之後，抽取版本加一重抽、同步到另一台都不再連。
#[test]
fn reject_survives_reextraction_and_sync() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    let beta = find(&p.a, "Beta Paper");
    write(&mut p.a, &alpha, 1, &["Intro text", ENTRY]);
    let tok = p.a.read_references(&alpha).unwrap().unwrap().1;
    p.a.add_link_judgment(&alpha, 1, JudgmentKind::Reject, &beta, &tok)
        .unwrap();
    // 版本加一重抽：斷字與編號不同、順序變了
    write(
        &mut p.a,
        &alpha,
        2,
        &[
            "[7] A. Vaswani et al. Atten- tion is all you need. In NeurIPS, 2017.",
            "Intro text",
        ],
    );
    let check = |lib: &LocalLibrary| {
        let doc = read(lib, &alpha);
        let r = resolve_judgments(&doc.entries, &doc.judgments);
        assert_eq!(doc.extraction.version, 2);
        assert_eq!(
            r.entries[0]
                .rejected
                .iter()
                .map(|&i| doc.judgments[i].paper.clone())
                .collect::<Vec<_>>(),
            vec![beta.clone()]
        );
        assert!(r.lost.is_empty());
    };
    check(&p.a);
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    check(&p.b);
}
