//! 備份檔與同步包共用的 zip 寫出。
//!
//! 兩種匯出的流程相同：目標檢查（不存在、不在文獻庫裡）→ 清掉上次中斷留下的暫存檔 → 寫成同資料夾的暫存檔：
//! 逐檔放進去（打不開的記為略過、邊寫邊算指紋、放完再比大小與修改時間）→ 清單最後放 → 落盤 → 確認目標仍不存在
//! 再改名。放哪些檔、大小與時間什麼時候取、清單的內容，由呼叫的一方決定。
//!
//! 獨立成一個模組、不放進 `export.rs`：備份（`export.rs`）與同步包（`sync_export.rs`）
//! 是平行的兩種匯出，共用的部分放在任一方，另一方就要依賴它；放在兩者底下，依賴的方向單純。

use crate::backup::{LIBRARY_PREFIX, MANIFEST_NAME};
use crate::error::{LibError, Result, io_error};
use crate::ids::is_valid_id;
use crate::jsonio::hash_reader;
use crate::store::SkippedFile;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

/// 單一檔案到這個大小以上就開 zip64 的大檔選項（`zip` 套件不開的話，寫超過 4 GiB 的檔案會失敗）。
/// 取 1 GiB 而不是剛好 4 GiB，是因為檔案在匯出途中可能變大。
const LARGE_FILE_BYTES: u64 = 1 << 30;
const TEMP_SUFFIX: &str = ".zip";

/// 匯出的目標檢查：目標已存在、或所在的資料夾在文獻庫裡面，都是 `InvalidInput`。交回目標所在的資料夾。
pub(crate) fn check_target<'a>(out: &'a Path, root: &Path) -> Result<&'a Path> {
    if fs::symlink_metadata(out).is_ok() {
        return Err(target_exists(out));
    }
    let parent = match out.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let parent_canon = fs::canonicalize(parent).map_err(|e| io_error(parent, e))?;
    let root_canon = fs::canonicalize(root).map_err(|e| io_error(root, e))?;
    if parent_canon.starts_with(&root_canon) {
        return Err(LibError::InvalidInput {
            reason: "export target must be outside the library".to_string(),
        });
    }
    Ok(parent)
}

fn target_exists(out: &Path) -> LibError {
    LibError::InvalidInput {
        reason: format!("export target already exists: {}", out.display()),
    }
}

/// 暫存檔的位置：`<dir>/<prefix><代號>.zip`。
pub(crate) fn temp_path(dir: &Path, prefix: &str, id: &str) -> PathBuf {
    dir.join(format!("{prefix}{id}{TEMP_SUFFIX}"))
}

/// 清掉 `dir` 裡名稱完全符合 `<prefix><26 個字元的代號>.zip` 的暫存檔（上次中斷留下的）。
pub(crate) fn clean_temp(dir: &Path, prefix: &str) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let is_ours = name
            .strip_prefix(prefix)
            .and_then(|r| r.strip_suffix(TEMP_SUFFIX))
            .is_some_and(is_valid_id);
        if is_ours {
            let _ = fs::remove_file(e.path());
        }
    }
}

/// 寫到一半的 zip 暫存檔。
pub(crate) struct ZipOut {
    zip: zip::ZipWriter<fs::File>,
    tmp: PathBuf,
}

/// 放進去的一個檔。
pub(crate) struct Put {
    pub bytes: u64,
    pub sha256: String,
    /// 放完再看一次，大小或修改時間與交來的不同（或讀不到）。
    pub changed: bool,
}

impl ZipOut {
    pub(crate) fn create(tmp: &Path) -> Result<ZipOut> {
        let file = fs::File::create(tmp).map_err(|e| io_error(tmp, e))?;
        Ok(ZipOut {
            zip: zip::ZipWriter::new(file),
            tmp: tmp.to_path_buf(),
        })
    }

    /// 把文獻庫裡的 `abs`（相對於根目錄的 `rel`）放成 zip 裡的 `library/<rel>`，邊寫邊算指紋。放的是讀到的那一份。
    /// `size`、`mtime` 是呼叫的一方先前讀到的，放完用來判斷途中有沒有變。
    ///
    /// 打不開：消失了 → `Conflict`（整個匯出失敗）；是 `library.json` → 錯誤（還原端一定要有它）；
    /// 其他（例如沒有權限）→ 記進 `skipped`、交回 `None`。放完再看一次：消失了 → `Conflict`。
    pub(crate) fn put_file(
        &mut self,
        rel: &str,
        abs: &Path,
        size: u64,
        mtime: Option<SystemTime>,
        skipped: &mut Vec<SkippedFile>,
    ) -> Result<Option<Put>> {
        let mut input = match fs::File::open(abs) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(LibError::Conflict),
            Err(e) if rel == super::LIBRARY_FILE => return Err(io_error(abs, e)),
            Err(_) => {
                skipped.push(SkippedFile {
                    path: rel.to_string(),
                    reason: "unreadable".to_string(),
                });
                return Ok(None);
            }
        };
        let mut options =
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        if size >= LARGE_FILE_BYTES {
            options = options.large_file(true);
        }
        let (zip, tmp) = (&mut self.zip, &self.tmp);
        zip.start_file(format!("{LIBRARY_PREFIX}{rel}"), options)
            .map_err(|e| zip_error(tmp, e))?;
        let (sha256, bytes) = hash_reader(
            &mut input,
            |e| io_error(abs, e),
            |chunk| zip.write_all(chunk).map_err(|e| io_error(tmp, e)),
        )?;
        let changed = match fs::metadata(abs) {
            Ok(m) => m.len() != size || m.modified().ok() != mtime,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(LibError::Conflict),
            Err(_) => true,
        };
        Ok(Some(Put {
            bytes,
            sha256,
            changed,
        }))
    }

    /// 清單最後放（要等全部檔案的指紋算完）、寫完落盤；確認 `out` 仍不存在，再把暫存檔改名成 `out`
    /// （`fs::rename` 會蓋掉同名的檔）。
    pub(crate) fn finish_into(self, manifest: &str, out: &Path) -> Result<()> {
        let ZipOut { mut zip, tmp } = self;
        zip.start_file(
            MANIFEST_NAME,
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .map_err(|e| zip_error(&tmp, e))?;
        zip.write_all(manifest.as_bytes())
            .map_err(|e| io_error(&tmp, e))?;
        let file = zip.finish().map_err(|e| zip_error(&tmp, e))?;
        file.sync_all().map_err(|e| io_error(&tmp, e))?;
        drop(file);
        if fs::symlink_metadata(out).is_ok() {
            return Err(target_exists(out));
        }
        fs::rename(&tmp, out).map_err(|e| io_error(out, e))
    }
}

fn zip_error(path: &Path, e: zip::result::ZipError) -> LibError {
    match e {
        zip::result::ZipError::Io(io) => io_error(path, io),
        other => LibError::Io {
            path: path.display().to_string(),
            reason: other.to_string(),
        },
    }
}
