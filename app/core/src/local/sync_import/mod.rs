//! 匯入同步包：準備（解到暫存、算預覽）、寫入（組好暫存、寫計畫檔、逐項執行）、
//! 丟棄、重做。合併規則在 `crate::sync::merge`。
//!
//! 分成四個檔：這裡是準備、丟棄，以及其餘三個檔共用的型別與小工具；`compute.rs` 算整份合併；
//! `units.rs` 逐篇（一個代號）合併；`apply.rs` 寫入與重做。

mod apply;
mod compute;
mod units;

use super::notes::{notes_dir_path, read_notes_text};
use super::{
    LocalLibrary, PAPER_FILE, PAPERS_DIR, TRASH_DIR, is_valid_material_path, material_status,
};
use crate::changes::{
    ChangeRecord, DIGEST_POINT_DELETED, LINK_JUDGMENT_DELETED, PAPER_RESTORED, STROKES_DELETED,
    SYNC_IMPORTED, changes_file_name, parse_change_line,
};
use crate::error::{LibError, Result, io_error};
use crate::format::{FORMAT_VERSION, LibraryFile, PaperFile};
use crate::ids::{is_valid_id, new_id, now_compact, now_utc};
use crate::ink::{INK_FILE, InkFile};
use crate::jsonio::{
    hash_reader, read_json, sha256_hex, sync_dir, to_canonical_json, write_json_atomic,
};
use crate::naming::{last_segment, unique_name, unique_stem};
use crate::notes::{as_conflict_copy, conflict_of, unify_newlines};
use crate::overlay::{OVERLAY_FORM, OverlayFile};
use crate::references::{REFERENCES_FILE, read_references_file};
use crate::store::{
    CheckLevel, ConflictChoice, ItemKind, NoticeKind, PackageInfo, SkippedPaper, SyncImportReport,
    SyncNotice, SyncPreview,
};
use crate::sync::merge::library::merge_library;
use crate::sync::merge::logs::{LogMerge, merge_log, union_records};
use crate::sync::merge::notes::merge_notes;
use crate::sync::merge::paper::{LocalFolder, merge_paper, prepare_new_paper};
use crate::sync::merge::trash::{
    Place, latest_place_record, merge_place, purged_last, restored_after_purge,
};
use crate::sync::merge::{Change, MergeCtx, Summary, ink as merge_ink_mod};
use crate::sync::package::{SyncManifest, extract_package, open_package};
use crate::sync::plan::{PLAN_FILE, Plan, PlanItem, PlanReport, SYNC_TEMP_PREFIX, StagedFile};
use crate::sync::tree::{RemoteTree, base_of, paper_rel};
use crate::text::{TEXT_FILE, read_text_file};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 記憶體裡的一份預覽（同一時間只有一份）。
#[derive(Debug)]
pub(crate) struct PendingSync {
    preview_id: String,
    staging: PathBuf,
    manifest: SyncManifest,
    /// 計畫要改、要移的每一個本機檔案在預覽那一刻的指紋（相對於根目錄；不存在是 `None`）。
    tokens: BTreeMap<String, Option<String>>,
    /// 預覽裡的衝突代號（`c1` …）→（內部的鍵, 可選的選項）。
    keys: Vec<(String, String, Vec<ConflictChoice>)>,
}

/// 寫入時要放進暫存的內容從哪裡來。
#[derive(Debug, Clone)]
enum Src {
    Bytes(Vec<u8>),
    /// 同步包解開的 `in/` 裡的路徑。
    Remote(String),
    /// 本機另一個指紋相同的檔（只帶變更的同步包省略的材料）。
    Local(PathBuf),
}

#[derive(Debug, Clone)]
struct WriteItem {
    paper: Option<String>,
    target: String,
    src: Src,
    before: Option<String>,
    after: String,
    notes: bool,
}

#[derive(Debug, Clone)]
struct NewPaperItem {
    paper: String,
    to: Place,
    name: String,
    files: Vec<(String, Src, String)>,
}

/// 算出來的合併：衝突、提醒、彙總、要寫的東西。
struct Computed {
    ctx: MergeCtx,
    skipped: Vec<SkippedPaper>,
    new_papers: Vec<NewPaperItem>,
    moves: Vec<(String, Place, String)>,
    paper_writes: Vec<WriteItem>,
    root_writes: Vec<WriteItem>,
    /// 刪除記錄拿掉的本機材料：（文獻, 那一項），檔要搬進回收區。
    trash_materials: Vec<(String, crate::format::Material)>,
    /// 永久刪除了、本機在回收區的文獻：回收區裡的資料夾刪掉。
    purges: Vec<String>,
    copies: Vec<String>,
    tokens: BTreeMap<String, Option<String>>,
    logs: Vec<String>,
    already_imported: bool,
}

/// 提醒裡記的衝突從內部鍵（`conflict_key`）換成預覽的代號（`c1` …）；`keys` 是（代號, 鍵, 選項）。
fn notices_with_conflict_ids(
    mut notices: Vec<SyncNotice>,
    keys: &[(String, String, Vec<ConflictChoice>)],
) -> Vec<SyncNotice> {
    for n in &mut notices {
        n.conflicts = n
            .conflicts
            .iter()
            .map(|k| {
                keys.iter()
                    .find(|(_, key, _)| key == k)
                    .map(|(id, _, _)| id.clone())
                    .unwrap_or_else(|| k.clone())
            })
            .collect();
    }
    notices
}

/// 分段讀檔算 SHA-256；不存在或讀不了是 `None`。
fn file_sha(path: &Path) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    hash_reader(&mut f, |e| io_error(path, e), |_| Ok(()))
        .ok()
        .map(|(sha, _)| sha)
}

/// 資料夾裡每一個項目的名稱；資料夾不存在或讀不了時交回空的（和 `local::names_in` 不同：那個讀不了是錯誤）。
fn names_in_or_empty(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn canonical(v: &impl serde::Serialize) -> Result<Vec<u8>> {
    Ok(to_canonical_json(v)?.into_bytes())
}

/// 讀一份 JSON（ink.json、譯文疊加檔）：不存在 `Ok(None)`；讀不懂 `Err(())`。
fn read_opt<T: serde::de::DeserializeOwned>(path: &Path) -> std::result::Result<Option<T>, ()> {
    if !path.exists() {
        return Ok(None);
    }
    read_json::<T>(path).map(|(v, _)| Some(v)).map_err(|_| ())
}

/// 根目錄裡名稱完全符合 `.pr-sync-<26 個字元的代號>` 而且有讀得懂的 `plan.json` 的資料夾（依名稱第一個）。
pub(crate) fn find_plan_dir(root: &Path) -> Option<PathBuf> {
    let mut names: Vec<String> = names_in_or_empty(root)
        .into_iter()
        .filter(|n| n.strip_prefix(SYNC_TEMP_PREFIX).is_some_and(is_valid_id))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| root.join(n))
        .find(|d| read_json::<Plan>(&d.join(PLAN_FILE)).is_ok())
}

impl LocalLibrary {
    fn rel_of(&self, abs: &Path) -> String {
        super::rel_lossy(&self.root, abs)
    }

    /// 只給測試用：每做計畫的一項之前呼叫一次（參數：0 是寫計畫檔之前，k 是第 k 項之前，從 1 起算）；
    /// 交回 `false` 就在那裡停下、交回錯誤、不收尾（模擬被強制結束）。
    #[doc(hidden)]
    pub fn set_sync_apply_hook(&mut self, hook: Option<crate::changes::ApplyHookFn>) {
        self.sync_apply_hook = super::ApplyHook(hook);
    }

    fn hook_says_stop(&mut self, k: usize) -> bool {
        match self.sync_apply_hook.0.as_mut() {
            Some(h) => !h(k),
            None => false,
        }
    }

    // ---- 準備 ----

    /// 匯入的準備：解到 `.pr-sync-<預覽代號>/in/`、檢查、補記想法、算出預覽。
    /// 除了暫存資料夾與補記的想法記錄，不寫文獻庫的任何東西。
    pub fn prepare_sync_import(
        &mut self,
        package: &Path,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<SyncPreview> {
        self.require_writable()?;
        self.resumed_report = None;
        if let Some(p) = self.pending_sync.take() {
            let _ = fs::remove_dir_all(&p.staging);
        }
        let mut opened = open_package(package)?;
        let manifest = opened.manifest.clone();
        if manifest.library_id != self.library.id || opened.library.id != self.library.id {
            return Err(LibError::InvalidInput {
                reason: format!(
                    "library id mismatch: package {} vs library {}",
                    manifest.library_id, self.library.id
                ),
            });
        }
        if manifest.device == self.device_id {
            return Err(LibError::InvalidInput {
                reason: "package from this device".to_string(),
            });
        }
        let preview_id = new_id();
        let staging = self.root.join(format!("{SYNC_TEMP_PREFIX}{preview_id}"));
        let inn = staging.join("in");
        fs::create_dir_all(&inn).map_err(|e| io_error(&inn, e))?;
        // 逐檔回報進度
        if let Err(e) = extract_package(package, &mut opened, &inn, progress) {
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }

        let result = (|| -> Result<(Computed, PackageInfo)> {
            self.catch_up_thoughts()?;
            let tree = RemoteTree::new(inn.clone(), manifest.clone());
            let computed = self.compute_merge(&tree, &HashMap::new())?;
            let info = PackageInfo {
                package: manifest.package.clone(),
                created: manifest.created.clone(),
                device: manifest.device.clone(),
                device_label: manifest.device_label.clone(),
                mode: manifest.mode,
                already_imported: computed.already_imported,
            };
            Ok((computed, info))
        })();
        let (computed, info) = match result {
            Ok(v) => v,
            Err(e) => {
                let _ = fs::remove_dir_all(&staging);
                return Err(e);
            }
        };
        let mut conflicts = Vec::new();
        let mut keys = Vec::new();
        for (i, (key, mut c)) in computed.ctx.conflicts.into_iter().enumerate() {
            c.id = format!("c{}", i + 1);
            keys.push((c.id.clone(), key, c.options.clone()));
            conflicts.push(c);
        }
        let preview = SyncPreview {
            preview_id: preview_id.clone(),
            package: info,
            added: Summary::items(&computed.ctx.summary.added),
            updated: Summary::items(&computed.ctx.summary.updated),
            removed: Summary::items(&computed.ctx.summary.removed),
            conflicts,
            notices: notices_with_conflict_ids(computed.ctx.notices, &keys),
            skipped_papers: computed.skipped,
        };
        self.pending_sync = Some(PendingSync {
            preview_id,
            staging,
            manifest,
            tokens: computed.tokens,
            keys,
        });
        Ok(preview)
    }

    /// 丟棄目前的預覽（刪它的暫存資料夾）。代號不是目前這一份 → `NotFound`。
    pub fn discard_sync_import(&mut self, preview_id: &str) -> Result<()> {
        match &self.pending_sync {
            Some(p) if p.preview_id == preview_id => {
                let p = self.pending_sync.take().expect("checked");
                let _ = fs::remove_dir_all(&p.staging);
                Ok(())
            }
            _ => Err(LibError::NotFound {
                what: format!("sync preview {preview_id}"),
            }),
        }
    }
}

/// 一篇（一個代號）的結果。
#[derive(Default)]
struct Unit {
    /// 本機那篇的資料夾（相對於根目錄）。
    dir_rel: Option<String>,
    /// 讀過的本機檔（有衝突時記成版本記號）。
    read: Vec<(String, Option<String>)>,
    new_papers: Vec<NewPaperItem>,
    moves: Vec<(String, Place, String)>,
    paper_writes: Vec<WriteItem>,
    root_writes: Vec<WriteItem>,
    trash_materials: Vec<(String, crate::format::Material)>,
    /// 永久刪除了、本機在回收區的文獻。
    purges: Vec<String>,
    copies: Vec<String>,
}

struct UnitNames<'a> {
    papers: &'a mut HashSet<String>,
    trash: &'a mut HashSet<String>,
    notes: &'a mut HashSet<String>,
}

struct NotesCtx<'a> {
    ok: bool,
    dir: &'a str,
    local: &'a BTreeMap<String, String>,
    remote: &'a BTreeMap<String, (String, Option<String>)>,
    existing_copies: &'a HashMap<String, Vec<(String, String)>>,
}

/// 只帶變更的同步包省略的檔：在本機目錄與回收區的全部材料裡依指紋找。
struct CandidateFinder {
    by_sha: HashMap<String, Vec<PathBuf>>,
    verified: HashMap<PathBuf, Option<String>>,
}

impl CandidateFinder {
    fn new(
        lib: &LocalLibrary,
        _local: &BTreeMap<String, (Place, String, PaperFile)>,
    ) -> CandidateFinder {
        let mut by_sha: HashMap<String, Vec<PathBuf>> = HashMap::new();
        let mut add = |dir: PathBuf, p: &PaperFile| {
            for m in &p.materials {
                if m.form == OVERLAY_FORM || !is_valid_material_path(&m.path) {
                    continue;
                }
                if let Some(s) = &m.sha256 {
                    by_sha.entry(s.clone()).or_default().push(dir.join(&m.path));
                }
            }
        };
        if let Ok(fresh) = super::scan::scan_folders(&lib.papers_dir(), None) {
            for f in fresh.values() {
                if let Some(p) = &f.paper {
                    add(lib.papers_dir().join(&f.name), p);
                }
            }
        }
        let trash = lib.root.join(TRASH_DIR);
        for name in names_in_or_empty(&trash) {
            if let Ok((p, _)) = read_json::<PaperFile>(&trash.join(&name).join(PAPER_FILE)) {
                add(trash.join(&name), &p);
            }
        }
        let _ = PAPERS_DIR;
        CandidateFinder {
            by_sha,
            verified: HashMap::new(),
        }
    }

    /// 匯入那邊的檔從哪裡來：放進來的用 `in/` 裡的；省略的在本機依指紋找（實際重算指紋）。
    /// 交回（來源, 預期的指紋）；找不到是 `None`。
    fn resolve(
        &mut self,
        tree: &RemoteTree,
        rel: &str,
        sha: Option<&str>,
    ) -> Option<(Src, String)> {
        let entry = tree.entry(rel)?;
        if entry.included {
            return Some((
                Src::Remote(rel.to_string()),
                entry.sha256.clone().unwrap_or_default(),
            ));
        }
        let want = entry.sha256.as_deref().or(sha)?;
        for p in self.by_sha.get(want).cloned().unwrap_or_default() {
            let got = self
                .verified
                .entry(p.clone())
                .or_insert_with(|| file_sha(&p))
                .clone();
            if got.as_deref() == Some(want) {
                return Some((Src::Local(p), want.to_string()));
            }
        }
        None
    }
}
