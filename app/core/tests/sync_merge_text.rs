//! 抽出的文字的同步：合併層的純計算與兩台文獻庫走同步包。

mod common;

use common::*;
use paperreader_core::jsonio::sha256_hex;
use paperreader_core::local::LocalLibrary;
use paperreader_core::references::Extraction;
use paperreader_core::store::{ConflictKind, ItemKind, NoticeKind, SyncMode};
use paperreader_core::sync::merge::MergeCtx;
use paperreader_core::sync::merge::text::merge_text;
use paperreader_core::text::*;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

const SHA_NOW: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const SHA_OLD: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const T1: &str = "2026-11-18T08:00:00.000Z";
const T2: &str = "2026-11-18T09:00:00.000Z";
const PATH: &str = "papers/P/text.json";

fn tf(sha: &str, version: u32, created: &str, by: &str, texts: &[&str]) -> TextFile {
    TextFile {
        format: 1,
        extraction: Extraction {
            extractor: "pdfjs-text".into(),
            version,
            source_material: "M1".into(),
            source_sha256: sha.into(),
            created: created.into(),
            created_by: by.into(),
            extra: Default::default(),
        },
        pages: texts
            .iter()
            .enumerate()
            .map(|(i, t)| TextPage {
                page: i as u32,
                text: t.to_string(),
                extra: Default::default(),
            })
            .collect(),
        truncated: false,
        extra: Default::default(),
    }
}

fn texts(f: &TextFile) -> Vec<&str> {
    f.pages.iter().map(|p| p.text.as_str()).collect()
}

fn merge(l: Option<&TextFile>, r: Option<&TextFile>) -> (Option<TextFile>, MergeCtx) {
    let mut ctx = mctx(&[]);
    let out = merge_text(l, r, Some(SHA_NOW), "P", PATH, Some("t"), &mut ctx);
    (out, ctx)
}

fn noted(ctx: &MergeCtx, added: bool) -> bool {
    let map = if added {
        &ctx.summary.added
    } else {
        &ctx.summary.updated
    };
    map.contains_key(&(ItemKind::Text, Some("P".into())))
}

// ---------- 11. 合併層 ----------

#[test]
fn only_remote_is_copied_and_counted_as_added() {
    let r = tf(SHA_NOW, 1, T1, DEVICE, &["a", "b"]);
    let (out, ctx) = merge(None, Some(&r));
    assert_eq!(out.unwrap(), r);
    assert!(noted(&ctx, true));
    assert!(!noted(&ctx, false));
}

#[test]
fn only_local_is_left_alone() {
    let l = tf(SHA_NOW, 1, T1, DEVICE_B, &["a"]);
    let (out, ctx) = merge(Some(&l), None);
    assert!(out.is_none());
    assert!(ctx.summary.added.is_empty() && ctx.summary.updated.is_empty());
    assert!(merge(None, None).0.is_none());
}

#[test]
fn same_file_is_not_rewritten() {
    let l = tf(SHA_NOW, 1, T1, DEVICE, &["a"]);
    let (out, ctx) = merge(Some(&l), Some(&l.clone()));
    assert!(out.is_none());
    assert!(ctx.summary.updated.is_empty());
}

#[test]
fn step_one_extraction_from_the_current_source_wins() {
    // 本機的較新、版本較高，但抽自舊的原文
    let l = tf(SHA_OLD, 3, T2, DEVICE_B, &["old"]);
    let r = tf(SHA_NOW, 1, T1, DEVICE, &["new"]);
    let (out, ctx) = merge(Some(&l), Some(&r));
    let out = out.unwrap();
    assert_eq!(texts(&out), ["new"]);
    assert_eq!(out.extraction, r.extraction);
    assert!(noted(&ctx, false), "counted as updated");
    // 反過來：匯入的抽自舊原文，本機不動
    assert!(merge(Some(&r), Some(&l)).0.is_none());
}

#[test]
fn step_two_higher_version_of_the_same_extractor_wins() {
    let l = tf(SHA_NOW, 1, T2, DEVICE_B, &["v1"]);
    let r = tf(SHA_NOW, 2, T1, DEVICE, &["v2"]);
    assert_eq!(texts(&merge(Some(&l), Some(&r)).0.unwrap()), ["v2"]);
    assert!(merge(Some(&r), Some(&l)).0.is_none());
    // 抽法不同時不比版本，看時間
    let mut other = tf(SHA_NOW, 9, T1, DEVICE, &["other"]);
    other.extraction.extractor = "ocr".into();
    let newer = tf(SHA_NOW, 1, T2, DEVICE_B, &["newer"]);
    assert!(merge(Some(&newer), Some(&other)).0.is_none());
    assert_eq!(
        texts(&merge(Some(&other), Some(&newer)).0.unwrap()),
        ["newer"]
    );
}

#[test]
fn step_three_newer_extraction_wins_and_a_tie_keeps_local() {
    let l = tf(SHA_NOW, 1, T1, DEVICE_B, &["early"]);
    let r = tf(SHA_NOW, 1, T2, DEVICE, &["late"]);
    assert_eq!(texts(&merge(Some(&l), Some(&r)).0.unwrap()), ["late"]);
    assert!(merge(Some(&r), Some(&l)).0.is_none());
    // 時間相同比裝置代號
    let a = tf(SHA_NOW, 1, T1, DEVICE, &["by A"]);
    let b = tf(SHA_NOW, 1, T1, DEVICE_B, &["by B"]);
    let (hi, lo) = if DEVICE_B > DEVICE {
        (&b, &a)
    } else {
        (&a, &b)
    };
    assert_eq!(texts(&merge(Some(lo), Some(hi)).0.unwrap()), texts(hi));
    // 三步都比不出來（檔頭完全相同、內容不同）：留本機的
    let mut same = l.clone();
    same.pages[0].text = "different".into();
    assert!(merge(Some(&l), Some(&same)).0.is_none());
}

#[test]
fn unknown_fields_follow_the_table() {
    // 勝的是匯入那邊：extraction 整個取它的；輸的（本機）頁上的欄位依頁序搬過去（同一份原文）；
    // 頂層本機的留著、匯入那邊多的加進來
    let mut l = tf(SHA_NOW, 1, T1, DEVICE_B, &["l0", "l1"]);
    l.extra.insert("x_top_local".into(), json!("L"));
    l.extra.insert("x_both".into(), json!("local"));
    l.extraction.extra.insert("x_ext_local".into(), json!(1));
    l.pages[0].extra.insert("x_p0".into(), json!("from local"));
    l.pages[1].extra.insert("x_p1".into(), json!("from local"));
    let mut r = tf(SHA_NOW, 1, T2, DEVICE, &["r0"]);
    r.truncated = true;
    r.extra.insert("x_top_remote".into(), json!("R"));
    r.extra.insert("x_both".into(), json!("remote"));
    r.extraction.extra.insert("x_ext_remote".into(), json!(2));
    r.pages[0].extra.insert("x_p0".into(), json!("from remote"));
    let out = merge(Some(&l), Some(&r)).0.unwrap();
    assert_eq!(
        out.extraction, r.extraction,
        "extraction comes whole from the winner"
    );
    assert_eq!(texts(&out), ["r0"]);
    assert_eq!(
        out.pages[0].extra["x_p0"],
        json!("from remote"),
        "winner's key stays"
    );
    assert_eq!(
        out.pages.len(),
        1,
        "pages beyond the winner's count are dropped"
    );
    assert_eq!(out.extra["x_top_local"], json!("L"));
    assert_eq!(out.extra["x_top_remote"], json!("R"));
    assert_eq!(out.extra["x_both"], json!("local"));
    assert!(out.truncated);
    // 勝的是本機：匯入那邊頁上的欄位依頁序搬過來
    let mut l = tf(SHA_NOW, 2, T1, DEVICE_B, &["l0", "l1"]);
    let mut r = tf(SHA_NOW, 1, T2, DEVICE, &["r0", "r1"]);
    r.pages[1]
        .extra
        .insert("x_p1".into(), json!("remote extra"));
    r.extraction.extra.insert("x_ext_remote".into(), json!(2));
    l.extra.insert("x_top".into(), json!("L"));
    let out = merge(Some(&l), Some(&r)).0.unwrap();
    assert_eq!(texts(&out), ["l0", "l1"]);
    assert_eq!(out.extraction, l.extraction);
    assert_eq!(out.pages[1].extra["x_p1"], json!("remote extra"));
    // 兩邊的原文不同：輸的一邊頁上的欄位丟掉
    let l = tf(SHA_NOW, 1, T1, DEVICE_B, &["l0"]);
    let mut r = tf(SHA_OLD, 1, T2, DEVICE, &["r0"]);
    r.pages[0].extra.insert("x_p0".into(), json!("old source"));
    assert!(merge(Some(&l), Some(&r)).0.is_none());
}

#[test]
fn merged_result_over_24_megabytes_is_not_written() {
    // 控制字元寫成 JSON 是 6 個位元組：500 萬個約 30 MB
    let big = "\u{1}".repeat(MAX_TOTAL_CHARS);
    let r = tf(SHA_NOW, 1, T2, DEVICE, &[big.as_str()]);
    let l = tf(SHA_NOW, 1, T1, DEVICE_B, &["small"]);
    for local in [None, Some(&l)] {
        let (out, ctx) = merge(local, Some(&r));
        assert!(out.is_none());
        assert!(
            ctx.notices
                .iter()
                .any(|n| n.kind == NoticeKind::TooLargeToMerge
                    && n.paper.as_deref() == Some("P")
                    && n.path.as_deref() == Some(PATH)),
            "{:?}",
            ctx.notices
        );
        assert!(ctx.summary.added.is_empty() && ctx.summary.updated.is_empty());
    }
}

// ---------- 兩台文獻庫 ----------

fn folder(lib: &LocalLibrary, id: &str) -> String {
    lib.list_papers()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap()
        .folder
}

fn text_file(lib: &LocalLibrary, root: &Path, id: &str) -> PathBuf {
    root.join("papers").join(folder(lib, id)).join(TEXT_FILE)
}

fn write(lib: &mut LocalLibrary, id: &str, version: u32, pages: &[&str]) {
    let m = lib.get_paper(id).unwrap().0.materials[0].clone();
    let n = m.pages.unwrap() as usize;
    let mut texts: Vec<String> = pages.iter().map(|s| s.to_string()).collect();
    texts.resize(n, String::new());
    lib.write_text(
        id,
        TextDraft {
            extractor: "pdfjs-text".into(),
            version,
            source_material: m.id,
            source_sha256: m.sha256.unwrap(),
            pages: texts,
            truncated: false,
        },
    )
    .unwrap();
}

fn read(lib: &LocalLibrary, id: &str) -> TextFile {
    lib.read_text(id).unwrap().unwrap().0
}

fn manifest(pkg: &Path) -> Value {
    let (_, bytes, _) = zip_entries(pkg)
        .into_iter()
        .find(|e| e.0 == "manifest.json")
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn entry_for(m: &Value, suffix: &str) -> Value {
    m["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"].as_str().unwrap().ends_with(suffix))
        .cloned()
        .unwrap_or_else(|| panic!("no {suffix} in {m}"))
}

/// A 寫 Alpha 的文字，同步 A→B、B→A（兩邊都有了，A 的下一份「只帶變更」有起算點）。
fn synced_with_alpha_text() -> (Pair, String) {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    write(&mut p.a, &alpha, 1, &["alpha page 0", "alpha page 1"]);
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    sync_once(&mut p.b, &mut p.a, &out);
    (p, alpha)
}

#[test]
fn full_package_carries_text_changes_package_only_newer_files() {
    let (mut p, alpha) = synced_with_alpha_text();
    assert_eq!(
        texts(&read(&p.b, &alpha))[..2],
        ["alpha page 0", "alpha page 1"]
    );
    // 完整的同步包：帶
    let out = p.out.path().to_path_buf();
    let full = out.join("full.zip");
    p.a.export_sync_package(&full, SyncMode::Full, None, &mut |_, _| {})
        .unwrap();
    let e = entry_for(
        &manifest(&full),
        &format!("{}/text.json", folder(&p.a, &alpha)),
    );
    assert_eq!(e["included"], json!(true));
    // A 加一篇、抽文字；只帶變更：新的那份帶，Alpha 的那份不帶，清單裡 bytes 與 sha256 是 null
    let (pid, _) = add_paper(&mut p.a, 80, "New On A", Some(2));
    write(&mut p.a, &pid, 1, &["new page"]);
    let pkg = export_pkg(&mut p.a, &out);
    let m = manifest(&pkg);
    assert_eq!(m["mode"], json!("changes"));
    let old = entry_for(&m, &format!("{}/text.json", folder(&p.a, &alpha)));
    assert_eq!(old["included"], json!(false));
    assert_eq!(old["bytes"], Value::Null);
    assert_eq!(old["sha256"], Value::Null);
    let new = entry_for(&m, &format!("{}/text.json", folder(&p.a, &pid)));
    assert_eq!(new["included"], json!(true));
    let names: Vec<String> = zip_entries(&pkg).into_iter().map(|e| e.0).collect();
    assert_eq!(
        names.iter().filter(|n| n.ends_with("/text.json")).count(),
        1,
        "{names:?}"
    );
    // 匯入：沒帶的不算缺檔、本機的留著；新的那篇帶著它的文字
    let before = fs::read(text_file(&p.b, &p.b_root(), &alpha)).unwrap();
    let report = import_defaults(&mut p.b, &pkg);
    assert!(
        report.skipped_papers.is_empty(),
        "{:?}",
        report.skipped_papers
    );
    assert!(
        !report
            .notices
            .iter()
            .any(|n| n.kind == NoticeKind::MissingFile),
        "{:?}",
        report.notices
    );
    assert_eq!(
        fs::read(text_file(&p.b, &p.b_root(), &alpha)).unwrap(),
        before
    );
    assert_eq!(texts(&read(&p.b, &pid)), ["new page", ""]);
    assert!(
        report
            .added
            .iter()
            .any(|i| i.kind == ItemKind::Text && i.paper.as_deref() == Some(pid.as_str())),
        "{:?}",
        report.added
    );
}

/// 匯入寫進來的 `text.json` 修改時間晚於另一台的起算點：B 的下一份「只帶變更」會把它帶回去一次（規則的結果，
/// 不是錯；A 匯入時內容相同不寫）。
#[test]
fn text_received_by_import_goes_back_once() {
    let (mut p, _) = synced_with_alpha_text();
    let out = p.out.path().to_path_buf();
    let (pid, _) = add_paper(&mut p.a, 81, "Echo", Some(1));
    write(&mut p.a, &pid, 1, &["echo"]);
    sync_once(&mut p.a, &mut p.b, &out);
    let a_file = text_file(&p.a, &p.a_root(), &pid);
    let a_before = fs::read(&a_file).unwrap();
    let pkg = export_pkg(&mut p.b, &out);
    let e = entry_for(
        &manifest(&pkg),
        &format!("{}/text.json", folder(&p.b, &pid)),
    );
    assert_eq!(e["included"], json!(true));
    let report = import_defaults(&mut p.a, &pkg);
    assert!(
        !report.updated.iter().any(|i| i.kind == ItemKind::Text),
        "{:?}",
        report.updated
    );
    assert_eq!(fs::read(&a_file).unwrap(), a_before);
    // B 知道 A 匯入過之後（A→B 一次），就不再帶了
    sync_once(&mut p.a, &mut p.b, &out);
    let pkg = export_pkg(&mut p.b, &out);
    let e = entry_for(
        &manifest(&pkg),
        &format!("{}/text.json", folder(&p.b, &pid)),
    );
    assert_eq!(e["included"], json!(false));
}

#[test]
fn text_not_in_the_package_is_not_read_on_this_side() {
    let (mut p, alpha) = synced_with_alpha_text();
    let out = p.out.path().to_path_buf();
    // 兩邊各改想法：Alpha 有一個衝突（衝突的那篇，讀過的本機檔都記成版本記號）
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "A version", nt.as_ref()).unwrap();
    let nt = p.b.read_notes(&alpha).unwrap().1;
    p.b.set_thoughts(&alpha, "B version", nt.as_ref()).unwrap();
    // B 這邊的 text.json 讀不懂
    let b_text = text_file(&p.b, &p.b_root(), &alpha);
    fs::write(&b_text, b"not json at all").unwrap();
    let pkg = export_pkg(&mut p.a, &out);
    let pre = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        pre.conflicts
            .iter()
            .any(|c| c.kind == ConflictKind::Thoughts),
        "{:?}",
        pre.conflicts
    );
    // 沒帶就沒讀：不提醒本機讀不懂
    assert!(
        !pre.notices
            .iter()
            .any(|n| n.path.as_deref().is_some_and(|p| p.ends_with("text.json"))),
        "{:?}",
        pre.notices
    );
    // 預覽之後改了本機那份：不在記下的版本記號裡，照樣寫得進去
    fs::write(&b_text, b"still not json, but different").unwrap();
    let choices: Vec<_> = pre
        .conflicts
        .iter()
        .map(|c| (c.id.clone(), c.default))
        .collect();
    p.b.apply_sync_import(&pre.preview_id, &choices).unwrap();
    assert_eq!(fs::read(&b_text).unwrap(), b"still not json, but different");
}

/// 對照組：同一個做法，改的是讀過的本機檔（`references.json`），寫入會被擋下。證明上一條測得到「讀了」。
#[test]
fn control_a_file_that_was_read_blocks_the_apply() {
    let (mut p, alpha) = synced_with_alpha_text();
    let out = p.out.path().to_path_buf();
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "A version", nt.as_ref()).unwrap();
    let nt = p.b.read_notes(&alpha).unwrap().1;
    p.b.set_thoughts(&alpha, "B version", nt.as_ref()).unwrap();
    let pkg = export_pkg(&mut p.a, &out);
    let pre = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let refs = p
        .b_root()
        .join("papers")
        .join(folder(&p.b, &alpha))
        .join("references.json");
    fs::write(&refs, b"{}").unwrap();
    let choices: Vec<_> = pre
        .conflicts
        .iter()
        .map(|c| (c.id.clone(), c.default))
        .collect();
    assert!(matches!(
        p.b.apply_sync_import(&pre.preview_id, &choices),
        Err(paperreader_core::error::LibError::Conflict)
    ));
}

#[test]
fn existing_paper_with_text_only_on_the_other_side_gets_a_copy() {
    let mut p = pair();
    let beta = find(&p.a, "Beta Paper");
    write(&mut p.a, &beta, 1, &["beta"]);
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let pre = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        pre.added
            .iter()
            .any(|i| i.kind == ItemKind::Text && i.paper.as_deref() == Some(beta.as_str())),
        "{:?}",
        pre.added
    );
    p.b.discard_sync_import(&pre.preview_id).unwrap();
    import_defaults(&mut p.b, &pkg);
    assert_eq!(read(&p.b, &beta), read(&p.a, &beta));
}

#[test]
fn new_paper_brings_its_text() {
    let mut p = pair();
    let (pid, _) = add_paper(&mut p.a, 82, "Only On A", Some(3));
    write(&mut p.a, &pid, 1, &["one", "two", "three"]);
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    assert_eq!(read(&p.b, &pid), read(&p.a, &pid));
}

#[test]
fn both_sides_extracted_and_end_up_with_one_file() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    write(&mut p.a, &alpha, 2, &["from A, version 2"]);
    write(&mut p.b, &alpha, 1, &["from B, version 1"]);
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let pre = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        pre.updated
            .iter()
            .any(|i| i.kind == ItemKind::Text && i.paper.as_deref() == Some(alpha.as_str())),
        "{:?}",
        pre.updated
    );
    p.b.discard_sync_import(&pre.preview_id).unwrap();
    import_defaults(&mut p.b, &pkg);
    sync_once(&mut p.b, &mut p.a, &out);
    assert_eq!(read(&p.a, &alpha).pages[0].text, "from A, version 2");
    assert_eq!(read(&p.a, &alpha), read(&p.b, &alpha));
    assert_eq!(sync_state(&p.a, &p.a_root()), sync_state(&p.b, &p.b_root()));
    assert!(
        p.b.check(paperreader_core::store::CheckLevel::Quick)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unreadable_or_newer_text_is_not_merged() {
    let mut p = pair();
    let alpha = find(&p.a, "Alpha Paper");
    write(&mut p.a, &alpha, 2, &["A"]);
    write(&mut p.b, &alpha, 1, &["B"]);
    let out = p.out.path().to_path_buf();
    let afile = text_file(&p.a, &p.a_root(), &alpha);
    let bfile = text_file(&p.b, &p.b_root(), &alpha);
    let good = fs::read(&afile).unwrap();
    let b_good = fs::read(&bfile).unwrap();
    let preview_has = |lib: &mut LocalLibrary, pkg: &Path, kind: NoticeKind| {
        let pre = lib.prepare_sync_import(pkg, &mut |_, _| {}).unwrap();
        let hit = pre
            .notices
            .iter()
            .any(|n| n.kind == kind && n.paper.as_deref() == Some(alpha.as_str()));
        lib.discard_sync_import(&pre.preview_id).unwrap();
        hit
    };
    // 本機讀不懂
    fs::write(&bfile, b"garbage").unwrap();
    let pkg = export_pkg(&mut p.a, &out);
    assert!(preview_has(&mut p.b, &pkg, NoticeKind::LocalUnreadable));
    fs::write(&bfile, &b_good).unwrap();
    // 匯入的讀不懂：本機不動
    fs::write(&afile, b"garbage").unwrap();
    let pkg = export_pkg(&mut p.a, &out);
    fs::write(&afile, &good).unwrap();
    assert!(preview_has(&mut p.b, &pkg, NoticeKind::IncomingUnreadable));
    import_defaults(&mut p.b, &pkg);
    assert_eq!(fs::read(&bfile).unwrap(), b_good);
    // 匯入的格式較新
    let mut v: Value = serde_json::from_slice(&good).unwrap();
    v["format"] = json!(2);
    fs::write(&afile, v.to_string()).unwrap();
    let pkg = export_pkg(&mut p.a, &out);
    assert!(preview_has(&mut p.b, &pkg, NoticeKind::NewerFormat));
    import_defaults(&mut p.b, &pkg);
    assert_eq!(fs::read(&bfile).unwrap(), b_good);
}

#[test]
fn new_paper_with_unreadable_or_newer_text_comes_without_it() {
    for (garbage, kind) in [
        (true, NoticeKind::IncomingUnreadable),
        (false, NoticeKind::NewerFormat),
    ] {
        let mut p = pair();
        let (pid, _) = add_paper(&mut p.a, 83, "New With Text", Some(1));
        write(&mut p.a, &pid, 1, &["x"]);
        let afile = text_file(&p.a, &p.a_root(), &pid);
        if garbage {
            fs::write(&afile, b"garbage").unwrap();
        } else {
            let mut v: Value = serde_json::from_slice(&fs::read(&afile).unwrap()).unwrap();
            v["format"] = json!(2);
            fs::write(&afile, v.to_string()).unwrap();
        }
        let out = p.out.path().to_path_buf();
        let pkg = export_pkg(&mut p.a, &out);
        let pre = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
        assert!(
            pre.notices
                .iter()
                .any(|n| n.kind == kind && n.paper.as_deref() == Some(pid.as_str())),
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
        assert!(!text_file(&p.b, &p.b_root(), &pid).exists());
    }
}

/// 防回歸：依指紋找的候選只收 `paper.json` 列的材料；候選若改成收資料夾裡的每一個檔，這條（與參考文獻清單的同一條）就會失敗。
#[test]
fn text_file_is_never_a_candidate_for_a_missing_material() {
    // 匯入那邊一篇新文獻的原文沒放進同步包；它的指紋剛好等於本機某份 text.json 的內容：不可以拿它來用
    let mut p = pair();
    let alpha = find(&p.b, "Alpha Paper");
    write(&mut p.b, &alpha, 1, &["b"]);
    let bytes = fs::read(text_file(&p.b, &p.b_root(), &alpha)).unwrap();
    let sha = sha256_hex(&bytes);
    let (pid, _) = add_paper(&mut p.a, 84, "Only A", Some(2));
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
