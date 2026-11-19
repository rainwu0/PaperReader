//! 譯文疊加檔：建立、讀、更新一批段落；原子寫入、版本記號、衝突、讀不懂不覆寫、
//! 不認得的欄位保留、`check` 認得它、備份與回收區自然包含。

mod common;

use common::*;
use paperreader_core::backup::restore_backup;
use paperreader_core::error::LibError;
use paperreader_core::jsonio::{VersionToken, sha256_hex};
use paperreader_core::local::LocalLibrary;
use paperreader_core::overlay::*;
use paperreader_core::store::{CheckLevel, ProblemKind};
use std::fs;
use std::path::PathBuf;

struct Fx {
    dir: tempfile::TempDir,
    lib: LocalLibrary,
    paper: String,
    material: String,
}

fn setup() -> Fx {
    let (dir, mut lib) = temp_library();
    let (paper, material) = add_paper(&mut lib, 1, "Zhao Paper", Some(10));
    Fx {
        dir,
        lib,
        paper,
        material,
    }
}

fn seg(page: u32, source: &str) -> SegmentDraft {
    SegmentDraft {
        page,
        bbox: [72.0, 600.0, 300.0, 700.0],
        kind: "body".into(),
        size: Some(9.96),
        source: source.into(),
    }
}

impl Fx {
    fn folder(&self) -> PathBuf {
        self.dir.path().join("papers").join("Zhao Paper")
    }
    fn overlay_path(&self) -> PathBuf {
        self.folder().join("translation-overlay.json")
    }
    fn draft(&self, n: usize) -> OverlayDraft {
        OverlayDraft {
            layout_model_pages: Vec::new(),
            source_material: self.material.clone(),
            provider: "anthropic".into(),
            model: "claude-test".into(),
            source_lang: Some("en".into()),
            target_lang: "zh-TW".into(),
            segments: (0..n)
                .map(|i| seg(i as u32 % 10, &format!("Paragraph {i}.")))
                .collect(),
        }
    }
    fn paper_token(&self) -> VersionToken {
        self.lib.get_paper(&self.paper).unwrap().1
    }
    fn create(&mut self, n: usize) -> (String, VersionToken) {
        let token = self.paper_token();
        let r = self
            .lib
            .create_overlay(&self.paper, self.draft(n), &token)
            .unwrap();
        (r.material_id, r.overlay_token)
    }
    fn raw(&self) -> serde_json::Value {
        serde_json::from_slice(&fs::read(self.overlay_path()).unwrap()).unwrap()
    }
}

fn update(id: &str, status: &str, target: Option<&str>) -> SegmentUpdate {
    SegmentUpdate {
        id: id.into(),
        status: status.into(),
        target: target.map(String::from),
        error: if status == STATUS_FAILED {
            Some("BadResponse".into())
        } else {
            None
        },
        issues: vec![],
        service: None,
        by_user: false,
    }
}

#[test]
fn create_writes_the_file_and_lists_the_material() {
    let mut fx = setup();
    let token = fx.paper_token();
    let r = fx
        .lib
        .create_overlay(&fx.paper, fx.draft(3), &token)
        .unwrap();

    // 檔案、版本記號
    let bytes = fs::read(fx.overlay_path()).unwrap();
    assert_eq!(r.overlay_token, VersionToken(sha256_hex(&bytes)));
    let v = fx.raw();
    assert_eq!(v["format"], 1);
    assert_eq!(v["source_material"], fx.material);
    assert_eq!(v["provider"], "anthropic");
    assert_eq!(v["target_lang"], "zh-TW");
    assert_eq!(v["created_by"], DEVICE);
    let segs = v["segments"].as_array().unwrap();
    assert_eq!(segs.len(), 3);
    assert_eq!(segs[0]["id"], "g0001");
    assert_eq!(segs[2]["id"], "g0003");
    assert_eq!(segs[0]["status"], "pending");
    assert!(segs[0]["target"].is_null());
    assert_eq!(
        segs[0]["bbox"],
        serde_json::json!([72.0, 600.0, 300.0, 700.0])
    );

    // 材料清單
    let (paper, new_token) = fx.lib.get_paper(&fx.paper).unwrap();
    assert_eq!(new_token, r.paper_token);
    let m = paper
        .materials
        .iter()
        .find(|m| m.id == r.material_id)
        .unwrap();
    assert_eq!(m.role, "translation");
    assert_eq!(m.form, "overlay");
    assert_eq!(m.path, "translation-overlay.json");
    assert_eq!(m.sha256, None);
    assert_eq!(m.bytes, None);
    assert_eq!(m.pages, Some(10), "same page count as the source");
    assert_eq!(m.provenance.method.as_deref(), Some("generated"));
    assert_eq!(m.provenance.proofread, Some(false));
    assert!(m.provenance.source_sha256.is_some());

    // 目錄與讀出
    let summary = fx
        .lib
        .list_papers()
        .into_iter()
        .find(|p| p.id == fx.paper)
        .unwrap();
    assert!(summary.materials.iter().any(|m| m.form == "overlay"));
    let (doc, t) = fx.lib.read_overlay(&fx.paper, &r.material_id).unwrap();
    assert_eq!(t, r.overlay_token);
    assert_eq!(doc.segments.len(), 3);
    assert_eq!(doc.segments[1].source, "Paragraph 1.");
}

#[test]
fn stale_paper_token_is_a_conflict_and_writes_nothing() {
    let mut fx = setup();
    let before = snapshot(fx.dir.path());
    let stale = VersionToken("0".repeat(64));
    let r = fx.lib.create_overlay(&fx.paper, fx.draft(2), &stale);
    assert!(matches!(r, Err(LibError::Conflict)));
    assert_eq!(snapshot(fx.dir.path()), before);
}

#[test]
fn invalid_drafts_are_refused_and_leave_nothing_behind() {
    let mut fx = setup();
    let token = fx.paper_token();
    let before = snapshot(fx.dir.path());
    let mut cases: Vec<(&str, OverlayDraft)> = Vec::new();
    let mut d = fx.draft(1);
    d.segments[0].page = 10; // 只有 10 頁，頁序 0 到 9
    cases.push(("page beyond the material", d));
    let mut d = fx.draft(1);
    d.segments[0].bbox = [300.0, 600.0, 72.0, 700.0];
    cases.push(("reversed bbox", d));
    let mut d = fx.draft(1);
    d.segments[0].bbox = [f64::NAN, 0.0, 1.0, 1.0];
    cases.push(("NaN bbox", d));
    let mut d = fx.draft(1);
    d.segments[0].source = "  ".into();
    cases.push(("empty source", d));
    let mut d = fx.draft(1);
    d.segments[0].source = "x".repeat(MAX_TEXT_CHARS + 1);
    cases.push(("too long source", d));
    let mut d = fx.draft(1);
    d.target_lang = "".into();
    cases.push(("no target language", d));
    let mut d = fx.draft(1);
    d.segments = (0..MAX_SEGMENTS + 1).map(|_| seg(0, "x")).collect();
    cases.push(("too many segments", d));
    for (what, d) in cases {
        let r = fx.lib.create_overlay(&fx.paper, d, &token);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{what}: {r:?}"
        );
        assert_eq!(snapshot(fx.dir.path()), before, "{what}");
    }
    let mut d = fx.draft(1);
    d.source_material = "01JCK3T1A2B3C4D5E6F7G8H9JK".into();
    assert!(matches!(
        fx.lib.create_overlay(&fx.paper, d, &token),
        Err(LibError::NotFound { .. })
    ));
    assert_eq!(snapshot(fx.dir.path()), before);
}

#[test]
fn a_second_overlay_gets_a_free_name_and_never_overwrites() {
    let mut fx = setup();
    // 使用者自己放了一個同名的檔案
    fs::write(fx.overlay_path(), b"mine").unwrap();
    let token = fx.paper_token();
    let r = fx
        .lib
        .create_overlay(&fx.paper, fx.draft(1), &token)
        .unwrap();
    assert_eq!(fs::read(fx.overlay_path()).unwrap(), b"mine");
    let (paper, _) = fx.lib.get_paper(&fx.paper).unwrap();
    let m = paper
        .materials
        .iter()
        .find(|m| m.id == r.material_id)
        .unwrap();
    assert_eq!(m.path, "translation-overlay-2.json");
    let t2 = fx.paper_token();
    let r2 = fx.lib.create_overlay(&fx.paper, fx.draft(1), &t2).unwrap();
    let (paper, _) = fx.lib.get_paper(&fx.paper).unwrap();
    let m2 = paper
        .materials
        .iter()
        .find(|m| m.id == r2.material_id)
        .unwrap();
    assert_eq!(m2.path, "translation-overlay-3.json");
}

#[test]
fn update_changes_only_the_overlay_file_and_the_given_segments() {
    let mut fx = setup();
    let (mid, t0) = fx.create(4);
    let paper_before = fs::read(fx.folder().join("paper.json")).unwrap();
    let t1 = fx
        .lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[
                update("g0001", STATUS_DONE, Some("第一段")),
                update("g0003", STATUS_FAILED, None),
            ],
            &t0,
        )
        .unwrap();
    assert_ne!(t0, t1);
    assert_eq!(
        fs::read(fx.folder().join("paper.json")).unwrap(),
        paper_before,
        "paper.json is not touched by a batch update"
    );
    let (doc, t) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert_eq!(t, t1);
    assert_eq!(doc.segments[0].status, "done");
    assert_eq!(doc.segments[0].target.as_deref(), Some("第一段"));
    assert_eq!(doc.segments[1].status, "pending");
    assert_eq!(doc.segments[2].status, "failed");
    assert_eq!(doc.segments[2].error.as_deref(), Some("BadResponse"));
    assert_eq!(doc.segments[3].status, "pending");
    assert!(doc.modified.is_some() && doc.modified_by.as_deref() == Some(DEVICE));

    // 失敗的再翻成功：錯誤清掉；清回待翻：譯文清掉
    let t2 = fx
        .lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[
                update("g0003", STATUS_DONE, Some("第三段")),
                update("g0001", STATUS_PENDING, None),
            ],
            &t1,
        )
        .unwrap();
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert_eq!(doc.segments[2].error, None);
    assert_eq!(doc.segments[2].target.as_deref(), Some("第三段"));
    assert_eq!(doc.segments[0].target, None);
    let _ = t2;
}

#[test]
fn check_issues_are_kept_on_done_segments_and_cleared_otherwise() {
    let mut fx = setup();
    let (mid, t0) = fx.create(2);
    let mut done = update("g0001", STATUS_DONE, Some("第一段"));
    done.issues = vec!["missing_citation".into(), "missing_number".into()];
    let t1 = fx
        .lib
        .update_overlay_segments(&fx.paper, &mid, &[done], &t0)
        .unwrap();
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert_eq!(
        doc.segments[0].issues,
        vec!["missing_citation", "missing_number"]
    );
    let raw = String::from_utf8(fs::read(fx.overlay_path()).unwrap()).unwrap();
    assert_eq!(
        raw.matches("\"issues\"").count(),
        1,
        "a segment without issues does not write the field"
    );

    // 重翻成沒有問題：清掉；清回待翻：也清掉
    let t2 = fx
        .lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0001", STATUS_DONE, Some("第一段（重翻）"))],
            &t1,
        )
        .unwrap();
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert!(doc.segments[0].issues.is_empty());
    let mut again = update("g0001", STATUS_DONE, Some("第一段"));
    again.issues = vec!["untranslated".into()];
    let t3 = fx
        .lib
        .update_overlay_segments(&fx.paper, &mid, &[again], &t2)
        .unwrap();
    fx.lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0001", STATUS_PENDING, None)],
            &t3,
        )
        .unwrap();
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert!(doc.segments[0].issues.is_empty());
}

fn by(id: &str, provider: &str, model: &str) -> SegmentService {
    SegmentService {
        id: id.into(),
        provider: provider.into(),
        model: model.into(),
        extra: Default::default(),
    }
}

/// 每一段記下是哪個服務翻的。已翻與翻不出來的記，清回待翻時清掉；沒有時不寫出欄位。
#[test]
fn each_segment_records_the_service_that_translated_it() {
    let mut fx = setup();
    let (mid, t0) = fx.create(3);
    let mut a = update("g0001", STATUS_DONE, Some("第一段"));
    a.service = Some(by("svc-main", "anthropic", "claude-test"));
    let mut b = update("g0002", STATUS_FAILED, None);
    b.service = Some(by("deepl-1", "deepl", ""));
    let t1 = fx
        .lib
        .update_overlay_segments(&fx.paper, &mid, &[a, b], &t0)
        .unwrap();
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert_eq!(
        doc.segments[0].service,
        Some(by("svc-main", "anthropic", "claude-test"))
    );
    assert_eq!(doc.segments[1].service, Some(by("deepl-1", "deepl", "")));
    assert_eq!(doc.segments[2].service, None);
    let v = fx.raw();
    assert_eq!(
        v["segments"][0]["service"],
        serde_json::json!({"id": "svc-main", "provider": "anthropic", "model": "claude-test"})
    );
    assert!(
        v["segments"][2].get("service").is_none(),
        "a segment without a service does not write the field"
    );

    // 換一個服務重翻：換成新的；清回待翻：清掉
    let mut again = update("g0001", STATUS_DONE, Some("第一段（重翻）"));
    again.service = Some(by("bergamot", "bergamot", ""));
    let t2 = fx
        .lib
        .update_overlay_segments(&fx.paper, &mid, &[again], &t1)
        .unwrap();
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert_eq!(
        doc.segments[0].service,
        Some(by("bergamot", "bergamot", ""))
    );
    fx.lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0001", STATUS_PENDING, None)],
            &t2,
        )
        .unwrap();
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert_eq!(doc.segments[0].service, None);
}

/// 使用者決定的段落（並排比較按「用這個」、日後就地改）記 `by_user: true`，任何重翻都不蓋掉。
/// 只有已翻的段落可以帶；沒有時不寫出這個欄位；之後不是使用者的更新會清掉它。
#[test]
fn a_segment_the_user_chose_is_marked() {
    let mut fx = setup();
    let (mid, t0) = fx.create(2);
    let mut a = update("g0001", STATUS_DONE, Some("挑的"));
    a.by_user = true;
    let t1 = fx
        .lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[a, update("g0002", STATUS_DONE, Some("機器翻的"))],
            &t0,
        )
        .unwrap();
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert!(doc.segments[0].by_user);
    assert!(!doc.segments[1].by_user);
    let v = fx.raw();
    assert_eq!(v["segments"][0]["by_user"], serde_json::json!(true));
    assert!(v["segments"][1].get("by_user").is_none());

    // 不是已翻的段落不可以帶，什麼都不寫
    let before = fs::read(fx.overlay_path()).unwrap();
    for status in [STATUS_PENDING, STATUS_FAILED] {
        let mut u = update("g0002", status, None);
        u.by_user = true;
        let e = fx
            .lib
            .update_overlay_segments(&fx.paper, &mid, &[u], &t1)
            .unwrap_err();
        assert!(matches!(e, LibError::InvalidInput { .. }), "{e:?}");
    }
    assert_eq!(fs::read(fx.overlay_path()).unwrap(), before);

    // 使用者在那一段明確重翻（不是使用者決定的譯文）：清掉
    fx.lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0001", STATUS_DONE, Some("重翻"))],
            &t1,
        )
        .unwrap();
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert!(!doc.segments[0].by_user);
}

/// 這一版之前的檔沒有這個欄位：照樣讀得懂；服務裡不認得的欄位寫回時保留。
#[test]
fn files_without_the_service_field_still_read_and_unknown_service_fields_survive() {
    let mut fx = setup();
    let (mid, _) = fx.create(2);
    let mut v = fx.raw();
    v["segments"][0]["status"] = "done".into();
    v["segments"][0]["target"] = "舊的譯文".into();
    v["segments"][1]["status"] = "done".into();
    v["segments"][1]["target"] = "別台的譯文".into();
    v["segments"][1]["service"] =
        serde_json::json!({"id": "x", "provider": "openai", "model": "m", "future": 7});
    fs::write(fx.overlay_path(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let (doc, t) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert_eq!(doc.segments[0].service, None);
    assert_eq!(doc.segments[0].target.as_deref(), Some("舊的譯文"));
    // 改另一段：沒動的兩段原樣
    let mut u = update("g0001", STATUS_DONE, Some("新"));
    u.service = Some(by("svc-main", "anthropic", "claude-test"));
    fx.lib
        .update_overlay_segments(&fx.paper, &mid, &[u], &t)
        .unwrap();
    let v = fx.raw();
    assert_eq!(v["segments"][1]["service"]["future"], 7);
    assert_eq!(v["segments"][1]["service"]["provider"], "openai");
}

#[test]
fn bad_services_are_refused_and_write_nothing() {
    let mut fx = setup();
    let (mid, t0) = fx.create(1);
    let before = fs::read(fx.overlay_path()).unwrap();
    let long = "x".repeat(201);
    let cases: Vec<(&str, SegmentService)> = vec![
        // 待翻的段落沒有服務
        (STATUS_PENDING, by("svc", "anthropic", "")),
        (STATUS_DONE, by("", "anthropic", "")),
        (STATUS_DONE, by("svc", " ", "")),
        (STATUS_DONE, by(&long, "anthropic", "")),
        (STATUS_DONE, by("svc", &long, "")),
        (STATUS_DONE, by("svc", "anthropic", &long)),
    ];
    for (status, s) in cases {
        let mut u = update(
            "g0001",
            status,
            if status == STATUS_DONE {
                Some("譯")
            } else {
                None
            },
        );
        u.service = Some(s.clone());
        let e = fx
            .lib
            .update_overlay_segments(&fx.paper, &mid, &[u], &t0)
            .unwrap_err();
        assert!(matches!(e, LibError::InvalidInput { .. }), "{s:?}: {e:?}");
        assert_eq!(fs::read(fx.overlay_path()).unwrap(), before);
    }
}

#[test]
fn bad_issue_codes_are_refused_and_write_nothing() {
    let mut fx = setup();
    let (mid, t0) = fx.create(1);
    let before = fs::read(fx.overlay_path()).unwrap();
    let cases: Vec<(&str, Vec<String>)> = vec![
        (STATUS_FAILED, vec!["missing_number".into()]),
        (STATUS_DONE, vec!["Has Spaces".into()]),
        (STATUS_DONE, vec![String::new()]),
        (STATUS_DONE, vec!["x".repeat(41)]),
        (STATUS_DONE, vec!["a".into(); 17]),
    ];
    for (status, issues) in cases {
        let mut u = update(
            "g0001",
            status,
            if status == STATUS_DONE {
                Some("譯")
            } else {
                None
            },
        );
        u.issues = issues;
        let e = fx
            .lib
            .update_overlay_segments(&fx.paper, &mid, &[u], &t0)
            .unwrap_err();
        assert!(matches!(e, LibError::InvalidInput { .. }), "{e:?}");
        assert_eq!(fs::read(fx.overlay_path()).unwrap(), before);
    }
}

#[test]
fn stale_token_and_bad_updates_write_nothing() {
    let mut fx = setup();
    let (mid, t0) = fx.create(3);
    let t1 = fx
        .lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0001", STATUS_DONE, Some("一"))],
            &t0,
        )
        .unwrap();
    let before = fs::read(fx.overlay_path()).unwrap();

    // 舊的記號
    assert!(matches!(
        fx.lib.update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0002", STATUS_DONE, Some("二"))],
            &t0
        ),
        Err(LibError::Conflict)
    ));
    // 其中一筆找不到：整批不寫
    assert!(matches!(
        fx.lib.update_overlay_segments(
            &fx.paper,
            &mid,
            &[
                update("g0002", STATUS_DONE, Some("二")),
                update("g9999", STATUS_DONE, Some("x"))
            ],
            &t1
        ),
        Err(LibError::NotFound { .. })
    ));
    // 已翻卻沒有譯文、有譯文卻不是已翻、不認得的狀態
    for bad in [
        update("g0002", STATUS_DONE, None),
        update("g0002", STATUS_DONE, Some("  ")),
        update("g0002", STATUS_PENDING, Some("x")),
        update("g0002", "weird", None),
    ] {
        assert!(
            matches!(
                fx.lib
                    .update_overlay_segments(&fx.paper, &mid, std::slice::from_ref(&bad), &t1),
                Err(LibError::InvalidInput { .. })
            ),
            "{bad:?}"
        );
    }
    assert_eq!(fs::read(fx.overlay_path()).unwrap(), before);
}

#[test]
fn unreadable_file_is_reported_and_never_overwritten() {
    let mut fx = setup();
    let (mid, _) = fx.create(2);
    fs::write(fx.overlay_path(), "{ not json").unwrap();
    let before = fs::read(fx.overlay_path()).unwrap();
    let t = VersionToken(sha256_hex(&before));
    assert!(matches!(
        fx.lib.read_overlay(&fx.paper, &mid),
        Err(LibError::Unreadable { .. })
    ));
    assert!(matches!(
        fx.lib.update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0001", STATUS_DONE, Some("x"))],
            &t
        ),
        Err(LibError::Unreadable { .. })
    ));
    assert_eq!(fs::read(fx.overlay_path()).unwrap(), before);
    let problems = fx.lib.check(CheckLevel::Quick).unwrap();
    assert!(
        problems
            .iter()
            .any(|p| p.kind == ProblemKind::UnreadableOverlayFile)
    );
}

#[test]
fn newer_format_can_be_read_but_not_written() {
    let mut fx = setup();
    let (mid, _) = fx.create(1);
    let mut v = fx.raw();
    v["format"] = serde_json::json!(99);
    fs::write(fx.overlay_path(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let (doc, t) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert_eq!(doc.format, 99);
    assert!(matches!(
        fx.lib.update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0001", STATUS_DONE, Some("x"))],
            &t
        ),
        Err(LibError::FormatTooNew { found: 99 })
    ));
}

#[test]
fn unknown_fields_survive_a_round_trip() {
    let mut fx = setup();
    let (mid, _) = fx.create(2);
    let mut v = fx.raw();
    v["future_top"] = serde_json::json!({"a": 1});
    v["segments"][0]["future_seg"] = serde_json::json!("keep me");
    fs::write(fx.overlay_path(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let (_, t) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    fx.lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0002", STATUS_DONE, Some("二"))],
            &t,
        )
        .unwrap();
    let v = fx.raw();
    assert_eq!(v["future_top"], serde_json::json!({"a": 1}));
    assert_eq!(v["segments"][0]["future_seg"], "keep me");
}

#[test]
fn only_overlay_materials_can_be_read_as_overlays() {
    let mut fx = setup();
    assert!(matches!(
        fx.lib.read_overlay(&fx.paper, &fx.material),
        Err(LibError::InvalidInput { .. })
    ));
    assert!(matches!(
        fx.lib.read_overlay(&fx.paper, "01JCK3T1A2B3C4D5E6F7G8H9JK"),
        Err(LibError::NotFound { .. })
    ));
    let _ = &mut fx;
}

#[test]
fn full_check_stays_quiet_while_the_file_keeps_changing() {
    let mut fx = setup();
    let (mid, t0) = fx.create(2);
    fx.lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0001", STATUS_DONE, Some("一"))],
            &t0,
        )
        .unwrap();
    for level in [CheckLevel::Quick, CheckLevel::Full] {
        let problems = fx.lib.check(level).unwrap();
        assert!(problems.is_empty(), "{level:?}: {problems:?}");
    }
}

#[test]
fn backup_and_trash_include_the_overlay() {
    let mut fx = setup();
    let (mid, t0) = fx.create(2);
    fx.lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0001", STATUS_DONE, Some("一"))],
            &t0,
        )
        .unwrap();

    let work = tempfile::tempdir().unwrap();
    let archive = work.path().join("backup.zip");
    fx.lib.export_backup(&archive, &mut |_, _| {}).unwrap();
    let target = work.path().join("restored");
    let report = restore_backup(&archive, &target, DEVICE).unwrap();
    assert!(report.manifest_mismatches.is_empty());
    assert_eq!(
        fs::read(target.join("papers/Zhao Paper/translation-overlay.json")).unwrap(),
        fs::read(fx.overlay_path()).unwrap()
    );

    let bytes = fs::read(fx.overlay_path()).unwrap();
    fx.lib.remove_paper(&fx.paper).unwrap();
    assert!(!fx.overlay_path().exists());
    fx.lib.restore_paper(&fx.paper).unwrap();
    assert_eq!(fs::read(fx.overlay_path()).unwrap(), bytes);
}

#[test]
fn read_only_library_refuses_writes() {
    let mut fx = setup();
    let (mid, t0) = fx.create(1);
    let mut v: serde_json::Value =
        serde_json::from_slice(&fs::read(fx.dir.path().join("library.json")).unwrap()).unwrap();
    v["format"] = serde_json::json!(99);
    fs::write(
        fx.dir.path().join("library.json"),
        serde_json::to_string(&v).unwrap(),
    )
    .unwrap();
    let mut ro = LocalLibrary::open(fx.dir.path(), DEVICE).unwrap();
    assert!(ro.is_read_only());
    assert!(matches!(
        ro.update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0001", STATUS_DONE, Some("x"))],
            &t0
        ),
        Err(LibError::ReadOnly)
    ));
    let pt = ro.get_paper(&fx.paper).unwrap().1;
    assert!(matches!(
        ro.create_overlay(&fx.paper, fx.draft(1), &pt),
        Err(LibError::ReadOnly)
    ));
    assert!(ro.read_overlay(&fx.paper, &mid).is_ok());
}

// ---- 這份譯本的術語----

fn term(source: &str, target: &str, state: &str, batches: u32) -> OverlayTerm {
    OverlayTerm {
        source: source.into(),
        target: target.into(),
        state: state.into(),
        batches,
        extra: Default::default(),
    }
}

#[test]
fn terms_live_in_the_overlay_and_old_files_have_none() {
    let mut fx = setup();
    let (mid, token) = fx.create(3);
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert!(doc.terms.is_empty());
    assert!(
        fx.raw().get("terms").is_none(),
        "no terms key until there are terms"
    );
    let terms = vec![
        term("token", "詞元", TERM_LEARNED, 2),
        term("stretch", "伸展參數", TERM_CANDIDATE, 1),
        term("policy", "策略", TERM_USER, 0),
        term("state", "", TERM_REMOVED, 0),
    ];
    // 一批段落與術語在同一次寫入
    let t = fx
        .lib
        .update_overlay(
            &fx.paper,
            &mid,
            &[update("g0001", STATUS_DONE, Some("一"))],
            Some(&terms),
            &token,
        )
        .unwrap();
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert_eq!(doc.terms, terms);
    assert_eq!(doc.segments[0].target.as_deref(), Some("一"));
    assert_eq!(fx.raw()["terms"][0]["state"], "learned");
    // 只更新段落：術語不動
    fx.lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0002", STATUS_DONE, Some("二"))],
            &t,
        )
        .unwrap();
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert_eq!(doc.terms, terms);
}

#[test]
fn unknown_fields_of_a_term_are_kept() {
    let mut fx = setup();
    let (mid, _) = fx.create(1);
    let mut v = fx.raw();
    v["terms"] = serde_json::json!([{"source": "token", "target": "詞元", "state": "learned", "batches": 2, "later": {"x": 1}}]);
    fs::write(fx.overlay_path(), serde_json::to_vec(&v).unwrap()).unwrap();
    let (_, token) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    fx.lib
        .update_overlay_segments(
            &fx.paper,
            &mid,
            &[update("g0001", STATUS_DONE, Some("一"))],
            &token,
        )
        .unwrap();
    assert_eq!(fx.raw()["terms"][0]["later"]["x"], 1);
}

#[test]
fn bad_terms_are_refused_and_nothing_is_written() {
    let mut fx = setup();
    let (mid, token) = fx.create(1);
    let before = fs::read(fx.overlay_path()).unwrap();
    let cases: Vec<Vec<OverlayTerm>> = vec![
        vec![term("token", "詞元", "Weird!", 1)],
        vec![term("", "詞元", TERM_LEARNED, 2)],
        vec![term("token", "", TERM_LEARNED, 2)],
        vec![term(&"x".repeat(MAX_TERM_CHARS + 1), "詞元", TERM_USER, 0)],
        vec![term("tok\nen", "詞元", TERM_USER, 0)],
        vec![
            term("Token", "詞元", TERM_USER, 0),
            term("token", "符記", TERM_LEARNED, 2),
        ],
        (0..=MAX_TERMS)
            .map(|i| term(&format!("term{i}"), "詞", TERM_CANDIDATE, 1))
            .collect(),
    ];
    for (i, terms) in cases.iter().enumerate() {
        let e = fx
            .lib
            .update_overlay(&fx.paper, &mid, &[], Some(terms), &token)
            .unwrap_err();
        assert!(
            matches!(e, LibError::InvalidInput { .. }),
            "case {i}: {e:?}"
        );
        assert_eq!(fs::read(fx.overlay_path()).unwrap(), before, "case {i}");
    }
}

#[test]
fn pages_segmented_by_the_layout_model_are_kept_in_the_header() {
    let mut fx = setup();
    // 沒有用版面模型：不寫出這個欄位，讀回是空的（0.1.5 之前的檔也是這樣）
    let (mid, _) = fx.create(3);
    assert!(fx.raw().get("layout_model_pages").is_none());
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &mid).unwrap();
    assert!(doc.layout_model_pages.is_empty());

    // 有用：依頁序排好、去掉重複；更新段落之後照樣留著
    let mut fx = setup();
    let mut draft = fx.draft(3);
    draft.layout_model_pages = vec![3, 0, 3, 1];
    let token = fx.paper_token();
    let r = fx.lib.create_overlay(&fx.paper, draft, &token).unwrap();
    assert_eq!(fx.raw()["layout_model_pages"], serde_json::json!([0, 1, 3]));
    fx.lib
        .update_overlay_segments(
            &fx.paper,
            &r.material_id,
            &[update("g0001", STATUS_DONE, Some("一"))],
            &r.overlay_token,
        )
        .unwrap();
    let (doc, _) = fx.lib.read_overlay(&fx.paper, &r.material_id).unwrap();
    assert_eq!(doc.layout_model_pages, vec![0, 1, 3]);
    let back: OverlayFile = serde_json::from_value(serde_json::to_value(&doc).unwrap()).unwrap();
    assert_eq!(back, doc);

    // 頁序超出原文的頁數：不建立
    let mut fx = setup();
    let mut draft = fx.draft(3);
    draft.layout_model_pages = vec![10];
    let token = fx.paper_token();
    let e = fx.lib.create_overlay(&fx.paper, draft, &token).unwrap_err();
    assert!(matches!(e, LibError::InvalidInput { .. }), "{e:?}");
}
