//! 由事件算出閱讀狀態、閱讀位置、最遠頁、繼續閱讀。

mod common;

use common::*;
use paperreader_core::local::LocalLibrary;
use paperreader_core::progress::{ReadStatus, ReadingProgress};
use serde_json::{Value, json};
use std::fs;

const OTHER: &str = "01OTHERDEVICE00000000000A";
const OTHER2: &str = "01OTHERDEVICE00000000000B";

fn eid(n: u32) -> String {
    format!("01EV{n:022}")
}

fn open_ev(n: u32, t: &str, dev: &str, paper: &str, material: &str) -> String {
    log_line(
        &eid(n),
        t,
        dev,
        paper,
        "open",
        json!({"material": material}),
    )
}

fn pos_ev(
    n: u32,
    t: &str,
    dev: &str,
    paper: &str,
    material: &str,
    page: i64,
    offset: f64,
) -> String {
    log_line(
        &eid(n),
        t,
        dev,
        paper,
        "position",
        json!({"material": material, "page": page, "offset": offset, "zoom": 1.0}),
    )
}

fn read_ev(n: u32, t: &str, dev: &str, paper: &str, read: bool) -> String {
    log_line(&eid(n), t, dev, paper, "status", json!({"read": read}))
}

fn setup(pages: Option<u32>) -> (tempfile::TempDir, LocalLibrary, String, String) {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Paper", pages);
    (dir, lib, p, m)
}

fn reopen(dir: &tempfile::TempDir) -> LocalLibrary {
    LocalLibrary::open(dir.path(), DEVICE).unwrap()
}

fn progress(lib: &LocalLibrary, p: &str) -> ReadingProgress {
    lib.reading_progress(p).unwrap()
}

// ---- 狀態 ----

#[test]
fn no_events_means_unread_and_no_position() {
    let (_dir, lib, p, _m) = setup(Some(10));
    let pr = progress(&lib, &p);
    assert_eq!(pr.status, ReadStatus::Unread);
    assert_eq!(pr.read_at, None);
    assert!(pr.position.is_none());
    assert!(pr.device_positions.is_empty());
    assert!(pr.furthest_pages.is_empty());
    assert_eq!(pr.last_read_at, None);
    assert!(lib.continue_reading(10).is_empty());
}

#[test]
fn only_open_means_reading_without_position_and_not_in_continue() {
    let (_dir, mut lib, p, m) = setup(Some(10));
    let e = lib.record_open(&p, &m).unwrap();
    let pr = progress(&lib, &p);
    assert_eq!(pr.status, ReadStatus::Reading);
    assert!(pr.position.is_none());
    assert_eq!(pr.last_read_at.as_deref(), Some(e.t.as_str()));
    assert!(lib.continue_reading(10).is_empty());
}

#[test]
fn position_means_reading_with_latest_position_and_in_continue() {
    let (_dir, mut lib, p, m) = setup(Some(10));
    lib.record_position(&p, &m, 3, 0.2, Some(1.5)).unwrap();
    let e = lib.record_position(&p, &m, 4, 0.5, None).unwrap().unwrap();
    let pr = progress(&lib, &p);
    assert_eq!(pr.status, ReadStatus::Reading);
    let pos = pr.position.unwrap();
    assert_eq!(
        (pos.material.as_str(), pos.page, pos.offset, pos.zoom),
        (m.as_str(), 4, 0.5, None)
    );
    assert_eq!(
        (pos.t.as_str(), pos.device.as_str()),
        (e.t.as_str(), DEVICE)
    );
    assert_eq!(pr.last_read_at.as_deref(), Some(e.t.as_str()));
    assert_eq!(lib.continue_reading(10), vec![p]);
}

#[test]
fn marked_read_is_read_with_its_time_and_not_in_continue_and_position_still_updates() {
    let (_dir, mut lib, p, m) = setup(Some(10));
    lib.record_position(&p, &m, 3, 0.2, None).unwrap();
    let mark = lib.set_read(&p, true).unwrap();
    let pr = progress(&lib, &p);
    assert_eq!(pr.status, ReadStatus::Read);
    assert_eq!(pr.read_at.as_deref(), Some(mark.t.as_str()));
    assert!(lib.continue_reading(10).is_empty());

    lib.record_open(&p, &m).unwrap();
    lib.record_position(&p, &m, 6, 0.1, None).unwrap();
    let pr = progress(&lib, &p);
    assert_eq!(
        pr.status,
        ReadStatus::Read,
        "標成已讀之後再打開、再翻頁，仍是已讀"
    );
    assert_eq!(pr.read_at.as_deref(), Some(mark.t.as_str()));
    assert_eq!(pr.position.unwrap().page, 6, "閱讀位置照樣更新");
}

#[test]
fn marking_read_twice_uses_the_second_time() {
    let (_dir, mut lib, p, _m) = setup(Some(10));
    let first = lib.set_read(&p, true).unwrap();
    let second = lib.set_read(&p, true).unwrap();
    assert!(first.t < second.t);
    assert_eq!(
        progress(&lib, &p).read_at.as_deref(),
        Some(second.t.as_str())
    );
}

#[test]
fn unmarking_goes_back_to_what_the_other_events_say() {
    let (_dir, mut lib, p, m) = setup(Some(10));
    // 打開過的文獻取消標記 → 讀到一半
    lib.record_open(&p, &m).unwrap();
    lib.set_read(&p, true).unwrap();
    lib.set_read(&p, false).unwrap();
    let pr = progress(&lib, &p);
    assert_eq!(pr.status, ReadStatus::Reading);
    assert_eq!(pr.read_at, None);

    // 從沒打開過的文獻標成已讀再取消 → 未讀
    let (p2, _m2) = add_paper(&mut lib, 2, "Never opened", Some(5));
    lib.set_read(&p2, true).unwrap();
    assert_eq!(progress(&lib, &p2).status, ReadStatus::Read);
    lib.set_read(&p2, false).unwrap();
    assert_eq!(progress(&lib, &p2).status, ReadStatus::Unread);
}

#[test]
fn mark_then_unmark_in_two_quick_calls_always_ends_unmarked() {
    let (_dir, mut lib, p, _m) = setup(Some(10));
    for _ in 0..20 {
        lib.set_read(&p, true).unwrap();
        lib.set_read(&p, false).unwrap();
        assert_eq!(progress(&lib, &p).status, ReadStatus::Unread);
    }
}

#[test]
fn going_back_a_page_moves_the_position_but_not_the_furthest_page() {
    let (_dir, mut lib, p, m) = setup(Some(20));
    lib.record_position(&p, &m, 9, 0.0, None).unwrap();
    lib.record_position(&p, &m, 3, 0.0, None).unwrap();
    let pr = progress(&lib, &p);
    assert_eq!(pr.position.unwrap().page, 3);
    assert_eq!(pr.furthest_pages.get(&m), Some(&9));
}

// ---- 兩台裝置 ----

#[test]
fn two_devices_interleaved_logs() {
    let (dir, lib, p, m) = setup(Some(20));
    drop(lib);
    // 本裝置：t1 第 2 頁、t3 第 4 頁；別台：t2 第 8 頁、t4 第 9 頁；更晚的別台 t5 第 1 頁
    write_log(
        dir.path(),
        DEVICE,
        "2026-11",
        &(pos_ev(1, "2026-11-13T09:00:01.000Z", DEVICE, &p, &m, 2, 0.0)
            + &pos_ev(3, "2026-11-13T09:00:03.000Z", DEVICE, &p, &m, 4, 0.0)),
    );
    write_log(
        dir.path(),
        OTHER,
        "2026-11",
        &(pos_ev(2, "2026-11-13T09:00:02.000Z", OTHER, &p, &m, 8, 0.0)
            + &pos_ev(4, "2026-11-13T09:00:04.000Z", OTHER, &p, &m, 9, 0.0)),
    );
    let lib = reopen(&dir);
    let pr = progress(&lib, &p);
    let pos = pr.position.unwrap();
    assert_eq!(
        (pos.page, pos.device.as_str()),
        (9, OTHER),
        "閱讀位置是時間較晚的那一筆"
    );
    assert_eq!(pr.device_positions.len(), 2, "各裝置各有一筆");
    let mine = pr
        .device_positions
        .iter()
        .find(|d| d.device == DEVICE)
        .unwrap();
    let theirs = pr
        .device_positions
        .iter()
        .find(|d| d.device == OTHER)
        .unwrap();
    assert_eq!((mine.page, theirs.page), (4, 9));
    assert_eq!(pr.furthest_pages.get(&m), Some(&9));
}

#[test]
fn same_time_on_two_devices_prefers_the_larger_device_code() {
    let (dir, lib, p, m) = setup(Some(20));
    drop(lib);
    let t = "2026-11-13T09:00:00.000Z";
    write_log(
        dir.path(),
        OTHER,
        "2026-11",
        &pos_ev(1, t, OTHER, &p, &m, 5, 0.0),
    );
    write_log(
        dir.path(),
        OTHER2,
        "2026-11",
        &pos_ev(2, t, OTHER2, &p, &m, 6, 0.0),
    );
    let lib = reopen(&dir);
    let pos = progress(&lib, &p).position.unwrap();
    assert_eq!((pos.page, pos.device.as_str()), (6, OTHER2));
}

// ---- 無效事件 ----

#[test]
fn invalid_events_are_skipped_and_the_file_is_untouched() {
    let (dir, lib, p, m) = setup(Some(20));
    drop(lib);
    let good_t = "2026-11-13T09:00:00.000Z";
    let bad: Vec<(&str, String)> = vec![
        (
            "data 缺欄位",
            log_line(
                &eid(1),
                good_t,
                OTHER,
                &p,
                "position",
                json!({"material": m, "page": 3, "zoom": 1.0}),
            ),
        ),
        (
            "page 是 7.0",
            format!(
                r#"{{"id":"{}","t":"{good_t}","device":"{OTHER}","paper":"{p}","type":"position","data":{{"material":"{m}","page":7.0,"offset":0.1,"zoom":1.0}}}}"#,
                eid(2)
            ) + "\n",
        ),
        (
            "page 是字串",
            log_line(
                &eid(3),
                good_t,
                OTHER,
                &p,
                "position",
                json!({"material": m, "page": "7", "offset": 0.1, "zoom": 1.0}),
            ),
        ),
        (
            "offset 是 2",
            log_line(
                &eid(4),
                good_t,
                OTHER,
                &p,
                "position",
                json!({"material": m, "page": 7, "offset": 2, "zoom": 1.0}),
            ),
        ),
        (
            "zoom 是 0",
            log_line(
                &eid(5),
                good_t,
                OTHER,
                &p,
                "position",
                json!({"material": m, "page": 7, "offset": 0.1, "zoom": 0}),
            ),
        ),
        (
            "缺 zoom 欄位",
            log_line(
                &eid(6),
                good_t,
                OTHER,
                &p,
                "position",
                json!({"material": m, "page": 7, "offset": 0.1}),
            ),
        ),
        (
            "t 少了毫秒",
            log_line(
                &eid(7),
                "2026-11-13T09:00:00Z",
                OTHER,
                &p,
                "open",
                json!({"material": m}),
            ),
        ),
        (
            "t 不是時間",
            log_line(
                &eid(8),
                "yesterday",
                OTHER,
                &p,
                "open",
                json!({"material": m}),
            ),
        ),
        (
            "t 的月份是 13",
            log_line(
                &eid(9),
                "2026-13-13T09:00:00.000Z",
                OTHER,
                &p,
                "open",
                json!({"material": m}),
            ),
        ),
        (
            "不認得的 type",
            log_line(&eid(10), good_t, OTHER, &p, "close", json!({})),
        ),
        (
            "device 是空字串",
            log_line(&eid(11), good_t, "", &p, "open", json!({"material": m})),
        ),
        (
            "paper 是空字串",
            log_line(&eid(12), good_t, OTHER, "", "open", json!({"material": m})),
        ),
        (
            "id 是空字串",
            log_line("", good_t, OTHER, &p, "open", json!({"material": m})),
        ),
        (
            "open 缺 material",
            log_line(&eid(13), good_t, OTHER, &p, "open", json!({})),
        ),
        (
            "open 的 material 是空字串",
            log_line(&eid(14), good_t, OTHER, &p, "open", json!({"material": ""})),
        ),
        (
            "status 的 read 不是布林",
            log_line(
                &eid(15),
                good_t,
                OTHER,
                &p,
                "status",
                json!({"read": "yes"}),
            ),
        ),
        (
            "status 缺 read",
            log_line(&eid(16), good_t, OTHER, &p, "status", json!({})),
        ),
        (
            "page 是 0",
            log_line(
                &eid(17),
                good_t,
                OTHER,
                &p,
                "position",
                json!({"material": m, "page": 0, "offset": 0.1, "zoom": 1.0}),
            ),
        ),
        (
            "page 是 100001",
            log_line(
                &eid(18),
                good_t,
                OTHER,
                &p,
                "position",
                json!({"material": m, "page": 100001, "offset": 0.1, "zoom": 1.0}),
            ),
        ),
    ];
    for (name, text) in &bad {
        let f = write_log(dir.path(), OTHER, "2026-11", text);
        let before = fs::read(&f).unwrap();
        let lib = reopen(&dir);
        let pr = progress(&lib, &p);
        assert_eq!(
            pr.status,
            ReadStatus::Unread,
            "{name}：無效事件不該影響狀態"
        );
        assert!(pr.position.is_none(), "{name}");
        assert_eq!(fs::read(&f).unwrap(), before, "{name}：檔案內容不變");
    }
    // 一起放進同一個檔案，再加一筆有效的：其餘照算
    let mut all: String = bad.iter().map(|(_, t)| t.clone()).collect();
    all.push_str(&pos_ev(
        99,
        "2026-11-13T09:05:00.000Z",
        OTHER,
        &p,
        &m,
        5,
        0.25,
    ));
    let f = write_log(dir.path(), OTHER, "2026-11", &all);
    let before = fs::read(&f).unwrap();
    let lib = reopen(&dir);
    let pr = progress(&lib, &p);
    assert_eq!(pr.status, ReadStatus::Reading);
    assert_eq!(pr.position.unwrap().page, 5);
    assert_eq!(fs::read(&f).unwrap(), before);
}

#[test]
fn integer_offsets_zero_and_one_are_valid() {
    let (dir, lib, p, m) = setup(Some(20));
    drop(lib);
    let text = log_line(
        &eid(1),
        "2026-11-13T09:00:00.000Z",
        OTHER,
        &p,
        "position",
        json!({"material": m, "page": 2, "offset": 1, "zoom": null}),
    ) + &log_line(
        &eid(2),
        "2026-11-13T09:00:01.000Z",
        OTHER,
        &p,
        "position",
        json!({"material": m, "page": 3, "offset": 0, "zoom": 2}),
    );
    write_log(dir.path(), OTHER, "2026-11", &text);
    let lib = reopen(&dir);
    let pos = progress(&lib, &p).position.unwrap();
    assert_eq!((pos.page, pos.offset, pos.zoom), (3, 0.0, Some(2.0)));
    let first = progress(&lib, &p).device_positions;
    assert_eq!(first.len(), 1);
}

#[test]
fn an_event_with_integer_offset_one_alone_is_the_reading_position() {
    // 只有 offset 寫成整數 1 的一筆：它若被當成無效，就沒有閱讀位置
    let (dir, lib, p, m) = setup(Some(20));
    drop(lib);
    let text = log_line(
        &eid(1),
        "2026-11-13T09:00:00.000Z",
        OTHER,
        &p,
        "position",
        json!({"material": m, "page": 2, "offset": 1, "zoom": null}),
    );
    write_log(dir.path(), OTHER, "2026-11", &text);
    let lib = reopen(&dir);
    let pr = progress(&lib, &p);
    let pos = pr.position.expect("offset 為整數 1 的事件有效");
    assert_eq!((pos.page, pos.zoom), (2, None));
    assert_eq!(pos.offset, 1.0);
    assert_eq!(pr.status, ReadStatus::Reading);
    assert_eq!(lib.continue_reading(5), vec![p]);
}

// ---- 補充 ----

#[test]
fn an_unreadable_log_file_is_skipped_everywhere_and_read_again_once_it_changes() {
    // 名稱符合的資料夾讀不了，開啟、重新掃描、寫事件、讀事件都不受影響
    let (dir, lib, p, m) = setup(Some(10));
    drop(lib);
    let bad = dir.path().join("logs").join("reading-foo.jsonl");
    fs::create_dir_all(&bad).unwrap();
    let mut lib = reopen(&dir);
    lib.rescan().unwrap();
    assert!(lib.read_events(&Default::default()).unwrap().is_empty());
    let e = lib.record_open(&p, &m).unwrap();
    assert_eq!(lib.read_events(&Default::default()).unwrap().len(), 1);
    assert_eq!(progress(&lib, &p).status, ReadStatus::Reading);

    // 它變成讀得了的檔案之後，下一次重新掃描就讀進來
    fs::remove_dir(&bad).unwrap();
    let text = pos_ev(1, "2026-11-13T09:00:00.000Z", OTHER, &p, &m, 4, 0.5);
    fs::write(&bad, text).unwrap();
    lib.rescan().unwrap();
    let pr = progress(&lib, &p);
    assert_eq!(pr.position.unwrap().page, 4);
    let all = lib.read_events(&Default::default()).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|x| x.id == e.id));
}

#[test]
fn an_event_later_than_the_representable_limit_is_invalid() {
    // 晚於 9999-12-31T23:59:59.998Z 的事件算無效
    let (dir, lib, p, m) = setup(Some(10));
    drop(lib);
    let text = open_ev(1, "9999-12-31T23:59:59.999Z", DEVICE, &p, &m);
    write_log(dir.path(), DEVICE, "9999-12", &text);
    let lib = reopen(&dir);
    let pr = progress(&lib, &p);
    assert_eq!(pr.status, ReadStatus::Unread);
    assert_eq!(pr.last_read_at, None);
    // 剛好在界線上的事件有效
    let (dir2, lib2, p2, m2) = setup(Some(10));
    drop(lib2);
    write_log(
        dir2.path(),
        DEVICE,
        "9999-12",
        &open_ev(1, "9999-12-31T23:59:59.998Z", DEVICE, &p2, &m2),
    );
    let lib2 = reopen(&dir2);
    let pr2 = progress(&lib2, &p2);
    assert_eq!(pr2.status, ReadStatus::Reading);
    assert_eq!(
        pr2.last_read_at.as_deref(),
        Some("9999-12-31T23:59:59.998Z")
    );
}

#[test]
fn events_for_a_paper_that_does_not_exist_affect_nobody() {
    let (dir, mut lib, p, m) = setup(Some(10));
    drop(lib.record_open(&p, &m).unwrap());
    let before = progress(&lib, &p);
    drop(lib);
    let ghost = "01ZZZZZZZZZZZZZZZZZZZZZZZZ";
    write_log(
        dir.path(),
        OTHER,
        "2026-11",
        &(pos_ev(1, "2026-11-13T09:00:00.000Z", OTHER, ghost, &m, 5, 0.0)
            + &read_ev(2, "2026-11-13T09:00:01.000Z", OTHER, ghost, true)),
    );
    let lib = reopen(&dir);
    assert_eq!(progress(&lib, &p), before);
    assert!(
        lib.reading_progress(ghost).is_err(),
        "不在目錄裡的文獻查不到狀況"
    );
}

// ---- 材料與頁數 ----

#[test]
fn position_on_a_material_that_is_gone_keeps_reading_status_but_gives_no_position() {
    let (dir, lib, p, m) = setup(Some(10));
    drop(lib);
    let gone = "01MGONE0000000000000000AA";
    write_log(
        dir.path(),
        OTHER,
        "2026-11",
        &pos_ev(1, "2026-11-13T09:00:00.000Z", OTHER, &p, gone, 4, 0.5),
    );
    let mut lib = reopen(&dir);
    let pr = progress(&lib, &p);
    assert_eq!(pr.status, ReadStatus::Reading);
    assert!(pr.position.is_none());
    assert!(pr.furthest_pages.is_empty());
    assert_eq!(pr.last_read_at.as_deref(), Some("2026-11-13T09:00:00.000Z"));
    assert!(lib.continue_reading(10).is_empty());

    // 把材料加回描述檔並重新掃描 → 閱讀位置回來，不必重讀紀錄檔
    let folder = lib.list_papers()[0].folder.clone();
    let pj = dir.path().join("papers").join(&folder).join("paper.json");
    let mut v: Value = serde_json::from_str(&fs::read_to_string(&pj).unwrap()).unwrap();
    let mut extra = v["materials"][0].clone();
    extra["id"] = json!(gone);
    extra["role"] = json!("translation");
    extra["pages"] = json!(10);
    v["materials"].as_array_mut().unwrap().push(extra);
    fs::write(&pj, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    lib.rescan().unwrap();
    let pr = progress(&lib, &p);
    let pos = pr.position.expect("材料回來之後閱讀位置回來");
    assert_eq!((pos.material.as_str(), pos.page), (gone, 4));
    assert_eq!(pr.furthest_pages.get(gone), Some(&4));
    assert_eq!(lib.continue_reading(10), vec![p.clone()]);
    let _ = m;
}

#[test]
fn page_beyond_the_material_page_count_is_not_a_usable_position() {
    let (dir, lib, p, m) = setup(Some(10));
    drop(lib);
    write_log(
        dir.path(),
        OTHER,
        "2026-11",
        &(pos_ev(1, "2026-11-13T09:00:00.000Z", OTHER, &p, &m, 12, 0.0)
            + &pos_ev(2, "2026-11-13T09:00:01.000Z", OTHER, &p, &m, 4, 0.0)
            + &pos_ev(3, "2026-11-13T09:00:02.000Z", OTHER, &p, &m, 11, 0.0)),
    );
    let lib = reopen(&dir);
    let pr = progress(&lib, &p);
    assert_eq!(pr.status, ReadStatus::Reading);
    assert_eq!(
        pr.position.unwrap().page,
        4,
        "第 11、12 頁超過頁數 10，不當閱讀位置"
    );
    assert_eq!(pr.furthest_pages.get(&m), Some(&4), "也不算進最遠頁");
    assert_eq!(
        pr.last_read_at.as_deref(),
        Some("2026-11-13T09:00:02.000Z"),
        "仍算進最近一次閱讀的時間"
    );
}

// ---- 繼續閱讀 ----

#[test]
fn continue_reading_orders_by_recency_limits_and_breaks_ties_by_paper_id() {
    let (dir, mut lib) = temp_library();
    let mut papers = Vec::new();
    for i in 0..4u64 {
        papers.push(add_paper(&mut lib, 10 + i, &format!("P{i}"), Some(10)));
    }
    drop(lib);
    let (p0, m0) = &papers[0];
    let (p1, m1) = &papers[1];
    let (p2, m2) = &papers[2];
    let (p3, _m3) = &papers[3];
    // p0 最舊、p1 與 p2 同時（最新）、p3 只有打開
    let text = pos_ev(1, "2026-11-13T09:00:00.000Z", DEVICE, p0, m0, 2, 0.0)
        + &pos_ev(2, "2026-11-13T10:00:00.000Z", DEVICE, p1, m1, 2, 0.0)
        + &pos_ev(3, "2026-11-13T10:00:00.000Z", OTHER, p2, m2, 2, 0.0)
        + &open_ev(4, "2026-11-13T11:00:00.000Z", DEVICE, p3, &papers[3].1);
    write_log(
        dir.path(),
        DEVICE,
        "2026-11",
        &text
            .lines()
            .filter(|l| l.contains(DEVICE))
            .map(|l| l.to_string() + "\n")
            .collect::<String>(),
    );
    write_log(
        dir.path(),
        OTHER,
        "2026-11",
        &text
            .lines()
            .filter(|l| l.contains(OTHER))
            .map(|l| l.to_string() + "\n")
            .collect::<String>(),
    );
    let lib = reopen(&dir);

    let mut tied = [p1.clone(), p2.clone()];
    tied.sort();
    let all = lib.continue_reading(10);
    assert_eq!(
        all,
        vec![tied[0].clone(), tied[1].clone(), p0.clone()],
        "新到舊；時間相同依文獻代號；只打開的不算"
    );
    assert_eq!(
        lib.continue_reading(2),
        vec![tied[0].clone(), tied[1].clone()]
    );
    assert_eq!(lib.continue_reading(1), vec![tied[0].clone()]);
    assert!(lib.continue_reading(0).is_empty());
    let _ = p3;
}

// ---- 重開、重新掃描、移除與還原 ----

#[test]
fn reopening_gives_the_same_progress() {
    let (dir, mut lib, p, m) = setup(Some(10));
    lib.record_open(&p, &m).unwrap();
    lib.record_position(&p, &m, 6, 0.5, Some(1.2)).unwrap();
    lib.set_read(&p, true).unwrap();
    lib.record_position(&p, &m, 7, 0.1, None).unwrap();
    let before = progress(&lib, &p);
    let list_before = lib.list_papers();
    drop(lib);
    let lib = reopen(&dir);
    assert_eq!(progress(&lib, &p), before);
    assert_eq!(lib.list_papers(), list_before);
}

#[test]
fn rescan_sees_an_appended_line_a_new_device_file_and_a_deleted_file() {
    let (dir, mut lib, p, m) = setup(Some(20));
    let mine = lib.record_position(&p, &m, 3, 0.0, None).unwrap().unwrap();
    assert_eq!(progress(&lib, &p).position.unwrap().page, 3);

    // 1. 在既有的紀錄檔附加一行（別的工具寫的，時間比較晚）
    let file = dir
        .path()
        .join("logs")
        .join(format!("reading-{DEVICE}-{}.jsonl", &mine.t[..7]));
    let mut text = fs::read_to_string(&file).unwrap();
    text.push_str(&pos_ev(
        50,
        "2099-01-01T00:00:00.000Z",
        DEVICE,
        &p,
        &m,
        5,
        0.0,
    ));
    fs::write(&file, &text).unwrap();
    lib.rescan().unwrap();
    assert_eq!(progress(&lib, &p).position.unwrap().page, 5);

    // 2. 新增別台裝置的紀錄檔
    write_log(
        dir.path(),
        OTHER,
        "2099-02",
        &pos_ev(51, "2099-02-01T00:00:00.000Z", OTHER, &p, &m, 8, 0.0),
    );
    lib.rescan().unwrap();
    let pr = progress(&lib, &p);
    assert_eq!(
        (pr.position.unwrap().page, pr.device_positions.len()),
        (8, 2)
    );

    // 3. 刪掉一個紀錄檔
    fs::remove_file(
        dir.path()
            .join("logs")
            .join(format!("reading-{OTHER}-2099-02.jsonl")),
    )
    .unwrap();
    lib.rescan().unwrap();
    let pr = progress(&lib, &p);
    assert_eq!(
        (pr.position.unwrap().page, pr.device_positions.len()),
        (5, 1)
    );
}

#[test]
fn removing_hides_the_paper_and_restoring_brings_back_the_same_progress() {
    let (_dir, mut lib, p, m) = setup(Some(10));
    lib.record_position(&p, &m, 4, 0.5, None).unwrap();
    lib.set_read(&p, true).unwrap();
    let before = progress(&lib, &p);
    assert_eq!(lib.continue_reading(5).len(), 0);
    let (p2, m2) = add_paper(&mut lib, 2, "Second", Some(10));
    lib.record_position(&p2, &m2, 1, 0.0, None).unwrap();
    lib.record_position(&p2, &m2, 6, 0.25, Some(1.5)).unwrap();
    assert_eq!(lib.continue_reading(5), vec![p2.clone()]);
    // 真正被移除又還原的是 p2：比對它自己移除前的狀況
    let before2 = progress(&lib, &p2);
    assert_eq!(before2.position.as_ref().unwrap().page, 6);

    lib.remove_paper(&p2).unwrap();
    assert!(lib.list_papers().iter().all(|s| s.id != p2));
    assert!(lib.continue_reading(5).is_empty());
    assert!(lib.reading_progress(&p2).is_err());

    lib.restore_paper(&p2).unwrap();
    assert_eq!(lib.continue_reading(5), vec![p2.clone()]);
    assert_eq!(progress(&lib, &p2), before2);
    assert_eq!(progress(&lib, &p), before);
}

#[test]
fn every_listed_paper_carries_its_progress() {
    let (_dir, mut lib, p, m) = setup(Some(10));
    let (p2, _m2) = add_paper(&mut lib, 2, "Second", Some(10));
    lib.record_position(&p, &m, 4, 0.5, None).unwrap();
    for s in lib.list_papers() {
        assert_eq!(s.progress, progress(&lib, &s.id));
    }
    let listed = lib.list_papers();
    assert_eq!(
        listed.iter().find(|s| s.id == p).unwrap().progress.status,
        ReadStatus::Reading
    );
    assert_eq!(
        listed.iter().find(|s| s.id == p2).unwrap().progress.status,
        ReadStatus::Unread
    );
}

#[test]
fn progress_is_available_through_the_trait_object() {
    let (_dir, mut lib, p, m) = setup(Some(10));
    let store = &mut lib;
    store.record_position(&p, &m, 2, 0.0, None).unwrap();
    assert_eq!(
        store.reading_progress(&p).unwrap().status,
        ReadStatus::Reading
    );
    assert_eq!(store.continue_reading(3), vec![p]);
}
