//! 匯出同步包。只寫補記的想法記錄，不改文獻庫的其他東西。

use super::notes::notes_dir_path;
use super::zipout::{self, ZipOut};
use super::{
    LocalLibrary, PAPER_FILE, PAPER_SIDE_FILES, PAPERS_DIR, TRASH_DIR, is_valid_material_path,
};
use crate::backup::classify_backup_paths;
use crate::changes::{SYNC_IMPORTED, is_changes_file};
use crate::error::{LibError, Result};
use crate::format::PaperFile;
use crate::ids::{new_id, now_utc, parse_utc_millis};
use crate::jsonio::{read_json, sync_dir, to_canonical_json};
use crate::overlay::OVERLAY_FORM;
use crate::store::{SkippedFile, SyncExportReport, SyncMode};
use crate::sync::package::{SYNC_FORMAT, SYNC_KIND, SyncFile, SyncManifest};
use crate::text::TEXT_FILE;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 暫存檔的名稱：`.pr-syncout-<26 個字元的代號>.zip`。
const TEMP_PREFIX: &str = ".pr-syncout-";

/// 一個可同步的檔。
pub(crate) struct Syncable {
    /// 相對於根目錄、用 `/` 分隔。
    pub rel: String,
    pub abs: PathBuf,
    /// 是材料檔（譯文疊加檔除外）時：（`provenance.time`、`paper.json` 記的大小與指紋）。
    pub material: Option<(Option<String>, Option<u64>, Option<String>)>,
    /// 「只帶變更」時看檔案的修改時間決定放不放（`text.json`）。
    pub by_mtime: bool,
}

/// 列出文獻庫裡可同步的檔，依路徑的位元組順序。
pub(crate) fn list_syncable(lib: &LocalLibrary) -> Result<Vec<Syncable>> {
    let root = &lib.root;
    let mut out: Vec<Syncable> = Vec::new();
    type Info = Option<(Option<String>, Option<u64>, Option<String>)>;
    let mut push = |rel: String, material: Info| {
        let abs = root.join(&rel);
        let by_mtime = material.is_none() && rel.ends_with(&format!("/{TEXT_FILE}"));
        out.push(Syncable {
            rel,
            abs,
            material,
            by_mtime,
        });
    };
    push(super::LIBRARY_FILE.to_string(), None);

    // 一篇文獻資料夾裡的檔：paper.json、清單列的材料、ink.json、references.json
    let paper_files =
        |base: &str, folder: &str, paper: &PaperFile, push: &mut dyn FnMut(String, Info)| {
            let dir = root.join(base).join(folder);
            push(format!("{base}/{folder}/{PAPER_FILE}"), None);
            for m in &paper.materials {
                if !is_valid_material_path(&m.path) || m.path.contains('\\') {
                    continue;
                }
                if !dir.join(&m.path).is_file() {
                    continue;
                }
                // 譯文疊加檔與重點筆記一律放進來（合併要讀內容）
                let info = (m.form != OVERLAY_FORM && !crate::digest::is_digest(m))
                    .then(|| (m.provenance.time.clone(), m.bytes, m.sha256.clone()));
                push(format!("{base}/{folder}/{}", m.path), info);
            }
            // 每篇固定一個的檔（ink.json、references.json、text.json）：存在就放，讀不懂也放（由匯入端決定略過）；
            // 不是材料，所以「只帶變更」也一律放——text.json 除外，它只放修改時間晚於起算點的
            for name in PAPER_SIDE_FILES {
                if dir.join(name).is_file() {
                    push(format!("{base}/{folder}/{name}"), None);
                }
            }
        };

    let mut ids: HashSet<String> = HashSet::new();
    let fresh = super::scan::scan_folders(&lib.papers_dir(), None)?;
    let mut seen = HashSet::new();
    for f in fresh.values() {
        if let Some(p) = &f.paper
            && seen.insert(p.id.clone())
        {
            ids.insert(p.id.clone());
            paper_files(PAPERS_DIR, &f.name, p, &mut push);
        }
    }
    // 回收區：每個代號只放最晚移除的那一份；同時在目錄的不放
    for e in lib.list_trash()? {
        if !ids.insert(e.paper_id.clone()) {
            continue;
        }
        let path = root.join(TRASH_DIR).join(&e.folder).join(PAPER_FILE);
        if let Ok((p, _)) = read_json::<PaperFile>(&path) {
            paper_files(TRASH_DIR, &e.folder, &p, &mut push);
        }
    }

    // 筆記檔：屬性區有 paperreader_paper 的
    if let Some(dir) = notes_dir_path(root, &lib.library.notes_dir) {
        let index = super::notes::scan_notes_heads(root, &lib.library.notes_dir);
        for name in index.claimed_files() {
            if dir.join(&name).is_file() {
                push(format!("{}/{name}", lib.library.notes_dir), None);
            }
        }
    }

    // 紀錄檔：全部，包括別台裝置的
    let logs = root.join(super::events::LOGS_DIR);
    if let Ok(rd) = fs::read_dir(&logs) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_reading = name.starts_with("reading-") && name.ends_with(".jsonl");
            if (is_reading || is_changes_file(&name)) && e.path().is_file() {
                push(format!("logs/{name}"), None);
            }
        }
    }

    out.sort_by(|a, b| a.rel.as_bytes().cmp(b.rel.as_bytes()));
    out.dedup_by(|a, b| a.rel == b.rel);
    Ok(out)
}

impl LocalLibrary {
    /// 匯出一份同步包到 `out`。`device_label` 由外殼交來，核心不解讀。
    pub fn export_sync_package(
        &mut self,
        out: &Path,
        mode: SyncMode,
        device_label: Option<&str>,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<SyncExportReport> {
        // 1. 唯讀、目標檢查
        self.require_writable()?;
        let parent = zipout::check_target(out, &self.root)?;

        // 2. 清掉上次中斷留下的暫存檔
        zipout::clean_temp(parent, TEMP_PREFIX);

        // 3. 起算點
        let records = super::changes::read_all_changes(&self.root)?;
        let since = since_for(&records, &self.device_id);
        let (mode, since) = match (mode, since) {
            (SyncMode::Changes, Some(s)) => (SyncMode::Changes, Some(s)),
            _ => (SyncMode::Full, None),
        };
        let since_ms = since.as_deref().and_then(parse_utc_millis);

        // 4. 補記想法
        self.catch_up_thoughts()?;

        // 5. 列出可同步的檔，擋下不安全的路徑
        let candidates = list_syncable(self)?;
        let rels: Vec<String> = candidates.iter().map(|c| c.rel.clone()).collect();
        let (keep, mut skipped) = classify_backup_paths(&rels);
        let keep: HashSet<String> = keep.into_iter().collect();
        let candidates: Vec<Syncable> = candidates
            .into_iter()
            .filter(|c| keep.contains(&c.rel))
            .collect();

        // 6–7. 寫暫存檔、完成、改名
        let package = new_id();
        let tmp = zipout::temp_path(parent, TEMP_PREFIX, &package);
        let manifest_head = ManifestHead {
            package: package.clone(),
            library_id: self.library.id.clone(),
            device: self.device_id.clone(),
            device_label: device_label.map(String::from),
            mode,
            since: since.clone(),
        };
        match write_package(
            &tmp,
            &candidates,
            since_ms,
            mode,
            &manifest_head,
            progress,
            &mut skipped,
            out,
        ) {
            Ok((listed, included, bytes, changed)) => {
                sync_dir(parent);
                skipped.sort_by(|a, b| a.path.cmp(&b.path));
                Ok(SyncExportReport {
                    package,
                    mode,
                    since,
                    listed,
                    included,
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
}

/// 3.4 的起算點：`device` 不是這台、`from_device` 是這台的 `sync-imported` 裡最晚的 `package_created`。
pub(crate) fn since_for(records: &[crate::changes::ChangeRecord], me: &str) -> Option<String> {
    records
        .iter()
        .filter(|r| {
            r.kind == SYNC_IMPORTED && r.device != me && r.data_str("from_device") == Some(me)
        })
        .filter_map(|r| {
            let t = r.data_str("package_created")?;
            Some((parse_utc_millis(t)?, t.to_string()))
        })
        .max_by_key(|(ms, _)| *ms)
        .map(|(_, t)| t)
}

struct ManifestHead {
    package: String,
    library_id: String,
    device: String,
    device_label: Option<String>,
    mode: SyncMode,
    since: Option<String>,
}

/// 這個材料檔要不要放進 `changes` 模式的同步包：`provenance.time` 晚於起算點，或沒有指紋（對方無從找起）。
fn include_in_changes(
    since_ms: Option<i64>,
    info: &(Option<String>, Option<u64>, Option<String>),
) -> bool {
    let (time, _, sha) = info;
    if sha.is_none() {
        return true;
    }
    match (time.as_deref().and_then(parse_utc_millis), since_ms) {
        (Some(t), Some(s)) => t > s,
        (None, _) => false,
        (_, None) => true,
    }
}

/// 檔案的修改時間晚於起算點（`text.json` 在「只帶變更」時）：只看檔案屬性，不讀內容。沒有起算點、
/// 取不到修改時間都算晚（放進去）。
fn modified_after(path: &Path, since_ms: Option<i64>) -> bool {
    let Some(since) = since_ms else { return true };
    let modified = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64);
    modified.is_none_or(|t| t > since)
}

#[allow(clippy::too_many_arguments)]
fn write_package(
    tmp: &Path,
    candidates: &[Syncable],
    since_ms: Option<i64>,
    mode: SyncMode,
    head: &ManifestHead,
    progress: &mut dyn FnMut(u64, u64),
    skipped: &mut Vec<SkippedFile>,
    out: &Path,
) -> Result<(usize, usize, u64, Vec<String>)> {
    let mut zip = ZipOut::create(tmp)?;
    let mut entries: Vec<SyncFile> = Vec::new();
    let mut changed: Vec<String> = Vec::new();

    // 每個檔放不放
    let plan: Vec<(&Syncable, bool)> = candidates
        .iter()
        .map(|c| {
            let include = match (&c.material, mode) {
                (Some(info), SyncMode::Changes) => include_in_changes(since_ms, info),
                (None, SyncMode::Changes) if c.by_mtime => modified_after(&c.abs, since_ms),
                _ => true,
            };
            (c, include)
        })
        .collect();
    let mut sizes: Vec<(u64, Option<SystemTime>)> = Vec::with_capacity(plan.len());
    for (c, include) in &plan {
        if *include {
            match fs::metadata(&c.abs) {
                Ok(m) => sizes.push((m.len(), m.modified().ok())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(LibError::Conflict);
                }
                Err(_) => sizes.push((0, None)),
            }
        } else {
            sizes.push((0, None));
        }
    }
    let total: u64 = sizes.iter().map(|s| s.0).sum();
    let mut done: u64 = 0;
    let mut included = 0usize;

    for ((c, include), (size, mtime)) in plan.iter().zip(sizes) {
        if !*include {
            let (_, bytes, sha) = c.material.clone().unwrap_or_default();
            entries.push(SyncFile {
                path: c.rel.clone(),
                bytes,
                sha256: sha,
                included: false,
                extra: Default::default(),
            });
            continue;
        }
        let Some(put) = zip.put_file(&c.rel, &c.abs, size, mtime, skipped)? else {
            continue;
        };
        entries.push(SyncFile {
            path: c.rel.clone(),
            bytes: Some(put.bytes),
            sha256: Some(put.sha256),
            included: true,
            extra: Default::default(),
        });
        included += 1;
        if put.changed {
            changed.push(c.rel.clone());
        }
        done += put.bytes;
        progress(done, total);
    }
    if !entries
        .iter()
        .any(|e| e.path == super::LIBRARY_FILE && e.included)
    {
        return Err(LibError::Unreadable {
            path: super::LIBRARY_FILE.to_string(),
            reason: "library.json is missing or cannot be read".to_string(),
        });
    }

    let manifest = SyncManifest {
        format: SYNC_FORMAT,
        kind: SYNC_KIND.to_string(),
        package: head.package.clone(),
        created: now_utc(),
        library_id: head.library_id.clone(),
        device: head.device.clone(),
        device_label: head.device_label.clone(),
        mode: head.mode,
        since: head.since.clone(),
        base: None,
        files: entries.clone(),
        extra: Default::default(),
    };
    zip.finish_into(&to_canonical_json(&manifest)?, out)?;
    let bytes: u64 = entries
        .iter()
        .filter(|e| e.included)
        .filter_map(|e| e.bytes)
        .sum();
    Ok((entries.len(), included, bytes, changed))
}
