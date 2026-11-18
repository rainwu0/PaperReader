//! 備份檔：不壓縮的 zip，內含備份清單 `manifest.json` 與 `library/` 底下的整個文獻庫。
//!
//! 匯出（`local/export.rs`）與還原（這個模組的 `restore_backup`）用同一個路徑檢查
//! `is_safe_backup_path`，所以每一份成功匯出的備份檔都還原得了。

use crate::format::Map;
use crate::store::SkippedFile;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MANIFEST_NAME: &str = "manifest.json";
pub const LIBRARY_PREFIX: &str = "library/";
/// 備份清單的 `kind`。
pub const MANIFEST_KIND: &str = "paperreader-backup";
/// 備份清單的格式版本。
pub const MANIFEST_FORMAT: u32 = 1;
/// 備份清單最大的位元組數（還原時讀進記憶體之前先檢查）。
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
/// 路徑最長的位元組數。
pub const MAX_PATH_BYTES: usize = 1024;

/// 備份清單：列出每個檔的路徑、大小、指紋。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format: u32,
    pub kind: String,
    pub created: String,
    pub library_id: String,
    pub files: Vec<BackupFile>,
    #[serde(flatten)]
    pub extra: Map,
}

/// 備份清單裡的一個檔。`path` 相對於文獻庫根目錄、用 `/` 分隔，在 zip 裡的位置是 `library/` 加上它；
/// `bytes`、`sha256` 是放進 zip 的那份內容的大小與指紋。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(flatten)]
    pub extra: Map,
}

/// 備份檔裡、備份清單裡的路徑是否安全（匯出與還原共用）：非空；不含 NUL 與 `\`；
/// 不以 `/` 開頭；以 `/` 切開的每一段都非空、不是 `.` 或 `..`、不以 `.pr-` 開頭、不含 `:`；
/// 整條不超過 1024 個位元組。
///
/// 不含 `:` 同時擋掉磁碟代號（Windows 上 `staging.join("C:/x")` 會整個換掉基準路徑）與
/// Windows 的替代資料流（`a:b`）。
pub fn is_safe_backup_path(path: &str) -> bool {
    if path.is_empty() || path.len() > MAX_PATH_BYTES {
        return false;
    }
    if path.contains('\0') || path.contains('\\') || path.starts_with('/') {
        return false;
    }
    path.split('/').all(|seg| {
        !seg.is_empty()
            && seg != "."
            && seg != ".."
            && !seg.starts_with(crate::local::TEMP_PREFIX)
            && !seg.contains(':')
    })
}

/// 匯出前的分類：給依位元組順序排好的候選路徑，交回要放進去的與被擋下的（含原因）。
/// 被擋下的：路徑不安全（`unsafe-path`）、與已放進去的一條只差大小寫（`case-duplicate`，
/// 依位元組順序第一個放進去，其餘不放）。純計算，所有平台都測得到。
pub fn classify_backup_paths(paths: &[String]) -> (Vec<String>, Vec<SkippedFile>) {
    let mut keep = Vec::new();
    let mut skipped = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for p in paths {
        if !is_safe_backup_path(p) {
            skipped.push(SkippedFile {
                path: p.clone(),
                reason: "unsafe-path".to_string(),
            });
        } else if !seen.insert(p.to_lowercase()) {
            skipped.push(SkippedFile {
                path: p.clone(),
                reason: "case-duplicate".to_string(),
            });
        } else {
            keep.push(p.clone());
        }
    }
    (keep, skipped)
}

// ---- 從備份檔還原 ----

use crate::error::{LibError, Result, io_error};
use crate::format::LibraryFile;
use crate::ids::{is_valid_id, new_id};
use crate::jsonio::{hash_file, hash_reader};
use crate::local::LocalLibrary;
use crate::store::{CheckLevel, RestoreReport};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub(crate) const RESTORE_TEMP_PREFIX: &str = ".pr-restoring-";
pub(crate) const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

pub(crate) fn unreadable(archive: &Path, reason: impl Into<String>) -> LibError {
    LibError::Unreadable {
        path: archive.display().to_string(),
        reason: reason.into(),
    }
}

/// zip 套件的錯誤：讀取途中的錯誤（含 CRC 不符）當成備份檔壞了。
pub(crate) fn zip_unreadable(archive: &Path, e: zip::result::ZipError) -> LibError {
    unreadable(archive, e.to_string())
}

/// 檢查整份 zip 之後留下的每個要解出的檔：路徑、在 zip 裡的位置、備份清單記的大小與指紋。
pub(crate) struct Planned {
    pub path: String,
    pub index: usize,
    pub bytes: u64,
    pub sha256: String,
}

/// 從備份檔還原一個文獻庫到 `target`。不需要先開啟文獻庫，所以不在 `LocalLibrary` 上。
///
/// 只還原到空的位置（不存在，或存在但是空的資料夾）。在寫任何東西之前先檢查整份 zip；一律先解到
/// 同一個上層資料夾裡的 `.pr-restoring-<代號>/`，每個檔都驗過大小與指紋，才改名成 `target`
/// （`LocalLibrary::open` 看到沒有 `library.json` 會自己建一份新的：若直接解到 `target`、中途失敗，
/// 日後會得到一個有新代號、內容殘缺、看起來卻像正常的文獻庫）。上層資料夾不存在時，等檢查都過了、
/// 要解出之前才建立（失敗的還原不留下任何東西）。
pub fn restore_backup(archive: &Path, target: &Path, device_id: &str) -> Result<RestoreReport> {
    restore_backup_with_progress(archive, target, device_id, &mut |_, _| {})
}

/// [`restore_backup`]，解出每個檔之後回報一次進度（`progress(已解出的位元組, 全部的位元組)`）。
///
/// 要顯示還原進度時用這個，不要在還原途中另外走訪暫存資料夾量大小：Windows 上那個資料夾底下有任何
/// 東西開著時，第 6 步的改名會回 ERROR_ACCESS_DENIED。
pub fn restore_backup_with_progress(
    archive: &Path,
    target: &Path,
    device_id: &str,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<RestoreReport> {
    // 1. 裝置代號、目標
    if !is_valid_id(device_id) {
        return Err(LibError::InvalidInput {
            reason: format!("device id must be 26 Crockford Base32 characters: {device_id:?}"),
        });
    }
    let target_is_empty_dir = match fs::symlink_metadata(target) {
        Ok(m) if m.is_dir() => {
            let mut rd = fs::read_dir(target).map_err(|e| io_error(target, e))?;
            if rd.next().is_some() {
                return Err(LibError::InvalidInput {
                    reason: format!("restore target is not empty: {}", target.display()),
                });
            }
            true
        }
        Ok(_) => {
            return Err(LibError::InvalidInput {
                reason: format!("restore target is not a folder: {}", target.display()),
            });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(io_error(target, e)),
    };
    let parent: PathBuf = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };

    // 2. 清掉上次中斷留下的暫存資料夾：名稱完全符合 `.pr-restoring-<26 個字元的代號>` 的才刪
    if let Ok(rd) = fs::read_dir(&parent) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name
                .strip_prefix(RESTORE_TEMP_PREFIX)
                .is_some_and(is_valid_id)
            {
                let _ = fs::remove_dir_all(e.path());
            }
        }
    }

    // 3–4. 讀備份清單、在寫任何東西之前檢查整份 zip
    let file = fs::File::open(archive).map_err(|e| io_error(archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| zip_unreadable(archive, e))?;
    let (manifest, plan) = validate_archive(archive, &mut zip)?;

    // 5. 解到暫存資料夾，逐一驗大小與指紋
    let parent_existed = parent.exists();
    fs::create_dir_all(&parent).map_err(|e| io_error(&parent, e))?;
    // 失敗時把自己建的上層資料夾（最底下一層，而且還是空的才刪）也拿掉
    let undo_parent = || {
        if !parent_existed {
            let _ = fs::remove_dir(&parent);
        }
    };
    let staging = parent.join(format!("{RESTORE_TEMP_PREFIX}{}", new_id()));
    if let Err(e) = fs::create_dir(&staging) {
        undo_parent();
        return Err(io_error(&staging, e));
    }
    if let Err(e) = extract_all(archive, &mut zip, &plan, &staging, progress) {
        let _ = fs::remove_dir_all(&staging);
        undo_parent();
        return Err(e);
    }

    // 6. 改名成目標（目標是空的資料夾時先刪掉它；只刪空資料夾）
    let renamed = (|| -> Result<()> {
        if target_is_empty_dir {
            fs::remove_dir(target).map_err(|e| io_error(target, e))?;
        }
        fs::rename(&staging, target).map_err(|e| io_error(target, e))
    })();
    if let Err(e) = renamed {
        let _ = fs::remove_dir_all(&staging);
        undo_parent();
        return Err(e);
    }
    crate::jsonio::sync_dir(&parent);

    // 7. 重新讀每個檔，與備份清單比對
    let mut mismatches = Vec::new();
    for p in &plan {
        if !file_matches(&target.join(&p.path), p.bytes, &p.sha256) {
            mismatches.push(p.path.clone());
        }
    }

    // 8. 開啟、快速檢查（第 7 步已經重算過每個檔的指紋）
    let lib = LocalLibrary::open(target, device_id)?;
    let problems = lib.check(CheckLevel::Quick)?;
    Ok(RestoreReport {
        library_id: manifest.library_id,
        files: plan.len(),
        bytes: plan.iter().map(|p| p.bytes).sum(),
        manifest_mismatches: mismatches,
        problems,
    })
}

/// 讀 zip 裡一項的全部內容（最多 `MAX_MANIFEST_BYTES`）。
pub(crate) fn read_entry(
    archive: &Path,
    zip: &mut zip::ZipArchive<fs::File>,
    index: usize,
    what: &str,
) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    zip.by_index(index)
        .map_err(|e| zip_unreadable(archive, e))?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|e| unreadable(archive, format!("cannot read {what}: {e}")))?;
    if data.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(unreadable(archive, format!("{what} is too large")));
    }
    Ok(data)
}

/// zip 每一項的位置與大小（檢查之後）。
pub(crate) struct ZipIndex {
    pub manifest: Option<usize>,
    /// 庫內路徑 → zip 位置。
    pub entries: HashMap<String, usize>,
    pub sizes: HashMap<usize, u64>,
}

/// 在寫任何東西之前檢查 zip 的每一項：名稱是 UTF-8、不是符號連結、只有 `Stored`、沒有加密、
/// 不分大小寫不重複、只有 `manifest.json` 與 `library/` 底下安全的路徑（備份檔與同步包共用）。
pub(crate) fn scan_zip(archive: &Path, zip: &mut zip::ZipArchive<fs::File>) -> Result<ZipIndex> {
    use std::collections::HashSet;
    let mut manifest_index: Option<usize> = None;
    let mut entries: HashMap<String, usize> = HashMap::new(); // 庫內路徑 → zip 位置
    let mut sizes: HashMap<usize, u64> = HashMap::new();
    let mut lower: HashSet<String> = HashSet::new();
    for i in 0..zip.len() {
        let e = zip
            .by_index_raw(i)
            .map_err(|e| zip_unreadable(archive, e))?;
        let name = String::from_utf8(e.name_raw().to_vec())
            .map_err(|_| unreadable(archive, "an entry name is not valid UTF-8"))?;
        if e.is_dir() || name.ends_with('/') {
            continue;
        }
        if e.is_symlink() || e.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(unreadable(archive, format!("{name} is a symbolic link")));
        }
        if e.compression() != zip::CompressionMethod::Stored {
            return Err(unreadable(
                archive,
                format!("{name} uses an unsupported compression method"),
            ));
        }
        if e.encrypted() {
            return Err(unreadable(archive, format!("{name} is encrypted")));
        }
        if !lower.insert(name.to_lowercase()) {
            return Err(unreadable(
                archive,
                format!("{name} differs from another entry only in case"),
            ));
        }
        sizes.insert(i, e.size());
        if name == MANIFEST_NAME {
            manifest_index = Some(i);
        } else if let Some(rest) = name.strip_prefix(LIBRARY_PREFIX) {
            if !is_safe_backup_path(rest) {
                return Err(unreadable(
                    archive,
                    format!("unsafe path in archive: {name}"),
                ));
            }
            entries.insert(rest.to_string(), i);
        } else {
            return Err(unreadable(archive, format!("unexpected entry: {name}")));
        }
    }
    Ok(ZipIndex {
        manifest: manifest_index,
        entries,
        sizes,
    })
}

/// 檔案與資料夾同名（`a` 與 `a/b`）、頂層的 papers、logs、notes、jobs 是檔案：解不出文獻庫。
pub(crate) fn check_tree_shape(
    archive: &Path,
    paths: &[&str],
    listed: &std::collections::HashSet<String>,
) -> Result<()> {
    for p in paths {
        if ["papers", "logs", "notes", "jobs"].contains(p) {
            return Err(unreadable(
                archive,
                format!("{p} must be a folder, not a file"),
            ));
        }
        let mut end = 0;
        while let Some(i) = p[end..].find('/') {
            end += i;
            if listed.contains(&p[..end]) {
                return Err(unreadable(
                    archive,
                    format!("{} is both a file and a folder", &p[..end]),
                ));
            }
            end += 1;
        }
    }
    Ok(())
}

/// 第 3、4 步：讀備份清單並檢查整份 zip。任何一項不合就回報 `Unreadable`，什麼都沒寫。
fn validate_archive(
    archive: &Path,
    zip: &mut zip::ZipArchive<fs::File>,
) -> Result<(BackupManifest, Vec<Planned>)> {
    use std::collections::HashSet;

    let ZipIndex {
        manifest: manifest_index,
        entries,
        sizes,
    } = scan_zip(archive, zip)?;

    // 備份清單
    let mi = manifest_index.ok_or_else(|| unreadable(archive, "manifest.json is missing"))?;
    if sizes[&mi] > MAX_MANIFEST_BYTES {
        return Err(unreadable(archive, "manifest.json is too large"));
    }
    let text = read_entry(archive, zip, mi, MANIFEST_NAME)?;
    let value: serde_json::Value = serde_json::from_slice(text.strip_prefix(&BOM).unwrap_or(&text))
        .map_err(|e| unreadable(archive, format!("manifest.json is not valid JSON: {e}")))?;
    if value.get("kind").and_then(|k| k.as_str()) != Some(MANIFEST_KIND) {
        return Err(unreadable(
            archive,
            "manifest.json is not a PaperReader backup manifest",
        ));
    }
    if let Some(f) = value.get("format").and_then(|f| f.as_u64())
        && f > u64::from(MANIFEST_FORMAT)
    {
        return Err(LibError::FormatTooNew {
            found: u32::try_from(f).unwrap_or(u32::MAX),
        });
    }
    let manifest: BackupManifest = serde_json::from_value(value)
        .map_err(|e| unreadable(archive, format!("manifest.json cannot be read: {e}")))?;

    // 備份清單的路徑：安全、不重複（不分大小寫）；與 zip 裡的檔一一對應
    let mut listed: HashSet<String> = HashSet::new();
    let mut listed_lower: HashSet<String> = HashSet::new();
    let mut plan = Vec::new();
    for f in &manifest.files {
        if !is_safe_backup_path(&f.path) {
            return Err(unreadable(
                archive,
                format!("unsafe path in manifest: {}", f.path),
            ));
        }
        if !listed_lower.insert(f.path.to_lowercase()) {
            return Err(unreadable(
                archive,
                format!("duplicate path in manifest: {}", f.path),
            ));
        }
        listed.insert(f.path.clone());
        let index = *entries.get(&f.path).ok_or_else(|| {
            unreadable(
                archive,
                format!("{} is listed but not in the archive", f.path),
            )
        })?;
        plan.push(Planned {
            path: f.path.clone(),
            index,
            bytes: f.bytes,
            sha256: f.sha256.clone(),
        });
    }
    if let Some(extra) = entries.keys().find(|k| !listed.contains(*k)) {
        return Err(unreadable(
            archive,
            format!("{extra} is in the archive but not listed"),
        ));
    }

    // library.json：在備份清單裡，而且讀得成 LibraryFile（先在記憶體裡解析，不寫檔）
    let lp = plan
        .iter()
        .find(|p| p.path == crate::local::LIBRARY_FILE)
        .ok_or_else(|| unreadable(archive, "library.json is not in the backup"))?;
    let lib_text = read_entry(archive, zip, lp.index, "library.json")?;
    let lf =
        serde_json::from_slice::<LibraryFile>(lib_text.strip_prefix(&BOM).unwrap_or(&lib_text))
            .map_err(|e| unreadable(archive, format!("library.json cannot be read: {e}")))?;
    if lf.id != manifest.library_id {
        return Err(unreadable(
            archive,
            "library_id in the manifest differs from the id in library.json",
        ));
    }

    // 備份清單記的大小必須等於 zip 項目的大小（之後解出時用它當讀取的上限，不能溢位）
    for p in &plan {
        if sizes.get(&p.index).copied() != Some(p.bytes) {
            return Err(unreadable(
                archive,
                format!(
                    "{} has a different size in the manifest and in the archive",
                    p.path
                ),
            ));
        }
    }

    // 檔案與資料夾同名、頂層的 papers 等是檔案：這樣的備份檔解不出文獻庫，在寫任何東西之前就拒絕
    let paths: Vec<&str> = plan.iter().map(|p| p.path.as_str()).collect();
    check_tree_shape(archive, &paths, &listed)?;

    Ok((manifest, plan))
}

/// 第 5 步：逐一解出。每一項最多讀「備份清單的大小加 1」個位元組，邊讀邊算指紋；
/// 大小或指紋與備份清單不符回報 `Unreadable`。
pub(crate) fn extract_all(
    archive: &Path,
    zip: &mut zip::ZipArchive<fs::File>,
    plan: &[Planned],
    staging: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    let all: u64 = plan.iter().map(|p| p.bytes).sum();
    let mut done: u64 = 0;
    for p in plan {
        let dest = staging.join(&p.path);
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir).map_err(|e| io_error(dir, e))?;
        }
        let entry = zip
            .by_index(p.index)
            .map_err(|e| zip_unreadable(archive, e))?;
        let mut input = entry.take(p.bytes.saturating_add(1));
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
            .map_err(|e| io_error(&dest, e))?;
        let (sha, total) = hash_reader(
            &mut input,
            |e| unreadable(archive, format!("cannot read {}: {e}", p.path)),
            |chunk| out.write_all(chunk).map_err(|e| io_error(&dest, e)),
        )?;
        out.sync_all().map_err(|e| io_error(&dest, e))?;
        if total != p.bytes || sha != p.sha256 {
            return Err(unreadable(
                archive,
                format!(
                    "{} does not match the size or fingerprint in the manifest",
                    p.path
                ),
            ));
        }
        done += p.bytes;
        progress(done, all);
    }
    Ok(())
}

/// 第 7 步：重新讀檔、比對大小與指紋。
pub(crate) fn file_matches(path: &Path, bytes: u64, sha256: &str) -> bool {
    hash_file(path).is_ok_and(|(sha, total)| total == bytes && sha == sha256)
}
