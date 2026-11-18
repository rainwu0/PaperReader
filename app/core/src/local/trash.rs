//! 移除與還原。移除不刪除任何東西，只把整個資料夾移到 `.trash/`。
//! 從回收區永久刪除、移除一項材料在 `remove.rs`。

use super::scan::scan_folders;
use super::{LocalLibrary, PAPER_FILE, TRASH_DIR, names_in};
use crate::error::{LibError, Result, io_error};
use crate::format::PaperFile;
use crate::ids::{now_compact, parse_utc_millis};
use crate::jsonio::{read_json, sync_dir};
use crate::naming::unique_name;
use crate::store::TrashEntry;
use std::fs;

/// `<時間>-` 的長度：`20261113T083011Z` 是 16 個字元，再加一個 `-`。
const TRASH_PREFIX_LEN: usize = 17;

impl LocalLibrary {
    /// 把整個文獻資料夾移到 `.trash/<時間>-<資料夾名稱>/`。資料夾裡使用者自己放的檔案一起移過去。
    pub fn remove_paper(&mut self, id: &str) -> Result<()> {
        self.require_writable()?;
        let name = self.locate_mut(id)?.folder;
        let src = self.papers_dir().join(&name);

        let trash = self.root.join(TRASH_DIR);
        fs::create_dir_all(&trash).map_err(|e| io_error(&trash, e))?;
        let existing = names_in(&trash)?;
        let target_name = unique_name(&existing, &format!("{}-{}", now_compact(), name));
        let target = trash.join(&target_name);

        // 先紀錄後檔案
        let rec = self.new_change(
            Some(id),
            crate::changes::PAPER_REMOVED,
            serde_json::json!({ "folder": target_name }),
        );
        self.append_change(&rec)?;
        fs::rename(&src, &target).map_err(|e| io_error(&src, e))?;
        sync_dir(&trash);
        sync_dir(&self.papers_dir());
        self.folders.remove(&name);
        Ok(())
    }

    /// 把資料夾移回 `papers/`，名稱去掉前面的時間；已有同名時加序號。代號不變。
    pub fn restore_paper(&mut self, id: &str) -> Result<()> {
        self.require_writable()?;
        let trash = self.root.join(TRASH_DIR);
        // 回收區裡可能有代號相同的兩個資料夾：取移除時間最晚的那一個（`list_trash` 的第一個）
        let entry = self
            .list_trash()?
            .into_iter()
            .find(|e| e.paper_id == id)
            .ok_or_else(|| LibError::NotFound {
                what: format!("removed paper {id}"),
            })?;
        let trash_name = entry.folder;

        let papers = self.papers_dir();
        fs::create_dir_all(&papers).map_err(|e| io_error(&papers, e))?;
        let base = entry.name;
        let name = unique_name(&names_in(&papers)?, &base);
        let src = trash.join(&trash_name);
        let target = papers.join(&name);

        let rec = self.new_change(
            Some(id),
            crate::changes::PAPER_RESTORED,
            serde_json::json!({}),
        );
        self.append_change(&rec)?;
        fs::rename(&src, &target).map_err(|e| io_error(&src, e))?;
        sync_dir(&trash);
        sync_dir(&papers);
        self.folders = scan_folders(&papers, Some(&self.folders))?;
        Ok(())
    }

    /// 列出回收區裡的文獻。只讀不寫，唯讀的文獻庫也能列；`.trash/` 不存在時交回空的清單。
    ///
    /// 沒有讀得懂的 `paper.json` 的資料夾不列出，也不動它。兩個資料夾的文獻代號相同時兩個都列出。
    /// 排序：移除時間由新到舊，時間相同依名稱；時間不知道的排在最後，依名稱。
    pub fn list_trash(&self) -> Result<Vec<TrashEntry>> {
        let trash = self.root.join(TRASH_DIR);
        let rd = match fs::read_dir(&trash) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_error(&trash, e)),
        };
        let mut out = Vec::new();
        for entry in rd {
            let entry = entry.map_err(|e| io_error(&trash, e))?;
            let folder = entry.file_name().to_string_lossy().into_owned();
            if folder.starts_with('.') {
                continue;
            }
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let Ok((paper, _)) = read_json::<PaperFile>(&entry.path().join(PAPER_FILE)) else {
                continue;
            };
            let (removed_at, name) = split_trash_name(&folder);
            out.push(TrashEntry {
                paper_id: paper.id,
                name: name.to_string(),
                removed_at,
                folder,
                bib: paper.bib,
            });
        }
        out.sort_by(|a, b| {
            match (&a.removed_at, &b.removed_at) {
                (Some(x), Some(y)) => y.cmp(x),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.folder.cmp(&b.folder))
        });
        Ok(out)
    }
}

/// 把 `.trash/` 裡的資料夾名稱拆成（移除時間, 名稱）：前面符合 `20261113T083011Z-` 的是時間，
/// 後面是名稱，而且那個時間要是真實存在的。不是這個樣子的名稱（使用者自己放進 `.trash/` 的）時間是 `None`（不知道），
/// 名稱就是整個資料夾名稱。時間轉成 `2026-11-13T08:30:11.000Z`。
fn split_trash_name(folder: &str) -> (Option<String>, &str) {
    let b = folder.as_bytes();
    let looks_timestamped = b.len() > TRASH_PREFIX_LEN
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'T'
        && b[9..15].iter().all(u8::is_ascii_digit)
        && b[15] == b'Z'
        && b[16] == b'-';
    if !looks_timestamped {
        return (None, folder);
    }
    let t = format!(
        "{}-{}-{}T{}:{}:{}.000Z",
        &folder[0..4],
        &folder[4..6],
        &folder[6..8],
        &folder[9..11],
        &folder[11..13],
        &folder[13..15]
    );
    // 看起來像時間、但不是真實存在的時間（月份 13、`99999999T999999Z` 等）：當作沒有時間前綴
    if parse_utc_millis(&t).is_none() {
        return (None, folder);
    }
    (Some(t), &folder[TRASH_PREFIX_LEN..])
}
