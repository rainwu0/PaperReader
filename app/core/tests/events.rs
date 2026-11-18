mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::{Event, Map};
use paperreader_core::local::{EventFilter, LocalLibrary};
use serde_json::json;
use std::fs;
use std::path::Path;

const P1: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const P2: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";

fn ev(id: &str, t: &str, device: &str, paper: &str, kind: &str) -> Event {
    Event {
        id: id.into(),
        t: t.into(),
        device: device.into(),
        paper: paper.into(),
        kind: kind.into(),
        data: json!({}),
        extra: Map::new(),
    }
}

fn line(e: &Event) -> String {
    serde_json::to_string(e).unwrap() + "\n"
}

fn logs(root: &Path) -> std::path::PathBuf {
    let d = root.join("logs");
    fs::create_dir_all(&d).unwrap();
    d
}

fn all() -> EventFilter {
    EventFilter::default()
}

#[test]
fn append_creates_month_file() {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Paper", Some(10));
    let e = lib.record_position(&p, &m, 3, 0.5, None).unwrap().unwrap();
    assert_eq!(e.id.len(), 26);
    assert_eq!(e.device, DEVICE);
    assert_eq!(e.paper, p);
    assert_eq!(e.kind, "position");

    let name = format!("reading-{DEVICE}-{}.jsonl", &e.t[..7]);
    let files: Vec<_> = fs::read_dir(dir.path().join("logs"))
        .unwrap()
        .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files, vec![name.clone()]);
    // 檔名：reading-<裝置代號>-<年>-<月>.jsonl
    let tail = &name[("reading-".len() + DEVICE.len() + 1)..];
    assert_eq!(tail.len(), "2026-11.jsonl".len());
    assert_eq!(tail.as_bytes()[4], b'-');

    let text = fs::read_to_string(dir.path().join("logs").join(&name)).unwrap();
    assert!(text.ends_with('\n'));
    assert_eq!(text.matches('\n').count(), 1);
    let parsed: Event = serde_json::from_str(text.trim_end()).unwrap();
    assert_eq!(parsed, e);
    let raw: serde_json::Value = serde_json::from_str(text.trim_end()).unwrap();
    assert_eq!(raw["type"], json!("position"));
    assert_eq!(raw["data"]["page"], json!(3));
}

#[test]
fn append_twice_two_lines() {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Paper", Some(10));
    let a = lib.record_open(&p, &m).unwrap();
    let b = lib.set_read(&p, true).unwrap();
    let f = dir
        .path()
        .join("logs")
        .join(format!("reading-{DEVICE}-{}.jsonl", &a.t[..7]));
    let text = fs::read_to_string(f).unwrap();
    assert_eq!(text.lines().count(), 2);
    let got = lib.read_events(&all()).unwrap();
    assert_eq!(
        got.iter().map(|e| e.id.clone()).collect::<Vec<_>>().len(),
        2
    );
    assert!(got.contains(&a) && got.contains(&b));
}

#[test]
fn read_merges_devices_in_time_order() {
    let (dir, lib) = temp_library();
    let l = logs(dir.path());
    let a1 = ev("01A1", "2026-11-13T09:00:00.000Z", "DEVA", P1, "open");
    let a2 = ev("01A2", "2026-11-13T09:00:02.000Z", "DEVA", P1, "scroll");
    let b1 = ev("01B1", "2026-11-13T09:00:01.000Z", "DEVB", P1, "open");
    let b2 = ev("01B2", "2026-11-13T09:00:03.000Z", "DEVB", P2, "open");
    fs::write(l.join("reading-DEVA-2026-11.jsonl"), line(&a1) + &line(&a2)).unwrap();
    fs::write(l.join("reading-DEVB-2026-11.jsonl"), line(&b1) + &line(&b2)).unwrap();
    let got = lib.read_events(&all()).unwrap();
    let ids: Vec<_> = got.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["01A1", "01B1", "01A2", "01B2"]);
}

#[test]
fn read_filters_by_paper_and_time() {
    let (dir, lib) = temp_library();
    let l = logs(dir.path());
    let evs = [
        ev("01E1", "2026-11-13T09:00:00.000Z", DEVICE, P1, "open"),
        ev("01E2", "2026-11-13T10:00:00.000Z", DEVICE, P2, "open"),
        ev("01E3", "2026-11-13T11:00:00.000Z", DEVICE, P1, "open"),
        ev("01E4", "2026-11-13T12:00:00.000Z", DEVICE, P1, "open"),
    ];
    let text: String = evs.iter().map(line).collect();
    fs::write(l.join(format!("reading-{DEVICE}-2026-11.jsonl")), text).unwrap();

    let ids = |f: EventFilter| -> Vec<String> {
        lib.read_events(&f)
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect()
    };
    assert_eq!(
        ids(EventFilter {
            paper: Some(P1.into()),
            ..all()
        }),
        ["01E1", "01E3", "01E4"]
    );
    // from 含、to 不含
    assert_eq!(
        ids(EventFilter {
            from: Some("2026-11-13T10:00:00.000Z".into()),
            to: Some("2026-11-13T12:00:00.000Z".into()),
            ..all()
        }),
        ["01E2", "01E3"]
    );
    assert_eq!(
        ids(EventFilter {
            paper: Some(P1.into()),
            from: Some("2026-11-13T10:30:00.000Z".into()),
            to: None,
        }),
        ["01E3", "01E4"]
    );
}

#[test]
fn partial_last_line_is_skipped() {
    let (dir, lib) = temp_library();
    let l = logs(dir.path());
    let a = ev("01A1", "2026-11-13T09:00:00.000Z", DEVICE, P1, "open");
    let b = ev("01A2", "2026-11-13T09:00:01.000Z", DEVICE, P1, "close");
    fs::write(
        l.join(format!("reading-{DEVICE}-2026-11.jsonl")),
        line(&a) + &line(&b) + "{\"id\":\"01J",
    )
    .unwrap();
    let got = lib.read_events(&all()).unwrap();
    assert_eq!(got, [a, b]);
}

#[test]
fn append_after_partial_line_starts_new_line() {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Paper", Some(10));
    let l = logs(dir.path());
    let old = ev("01A1", "2026-11-13T09:00:00.000Z", DEVICE, &p, "open");
    // 檔名要與新事件所在的月份相同：用現在的月份
    let now_month = paperreader_core::ids::now_utc()[..7].to_string();
    let mut old = old;
    old.t = format!("{now_month}-01T09:00:00.000Z");
    let file = l.join(format!("reading-{DEVICE}-{now_month}.jsonl"));
    fs::write(&file, line(&old) + "{\"id\":\"01J").unwrap();

    // 紀錄檔在開啟之後被別的程序改過（附加了半行）：寫入前會重讀，新事件自成一行
    let new = lib.record_open(&p, &m).unwrap();
    let got = lib.read_events(&all()).unwrap();
    assert_eq!(got, [old, new.clone()]);

    let text = fs::read_to_string(&file).unwrap();
    assert!(text.ends_with('\n'));
    let last = text.lines().last().unwrap();
    let parsed: Event = serde_json::from_str(last).unwrap();
    assert_eq!(parsed, new);
}

#[test]
fn garbage_middle_line_is_skipped() {
    let (dir, lib) = temp_library();
    let l = logs(dir.path());
    let a = ev("01A1", "2026-11-13T09:00:00.000Z", DEVICE, P1, "open");
    let b = ev("01A2", "2026-11-13T09:00:01.000Z", DEVICE, P1, "close");
    fs::write(
        l.join(format!("reading-{DEVICE}-2026-11.jsonl")),
        line(&a) + "garbage line\n\n{\"id\": 5}\n" + &line(&b),
    )
    .unwrap();
    assert_eq!(lib.read_events(&all()).unwrap(), [a, b]);
}

#[test]
fn unknown_type_is_returned_and_kept() {
    let (dir, mut lib) = temp_library();
    let (p, m) = add_paper(&mut lib, 1, "Paper", Some(10));
    let l = logs(dir.path());
    let mut e = ev(
        "01A1",
        "2026-11-13T09:00:00.000Z",
        DEVICE,
        &p,
        "from-the-future",
    );
    e.extra.insert("x_new".into(), json!(1));
    let file = l.join(format!("reading-{DEVICE}-2026-11.jsonl"));
    fs::write(&file, line(&e)).unwrap();
    let got = lib.read_events(&all()).unwrap();
    assert_eq!(got, [e.clone()]);
    // 附加別的事件之後，舊的那一行原封不動
    lib.record_open(&p, &m).unwrap();
    assert!(fs::read_to_string(&file).unwrap().starts_with(&line(&e)));
}

#[test]
fn writing_events_in_read_only_library_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("library.json"),
        "{\"format\":2,\"id\":\"01JCK3V7Q8Z9X5M2N4P6R8T0AB\"}",
    )
    .unwrap();
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let before = snapshot(dir.path());
    let r = lib.record_open(P1, "any-material");
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
    let r = lib.set_read(P1, true);
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
    assert_eq!(snapshot(dir.path()), before);
    // 唯讀也讀得到
    assert!(lib.read_events(&all()).unwrap().is_empty());
}

#[test]
fn read_with_no_logs_folder_is_empty() {
    let (_dir, lib) = temp_library();
    assert!(lib.read_events(&all()).unwrap().is_empty());
}
