//! 閱讀：記下打開、記下位置、設定已讀，以及分段讀取材料的位元組（給 PDF 元件用，只讀）。

use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::state::AppState;
use paperreader_core::error::io_error;
use paperreader_core::format::Event;
use paperreader_core::progress::ReadingProgress;
use paperreader_core::reading::{ReadingKind, ValidEvent, parse_event};
use paperreader_core::store::EventFilter;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use tauri::State;
use tauri::ipc::Response;

/// 一次最多讀這麼多位元組，避免畫面一次要太大一塊。
const MAX_RANGE: u64 = 4 * 1024 * 1024;

#[tauri::command]
pub async fn record_open(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
) -> ApiResult<()> {
    state
        .lib("record_open", move |l| {
            l.record_open(&paper_id, &material_id).map(|_| ())
        })
        .await
}

/// 記下閱讀位置（`left`：放大讀一欄時畫面左緣在那一頁寬度的幾分之幾）。
/// 與上一筆相同時核心不寫，交回 `false`。
#[tauri::command]
pub async fn record_position(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
    page: i64,
    offset: f64,
    zoom: Option<f64>,
    left: Option<f64>,
) -> ApiResult<bool> {
    state
        .lib("record_position", move |l| {
            l.record_position_with_left(&paper_id, &material_id, page, offset, zoom, left)
                .map(|e| e.is_some())
        })
        .await
}

#[tauri::command]
pub async fn set_read(state: State<'_, AppState>, paper_id: String, read: bool) -> ApiResult<()> {
    state
        .lib("set_read", move |l| l.set_read(&paper_id, read).map(|_| ()))
        .await
}

#[tauri::command]
pub async fn reading_progress(
    state: State<'_, AppState>,
    paper_id: String,
) -> ApiResult<ReadingProgress> {
    state
        .lib("reading_progress", move |l| l.reading_progress(&paper_id))
        .await
}

/// 閱讀紀錄的一列：某一次打開某篇文獻，以及那一次讀到第幾頁。
#[derive(Debug, Serialize, PartialEq)]
pub struct HistoryEntry {
    pub paper_id: String,
    /// 打開的時間（世界協調時間）。依哪一天分組由畫面依手機的時區做。
    pub t: String,
    /// 那一次打開之後最後記下的頁碼；沒記下就是 `null`。
    pub page: Option<u32>,
}

/// 最多交回這麼多列（最近的）。
const MAX_HISTORY: usize = 1000;

/// 把閱讀事件整理成閱讀紀錄：每個 `open` 事件一列，之後同一篇的 `position` 事件更新那一列的頁碼，
/// 直到同一篇再次打開。不在目錄裡的文獻（已移除）略過；無效的事件（讀不懂、缺欄位）略過。最近的在前。
fn build_history(events: &[Event], known: &HashSet<String>) -> Vec<HistoryEntry> {
    let mut valid: Vec<ValidEvent> = events
        .iter()
        .filter(|e| known.contains(&e.paper))
        .filter_map(parse_event)
        .collect();
    valid.sort_by(|a, b| (&a.t, &a.device, &a.id).cmp(&(&b.t, &b.device, &b.id)));
    let mut rows: Vec<HistoryEntry> = Vec::new();
    // 每篇文獻目前「還在接收頁碼」的那一列
    let mut current: HashMap<String, usize> = HashMap::new();
    for ev in valid {
        match ev.kind {
            ReadingKind::Open { .. } => {
                current.insert(ev.paper.clone(), rows.len());
                rows.push(HistoryEntry {
                    paper_id: ev.paper,
                    t: ev.t,
                    page: None,
                });
            }
            ReadingKind::Position { page, .. } => {
                if let Some(&i) = current.get(&ev.paper) {
                    rows[i].page = Some(page);
                }
            }
            ReadingKind::Status { .. } => {}
        }
    }
    rows.reverse();
    rows.truncate(MAX_HISTORY);
    rows
}

/// 閱讀紀錄：哪天打開了哪篇、讀到第幾頁（用核心的讀事件，整理在這裡，只讀）。
#[tauri::command]
pub async fn reading_history(state: State<'_, AppState>) -> ApiResult<Vec<HistoryEntry>> {
    state
        .lib("reading_history", move |l| {
            let known: HashSet<String> = l.list_papers().into_iter().map(|p| p.id).collect();
            let events = l.read_events(&EventFilter::default())?;
            Ok(build_history(&events, &known))
        })
        .await
}

/// 材料檔的大小（位元組）。
#[tauri::command]
pub async fn material_size(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
) -> ApiResult<u64> {
    state
        .run("material_size", move |s| {
            let path = s.with_lib(|l| l.material_path(&paper_id, &material_id))?;
            std::fs::metadata(&path)
                .map(|m| m.len())
                .map_err(|e| ApiError::from(io_error(&path, e)))
        })
        .await
}

/// 從材料檔的 `offset` 起讀最多 `len` 位元組（到檔尾為止）。交回原始位元組，不經 JSON。
/// 路徑由核心決定並確認在文獻資料夾之內，畫面給不出路徑。
#[tauri::command]
pub async fn read_material_range(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
    offset: u64,
    len: u64,
) -> ApiResult<Response> {
    if len > MAX_RANGE {
        return Err(ApiError::new(ErrorKind::InvalidInput, "range too large"));
    }
    let bytes = state
        .run("read_material_range", move |s| {
            let path = s.with_lib(|l| l.material_path(&paper_id, &material_id))?;
            let io = |e| ApiError::from(io_error(&path, e));
            let mut f = std::fs::File::open(&path).map_err(io)?;
            f.seek(SeekFrom::Start(offset)).map_err(io)?;
            let mut buf = Vec::new();
            f.take(len).read_to_end(&mut buf).map_err(io)?;
            Ok(buf)
        })
        .await?;
    Ok(Response::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperreader_core::local::LocalLibrary;
    use paperreader_core::store::AddOutcome;
    use serde_json::json;

    fn ev(id: &str, t: &str, paper: &str, kind: &str, data: serde_json::Value) -> Event {
        Event {
            id: id.into(),
            t: t.into(),
            device: "dev".into(),
            paper: paper.into(),
            kind: kind.into(),
            data,
            extra: Default::default(),
        }
    }

    fn known(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn history_lists_each_open_with_the_last_page_read_in_it() {
        let events = vec![
            ev(
                "a1",
                "2026-11-10T08:00:00.000Z",
                "p1",
                "open",
                json!({"material": "m"}),
            ),
            ev(
                "a2",
                "2026-11-10T08:05:00.000Z",
                "p1",
                "position",
                json!({"material": "m", "page": 3, "offset": 0.1, "zoom": null}),
            ),
            ev(
                "a3",
                "2026-11-10T08:09:00.000Z",
                "p1",
                "position",
                json!({"material": "m", "page": 7, "offset": 0.2, "zoom": null}),
            ),
            ev(
                "a4",
                "2026-11-11T09:00:00.000Z",
                "p1",
                "open",
                json!({"material": "m"}),
            ),
            ev(
                "a5",
                "2026-11-11T09:30:00.000Z",
                "p2",
                "open",
                json!({"material": "m"}),
            ),
            ev(
                "a6",
                "2026-11-11T09:31:00.000Z",
                "p2",
                "position",
                json!({"material": "m", "page": 2, "offset": 0.0, "zoom": null}),
            ),
        ];
        let rows = build_history(&events, &known(&["p1", "p2"]));
        // 最近的在前；p1 第二次打開之後沒有記下頁碼
        let got: Vec<(&str, Option<u32>)> =
            rows.iter().map(|r| (r.paper_id.as_str(), r.page)).collect();
        assert_eq!(got, vec![("p2", Some(2)), ("p1", None), ("p1", Some(7))]);
    }

    #[test]
    fn history_skips_unknown_papers_garbage_and_status_events() {
        let events = vec![
            ev(
                "b1",
                "2026-11-10T08:00:00.000Z",
                "gone",
                "open",
                json!({"material": "m"}),
            ),
            ev("b2", "2026-11-10T08:01:00.000Z", "p1", "open", json!({})), // 缺材料
            ev(
                "b3",
                "2026-11-10T08:02:00.000Z",
                "p1",
                "position",
                json!({"material": "m", "page": 4, "offset": 0.0, "zoom": null}),
            ), // 之前沒有有效的打開
            ev(
                "b4",
                "2026-11-10T08:03:00.000Z",
                "p1",
                "status",
                json!({"read": true}),
            ),
            ev(
                "b5",
                "2026-11-10T08:04:00.000Z",
                "p1",
                "from-the-future",
                json!({}),
            ),
            ev("b6", "not a time", "p1", "open", json!({"material": "m"})),
        ];
        assert!(build_history(&events, &known(&["p1"])).is_empty());
    }

    #[test]
    fn history_is_capped_to_the_most_recent_rows() {
        let events: Vec<Event> = (0..MAX_HISTORY + 5)
            .map(|i| {
                ev(
                    &format!("c{i:05}"),
                    &format!("2026-11-10T08:00:{:02}.{:03}Z", (i / 1000) % 60, i % 1000),
                    "p1",
                    "open",
                    json!({"material": "m"}),
                )
            })
            .collect();
        assert_eq!(build_history(&events, &known(&["p1"])).len(), MAX_HISTORY);
    }

    #[test]
    fn history_comes_from_the_real_event_log_and_drops_removed_papers() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib =
            LocalLibrary::open(&dir.path().join("lib"), &paperreader_core::ids::new_id()).unwrap();
        let mut ids = Vec::new();
        for name in ["one", "two"] {
            let pdf = dir.path().join(format!("{name}.pdf"));
            std::fs::write(&pdf, format!("%PDF-1.4 {name}")).unwrap();
            match lib.add_paper(&pdf, Default::default(), None).unwrap() {
                AddOutcome::Added(id) => ids.push(id),
                other => panic!("unexpected {other:?}"),
            }
        }
        for id in &ids {
            let (paper, _) = lib.get_paper(id).unwrap();
            let material = paper.materials[0].id.clone();
            lib.record_open(id, &material).unwrap();
            lib.record_position(id, &material, 5, 0.5, None).unwrap();
        }
        let known_now: HashSet<String> = lib.list_papers().into_iter().map(|p| p.id).collect();
        let events = lib.read_events(&EventFilter::default()).unwrap();
        let rows = build_history(&events, &known_now);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.page == Some(5)));
        // 移除一篇之後，它的紀錄不再列出
        lib.remove_paper(&ids[0]).unwrap();
        let known_after: HashSet<String> = lib.list_papers().into_iter().map(|p| p.id).collect();
        let rows = build_history(&events, &known_after);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].paper_id, ids[1]);
    }
}
