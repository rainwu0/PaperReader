//! `ink.json` 與譯文疊加檔的合併。純計算。

mod common;

use common::*;
use paperreader_core::ink::{InkFile, Stroke};
use paperreader_core::overlay::{
    OverlayFile, OverlaySegment, OverlayTerm, TERM_LEARNED, TERM_USER,
};
use paperreader_core::store::{ConflictChoice, ConflictKind, NoticeKind};
use paperreader_core::sync::merge::ink::{merge_ink, merge_overlay};
use serde_json::json;
use std::collections::HashSet;

fn stroke(id: &str, page: u32) -> Stroke {
    Stroke {
        id: id.into(),
        material: "M".into(),
        page,
        color: "#000000".into(),
        width: 1.0,
        opacity: 1.0,
        created: "2026-11-01T00:00:00.000Z".into(),
        points: vec![[1.0, 1.0]],
        extra: Default::default(),
    }
}

fn ink(ids: &[(&str, u32)]) -> InkFile {
    InkFile {
        strokes: ids.iter().map(|(i, p)| stroke(i, *p)).collect(),
        ..Default::default()
    }
}

#[test]
fn strokes_union_minus_deleted() {
    let l = ink(&[("s1", 0), ("s2", 0)]);
    let r = ink(&[("s2", 0), ("s3", 1), ("s4", 1)]);
    let deleted: HashSet<String> = ["s1".to_string(), "s4".to_string()].into();
    let mut ctx = mctx(&[]);
    let doc = merge_ink(Some(&l), Some(&r), &deleted, "P", None, &mut ctx).unwrap();
    let ids: Vec<&str> = doc.strokes.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["s2", "s3"]);
    assert_eq!(doc.modified.as_deref(), Some(NOW));
    assert_eq!(doc.modified_by.as_deref(), Some(DEVICE_B));
}

#[test]
fn clear_page_record_removes_only_listed_ids() {
    // 對方後來畫在同一頁的筆跡（代號不在記錄裡）保留
    let l = ink(&[("s1", 2), ("s2", 2)]);
    let r = ink(&[("s9", 2)]);
    let deleted: HashSet<String> = ["s1".to_string(), "s2".to_string()].into();
    let mut ctx = mctx(&[]);
    let doc = merge_ink(Some(&l), Some(&r), &deleted, "P", None, &mut ctx).unwrap();
    let ids: Vec<&str> = doc.strokes.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["s9"]);
}

#[test]
fn same_id_different_content_keeps_local() {
    let l = ink(&[("s1", 0)]);
    let mut r = ink(&[("s1", 0)]);
    r.strokes[0].color = "#ffffff".into();
    let mut ctx = mctx(&[]);
    assert!(merge_ink(Some(&l), Some(&r), &HashSet::new(), "P", None, &mut ctx).is_none());
}

#[test]
fn unchanged_ink_not_rewritten() {
    let l = ink(&[("s1", 0), ("s2", 0)]);
    let r = ink(&[("s2", 0)]);
    let mut ctx = mctx(&[]);
    assert!(merge_ink(Some(&l), Some(&r), &HashSet::new(), "P", None, &mut ctx).is_none());
}

#[test]
fn unreadable_ink_not_merged() {
    let mut p = pair();
    let alpha = p.a.list_papers()[0].clone();
    let m = alpha.materials[0].id.clone();
    p.a.add_stroke(&alpha.id, stroke_draft(&m, 0), None)
        .unwrap();
    std::fs::write(
        p.b_root()
            .join("papers")
            .join(&alpha.folder)
            .join("ink.json"),
        "{broken",
    )
    .unwrap();
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        preview
            .notices
            .iter()
            .any(|n| n.kind == NoticeKind::LocalUnreadable)
    );
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert_eq!(
        std::fs::read_to_string(
            p.b_root()
                .join("papers")
                .join(&alpha.folder)
                .join("ink.json")
        )
        .unwrap(),
        "{broken"
    );
}

fn seg(id: &str, status: &str, target: Option<&str>, modified: Option<&str>) -> OverlaySegment {
    OverlaySegment {
        id: id.into(),
        page: 0,
        bbox: [1.0, 1.0, 2.0, 2.0],
        kind: "body".into(),
        size: None,
        source: "s".into(),
        target: target.map(String::from),
        status: status.into(),
        error: None,
        issues: vec![],
        modified: modified.map(String::from),
        modified_by: modified.map(|_| DEVICE.to_string()),
        service: None,
        by_user: false,
        extra: Default::default(),
    }
}

/// 「使用者決定的」標記跟著勝出的那一段走；只差在這個標記的兩段也算不同。
#[test]
fn overlay_segment_user_mark_moves_with_the_winning_segment() {
    let a = seg("g1", "done", Some("same"), Some("2026-11-01T00:00:00.000Z"));
    let mut b = seg("g1", "done", Some("same"), Some("2026-11-02T00:00:00.000Z"));
    b.by_user = true;
    let (o, _) = om(&overlay(vec![a.clone()]), &overlay(vec![b.clone()]), &[]);
    assert!(o.unwrap().segments[0].by_user);
    // 本機是使用者挑的（較舊）、匯入那段是較新的整批翻譯（沒有標記）：使用者挑的留著，不看時間（
    // 任何重翻都不蓋掉，同步帶來的也一樣）
    let mut newer_plain = b.clone();
    newer_plain.by_user = false;
    newer_plain.target = Some("machine".into());
    let mut old_mark = a.clone();
    old_mark.by_user = true;
    old_mark.target = Some("chosen".into());
    let (o, _) = om(
        &overlay(vec![old_mark.clone()]),
        &overlay(vec![newer_plain.clone()]),
        &[],
    );
    assert!(
        o.is_none(),
        "nothing to write: the local chosen segment stays"
    );
    // 反過來：本機是較新的整批翻譯、匯入那段是使用者挑的（較舊）：取匯入那段
    let (o, _) = om(&overlay(vec![newer_plain]), &overlay(vec![old_mark]), &[]);
    let o = o.unwrap();
    assert!(o.segments[0].by_user);
    assert_eq!(o.segments[0].target.as_deref(), Some("chosen"));
    // 兩邊都是挑的：照較新的
    let mut a2 = a;
    a2.by_user = true;
    a2.target = Some("first pick".into());
    let mut b2 = b;
    b2.by_user = true;
    b2.target = Some("second pick".into());
    let (o, _) = om(&overlay(vec![a2]), &overlay(vec![b2]), &[]);
    assert_eq!(
        o.unwrap().segments[0].target.as_deref(),
        Some("second pick")
    );
}

fn svc(id: &str) -> paperreader_core::overlay::SegmentService {
    paperreader_core::overlay::SegmentService {
        id: id.into(),
        provider: "deepl".into(),
        model: String::new(),
        extra: Default::default(),
    }
}

/// 較新的一段勝出時，翻它的服務跟著那一段走；只差在服務的兩段也算不同。
#[test]
fn overlay_segment_service_moves_with_the_winning_segment() {
    let mut a = seg("g1", "done", Some("old"), Some("2026-11-01T00:00:00.000Z"));
    a.service = Some(svc("local-svc"));
    let mut b = seg("g1", "done", Some("new"), Some("2026-11-02T00:00:00.000Z"));
    b.service = Some(svc("remote-svc"));
    let (o, _) = om(&overlay(vec![a.clone()]), &overlay(vec![b.clone()]), &[]);
    let o = o.unwrap();
    assert_eq!(o.segments[0].target.as_deref(), Some("new"));
    assert_eq!(o.segments[0].service, Some(svc("remote-svc")));
    // 匯入那邊較新、而且沒有服務：本機的服務不留下來
    let mut c = b.clone();
    c.service = None;
    let (o, _) = om(&overlay(vec![a.clone()]), &overlay(vec![c]), &[]);
    assert_eq!(o.unwrap().segments[0].service, None);
    // 只差在服務、時間較新：取匯入那一段
    let mut d = a.clone();
    d.modified = Some("2026-11-03T00:00:00.000Z".into());
    d.service = Some(svc("other"));
    let (o, _) = om(&overlay(vec![a]), &overlay(vec![d]), &[]);
    assert_eq!(o.unwrap().segments[0].service, Some(svc("other")));
}

fn overlay(segs: Vec<OverlaySegment>) -> OverlayFile {
    OverlayFile {
        format: 1,
        source_material: "M".into(),
        provider: "p".into(),
        model: "m".into(),
        source_lang: None,
        target_lang: "zh-TW".into(),
        created: None,
        created_by: None,
        modified: None,
        modified_by: None,
        segments: segs,
        terms: vec![],
        layout_model_pages: vec![],
        extra: Default::default(),
    }
}

/// 這份譯本的術語跟著疊加檔同步：依原文詞合併，另一台使用者改的勝過這台學到的。
#[test]
fn overlay_terms_are_merged_and_the_users_decision_wins() {
    let t = |s: &str, tg: &str, st: &str| OverlayTerm {
        source: s.into(),
        target: tg.into(),
        state: st.into(),
        batches: 2,
        extra: Default::default(),
    };
    let mut local = overlay(vec![]);
    local.terms = vec![t("token", "權杖", TERM_LEARNED)];
    let mut remote = overlay(vec![]);
    remote.terms = vec![
        t("token", "詞元", TERM_USER),
        t("stretch", "伸展參數", TERM_LEARNED),
    ];
    let (o, _) = om(&local, &remote, &[]);
    let o = o.expect("the terms changed, so the file is written");
    assert_eq!(
        o.terms,
        vec![
            t("token", "詞元", TERM_USER),
            t("stretch", "伸展參數", TERM_LEARNED)
        ]
    );
    // 一樣的時候不寫
    let (o, _) = om(&local, &local.clone(), &[]);
    assert!(o.is_none());
}

fn om(
    l: &OverlayFile,
    r: &OverlayFile,
    choices: &[(ConflictKind, Option<&str>, Option<&str>, ConflictChoice)],
) -> (Option<OverlayFile>, paperreader_core::sync::merge::MergeCtx) {
    let mut ctx = mctx(choices);
    let out = merge_overlay(l, r, "P", "OV", "t.json", None, &mut ctx);
    (out, ctx)
}

#[test]
fn overlay_segment_newer_modified_wins() {
    let l = overlay(vec![seg(
        "g1",
        "done",
        Some("old"),
        Some("2026-11-01T00:00:00.000Z"),
    )]);
    let r = overlay(vec![seg(
        "g1",
        "pending",
        None,
        Some("2026-11-02T00:00:00.000Z"),
    )]);
    let (o, _) = om(&l, &r, &[]);
    let o = o.unwrap();
    assert_eq!(o.segments[0].status, "pending", "較新的清回待翻勝出");
    assert_eq!(o.modified.as_deref(), Some(NOW));
}

#[test]
fn overlay_segment_with_time_beats_without() {
    let l = overlay(vec![seg("g1", "done", Some("old"), None)]);
    let r = overlay(vec![seg(
        "g1",
        "pending",
        None,
        Some("2026-11-02T00:00:00.000Z"),
    )]);
    let (o, _) = om(&l, &r, &[]);
    assert_eq!(o.unwrap().segments[0].status, "pending");
    let (o, _) = om(&r, &l, &[]);
    assert!(o.is_none(), "有時間的本機勝出：不必寫");
}

#[test]
fn overlay_without_times_done_beats_failed_beats_pending() {
    let l = overlay(vec![
        seg("g1", "pending", None, None),
        seg("g2", "failed", None, None),
    ]);
    let r = overlay(vec![
        seg("g1", "failed", None, None),
        seg("g2", "done", Some("t"), None),
    ]);
    let (o, _) = om(&l, &r, &[]);
    let o = o.unwrap();
    assert_eq!(o.segments[0].status, "failed");
    assert_eq!(o.segments[1].status, "done");
}

#[test]
fn overlay_both_done_different_one_conflict_with_count_and_resolution_stamps() {
    let l = overlay(vec![
        seg("g1", "done", Some("a"), None),
        seg("g2", "done", Some("b"), None),
        seg("g3", "done", Some("same"), None),
    ]);
    let r = overlay(vec![
        seg("g1", "done", Some("A"), None),
        seg("g2", "done", Some("B"), None),
        seg("g3", "done", Some("same"), None),
    ]);
    let (o, ctx) = om(&l, &r, &[]);
    assert_eq!(ctx.conflicts.len(), 1);
    assert_eq!(ctx.conflicts[0].1.kind, ConflictKind::Overlay);
    assert_eq!(ctx.conflicts[0].1.count, 2);
    let o = o.unwrap();
    assert_eq!(o.segments[0].target.as_deref(), Some("a"));
    assert_eq!(o.segments[0].modified.as_deref(), Some(NOW));
    assert_eq!(o.segments[0].modified_by.as_deref(), Some(DEVICE_B));
    assert_eq!(o.segments[2].modified, None);
    // 選 incoming
    let (o, _) = om(
        &l,
        &r,
        &[(
            ConflictKind::Overlay,
            Some("P"),
            Some("OV"),
            ConflictChoice::Incoming,
        )],
    );
    assert_eq!(o.unwrap().segments[1].target.as_deref(), Some("B"));
    // 反向：對方那邊沒有時間，這邊有時間的勝出，不再問
    let (resolved, _) = om(&l, &r, &[]);
    let (_, ctx) = om(&r, &resolved.unwrap(), &[]);
    assert!(ctx.conflicts.is_empty());
}

#[test]
fn overlay_segment_ids_differ_keeps_local() {
    let l = overlay(vec![seg("g1", "done", Some("a"), None)]);
    let r = overlay(vec![seg("g9", "done", Some("b"), None)]);
    let (o, ctx) = om(&l, &r, &[]);
    assert!(o.is_none());
    assert!(
        ctx.notices
            .iter()
            .any(|n| n.kind == NoticeKind::IncomingUnreadable)
    );
}

#[test]
fn overlay_header_kept_local() {
    let l = overlay(vec![seg("g1", "pending", None, None)]);
    let mut r = overlay(vec![seg("g1", "done", Some("x"), None)]);
    r.provider = "other".into();
    r.extra.insert("k".into(), json!(1));
    let (o, _) = om(&l, &r, &[]);
    let o = o.unwrap();
    assert_eq!(o.provider, "p");
}

/// 用版面模型分段的頁跟著檔頭：兩份相同時不動；本機沒有記（較舊的寫入）時用另一台的；兩邊都有、不同時留本機的。
#[test]
fn overlay_layout_model_pages_survive_a_merge() {
    let mut l = overlay(vec![seg("g1", "pending", None, None)]);
    l.layout_model_pages = vec![0, 1];
    let mut r = overlay(vec![seg("g1", "done", Some("x"), None)]);
    r.layout_model_pages = vec![0, 1];
    let (o, _) = om(&l, &r, &[]);
    assert_eq!(o.unwrap().layout_model_pages, vec![0, 1]);

    let mut empty = l.clone();
    empty.layout_model_pages = vec![];
    let (o, _) = om(&empty, &r, &[]);
    assert_eq!(o.unwrap().layout_model_pages, vec![0, 1]);
    // 只差這個欄位也要寫
    let mut same = r.clone();
    same.layout_model_pages = vec![];
    let (o, _) = om(&same, &r, &[]);
    assert_eq!(
        o.expect("the local copy lost the pages").layout_model_pages,
        vec![0, 1]
    );

    let mut other = r.clone();
    other.layout_model_pages = vec![2];
    let (o, _) = om(&l, &other, &[]);
    assert_eq!(o.unwrap().layout_model_pages, vec![0, 1]);
}
