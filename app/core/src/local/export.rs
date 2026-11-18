//! 匯出備份檔。只讀文獻庫、不改它；唯讀的文獻庫也能匯出。

use super::zipout::{self, ZipOut};
use super::{LocalLibrary, rel_lossy};
use crate::backup::{
    BackupFile, BackupManifest, MANIFEST_FORMAT, MANIFEST_KIND, classify_backup_paths,
};
use crate::error::{LibError, Result};
use crate::ids::{new_id, now_utc};
use crate::jsonio::{sync_dir, to_canonical_json};
use crate::store::{ExportReport, SkippedFile};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 暫存檔的名稱：`.pr-export-<26 個字元的代號>.zip`。
const TEMP_PREFIX: &str = ".pr-export-";

/// 開始時列出的一個候選檔案。
struct Candidate {
    /// 相對於根目錄、用 `/` 分隔。
    rel: String,
    abs: PathBuf,
    size: u64,
    mtime: Option<SystemTime>,
}

impl LocalLibrary {
    /// 匯出整個文獻庫成一個備份檔 `out`。`progress(已處理的位元組, 全部的位元組)` 每個檔案處理完
    /// 呼叫一次；總數是開始時列出的大小總和，只是估計。
    ///
    /// 先寫成同資料夾的 `.pr-export-<代號>.zip` 暫存檔，完成後確認 `out` 仍不存在再改名；任何一步失敗
    /// 都刪掉暫存檔。每個檔案放進去的是讀到的那一份，備份清單的大小與指紋在寫進 zip 的同一趟算。
    pub fn export_backup(
        &self,
        out: &Path,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<ExportReport> {
        // 1. 目標檢查
        let parent = zipout::check_target(out, &self.root)?;

        // 2. 清掉上次中斷留下的暫存檔：名稱完全符合的才刪
        zipout::clean_temp(parent, TEMP_PREFIX);

        // 3. 列出候選的檔案
        let mut skipped: Vec<SkippedFile> = Vec::new();
        let mut candidates: Vec<Candidate> = Vec::new();
        walk(&self.root, &self.root, &mut candidates, &mut skipped);
        candidates.sort_by(|a, b| a.rel.as_bytes().cmp(b.rel.as_bytes()));
        let rels: Vec<String> = candidates.iter().map(|c| c.rel.clone()).collect();
        let (keep, blocked) = classify_backup_paths(&rels);
        skipped.extend(blocked);
        let keep_set: std::collections::HashSet<&str> = keep.iter().map(String::as_str).collect();
        let candidates: Vec<Candidate> = candidates
            .into_iter()
            .filter(|c| keep_set.contains(c.rel.as_str()))
            .collect();
        // 還原端一定要有 `library.json`：它不在要放進去的清單裡（不見了、根目錄讀不了、被擋下），
        // 整個匯出失敗，不產生還原不了的備份檔
        if !candidates
            .iter()
            .any(|c| c.rel == crate::local::LIBRARY_FILE)
        {
            return Err(LibError::Unreadable {
                path: self.root.join(crate::local::LIBRARY_FILE).display().to_string(),
                reason: "library.json is missing or cannot be read, so a restorable backup cannot be made"
                    .to_string(),
            });
        }
        let total: u64 = candidates.iter().map(|c| c.size).sum();

        // 4–7. 寫暫存檔、完成、改名
        let tmp = zipout::temp_path(parent, TEMP_PREFIX, &new_id());
        let result = self.write_backup(&tmp, &candidates, total, progress, &mut skipped, out);
        match result {
            Ok((files, bytes, changed)) => {
                sync_dir(parent);
                skipped.sort_by(|a, b| a.path.cmp(&b.path));
                Ok(ExportReport {
                    files,
                    bytes,
                    changed_during_export: changed,
                    skipped,
                })
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(e)
            }
        }
    }

    fn write_backup(
        &self,
        tmp: &Path,
        candidates: &[Candidate],
        total: u64,
        progress: &mut dyn FnMut(u64, u64),
        skipped: &mut Vec<SkippedFile>,
        out: &Path,
    ) -> Result<(usize, u64, Vec<String>)> {
        let mut zip = ZipOut::create(tmp)?;
        let mut entries: Vec<BackupFile> = Vec::new();
        let mut changed: Vec<String> = Vec::new();
        let mut done: u64 = 0;

        for c in candidates {
            // 打不開（沒有權限）的檔記進 skipped，不放進備份清單；消失了、或打不開的是 `library.json`
            // （還原端一定要有它），整個匯出失敗
            let Some(put) = zip.put_file(&c.rel, &c.abs, c.size, c.mtime, skipped)? else {
                continue;
            };
            entries.push(BackupFile {
                path: c.rel.clone(),
                bytes: put.bytes,
                sha256: put.sha256,
                extra: Default::default(),
            });
            // 放完之後大小或修改時間與開始時不同，記進 changed
            if put.changed {
                changed.push(c.rel.clone());
            }
            done += put.bytes;
            progress(done, total);
        }

        // 6. 備份清單最後放（要等全部檔案的指紋算完）
        let manifest = BackupManifest {
            format: MANIFEST_FORMAT,
            kind: MANIFEST_KIND.to_string(),
            created: now_utc(),
            library_id: self.library.id.clone(),
            files: entries.clone(),
            extra: Default::default(),
        };
        // 7. 落盤，確認目標仍不存在再把暫存檔改名成目標
        zip.finish_into(&to_canonical_json(&manifest)?, out)?;
        let bytes: u64 = entries.iter().map(|e| e.bytes).sum();
        Ok((entries.len(), bytes, changed))
    }
}

/// 遞迴列出 `dir` 底下的檔案。路徑中任何一段以 `.pr-` 開頭的不列；以 `.` 開頭的（根目錄的 `.trash/` 除外）
/// 不列；名稱不是 UTF-8、符號連結、不是一般檔案、讀不了的記進 `skipped`。
fn walk(root: &Path, dir: &Path, out: &mut Vec<Candidate>, skipped: &mut Vec<SkippedFile>) {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => {
            skipped.push(SkippedFile {
                path: rel_lossy(root, dir),
                reason: "unreadable".to_string(),
            });
            return;
        }
    };
    let at_root = dir == root;
    for entry in rd.flatten() {
        let name = entry.file_name();
        let path = entry.path();
        let Some(name_str) = name.to_str() else {
            skipped.push(SkippedFile {
                path: rel_lossy(root, &path),
                reason: "non-utf8-name".to_string(),
            });
            continue;
        };
        if name_str.starts_with(crate::local::TEMP_PREFIX) {
            continue;
        }
        if name_str.starts_with('.') && !(at_root && name_str == crate::local::TRASH_DIR) {
            continue;
        }
        let Ok(ft) = entry.file_type() else {
            skipped.push(SkippedFile {
                path: rel_lossy(root, &path),
                reason: "unreadable".to_string(),
            });
            continue;
        };
        if ft.is_symlink() {
            skipped.push(SkippedFile {
                path: rel_lossy(root, &path),
                reason: "symlink".to_string(),
            });
        } else if ft.is_dir() {
            walk(root, &path, out, skipped);
        } else if ft.is_file() {
            match fs::metadata(&path) {
                Ok(m) => out.push(Candidate {
                    rel: rel_lossy(root, &path),
                    abs: path,
                    size: m.len(),
                    mtime: m.modified().ok(),
                }),
                Err(_) => skipped.push(SkippedFile {
                    path: rel_lossy(root, &path),
                    reason: "unreadable".to_string(),
                }),
            }
        } else {
            skipped.push(SkippedFile {
                path: rel_lossy(root, &path),
                reason: "not-a-file".to_string(),
            });
        }
    }
}
