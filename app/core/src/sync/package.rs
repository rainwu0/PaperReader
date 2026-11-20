//! 同步包的清單與容器。容器與路徑檢查沿用備份檔（`crate::backup`）。

use crate::backup::{
    BOM, MANIFEST_KIND, MANIFEST_NAME, MAX_MANIFEST_BYTES, Planned, check_tree_shape, extract_all,
    is_safe_backup_path, read_entry, scan_zip, unreadable, zip_unreadable,
};
use crate::error::{LibError, Result, io_error};
use crate::format::{LibraryFile, Map};
use crate::local::LIBRARY_FILE;
use crate::store::SyncMode;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// 同步包清單的 `kind`。
pub const SYNC_KIND: &str = "paperreader-sync";
/// 同步包清單的格式版本。
pub const SYNC_FORMAT: u32 = 1;

/// 同步包的清單 `manifest.json`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncManifest {
    pub format: u32,
    pub kind: String,
    pub package: String,
    pub created: String,
    pub library_id: String,
    pub device: String,
    #[serde(default)]
    pub device_label: Option<String>,
    pub mode: SyncMode,
    #[serde(default)]
    pub since: Option<String>,
    /// 保留給日後的同步基準，這一版一律 `null`，匯入端不讀。
    #[serde(default)]
    pub base: Option<serde_json::Value>,
    pub files: Vec<SyncFile>,
    #[serde(flatten)]
    pub extra: Map,
}

/// 打開的同步包：清單、要解出的檔、讀好的 `library.json`。
pub(crate) struct OpenedPackage {
    pub zip: zip::ZipArchive<fs::File>,
    pub manifest: SyncManifest,
    pub plan: Vec<Planned>,
    pub library: LibraryFile,
}

/// 打開同步包並在寫任何東西之前檢查整份（不比對文獻庫代號與裝置代號）。
pub(crate) fn open_package(archive: &Path) -> Result<OpenedPackage> {
    let file = fs::File::open(archive).map_err(|e| io_error(archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| zip_unreadable(archive, e))?;
    let idx = scan_zip(archive, &mut zip)?;
    let mi = idx
        .manifest
        .ok_or_else(|| unreadable(archive, "manifest.json is missing"))?;
    if idx.sizes[&mi] > MAX_MANIFEST_BYTES {
        return Err(unreadable(archive, "manifest.json is too large"));
    }
    let text = read_entry(archive, &mut zip, mi, MANIFEST_NAME)?;
    let value: serde_json::Value = serde_json::from_slice(text.strip_prefix(&BOM).unwrap_or(&text))
        .map_err(|e| unreadable(archive, format!("manifest.json is not valid JSON: {e}")))?;
    match value.get("kind").and_then(|k| k.as_str()) {
        Some(SYNC_KIND) => {}
        Some(MANIFEST_KIND) => {
            return Err(unreadable(archive, "backup file, not a sync package"));
        }
        _ => {
            return Err(unreadable(
                archive,
                "manifest.json is not a PaperReader sync package manifest",
            ));
        }
    }
    if let Some(f) = value.get("format").and_then(|f| f.as_u64())
        && f > u64::from(SYNC_FORMAT)
    {
        return Err(LibError::FormatTooNew {
            found: u32::try_from(f).unwrap_or(u32::MAX),
        });
    }
    let manifest: SyncManifest = serde_json::from_value(value)
        .map_err(|e| unreadable(archive, format!("manifest.json cannot be read: {e}")))?;
    // 裝置代號會組成衝突副本的檔名：不是合法的代號就不收
    if !crate::ids::is_valid_id(&manifest.device)
        || !crate::ids::is_valid_id(&manifest.package)
        || crate::ids::parse_utc_millis(&manifest.created).is_none()
    {
        return Err(unreadable(
            archive,
            "manifest.json has an invalid device, package or created",
        ));
    }

    // 清單的路徑：安全、不重複（不分大小寫）；included 與 zip 一一對應
    let mut listed: HashSet<String> = HashSet::new();
    let mut lower: HashSet<String> = HashSet::new();
    let mut plan = Vec::new();
    for f in &manifest.files {
        if !is_safe_backup_path(&f.path) {
            return Err(unreadable(
                archive,
                format!("unsafe path in manifest: {}", f.path),
            ));
        }
        if !lower.insert(f.path.to_lowercase()) {
            return Err(unreadable(
                archive,
                format!("duplicate path in manifest: {}", f.path),
            ));
        }
        listed.insert(f.path.clone());
        match (f.included, idx.entries.get(&f.path)) {
            (true, Some(&index)) => {
                let (Some(bytes), Some(sha)) = (f.bytes, f.sha256.clone()) else {
                    return Err(unreadable(
                        archive,
                        format!("{} has no size or fingerprint in the manifest", f.path),
                    ));
                };
                if idx.sizes.get(&index).copied() != Some(bytes) {
                    return Err(unreadable(
                        archive,
                        format!(
                            "{} has a different size in the manifest and in the archive",
                            f.path
                        ),
                    ));
                }
                plan.push(Planned {
                    path: f.path.clone(),
                    index,
                    bytes,
                    sha256: sha,
                });
            }
            (true, None) => {
                return Err(unreadable(
                    archive,
                    format!("{} is listed as included but not in the archive", f.path),
                ));
            }
            (false, Some(_)) => {
                return Err(unreadable(
                    archive,
                    format!("{} is listed as not included but is in the archive", f.path),
                ));
            }
            (false, None) => {}
        }
    }
    if let Some(extra) = idx.entries.keys().find(|k| !listed.contains(*k)) {
        return Err(unreadable(
            archive,
            format!("{extra} is in the archive but not listed"),
        ));
    }
    let lp = plan
        .iter()
        .find(|p| p.path == LIBRARY_FILE)
        .ok_or_else(|| unreadable(archive, "library.json is not in the sync package"))?;
    let lib_text = read_entry(archive, &mut zip, lp.index, LIBRARY_FILE)?;
    let library =
        serde_json::from_slice::<LibraryFile>(lib_text.strip_prefix(&BOM).unwrap_or(&lib_text))
            .map_err(|e| unreadable(archive, format!("library.json cannot be read: {e}")))?;
    if library.format > crate::format::FORMAT_VERSION {
        return Err(LibError::FormatTooNew {
            found: library.format,
        });
    }
    let paths: Vec<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    check_tree_shape(archive, &paths, &listed)?;
    Ok(OpenedPackage {
        zip,
        manifest,
        plan,
        library,
    })
}

/// 只看一份檔是哪一種：清單 `manifest.json` 的 `kind` 是同步包或備份檔就是那一種；否則根目錄有匯入清單
/// `paperreader-import.json` 的是整批匯入用的 zip（清單的內容不看）；其餘（不是 zip、兩份清單都沒有、
/// 清單讀不懂）都不是。不驗整份、不寫任何東西。檔案打不開時交回錯誤。
pub fn peek_kind(archive: &Path) -> Result<crate::store::ArchiveKind> {
    use crate::store::ArchiveKind;
    let file = fs::File::open(archive).map_err(|e| io_error(archive, e))?;
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return Ok(ArchiveKind::Other);
    };
    match manifest_kind(&mut zip).as_deref() {
        Some(SYNC_KIND) => return Ok(ArchiveKind::SyncPackage),
        Some(MANIFEST_KIND) => return Ok(ArchiveKind::Backup),
        _ => {}
    }
    Ok(
        if zip.index_for_name(crate::import::MANIFEST_NAME).is_some() {
            ArchiveKind::ImportZip
        } else {
            ArchiveKind::Other
        },
    )
}

/// 清單 `manifest.json` 的 `kind`；沒有清單、太大、讀不懂時是 `None`。
fn manifest_kind(zip: &mut zip::ZipArchive<fs::File>) -> Option<String> {
    use std::io::Read;
    let entry = zip.by_name(MANIFEST_NAME).ok()?;
    if entry.size() > MAX_MANIFEST_BYTES {
        return None;
    }
    let mut text = Vec::new();
    entry.take(MAX_MANIFEST_BYTES).read_to_end(&mut text).ok()?;
    serde_json::from_slice::<serde_json::Value>(text.strip_prefix(&BOM).unwrap_or(&text))
        .ok()?
        .get("kind")?
        .as_str()
        .map(String::from)
}

/// 解出到 `dest`，逐一驗大小與指紋（同從備份檔還原）。
pub(crate) fn extract_package(
    archive: &Path,
    opened: &mut OpenedPackage,
    dest: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    extract_all(archive, &mut opened.zip, &opened.plan, dest, progress)
}

/// 清單裡的一個可同步的檔。`included: false` 的不在 zip 裡，`bytes`、`sha256` 是 `paper.json`
/// 材料清單記的（可能是 `null`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncFile {
    pub path: String,
    #[serde(default)]
    pub bytes: Option<u64>,
    #[serde(default)]
    pub sha256: Option<String>,
    pub included: bool,
    #[serde(flatten)]
    pub extra: Map,
}
