//! 傳輸接口：同步包從哪裡來、送到哪裡。合併層只認「一份放在本機的同步包」。
//! 核心本身不連網；會連網的做法（WebDAV 等）放在核心以外，只依賴這個接口。

use crate::error::{LibError, Result, io_error};
use crate::ids::{format_utc_millis, new_id};
use crate::jsonio::sync_dir;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 一個位置上的一份同步包。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PackageRef {
    /// 在那個位置上的名稱，例如 `paperreader-sync-<文獻庫代號>-<裝置代號>.zip`。
    pub name: String,
    pub bytes: Option<u64>,
    pub modified: Option<String>,
}

pub trait SyncTransport {
    /// 列出這個位置上的同步包。只看名稱，不打開；屬於哪個文獻庫、哪台裝置由匯入時的清單判斷。
    fn list(&mut self) -> Result<Vec<PackageRef>>;
    /// 把一份同步包取到本機的 `dest`（一個還不存在的檔）。
    fn fetch(&mut self, package: &PackageRef, dest: &Path) -> Result<()>;
    /// 把本機的一份同步包放上去，名稱是 `name`；已有同名的就取代（先放成暫存名稱再改名）。
    fn put(&mut self, src: &Path, name: &str) -> Result<PackageRef>;
}

/// 一個資料夾當成傳輸的位置：給命令列與測試用，也是日後「同步資料夾」的雛形。
pub struct FolderTransport {
    dir: PathBuf,
}

const PUT_PREFIX: &str = ".pr-put-";

impl FolderTransport {
    pub fn new(dir: &Path) -> FolderTransport {
        FolderTransport {
            dir: dir.to_path_buf(),
        }
    }

    /// 名稱只能是單純的檔名（不含路徑分隔、不以 `.` 開頭）。
    fn check_name(name: &str) -> Result<()> {
        if name.is_empty()
            || name.starts_with('.')
            || name.contains(['/', '\\', ':', '\0'])
            || !name.ends_with(".zip")
        {
            return Err(LibError::InvalidInput {
                reason: format!("invalid package name: {name:?}"),
            });
        }
        Ok(())
    }

    fn package_ref(&self, name: &str) -> Result<PackageRef> {
        let p = self.dir.join(name);
        let m = fs::metadata(&p).map_err(|e| io_error(&p, e))?;
        let modified = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .and_then(format_utc_millis);
        Ok(PackageRef {
            name: name.to_string(),
            bytes: Some(m.len()),
            modified,
        })
    }
}

impl SyncTransport for FolderTransport {
    /// 資料夾裡以 `.zip` 結尾、不以 `.` 開頭的一般檔案，依名稱排序。
    fn list(&mut self) -> Result<Vec<PackageRef>> {
        let rd = fs::read_dir(&self.dir).map_err(|e| io_error(&self.dir, e))?;
        let mut names: Vec<String> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".zip") && !n.starts_with('.'))
            .collect();
        names.sort();
        names.iter().map(|n| self.package_ref(n)).collect()
    }

    fn fetch(&mut self, package: &PackageRef, dest: &Path) -> Result<()> {
        Self::check_name(&package.name)?;
        if fs::symlink_metadata(dest).is_ok() {
            return Err(LibError::InvalidInput {
                reason: format!("fetch target already exists: {}", dest.display()),
            });
        }
        let src = self.dir.join(&package.name);
        if let Err(e) = fs::copy(&src, dest) {
            let _ = fs::remove_file(dest);
            return Err(io_error(&src, e));
        }
        Ok(())
    }

    fn put(&mut self, src: &Path, name: &str) -> Result<PackageRef> {
        Self::check_name(name)?;
        let tmp = self.dir.join(format!("{PUT_PREFIX}{}.zip", new_id()));
        let result = (|| -> Result<()> {
            fs::copy(src, &tmp).map_err(|e| io_error(src, e))?;
            let f = fs::OpenOptions::new()
                .write(true)
                .open(&tmp)
                .map_err(|e| io_error(&tmp, e))?;
            f.sync_all().map_err(|e| io_error(&tmp, e))?;
            let mut f = f;
            f.flush().map_err(|e| io_error(&tmp, e))?;
            drop(f);
            let target = self.dir.join(name);
            fs::rename(&tmp, &target).map_err(|e| io_error(&target, e))?;
            Ok(())
        })();
        if let Err(e) = result {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        sync_dir(&self.dir);
        self.package_ref(name)
    }
}
