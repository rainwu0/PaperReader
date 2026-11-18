//! 閱讀事件的檔案與記憶體裡的整理結果。
//!
//! 每台裝置、每個月一個 `.jsonl` 檔，一行一筆，只往後附加。這個檔案只管「讀進來、附加上去」；
//! 事件的種類與驗證在 `reading.rs`，狀況的計算在 `progress.rs`，三個寫事件的操作在 `record.rs`。

use super::LocalLibrary;
use crate::error::{LibError, Result, io_error};
use crate::format::Event;
use crate::ids::parse_event_millis;
use crate::reading::{ValidEvent, parse_event};
use crate::store::EventFilter;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub(crate) const LOGS_DIR: &str = "logs";

/// 一個紀錄檔的檔名、修改時間與大小。清單有任何變動，就表示紀錄檔變了。
type LogSig = (String, Option<SystemTime>, u64);

/// 事件的整理結果，以文獻代號為鍵，與目錄分開存。文獻在不在目錄裡、材料還在不在、頁數是多少，
/// 都在查詢時才對照目錄，所以描述檔被改、文獻被移除或還原之後不必重讀紀錄檔。
#[derive(Debug, Default)]
pub(crate) struct EventIndex {
    /// 每篇文獻有效的事件，依（`t`、`device`、`id`）排序。
    by_paper: HashMap<String, Vec<ValidEvent>>,
    /// 這台裝置上一筆事件的時間（毫秒）；寫新事件時用它保證時間只往前走。
    own_last_ms: Option<i64>,
    signature: Vec<LogSig>,
}

impl EventIndex {
    /// 讀一次全部的紀錄檔。
    pub(crate) fn load(root: &Path, device: &str) -> Result<EventIndex> {
        let dir = root.join(LOGS_DIR);
        // 先記下紀錄檔的清單、再讀內容：兩者之間若有人寫入，下一次比對會發現清單變了、重讀
        let signature = log_signature(&dir)?;
        let mut idx = EventIndex {
            signature,
            ..EventIndex::default()
        };
        for raw in read_raw(&dir)? {
            if raw.device == device
                && let Some(ms) = parse_event_millis(&raw.t)
            {
                idx.own_last_ms = Some(idx.own_last_ms.map_or(ms, |m| m.max(ms)));
            }
            if let Some(v) = parse_event(&raw) {
                idx.by_paper.entry(v.paper.clone()).or_default().push(v);
            }
        }
        for events in idx.by_paper.values_mut() {
            events.sort_by(sort_key);
        }
        Ok(idx)
    }

    /// 這篇文獻有效的事件，依（`t`、`device`、`id`）排好序。
    pub(crate) fn events_of(&self, paper: &str) -> &[ValidEvent] {
        self.by_paper.get(paper).map_or(&[], Vec::as_slice)
    }

    /// 這台裝置上一筆事件的時間（毫秒）。
    pub(crate) fn own_last_ms(&self) -> Option<i64> {
        self.own_last_ms
    }
}

fn sort_key(a: &ValidEvent, b: &ValidEvent) -> std::cmp::Ordering {
    (&a.t, &a.device, &a.id).cmp(&(&b.t, &b.device, &b.id))
}

fn is_log_file(name: &str) -> bool {
    name.starts_with("reading-") && name.ends_with(".jsonl")
}

fn log_signature(dir: &Path) -> Result<Vec<LogSig>> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error(dir, e)),
    };
    let mut out = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| io_error(dir, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_log_file(&name) {
            continue;
        }
        let (modified, len) = match entry.metadata() {
            Ok(m) => (m.modified().ok(), m.len()),
            // 清單與讀取之間檔案消失了：當作沒有這個檔
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            // 讀不到資訊的檔案照樣記下（它之後有變就會再讀一次），不擋住別的檔案
            Err(_) => (None, 0),
        };
        out.push((name, modified, len));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// 讀所有紀錄檔的每一行。讀不懂的行（半行、亂碼、缺欄位）略過，檔案不動。順序不保證。
fn read_raw(dir: &Path) -> Result<Vec<Event>> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error(dir, e)),
    };
    let mut out = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| io_error(dir, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_log_file(&name) {
            continue;
        }
        let path = entry.path();
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            // 讀不了的紀錄檔（權限不足、是資料夾等）整個略過；「檢查文獻庫」會把它列成問題
            Err(_) => continue,
        };
        for raw in bytes.split(|b| *b == b'\n') {
            if raw.iter().all(|b| b.is_ascii_whitespace()) {
                continue;
            }
            if let Ok(ev) = serde_json::from_slice::<Event>(raw) {
                out.push(ev);
            }
        }
    }
    Ok(out)
}

/// `logs/` 裡讀不了的紀錄檔（權限不足、是資料夾等）的檔名，供「檢查文獻庫」列成問題。
pub(crate) fn unreadable_log_files(root: &Path) -> Result<Vec<String>> {
    let dir = root.join(LOGS_DIR);
    let rd = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error(&dir, e)),
    };
    let mut out = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| io_error(&dir, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_log_file(&name) {
            continue;
        }
        match fs::File::open(entry.path()).and_then(|mut f| f.read(&mut [0u8; 1])) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => out.push(name),
        }
    }
    out.sort();
    Ok(out)
}

impl LocalLibrary {
    /// 讀所有裝置的紀錄檔，依 `(t, id)` 排序。讀不懂的行略過；不認得的 `type` 照樣交回。
    /// 這一層不檢查 `paper` 是否存在於目錄，也不檢查事件有效與否（那是算狀況時的事）。
    pub fn read_events(&self, filter: &EventFilter) -> Result<Vec<Event>> {
        let mut out: Vec<Event> = read_raw(&self.root.join(LOGS_DIR))?
            .into_iter()
            .filter(|ev| matches(filter, ev))
            .collect();
        out.sort_by(|a, b| (&a.t, &a.id).cmp(&(&b.t, &b.id)));
        Ok(out)
    }

    /// 紀錄檔的清單、修改時間或大小有變（包括新出現的與消失的），就全部重讀。
    /// 重新掃描與每次寫事件之前都會呼叫。
    pub(crate) fn refresh_events_if_changed(&mut self) -> Result<()> {
        let now = log_signature(&self.root.join(LOGS_DIR))?;
        if now != self.events.signature {
            self.events = EventIndex::load(&self.root, &self.device_id)?;
        }
        Ok(())
    }

    /// 附加一筆事件到這台裝置、事件發生當月（世界協調時間）的紀錄檔，並更新記憶體裡的整理結果。
    /// 只給核心內部的三個寫事件操作用，不對外。
    /// 檔案結尾不是換行（上次寫到一半被中斷）時，先補一個換行，新事件自成一行。
    pub(crate) fn append_raw(&mut self, event: &Event) -> Result<()> {
        let mut text = serde_json::to_string(event).map_err(|e| LibError::InvalidInput {
            reason: e.to_string(),
        })?;
        text.push('\n');

        let dir = self.root.join(LOGS_DIR);
        let path: PathBuf = dir.join(format!(
            "reading-{}-{}.jsonl",
            self.device_id,
            &event.t[..7]
        ));
        let (len, written, modified) = append_line(&path, &text)?;
        let payload_len = written;

        // 更新記憶體：這台裝置上一筆的時間（不論事件是否有效，一律更新）、事件、紀錄檔清單
        if let Some(ms) = parse_event_millis(&event.t) {
            self.events.own_last_ms = Some(self.events.own_last_ms.map_or(ms, |m| m.max(ms)));
        }
        if let Some(v) = parse_event(event) {
            let list = self.events.by_paper.entry(v.paper.clone()).or_default();
            list.push(v);
            list.sort_by(sort_key);
        }
        // 紀錄檔清單只更新自己寫的這一個檔（大小是寫入前的長度加上寫入的位元組數）；其餘檔案保留
        // 寫入前記下的樣子，這樣同一時間別處的變動，下一次比對才發現得了
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let entry = (name, modified, len + payload_len);
        match self.events.signature.iter_mut().find(|s| s.0 == entry.0) {
            Some(slot) => *slot = entry,
            None => {
                self.events.signature.push(entry);
                self.events.signature.sort_by(|a, b| a.0.cmp(&b.0));
            }
        }
        Ok(())
    }
}

/// 把一行（含結尾換行）附加到 `path`（`logs/` 不存在就建立）：開檔附加、寫一整行、`sync_data`。
/// 檔案結尾不是換行（上次寫到一半被中斷）時，先補一個換行。閱讀紀錄與變更紀錄檔共用。
/// 交回（寫入前的長度, 這次寫入的位元組數, 寫入後的修改時間）。
pub(crate) fn append_line(path: &Path, text: &str) -> Result<(u64, u64, Option<SystemTime>)> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| io_error(dir, e))?;
    }
    let mut f = fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(path)
        .map_err(|e| io_error(path, e))?;
    let len = f.metadata().map_err(|e| io_error(path, e))?.len();
    let mut payload = Vec::with_capacity(text.len() + 1);
    if len > 0 {
        let mut last = [0u8; 1];
        f.seek(SeekFrom::Start(len - 1))
            .map_err(|e| io_error(path, e))?;
        f.read_exact(&mut last).map_err(|e| io_error(path, e))?;
        if last[0] != b'\n' {
            payload.push(b'\n');
        }
    }
    payload.extend_from_slice(text.as_bytes());
    f.write_all(&payload).map_err(|e| io_error(path, e))?;
    f.sync_data().map_err(|e| io_error(path, e))?;
    let modified = f.metadata().ok().and_then(|m| m.modified().ok());
    Ok((len, payload.len() as u64, modified))
}

fn matches(f: &EventFilter, e: &Event) -> bool {
    f.paper.as_deref().is_none_or(|p| e.paper == p)
        && f.from.as_deref().is_none_or(|t| e.t.as_str() >= t)
        && f.to.as_deref().is_none_or(|t| e.t.as_str() < t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Map;
    use serde_json::json;

    const DEVICE: &str = "01JCK3T1A2B3C4D5E6F7G8H9JK";
    const OTHER: &str = "01OTHERDEVICE00000000000A";

    /// 寫入之後，紀錄檔清單只更新自己寫的那個檔；
    /// 同一時間別處的變動要讓下一次比對發現。直接呼叫 `append_raw`（跳過寫入前的比對），
    /// 模擬「比對之後、寫入完成之前」別的程序改了別台裝置的紀錄檔。
    #[test]
    fn writing_an_event_does_not_mark_changes_in_other_files_as_seen() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
        let logs = dir.path().join(LOGS_DIR);
        fs::create_dir_all(&logs).unwrap();
        let line = json!({
            "id": "01OTHER00000000000000000A", "t": "2026-11-13T09:00:00.000Z",
            "device": OTHER, "paper": "P", "type": "open", "data": {"material": "M"}
        })
        .to_string()
            + "\n";
        fs::write(logs.join(format!("reading-{OTHER}-2026-11.jsonl")), line).unwrap();

        let own = Event {
            id: "01OWN0000000000000000000A".into(),
            t: "2026-11-13T09:00:05.000Z".into(),
            device: DEVICE.into(),
            paper: "P".into(),
            kind: "open".into(),
            data: json!({"material": "M"}),
            extra: Map::new(),
        };
        lib.append_raw(&own).unwrap();
        lib.refresh_events_if_changed().unwrap();

        let devices: Vec<&str> = lib
            .events
            .events_of("P")
            .iter()
            .map(|e| e.device.as_str())
            .collect();
        assert_eq!(devices, [OTHER, DEVICE], "別台裝置的事件要被讀進來");
    }
}
