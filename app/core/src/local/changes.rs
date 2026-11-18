//! 變更紀錄檔的附加與讀取。格式在 `crate::changes`。

use super::LocalLibrary;
use super::events::{LOGS_DIR, append_line};
use crate::changes::{
    ChangeRecord, THOUGHTS_WRITTEN, changes_file_name, is_changes_file, parse_change_line,
    sort_records, thoughts_catch_up, thoughts_hash,
};
use crate::error::{LibError, Result, io_error};
use crate::ids::{new_id, now_utc};
use crate::store::ChangeFilter;
use serde_json::json;
use std::fs;
use std::path::Path;

/// 讀 `logs/` 底下所有 `changes-*.jsonl` 的每一行，依（`t`, `device`, `id`）排序。
/// 最後一行不完整、讀不懂的行略過，檔案不動；讀不了的檔整個略過。
pub(crate) fn read_all_changes(root: &Path) -> Result<Vec<ChangeRecord>> {
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
        if !is_changes_file(&name) {
            continue;
        }
        let Ok(bytes) = fs::read(entry.path()) else {
            continue;
        };
        out.extend(parse_change_bytes(&bytes));
    }
    sort_records(&mut out);
    Ok(out)
}

/// 一份檔的內容 → 讀得懂的每一行（依檔案裡的順序）。
pub(crate) fn parse_change_bytes(bytes: &[u8]) -> Vec<ChangeRecord> {
    bytes
        .split(|b| *b == b'\n')
        .filter(|l| !l.iter().all(|b| b.is_ascii_whitespace()))
        .filter_map(parse_change_line)
        .collect()
}

impl LocalLibrary {
    /// 讀所有裝置的變更紀錄，依（`t`, `device`, `id`）排序。不認得的 `type` 照樣交回。
    pub fn read_changes(&self, filter: &ChangeFilter) -> Result<Vec<ChangeRecord>> {
        Ok(read_all_changes(&self.root)?
            .into_iter()
            .filter(|r| {
                filter
                    .paper
                    .as_deref()
                    .is_none_or(|p| r.paper.as_deref() == Some(p))
                    && filter.kind.as_deref().is_none_or(|k| r.kind == k)
            })
            .collect())
    }

    /// 這台的一筆新記錄（代號與時間是現在）。
    pub(crate) fn new_change(
        &self,
        paper: Option<&str>,
        kind: &str,
        data: serde_json::Value,
    ) -> ChangeRecord {
        ChangeRecord {
            id: new_id(),
            t: now_utc(),
            device: self.device_id.clone(),
            paper: paper.map(String::from),
            kind: kind.to_string(),
            data,
            extra: Default::default(),
        }
    }

    /// 附加一筆到 `logs/changes-<這台>.jsonl`。失敗就交回錯誤，不動其他檔案。
    pub(crate) fn append_change(&mut self, rec: &ChangeRecord) -> Result<()> {
        let mut text = serde_json::to_string(rec).map_err(|e| LibError::InvalidInput {
            reason: e.to_string(),
        })?;
        text.push('\n');
        let path = self
            .root
            .join(LOGS_DIR)
            .join(changes_file_name(&self.device_id));
        append_line(&path, &text)?;
        if let Some(hook) = self.change_hook.0.as_mut()
            && !hook(rec)
        {
            return Err(LibError::Io {
                path: path.display().to_string(),
                reason: "stopped by test hook after appending a change record".to_string(),
            });
        }
        Ok(())
    }

    /// 只給測試用：每附加一筆變更記錄之後呼叫一次；交回 `false` 時那個操作在寫檔之前失敗
    /// （驗「先紀錄後檔案」的順序）。
    #[doc(hidden)]
    pub fn set_change_hook(&mut self, hook: Option<crate::changes::ChangeHookFn>) {
        self.change_hook = super::ChangeHook(hook);
    }

    /// 補記整篇的想法：每一篇有筆記檔的文獻，目前的內容沒有節點（而且不是空的）就
    /// 附加一筆 `thoughts-written`，接在這台這篇的上一筆之後。匯出前與準備匯入時呼叫。
    /// 讀不了的筆記檔、想法不完整的略過。
    pub(crate) fn catch_up_thoughts(&mut self) -> Result<()> {
        self.notes_index = super::notes::scan_notes_dir(
            &self.root,
            &self.library.notes_dir,
            Some(&self.notes_index),
        );
        let mut records = read_all_changes(&self.root)?;
        for (paper, text) in self.all_thoughts() {
            if let Some(prev) = thoughts_catch_up(&records, &self.device_id, &paper, &text) {
                let rec = self.new_change(
                    Some(&paper),
                    THOUGHTS_WRITTEN,
                    json!({"sha256": thoughts_hash(&text), "prev": prev}),
                );
                self.append_change(&rec)?;
                records.push(rec);
            }
        }
        Ok(())
    }
}
