//! 移除一項材料，以及回收區的永久刪除。
//!
//! 移除一項材料（原文以外：使用者加的譯本、譯文疊加檔、產生的譯本 PDF、重點筆記）：
//! 1. 全部的檢查；2. 附加一筆 `material-removed`（先記錄後檔案）；
//! 3. 寫 `paper.json`（拿掉那一項）；4. 材料檔搬進回收區的一個新資料夾：
//!
//! ```text
//! .trash/<時間>-<資料夾名稱> - <檔名>/
//!   removed-material.json   說明：哪一篇、何時、哪台、原本清單裡的那一項
//!   <檔名>                  材料檔，原樣
//! ```
//!
//! 資料夾裡沒有 `paper.json`，所以不會被當成一篇文獻（目錄、回收區的文獻清單、同步都只認 `paper.json`）；
//! 「檢查文獻庫」不看 `.trash/`；備份照樣帶著。`restore_material` 放回（記 `material-restored`）。
//!
//! 第 3 步之後、第 4 步之前被中斷：檔案留在那篇的資料夾裡，「檢查文獻庫」列成清單沒列的檔，不會遺失。
//! 第 4 步搬不成（交回錯誤）：清單放回原樣。第 4 步裡被中斷：下次開啟時補完或清掉（`finish_removed_materials`）。

use super::{
    LocalLibrary, PAPER_FILE, TEMP_PREFIX, TRASH_DIR, is_valid_material_path, names_in,
    pick_material_name,
};
use crate::changes::{MATERIAL_REMOVED, MATERIAL_RESTORED, PAPER_PURGED};
use crate::error::{LibError, Result, io_error};
use crate::format::{
    FORMAT_VERSION, Map, Material, REMOVED_MATERIAL_FILE, ROLE_SOURCE, RemovedMaterialFile,
};
use crate::ids::{now_compact, now_utc, parse_utc_millis};
use crate::jsonio::{VersionToken, read_json, sync_dir, write_json_atomic};
use crate::naming::{last_segment, split_extension, unique_name};
use crate::store::{PurgeReport, RemovedMaterialEntry};
use std::fs;
use std::path::Path;

/// 回收區資料夾名稱的上限（UTF-8 位元組）；`<時間>-<資料夾名稱> - <檔名>` 超過時只用 `<時間>-<檔名>`。
const MAX_TRASH_NAME_BYTES: usize = 200;

impl LocalLibrary {
    /// 把一項材料（原文以外）從那篇移到回收區。`token` 是那篇 `paper.json` 的版本記號；交回新的記號。
    pub fn remove_material(
        &mut self,
        paper_id: &str,
        material_id: &str,
        token: &VersionToken,
    ) -> Result<VersionToken> {
        let located = self.locate_for_write(paper_id, token)?;
        let folder = located.folder;
        let mut paper = located.paper;
        let i = paper
            .materials
            .iter()
            .position(|m| m.id == material_id)
            .ok_or_else(|| LibError::NotFound {
                what: format!("material {material_id}"),
            })?;
        if paper.materials[i].role == ROLE_SOURCE {
            return Err(LibError::InvalidInput {
                reason: "the source cannot be removed on its own; remove the whole paper"
                    .to_string(),
            });
        }
        let before = paper.clone();
        let m = paper.materials.remove(i);

        // 先記錄後檔案
        let rec = self.new_change(
            Some(paper_id),
            MATERIAL_REMOVED,
            serde_json::json!({ "material": material_id }),
        );
        self.append_change(&rec)?;

        let dir = self.papers_dir().join(&folder);
        let new_token = write_json_atomic(&dir.join(PAPER_FILE), &paper)?;
        // 同一個檔還被別的材料用著（不該發生）：清單照拿掉，檔不搬
        let shared = paper.materials.iter().any(|o| o.path == m.path);
        if !shared
            && let Err(e) =
                trash_material_file(&self.root, &self.device_id, paper_id, &folder, &dir, &m)
        {
            // 搬不成（例如檔正被別的程式開著）：清單放回原樣、交回錯誤。記錄已經寫了：
            // 「項目還在、下一次合併時拿掉」；再按一次移除會重來
            write_json_atomic(&dir.join(PAPER_FILE), &before)?;
            self.remember_paper(&folder, before)?;
            return Err(e);
        }
        self.remember_paper(&folder, paper)?;
        sync_dir(&dir);
        Ok(new_token)
    }

    /// 回收區裡移除的材料（`.trash/` 裡有讀得懂的 `removed-material.json`、沒有 `paper.json` 的資料夾）。
    /// 移除時間由新到舊；時間不知道的排在最後。只讀不寫，唯讀的文獻庫也能列。
    pub fn list_removed_materials(&self) -> Result<Vec<RemovedMaterialEntry>> {
        let trash = self.root.join(TRASH_DIR);
        let mut out: Vec<RemovedMaterialEntry> = removed_entries(&self.root)?
            .into_iter()
            .map(|(folder, marker)| RemovedMaterialEntry {
                bytes: folder_bytes(&trash.join(&folder)),
                removed_at: parse_utc_millis(&marker.removed).map(|_| marker.removed.clone()),
                folder,
                paper_id: marker.paper,
                paper_folder: marker.paper_folder,
                material: marker.material,
            })
            .collect();
        out.sort_by(|a, b| {
            let ka = a.removed_at.as_deref().and_then(parse_utc_millis);
            let kb = b.removed_at.as_deref().and_then(parse_utc_millis);
            match (ka, kb) {
                (Some(x), Some(y)) => y.cmp(&x),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
            .then_with(|| a.folder.cmp(&b.folder))
        });
        Ok(out)
    }

    /// 永久刪除回收區裡的一項，以它在 `.trash/` 裡的資料夾名稱指定（同一篇可以有兩個資料夾）。
    /// 一篇文獻（有讀得懂的 `paper.json`）：先記一筆 `paper-purged`，再刪整個資料夾；一項移除的材料
    /// （有讀得懂的 `removed-material.json`）：直接刪。筆記檔在 `notes/`、閱讀紀錄在 `logs/`，都不動。
    /// 名稱不合規則（含路徑分隔、以 `.` 開頭）或不是應用放的：`InvalidInput`；不存在：`NotFound`。
    pub fn purge_trash_entry(&mut self, folder: &str) -> Result<()> {
        self.require_writable()?;
        match self.trash_kind(folder)? {
            TrashKind::Paper(id) => self.purge_paper_folder(folder, &id),
            TrashKind::Material => delete_trash_folder(&self.root, folder),
        }
    }

    /// 永久刪除回收區裡的每一篇文獻與每一項移除的材料（照 `purge_trash_entry`）。使用者自己放進 `.trash/` 的
    /// 東西不動。中途失敗就停下、交回錯誤，已刪的已經刪了；再做一次會接著刪剩下的。
    pub fn empty_trash(&mut self) -> Result<PurgeReport> {
        self.require_writable()?;
        let mut report = PurgeReport::default();
        for e in self.list_trash()? {
            self.purge_paper_folder(&e.folder, &e.paper_id)?;
            report.papers += 1;
        }
        for (folder, _) in removed_entries(&self.root)? {
            delete_trash_folder(&self.root, &folder)?;
            report.materials += 1;
        }
        Ok(report)
    }

    fn trash_kind(&self, folder: &str) -> Result<TrashKind> {
        let bad = || LibError::InvalidInput {
            reason: format!("not a removed paper or material in the trash: {folder:?}"),
        };
        if !is_plain_name(folder) {
            return Err(bad());
        }
        let dir = self.root.join(TRASH_DIR).join(folder);
        match fs::symlink_metadata(&dir) {
            Ok(md) if md.is_dir() => {}
            Ok(_) => return Err(bad()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(LibError::NotFound {
                    what: format!("trash entry {folder}"),
                });
            }
            Err(e) => return Err(io_error(&dir, e)),
        }
        if let Ok((p, _)) = read_json::<crate::format::PaperFile>(&dir.join(PAPER_FILE)) {
            return Ok(TrashKind::Paper(p.id));
        }
        if !dir.join(PAPER_FILE).exists()
            && read_json::<RemovedMaterialFile>(&dir.join(REMOVED_MATERIAL_FILE)).is_ok()
        {
            return Ok(TrashKind::Material);
        }
        Err(bad())
    }

    /// 把回收區裡移除的材料（以 `.trash/` 裡的資料夾名稱指定）放回那篇，交回那篇的代號。
    /// 那篇要在目錄裡（在回收區就先放回那篇）。順序：記一筆 `material-restored` → `paper.json` 加回原本那一項
    /// （原本的檔名被占用時換一個不重複的名稱）→ 把檔搬回那篇的資料夾 → 刪掉回收區的那個資料夾。
    /// 中途被中斷：再放回一次會接著做（清單已經有那一項、檔還不在時只搬檔）。
    /// 名稱不合規則、不是移除的材料：`InvalidInput`；那一項已經在那篇裡：`InvalidInput`；那篇不在目錄裡：`NotFound`。
    pub fn restore_material(&mut self, folder: &str) -> Result<String> {
        self.require_writable()?;
        if !matches!(self.trash_kind(folder)?, TrashKind::Material) {
            return Err(LibError::InvalidInput {
                reason: format!("not a removed material: {folder:?}"),
            });
        }
        let tdir = self.root.join(TRASH_DIR).join(folder);
        let (marker, _) = read_json::<RemovedMaterialFile>(&tdir.join(REMOVED_MATERIAL_FILE))?;
        if marker.format > FORMAT_VERSION {
            return Err(LibError::FormatTooNew {
                found: marker.format,
            });
        }
        let inner = marker
            .file
            .clone()
            .unwrap_or_else(|| last_segment(&marker.material.path).to_string());
        if !is_plain_name(&inner) {
            return Err(LibError::InvalidInput {
                reason: format!("invalid file name in the trash: {inner:?}"),
            });
        }
        let from = tdir.join(&inner);
        let paper_id = marker.paper.clone();
        let located = self.locate_mut(&paper_id)?;
        if located.paper.format > FORMAT_VERSION {
            return Err(LibError::FormatTooNew {
                found: located.paper.format,
            });
        }
        let folder_name = located.folder;
        let mut paper = located.paper;
        let pdir = self.papers_dir().join(&folder_name);
        let has_file = fs::symlink_metadata(&from).is_ok_and(|md| md.is_file());
        let path = match paper.materials.iter().find(|x| x.id == marker.material.id) {
            // 上次放到一半（清單寫好了、檔還沒搬）：只搬檔
            Some(x) if has_file && !pdir.join(&x.path).exists() => x.path.clone(),
            Some(_) => {
                return Err(LibError::InvalidInput {
                    reason: "the material is already in the paper".to_string(),
                });
            }
            None => {
                if !has_file {
                    return Err(LibError::NotFound {
                        what: format!("file of removed material {inner}"),
                    });
                }
                let mut m = marker.material.clone();
                if !is_valid_material_path(&m.path) {
                    m.path = inner.clone();
                }
                let listed_name = paper
                    .materials
                    .iter()
                    .any(|x| last_segment(&x.path).eq_ignore_ascii_case(last_segment(&m.path)));
                if pdir.join(&m.path).exists() || listed_name {
                    m.path = free_path(&pdir, &m.path, &paper.materials)?;
                }
                // 先記錄後檔案
                let rec = self.new_change(
                    Some(&paper_id),
                    MATERIAL_RESTORED,
                    serde_json::json!({ "material": m.id }),
                );
                self.append_change(&rec)?;
                let path = m.path.clone();
                paper.materials.push(m);
                write_json_atomic(&pdir.join(PAPER_FILE), &paper)?;
                self.remember_paper(&folder_name, paper)?;
                path
            }
        };
        let to = pdir.join(&path);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        fs::rename(&from, &to).map_err(|e| io_error(&from, e))?;
        sync_dir(&pdir);
        // 回收區那個資料夾只剩說明檔；刪不成時留著，下次開啟時清掉（說明檔指向的檔不在）
        let _ = delete_trash_folder(&self.root, folder);
        Ok(paper_id)
    }

    /// 先記錄後刪除。這台還有同一篇（目錄裡、或回收區的另一個資料夾）時不記：
    /// 那篇沒有從這台消失，記了會讓合併把別處那份當成永久刪除。
    fn purge_paper_folder(&mut self, folder: &str, paper_id: &str) -> Result<()> {
        let elsewhere = self.locate(paper_id).is_ok()
            || self
                .list_trash()?
                .iter()
                .any(|e| e.paper_id == paper_id && e.folder != folder);
        if !elsewhere {
            let rec = self.new_change(Some(paper_id), PAPER_PURGED, serde_json::json!({}));
            self.append_change(&rec)?;
        }
        delete_trash_folder(&self.root, folder)
    }
}

/// 恰好一段的一般名稱：不是空的、不以 `.` 開頭、沒有分隔字元與 `:`（Windows 上 `E:x` 是磁碟機
/// 相對的路徑，`join` 之後會換掉整個路徑；應用放的名稱不會有 `:`，`naming.rs`），而且 `Path` 拆出來只有一個
/// 一般的部分、等於它自己。
pub(crate) fn is_plain_name(name: &str) -> bool {
    use std::path::Component;
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\', ':', '\0']) {
        return false;
    }
    let mut parts = Path::new(name).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(x)), None) if x == std::ffi::OsStr::new(name)
    )
}

/// `dir/<rel>` 被占用時，同一層的一個不重複的名稱（`<名稱>-2.<副檔名>` …，同新材料的挑法），交回新的相對路徑。
fn free_path(dir: &Path, rel: &str, materials: &[Material]) -> Result<String> {
    let (parent, file) = match rel.rfind('/') {
        Some(i) => (&rel[..i], &rel[i + 1..]),
        None => ("", rel),
    };
    let base = if parent.is_empty() {
        dir.to_path_buf()
    } else {
        dir.join(parent)
    };
    let (stem, ext) = split_extension(file);
    let name = pick_material_name(&base, materials, stem, ext.strip_prefix('.').unwrap_or(ext))?;
    Ok(if parent.is_empty() {
        name
    } else {
        format!("{parent}/{name}")
    })
}

enum TrashKind {
    Paper(String),
    Material,
}

/// 刪掉 `.trash/<folder>`：先改名成 `.trash/.pr-purge-<代號>`（一步就從回收區消失），再整個刪掉。
/// 刪到一半失敗時留下的 `.pr-` 項目，下次開啟文獻庫時清掉。
pub(crate) fn delete_trash_folder(root: &Path, folder: &str) -> Result<()> {
    let trash = root.join(TRASH_DIR);
    let src = trash.join(folder);
    let tmp = trash.join(format!("{TEMP_PREFIX}purge-{}", crate::ids::new_id()));
    fs::rename(&src, &tmp).map_err(|e| io_error(&src, e))?;
    sync_dir(&trash);
    fs::remove_dir_all(&tmp).map_err(|e| io_error(&tmp, e))
}

/// 把 `dir/<m.path>` 搬進 `.trash/` 的一個新資料夾，旁邊放說明檔，交回資料夾名稱。檔案不在、路徑不合規則時
/// 什麼都不做，交回 `None`。順序：建資料夾 → 說明檔用暫存名 `.pr-removed-material.json` 寫好 →
/// 搬檔 → 說明檔改成正式的名稱。搬不成時刪掉暫存說明檔與資料夾、交回錯誤；被強制結束留下的半成品，
/// 下次開啟時由 `finish_removed_materials` 補完或清掉。
pub(crate) fn trash_material_file(
    root: &Path,
    device: &str,
    paper_id: &str,
    paper_folder: &str,
    dir: &Path,
    m: &Material,
) -> Result<Option<String>> {
    if !is_valid_material_path(&m.path) {
        return Ok(None);
    }
    let src = dir.join(&m.path);
    if !fs::symlink_metadata(&src).is_ok_and(|md| md.is_file()) {
        return Ok(None);
    }
    let file = last_segment(&m.path).to_string();
    let trash = root.join(TRASH_DIR);
    fs::create_dir_all(&trash).map_err(|e| io_error(&trash, e))?;

    // 材料檔剛好叫 removed-material.json 或以 `.` 開頭（不該發生）：換個名字放，免得蓋掉說明檔或被當成暫存
    let inner = if file.eq_ignore_ascii_case(REMOVED_MATERIAL_FILE) || file.starts_with('.') {
        format!("material-{}", file.trim_start_matches('.'))
    } else {
        file.clone()
    };
    let stamp = now_compact();
    let mut base = format!("{stamp}-{paper_folder} - {file}");
    if base.len() > MAX_TRASH_NAME_BYTES {
        base = format!("{stamp}-{file}");
    }
    let name = unique_name(&names_in(&trash)?, &base);
    let target = trash.join(&name);
    fs::create_dir(&target).map_err(|e| io_error(&target, e))?;
    let marker = RemovedMaterialFile {
        format: FORMAT_VERSION,
        paper: paper_id.to_string(),
        paper_folder: paper_folder.to_string(),
        removed: now_utc(),
        removed_by: device.to_string(),
        file: Some(inner.clone()),
        material: m.clone(),
        extra: Map::new(),
    };
    let tmp_marker = target.join(temp_marker_name());
    let moved = write_json_atomic(&tmp_marker, &marker)
        .map(|_| ())
        .and_then(|_| fs::rename(&src, target.join(&inner)).map_err(|e| io_error(&src, e)));
    if let Err(e) = moved {
        let _ = fs::remove_file(&tmp_marker);
        let _ = fs::remove_dir(&target);
        return Err(e);
    }
    // 檔已經在回收區：改名失敗就留給下次開啟時補完（`finish_removed_materials`）
    let _ = fs::rename(&tmp_marker, target.join(REMOVED_MATERIAL_FILE));
    sync_dir(&target);
    sync_dir(&trash);
    Ok(Some(name))
}

fn temp_marker_name() -> String {
    format!("{TEMP_PREFIX}{REMOVED_MATERIAL_FILE}")
}

/// 開啟文獻庫時（可寫）：把移除材料時被強制結束留下的半成品補完或清掉。只看 `.trash/` 第一層
/// 沒有 `paper.json` 的資料夾：
/// - 有暫存說明檔、還有別的檔（材料檔已經搬進來）：暫存說明檔改成正式的名稱；
/// - 有暫存說明檔、沒有別的檔：刪掉暫存說明檔與資料夾；
/// - 有正式說明檔、它指的檔不在、也沒有別的檔（放回時刪資料夾沒刪成）：刪掉；
/// - 空的資料夾：刪掉（`remove_dir` 只刪空的）。
///
/// 其餘（使用者自己放的東西）不動。
pub(crate) fn finish_removed_materials(root: &Path) {
    let trash = root.join(TRASH_DIR);
    let Ok(rd) = fs::read_dir(&trash) else {
        return;
    };
    let tmp = temp_marker_name();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let dir = entry.path();
        if dir.join(PAPER_FILE).exists() {
            continue;
        }
        let Ok(names) = names_in(&dir) else {
            continue;
        };
        let others = names
            .iter()
            .filter(|n| **n != tmp && !n.eq_ignore_ascii_case(REMOVED_MATERIAL_FILE))
            .count();
        let has_tmp = names.contains(&tmp);
        let final_marker = dir.join(REMOVED_MATERIAL_FILE);
        if has_tmp && !final_marker.exists() {
            if others > 0 {
                let _ = fs::rename(dir.join(&tmp), &final_marker);
            } else {
                let _ = fs::remove_file(dir.join(&tmp));
                let _ = fs::remove_dir(&dir);
            }
        } else if names.is_empty() {
            let _ = fs::remove_dir(&dir);
        } else if others == 0
            && !has_tmp
            && let Ok((mk, _)) = read_json::<RemovedMaterialFile>(&final_marker)
            && mk.file.as_deref().is_some_and(|f| !dir.join(f).exists())
        {
            let _ = fs::remove_file(&final_marker);
            let _ = fs::remove_dir(&dir);
        }
    }
}

/// `.trash/` 裡每一個移除的材料：（資料夾名稱, 說明）。名稱以 `.` 開頭的、有 `paper.json` 的（一篇文獻）、
/// 說明檔讀不懂的都不算。`.trash/` 不存在時是空的。
pub(crate) fn removed_entries(root: &Path) -> Result<Vec<(String, RemovedMaterialFile)>> {
    let trash = root.join(TRASH_DIR);
    let rd = match fs::read_dir(&trash) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error(&trash, e)),
    };
    let mut out = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| io_error(&trash, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let dir = entry.path();
        if dir.join(PAPER_FILE).exists() {
            continue;
        }
        if let Ok((marker, _)) = read_json::<RemovedMaterialFile>(&dir.join(REMOVED_MATERIAL_FILE))
        {
            out.push((name, marker));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// 資料夾裡（含子資料夾）一般檔案的位元組數總和；讀不了的不算。
fn folder_bytes(dir: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(dir) else {
        return 0;
    };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => folder_bytes(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}
