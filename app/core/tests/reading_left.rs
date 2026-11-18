//! 位置事件的水平位置（放大讀一欄之後重開，停在同一欄）。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::local::LocalLibrary;
use serde_json::{Value, json};

fn setup() -> (tempfile::TempDir, LocalLibrary, String, String) {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Paper", Some(10));
    (dir, lib, p, m)
}

fn reopen(dir: &tempfile::TempDir) -> LocalLibrary {
    LocalLibrary::open(dir.path(), DEVICE).unwrap()
}

// ---- 寫入 ----

#[test]
fn the_horizontal_position_is_written_into_the_event() {
    let (dir, mut lib, p, m) = setup();
    let e = lib
        .record_position_with_left(&p, &m, 3, 0.25, Some(2.4), Some(0.48))
        .unwrap()
        .expect("第一筆一定寫");
    assert_eq!(e.data["left"].as_f64(), Some(0.48));
    let line = std::fs::read_dir(dir.path().join("logs"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
        .collect::<String>();
    let v: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(v["data"]["left"].as_f64(), Some(0.48));
    assert_eq!(v["data"]["page"], 3);
}

#[test]
fn without_a_horizontal_position_the_event_has_no_left_field() {
    let (_dir, mut lib, p, m) = setup();
    let e = lib
        .record_position_with_left(&p, &m, 3, 0.25, None, None)
        .unwrap()
        .unwrap();
    assert!(
        !e.data.as_object().unwrap().contains_key("left"),
        "{}",
        e.data
    );
    // 原本的操作也一樣（沒有水平位置）
    let e2 = lib.record_position(&p, &m, 4, 0.25, None).unwrap().unwrap();
    assert!(
        !e2.data.as_object().unwrap().contains_key("left"),
        "{}",
        e2.data
    );
}

#[test]
fn an_out_of_range_horizontal_position_is_invalid_input_and_writes_nothing() {
    let (dir, mut lib, p, m) = setup();
    for bad in [-0.1, 1.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let before = logs_snapshot(dir.path());
        let r = lib.record_position_with_left(&p, &m, 3, 0.25, None, Some(bad));
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{bad}: {r:?}"
        );
        assert_eq!(logs_snapshot(dir.path()), before, "{bad}");
    }
}

#[test]
fn the_ends_of_the_range_are_accepted() {
    let (_dir, mut lib, p, m) = setup();
    let e = lib
        .record_position_with_left(&p, &m, 3, 0.25, Some(2.0), Some(0.0))
        .unwrap()
        .unwrap();
    assert_eq!(e.data["left"].as_f64(), Some(0.0));
    let e = lib
        .record_position_with_left(&p, &m, 4, 0.25, Some(2.0), Some(1.0))
        .unwrap()
        .unwrap();
    assert_eq!(e.data["left"].as_f64(), Some(1.0));
}

// ---- 不重複寫 ----

#[test]
fn duplicate_rules_include_the_horizontal_position() {
    let (_dir, mut lib, p, m) = setup();
    let z = Some(2.4);
    assert!(
        lib.record_position_with_left(&p, &m, 3, 0.25, z, Some(0.1))
            .unwrap()
            .is_some()
    );
    // 差 0.01：同一處，不寫
    assert!(
        lib.record_position_with_left(&p, &m, 3, 0.25, z, Some(0.11))
            .unwrap()
            .is_none()
    );
    // 只有水平位置變了（同一頁、同一高度，從左欄到右欄）：寫
    assert!(
        lib.record_position_with_left(&p, &m, 3, 0.25, z, Some(0.52))
            .unwrap()
            .is_some()
    );
    // 差 0.03：寫
    assert!(
        lib.record_position_with_left(&p, &m, 3, 0.25, z, Some(0.55))
            .unwrap()
            .is_some()
    );
    // 一邊知道、一邊不知道：不同，寫
    assert!(
        lib.record_position_with_left(&p, &m, 3, 0.25, z, None)
            .unwrap()
            .is_some()
    );
    // 兩邊都不知道：同一處，不寫
    assert!(
        lib.record_position_with_left(&p, &m, 3, 0.25, z, None)
            .unwrap()
            .is_none()
    );
    assert!(lib.record_position(&p, &m, 3, 0.25, z).unwrap().is_none());
    // 從不知道到知道：寫
    assert!(
        lib.record_position_with_left(&p, &m, 3, 0.25, z, Some(0.55))
            .unwrap()
            .is_some()
    );
}

#[test]
fn duplicate_check_with_the_horizontal_position_survives_reopening() {
    let (dir, mut lib, p, m) = setup();
    lib.record_position_with_left(&p, &m, 3, 0.25, Some(2.4), Some(0.5))
        .unwrap()
        .unwrap();
    drop(lib);
    let mut lib = reopen(&dir);
    assert!(
        lib.record_position_with_left(&p, &m, 3, 0.25, Some(2.4), Some(0.5))
            .unwrap()
            .is_none()
    );
    assert!(
        lib.record_position_with_left(&p, &m, 3, 0.25, Some(2.4), Some(0.1))
            .unwrap()
            .is_some()
    );
}

// ---- 讀取與閱讀位置 ----

#[test]
fn the_reading_position_carries_the_horizontal_position() {
    let (dir, mut lib, p, m) = setup();
    lib.record_position_with_left(&p, &m, 3, 0.25, Some(2.4), Some(0.48))
        .unwrap()
        .unwrap();
    let pos = lib.reading_progress(&p).unwrap().position.unwrap();
    assert_eq!((pos.page, pos.left), (3, Some(0.48)));
    // 關閉再開啟也一樣
    drop(lib);
    let lib = reopen(&dir);
    let prog = lib.reading_progress(&p).unwrap();
    assert_eq!(prog.position.unwrap().left, Some(0.48));
    assert_eq!(prog.device_positions[0].left, Some(0.48));
}

#[test]
fn old_events_without_left_have_an_unknown_horizontal_position() {
    let (dir, mut lib, p, m) = setup();
    lib.record_position(&p, &m, 3, 0.25, Some(1.0))
        .unwrap()
        .unwrap();
    assert_eq!(
        lib.reading_progress(&p).unwrap().position.unwrap().left,
        None
    );
    drop(lib);
    // 舊版應用寫的事件（`data` 沒有 left）
    write_log(
        dir.path(),
        DEVICE,
        "2026-01",
        &log_line(
            "01AAAAAAAAAAAAAAAAAAAAAAAA",
            "2026-01-05T08:00:00.000Z",
            DEVICE,
            &p,
            "position",
            json!({"material": m, "page": 9, "offset": 0.1, "zoom": null}),
        ),
    );
    let lib = reopen(&dir);
    // 最新的一筆仍是剛才寫的那一筆；舊檔的那一筆有效（算進最遠頁）
    let prog = lib.reading_progress(&p).unwrap();
    assert_eq!(prog.position.unwrap().left, None);
    assert_eq!(prog.furthest_pages.get(&m), Some(&9));
}

#[test]
fn a_bad_left_does_not_make_the_event_invalid() {
    for (i, bad) in [json!(2), json!(-1), json!("0.5"), Value::Null, json!(true)]
        .into_iter()
        .enumerate()
    {
        let (dir, lib, p, m) = setup();
        drop(lib);
        write_log(
            dir.path(),
            DEVICE_B,
            "2026-01",
            &log_line(
                &format!("01BBBBBBBBBBBBBBBBBBBBBBB{i}"),
                "2026-01-05T08:00:00.000Z",
                DEVICE_B,
                &p,
                "position",
                json!({"material": m, "page": 6, "offset": 0.5, "zoom": 2.0, "left": bad}),
            ),
        );
        let lib = reopen(&dir);
        let pos = lib
            .reading_progress(&p)
            .unwrap()
            .position
            .unwrap_or_else(|| panic!("left = {bad} 時事件仍要有效"));
        assert_eq!((pos.page, pos.left), (6, None), "left = {bad}");
    }
}

#[test]
fn an_integer_left_in_the_file_is_read() {
    let (dir, lib, p, m) = setup();
    drop(lib);
    write_log(
        dir.path(),
        DEVICE_B,
        "2026-01",
        &log_line(
            "01CCCCCCCCCCCCCCCCCCCCCCCC",
            "2026-01-05T08:00:00.000Z",
            DEVICE_B,
            &p,
            "position",
            json!({"material": m, "page": 6, "offset": 0.5, "zoom": 2.0, "left": 1}),
        ),
    );
    let lib = reopen(&dir);
    assert_eq!(
        lib.reading_progress(&p).unwrap().position.unwrap().left,
        Some(1.0)
    );
}
