//! 手動選檔的傳輸：外殼對核心 `SyncTransport` 的實作。
//!
//! 核心的接口是同步的，而系統的選檔畫面、「存到…」與分享是非同步的外掛呼叫，所以分兩段：
//! - 取：指令先用系統選檔畫面選好一份（外掛已複製到 `cache/picks/`），這個物件只包著那一份；
//!   `list` 交回它、`fetch` 把它改名到指定的位置（同一個暫存空間，不多複製一份）。
//! - 放：`put` 把匯出好的同步包放進 `cache/exports/`，交給系統的「存到…」或分享由畫面接著呼叫
//!   `deliver_export`（同備份檔）。日後換成同步資料夾或 WebDAV 只換這裡。

use paperreader_core::error::{LibError, Result, io_error};
use paperreader_core::sync::transport::{PackageRef, SyncTransport};
use std::fs;
use std::path::{Path, PathBuf};

pub struct PickerTransport {
    picked: Option<PathBuf>,
    outbox: PathBuf,
}

impl PickerTransport {
    /// `picked`：使用者選好、已在暫存裡的那一份（匯出時沒有）；`outbox`：交出去之前放的資料夾。
    pub fn new(picked: Option<PathBuf>, outbox: &Path) -> PickerTransport {
        PickerTransport {
            picked,
            outbox: outbox.to_path_buf(),
        }
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 改名；跨檔案系統改不了時複製再刪。
fn move_file(from: &Path, to: &Path) -> Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to).map_err(|e| io_error(to, e))?;
    let _ = fs::remove_file(from);
    Ok(())
}

impl SyncTransport for PickerTransport {
    /// 只有使用者選的那一份。
    fn list(&mut self) -> Result<Vec<PackageRef>> {
        Ok(self
            .picked
            .iter()
            .map(|p| PackageRef {
                name: name_of(p),
                bytes: fs::metadata(p).ok().map(|m| m.len()),
                modified: None,
            })
            .collect())
    }

    fn fetch(&mut self, package: &PackageRef, dest: &Path) -> Result<()> {
        if dest.exists() {
            return Err(LibError::InvalidInput {
                reason: "fetch destination already exists".into(),
            });
        }
        let src = self
            .picked
            .as_ref()
            .filter(|p| name_of(p) == package.name && p.is_file())
            .ok_or_else(|| LibError::NotFound {
                what: package.name.clone(),
            })?
            .clone();
        move_file(&src, dest)?;
        self.picked = None;
        Ok(())
    }

    /// 放進交出去的資料夾，同名的取代。
    fn put(&mut self, src: &Path, name: &str) -> Result<PackageRef> {
        if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\', ':', '\0']) {
            return Err(LibError::InvalidInput {
                reason: "invalid package name".into(),
            });
        }
        fs::create_dir_all(&self.outbox).map_err(|e| io_error(&self.outbox, e))?;
        let target = self.outbox.join(name);
        if target.exists() {
            fs::remove_file(&target).map_err(|e| io_error(&target, e))?;
        }
        move_file(src, &target)?;
        Ok(PackageRef {
            name: name.to_string(),
            bytes: fs::metadata(&target).ok().map(|m| m.len()),
            modified: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_and_fetches_only_the_picked_file() {
        let dir = tempfile::tempdir().unwrap();
        let picked = dir.path().join("picks/x/a.zip");
        fs::create_dir_all(picked.parent().unwrap()).unwrap();
        fs::write(&picked, b"zip").unwrap();
        let mut t = PickerTransport::new(Some(picked.clone()), &dir.path().join("exports"));
        let refs = t.list().unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "a.zip");
        assert_eq!(refs[0].bytes, Some(3));
        // 名稱不對：找不到
        let other = PackageRef {
            name: "b.zip".into(),
            bytes: None,
            modified: None,
        };
        let dest = dir.path().join("in.zip");
        assert!(matches!(
            t.fetch(&other, &dest),
            Err(LibError::NotFound { .. })
        ));
        t.fetch(&refs[0], &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"zip");
        assert!(!picked.exists(), "moved, not copied");
        assert!(t.list().unwrap().is_empty());
    }

    #[test]
    fn fetch_refuses_an_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let picked = dir.path().join("a.zip");
        fs::write(&picked, b"zip").unwrap();
        let dest = dir.path().join("in.zip");
        fs::write(&dest, b"old").unwrap();
        let mut t = PickerTransport::new(Some(picked), dir.path());
        let r = t.list().unwrap();
        assert!(matches!(
            t.fetch(&r[0], &dest),
            Err(LibError::InvalidInput { .. })
        ));
        assert_eq!(fs::read(&dest).unwrap(), b"old");
    }

    #[test]
    fn put_moves_into_the_outbox_and_replaces_the_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let outbox = dir.path().join("exports");
        let mut t = PickerTransport::new(None, &outbox);
        for body in [b"one".as_slice(), b"two".as_slice()] {
            let src = dir.path().join("out.zip");
            fs::write(&src, body).unwrap();
            let r = t.put(&src, "PaperReader-sync-x.zip").unwrap();
            assert_eq!(r.bytes, Some(3));
            assert!(!src.exists());
        }
        assert_eq!(
            fs::read(outbox.join("PaperReader-sync-x.zip")).unwrap(),
            b"two"
        );
        let src = dir.path().join("out.zip");
        fs::write(&src, b"x").unwrap();
        assert!(t.put(&src, "../escape.zip").is_err());
        assert!(t.put(&src, ".hidden.zip").is_err());
    }
}
