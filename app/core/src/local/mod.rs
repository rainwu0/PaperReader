//! 裝置本機的文獻庫：直接用一般檔案操作讀寫一個資料夾。
//!
//! 檔案是全部的資料；目錄只存在記憶體裡，開啟與重新掃描時由磁碟建立。

mod add;
mod changes;
mod check;
mod digest;
mod edit;
mod events;
mod export;
mod generated;
mod groups;
mod import;
mod ink;
mod notes;
mod overlay;
mod pages;
mod record;
mod references;
mod remove;
mod scan;
mod status;
mod sync_export;
mod sync_import;
mod text;
mod translation;
mod trash;
mod zipout;

pub use crate::store::{AddOutcome, EventFilter};
pub use import::ImportCopyHookFn;
pub use notes::is_valid_notes_dir;

use crate::error::{LibError, Result, io_error};
use crate::format::{FORMAT_VERSION, LibraryFile, Material, PaperFile};
use crate::ids::{new_id, now_utc};
use crate::jsonio::{VersionToken, read_json, sync_dir, write_json_atomic};
use crate::naming::{last_segment, numbered_names};
use crate::store::{MaterialStatus, MaterialSummary, PaperSummary};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const LIBRARY_FILE: &str = "library.json";
pub(crate) const PAPER_FILE: &str = "paper.json";
pub(crate) const PAPERS_DIR: &str = "papers";
pub(crate) const TRASH_DIR: &str = ".trash";
/// 應用自己的暫存項目一律以這個開頭；應用只清理這個開頭的項目。
pub(crate) const TEMP_PREFIX: &str = ".pr-";
/// 每篇固定一個、不列在材料清單的檔：檢查認得它們、是同步包的候選（`text.json` 在
/// 「只帶變更」時另看修改時間，`sync_export.rs`）。匯入不經這裡，每一種在 `sync_import/units.rs` 各自合併。
pub(crate) const PAPER_SIDE_FILES: [&str; 3] = [
    crate::ink::INK_FILE,
    crate::references::REFERENCES_FILE,
    crate::text::TEXT_FILE,
];

/// `papers/` 底下的一個資料夾在記憶體裡的樣子。
#[derive(Debug, Clone)]
pub(crate) struct Folder {
    pub name: String,
    /// `paper.json` 的修改時間與大小，重新掃描時用來判斷要不要重讀。
    pub mtime: Option<SystemTime>,
    pub size: u64,
    /// `None`：`paper.json` 讀不懂。
    pub paper: Option<PaperFile>,
}

#[derive(Debug)]
pub struct LocalLibrary {
    pub(crate) root: PathBuf,
    pub(crate) device_id: String,
    pub(crate) read_only: bool,
    pub(crate) library: LibraryFile,
    /// 依資料夾名稱排序。
    pub(crate) folders: BTreeMap<String, Folder>,
    /// 閱讀事件的整理結果，以文獻代號為鍵，與目錄分開存。
    pub(crate) events: events::EventIndex,
    /// 筆記檔的索引（文獻代號 → 檔名），只存在記憶體裡。
    pub(crate) notes_index: notes::NotesIndex,
    /// 新建筆記文字用的標題與連結文字，由畫面設定。
    pub(crate) notes_labels: crate::notes::NotesLabels,
    /// 測試掛勾：加入譯本複製完之後、檢查檔頭之前呼叫一次（模擬「複製期間別處改了文獻資料夾」，
    /// 在不允許改名開著檔案的資料夾的平台上，資料流裡做不到）。
    pub(crate) translation_copy_hook: CopyHook,
    /// 測試掛勾：新材料挑好檔名之後、確認那個名稱沒被占用之前呼叫一次，參數是要放的位置
    /// （模擬「挑好之後別處剛放了同名的檔」）。
    pub(crate) material_name_hook: PathHook,
    /// 測試掛勾：每附加一筆變更記錄之後呼叫一次（`set_change_hook`）。
    pub(crate) change_hook: ChangeHook,
    /// 目前的匯入預覽（同一時間只有一份）。
    pub(crate) pending_sync: Option<sync_import::PendingSync>,
    /// 有寫到一半的合併時，它的暫存資料夾：計畫做完之前文獻庫唯讀。
    pub(crate) unfinished_sync: Option<PathBuf>,
    /// 開啟時自動做完的合併的回報（`resume_sync_import` 交回它）。
    pub(crate) resumed_report: Option<crate::store::SyncImportReport>,
    /// 測試掛勾：匯入寫入時每做計畫的一項之前呼叫一次。
    pub(crate) sync_apply_hook: ApplyHook,
    /// 測試掛勾：背景寫入（`update_paper_background`、`write_references`）在「寫之前再讀一次」之前呼叫，
    /// 參數是要寫的檔（模擬「讀了之後別處改了那份檔」）。不會自動清掉。
    pub(crate) background_write_hook: PathHook,
    /// 目前的整批匯入計畫（同一時間只有一份）。
    pub(crate) pending_import: Option<import::PendingImport>,
    /// 測試掛勾：複製匯入的檔之前呼叫（`set_import_copy_hook`）。
    pub(crate) import_copy_hook: import::ImportCopyHook,
}

/// 匯入寫入的測試掛勾外框（同 `CopyHook`）。
#[derive(Default)]
pub(crate) struct ApplyHook(pub(crate) Option<crate::changes::ApplyHookFn>);

impl std::fmt::Debug for ApplyHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "ApplyHook(set)"
        } else {
            "ApplyHook(none)"
        })
    }
}

/// 變更記錄的測試掛勾外框（同 `CopyHook`）。
#[derive(Default)]
pub(crate) struct ChangeHook(pub(crate) Option<crate::changes::ChangeHookFn>);

impl std::fmt::Debug for ChangeHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "ChangeHook(set)"
        } else {
            "ChangeHook(none)"
        })
    }
}

/// 讓 `LocalLibrary` 仍可以 `Debug`、`Send` 的測試掛勾外框。
#[derive(Default)]
pub(crate) struct CopyHook(pub(crate) Option<Box<dyn FnMut() + Send>>);

/// 帶一個路徑參數的測試掛勾（`set_material_name_hook`）。
#[doc(hidden)]
pub type PathHookFn = Box<dyn FnMut(&Path) + Send>;

/// `PathHookFn` 的外框（同 `CopyHook`）。
#[derive(Default)]
pub(crate) struct PathHook(pub(crate) Option<PathHookFn>);

impl std::fmt::Debug for PathHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "PathHook(set)"
        } else {
            "PathHook(none)"
        })
    }
}

impl std::fmt::Debug for CopyHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "CopyHook(set)"
        } else {
            "CopyHook(none)"
        })
    }
}

impl LocalLibrary {
    /// 開啟文獻庫。順序：建立根目錄 → 讀或建立 `library.json` → `format` 較新就設為唯讀
    /// → 非唯讀時清掉自己的暫存項目 → 掃描。
    pub fn open(root: &Path, device_id: &str) -> Result<LocalLibrary> {
        Self::open_inner(root, device_id, true)
    }

    /// 只給命令列的測試用：開啟時不把寫到一半的合併做完（文獻庫因此唯讀，計畫留著）。
    #[doc(hidden)]
    pub fn open_without_resume(root: &Path, device_id: &str) -> Result<LocalLibrary> {
        Self::open_inner(root, device_id, false)
    }

    fn open_inner(root: &Path, device_id: &str, resume: bool) -> Result<LocalLibrary> {
        if !crate::ids::is_valid_id(device_id) {
            return Err(LibError::InvalidInput {
                reason: format!("device id must be 26 Crockford Base32 characters: {device_id:?}"),
            });
        }
        fs::create_dir_all(root).map_err(|e| io_error(root, e))?;

        let lib_path = root.join(LIBRARY_FILE);
        let (library, read_only) = match fs::metadata(&lib_path) {
            Ok(_) => {
                let (lf, _) = read_json::<LibraryFile>(&lib_path)?;
                let ro = lf.format > FORMAT_VERSION;
                (lf, ro)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let lf = LibraryFile {
                    format: FORMAT_VERSION,
                    id: new_id(),
                    created: now_utc(),
                    notes_dir: "notes".to_string(),
                    groups: serde_json::json!([]),
                    deleted_groups: serde_json::json!([]),
                    extra: Default::default(),
                };
                write_json_atomic(&lib_path, &lf)?;
                (lf, false)
            }
            Err(e) => return Err(io_error(&lib_path, e)),
        };

        let papers = root.join(PAPERS_DIR);
        let mut lib = LocalLibrary {
            root: root.to_path_buf(),
            device_id: device_id.to_string(),
            read_only,
            library,
            folders: BTreeMap::new(),
            events: events::EventIndex::default(),
            notes_index: notes::NotesIndex::default(),
            notes_labels: Default::default(),
            translation_copy_hook: CopyHook::default(),
            material_name_hook: PathHook::default(),
            change_hook: ChangeHook::default(),
            pending_sync: None,
            unfinished_sync: None,
            resumed_report: None,
            sync_apply_hook: ApplyHook::default(),
            background_write_hook: PathHook::default(),
            pending_import: None,
            import_copy_hook: import::ImportCopyHook::default(),
        };
        if !read_only {
            if !papers.exists() {
                fs::create_dir_all(&papers).map_err(|e| io_error(&papers, e))?;
            }
            // 寫到一半的合併：在清理暫存項目之前先把計畫做完；重做又失敗就唯讀開啟、不清它
            if let Some(dir) = sync_import::find_plan_dir(root) {
                if !resume {
                    lib.unfinished_sync = Some(dir);
                } else {
                    match lib.resume_at(&dir) {
                        Ok(report) => lib.resumed_report = Some(report),
                        Err(_) => lib.unfinished_sync = Some(dir),
                    }
                }
            }
            clean_temp_items(root, &papers, lib.unfinished_sync.as_deref());
            // `notes_dir` 不合規則時不清理、不讀（不碰文獻庫外面或別處的東西）
            notes::clean_notes_dir_temp(root, &lib.library.notes_dir);
        }
        lib.notes_index = notes::scan_notes_dir(&lib.root, &lib.library.notes_dir, None);
        lib.folders = scan::scan_folders(&lib.papers_dir(), None)?;
        lib.events = events::EventIndex::load(&lib.root, &lib.device_id)?;
        Ok(lib)
    }

    /// 只給測試用：設定「加入譯本複製完之後呼叫一次」的掛勾（呼叫後自動清掉）。
    #[doc(hidden)]
    pub fn set_translation_copy_hook(&mut self, hook: Option<Box<dyn FnMut() + Send>>) {
        self.translation_copy_hook = CopyHook(hook);
    }

    /// 只給測試用：設定「新材料挑好檔名之後、確認那個名稱沒被占用之前呼叫一次」的掛勾，參數是要放的位置
    /// （呼叫後自動清掉）。加入譯本、存產生的譯本（第一次）、建立疊加檔、建立重點筆記都經過這裡。
    #[doc(hidden)]
    pub fn set_material_name_hook(&mut self, hook: Option<PathHookFn>) {
        self.material_name_hook = PathHook(hook);
    }

    /// 只給測試用：背景寫入在「寫之前再讀一次」之前呼叫的掛勾，參數是要寫的檔（`None` 清掉）。
    #[doc(hidden)]
    pub fn set_background_write_hook(&mut self, hook: Option<PathHookFn>) {
        self.background_write_hook = PathHook(hook);
    }

    /// 背景寫入在「寫之前再讀一次」之前呼叫測試掛勾。
    pub(crate) fn before_background_recheck(&mut self, path: &Path) {
        if let Some(hook) = self.background_write_hook.0.as_mut() {
            hook(path);
        }
    }

    /// `format` 較新，或有寫到一半的合併。
    pub fn is_read_only(&self) -> bool {
        self.read_only || self.unfinished_sync.is_some()
    }

    pub fn library_file(&self) -> &LibraryFile {
        &self.library
    }

    /// 目錄：依資料夾名稱排序。材料的狀態在呼叫的當下查檔案。
    pub fn list_papers(&self) -> Vec<PaperSummary> {
        self.catalog()
            .into_iter()
            .map(|(folder, paper)| PaperSummary {
                id: paper.id.clone(),
                folder: folder.name.clone(),
                bib: paper.bib.clone(),
                materials: paper
                    .materials
                    .iter()
                    .map(|m| MaterialSummary {
                        id: m.id.clone(),
                        role: m.role.clone(),
                        form: m.form.clone(),
                        pages: m.pages,
                        status: material_status(&self.papers_dir().join(&folder.name), m),
                        sha256: m.sha256.clone(),
                    })
                    .collect(),
                needs_newer_app: paper.format > FORMAT_VERSION,
                progress: self.progress_of(paper),
                added: paper.added.clone(),
                groups: paper.groups.clone(),
                has_notes: self.notes_index.has_notes(&paper.id),
            })
            .collect()
    }

    /// 每次都從磁碟讀；版本記號由檔案當時的內容算出。讀出來的代號一定是要求的那一個
    /// （資料夾被改名或對調過時，會重新找，見 `locate`）。
    pub fn get_paper(&self, id: &str) -> Result<(PaperFile, VersionToken)> {
        let l = self.locate(id)?;
        Ok((l.paper, l.token))
    }

    /// 可供讀取的材料檔位置。路徑不合法交回 `InvalidInput`，檔案不存在交回 `NotFound`。
    pub fn material_path(&self, paper_id: &str, material_id: &str) -> Result<PathBuf> {
        let l = self.locate(paper_id)?;
        let folder_name = l.folder;
        let paper = l.paper;
        let m = paper
            .materials
            .iter()
            .find(|m| m.id == material_id)
            .ok_or_else(|| LibError::NotFound {
                what: format!("material {material_id}"),
            })?;
        let dir = self.papers_dir().join(&folder_name);
        if !is_valid_material_path(&m.path) {
            return Err(LibError::InvalidInput {
                reason: format!("invalid material path: {}", m.path),
            });
        }
        let full = dir.join(&m.path);
        match fs::metadata(&full) {
            Ok(_) => Ok(full),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(LibError::NotFound {
                what: format!("file {}", m.path),
            }),
            Err(e) => Err(io_error(&full, e)),
        }
    }

    // ---- 內部共用 ----

    pub(crate) fn papers_dir(&self) -> PathBuf {
        self.root.join(PAPERS_DIR)
    }

    pub(crate) fn require_writable(&self) -> Result<()> {
        if self.read_only || self.unfinished_sync.is_some() {
            Err(LibError::ReadOnly)
        } else {
            Ok(())
        }
    }

    /// 目錄裡實際列入的文獻：讀得懂、代號沒有重複（依資料夾名稱排序，第一個算數）。
    pub(crate) fn catalog(&self) -> Vec<(&Folder, &PaperFile)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for f in self.folders.values() {
            if let Some(p) = &f.paper
                && seen.insert(p.id.as_str())
            {
                out.push((f, p));
            }
        }
        out
    }

    /// 找出代號對應的文獻，並確認磁碟上那個資料夾裡的 `paper.json` 真的就是它。
    ///
    /// 記憶體裡的目錄可能已經過期：使用者可能在應用開著的時候，用檔案管理員把資料夾改了名或對調了。
    /// 所以先讀記憶體裡記的那個資料夾的 `paper.json`，代號對得上才用；對不上或讀不到，就從磁碟重新掃描、
    /// 再找一次；還是找不到交回 `NotFound`，不對任何資料夾動作。
    /// 這個版本只讀不改記憶體（供 `&self` 的操作用）；會動檔案的操作用 `locate_mut`。
    pub(crate) fn locate(&self, id: &str) -> Result<Located> {
        if let Some(l) = self.read_in_memory_folder(id) {
            return Ok(l);
        }
        let fresh = scan::scan_folders(&self.papers_dir(), None)?;
        self.read_folder_for(&fresh, id)
    }

    /// 同 `locate`，但需要重新找的時候，把重新掃描的結果存回記憶體裡的目錄。
    /// 重新掃描一律重讀每份 `paper.json`，不靠修改時間與大小：資料夾對調之後，兩份描述檔
    /// 可能一樣大、修改時間也一樣，靠它們判斷會把兩篇搞混。
    pub(crate) fn locate_mut(&mut self, id: &str) -> Result<Located> {
        if let Some(l) = self.read_in_memory_folder(id) {
            return Ok(l);
        }
        self.folders = scan::scan_folders(&self.papers_dir(), None)?;
        self.read_folder_for(&self.folders, id)
    }

    /// 寫完 `paper.json` 之後更新記憶體裡那一篇（新的內容、修改時間、大小），不必整個重掃。
    pub(crate) fn remember_paper(&mut self, folder_name: &str, paper: PaperFile) -> Result<()> {
        let path = self.papers_dir().join(folder_name).join(PAPER_FILE);
        let meta = fs::metadata(&path).map_err(|e| io_error(&path, e))?;
        self.folders.insert(
            folder_name.to_string(),
            Folder {
                name: folder_name.to_string(),
                mtime: meta.modified().ok(),
                size: meta.len(),
                paper: Some(paper),
            },
        );
        Ok(())
    }

    /// 「連帶或背景的寫入」：自己讀、改、寫；寫入前再讀一次比版本記號，
    /// 不同就從頭重做一次，仍不同回報 `Conflict`。`change` 交回 `false` 表示不需要寫。
    /// 檢查順序：文獻不在目錄 → `NotFound`；`format` 較新 → `FormatTooNew`；然後才是 `change`。
    /// 呼叫的一方自己先檢查唯讀。交回新的版本記號（沒寫時是 `None`）。
    pub(crate) fn update_paper_background<F>(
        &mut self,
        paper_id: &str,
        mut change: F,
    ) -> Result<Option<VersionToken>>
    where
        F: FnMut(&mut PaperFile) -> Result<bool>,
    {
        for _ in 0..2 {
            let located = self.locate_mut(paper_id)?;
            let mut paper = located.paper;
            if paper.format > FORMAT_VERSION {
                return Err(LibError::FormatTooNew {
                    found: paper.format,
                });
            }
            if !change(&mut paper)? {
                return Ok(None);
            }
            let path = self.papers_dir().join(&located.folder).join(PAPER_FILE);
            self.before_background_recheck(&path);
            // 寫入前再讀一次：磁碟上的內容與讀到的不同，從頭重做
            match fs::read(&path) {
                Ok(bytes) if crate::jsonio::sha256_hex(&bytes) == located.token.0 => {}
                _ => continue,
            }
            let token = write_json_atomic(&path, &paper)?;
            self.remember_paper(&located.folder, paper)?;
            return Ok(Some(token));
        }
        Err(LibError::Conflict)
    }

    /// 「由畫面發起、畫面先讀過那份檔」的寫入在動手之前的檢查，先後固定：唯讀 → `ReadOnly`；
    /// 不在目錄 → `NotFound`；`token`（那篇 `paper.json` 的版本記號）不符 → `Conflict`；`format` 較新 →
    /// `FormatTooNew`。還沒寫任何東西。帶 `paper.json` 記號的寫入入口一律從這裡開始。
    pub(crate) fn locate_for_write(
        &mut self,
        paper_id: &str,
        token: &VersionToken,
    ) -> Result<Located> {
        self.require_writable()?;
        let located = self.locate_mut(paper_id)?;
        if &located.token != token {
            return Err(LibError::Conflict);
        }
        if located.paper.format > FORMAT_VERSION {
            return Err(LibError::FormatTooNew {
                found: located.paper.format,
            });
        }
        Ok(located)
    }

    /// 來源複製進文獻資料夾的暫存檔 `tmp` 之後再確認一次：`token` 不符，或資料夾名稱不再是 `folder`（複製期間
    /// 被改名，暫存檔跟著搬到新名稱的資料夾、舊路徑已不存在）→ 新位置的暫存檔也清掉、`Conflict`。
    /// 交回剛重讀的那篇。
    pub(crate) fn relocate_after_copy(
        &mut self,
        paper_id: &str,
        token: &VersionToken,
        folder: &str,
        tmp: &Path,
    ) -> Result<Located> {
        let again = self.locate_mut(paper_id)?;
        if &again.token != token || again.folder != folder {
            if let Some(name) = tmp.file_name() {
                let _ = fs::remove_file(self.papers_dir().join(&again.folder).join(name));
            }
            return Err(LibError::Conflict);
        }
        Ok(again)
    }

    /// 新材料的檔已經放在 `papers/<folder>/<material.path>`：材料清單加這一項、寫 `paper.json`。清單寫不成時
    /// 盡力拿掉剛放的檔（是我們放的，不留下清單沒列的檔）；寫成之後資料夾落盤、更新記憶體裡的目錄。
    /// 交回 `paper.json` 的新記號。加一項材料的寫入一律從這裡收尾。
    pub(crate) fn commit_new_material(
        &mut self,
        folder: &str,
        mut paper: PaperFile,
        material: Material,
    ) -> Result<VersionToken> {
        let dir = self.papers_dir().join(folder);
        let placed = dir.join(&material.path);
        paper.materials.push(material);
        let token = match write_json_atomic(&dir.join(PAPER_FILE), &paper) {
            Ok(t) => t,
            Err(e) => {
                let _ = fs::remove_file(&placed);
                return Err(e);
            }
        };
        sync_dir(&dir);
        self.remember_paper(folder, paper)?;
        Ok(token)
    }

    fn read_in_memory_folder(&self, id: &str) -> Option<Located> {
        let name = &find_folder(&self.folders, id)?.name;
        self.read_candidate(name, id)
    }

    fn read_folder_for(&self, folders: &BTreeMap<String, Folder>, id: &str) -> Result<Located> {
        find_folder(folders, id)
            .and_then(|f| self.read_candidate(&f.name, id))
            .ok_or_else(|| LibError::NotFound {
                what: format!("paper {id}"),
            })
    }

    /// 讀 `papers/<資料夾>/paper.json`；讀不到、讀不懂、或代號不是要的那一個，都是 `None`。
    fn read_candidate(&self, folder: &str, id: &str) -> Option<Located> {
        let path = self.papers_dir().join(folder).join(PAPER_FILE);
        match read_json::<PaperFile>(&path) {
            Ok((paper, token)) if paper.id == id => Some(Located {
                folder: folder.to_string(),
                paper,
                token,
            }),
            _ => None,
        }
    }
}

/// `locate` 的結果：資料夾名稱、剛從磁碟讀到的描述檔與它的版本記號。
pub(crate) struct Located {
    pub folder: String,
    pub paper: PaperFile,
    pub token: VersionToken,
}

/// 在一份資料夾表裡找代號對應的資料夾：讀得懂、代號重複時依資料夾名稱排序第一個算數。
fn find_folder<'a>(folders: &'a BTreeMap<String, Folder>, id: &str) -> Option<&'a Folder> {
    folders
        .values()
        .find(|f| f.paper.as_ref().is_some_and(|p| p.id == id))
}

/// 新材料挑檔名試幾次（`<名稱>.<副檔名>`、`<名稱>-2.<副檔名>` … `<名稱>-100.<副檔名>`）。
const MAX_NAME_TRIES: usize = 100;

/// 新材料在 `dir` 裡的檔名：`<stem>.<ext>`，被占用時依序 `<stem>-2.<ext>` …。資料夾裡實際的任何項目，
/// 以及 `materials`（那篇清單裡的每一項）的檔名，都算被占用（不分大小寫）：清單列了、檔卻不見的名稱
/// 不可以再挑，免得兩項材料指向同一個檔。核心裡挑材料檔名只用這一個。
pub(crate) fn pick_material_name(
    dir: &Path,
    materials: &[Material],
    stem: &str,
    ext: &str,
) -> Result<String> {
    let mut taken: HashSet<String> = names_in(dir)?.iter().map(|n| n.to_lowercase()).collect();
    taken.extend(
        materials
            .iter()
            .map(|m| last_segment(&m.path).to_lowercase()),
    );
    let dot = if ext.is_empty() {
        String::new()
    } else {
        format!(".{ext}")
    };
    numbered_names(stem, &dot)
        .take(MAX_NAME_TRIES)
        .find(|n| !taken.contains(&n.to_lowercase()))
        .ok_or_else(|| LibError::Io {
            path: dir.display().to_string(),
            reason: format!("no free file name for {stem}{dot} after {MAX_NAME_TRIES} tries"),
        })
}

impl LocalLibrary {
    /// 新材料的檔要放的位置：用 `pick_material_name` 挑檔名，再確認那個名稱仍沒被占用——挑好之後、放進去之前
    /// 又出現了同名的項目（別處剛放的）：`Conflict`，不蓋掉它。交回（檔名, 完整路徑）。
    pub(crate) fn new_material_target(
        &mut self,
        dir: &Path,
        materials: &[Material],
        stem: &str,
        ext: &str,
    ) -> Result<(String, PathBuf)> {
        let name = pick_material_name(dir, materials, stem, ext)?;
        let target = dir.join(&name);
        if let Some(mut hook) = self.material_name_hook.0.take() {
            hook(&target);
        }
        claim_free(&target)?;
        Ok((name, target))
    }
}

/// `target` 已經有任何項目（含符號連結）：`Conflict`。`fs::rename` 與原子寫入都會蓋掉同名的檔，放之前先確認。
fn claim_free(target: &Path) -> Result<()> {
    if fs::symlink_metadata(target).is_ok() {
        Err(LibError::Conflict)
    } else {
        Ok(())
    }
}

/// 資料夾裡每一個項目的名稱（含資料夾與以 `.` 開頭的），依讀到的順序。讀不了是錯誤。
pub(crate) fn names_in(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).map_err(|e| io_error(dir, e))? {
        let e = e.map_err(|e| io_error(dir, e))?;
        out.push(e.file_name().to_string_lossy().into_owned());
    }
    Ok(out)
}

/// 相對於 `root`、用 `/` 分隔的路徑（名稱不是 UTF-8 時用 `to_string_lossy`）。
pub(crate) fn rel_lossy(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// 頁數的合理範圍（含）。
const MIN_PAGES: u32 = 1;
const MAX_PAGES: u32 = 100_000;

/// 交來的總頁數要在合理範圍內。
pub(crate) fn check_pages(pages: u32) -> Result<()> {
    if (MIN_PAGES..=MAX_PAGES).contains(&pages) {
        Ok(())
    } else {
        Err(LibError::InvalidInput {
            reason: format!("pages must be between {MIN_PAGES} and {MAX_PAGES}: {pages}"),
        })
    }
}

/// 材料路徑：相對於文獻資料夾、用 `/` 分隔、不含 `..`、不以 `@` 開頭。
pub(crate) fn is_valid_material_path(path: &str) -> bool {
    if path.is_empty() || path.contains('\0') || path.starts_with('@') {
        return false;
    }
    let norm = path.replace('\\', "/");
    if norm.starts_with('/') {
        return false;
    }
    let bytes = norm.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return false;
    }
    !norm.split('/').any(|seg| seg == "..")
}

pub(crate) fn material_status(folder_dir: &Path, m: &Material) -> MaterialStatus {
    if !is_valid_material_path(&m.path) {
        return MaterialStatus::InvalidPath;
    }
    match fs::metadata(folder_dir.join(&m.path)) {
        Err(_) => MaterialStatus::Missing,
        Ok(meta) if !meta.is_file() => MaterialStatus::Missing,
        Ok(meta) => match m.bytes {
            Some(b) if b != meta.len() => MaterialStatus::SizeMismatch,
            _ => MaterialStatus::Ok,
        },
    }
}

/// 清掉根目錄、`papers/`、每個文獻資料夾這三層裡以 `.pr-` 開頭的項目。
///
/// 盡力而為：某一項清不掉（例如沒有權限、被別的程式開著）就留著、繼續清下一項，
/// 不讓一個清不掉的暫存檔使整個文獻庫打不開。留下來的由「檢查文獻庫」列出（`LeftoverTemp`）。
fn clean_temp_items(root: &Path, papers: &Path, keep: Option<&Path>) {
    remove_temp_in(root, keep);
    // 永久刪除到一半留下的 `.trash/.pr-purge-*`（使用者已經確認要刪的）
    remove_temp_in(&root.join(TRASH_DIR), None);
    // 移除、放回材料時被中斷留下的半成品
    remove::finish_removed_materials(root);
    if !papers.is_dir() {
        return;
    }
    remove_temp_in(papers, None);
    let Ok(rd) = fs::read_dir(papers) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            remove_temp_in(&entry.path(), None);
        }
    }
}

fn remove_temp_in(dir: &Path, keep: Option<&Path>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(TEMP_PREFIX) {
            continue;
        }
        let p = entry.path();
        if keep.is_some_and(|k| k == p) {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let _ = if is_dir {
            fs::remove_dir_all(&p)
        } else {
            fs::remove_file(&p)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 挑好檔名之後、放進去之前那個名稱被占用了（別處剛放了同名的檔、資料夾或連結）：`Conflict`，不蓋掉它。
    /// 加入譯本、存產生的譯本、建立重點筆記、建立疊加檔都有這個檢查。
    #[test]
    fn a_name_taken_after_picking_is_a_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let free = dir.path().join("translation-overlay.json");
        assert!(claim_free(&free).is_ok());
        fs::write(&free, b"someone else's").unwrap();
        assert!(matches!(claim_free(&free), Err(LibError::Conflict)));
        let sub = dir.path().join("digest.md");
        fs::create_dir(&sub).unwrap();
        assert!(matches!(claim_free(&sub), Err(LibError::Conflict)));
        assert_eq!(fs::read(&free).unwrap(), b"someone else's");
    }
}
