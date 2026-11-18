//! 三種操作、驗證、不重複寫、時間只往前走。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::Event;
use paperreader_core::local::LocalLibrary;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

/// 開一個有一篇文獻（材料記有 10 頁）的文獻庫。
fn setup() -> (tempfile::TempDir, LocalLibrary, String, String) {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Paper", Some(10));
    (dir, lib, p, m)
}

/// 本裝置所有紀錄檔裡的每一行，解析成 JSON。
fn log_values(root: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    let dir = root.join("logs");
    if !dir.exists() {
        return out;
    }
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    for f in files {
        for l in fs::read_to_string(f).unwrap().lines() {
            out.push(serde_json::from_str(l).unwrap());
        }
    }
    out
}

fn reopen(dir: &tempfile::TempDir) -> LocalLibrary {
    LocalLibrary::open(dir.path(), DEVICE).unwrap()
}

// ---- 三種操作各寫出一行 ----

#[test]
fn record_open_writes_one_line_with_the_spec_fields() {
    let (dir, mut lib, p, m) = setup();
    let e = lib.record_open(&p, &m).unwrap();
    assert_eq!(
        (e.kind.as_str(), e.paper.as_str(), e.device.as_str()),
        ("open", p.as_str(), DEVICE)
    );
    let lines = log_values(dir.path());
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["type"], "open");
    assert_eq!(lines[0]["data"], json!({"material": m}));
    assert_eq!(lines[0]["id"], e.id.as_str());
    assert_eq!(lines[0]["t"], e.t.as_str());
}

#[test]
fn record_position_writes_one_line_with_the_spec_fields() {
    let (dir, mut lib, p, m) = setup();
    let e = lib
        .record_position(&p, &m, 7, 0.35, Some(1.0))
        .unwrap()
        .expect("第一筆一定寫");
    assert_eq!(e.kind, "position");
    let lines = log_values(dir.path());
    assert_eq!(lines.len(), 1);
    let d = &lines[0]["data"];
    assert_eq!(d["material"], m.as_str());
    assert!(d["page"].is_i64(), "page 必須是整數：{d}");
    assert_eq!(d["page"], 7);
    assert_eq!(d["offset"].as_f64(), Some(0.35));
    assert_eq!(d["zoom"].as_f64(), Some(1.0));
    // 縮放不知道時寫 null，欄位不省略
    let e2 = lib.record_position(&p, &m, 8, 0.0, None).unwrap().unwrap();
    assert_eq!(e2.data["zoom"], Value::Null);
    assert!(e2.data.as_object().unwrap().contains_key("zoom"));
}

#[test]
fn set_read_writes_one_line_with_the_spec_fields() {
    let (dir, mut lib, p, _m) = setup();
    let e = lib.set_read(&p, true).unwrap();
    assert_eq!(e.kind, "status");
    let e2 = lib.set_read(&p, false).unwrap();
    let lines = log_values(dir.path());
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["data"], json!({"read": true}));
    assert_eq!(lines[1]["data"], json!({"read": false}));
    assert_ne!(e.id, e2.id);
}

// ---- 錯誤：回報正確的原因，紀錄檔沒有多出任何一行 ----

fn assert_unchanged(
    dir: &tempfile::TempDir,
    before: &std::collections::BTreeMap<String, Option<Vec<u8>>>,
) {
    assert_eq!(&logs_snapshot(dir.path()), before, "紀錄檔不該有任何變動");
}

#[test]
fn missing_paper_is_not_found_and_writes_nothing() {
    let (dir, mut lib, _p, m) = setup();
    let before = logs_snapshot(dir.path());
    let ghost = "01ZZZZZZZZZZZZZZZZZZZZZZZZ";
    assert!(matches!(
        lib.record_open(ghost, &m),
        Err(LibError::NotFound { .. })
    ));
    assert!(matches!(
        lib.record_position(ghost, &m, 1, 0.0, None),
        Err(LibError::NotFound { .. })
    ));
    assert!(matches!(
        lib.set_read(ghost, true),
        Err(LibError::NotFound { .. })
    ));
    assert_unchanged(&dir, &before);
}

#[test]
fn paper_in_trash_is_not_found_and_writes_nothing() {
    let (dir, mut lib, p, m) = setup();
    lib.remove_paper(&p).unwrap();
    let before = logs_snapshot(dir.path());
    assert!(matches!(
        lib.record_open(&p, &m),
        Err(LibError::NotFound { .. })
    ));
    assert!(matches!(
        lib.record_position(&p, &m, 1, 0.0, None),
        Err(LibError::NotFound { .. })
    ));
    assert!(matches!(
        lib.set_read(&p, true),
        Err(LibError::NotFound { .. })
    ));
    assert_unchanged(&dir, &before);
}

#[test]
fn material_of_another_paper_is_invalid_input_and_writes_nothing() {
    let (dir, mut lib, p, _m) = setup();
    let (_p2, m2) = add_paper(&mut lib, 2, "Other", Some(10));
    let before = logs_snapshot(dir.path());
    for bad in [m2.as_str(), "no-such-material", ""] {
        assert!(
            matches!(lib.record_open(&p, bad), Err(LibError::InvalidInput { .. })),
            "{bad:?}"
        );
        assert!(
            matches!(
                lib.record_position(&p, bad, 1, 0.0, None),
                Err(LibError::InvalidInput { .. })
            ),
            "{bad:?}"
        );
    }
    assert_unchanged(&dir, &before);
}

#[test]
fn out_of_range_fields_are_invalid_input_and_write_nothing() {
    let (dir, mut lib, p, m) = setup();
    let before = logs_snapshot(dir.path());
    let cases: Vec<(&str, i64, f64, Option<f64>)> = vec![
        ("頁碼 0", 0, 0.0, None),
        ("頁碼負數", -3, 0.0, None),
        ("頁碼 100001", 100_001, 0.0, None),
        ("頁碼大於材料的頁數 10", 11, 0.0, None),
        ("頁內位置 -0.1", 1, -0.1, None),
        ("頁內位置 1.1", 1, 1.1, None),
        ("頁內位置不是數字", 1, f64::NAN, None),
        ("縮放 0", 1, 0.0, Some(0.0)),
        ("縮放負數", 1, 0.0, Some(-1.0)),
        ("縮放不是數字", 1, 0.0, Some(f64::NAN)),
        ("縮放無限大", 1, 0.0, Some(f64::INFINITY)),
    ];
    for (name, page, offset, zoom) in cases {
        let r = lib.record_position(&p, &m, page, offset, zoom);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{name}：{r:?}"
        );
    }
    assert_unchanged(&dir, &before);
}

#[test]
fn boundary_values_are_accepted() {
    let (_dir, mut lib, p, m) = setup();
    assert!(lib.record_position(&p, &m, 1, 0.0, None).unwrap().is_some());
    assert!(
        lib.record_position(&p, &m, 10, 1.0, Some(0.001))
            .unwrap()
            .is_some()
    );
}

fn read_only_library() -> (tempfile::TempDir, String, String) {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Paper", Some(10));
    drop(lib);
    let lj = dir.path().join("library.json");
    let text = fs::read_to_string(&lj)
        .unwrap()
        .replace("\"format\": 1", "\"format\": 2");
    fs::write(&lj, text).unwrap();
    (dir, p, m)
}

#[test]
fn read_only_library_refuses_all_three_and_writes_nothing() {
    let (dir, p, m) = read_only_library();
    let mut lib = reopen(&dir);
    assert!(lib.is_read_only());
    let before = snapshot(dir.path());
    assert!(matches!(lib.record_open(&p, &m), Err(LibError::ReadOnly)));
    assert!(matches!(
        lib.record_position(&p, &m, 1, 0.0, None),
        Err(LibError::ReadOnly)
    ));
    assert!(matches!(lib.set_read(&p, true), Err(LibError::ReadOnly)));
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn page_above_the_old_limit_is_fine_when_the_material_has_no_page_count() {
    let (_dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Paper", None);
    assert!(
        lib.record_position(&p, &m, 5000, 0.0, None)
            .unwrap()
            .is_some()
    );
    assert!(matches!(
        lib.record_position(&p, &m, 100_001, 0.0, None),
        Err(LibError::InvalidInput { .. })
    ));
}

#[test]
fn paper_flagged_as_needing_a_newer_app_can_get_events_without_touching_its_description() {
    let (dir, lib, p, m) = setup();
    let folder = lib.list_papers()[0].folder.clone();
    drop(lib);
    let pj = dir.path().join("papers").join(&folder).join("paper.json");
    let text = fs::read_to_string(&pj)
        .unwrap()
        .replacen("\"format\": 1", "\"format\": 2", 1);
    fs::write(&pj, &text).unwrap();
    let mut lib = reopen(&dir);
    assert!(lib.list_papers()[0].needs_newer_app);

    assert!(lib.record_open(&p, &m).is_ok());
    assert!(lib.record_position(&p, &m, 3, 0.5, None).unwrap().is_some());
    assert!(lib.set_read(&p, true).is_ok());
    assert_eq!(fs::read_to_string(&pj).unwrap(), text, "描述檔原封不動");
    assert_eq!(log_values(dir.path()).len(), 3);
}

// ---- 不重複寫 ----

/// `(第一筆, 第二筆, 第二筆是否寫出)`；每筆是（頁碼、頁內位置、縮放）。
type Pos = (i64, f64, Option<f64>);

#[test]
fn duplicate_position_rules() {
    let cases: Vec<(&str, Pos, Pos, bool)> = vec![
        (
            "完全相同",
            (7, 0.35, Some(1.0)),
            (7, 0.35, Some(1.0)),
            false,
        ),
        (
            "頁內位置差 0.01",
            (7, 0.35, Some(1.0)),
            (7, 0.36, Some(1.0)),
            false,
        ),
        (
            "頁內位置差 0.03",
            (7, 0.35, Some(1.0)),
            (7, 0.38, Some(1.0)),
            true,
        ),
        ("換頁", (7, 0.35, Some(1.0)), (8, 0.35, Some(1.0)), true),
        (
            "縮放 1.0 與 1.5",
            (7, 0.35, Some(1.0)),
            (7, 0.35, Some(1.5)),
            true,
        ),
        (
            "縮放差 0.0005",
            (7, 0.35, Some(1.0)),
            (7, 0.35, Some(1.0005)),
            false,
        ),
        ("兩邊縮放都是 null", (7, 0.35, None), (7, 0.35, None), false),
        (
            "一邊 null 一邊 1.0",
            (7, 0.35, None),
            (7, 0.35, Some(1.0)),
            true,
        ),
        (
            "一邊 1.0 一邊 null",
            (7, 0.35, Some(1.0)),
            (7, 0.35, None),
            true,
        ),
    ];
    for (name, a, b, expect_written) in cases {
        let (dir, mut lib, p, m) = setup();
        assert!(
            lib.record_position(&p, &m, a.0, a.1, a.2)
                .unwrap()
                .is_some(),
            "{name}：第一筆"
        );
        let second = lib.record_position(&p, &m, b.0, b.1, b.2).unwrap();
        assert_eq!(second.is_some(), expect_written, "{name}");
        let want_lines = if expect_written { 2 } else { 1 };
        assert_eq!(
            log_values(dir.path()).len(),
            want_lines,
            "{name}：紀錄檔行數"
        );
    }
}

#[test]
fn duplicate_is_judged_against_the_last_written_one_not_the_last_attempt() {
    let (_dir, mut lib, p, m) = setup();
    assert!(
        lib.record_position(&p, &m, 7, 0.30, None)
            .unwrap()
            .is_some()
    );
    // 每次只差 0.015，都和上一筆「寫出的」比：0.315 不寫、0.33 與 0.30 差 0.03 → 寫
    assert!(
        lib.record_position(&p, &m, 7, 0.315, None)
            .unwrap()
            .is_none()
    );
    assert!(
        lib.record_position(&p, &m, 7, 0.33, None)
            .unwrap()
            .is_some()
    );
}

#[test]
fn duplicates_are_judged_per_material() {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Paper", Some(10));
    // 用手寫的方式替同一篇加第二項材料
    let folder = lib.list_papers()[0].folder.clone();
    drop(lib);
    let pj = dir.path().join("papers").join(&folder).join("paper.json");
    let mut v: Value = serde_json::from_str(&fs::read_to_string(&pj).unwrap()).unwrap();
    let mut second = v["materials"][0].clone();
    second["id"] = json!("01MMMMMMMMMMMMMMMMMMMMMMMM");
    second["role"] = json!("translation");
    v["materials"].as_array_mut().unwrap().push(second);
    fs::write(&pj, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let mut lib = reopen(&dir);
    assert!(
        lib.record_position(&p, &m, 7, 0.35, None)
            .unwrap()
            .is_some()
    );
    assert!(
        lib.record_position(&p, "01MMMMMMMMMMMMMMMMMMMMMMMM", 7, 0.35, None)
            .unwrap()
            .is_some(),
        "另一項材料的位置相同也要寫"
    );
}

#[test]
fn other_devices_last_position_does_not_matter() {
    let (dir, mut lib, p, m) = setup();
    // 別台裝置剛寫了完全相同的位置
    let other = log_line(
        "01OTHER00000000000000000A",
        &paperreader_core::ids::now_utc(),
        "01OTHERDEVICE00000000000A",
        &p,
        "position",
        json!({"material": m, "page": 7, "offset": 0.35, "zoom": 1.0}),
    );
    write_log(
        dir.path(),
        "01OTHERDEVICE00000000000A",
        &paperreader_core::ids::now_utc()[..7],
        &other,
    );
    lib.rescan().unwrap();
    assert!(
        lib.record_position(&p, &m, 7, 0.35, Some(1.0))
            .unwrap()
            .is_some()
    );
}

#[test]
fn duplicate_check_survives_reopening() {
    let (dir, mut lib, p, m) = setup();
    assert!(
        lib.record_position(&p, &m, 7, 0.35, Some(1.0))
            .unwrap()
            .is_some()
    );
    drop(lib);
    let mut lib = reopen(&dir);
    assert!(
        lib.record_position(&p, &m, 7, 0.35, Some(1.0))
            .unwrap()
            .is_none()
    );
    assert_eq!(log_values(dir.path()).len(), 1);
}

#[test]
fn duplicate_check_finds_the_last_position_in_last_months_file() {
    let (dir, lib, p, m) = setup();
    drop(lib);
    let old = log_line(
        "01OLD0000000000000000000A",
        "2026-10-15T09:00:00.000Z",
        DEVICE,
        &p,
        "position",
        json!({"material": m, "page": 7, "offset": 0.35, "zoom": 1.0}),
    );
    write_log(dir.path(), DEVICE, "2026-10", &old);
    let mut lib = reopen(&dir);
    assert!(
        lib.record_position(&p, &m, 7, 0.35, Some(1.0))
            .unwrap()
            .is_none()
    );
    assert!(
        lib.record_position(&p, &m, 8, 0.35, Some(1.0))
            .unwrap()
            .is_some()
    );
}

// ---- 時間只往前走 ----

#[test]
fn event_time_continues_after_a_time_in_the_future() {
    let (dir, lib, p, m) = setup();
    drop(lib);
    // 這台裝置的紀錄檔最後一筆的時間在遙遠的未來（手機時間後來被調回來）
    let future = log_line(
        "01FUT0000000000000000000A",
        "2099-01-01T00:00:00.000Z",
        DEVICE,
        &p,
        "open",
        json!({"material": m}),
    );
    write_log(dir.path(), DEVICE, "2099-01", &future);
    let mut lib = reopen(&dir);
    let e = lib.record_open(&p, &m).unwrap();
    assert_eq!(e.t, "2099-01-01T00:00:00.001Z");
    // 事件寫進它自己的時間所屬的月份檔
    assert!(
        dir.path()
            .join("logs")
            .join(format!("reading-{DEVICE}-2099-01.jsonl"))
            .exists()
    );
    let e2 = lib.set_read(&p, true).unwrap();
    assert_eq!(e2.t, "2099-01-01T00:00:00.002Z");
}

#[test]
fn ten_quick_events_have_strictly_increasing_times_also_after_reopening() {
    let (dir, mut lib, p, m) = setup();
    let mut times: Vec<String> = (0..10)
        .map(|_| lib.record_open(&p, &m).unwrap().t)
        .collect();
    drop(lib);
    let mut lib = reopen(&dir);
    times.push(lib.record_open(&p, &m).unwrap().t);
    for w in times.windows(2) {
        assert!(w[0] < w[1], "{} 應該早於 {}", w[0], w[1]);
    }
}

#[test]
fn ten_events_after_a_last_event_a_few_seconds_ahead_continue_one_millisecond_at_a_time() {
    // 本裝置紀錄檔裡有一筆比現在晚幾秒的事件：之後連寫十筆，一定走「上一筆加 1 毫秒」的路徑
    use paperreader_core::ids::{format_utc_millis, now_utc_millis, parse_utc_millis};
    let (dir, lib, p, m) = setup();
    drop(lib);
    let ahead_ms = now_utc_millis() + 5_000;
    let ahead = format_utc_millis(ahead_ms).unwrap();
    let text = log_line(
        "01AHEAD0000000000000000AA",
        &ahead,
        DEVICE,
        &p,
        "open",
        json!({"material": m}),
    );
    write_log(dir.path(), DEVICE, &ahead[..7], &text);
    let mut lib = reopen(&dir);
    for i in 1..=10 {
        let e = lib.record_open(&p, &m).unwrap();
        assert_eq!(
            parse_utc_millis(&e.t),
            Some(ahead_ms + i),
            "第 {i} 筆：{} 應該是 {ahead} 加 {i} 毫秒",
            e.t
        );
    }
}

#[test]
fn set_read_true_then_false_right_away_always_ends_unread_marker() {
    // 連續兩次呼叫：後一筆的時間一定比前一筆晚，取消一定生效
    let (_dir, mut lib, p, _m) = setup();
    for _ in 0..20 {
        let a = lib.set_read(&p, true).unwrap();
        let b = lib.set_read(&p, false).unwrap();
        assert!(a.t < b.t);
    }
}

// ---- 補充 ----

#[test]
fn a_last_time_at_the_end_of_year_9999_gives_an_error_instead_of_a_crash() {
    // 上一筆的時間加 1 毫秒會超出可表示的範圍時，不可以崩潰
    let (dir, lib, p, m) = setup();
    drop(lib);
    // 晚於界線的事件是無效的，不算「上一筆」：照常用現在的時間寫
    let beyond = log_line(
        "01END0000000000000000000A",
        "9999-12-31T23:59:59.999Z",
        DEVICE,
        &p,
        "open",
        json!({"material": m}),
    );
    write_log(dir.path(), DEVICE, "9999-12", &beyond);
    let mut lib = reopen(&dir);
    let e = lib.record_open(&p, &m).unwrap();
    assert!(e.t.as_str() < "9999", "{}", e.t);

    // 剛好在界線上的事件有效，但再往後一毫秒就超出界線：交回錯誤，什麼也不寫
    let (dir2, lib2, p2, m2) = setup();
    drop(lib2);
    let edge = log_line(
        "01EDGE000000000000000000A",
        "9999-12-31T23:59:59.998Z",
        DEVICE,
        &p2,
        "open",
        json!({"material": m2}),
    );
    write_log(dir2.path(), DEVICE, "9999-12", &edge);
    let mut lib2 = reopen(&dir2);
    let before = logs_snapshot(dir2.path());
    assert!(matches!(
        lib2.record_open(&p2, &m2),
        Err(LibError::InvalidInput { .. })
    ));
    assert!(matches!(
        lib2.set_read(&p2, true),
        Err(LibError::InvalidInput { .. })
    ));
    assert!(matches!(
        lib2.record_position(&p2, &m2, 1, 0.0, None),
        Err(LibError::InvalidInput { .. })
    ));
    assert_unchanged(&dir2, &before);
}

#[test]
fn an_event_that_could_not_be_read_back_is_refused() {
    // 描述檔裡的材料代號是空字串時，寫進去的事件讀回來是無效的，所以不寫
    let (dir, lib, p, _m) = setup();
    let folder = lib.list_papers()[0].folder.clone();
    drop(lib);
    let pj = dir.path().join("papers").join(&folder).join("paper.json");
    let mut v: Value = serde_json::from_str(&fs::read_to_string(&pj).unwrap()).unwrap();
    v["materials"][0]["id"] = json!("");
    fs::write(&pj, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let mut lib = reopen(&dir);
    let before = logs_snapshot(dir.path());
    assert!(matches!(
        lib.record_open(&p, ""),
        Err(LibError::InvalidInput { .. })
    ));
    assert!(matches!(
        lib.record_position(&p, "", 1, 0.0, None),
        Err(LibError::InvalidInput { .. })
    ));
    assert_unchanged(&dir, &before);
    // 不需要材料的事件照樣可以寫，而且讀回來有效
    let e = lib.set_read(&p, true).unwrap();
    let back = lib.read_events(&Default::default()).unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].id, e.id);
}

// ---- 對外的接口 ----

#[test]
fn the_three_operations_work_through_the_trait_object() {
    let (_dir, mut lib, p, m) = setup();
    let store = &mut lib;
    let e: Event = store.record_open(&p, &m).unwrap();
    assert_eq!(e.kind, "open");
    assert!(
        store
            .record_position(&p, &m, 2, 0.1, None)
            .unwrap()
            .is_some()
    );
    assert_eq!(store.set_read(&p, true).unwrap().kind, "status");
    assert_eq!(store.read_events(&Default::default()).unwrap().len(), 3);
}
