//! 筆記檔的合併：劃線、刪除記錄、想法的記錄圖、標記以外的文字、衝突副本。

mod common;

use common::*;
use paperreader_core::changes::{
    HIGHLIGHT_DELETED, HIGHLIGHT_RESTORED, THOUGHTS_WRITTEN, thoughts_hash,
};
use paperreader_core::notes::{
    FileRef, Highlight, NotesLabels, PageRef, Quote, Stamp, conflict_of, new_file, parse,
};
use paperreader_core::store::{ConflictChoice, ConflictKind, NoticeKind, ProblemKind};
use paperreader_core::sync::merge::notes::{NotesMerge, merge_notes};
use serde_json::json;
use std::fs;

const PID: &str = "01JCK3W2A2B3C4D5E6F7G8H9JK";
const T0: &str = "2026-11-01T00:00:00.000Z";
const T1: &str = "2026-11-02T00:00:00.000Z";
const T2: &str = "2026-11-03T00:00:00.000Z";
const T3: &str = "2026-11-04T00:00:00.000Z";

fn hl(id: &str, modified: &str, comment: &str) -> Highlight {
    Highlight {
        id: id.into(),
        kind: "highlight".into(),
        material: "M".into(),
        file: FileRef {
            sha256: None,
            fingerprint: None,
            extra: Default::default(),
        },
        page: PageRef {
            index: 0,
            label: None,
            extra: Default::default(),
        },
        rects: vec![[1.0, 2.0, 3.0, 4.0]],
        next_page_rects: vec![],
        position: None,
        quote: Quote {
            exact: format!("quote {id}"),
            prefix: None,
            suffix: None,
            extra: Default::default(),
        },
        color: None,
        created: T0.into(),
        modified: modified.into(),
        extra: Default::default(),
        comment: comment.into(),
        complete: true,
    }
}

/// 一份筆記檔：這些劃線、這段想法。
fn doc(hls: &[Highlight], thoughts: &str) -> String {
    let labels = NotesLabels::default();
    let stamp = Stamp {
        time: T0.into(),
        device: DEVICE.into(),
    };
    let mut t = new_file(PID, "Paper", &labels, &stamp);
    for h in hls {
        t = parse(&t)
            .with_new_highlight(h, &labels, PID, &stamp)
            .unwrap();
    }
    if !thoughts.is_empty() {
        t = parse(&t).with_thoughts(thoughts, &labels, &stamp).unwrap();
    }
    t
}

fn merge(
    local: Option<&str>,
    remote: Option<&str>,
    recs: &[paperreader_core::changes::ChangeRecord],
    choices: &[(ConflictKind, Option<&str>, Option<&str>, ConflictChoice)],
) -> (NotesMerge, paperreader_core::sync::merge::MergeCtx) {
    let mut ctx = mctx(choices);
    let m = merge_notes(
        PID,
        Some("Paper"),
        local,
        remote,
        recs,
        &NotesLabels::default(),
        "notes/Paper.md",
        &mut ctx,
    );
    (m, ctx)
}

fn ids(text: &str) -> Vec<String> {
    parse(text).highlights().into_iter().map(|h| h.id).collect()
}

fn del(id: &str, t: &str, hid: &str) -> paperreader_core::changes::ChangeRecord {
    rec(
        id,
        t,
        DEVICE,
        Some(PID),
        HIGHLIGHT_DELETED,
        json!({"highlight": hid}),
    )
}

#[test]
fn incoming_only_highlight_inserted_verbatim() {
    let r = doc(&[hl("h-a", T0, "remote comment")], "");
    let l = doc(&[], "");
    let (m, ctx) = merge(Some(&l), Some(&r), &[], &[]);
    let out = m.text.unwrap();
    let raw = parse(&r).highlight_raw("h-a").unwrap();
    assert!(out.contains(&raw), "原文照搬");
    assert!(ctx.conflicts.is_empty());
}

#[test]
fn front_matter_stamped() {
    let r = doc(&[hl("h-a", T0, "c")], "");
    let l = doc(&[], "");
    let (m, _) = merge(Some(&l), Some(&r), &[], &[]);
    let p = parse(m.text.as_ref().unwrap());
    assert_eq!(p.modified(), Some(NOW));
    assert_eq!(p.modified_by(), Some(DEVICE_B));
    // 沒有改：不寫
    let (m, _) = merge(Some(&l), Some(&l), &[], &[]);
    assert!(m.text.is_none());
}

#[test]
fn local_newline_style_kept() {
    let l = format!("\u{feff}{}", doc(&[], "").replace('\n', "\r\n"));
    let r = doc(&[hl("h-a", T0, "line one\nline two")], "");
    let (m, _) = merge(Some(&l), Some(&r), &[], &[]);
    let out = m.text.unwrap();
    assert!(out.starts_with('\u{feff}'));
    assert!(
        !out.replace("\r\n", "").contains('\n'),
        "插入的一條用 \\r\\n"
    );
    // 其餘逐位元組不變：拿掉插入的那段與屬性區兩行，剩下相同
    let without: String = out
        .split("\r\n")
        .filter(|l| !l.starts_with("paperreader_modified"))
        .collect::<Vec<_>>()
        .join("\r\n");
    let orig: String = l
        .split("\r\n")
        .filter(|l| !l.starts_with("paperreader_modified"))
        .collect::<Vec<_>>()
        .join("\r\n");
    let raw = parse(&r)
        .highlight_raw("h-a")
        .unwrap()
        .replace('\n', "\r\n")
        + "\r\n";
    assert_eq!(without.replacen(&raw, "", 1), orig);
}

#[test]
fn both_have_it_and_deletion_record_removed() {
    let t = doc(&[hl("h-a", T0, "c"), hl("h-b", T0, "keep")], "");
    let (m, _) = merge(
        Some(&t),
        Some(&t),
        &[del("01D00000000000000000000001", T1, "h-a")],
        &[],
    );
    assert_eq!(ids(&m.text.unwrap()), ["h-b"]);
}

#[test]
fn deleted_and_edited_earlier_just_deleted() {
    let l = doc(&[hl("h-a", T1, "edited before delete")], "");
    let (m, ctx) = merge(
        Some(&l),
        Some(&doc(&[], "")),
        &[del("01D00000000000000000000001", T2, "h-a")],
        &[],
    );
    assert!(ctx.conflicts.is_empty());
    assert!(ids(&m.text.unwrap()).is_empty());
}

#[test]
fn deleted_but_edited_later_conflict() {
    // 本機在刪除之後改過（只在本機）
    let l = doc(&[hl("h-a", T3, "edited after")], "");
    let r = doc(&[], "");
    let recs = [del("01D00000000000000000000001", T2, "h-a")];
    let (m, ctx) = merge(Some(&l), Some(&r), &recs, &[]);
    assert_eq!(ctx.conflicts.len(), 1);
    let c = &ctx.conflicts[0].1;
    assert_eq!(c.kind, ConflictKind::HighlightDeletedButEdited);
    assert_eq!(c.default, ConflictChoice::Both);
    // 預設：刪除，改過的那一邊（本機）的整份另存副本
    assert!(ids(m.text.as_ref().unwrap()).is_empty());
    assert_eq!(m.copies.len(), 1);
    assert_eq!(m.copies[0].device, DEVICE_B, "本機合併前那份");
    let copy = m.copies[0].text.clone();
    assert_eq!(conflict_of(&copy).as_deref(), Some(PID));
    assert!(copy.contains("edited after"), "副本是本機合併前的那份");
    assert_eq!(ctx.records[0].kind, HIGHLIGHT_DELETED);
    assert_eq!(ctx.records[0].t, NOW);
}

#[test]
fn deleted_but_edited_locally_copy_is_local_file() {
    let l = doc(&[hl("h-a", T3, "mine")], "local thoughts");
    let r = doc(&[hl("h-a", T0, "theirs")], "local thoughts");
    let recs = [del("01D00000000000000000000001", T2, "h-a")];
    let (m, ctx) = merge(Some(&l), Some(&r), &recs, &[]);
    assert_eq!(
        ctx.conflicts[0].1.kind,
        ConflictKind::HighlightDeletedButEdited
    );
    assert_eq!(m.copies.len(), 1);
    let copy = m.copies[0].text.clone();
    assert!(copy.contains("mine"));
    assert!(!copy.contains("paperreader_paper:"));
}

#[test]
fn both_edited_after_deletion_kept_without_asking() {
    let l = doc(&[hl("h-a", T3, "same")], "");
    let r = doc(&[hl("h-a", T3, "same")], "");
    let recs = [del("01D00000000000000000000001", T2, "h-a")];
    let (m, ctx) = merge(Some(&l), Some(&r), &recs, &[]);
    assert!(ctx.conflicts.is_empty());
    assert!(m.text.is_none());
}

#[test]
fn restored_record_later_than_deleted_keeps() {
    let t = doc(&[hl("h-a", T0, "c")], "");
    let recs = [
        del("01D00000000000000000000001", T1, "h-a"),
        rec(
            "01D00000000000000000000002",
            T2,
            DEVICE_B,
            Some(PID),
            HIGHLIGHT_RESTORED,
            json!({"highlight": "h-a"}),
        ),
    ];
    let (m, ctx) = merge(Some(&t), Some(&t), &recs, &[]);
    assert!(ctx.conflicts.is_empty());
    assert!(m.text.is_none());
}

#[test]
fn deleted_but_edited_resolution_converges() {
    let l = doc(&[hl("h-a", T3, "edited after")], "");
    let r = doc(&[], "");
    let recs = vec![del("01D00000000000000000000001", T2, "h-a")];
    // 選留下：寫 highlight-restored，資料行的 modified 改成現在
    let (m, ctx) = merge(
        Some(&l),
        Some(&r),
        &recs,
        &[(
            ConflictKind::HighlightDeletedButEdited,
            Some(PID),
            Some("h-a"),
            ConflictChoice::Incoming,
        )],
    );
    assert_eq!(ctx.records[0].kind, HIGHLIGHT_RESTORED);
    let kept = m.text.unwrap();
    assert_eq!(parse(&kept).highlights()[0].modified, NOW);
    let mut all = recs.clone();
    all.extend(ctx.records.clone());
    // 反向（對方匯入這邊）：不再問，對方加回這一條
    let mut ctx2 = mctx(&[]);
    ctx2.device = DEVICE.into();
    ctx2.remote_device = DEVICE_B.into();
    let m2 = merge_notes(
        PID,
        None,
        Some(&r),
        Some(&kept),
        &all,
        &NotesLabels::default(),
        "n",
        &mut ctx2,
    );
    assert!(ctx2.conflicts.is_empty());
    assert_eq!(ids(&m2.text.unwrap()), ["h-a"]);
    // 選刪除：寫一筆新的 highlight-deleted；反向不再問、對方也刪
    let (m, ctx) = merge(
        Some(&l),
        Some(&r),
        &recs,
        &[(
            ConflictKind::HighlightDeletedButEdited,
            Some(PID),
            Some("h-a"),
            ConflictChoice::Local,
        )],
    );
    assert!(m.copies.is_empty());
    let mut all = recs.clone();
    all.extend(ctx.records.clone());
    let mut ctx3 = mctx(&[]);
    let m3 = merge_notes(
        PID,
        None,
        Some(&l),
        Some(&m.text.unwrap()),
        &all,
        &NotesLabels::default(),
        "n",
        &mut ctx3,
    );
    assert!(ctx3.conflicts.is_empty());
    assert!(ids(&m3.text.unwrap()).is_empty());
}

#[test]
fn highlight_newer_replaces_block() {
    let l = doc(&[hl("h-a", T1, "old")], "");
    let r = doc(&[hl("h-a", T2, "new comment")], "");
    let (m, ctx) = merge(Some(&l), Some(&r), &[], &[]);
    assert!(ctx.conflicts.is_empty());
    let p = parse(m.text.as_ref().unwrap());
    assert_eq!(p.highlights()[0].comment, "new comment");
    assert_eq!(p.highlight_raw("h-a"), parse(&r).highlight_raw("h-a"));
}

#[test]
fn highlight_same_time_different_comment_conflict() {
    let l = doc(&[hl("h-a", T1, "mine")], "");
    let r = doc(&[hl("h-a", T1, "theirs")], "");
    let (m, ctx) = merge(Some(&l), Some(&r), &[], &[]);
    assert_eq!(ctx.conflicts[0].1.kind, ConflictKind::HighlightEdited);
    assert_eq!(ctx.conflicts[0].1.local.text.as_deref(), Some("mine"));
    // 預設 both：留本機的、蓋上現在，匯入的整份另存
    let p = parse(m.text.as_ref().unwrap());
    assert_eq!(p.highlights()[0].comment, "mine");
    assert_eq!(p.highlights()[0].modified, NOW);
    assert_eq!(m.copies.len(), 1);
    assert_eq!(m.copies[0].device, DEVICE);
    assert!(m.copies[0].text.contains("theirs"));
}

fn tw(
    id: &str,
    t: &str,
    dev: &str,
    text: &str,
    prev: &[&str],
) -> paperreader_core::changes::ChangeRecord {
    rec(
        id,
        t,
        dev,
        Some(PID),
        THOUGHTS_WRITTEN,
        json!({"sha256": thoughts_hash(text), "prev": prev}),
    )
}

#[test]
fn thoughts_descendant_taken() {
    // 本機（B）停在 r1（X）；匯入（A）寫了 r2（Y，接 r1）
    let recs = [
        tw("01T00000000000000000000001", T0, DEVICE, "X", &[]),
        tw(
            "01T00000000000000000000002",
            T1,
            DEVICE,
            "Y",
            &["01T00000000000000000000001"],
        ),
    ];
    let (m, ctx) = merge(Some(&doc(&[], "X")), Some(&doc(&[], "Y")), &recs, &[]);
    assert!(ctx.conflicts.is_empty());
    assert!(ctx.records.is_empty(), "直接採用不寫記錄");
    assert_eq!(parse(&m.text.unwrap()).thoughts(), "Y");
}

#[test]
fn thoughts_ancestor_kept() {
    let recs = [
        tw("01T00000000000000000000001", T0, DEVICE, "X", &[]),
        tw(
            "01T00000000000000000000002",
            T1,
            DEVICE_B,
            "Y",
            &["01T00000000000000000000001"],
        ),
    ];
    let (m, ctx) = merge(Some(&doc(&[], "Y")), Some(&doc(&[], "X")), &recs, &[]);
    assert!(ctx.conflicts.is_empty());
    assert!(m.text.is_none());
}

#[test]
fn thoughts_independent_edits_conflict_and_resolution_record_has_two_prevs() {
    let recs = [
        tw("01T00000000000000000000001", T0, DEVICE, "X", &[]),
        tw(
            "01T00000000000000000000002",
            T1,
            DEVICE_B,
            "L",
            &["01T00000000000000000000001"],
        ),
        tw(
            "01T00000000000000000000003",
            T1,
            DEVICE,
            "R",
            &["01T00000000000000000000001"],
        ),
    ];
    let (m, ctx) = merge(Some(&doc(&[], "L")), Some(&doc(&[], "R")), &recs, &[]);
    assert_eq!(ctx.conflicts[0].1.kind, ConflictKind::Thoughts);
    assert_eq!(ctx.conflicts[0].1.default, ConflictChoice::Both);
    assert_eq!(m.copies.len(), 1);
    assert_eq!(ctx.records.len(), 1);
    assert_eq!(
        ctx.records[0].data["prev"],
        json!(["01T00000000000000000000002", "01T00000000000000000000003"])
    );
    assert_eq!(ctx.records[0].data["sha256"], json!(thoughts_hash("L")));
}

#[test]
fn thoughts_external_edit_conflict() {
    // 匯入那邊的內容沒有任何節點（在筆記工具裡改、又沒有補記，例如較舊版本的應用）
    let recs = [tw("01T00000000000000000000001", T0, DEVICE_B, "L", &[])];
    let (_, ctx) = merge(
        Some(&doc(&[], "L")),
        Some(&doc(&[], "edited outside")),
        &recs,
        &[],
    );
    assert_eq!(ctx.conflicts[0].1.kind, ConflictKind::Thoughts);
}

#[test]
fn thoughts_mutually_reachable_conflict() {
    let recs = [
        tw(
            "01T00000000000000000000001",
            T0,
            DEVICE_B,
            "L",
            &["01T00000000000000000000002"],
        ),
        tw(
            "01T00000000000000000000002",
            T0,
            DEVICE,
            "R",
            &["01T00000000000000000000001"],
        ),
    ];
    let (_, ctx) = merge(Some(&doc(&[], "L")), Some(&doc(&[], "R")), &recs, &[]);
    assert_eq!(ctx.conflicts[0].1.kind, ConflictKind::Thoughts);
}

#[test]
fn thoughts_cleared_not_resurrected_and_reverted_not_overridden() {
    for revert in [false, true] {
        let mut p = pair();
        let alpha = p.a.list_papers()[0].id.clone();
        let nt = p.a.read_notes(&alpha).unwrap().1;
        let t = p.a.set_thoughts(&alpha, "X", nt.as_ref()).unwrap();
        // 讓下一筆不被節流
        let mut lines = change_lines(&p.a_root(), DEVICE);
        for l in &mut lines {
            l["t"] = json!(minutes_ago(30));
        }
        write_change_lines(&p.a_root(), DEVICE, &lines);
        let out = p.out.path().to_path_buf();
        sync_once(&mut p.a, &mut p.b, &out);
        let t = p.a.set_thoughts(&alpha, "Y", Some(&t)).unwrap();
        let mut lines = change_lines(&p.a_root(), DEVICE);
        for l in &mut lines {
            l["t"] = json!(minutes_ago(30));
        }
        write_change_lines(&p.a_root(), DEVICE, &lines);
        // 清空，或改回較早的內容 X
        let last = if revert { "X" } else { "" };
        p.a.set_thoughts(&alpha, last, Some(&t)).unwrap();
        // A 匯入 B 的（B 還是 X）：A 不變
        sync_once(&mut p.b, &mut p.a, &out);
        assert_eq!(
            p.a.read_notes(&alpha).unwrap().0.thoughts,
            last,
            "revert={revert}"
        );
        // B 匯入 A 的：B 也變成那樣
        sync_once(&mut p.a, &mut p.b, &out);
        assert_eq!(
            p.b.read_notes(&alpha).unwrap().0.thoughts,
            last,
            "revert={revert}"
        );
    }
}

#[test]
fn outside_text_normalization() {
    let a = doc(&[hl("h-a", T0, "c")], "t");
    // 篇名不同、標題語言不同、\r\n 與位元組順序記號、paperreader_modified 不同、多一個空行
    let b = format!(
        "\u{feff}{}",
        a.replace("# Paper", "# 另一個篇名")
            .replace("## Thoughts", "## 想法")
            .replace("## Highlights", "## 劃線")
            .replace(T0, T3)
            .replace("\n\n", "\n\n\n")
            .replace('\n', "\r\n")
    );
    assert_eq!(parse(&a).outside_text(), parse(&b).outside_text());
    assert_eq!(parse(&a).outside_text(), "---\n---");
    // 使用者在屬性區加一個鍵、或加一段文字：有差異
    let c = a.replacen("---\n\n", "tags: x\n---\n\n", 1);
    assert_ne!(parse(&a).outside_text(), parse(&c).outside_text());
    let d = a.clone() + "\nmy own paragraph\n";
    assert_ne!(parse(&a).outside_text(), parse(&d).outside_text());
}

#[test]
fn outside_text_difference_writes_conflict_copy() {
    let l = doc(&[], "");
    let r = doc(&[], "") + "\nremote paragraph\n";
    let (m, ctx) = merge(Some(&l), Some(&r), &[], &[]);
    assert!(m.text.is_none(), "留本機的");
    assert!(m.copies[0].text.contains("remote paragraph"));
    assert!(
        ctx.notices
            .iter()
            .any(|n| n.kind == NoticeKind::NotesOutsideText)
    );
}

#[test]
fn incoming_unreadable_block_not_merged() {
    let l = doc(&[], "");
    let r = doc(&[hl("h-a", T0, "c")], "").replace("\"kind\":\"highlight\"", "\"kind\":1");
    let (m, ctx) = merge(Some(&l), Some(&r), &[], &[]);
    assert!(
        ctx.notices
            .iter()
            .any(|n| n.kind == NoticeKind::IncomingUnreadable)
    );
    assert_eq!(m.copies.len(), 1);
    assert!(m.text.is_none());
}

#[test]
fn local_incomplete_highlight_untouched() {
    let full = doc(&[hl("h-a", T0, "c")], "");
    let l = full.replacen("<!-- paperreader:highlight-end -->\n", "", 1);
    let recs = [del("01D00000000000000000000001", T1, "h-a")];
    let (m, _) = merge(Some(&l), Some(&full), &recs, &[]);
    assert!(m.text.is_none(), "不完整的劃線逐位元組保留");
}

#[test]
fn deleted_highlight_removed_both_directions() {
    let mut p = pair();
    let alpha = p.a.list_papers()[0].id.clone();
    let (notes, nt) = p.a.read_notes(&alpha).unwrap();
    p.a.delete_highlight(&alpha, &notes.highlights[0].id, nt.as_ref().unwrap())
        .unwrap();
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    assert!(p.b.read_notes(&alpha).unwrap().0.highlights.is_empty());
    sync_once(&mut p.b, &mut p.a, &out);
    assert!(p.a.read_notes(&alpha).unwrap().0.highlights.is_empty());
}

#[test]
fn repeat_import_no_duplicate_copy() {
    let mut p = pair();
    let alpha = p.a.list_papers()[0].id.clone();
    let path = p.a_root().join("notes/Alpha Paper.md");
    let t = fs::read_to_string(&path).unwrap() + "\nA's own paragraph\n";
    fs::write(&path, t).unwrap();
    let out = p.out.path().to_path_buf();
    let r1 = sync_once(&mut p.a, &mut p.b, &out);
    assert_eq!(r1.conflict_copies.len(), 1);
    let r2 = sync_once(&mut p.a, &mut p.b, &out);
    assert!(r2.conflict_copies.is_empty());
    let n = r2
        .notices
        .iter()
        .find(|n| n.kind == NoticeKind::ConflictCopy)
        .unwrap();
    assert_eq!(n.path.as_deref(), Some(r1.conflict_copies[0].as_str()));
    let copies = fs::read_dir(p.b_root().join("notes"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("(conflict"))
        .count();
    assert_eq!(copies, 1);
    let _ = alpha;
}

#[test]
fn conflict_copy_not_indexed() {
    let mut p = pair();
    let alpha = p.a.list_papers()[0].id.clone();
    let path = p.a_root().join("notes/Alpha Paper.md");
    let t = fs::read_to_string(&path).unwrap() + "\nextra\n";
    fs::write(&path, t).unwrap();
    let r = sync_once(&mut p.a, &mut p.b, p.out.path());
    assert_eq!(r.conflict_copies.len(), 1);
    p.b.rescan().unwrap();
    assert_eq!(
        p.b.read_notes(&alpha).unwrap().0.path.as_deref(),
        Some("notes/Alpha Paper.md")
    );
    assert!(
        !p.b.check(paperreader_core::store::CheckLevel::Quick)
            .unwrap()
            .iter()
            .any(|x| x.kind == ProblemKind::DuplicateNotesFile)
    );
}

#[test]
fn incoming_only_notes_file_matched_by_paper_not_name() {
    let mut p = pair();
    let beta =
        p.a.list_papers()
            .into_iter()
            .find(|x| x.bib.title.as_deref() == Some("Beta Paper"))
            .unwrap()
            .id;
    // 兩邊的筆記檔名稱不同
    p.a.set_thoughts(&beta, "A on beta", None).unwrap();
    fs::rename(
        p.a_root().join("notes/Beta Paper.md"),
        p.a_root().join("notes/Renamed on A.md"),
    )
    .unwrap();
    p.a.rescan().unwrap();
    p.b.set_thoughts(&beta, "B on beta", None).unwrap();
    let r = sync_once(&mut p.a, &mut p.b, p.out.path());
    assert!(
        !p.b_root().join("notes/Renamed on A.md").exists(),
        "以代號配對，不另建"
    );
    assert_eq!(r.conflict_copies.len(), 1, "想法衝突：預設另存匯入的");
}

#[test]
fn local_notes_unreadable_not_merged() {
    let mut p = pair();
    let alpha = p.a.list_papers()[0].id.clone();
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "from A", nt.as_ref()).unwrap();
    let path = p.b_root().join("notes/Alpha Paper.md");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend_from_slice(&[0xff, 0xfe, 0x00]);
    fs::write(&path, &bytes).unwrap();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        preview
            .notices
            .iter()
            .any(|n| n.kind == NoticeKind::LocalUnreadable)
    );
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(
        fs::read_dir(p.b_root().join("notes")).unwrap().count(),
        1,
        "不另建一份"
    );
}
