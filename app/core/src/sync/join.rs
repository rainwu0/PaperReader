//! 第二台裝置加入：從完整的同步包建立一個代號相同的文獻庫。
//! 步驟照從備份檔還原（`backup::restore_backup`），驗 zip 與解出的函式共用。

use super::package::{extract_package, open_package};
use crate::backup::{RESTORE_TEMP_PREFIX, file_matches};
use crate::error::{LibError, Result, io_error};
use crate::ids::{is_valid_id, new_id};
use crate::local::LocalLibrary;
use crate::store::{CheckLevel, RestoreReport};
use std::fs;
use std::path::{Path, PathBuf};

/// 從同步包加入：只接受完整的同步包（清單裡每一項都 `included: true`），只建立到空的位置。
/// 不需要先開啟文獻庫，所以不在 `LocalLibrary` 上（同 `restore_backup`）。
pub fn join_from_sync_package(
    package: &Path,
    target: &Path,
    device_id: &str,
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
                    reason: format!("join target is not empty: {}", target.display()),
                });
            }
            true
        }
        Ok(_) => {
            return Err(LibError::InvalidInput {
                reason: format!("join target is not a folder: {}", target.display()),
            });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(io_error(target, e)),
    };
    let parent: PathBuf = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };

    // 2. 清掉上次中斷留下的暫存資料夾
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

    // 3. 讀清單、檢查整份
    let mut opened = open_package(package)?;
    if opened.manifest.files.iter().any(|f| !f.included) {
        return Err(LibError::InvalidInput {
            reason: "package is not complete: join needs a full sync package".to_string(),
        });
    }
    if opened.library.id != opened.manifest.library_id {
        return Err(crate::backup::unreadable(
            package,
            "library_id in the manifest differs from the id in library.json",
        ));
    }

    // 4. 解到暫存資料夾
    let parent_existed = parent.exists();
    fs::create_dir_all(&parent).map_err(|e| io_error(&parent, e))?;
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
    if let Err(e) = extract_package(package, &mut opened, &staging, &mut |_, _| {}) {
        let _ = fs::remove_dir_all(&staging);
        undo_parent();
        return Err(e);
    }

    // 5. 改名成目標
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

    // 6. 重讀比對、開啟、快速檢查
    let mut mismatches = Vec::new();
    for p in &opened.plan {
        if !file_matches(&target.join(&p.path), p.bytes, &p.sha256) {
            mismatches.push(p.path.clone());
        }
    }
    let lib = LocalLibrary::open(target, device_id)?;
    let problems = lib.check(CheckLevel::Quick)?;
    Ok(RestoreReport {
        library_id: opened.manifest.library_id.clone(),
        files: opened.plan.len(),
        bytes: opened.plan.iter().map(|p| p.bytes).sum(),
        manifest_mismatches: mismatches,
        problems,
    })
}
