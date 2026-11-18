//! 變更紀錄檔：刪除、移除與放回、想法的寫入照「先紀錄後檔案」附加；
//! 讀取容忍壞行；閱讀紀錄與檢查不受影響；備份帶著它；譯文疊加檔的段落蓋上時間。

mod common;

use common::*;
use paperreader_core::changes::{THOUGHTS_WRITTEN, thoughts_hash};
use paperreader_core::error::LibError;
use paperreader_core::ids::{is_valid_id, new_id, parse_utc_millis};
use paperreader_core::jsonio::VersionToken;
use paperreader_core::local::LocalLibrary;
use paperreader_core::overlay::{OverlayDraft, SegmentDraft, SegmentUpdate};
use paperreader_core::store::{ChangeFilter, CheckLevel, EventFilter};
use serde_json::{Value, json};
use std::fs;

fn kinds(lines: &[Value]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l["type"].as_str().unwrap().to_string())
        .collect()
}

fn notes_token(lib: &LocalLibrary, paper: &str) -> Option<VersionToken> {
    lib.read_notes(paper).unwrap().1
}

#[test]
fn remove_and_restore_write_records() {
    let (dir, mut lib) = temp_library();
    let (p, _) = add_paper(&mut lib, 1, "Alpha", None);
    lib.remove_paper(&p).unwrap();
    let trash_folder = lib.list_trash().unwrap()[0].folder.clone();
    lib.restore_paper(&p).unwrap();

    let lines = change_lines(dir.path(), DEVICE);
    assert_eq!(kinds(&lines), ["paper-removed", "paper-restored"]);
    assert_eq!(lines[0]["data"]["folder"], json!(trash_folder));
    assert_eq!(lines[1]["data"], json!({}));
    for l in &lines {
        assert!(is_valid_id(l["id"].as_str().unwrap()));
        assert!(parse_utc_millis(l["t"].as_str().unwrap()).is_some());
        assert_eq!(l["device"], json!(DEVICE));
        assert_eq!(l["paper"], json!(p));
    }
}

#[test]
fn delete_highlight_writes_record() {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Alpha", Some(5));
    let (h, t) = lib
        .add_highlight(&p, highlight_draft(&m, 0, "x"), None)
        .unwrap();
    lib.delete_highlight(&p, &h.id, &t).unwrap();
    let lines = change_lines(dir.path(), DEVICE);
    assert_eq!(kinds(&lines), ["highlight-deleted"]);
    assert_eq!(lines[0]["data"]["highlight"], json!(h.id));
    assert_eq!(lines[0]["paper"], json!(p));
}

#[test]
fn failed_operations_write_nothing() {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Alpha", Some(5));
    let (h, t) = lib
        .add_highlight(&p, highlight_draft(&m, 0, "x"), None)
        .unwrap();
    let before = change_lines(dir.path(), DEVICE).len();

    // 記號不符
    let stale = VersionToken("0".repeat(64));
    assert!(matches!(
        lib.delete_highlight(&p, &h.id, &stale),
        Err(LibError::Conflict)
    ));
    // 劃線不存在
    assert!(matches!(
        lib.delete_highlight(&p, "h-nope", &t),
        Err(LibError::NotFound { .. })
    ));
    // 劃線不完整：拿掉結尾標記
    let path = dir.path().join(lib.read_notes(&p).unwrap().0.path.unwrap());
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace("<!-- paperreader:highlight-end -->\n", ""),
    )
    .unwrap();
    let t2 = notes_token(&lib, &p).unwrap();
    assert!(matches!(
        lib.delete_highlight(&p, &h.id, &t2),
        Err(LibError::InvalidInput { .. })
    ));
    // 筆跡、移除：記號不符、不存在
    assert!(lib.delete_stroke(&p, "s-nope", &stale).is_err());
    assert!(lib.remove_paper("01NOPE0000000000000000000A").is_err());
    assert!(lib.restore_paper(&p).is_err());
    assert_eq!(change_lines(dir.path(), DEVICE).len(), before);

    // 唯讀的文獻庫
    let mut lf: Value =
        serde_json::from_slice(&fs::read(dir.path().join("library.json")).unwrap()).unwrap();
    lf["format"] = json!(2);
    fs::write(dir.path().join("library.json"), lf.to_string()).unwrap();
    let mut ro = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    assert!(matches!(ro.remove_paper(&p), Err(LibError::ReadOnly)));
    assert!(matches!(
        ro.set_thoughts(&p, "new", Some(&t2)),
        Err(LibError::ReadOnly)
    ));
    assert_eq!(change_lines(dir.path(), DEVICE).len(), before);
}

#[test]
fn clear_page_lists_removed_stroke_ids() {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Alpha", Some(5));
    let mut ids = Vec::new();
    let mut tok = None;
    for page in [2, 2, 2, 3] {
        let (s, t) = lib
            .add_stroke(&p, stroke_draft(&m, page), tok.as_ref())
            .unwrap();
        if page == 2 {
            ids.push(s.id);
        }
        tok = Some(t);
    }
    let t = lib
        .clear_ink_page(&p, &m, 2, tok.as_ref().unwrap())
        .unwrap();
    let lines = change_lines(dir.path(), DEVICE);
    assert_eq!(kinds(&lines), ["strokes-deleted"]);
    assert_eq!(lines[0]["data"]["strokes"], json!(ids));
    // 再清一次：沒有筆跡，不寫
    lib.clear_ink_page(&p, &m, 2, &t).unwrap();
    assert_eq!(change_lines(dir.path(), DEVICE).len(), 1);
}

#[test]
fn delete_stroke_record() {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Alpha", Some(5));
    let (s, t) = lib.add_stroke(&p, stroke_draft(&m, 0), None).unwrap();
    lib.delete_stroke(&p, &s.id, &t).unwrap();
    let lines = change_lines(dir.path(), DEVICE);
    assert_eq!(kinds(&lines), ["strokes-deleted"]);
    assert_eq!(lines[0]["data"]["strokes"], json!([s.id]));
}

#[test]
fn thoughts_records_chain() {
    let (dir, mut lib) = temp_library();
    let (p, _) = add_paper(&mut lib, 1, "Alpha", None);
    let t = lib.set_thoughts(&p, "first", None).unwrap();
    let mut lines = change_lines(dir.path(), DEVICE);
    assert_eq!(kinds(&lines), [THOUGHTS_WRITTEN]);
    assert_eq!(lines[0]["data"]["prev"], json!([]));
    assert_eq!(lines[0]["data"]["sha256"], json!(thoughts_hash("first")));

    // 上一筆改成 11 分鐘前
    lines[0]["t"] = json!(minutes_ago(11));
    write_change_lines(dir.path(), DEVICE, &lines);
    let t = lib.set_thoughts(&p, "second", Some(&t)).unwrap();
    let lines = change_lines(dir.path(), DEVICE);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["data"]["prev"], json!([lines[0]["id"]]));
    assert_eq!(lines[1]["data"]["sha256"], json!(thoughts_hash("second")));

    // 寫入與原本相同的文字：檔案照寫、不寫紀錄
    let t2 = lib.set_thoughts(&p, "second", Some(&t)).unwrap();
    assert_ne!(t, t2, "屬性區的時間會變，檔案照寫");
    assert_eq!(change_lines(dir.path(), DEVICE).len(), 2);
}

#[test]
fn thoughts_throttled_within_interval() {
    let (dir, mut lib) = temp_library();
    let (p, _) = add_paper(&mut lib, 1, "Alpha", None);
    let mut t = lib.set_thoughts(&p, "a", None).unwrap();
    t = lib.set_thoughts(&p, "ab", Some(&t)).unwrap();
    t = lib.set_thoughts(&p, "abc", Some(&t)).unwrap();
    let mut lines = change_lines(dir.path(), DEVICE);
    assert_eq!(lines.len(), 1, "上一筆不到 10 分鐘：只有第一次記");
    lines[0]["t"] = json!(minutes_ago(11));
    write_change_lines(dir.path(), DEVICE, &lines);
    lib.set_thoughts(&p, "abcd", Some(&t)).unwrap();
    let lines = change_lines(dir.path(), DEVICE);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["data"]["prev"], json!([lines[0]["id"]]));
    assert_eq!(lines[1]["data"]["sha256"], json!(thoughts_hash("abcd")));
}

#[test]
fn thoughts_not_throttled_after_import() {
    let (dir, mut lib) = temp_library();
    let (p, _) = add_paper(&mut lib, 1, "Alpha", None);
    lib.set_thoughts(&p, "mine", None).unwrap();
    // 模擬剛匯入：別台寫了 "theirs"（記錄），本機的筆記檔換成那份內容
    let other_id = new_id();
    write_change_lines(
        dir.path(),
        DEVICE_B,
        &[
            json!({"id": other_id, "t": minutes_ago(1), "device": DEVICE_B, "paper": p,
                 "type": THOUGHTS_WRITTEN, "data": {"sha256": thoughts_hash("theirs"), "prev": []}}),
        ],
    );
    let path = dir.path().join(lib.read_notes(&p).unwrap().0.path.unwrap());
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replace("mine", "theirs")).unwrap();

    let t = notes_token(&lib, &p).unwrap();
    lib.set_thoughts(&p, "theirs, edited", Some(&t)).unwrap();
    let lines = change_lines(dir.path(), DEVICE);
    assert_eq!(lines.len(), 2, "寫入前的節點是別台的：不節流");
    assert_eq!(lines[1]["data"]["prev"], json!([other_id]));
}

#[test]
fn record_precedes_file_write() {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Alpha", Some(5));
    let (h, t) = lib
        .add_highlight(&p, highlight_draft(&m, 0, "x"), None)
        .unwrap();
    let notes = dir.path().join(lib.read_notes(&p).unwrap().0.path.unwrap());
    let before = fs::read(&notes).unwrap();
    lib.set_change_hook(Some(Box::new(|_| false)));
    assert!(lib.delete_highlight(&p, &h.id, &t).is_err());
    assert_eq!(
        kinds(&change_lines(dir.path(), DEVICE)),
        ["highlight-deleted"]
    );
    assert_eq!(fs::read(&notes).unwrap(), before, "紀錄已在、檔案未改");

    // 移除文獻同樣：紀錄在、資料夾還在原位
    assert!(lib.remove_paper(&p).is_err());
    assert!(dir.path().join("papers").join("Alpha").is_dir());
    assert_eq!(change_lines(dir.path(), DEVICE).len(), 2);
}

#[test]
fn delete_group_writes_no_change_record() {
    let (dir, mut lib) = temp_library();
    let (_, gt) = lib.list_groups().unwrap();
    let (g, gt) = lib.create_group("Vision", &gt).unwrap();
    lib.delete_group(&g.id, &gt).unwrap();
    assert!(change_lines(dir.path(), DEVICE).is_empty());
    let (lf, _) =
        paperreader_core::jsonio::read_json::<Value>(&dir.path().join("library.json")).unwrap();
    assert_eq!(lf["deleted_groups"][0]["id"], json!(g.id));
}

#[test]
fn read_changes_tolerates_bad_lines() {
    let (dir, lib) = temp_library();
    let rec = |id: &str, t: &str, dev: &str, paper: &str, kind: &str| {
        json!({"id": id, "t": t, "device": dev, "paper": paper, "type": kind, "data": {}, "future": 1})
            .to_string()
    };
    let a = "01JCK5A7M3Q9X2V8B4N6R0T1YA";
    let b = "01JCK5A7M3Q9X2V8B4N6R0T1YB";
    let c = "01JCK5A7M3Q9X2V8B4N6R0T1YC";
    let text = [
        rec(b, "2026-11-20T08:00:00.000Z", DEVICE, "P1", "paper-removed"),
        "{not json".to_string(),
        rec(
            "bad id",
            "2026-11-20T08:00:00.000Z",
            DEVICE,
            "P1",
            "paper-removed",
        ),
        rec(a, "2026-11-19T08:00:00.000Z", DEVICE, "P2", "future-kind"),
        rec(
            c,
            "2026-11-20T08:00:00.000Z",
            DEVICE_B,
            "P1",
            "highlight-deleted",
        ),
        "{\"id\":\"01JCK5A7M3Q9X2V8B4N6R0T1YD\",\"t\":\"2026".to_string(),
    ]
    .join("\n");
    fs::create_dir_all(dir.path().join("logs")).unwrap();
    fs::write(changes_path(dir.path(), DEVICE), text).unwrap();

    let all = lib.read_changes(&ChangeFilter::default()).unwrap();
    let ids: Vec<&str> = all.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, [a, b, c], "依（t, device, id）排序，壞行略過");
    assert_eq!(all[0].kind, "future-kind", "不認得的種類照樣交回");
    assert_eq!(all[1].extra["future"], json!(1));
    let p1 = lib
        .read_changes(&ChangeFilter {
            paper: Some("P1".into()),
            kind: None,
        })
        .unwrap();
    assert_eq!(p1.len(), 2);
    let hd = lib
        .read_changes(&ChangeFilter {
            paper: None,
            kind: Some("highlight-deleted".into()),
        })
        .unwrap();
    assert_eq!(hd.len(), 1);
    assert_eq!(hd[0].id, c);
}

#[test]
fn reading_events_ignore_changes_file_and_check_does_not_flag_it() {
    let (dir, mut lib) = build_library();
    let events_before = lib.read_events(&EventFilter::default()).unwrap();
    let problems_before = lib.check(CheckLevel::Full).unwrap();
    let (p, _) = add_paper(&mut lib, 7, "Delta", None);
    lib.remove_paper(&p).unwrap();
    assert!(changes_path(dir.path(), DEVICE).is_file());
    let lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    assert_eq!(
        lib.read_events(&EventFilter::default()).unwrap(),
        events_before
    );
    assert_eq!(lib.check(CheckLevel::Full).unwrap(), problems_before);
}

#[test]
fn backup_includes_changes_file() {
    let (dir, mut lib) = build_library();
    let (p, _) = add_paper(&mut lib, 7, "Delta", None);
    lib.remove_paper(&p).unwrap();
    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("b.zip");
    lib.export_backup(&zip, &mut |_, _| {}).unwrap();
    let target = out.path().join("restored");
    paperreader_core::backup::restore_backup(&zip, &target, DEVICE_B).unwrap();
    let restored = LocalLibrary::open(&target, DEVICE_B).unwrap();
    assert_eq!(
        restored.read_changes(&ChangeFilter::default()).unwrap(),
        lib.read_changes(&ChangeFilter::default()).unwrap()
    );
    assert!(
        !lib.read_changes(&ChangeFilter::default())
            .unwrap()
            .is_empty()
    );
    drop(dir);
}

#[test]
fn overlay_update_stamps_segments() {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Zhao Paper", Some(10));
    let tok = lib.get_paper(&p).unwrap().1;
    let draft = OverlayDraft {
        source_material: m,
        provider: "anthropic".into(),
        model: "x".into(),
        source_lang: None,
        target_lang: "zh-TW".into(),
        segments: (0..3)
            .map(|i| SegmentDraft {
                page: 0,
                bbox: [1.0, 1.0, 2.0, 2.0],
                kind: "body".into(),
                size: None,
                source: format!("s{i}"),
            })
            .collect(),
    };
    let c = lib.create_overlay(&p, draft, &tok).unwrap();
    let path = dir
        .path()
        .join("papers/Zhao Paper/translation-overlay.json");
    // 沒有這兩個欄位的舊檔：拿掉之後加一個不認得的欄位
    let mut v: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for s in v["segments"].as_array_mut().unwrap() {
        let o = s.as_object_mut().unwrap();
        assert!(o.contains_key("modified"), "建立時寫出 null");
        o.remove("modified");
        o.remove("modified_by");
        o.insert("future".into(), json!("keep"));
    }
    fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let (_, t) = lib.read_overlay(&p, &c.material_id).unwrap();
    let up = |id: &str| SegmentUpdate {
        id: id.into(),
        status: "done".into(),
        target: Some("譯".into()),
        error: None,
        issues: vec![],
        service: None,
        by_user: false,
    };
    lib.update_overlay_segments(&p, &c.material_id, &[up("g0001"), up("g0003")], &t)
        .unwrap();
    let v: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let segs = v["segments"].as_array().unwrap();
    for i in [0, 2] {
        assert!(parse_utc_millis(segs[i]["modified"].as_str().unwrap()).is_some());
        assert_eq!(segs[i]["modified_by"], json!(DEVICE));
    }
    assert_eq!(segs[1]["modified"], Value::Null);
    assert_eq!(segs[1]["modified_by"], Value::Null);
    for s in segs {
        assert_eq!(s["future"], json!("keep"));
    }
}
