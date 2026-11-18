//! 掃描 `papers/` 一層與重新掃描。

use super::{Folder, LocalLibrary, PAPER_FILE};
use crate::error::{LibError, Result, io_error};
use crate::format::PaperFile;
use crate::jsonio::read_json;
use crate::store::ScanDiff;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

/// 讀 `papers/` 底下直接一層的資料夾。名稱以 `.` 開頭的略過；`prev` 裡修改時間與大小
/// 都沒變的 `paper.json` 不重讀。沒有 `paper.json` 的資料夾不列入。
pub(crate) fn scan_folders(
    papers: &Path,
    prev: Option<&BTreeMap<String, Folder>>,
) -> Result<BTreeMap<String, Folder>> {
    let mut out = BTreeMap::new();
    if !papers.is_dir() {
        return Ok(out);
    }
    for entry in fs::read_dir(papers).map_err(|e| io_error(papers, e))? {
        let entry = entry.map_err(|e| io_error(papers, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let ft = entry.file_type().map_err(|e| io_error(papers, e))?;
        if !ft.is_dir() {
            continue;
        }
        let paper_json = entry.path().join(PAPER_FILE);
        let meta = match fs::metadata(&paper_json) {
            Ok(m) if m.is_file() => m,
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io_error(&paper_json, e)),
        };
        let mtime: Option<SystemTime> = meta.modified().ok();
        let size = meta.len();
        if let Some(old) = prev.and_then(|p| p.get(&name))
            && old.mtime == mtime
            && old.size == size
        {
            out.insert(name, old.clone());
            continue;
        }
        let paper = match read_json::<PaperFile>(&paper_json) {
            Ok((p, _)) => Some(p),
            Err(LibError::Unreadable { .. }) => None,
            Err(e) => return Err(e),
        };
        out.insert(
            name.clone(),
            Folder {
                name,
                mtime,
                size,
                paper,
            },
        );
    }
    Ok(out)
}

/// 目錄的指紋：代號 → 資料夾名稱、`paper.json` 的修改時間與大小。
type Snapshot = BTreeMap<String, (String, Option<SystemTime>, u64)>;

fn snapshot(lib: &LocalLibrary) -> Snapshot {
    lib.catalog()
        .into_iter()
        .map(|(f, p)| (p.id.clone(), (f.name.clone(), f.mtime, f.size)))
        .collect()
}

impl LocalLibrary {
    /// 重新掃描：只重讀修改時間或大小變了的 `paper.json`；與上次相比列出新增、變動、消失。
    /// 代號相同、資料夾名稱不同算變動。
    pub fn rescan(&mut self) -> Result<ScanDiff> {
        let before = snapshot(self);
        self.folders = scan_folders(&self.papers_dir(), Some(&self.folders))?;
        // 紀錄檔的清單、修改時間或大小有變（包括新出現的與消失的），就全部重讀
        self.refresh_events_if_changed()?;
        // 筆記檔的索引：盡力而為，不會失敗
        self.notes_index = super::notes::scan_notes_dir(
            &self.root,
            &self.library.notes_dir,
            Some(&self.notes_index),
        );
        let after = snapshot(self);

        let mut diff = ScanDiff::default();
        for (id, v) in &after {
            match before.get(id) {
                None => diff.added.push(id.clone()),
                Some(old) if old != v => diff.changed.push(id.clone()),
                Some(_) => {}
            }
        }
        for id in before.keys() {
            if !after.contains_key(id) {
                diff.removed.push(id.clone());
            }
        }
        Ok(diff)
    }
}
