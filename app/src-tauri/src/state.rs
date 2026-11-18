//! 應用的狀態：設定檔與開著的文獻庫。核心的 `LocalLibrary` 不是執行緒安全的共用物件，
//! 所以一把鎖鎖住它；耗時的指令在背景執行緒上拿鎖做事（不卡畫面）。
//!
//! 設定檔另用一把鎖：深淺色這類只動設定的指令不必等文獻庫那把鎖（掃描大文獻庫時握著它很久）。
//! 兩把鎖同時要拿時，一律先拿文獻庫的、再拿設定的。

use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::location::{self, Plan, Storage};
use crate::translate::TranslateSettings;
use paperreader_core::error::{LibError, io_error};
use paperreader_core::ids::{is_valid_id, new_id, now_compact};
use paperreader_core::jsonio::write_json_atomic;
use paperreader_core::local::LocalLibrary;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri_plugin_paperreader::StorageInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// 首頁清單的排序依據。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    /// 最近閱讀（預設）。
    #[default]
    Recent,
    /// 加入時間。
    Added,
    Title,
    Author,
    Year,
}

/// 打開應用時先到哪裡。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenTo {
    #[default]
    Home,
    /// 上次讀的那一篇。
    Last,
}

/// 閱讀畫面的工具箱：縮小（邊緣一顆圓鈕）、浮動（拖到哪停在哪）、停靠（吸附成一整列或一欄）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolboxMode {
    #[default]
    Mini,
    Floating,
    Docked,
}

/// 工具箱停靠在哪一邊。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolboxDock {
    Top,
    #[default]
    Bottom,
    Left,
    Right,
}

/// 使用者的偏好：首頁排序、閱讀的選項。每個欄位都有預設值，所以舊的設定檔（沒有這些欄位）照樣讀得懂。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub sort_key: SortKey,
    pub sort_reverse: bool,
    /// 閱讀時頁面跟著深色（只在深色外觀時作用；預設關）。
    pub dark_pages: bool,
    /// 閱讀時螢幕不自動關閉。
    pub keep_awake: bool,
    pub open_to: OpenTo,
    /// 工具箱的狀態與位置（位置用畫面寬高的千分比，存整數）。
    pub toolbox_mode: ToolboxMode,
    pub toolbox_dock: ToolboxDock,
    pub toolbox_x: i32,
    pub toolbox_y: i32,
    /// 設定的「整理文獻庫」（逐篇處理）暫停了；這台裝置記住。
    pub organize_paused: bool,
}

/// 工具箱的預設位置（畫面寬高的千分比）：右側、中間偏下，不蓋住頂列的返回鈕。
/// 與畫面的 `DEFAULT_PREFS`（`src/prefs.ts`）相同。
pub const DEFAULT_TOOLBOX_X: i32 = 900;
pub const DEFAULT_TOOLBOX_Y: i32 = 600;

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            sort_key: SortKey::default(),
            sort_reverse: false,
            dark_pages: false,
            keep_awake: false,
            open_to: OpenTo::default(),
            toolbox_mode: ToolboxMode::default(),
            toolbox_dock: ToolboxDock::default(),
            toolbox_x: DEFAULT_TOOLBOX_X,
            toolbox_y: DEFAULT_TOOLBOX_Y,
            organize_paused: false,
        }
    }
}

/// 存在應用私有空間的設定（`settings.json`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// 這台裝置的代號：第一次啟動時產生，之後不變。
    pub device_id: String,
    #[serde(default)]
    pub storage: Option<Storage>,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub prefs: Prefs,
    /// 文獻庫換到了別的位置（從備份檔還原出來的新位置）；沒有就照 `storage` 的兩個固定位置。
    #[serde(default)]
    pub library_dir: Option<String>,
    /// 「產生」的設定（引擎、模型、目標語言）。金鑰不在這裡，在 Android Keystore 加密的私有空間。
    #[serde(default)]
    pub translate: TranslateSettings,
    /// 搬到共用空間做到一半：在共用空間建任何東西之前寫下，換設定的那一次一起清掉。
    #[serde(default)]
    pub pending_move: Option<PendingMove>,
    /// 搬過去了、應用專屬空間的舊文獻庫還沒刪完：換設定的那一次寫下，刪完才清；
    /// 沒刪完（刪不掉、或刪到一半程序被結束）下次開應用時接著刪。
    #[serde(default)]
    pub move_cleanup: Option<MoveCleanup>,
}

/// 搬過去之後還要刪的。只在設定指向共用空間、那裡的文獻庫開得起來時才刪。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveCleanup {
    /// 應用專屬空間裡的舊文獻庫。
    pub old: String,
    /// 這次的工作資料夾（共用空間的 `Documents/.pr-move-<代號>`）。
    pub work: String,
}

/// 搬到共用空間做到一半的紀錄。程序在途中被結束時，設定仍指向應用專屬空間，
/// 這一筆留著；下次再按「搬」時照它清掉上次留下的半份，不碰別人放的東西。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingMove {
    /// 這次的工作資料夾（共用空間的 `Documents/.pr-move-<代號>`）：暫存的備份檔與還原出來的文獻庫都在這裡。
    pub work: String,
    /// 搬過去的位置（`Documents/PaperReader`）。
    pub target: String,
    /// 搬的是哪一個文獻庫。正式位置裡的文獻庫代號與它相同，才是這次搬過去、還沒換設定的那一份。
    pub library_id: String,
    /// 開始前正式位置已經是一個空資料夾（清的時候留著）。
    pub target_existed: bool,
    /// 開始前正式位置的上層（`Documents`）已經在（清的時候不刪）。
    pub parent_existed: bool,
    /// 核對過、要放到正式位置了（改名之前才記）。沒有這一筆時正式位置上的東西一律不是這次的，不刪：
    /// 匯出或還原途中被結束時，紀錄已在、正式位置上卻還沒有這次的東西。
    #[serde(default)]
    pub placed: bool,
}

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

impl Settings {
    fn fresh() -> Settings {
        Settings {
            device_id: new_id(),
            storage: None,
            theme: Theme::System,
            prefs: Prefs::default(),
            library_dir: None,
            translate: TranslateSettings::default(),
            pending_move: None,
            move_cleanup: None,
        }
    }

    /// 讀設定；檔案不存在就建立。回傳 `(設定, 是否是從壞掉的檔案復原的)`。
    ///
    /// 讀不懂（內容壞了，或是別的版本寫的、有這個版本不認得的值）時不讓啟動失敗：
    /// 原檔改名成 `settings.json.bad-<時間>` 留著，裝置代號能讀出來就保留，其餘用預設值，
    /// 寫出新的設定檔。這樣降版或覆蓋安裝舊版也不會每次打開都閃退。
    pub fn load_or_create(path: &Path) -> ApiResult<(Settings, bool)> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let body = bytes.strip_prefix(&BOM).unwrap_or(&bytes);
                match serde_json::from_slice::<Settings>(body) {
                    Ok(mut s) if is_valid_id(&s.device_id) => {
                        // 舊版的翻譯設定（服務清單以前）讀進來時已轉成服務清單：馬上寫回，只轉一次
                        let old_translate = serde_json::from_slice::<serde_json::Value>(body)
                            .ok()
                            .and_then(|v| v.get("translate").map(|t| t.get("services").is_none()))
                            .unwrap_or(false);
                        // 舊版的預設位置是左上角 (0, 0)，會蓋住返回鈕：當成沒設過，換成現在的預設
                        let old_toolbox = s.prefs.toolbox_x == 0 && s.prefs.toolbox_y == 0;
                        if old_toolbox {
                            s.prefs.toolbox_x = DEFAULT_TOOLBOX_X;
                            s.prefs.toolbox_y = DEFAULT_TOOLBOX_Y;
                        }
                        if old_translate || old_toolbox {
                            write_json_atomic(path, &s)?;
                        }
                        Ok((s, false))
                    }
                    _ => Ok((Self::recover(path, body)?, true)),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let s = Self::fresh();
                write_json_atomic(path, &s)?;
                Ok((s, false))
            }
            Err(e) => Err(io_error(path, e).into()),
        }
    }

    fn recover(path: &Path, body: &[u8]) -> ApiResult<Settings> {
        let value: Option<serde_json::Value> = serde_json::from_slice(body).ok();
        let field = |name: &str| value.as_ref().and_then(|v| v.get(name)).cloned();
        let device_id = field("device_id")
            .and_then(|v| v.as_str().map(String::from))
            .filter(|id| is_valid_id(id))
            .unwrap_or_else(new_id);
        let storage = field("storage").and_then(|v| serde_json::from_value::<Storage>(v).ok());
        let theme = field("theme")
            .and_then(|v| serde_json::from_value::<Theme>(v).ok())
            .unwrap_or_default();
        let prefs = field("prefs")
            .and_then(|v| serde_json::from_value::<Prefs>(v).ok())
            .unwrap_or_default();
        // 原檔留著（裡面可能還有用得上的東西），改不了名也不擋啟動：新檔會把它蓋過去
        let bad = path.with_file_name(format!("settings.json.bad-{}", now_compact()));
        let _ = std::fs::rename(path, bad);
        let library_dir = field("library_dir").and_then(|v| v.as_str().map(String::from));
        let translate = field("translate")
            .and_then(|v| serde_json::from_value::<TranslateSettings>(v).ok())
            .unwrap_or_default();
        // 搬到一半的紀錄要留著：下次按「搬」時靠它認出上次留下的半份
        let pending_move =
            field("pending_move").and_then(|v| serde_json::from_value::<PendingMove>(v).ok());
        let move_cleanup =
            field("move_cleanup").and_then(|v| serde_json::from_value::<MoveCleanup>(v).ok());
        let s = Settings {
            device_id,
            storage,
            theme,
            prefs,
            library_dir,
            translate,
            pending_move,
            move_cleanup,
        };
        write_json_atomic(path, &s)?;
        Ok(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryState {
    NotCreated,
    Ready,
    ReadOnly,
    AccessLost,
    /// 記得文獻庫放在某個位置，但那裡沒有文獻庫了。
    Missing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStatus {
    pub state: LibraryState,
    pub storage: Option<Storage>,
    /// 文獻庫的路徑；還沒建立時是 `null`。顯示在設定頁，不進日誌。
    pub path: Option<String>,
    pub all_files_access: bool,
    /// 唯讀是因為上一次匯入同步包的合併沒做完（不是較新的格式）：畫面給「重試」。
    pub unfinished_sync: bool,
    /// 這次開啟文獻庫時，把上一次沒做完的合併做完了。畫面說了之後呼叫 `sync_resumed_seen`，之後就是 `false`。
    pub resumed_sync: bool,
}

/// 從備份檔還原或從同步包加入時，讀進來的是哪一種檔。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveKind {
    Backup,
    Sync,
}

/// 還原或加入的結果與放的位置。
#[derive(Debug)]
pub struct Placed {
    pub report: paperreader_core::store::RestoreReport,
    pub kind: ArchiveKind,
    /// 原本的文獻庫還留在這裡（目前的文獻庫不是空的，新的放在旁邊）；放在原位時是 `None`。
    pub old: Option<PathBuf>,
    pub new: PathBuf,
}

/// 還原出來的新資料夾的名稱前綴（放在目前文獻庫的旁邊）。
const RESTORED_PREFIX: &str = "PaperReader-restored-";
/// 換位時暫時移開那個空的文獻庫用的名稱前綴。
const REPLACED_PREFIX: &str = ".pr-replaced-";

// ---- 搬到共用空間 ----

/// 搬到共用空間的工作資料夾名稱前綴（放在共用空間的 `Documents` 裡）。
const MOVE_WORK_PREFIX: &str = ".pr-move-";
/// 工作資料夾裡暫存的備份檔、還原出來的文獻庫。
const MOVE_BACKUP: &str = "backup.zip";
const MOVE_LIBRARY: &str = "PaperReader";
/// 估空間時另外留的餘裕（暫存檔、備份清單、檔案系統的零頭）。
const MOVE_RESERVE: u64 = 16 * 1024 * 1024;

/// 搬到共用空間做到哪一步（畫面照它顯示文字與能不能取消）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveStep {
    Idle,
    /// 檢查、清掉上次留下的半份、估空間。
    Prepare,
    /// 把文獻庫匯出成暫存的備份檔（照位元組回報）。
    Export,
    /// 從暫存的備份檔還原到共用空間的工作資料夾（照位元組回報）。
    Copy,
    /// 核對還原出來的與原本的相同。
    Verify,
    /// 放到正式位置、換設定。從這一步起不能取消。
    Switch,
    /// 刪應用專屬空間的舊文獻庫與暫存檔。
    Clean,
    Done,
}

impl MoveStep {
    const ALL: [MoveStep; 8] = [
        MoveStep::Idle,
        MoveStep::Prepare,
        MoveStep::Export,
        MoveStep::Copy,
        MoveStep::Verify,
        MoveStep::Switch,
        MoveStep::Clean,
        MoveStep::Done,
    ];

    fn from_u8(v: u8) -> MoveStep {
        MoveStep::ALL
            .get(usize::from(v))
            .copied()
            .unwrap_or(MoveStep::Idle)
    }
}

/// 搬到共用空間的進度：做到哪一步；`done`／`total` 是匯出加還原的位元組（兩段各算一次文獻庫的大小）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveProgress {
    pub step: MoveStep,
    pub done: u64,
    pub total: u64,
    /// 已經按了取消（下一個段落之間停下）。
    pub cancelling: bool,
}

/// 搬之前的估計（確認框用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MovePlan {
    /// 搬過去的位置。
    pub to: String,
    /// 文獻庫的大小。
    pub bytes: u64,
    /// 需要的空間（暫存的備份檔加還原出來的，再加一點餘裕）。
    pub needed: u64,
    /// 共用空間可用的空間；量不到是 `None`。
    pub free: Option<u64>,
    /// 共用空間已經有文獻庫、這裡的是空的：不搬，改用那一個（`switch_to_shared`）。
    pub adopt: bool,
}

/// 搬完的結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveOutcome {
    /// 使用者在核對之前取消了：什麼都沒換，共用空間那邊這次建的已清掉。
    pub cancelled: bool,
    /// 文獻庫現在的位置（取消時是原本的位置）。
    pub path: String,
    /// 應用專屬空間的舊文獻庫刪掉了。刪不成不算搬移失敗（新的已經在用）。
    pub old_removed: bool,
}

/// 搬移的進度與取消。不放在文獻庫那把鎖後面：搬的時候鎖被握著，畫面照樣讀得到、按得到取消。
#[derive(Default)]
struct MoveCtl {
    step: std::sync::atomic::AtomicU8,
    done: AtomicU64,
    total: AtomicU64,
    cancel: AtomicBool,
    /// 只給測試：改名到正式位置之後、換設定之前，模擬開不成或存不進設定（交回錯誤）。
    #[cfg(test)]
    fail_after_rename: AtomicBool,
    /// 只給測試：改名到正式位置之後、換設定之前，模擬程序被結束（panic）。
    #[cfg(test)]
    panic_after_rename: AtomicBool,
}

/// 這個資料夾裡只有應用自己建的空骨架：根目錄的 `library.json`，加上不含任何檔案的資料夾。
/// 有任何別的檔（使用者放的、別的工具的隱藏項目、捷徑）就不算。不存在也算。
fn only_empty_skeleton(dir: &Path) -> bool {
    fn walk(dir: &Path, root: bool) -> bool {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in rd {
            let Ok(entry) = entry else { return false };
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                return false;
            };
            if meta.is_dir() {
                if !walk(&entry.path(), false) {
                    return false;
                }
            } else if !(root && meta.is_file() && entry.file_name() == "library.json") {
                return false;
            }
        }
        true
    }
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.is_dir() => walk(dir, true),
        Ok(_) => false,
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

/// 不存在，或是空的資料夾：核心可以直接還原、加入到這裡。
fn absent_or_empty_dir(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(mut rd) => rd.next().is_none(),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

/// 拿掉換位時移開的空骨架：只刪 `library.json` 與空的資料夾，遇到別的東西就停（留著不刪）。
fn remove_empty_skeleton(dir: &Path) {
    fn prune(dir: &Path) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    prune(&e.path());
                }
            }
        }
        let _ = std::fs::remove_dir(dir);
    }
    let _ = std::fs::remove_file(dir.join("library.json"));
    prune(dir);
}

/// 依清單的種類放出來（核心的 `peek_kind` 只看清單的 `kind`）：同步包在 `accept_sync` 時加入，否則說「這是同步包」；
/// 備份檔一律從備份檔還原；都不是：加入時說讀不懂，從備份檔還原時說「這不是備份檔」。都只寫到 `target`
/// （不存在或空的資料夾）。
fn unpack(
    archive: &Path,
    target: &Path,
    device_id: &str,
    accept_sync: bool,
) -> ApiResult<(paperreader_core::store::RestoreReport, ArchiveKind)> {
    use paperreader_core::backup::restore_backup;
    use paperreader_core::store::ArchiveKind as Peeked;
    use paperreader_core::sync::join::join_from_sync_package;
    match paperreader_core::sync::package::peek_kind(archive)? {
        Peeked::SyncPackage if accept_sync => join_from_sync_package(archive, target, device_id)
            .map(|r| (r, ArchiveKind::Sync))
            .map_err(crate::error::sync_error),
        Peeked::SyncPackage => Err(ApiError::new(
            ErrorKind::RestoreGotSyncPackage,
            "RestoreGotSyncPackage",
        )),
        Peeked::Backup => restore_backup(archive, target, device_id)
            .map(|r| (r, ArchiveKind::Backup))
            .map_err(crate::error::sync_error),
        Peeked::Other if accept_sync => Err(ApiError::new(ErrorKind::Unreadable, "Unreadable")),
        Peeked::Other => Err(ApiError::new(ErrorKind::NotBackupFile, "NotBackupFile")),
    }
}

struct SettingsBox {
    settings: Settings,
    path: PathBuf,
}

#[derive(Default)]
struct LibSlot {
    lib: Option<LocalLibrary>,
    path: Option<PathBuf>,
}

struct Shared {
    lib: Mutex<LibSlot>,
    settings: Mutex<SettingsBox>,
    /// 啟動時設定檔壞了、已復原：畫面要告知一次。
    recovered: AtomicBool,
    /// 取文獻庫的次數（`lock_lib` 每用一次加一）。外掛的指令拿它判斷要不要比對目錄摘要。
    lib_uses: AtomicU64,
    /// 搬到共用空間的進度與取消。
    moving: MoveCtl,
}

#[derive(Clone)]
pub struct AppState(Arc<Shared>);

fn dir_of(storage: Storage, info: &StorageInfo) -> PathBuf {
    match storage {
        Storage::Shared => PathBuf::from(&info.shared_dir),
        Storage::Private => PathBuf::from(&info.private_dir),
    }
}

impl AppState {
    pub fn new(settings: Settings, settings_path: PathBuf, recovered: bool) -> AppState {
        AppState(Arc::new(Shared {
            lib: Mutex::new(LibSlot::default()),
            settings: Mutex::new(SettingsBox {
                settings,
                path: settings_path,
            }),
            recovered: AtomicBool::new(recovered),
            lib_uses: AtomicU64::new(0),
            moving: MoveCtl::default(),
        }))
    }

    /// 取文獻庫：每一次都把計數加一。指令的樣板、背景工作、開啟、切換、還原都經過這裡，
    /// 所以任何寫入與重新掃描之後，外掛下一次查詢前都會比一次目錄摘要。只加計數，不做別的事。
    fn lock_lib(&self) -> MutexGuard<'_, LibSlot> {
        self.0.lib_uses.fetch_add(1, Ordering::SeqCst);
        self.lock_lib_quiet()
    }

    fn lock_lib_quiet(&self) -> MutexGuard<'_, LibSlot> {
        self.0.lib.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 到現在取了幾次文獻庫（`lock_lib`）。
    pub fn lib_uses(&self) -> u64 {
        self.0.lib_uses.load(Ordering::SeqCst)
    }

    /// 同 `with_lib`，但不加計數：外殼比對目錄摘要、讀每篇給外掛時用，免得比一次就又要再比。
    pub fn with_lib_quiet<T>(
        &self,
        f: impl FnOnce(&mut LocalLibrary) -> Result<T, LibError>,
    ) -> ApiResult<T> {
        let mut g = self.lock_lib_quiet();
        let lib = g.lib.as_mut().ok_or_else(ApiError::no_library)?;
        f(lib).map_err(ApiError::from)
    }

    /// 應用資料夾（設定檔所在的資料夾）。不進文獻庫、不同步、不進備份。
    pub fn app_dir(&self) -> PathBuf {
        self.lock_settings()
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    fn lock_settings(&self) -> MutexGuard<'_, SettingsBox> {
        self.0.settings.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 在開著的文獻庫上做一件事；還沒開就交回 `NoLibrary`。
    pub fn with_lib<T>(
        &self,
        f: impl FnOnce(&mut LocalLibrary) -> Result<T, LibError>,
    ) -> ApiResult<T> {
        let mut g = self.lock_lib();
        let lib = g.lib.as_mut().ok_or_else(ApiError::no_library)?;
        f(lib).map_err(ApiError::from)
    }

    /// 同 `with_lib`，但核心的錯誤照同步包的對應轉換（`reason` 的固定開頭，`error::sync_error`）。
    pub fn with_lib_sync<T>(
        &self,
        f: impl FnOnce(&mut LocalLibrary) -> Result<T, LibError>,
    ) -> ApiResult<T> {
        let mut g = self.lock_lib();
        let lib = g.lib.as_mut().ok_or_else(ApiError::no_library)?;
        f(lib).map_err(crate::error::sync_error)
    }

    /// 指令的樣板：在背景執行緒上做一件事（不卡畫面），失敗時記下錯誤的種類。
    /// 只動文獻庫的用 `lib`、`lib_sync`；這裡給還要讀檔、改設定的。
    pub async fn run<T, F>(&self, command: &'static str, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&AppState) -> ApiResult<T> + Send + 'static,
    {
        let s = self.clone();
        blocking(move || f(&s)).await.map_err(|e| e.log(command))
    }

    /// 在背景執行緒上對開著的文獻庫做一件事（`with_lib`），失敗時記下種類。
    pub async fn lib<T, F>(&self, command: &'static str, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut LocalLibrary) -> Result<T, LibError> + Send + 'static,
    {
        self.run(command, move |s| s.with_lib(f)).await
    }

    /// 同 `lib`，核心的錯誤照同步包的對應轉換（`with_lib_sync`）。
    pub async fn lib_sync<T, F>(&self, command: &'static str, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut LocalLibrary) -> Result<T, LibError> + Send + 'static,
    {
        self.run(command, move |s| s.with_lib_sync(f)).await
    }

    /// 文獻庫開著才往下做（例如開選檔畫面之前）；還沒開交回 `NoLibrary`，不記錄。
    pub async fn require_lib(&self) -> ApiResult<()> {
        let s = self.clone();
        blocking(move || s.with_lib(|_| Ok(()))).await
    }

    /// 啟動時設定檔壞了、已經復原？只回報一次。
    pub fn take_recovered(&self) -> bool {
        self.0.recovered.swap(false, Ordering::SeqCst)
    }

    /// 逐篇處理的紀錄檔：`<應用資料夾>/per-paper/<文獻庫代號>.json`。
    /// 應用資料夾就是設定檔所在的資料夾。不進文獻庫、不同步、不進備份；每個文獻庫一份。
    pub fn per_paper_path(&self) -> ApiResult<PathBuf> {
        let id = self.with_lib(|l| Ok(l.library_file().id.clone()))?;
        if !is_valid_id(&id) {
            return Err(ApiError::new(ErrorKind::InvalidInput, "bad library id"));
        }
        Ok(self.app_dir().join("per-paper").join(format!("{id}.json")))
    }

    pub fn device_id(&self) -> String {
        self.lock_settings().settings.device_id.clone()
    }

    pub fn storage(&self) -> Option<Storage> {
        self.lock_settings().settings.storage
    }

    /// 先存設定，存成功了才換掉記憶體裡的值；存不進去就維持原樣，交回錯誤。
    fn update_settings(&self, change: impl FnOnce(&mut Settings)) -> ApiResult<()> {
        let mut g = self.lock_settings();
        let mut next = g.settings.clone();
        change(&mut next);
        write_json_atomic(&g.path, &next)?;
        g.settings = next;
        Ok(())
    }

    /// 文獻庫的狀況；需要時開啟它。不會在「還沒建立」或「不見了」時建立任何東西。
    pub fn status(&self, info: &StorageInfo) -> ApiResult<LibraryStatus> {
        let mut slot = self.lock_lib();
        let chosen = self.storage();
        let mut missing = false;
        let custom = self.library_dir();
        if slot.lib.is_none() && custom.is_some() {
            // 文獻庫換到了別的位置（從備份檔還原出來的）：那裡有就開，沒有就說不見了
            if let Some(dir) = custom.as_ref().filter(|d| d.join("library.json").is_file()) {
                self.open_dir(&mut slot, dir)?;
            } else {
                missing = true;
            }
        } else if slot.lib.is_none() {
            let has = |dir: PathBuf| dir.join("library.json").is_file();
            let shared_has_library = has(PathBuf::from(&info.shared_dir));
            let chosen_has_library = chosen.is_some_and(|s| has(dir_of(s, info)));
            match location::plan(
                chosen,
                info.all_files_access,
                shared_has_library,
                chosen_has_library,
            ) {
                Plan::Open(s) => self.open_into(&mut slot, s, info)?,
                Plan::Missing(_) => missing = true,
                Plan::NotCreated | Plan::AccessLost => {}
            }
        }
        // 上次搬過去之後舊的還沒刪完：接著刪
        self.resume_cleanup(&slot, info);
        let storage = self.storage();
        let unfinished_sync = slot
            .lib
            .as_ref()
            .is_some_and(|l| l.has_unfinished_sync_import());
        // 開啟時自動做完的合併：留到畫面說了之後呼叫 `sync_resumed_seen` 才清
        let resumed_sync = slot
            .lib
            .as_ref()
            .is_some_and(|l| l.resumed_sync_report().is_some());
        let access_lost = slot.lib.is_none()
            && storage == Some(Storage::Shared)
            && !info.all_files_access
            && !missing;
        let state = match (&slot.lib, access_lost, missing) {
            (Some(l), _, _) if l.is_read_only() => LibraryState::ReadOnly,
            (Some(_), _, _) => LibraryState::Ready,
            (None, true, _) => LibraryState::AccessLost,
            (None, _, true) => LibraryState::Missing,
            (None, false, false) => LibraryState::NotCreated,
        };
        // 不見了的時候顯示「原本在哪」，讓使用者知道要去哪裡找
        let path = match (&slot.path, state, storage) {
            (Some(p), _, _) => Some(p.display().to_string()),
            (None, LibraryState::Missing, _) if custom.is_some() => {
                custom.as_ref().map(|d| d.display().to_string())
            }
            (None, LibraryState::Missing, Some(s)) => Some(dir_of(s, info).display().to_string()),
            _ => None,
        };
        Ok(LibraryStatus {
            state,
            storage,
            path,
            all_files_access: info.all_files_access,
            unfinished_sync,
            resumed_sync,
        })
    }

    /// 在指定的位置建立（或接回）文獻庫，並記住這個選擇。
    pub fn create(&self, storage: Storage, info: &StorageInfo) -> ApiResult<LibraryStatus> {
        if storage == Storage::Shared && !info.all_files_access {
            return Err(ApiError::new(
                ErrorKind::PermissionDenied,
                "all files access is not granted",
            ));
        }
        {
            let mut slot = self.lock_lib();
            self.open_into(&mut slot, storage, info)?;
        }
        self.status(info)
    }

    /// 應用專屬空間的文獻庫是空的（沒有文獻、回收區也是空的）時，改放到共用空間。
    /// 不搬任何東西：共用空間那邊已有文獻庫就接回去，沒有就建一個新的。
    /// 應用專屬空間裡那個空的文獻庫留在原處，不刪。
    pub fn switch_to_shared(&self, info: &StorageInfo) -> ApiResult<LibraryStatus> {
        if !info.all_files_access {
            return Err(ApiError::new(
                ErrorKind::PermissionDenied,
                "all files access is not granted",
            ));
        }
        {
            let mut slot = self.lock_lib();
            let (Some(lib), Some(root)) = (slot.lib.as_ref(), slot.path.as_ref()) else {
                return Err(ApiError::no_library());
            };
            let empty = library_is_empty(lib, root)?;
            if self.storage() != Some(Storage::Private) || !empty {
                return Err(ApiError::new(
                    ErrorKind::InvalidInput,
                    "library is not empty or not private",
                ));
            }
            self.open_into(&mut slot, Storage::Shared, info)?;
        }
        self.status(info)
    }

    pub fn set_theme(&self, theme: Theme) -> ApiResult<()> {
        self.update_settings(|s| s.theme = theme)
    }

    pub fn theme(&self) -> Theme {
        self.lock_settings().settings.theme
    }

    pub fn set_prefs(&self, prefs: Prefs) -> ApiResult<()> {
        self.update_settings(|s| s.prefs = prefs)
    }

    pub fn prefs(&self) -> Prefs {
        self.lock_settings().settings.prefs
    }

    pub fn translate_settings(&self) -> TranslateSettings {
        self.lock_settings().settings.translate.clone()
    }

    /// 改翻譯設定的一部分：在同一把鎖裡讀最新的、改、檢查、寫回。畫面各自只送自己改的欄位，
    /// 不會拿一份舊的整份設定蓋掉別的畫面剛改的東西。
    pub fn update_translate(
        &self,
        change: impl FnOnce(&mut TranslateSettings) -> ApiResult<()>,
    ) -> ApiResult<TranslateSettings> {
        let mut g = self.lock_settings();
        let mut next = g.settings.clone();
        change(&mut next.translate)?;
        next.translate.validate()?;
        write_json_atomic(&g.path, &next)?;
        g.settings = next;
        Ok(g.settings.translate.clone())
    }

    /// 術語庫的檔案：和設定檔放在一起（應用專屬空間，不進文獻庫、不進備份檔）。
    fn glossary_path(&self) -> PathBuf {
        self.lock_settings().path.with_file_name("glossary.json")
    }

    /// 和設定檔放在一起的一份資料檔（術語庫的做法：應用專屬空間，不進文獻庫、不進備份檔）；
    /// 還沒有或讀不懂時是預設值。
    pub fn read_side<T: serde::de::DeserializeOwned + Default>(&self, name: &str) -> T {
        let path = self.lock_settings().path.with_file_name(name);
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<T>(&b).ok())
            .unwrap_or_default()
    }

    /// 改那一份資料檔：讀、改、原子寫回都在設定的鎖裡（翻譯與重點筆記的工作可能同時寫同一份）。
    pub fn update_side<T, R>(&self, name: &str, change: impl FnOnce(&mut T) -> R) -> ApiResult<R>
    where
        T: serde::de::DeserializeOwned + Serialize + Default,
    {
        let g = self.lock_settings();
        let path = g.path.with_file_name(name);
        let mut data: T = match std::fs::read(&path) {
            Ok(b) => match serde_json::from_slice::<T>(&b) {
                Ok(d) => d,
                Err(_) => {
                    // 讀不懂：原檔改名留著（同設定檔的做法），再從空的開始
                    let bad = path.with_file_name(format!("{name}.bad-{}", now_compact()));
                    let _ = std::fs::rename(&path, bad);
                    T::default()
                }
            },
            Err(_) => T::default(),
        };
        let out = change(&mut data);
        write_json_atomic(&path, &data)?;
        drop(g);
        Ok(out)
    }

    /// 使用者的術語庫；還沒有或讀不懂時是空的。
    pub fn glossary(&self) -> Vec<paperreader_translate::glossary::Term> {
        #[derive(Deserialize)]
        struct File {
            terms: Vec<paperreader_translate::glossary::Term>,
        }
        std::fs::read(self.glossary_path())
            .ok()
            .and_then(|b| serde_json::from_slice::<File>(&b).ok())
            .map(|f| f.terms)
            .unwrap_or_default()
    }

    pub fn set_glossary(&self, terms: &[paperreader_translate::glossary::Term]) -> ApiResult<()> {
        #[derive(Serialize)]
        struct File<'a> {
            terms: &'a [paperreader_translate::glossary::Term],
        }
        write_json_atomic(&self.glossary_path(), &File { terms })?;
        Ok(())
    }

    /// 開啟並記住位置。順序：先開、再存設定、設定存好了才把文獻庫放進狀態。
    /// 任何一步失敗，狀態與設定都維持原樣（畫面看到的錯誤與之後問到的狀況一致）。
    fn open_into(&self, slot: &mut LibSlot, storage: Storage, info: &StorageInfo) -> ApiResult<()> {
        let dir = dir_of(storage, info);
        let mut lib = LocalLibrary::open(&dir, &self.device_id())?;
        set_labels(&mut lib);
        if self.storage() != Some(storage) || self.library_dir().is_some() {
            self.update_settings(|s| {
                s.storage = Some(storage);
                s.library_dir = None;
            })?;
        }
        slot.lib = Some(lib);
        slot.path = Some(dir);
        Ok(())
    }

    /// 開啟換過位置的文獻庫（不改設定）。
    fn open_dir(&self, slot: &mut LibSlot, dir: &Path) -> ApiResult<()> {
        let mut lib = LocalLibrary::open(dir, &self.device_id())?;
        set_labels(&mut lib);
        slot.lib = Some(lib);
        slot.path = Some(dir.to_path_buf());
        Ok(())
    }

    /// 開著的文獻庫的位置（匯出、交給別的應用時要把文獻庫裡的相對路徑接成完整路徑）。
    pub fn open_library_root(&self) -> Option<PathBuf> {
        self.lock_lib().path.clone()
    }

    pub fn library_dir(&self) -> Option<PathBuf> {
        self.lock_settings()
            .settings
            .library_dir
            .as_ref()
            .map(PathBuf::from)
    }

    /// 匯出備份檔：寫到 `out`。回報進度的是呼叫的一方傳進來的函式。
    pub fn export_backup(
        &self,
        out: &Path,
        progress: &mut dyn FnMut(u64, u64),
    ) -> ApiResult<paperreader_core::store::ExportReport> {
        self.with_lib(|l| l.export_backup(out, progress))
    }

    /// 從備份檔還原（`accept_sync` 時也接受同步包，即「從另一台加入」），成功後把文獻庫換過去。
    ///
    /// 放在哪裡：
    /// - 目前的文獻庫是空的（只有應用自己建的空骨架，沒有群組），或還沒有文獻庫：放在正式位置——
    ///   目前文獻庫的位置，或還沒有時 `storage` 的那一個（共用空間的 `Documents/PaperReader` 或應用專屬空間）。
    /// - 不是空的：放在旁邊的新資料夾 `PaperReader-restored-<時間>`，原本的留著。
    ///
    /// 換位一律「先還原到旁邊、成功之後才換」：選到壞檔時正式位置原樣不動。換位任一步失敗就退回新資料夾。
    /// `storage_if_none`：還沒有文獻庫時放哪一種空間；`None` 照「剛從授權頁回來」：有權限放共用空間。
    pub fn place_archive(
        &self,
        archive: &Path,
        info: &StorageInfo,
        storage_if_none: Option<Storage>,
        accept_sync: bool,
    ) -> ApiResult<Placed> {
        let mut slot = self.lock_lib();
        let device = self.device_id();
        // 開著文獻庫時照它的空間；還沒有時照指定的，沒指定照原本記得的，都沒有就看權限
        let storage = match (&slot.lib, self.storage()) {
            (Some(_), Some(s)) => s,
            (_, remembered) => storage_if_none
                .or(remembered)
                .unwrap_or_else(|| location::storage_after_permission_step(info.all_files_access)),
        };
        if slot.lib.is_none() && storage == Storage::Shared && !info.all_files_access {
            return Err(ApiError::new(
                ErrorKind::PermissionDenied,
                "all files access is not granted",
            ));
        }
        let official = dir_of(storage, info);
        // 目前的位置與它是不是空的
        let (current, current_empty) = match (&slot.lib, &slot.path) {
            (Some(lib), Some(path)) => {
                let no_groups = lib.list_groups().is_ok_and(|(g, _)| g.is_empty());
                (path.clone(), no_groups && only_empty_skeleton(path))
            }
            _ => (official.clone(), only_empty_skeleton(&official)),
        };

        let ((report, kind), final_dir, old) =
            if slot.lib.is_none() && absent_or_empty_dir(&official) {
                // 還沒有文獻庫、正式位置是空的：直接放進去
                let r = unpack(archive, &official, &device, accept_sync)?;
                (r, official.clone(), None)
            } else {
                let parent = current.parent().ok_or_else(|| {
                    ApiError::new(ErrorKind::InvalidInput, "library has no parent folder")
                })?;
                let staged = parent.join(format!("{RESTORED_PREFIX}{}", now_compact()));
                let r = unpack(archive, &staged, &device, accept_sync)?;
                if current_empty {
                    // 先關掉目前那個空的，再換位；換不成就用旁邊那個新資料夾
                    slot.lib = None;
                    slot.path = None;
                    let aside = parent.join(format!("{REPLACED_PREFIX}{}", new_id()));
                    let swapped = if current.exists() {
                        std::fs::rename(&current, &aside).is_ok()
                            && match std::fs::rename(&staged, &current) {
                                Ok(()) => true,
                                Err(_) => {
                                    let _ = std::fs::rename(&aside, &current);
                                    false
                                }
                            }
                    } else {
                        std::fs::rename(&staged, &current).is_ok()
                    };
                    if swapped {
                        remove_empty_skeleton(&aside);
                        (r, current.clone(), None)
                    } else {
                        let had = current.join("library.json").is_file();
                        (r, staged, had.then(|| current.clone()))
                    }
                } else {
                    let had = current.join("library.json").is_file();
                    (r, staged, had.then(|| current.clone()))
                }
            };

        // 開啟、記住位置（先開、再存設定、設定存好了才放進狀態）
        let mut lib = LocalLibrary::open(&final_dir, &device)?;
        set_labels(&mut lib);
        let custom = (final_dir != official).then(|| final_dir.display().to_string());
        // 存不進設定時，換位已經做了的話原本那個開著的已關掉：狀態留空，下次問狀況時照設定重新開
        self.update_settings(|s| {
            s.storage = Some(storage);
            s.library_dir = custom;
        })?;
        slot.lib = Some(lib);
        slot.path = Some(final_dir.clone());
        Ok(Placed {
            report,
            kind,
            old,
            new: final_dir,
        })
    }
}

/// 量一個位置可用的空間；量不到是 `None`（搬的時候照樣做，空間不夠時照失敗的規則退回）。
pub type FreeSpace<'a> = &'a dyn Fn(&Path) -> Option<u64>;

/// 一個資料夾底下所有檔案的大小總和（估空間、量還原的進度用；讀不到的略過）。
fn tree_bytes(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => tree_bytes(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// 工作資料夾裡各個子資料夾的大小（還原寫出來的；不算最上層的暫存備份檔）。
fn restored_bytes(work: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(work) else {
        return 0;
    };
    rd.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| tree_bytes(&e.path()))
        .sum()
}

/// 往上找到第一個存在的資料夾（量可用空間要量存在的位置）。
fn existing_ancestor(p: &Path) -> PathBuf {
    let mut q = p;
    while !q.exists() {
        match q.parent() {
            Some(up) if !up.as_os_str().is_empty() => q = up,
            _ => break,
        }
    }
    q.to_path_buf()
}

/// 這個位置的文獻庫代號（`library.json` 讀得懂時）。
fn library_id_at(dir: &Path) -> Option<String> {
    let bytes = std::fs::read(dir.join("library.json")).ok()?;
    let body = bytes.strip_prefix(&BOM).unwrap_or(&bytes);
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    v.get("id")?.as_str().map(String::from)
}

/// 空的文獻庫：沒有文獻、回收區是空的、沒有群組、也沒有筆記檔。
fn library_is_empty(lib: &LocalLibrary, root: &Path) -> ApiResult<bool> {
    let has_groups = !lib.list_groups()?.0.is_empty();
    let has_notes = root
        .join("notes")
        .read_dir()
        .is_ok_and(|mut r| r.next().is_some());
    Ok(lib.list_papers().is_empty() && lib.list_trash()?.is_empty() && !has_groups && !has_notes)
}

/// 這是搬移的工作資料夾，可以清：在正式位置的上層、名稱是 `.pr-move-<代號>`。不合就不碰。
fn is_work_dir(work: &Path, target: &Path) -> bool {
    work.parent() == target.parent()
        && work
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix(MOVE_WORK_PREFIX))
            .is_some_and(is_valid_id)
}

/// 紀錄記的工作資料夾（合 `is_work_dir` 才交回）。
fn own_work_dir(record: &PendingMove, target: &Path) -> Option<PathBuf> {
    let work = PathBuf::from(&record.work);
    is_work_dir(&work, target).then_some(work)
}

/// 正式位置上那一份是這個文獻庫上次搬到一半留下的：紀錄記的是這個位置、這個文獻庫、已經放上去了，
/// 那裡的代號也相同。
fn is_own_leftover(record: &PendingMove, target: &Path, library_id: &str) -> bool {
    record.placed
        && Path::new(&record.target) == target
        && record.library_id == library_id
        && library_id_at(target).as_deref() == Some(library_id)
}

/// 要搬的文獻庫。
struct MoveSource {
    root: PathBuf,
    library_id: String,
    bytes: u64,
    empty: bool,
}

/// 應用自己的暫存項目的名稱前綴（核心的寫入流程用；開啟文獻庫時清掉）。不搬、不比。
const APP_TEMP_PREFIX: &str = ".pr-";

/// 相對路徑（用 `/` 接起來，名稱讀不懂的部分換成替代字元；同核心記 `skipped` 的寫法）。
fn join_rel(rel: &str, name: &std::ffi::OsStr) -> String {
    let name = name.to_string_lossy();
    if rel.is_empty() {
        name.into_owned()
    } else {
        format!("{rel}/{name}")
    }
}

/// 備份檔不帶的項目：名稱以「.」開頭的（根目錄的 `.trash` 除外），同核心的匯出（`local/export.rs` 的 `walk`）。
fn hidden_from_backup(name: &std::ffi::OsStr, at_root: bool) -> bool {
    let name = name.to_string_lossy();
    name.starts_with('.') && !(at_root && name == ".trash")
}

/// 把舊的裡備份檔不帶的檔照原樣複製到新的：隱藏的項目整個複製，匯出時略過的檔
/// （`skipped`：名稱讀不懂、路徑不合備份檔的規則、只差大小寫）也複製；`.pr-` 開頭的暫存除外。新的已經有的不覆寫
/// （核對時比內容）。符號連結與特殊檔案搬不了，交回 `MoveMismatch`。讀資料夾出錯不吞掉。
fn copy_left_out(
    old: &Path,
    new: &Path,
    rel: &str,
    hidden: bool,
    skipped: &std::collections::HashSet<String>,
) -> ApiResult<()> {
    let rd = std::fs::read_dir(old).map_err(|e| io_error(old, e))?;
    for entry in rd {
        let entry = entry.map_err(|e| io_error(old, e))?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(APP_TEMP_PREFIX) {
            continue;
        }
        let here = join_rel(rel, &name);
        let hidden = hidden || hidden_from_backup(&name, rel.is_empty());
        let (from, to) = (entry.path(), new.join(&name));
        let kind = entry.file_type().map_err(|e| io_error(&from, e))?;
        if kind.is_dir() {
            if hidden {
                std::fs::create_dir_all(&to).map_err(|e| io_error(&to, e))?;
            }
            copy_left_out(&from, &to, &here, hidden, skipped)?;
        } else if kind.is_file() {
            if (hidden || skipped.contains(&here)) && !to.exists() {
                if let Some(dir) = to.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| io_error(dir, e))?;
                }
                std::fs::copy(&from, &to).map_err(|e| io_error(&to, e))?;
            }
        } else if hidden || skipped.contains(&here) {
            return Err(mismatch("a link or a special file cannot be moved"));
        }
    }
    Ok(())
}

/// 兩個檔的內容逐位元組相同。`b` 不在、不是一般檔案是 `false`。
fn same_content(a: &Path, b: &Path) -> std::io::Result<bool> {
    use std::io::BufRead;
    let mb = match std::fs::metadata(b) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if !mb.is_file() || std::fs::metadata(a)?.len() != mb.len() {
        return Ok(false);
    }
    let mut fa = std::io::BufReader::with_capacity(1 << 16, std::fs::File::open(a)?);
    let mut fb = std::io::BufReader::with_capacity(1 << 16, std::fs::File::open(b)?);
    loop {
        let (ba, bb) = (fa.fill_buf()?, fb.fill_buf()?);
        if ba.is_empty() || bb.is_empty() {
            return Ok(ba.is_empty() && bb.is_empty());
        }
        let n = ba.len().min(bb.len());
        if ba[..n] != bb[..n] {
            return Ok(false);
        }
        fa.consume(n);
        fb.consume(n);
    }
}

/// 整棵比對：舊的每個檔在新的都有、內容相同（`.pr-` 開頭的暫存除外）。
/// 交回第一個不符的相對路徑；舊的裡有符號連結或特殊檔案也算不符（搬不了）。
fn first_difference(old: &Path, new: &Path, rel: &str) -> ApiResult<Option<String>> {
    let rd = std::fs::read_dir(old).map_err(|e| io_error(old, e))?;
    for entry in rd {
        let entry = entry.map_err(|e| io_error(old, e))?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(APP_TEMP_PREFIX) {
            continue;
        }
        let here = join_rel(rel, &name);
        let (from, to) = (entry.path(), new.join(&name));
        let kind = entry.file_type().map_err(|e| io_error(&from, e))?;
        if kind.is_dir() {
            if let Some(found) = first_difference(&from, &to, &here)? {
                return Ok(Some(found));
            }
        } else if !kind.is_file() || !same_content(&from, &to).map_err(|e| io_error(&from, e))? {
            return Ok(Some(here));
        }
    }
    Ok(None)
}

fn mismatch(why: &str) -> ApiError {
    ApiError::new(ErrorKind::MoveMismatch, why.to_string())
}

impl AppState {
    fn pending_move(&self) -> Option<PendingMove> {
        self.lock_settings().settings.pending_move.clone()
    }

    fn set_move_step(&self, step: MoveStep) {
        self.0.moving.step.store(step as u8, Ordering::SeqCst);
    }

    /// 搬之前的檢查（估計與搬都用）：有權限、文獻庫開著、在應用專屬空間、可以寫。
    fn move_source(&self, slot: &LibSlot, info: &StorageInfo) -> ApiResult<MoveSource> {
        if !info.all_files_access {
            return Err(ApiError::new(
                ErrorKind::PermissionDenied,
                "all files access is not granted",
            ));
        }
        let (Some(lib), Some(root)) = (slot.lib.as_ref(), slot.path.as_ref()) else {
            return Err(ApiError::no_library());
        };
        if self.storage() != Some(Storage::Private) {
            return Err(ApiError::new(
                ErrorKind::InvalidInput,
                "the library is not in the app's own space",
            ));
        }
        if lib.is_read_only() {
            return Err(ApiError::new(ErrorKind::ReadOnly, "library is read-only"));
        }
        Ok(MoveSource {
            root: root.clone(),
            library_id: lib.library_file().id.clone(),
            bytes: tree_bytes(root),
            empty: library_is_empty(lib, root)?,
        })
    }

    /// 搬到共用空間之前的估計（確認框用）：要搬去哪裡、要多少空間。不寫任何東西。
    ///
    /// 共用空間那裡不是空的：這裡的文獻庫是空的、那裡是文獻庫時交回 `adopt`（改用那一個，`switch_to_shared`），
    /// 否則 `SharedNotEmpty`。上次搬到一半留下的那一份不算（搬的時候會先清掉）。
    pub fn move_plan(&self, info: &StorageInfo, free: FreeSpace) -> ApiResult<MovePlan> {
        let slot = self.lock_lib();
        let src = self.move_source(&slot, info)?;
        let target = dir_of(Storage::Shared, info);
        let to = target.display().to_string();
        let leftover = self
            .pending_move()
            .is_some_and(|r| is_own_leftover(&r, &target, &src.library_id));
        if !leftover && !absent_or_empty_dir(&target) {
            if src.empty && target.join("library.json").is_file() {
                return Ok(MovePlan {
                    to,
                    bytes: 0,
                    needed: 0,
                    free: None,
                    adopt: true,
                });
            }
            return Err(ApiError::new(
                ErrorKind::SharedNotEmpty,
                "the shared folder is not empty",
            ));
        }
        let needed = src.bytes * 2 + MOVE_RESERVE;
        let free = free(&existing_ancestor(&target));
        if free.is_some_and(|f| f < needed) {
            return Err(ApiError::new(
                ErrorKind::NoSpace,
                "not enough space to move",
            ));
        }
        Ok(MovePlan {
            to,
            bytes: src.bytes,
            needed,
            free,
            adopt: false,
        })
    }

    /// 把應用專屬空間的文獻庫搬到共用空間。`hook` 在每一步開始前呼叫一次
    /// （第二個參數是這次的工作資料夾；正式版不做事，測試在這裡改檔、按取消）。
    ///
    /// 匯出暫存的備份檔 → 還原到共用空間的工作資料夾 → 核對 → 放到正式位置、換設定 → 刪舊的與暫存。
    /// 從頭到換設定都握著文獻庫的鎖：其他寫入要等它做完，舊的不會在搬的途中多出搬不到的變動。
    /// 換設定之前任何一步失敗或取消：設定不換、這次在共用空間建的清掉、舊的原樣不動。
    pub fn move_to_shared(
        &self,
        info: &StorageInfo,
        free: FreeSpace,
        hook: &mut dyn FnMut(MoveStep, &Path),
    ) -> ApiResult<MoveOutcome> {
        let result = self.move_locked(info, free, hook);
        let switched = result.as_ref().is_ok_and(|o| !o.cancelled);
        self.set_move_step(if switched {
            MoveStep::Done
        } else {
            MoveStep::Idle
        });
        result
    }

    fn move_locked(
        &self,
        info: &StorageInfo,
        free: FreeSpace,
        hook: &mut dyn FnMut(MoveStep, &Path),
    ) -> ApiResult<MoveOutcome> {
        let mut slot = self.lock_lib();
        // 拿到鎖才歸零：同時有第二次呼叫時，不蓋掉正在搬的那一次的進度與取消
        let m = &self.0.moving;
        m.cancel.store(false, Ordering::SeqCst);
        m.done.store(0, Ordering::SeqCst);
        m.total.store(0, Ordering::SeqCst);
        self.set_move_step(MoveStep::Prepare);
        let target = dir_of(Storage::Shared, info);
        let parent = target
            .parent()
            .ok_or_else(|| ApiError::new(ErrorKind::InvalidInput, "shared folder has no parent"))?
            .to_path_buf();
        let work = parent.join(format!("{MOVE_WORK_PREFIX}{}", new_id()));
        hook(MoveStep::Prepare, &work);
        let src = self.move_source(&slot, info)?;

        // 上次搬到一半被結束（規則 7）：照紀錄清掉上次的工作資料夾與搬過去還沒換設定的那一份。
        // 清不掉就不開始（紀錄留著，下次再清），免得新的紀錄蓋掉舊的、舊的殘留再也認不出
        if let Some(record) = self.pending_move()
            && !self.clear_leftover(&record, &target, &src.library_id)
        {
            return Err(ApiError::new(
                ErrorKind::Io,
                "what the last move left could not be removed",
            ));
        }
        if !absent_or_empty_dir(&target) {
            return Err(ApiError::new(
                ErrorKind::SharedNotEmpty,
                "the shared folder is not empty",
            ));
        }
        let needed = src.bytes * 2 + MOVE_RESERVE;
        if free(&existing_ancestor(&target)).is_some_and(|f| f < needed) {
            return Err(ApiError::new(
                ErrorKind::NoSpace,
                "not enough space to move",
            ));
        }

        // 在共用空間建任何東西之前先記下來：程序在途中被結束時，下次靠它認出這次留下的
        let record = PendingMove {
            work: work.display().to_string(),
            target: target.display().to_string(),
            library_id: src.library_id.clone(),
            target_existed: target.exists(),
            parent_existed: parent.exists(),
            placed: false,
        };
        self.update_settings(|s| s.pending_move = Some(record.clone()))?;
        match self.move_steps(&mut slot, &src, &record, &work, &target, hook) {
            Ok(Some(outcome)) => Ok(outcome),
            Ok(None) => {
                self.undo_move(&record, &work, &target);
                Ok(MoveOutcome {
                    cancelled: true,
                    path: src.root.display().to_string(),
                    old_removed: false,
                })
            }
            Err(e) => {
                self.undo_move(&record, &work, &target);
                Err(e)
            }
        }
    }

    /// 各步驟。交回 `None` 是在換位置之前取消了（由呼叫的一方清）。
    fn move_steps(
        &self,
        slot: &mut LibSlot,
        src: &MoveSource,
        record: &PendingMove,
        work: &Path,
        target: &Path,
        hook: &mut dyn FnMut(MoveStep, &Path),
    ) -> ApiResult<Option<MoveOutcome>> {
        let m = &self.0.moving;
        let cancelled = || m.cancel.load(Ordering::SeqCst);
        let device = self.device_id();

        // 1. 匯出暫存的備份檔（照 `export_backup` 的做法，握著鎖）
        self.set_move_step(MoveStep::Export);
        hook(MoveStep::Export, work);
        if cancelled() {
            return Ok(None);
        }
        std::fs::create_dir_all(work).map_err(|e| io_error(work, e))?;
        m.total.store(src.bytes * 2, Ordering::SeqCst);
        let zip = work.join(MOVE_BACKUP);
        let lib = slot.lib.as_ref().ok_or_else(ApiError::no_library)?;
        let report = lib.export_backup(&zip, &mut |done, all| {
            m.total.store(all * 2, Ordering::SeqCst);
            m.done.store(done.min(all), Ordering::SeqCst);
        })?;
        // 匯出時略過的檔：核對之前另外照原樣複製過去（隱藏檔不在這張表裡，另外認）
        let skipped: std::collections::HashSet<String> =
            report.skipped.iter().map(|s| s.path.clone()).collect();
        let bytes = report.bytes;
        m.total.store(bytes * 2, Ordering::SeqCst);
        m.done.store(bytes, Ordering::SeqCst);

        // 2. 還原到工作資料夾（不換掉任何東西；進度量寫出來的位元組）
        self.set_move_step(MoveStep::Copy);
        hook(MoveStep::Copy, work);
        if cancelled() {
            return Ok(None);
        }
        let staged = work.join(MOVE_LIBRARY);
        let restored = watch_restore(work, bytes, m, || {
            paperreader_core::backup::restore_backup(&zip, &staged, &device)
        })?;
        m.done.store(bytes * 2, Ordering::SeqCst);

        // 3. 核對
        self.set_move_step(MoveStep::Verify);
        hook(MoveStep::Verify, work);
        if cancelled() {
            return Ok(None);
        }
        // 暫存的備份檔用完了：先放出空間
        let _ = std::fs::remove_file(&zip);
        if restored.library_id != src.library_id || !restored.manifest_mismatches.is_empty() {
            return Err(mismatch("the restored copy differs from the backup"));
        }
        // 備份檔不帶的（別的工具的隱藏檔、匯出時略過的）照原樣複製過去，再整棵比對：
        // 舊的每個檔在新的都有、內容相同，才可以往下（之後會刪掉舊的）
        copy_left_out(&src.root, &staged, "", false, &skipped)?;
        if first_difference(&src.root, &staged, "")?.is_some() {
            return Err(mismatch("a file of the library did not arrive intact"));
        }

        // 4. 放到正式位置、換設定。先改成這一步再看一次取消：收下的取消一定看得到，之後一律不收
        self.set_move_step(MoveStep::Switch);
        if cancelled() {
            return Ok(None);
        }
        hook(MoveStep::Switch, work);
        // 改名之前才記「放上去了」：之後被結束，下次才認得正式位置上那一份是這次的
        self.update_settings(|s| {
            if let Some(p) = s.pending_move.as_mut() {
                p.placed = true;
            }
        })?;
        if record.target_existed {
            // 開始時是空的資料夾（`absent_or_empty_dir`）：只刪空資料夾；失敗時由呼叫的一方建回來
            std::fs::remove_dir(target).map_err(|e| io_error(target, e))?;
        }
        std::fs::rename(&staged, target).map_err(|e| io_error(target, e))?;
        #[cfg(test)]
        if m.panic_after_rename.load(Ordering::SeqCst) {
            panic!("simulated: killed after the rename");
        }
        let opened = (|| -> ApiResult<LocalLibrary> {
            #[cfg(test)]
            if m.fail_after_rename.load(Ordering::SeqCst) {
                return Err(ApiError::new(
                    ErrorKind::Io,
                    "simulated: failed after the rename",
                ));
            }
            let mut lib = LocalLibrary::open(target, &device)?;
            set_labels(&mut lib);
            // 換設定、清掉「搬到一半」、記下「舊的還沒刪」是同一次寫入：之後紀錄不會指著正在用的文獻庫，
            // 刪舊的途中被結束，下次開應用也知道要接著刪
            self.update_settings(|s| {
                s.storage = Some(Storage::Shared);
                s.library_dir = None;
                s.pending_move = None;
                s.move_cleanup = Some(MoveCleanup {
                    old: src.root.display().to_string(),
                    work: work.display().to_string(),
                });
            })?;
            Ok(lib)
        })();
        let new_lib = match opened {
            Ok(lib) => lib,
            Err(e) => {
                // 設定沒換：放上去的這一份是這次的，拿掉（舊的還開著、原樣）。拿不掉時紀錄留著（已記「放上去了」），
                // 下次按「搬」照它再清
                let _ = std::fs::remove_dir_all(target);
                return Err(e);
            }
        };
        drop(slot.lib.replace(new_lib));
        slot.path = Some(target.to_path_buf());

        // 5. 刪舊的與暫存。刪不成不算失敗：新的已經在用（規則 5）；紀錄留著，下次開應用時接著刪
        self.set_move_step(MoveStep::Clean);
        hook(MoveStep::Clean, work);
        let old_removed = self.finish_cleanup(Some(&src.root), Some(work));
        if !old_removed {
            eprintln!("paperreader: move_to_shared: the old library was not removed yet");
        }
        Ok(Some(MoveOutcome {
            cancelled: false,
            path: target.display().to_string(),
            old_removed,
        }))
    }

    /// 換位置之前失敗或取消：清掉這次在共用空間建的（工作資料夾；改名過去又拿不掉的那一份由紀錄認），
    /// 事先存在的空資料夾建回來、原本不在的上層拿掉。都清乾淨了才清掉紀錄；還有留下的就留著紀錄，
    /// 下次按「搬」照它再清。舊的文獻庫沒有動過。
    fn undo_move(&self, record: &PendingMove, work: &Path, target: &Path) {
        let _ = std::fs::remove_dir_all(work);
        if record.target_existed && !target.exists() {
            let _ = std::fs::create_dir(target);
        }
        if !record.parent_existed
            && let Some(parent) = target.parent()
        {
            // 只刪空的（還有別的東西就留著）
            let _ = std::fs::remove_dir(parent);
        }
        let placed_left = library_id_at(target).as_deref() == Some(record.library_id.as_str())
            && self.pending_move().is_some_and(|r| r.placed);
        if !work.exists() && !placed_left {
            let _ = self.update_settings(|s| s.pending_move = None);
        }
    }

    /// 上次搬到一半被結束留下的（規則 7）。只清紀錄指得到、而且確定是這次的：工作資料夾要在正式位置的上層、
    /// 名稱合規則；正式位置上的文獻庫要紀錄記了「放上去了」、和紀錄與現在的文獻庫是同一個代號。其他一律不碰。
    /// 指得到的都清掉了才清紀錄，交回 `true`；清不掉的留著紀錄，交回 `false`。
    fn clear_leftover(&self, record: &PendingMove, target: &Path, library_id: &str) -> bool {
        let mut left = false;
        if Path::new(&record.target) == target {
            if let Some(work) = own_work_dir(record, target) {
                let _ = std::fs::remove_dir_all(&work);
                left |= work.exists();
            }
            if is_own_leftover(record, target, library_id) {
                let _ = std::fs::remove_dir_all(target);
                left |= target.exists();
                if record.target_existed && !target.exists() {
                    let _ = std::fs::create_dir(target);
                }
            }
            if !record.parent_existed
                && let Some(parent) = target.parent()
            {
                let _ = std::fs::remove_dir(parent);
            }
        }
        if !left {
            let _ = self.update_settings(|s| s.pending_move = None);
        }
        !left
    }

    /// 刪搬走之後的舊文獻庫與工作資料夾（`None` 的那一項不碰）；都處理完了才清掉「舊的還沒刪」，
    /// 交回舊的是不是不在了。
    fn finish_cleanup(&self, old: Option<&Path>, work: Option<&Path>) -> bool {
        let gone = |p: Option<&Path>| {
            p.is_none_or(|p| match std::fs::remove_dir_all(p) {
                Ok(()) => true,
                Err(e) => e.kind() == std::io::ErrorKind::NotFound || !p.exists(),
            })
        };
        let old_gone = gone(old);
        let work_gone = gone(work);
        if old_gone && work_gone {
            let _ = self.update_settings(|s| s.move_cleanup = None);
        }
        old_gone
    }

    /// 開應用（問狀況）時，上次搬過去之後舊的還沒刪完：接著刪。只在設定指向共用空間、
    /// 那裡的文獻庫開著時才刪；舊的位置要在應用專屬空間裡、工作資料夾要合規則，不合的不碰、紀錄清掉。
    /// 設定指向應用專屬空間時這一筆已經不對了（那裡就是正在用的），不刪、清掉紀錄。
    fn resume_cleanup(&self, slot: &LibSlot, info: &StorageInfo) {
        let Some(c) = self.lock_settings().settings.move_cleanup.clone() else {
            return;
        };
        let target = dir_of(Storage::Shared, info);
        let on_shared = self.storage() == Some(Storage::Shared)
            && self.library_dir().is_none()
            && slot.lib.is_some()
            && slot.path.as_deref() == Some(target.as_path());
        if !on_shared {
            if self.storage() == Some(Storage::Private) {
                let _ = self.update_settings(|s| s.move_cleanup = None);
            }
            return;
        }
        let old = PathBuf::from(&c.old);
        let app_space = Path::new(&info.private_dir).parent();
        let old_ok = app_space.is_some() && old.parent() == app_space && old != target;
        let work = PathBuf::from(&c.work);
        let work_ok = is_work_dir(&work, &target);
        // 不合規則的那一項不碰、當成處理完了
        self.finish_cleanup(
            old_ok.then_some(old.as_path()),
            work_ok.then_some(work.as_path()),
        );
    }

    /// 搬移的進度（畫面每隔一下來問；不拿文獻庫的鎖）。
    pub fn move_progress(&self) -> MoveProgress {
        let m = &self.0.moving;
        MoveProgress {
            step: MoveStep::from_u8(m.step.load(Ordering::SeqCst)),
            done: m.done.load(Ordering::SeqCst),
            total: m.total.load(Ordering::SeqCst),
            cancelling: m.cancel.load(Ordering::SeqCst),
        }
    }

    /// 取消搬移：換位置之前才收（核對做完、開始換位置之後交回 `false`）。在下一個段落之間停下。
    pub fn cancel_move(&self) -> bool {
        let step = MoveStep::from_u8(self.0.moving.step.load(Ordering::SeqCst));
        let open = matches!(
            step,
            MoveStep::Prepare | MoveStep::Export | MoveStep::Copy | MoveStep::Verify
        );
        if open {
            self.0.moving.cancel.store(true, Ordering::SeqCst);
        }
        open
    }
}

/// 還原的時候在旁邊量工作資料夾寫出了多少（核心的還原沒有進度回報），做完就停。
///
/// 還原途中程式出錯（panic）時也要叫停量的那一條：否則 `scope` 一直等它、文獻庫的鎖一直握著，
/// 整個應用卡住。叫停放在離開時一定會跑的地方。
fn watch_restore(
    work: &Path,
    bytes: u64,
    m: &MoveCtl,
    restore: impl FnOnce() -> Result<paperreader_core::store::RestoreReport, LibError>,
) -> ApiResult<paperreader_core::store::RestoreReport> {
    struct StopOnExit<'a>(&'a AtomicBool, std::thread::Thread);
    impl Drop for StopOnExit<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
            self.1.unpark();
        }
    }
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let watcher = scope.spawn(|| {
            while !stop.load(Ordering::SeqCst) {
                let written = restored_bytes(work).min(bytes);
                m.done.store(bytes + written, Ordering::SeqCst);
                std::thread::park_timeout(std::time::Duration::from_millis(250));
            }
        });
        let _stop = StopOnExit(&stop, watcher.thread().clone());
        restore().map_err(ApiError::from)
    })
}

/// 筆記檔的標題與連結文字（中文）。開啟文獻庫之後設定一次，不會寫進文獻庫。
fn set_labels(lib: &mut LocalLibrary) {
    let _ = lib.set_notes_labels(paperreader_core::notes::NotesLabels {
        thoughts: "整篇的想法".into(),
        highlights: "劃線".into(),
        back_link: "在 PaperReader 開啟".into(),
    });
}

/// 把耗時的工作放到背景執行緒做，等它做完（不佔畫面也不佔主執行緒）。
pub async fn blocking<T, F>(f: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> ApiResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|_| ApiError::new(ErrorKind::Io, "background task failed"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(root: &Path, all_files: bool) -> StorageInfo {
        StorageInfo {
            sdk: 36,
            all_files_access: all_files,
            shared_dir: root.join("Documents/PaperReader").display().to_string(),
            private_dir: root.join("private/PaperReader").display().to_string(),
            cache_dir: root.join("cache").display().to_string(),
        }
    }

    fn state(root: &Path) -> AppState {
        let sp = root.join("settings.json");
        let (s, recovered) = Settings::load_or_create(&sp).unwrap();
        AppState::new(s, sp, recovered)
    }

    /// `usage.json` 這類資料檔讀不懂時，原檔改名留著再重寫，不默默清空。
    #[test]
    fn an_unreadable_side_file_is_kept_aside_before_it_is_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let st = state(dir.path());
        std::fs::write(dir.path().join("usage.json"), b"{broken").unwrap();
        st.update_side("usage.json", |log: &mut crate::translate::UsageLog| {
            log.add(
                "j",
                "p",
                "translation",
                "s",
                paperreader_translate::TokenUsage {
                    input: 1,
                    output: 1,
                },
            )
        })
        .unwrap();
        let kept: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("usage.json.bad-"))
            .collect();
        assert_eq!(kept.len(), 1);
        let back: crate::translate::UsageLog = st.read_side("usage.json");
        assert_eq!(back.records.len(), 1);
    }

    #[test]
    fn first_launch_creates_nothing_but_the_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let st = state(dir.path());
        let s = st.status(&info(dir.path(), false)).unwrap();
        assert_eq!(s.state, LibraryState::NotCreated);
        assert!(!dir.path().join("Documents").exists());
        assert!(!dir.path().join("private").exists());
    }

    #[test]
    fn create_private_then_status_is_ready_and_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let i = info(dir.path(), false);
        let st = state(dir.path());
        let s = st.create(Storage::Private, &i).unwrap();
        assert_eq!(s.state, LibraryState::Ready);
        assert_eq!(s.storage, Some(Storage::Private));
        // 重開應用：從設定檔接回
        let st2 = state(dir.path());
        assert_eq!(st2.status(&i).unwrap().state, LibraryState::Ready);
    }

    #[test]
    fn shared_without_permission_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let st = state(dir.path());
        let e = st
            .create(Storage::Shared, &info(dir.path(), false))
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::PermissionDenied);
        assert!(!dir.path().join("Documents").exists());
    }

    #[test]
    fn revoked_permission_reports_access_lost() {
        let dir = tempfile::tempdir().unwrap();
        let st = state(dir.path());
        st.create(Storage::Shared, &info(dir.path(), true)).unwrap();
        let st2 = state(dir.path());
        let s = st2.status(&info(dir.path(), false)).unwrap();
        assert_eq!(s.state, LibraryState::AccessLost);
    }

    #[test]
    fn reinstall_adopts_existing_shared_library() {
        let dir = tempfile::tempdir().unwrap();
        let i = info(dir.path(), true);
        state(dir.path()).create(Storage::Shared, &i).unwrap();
        // 設定檔不見了（重新安裝）
        std::fs::remove_file(dir.path().join("settings.json")).unwrap();
        let s = state(dir.path()).status(&i).unwrap();
        assert_eq!(s.state, LibraryState::Ready);
        assert_eq!(s.storage, Some(Storage::Shared));
    }

    #[test]
    fn device_id_is_stable_across_launches() {
        let dir = tempfile::tempdir().unwrap();
        let a = state(dir.path()).device_id();
        let b = state(dir.path()).device_id();
        assert_eq!(a, b);
        assert!(is_valid_id(&a));
    }

    #[test]
    fn commands_need_an_open_library() {
        let dir = tempfile::tempdir().unwrap();
        let st = state(dir.path());
        let e = st.with_lib(|l| Ok(l.list_papers().len())).unwrap_err();
        assert_eq!(e.kind, ErrorKind::NoLibrary);
    }

    // ---- 設定檔壞了不讓啟動失敗 ----

    fn bad_files(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("settings.json.bad-"))
            .collect()
    }

    #[test]
    fn garbage_settings_file_does_not_stop_startup_and_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let sp = dir.path().join("settings.json");
        std::fs::write(&sp, b"{ this is not json").unwrap();
        let (s, recovered) = Settings::load_or_create(&sp).unwrap();
        assert!(recovered);
        assert!(is_valid_id(&s.device_id));
        assert_eq!(s.theme, Theme::System);
        // 原檔留著，新檔是合法的
        let bad = bad_files(dir.path());
        assert_eq!(bad.len(), 1);
        assert_eq!(
            std::fs::read(dir.path().join(&bad[0])).unwrap(),
            b"{ this is not json"
        );
        let (again, recovered_again) = Settings::load_or_create(&sp).unwrap();
        assert!(!recovered_again);
        assert_eq!(again.device_id, s.device_id);
    }

    #[test]
    fn unknown_values_from_another_version_keep_the_device_id() {
        let dir = tempfile::tempdir().unwrap();
        let sp = dir.path().join("settings.json");
        let id = new_id();
        std::fs::write(
            &sp,
            format!(r#"{{"device_id":"{id}","storage":"cloud","theme":"dark"}}"#),
        )
        .unwrap();
        let (s, recovered) = Settings::load_or_create(&sp).unwrap();
        assert!(recovered);
        assert_eq!(s.device_id, id, "device id must survive");
        assert_eq!(s.theme, Theme::Dark, "readable fields survive");
        assert_eq!(s.storage, None, "unknown storage value falls back to unset");
        assert_eq!(bad_files(dir.path()).len(), 1);
    }

    #[test]
    fn invalid_device_id_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let sp = dir.path().join("settings.json");
        std::fs::write(&sp, r#"{"device_id":"short"}"#).unwrap();
        let (s, recovered) = Settings::load_or_create(&sp).unwrap();
        assert!(recovered);
        assert!(is_valid_id(&s.device_id));
    }

    #[test]
    fn the_recovery_notice_is_given_once() {
        let dir = tempfile::tempdir().unwrap();
        let sp = dir.path().join("settings.json");
        std::fs::write(&sp, b"garbage").unwrap();
        let (s, recovered) = Settings::load_or_create(&sp).unwrap();
        let st = AppState::new(s, sp, recovered);
        assert!(st.take_recovered());
        assert!(!st.take_recovered());
    }

    // ---- 先存設定再改狀態 ----

    #[test]
    fn failing_to_save_settings_leaves_state_and_status_consistent() {
        let dir = tempfile::tempdir().unwrap();
        // 設定檔放在不存在的資料夾裡：寫不進去
        let st = AppState::new(
            Settings::fresh(),
            dir.path().join("no-such-dir/settings.json"),
            false,
        );
        let i = info(dir.path(), false);
        assert!(st.create(Storage::Private, &i).is_err());
        // 文獻庫沒有被放進狀態，之後問到的狀況和錯誤一致
        assert_eq!(st.storage(), None);
        assert_eq!(st.status(&i).unwrap().state, LibraryState::NotCreated);
        assert_eq!(
            st.with_lib(|l| Ok(l.list_papers().len())).unwrap_err().kind,
            ErrorKind::NoLibrary
        );
    }

    #[test]
    fn failing_to_save_the_theme_keeps_the_old_theme() {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(
            Settings::fresh(),
            dir.path().join("no-such-dir/settings.json"),
            false,
        );
        assert!(st.set_theme(Theme::Dark).is_err());
        assert_eq!(st.theme(), Theme::System);
    }

    // ---- 偏好（排序與閱讀選項）存在設定檔，舊的設定檔照樣讀得懂 ----

    #[test]
    fn a_settings_file_from_before_prefs_still_loads_without_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let sp = dir.path().join("settings.json");
        let id = new_id();
        std::fs::write(
            &sp,
            format!(r#"{{"device_id":"{id}","storage":"private","theme":"dark"}}"#),
        )
        .unwrap();
        let (s, recovered) = Settings::load_or_create(&sp).unwrap();
        assert!(
            !recovered,
            "an old file must not trigger the recovery notice"
        );
        assert_eq!(s.prefs, Prefs::default());
        assert_eq!(s.theme, Theme::Dark);
    }

    #[test]
    fn the_old_top_left_toolbox_position_becomes_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let sp = dir.path().join("settings.json");
        let id = new_id();
        std::fs::write(
            &sp,
            format!(r#"{{"device_id":"{id}","prefs":{{"toolbox_x":0,"toolbox_y":0}}}}"#),
        )
        .unwrap();
        let (s, recovered) = Settings::load_or_create(&sp).unwrap();
        assert!(!recovered);
        assert_eq!(
            (s.prefs.toolbox_x, s.prefs.toolbox_y),
            (DEFAULT_TOOLBOX_X, DEFAULT_TOOLBOX_Y)
        );
        let (again, _) = Settings::load_or_create(&sp).unwrap();
        assert_eq!(
            again.prefs.toolbox_x, DEFAULT_TOOLBOX_X,
            "written back once"
        );
    }

    #[test]
    fn old_translate_settings_are_rewritten_as_a_service_list_once() {
        let dir = tempfile::tempdir().unwrap();
        let sp = dir.path().join("settings.json");
        let id = new_id();
        std::fs::write(
            &sp,
            format!(
                r#"{{"device_id":"{id}","translate":{{"provider":"deepl","target_lang":"zh-TW",
                "profiles":{{"deepl":{{"model":"","endpoint":"https://x.example/"}}}}}}}}"#
            ),
        )
        .unwrap();
        let (s, recovered) = Settings::load_or_create(&sp).unwrap();
        assert!(!recovered);
        assert_eq!(s.translate.primary, "deepl");
        let on_disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&sp).unwrap()).unwrap();
        assert!(
            on_disk["translate"]["services"].is_array(),
            "written back at once"
        );
        assert!(on_disk["translate"].get("profiles").is_none());
        assert_eq!(
            Settings::load_or_create(&sp).unwrap().0.translate,
            s.translate
        );
    }

    #[test]
    fn a_new_install_puts_the_toolbox_away_from_the_top_bar() {
        // 新安裝：工具箱在右側、中間偏下，和畫面的預設相同（不是 0、0 的左上角，那裡是返回鈕）
        let dir = tempfile::tempdir().unwrap();
        let p = state(dir.path()).prefs();
        assert_eq!((p.toolbox_x, p.toolbox_y), (900, 600));
        // 舊的設定檔沒有這兩欄：也用同樣的預設
        let old: Prefs = serde_json::from_str(r#"{"sort_key":"title"}"#).unwrap();
        assert_eq!((old.toolbox_x, old.toolbox_y), (900, 600));
        assert_eq!(old.sort_key, SortKey::Title);
    }

    #[test]
    fn prefs_are_saved_and_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let st = state(dir.path());
        let prefs = Prefs {
            sort_key: SortKey::Title,
            sort_reverse: true,
            dark_pages: true,
            keep_awake: true,
            open_to: OpenTo::Last,
            toolbox_mode: ToolboxMode::Docked,
            toolbox_dock: ToolboxDock::Top,
            toolbox_x: 120,
            toolbox_y: 640,
            organize_paused: true,
        };
        st.set_prefs(prefs).unwrap();
        assert_eq!(state(dir.path()).prefs(), prefs);
    }

    #[test]
    fn restoring_a_backup_switches_to_a_new_place_and_keeps_the_old_one() {
        use paperreader_core::format::BibDraft;
        let dir = tempfile::tempdir().unwrap();
        let i = info(dir.path(), false);
        let st = state(dir.path());
        st.create(Storage::Private, &i).unwrap();
        st.with_lib(|l| {
            let mut body: &[u8] = b"%PDF-1.4 hello";
            l.add_paper_from_reader(&mut body, Some("a.pdf"), BibDraft::default(), None)
        })
        .unwrap();
        let out = dir.path().join("b.zip");
        st.export_backup(&out, &mut |_, _| {}).unwrap();
        let placed = st.place_archive(&out, &i, None, false).unwrap();
        assert_eq!(placed.report.manifest_mismatches.len(), 0);
        assert_eq!(placed.kind, ArchiveKind::Backup);
        let old = placed.old.expect("a library with papers is kept");
        let new = placed.new;
        assert_ne!(old, new);
        assert!(old.join("library.json").is_file(), "the old place stays");
        assert_eq!(st.with_lib(|l| Ok(l.list_papers().len())).unwrap(), 1);
        // 重開應用：設定檔記得換過去的位置
        let st2 = state(dir.path());
        let status = st2.status(&i).unwrap();
        assert_eq!(status.state, LibraryState::Ready);
        assert_eq!(status.path, Some(new.display().to_string()));
    }

    // ---- 還原與加入的位置 ----

    use paperreader_core::store::SyncMode;

    /// 在 `root` 建一個有一篇文獻與一則想法的文獻庫（另一台裝置），匯出完整的同步包與備份檔。
    fn other_device(root: &Path) -> (PathBuf, PathBuf, String) {
        let lib_dir = root.join("other/PaperReader");
        let mut lib = LocalLibrary::open(&lib_dir, &new_id()).unwrap();
        let mut body: &[u8] = b"%PDF-1.4 from the other device";
        let id = match lib
            .add_paper_from_reader(&mut body, Some("other.pdf"), Default::default(), None)
            .unwrap()
        {
            paperreader_core::local::AddOutcome::Added(id) => id,
            _ => unreachable!(),
        };
        lib.set_thoughts(&id, "from the other device", None)
            .unwrap();
        let sync = root.join("other-sync.zip");
        lib.export_sync_package(&sync, SyncMode::Full, Some("Other"), &mut |_, _| {})
            .unwrap();
        let backup = root.join("other-backup.zip");
        lib.export_backup(&backup, &mut |_, _| {}).unwrap();
        (sync, backup, lib.library_file().id.clone())
    }

    fn library_id(st: &AppState) -> String {
        st.with_lib(|l| Ok(l.library_file().id.clone())).unwrap()
    }

    #[test]
    fn joining_on_a_new_device_goes_to_the_shared_documents_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (sync, _, other_id) = other_device(dir.path());
        let i = info(dir.path(), true);
        let st = state(dir.path());
        assert_eq!(st.status(&i).unwrap().state, LibraryState::NotCreated);
        let placed = st.place_archive(&sync, &i, None, true).unwrap();
        assert_eq!(placed.kind, ArchiveKind::Sync);
        assert!(placed.old.is_none());
        let official = dir.path().join("Documents/PaperReader");
        assert_eq!(placed.new, official);
        assert_eq!(library_id(&st), other_id);
        assert_eq!(st.with_lib(|l| Ok(l.list_papers().len())).unwrap(), 1);
        // 筆記檔在正式位置的 notes/（別的應用與 Obsidian 打得開的地方）
        assert!(official.join("notes").read_dir().unwrap().next().is_some());
        // 重開應用：在共用空間、沒有另外記位置
        let st2 = state(dir.path());
        let s = st2.status(&i).unwrap();
        assert_eq!(s.state, LibraryState::Ready);
        assert_eq!(s.storage, Some(Storage::Shared));
        assert_eq!(st2.library_dir(), None);
    }

    #[test]
    fn joining_without_permission_goes_to_the_private_space() {
        let dir = tempfile::tempdir().unwrap();
        let (_, backup, other_id) = other_device(dir.path());
        let i = info(dir.path(), false);
        let st = state(dir.path());
        // 加入也收備份檔（照清單的種類改走從備份檔還原）
        let placed = st.place_archive(&backup, &i, None, true).unwrap();
        assert_eq!(placed.kind, ArchiveKind::Backup);
        assert_eq!(placed.new, dir.path().join("private/PaperReader"));
        assert_eq!(st.storage(), Some(Storage::Private));
        assert_eq!(library_id(&st), other_id);
        // 指定共用空間卻沒有權限：拒絕，什麼都沒寫
        let dir2 = tempfile::tempdir().unwrap();
        let (sync2, _, _) = other_device(dir2.path());
        let e = state(dir2.path())
            .place_archive(
                &sync2,
                &info(dir2.path(), false),
                Some(Storage::Shared),
                true,
            )
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::PermissionDenied);
        assert!(!dir2.path().join("Documents").exists());
    }

    #[test]
    fn restoring_into_an_empty_library_replaces_it_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let (_, backup, other_id) = other_device(dir.path());
        let i = info(dir.path(), true);
        let st = state(dir.path());
        st.create(Storage::Shared, &i).unwrap();
        let empty_id = library_id(&st);
        let placed = st.place_archive(&backup, &i, None, false).unwrap();
        let official = dir.path().join("Documents/PaperReader");
        assert_eq!(placed.new, official);
        assert!(placed.old.is_none());
        assert_eq!(library_id(&st), other_id);
        assert_ne!(empty_id, other_id);
        // 旁邊沒有留下新資料夾或移開的空文獻庫
        let left: Vec<String> = std::fs::read_dir(dir.path().join("Documents"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["PaperReader".to_string()]);
        assert_eq!(st.library_dir(), None);
        assert_eq!(
            state(dir.path()).status(&i).unwrap().path,
            Some(official.display().to_string())
        );
    }

    #[test]
    fn a_library_with_a_user_file_or_a_group_is_not_replaced() {
        for case in ["file", "group"] {
            let dir = tempfile::tempdir().unwrap();
            let (sync, _, _) = other_device(dir.path());
            let i = info(dir.path(), true);
            let st = state(dir.path());
            st.create(Storage::Shared, &i).unwrap();
            let official = dir.path().join("Documents/PaperReader");
            if case == "file" {
                // 別的工具放的隱藏項目：不是應用的，不能刪
                std::fs::create_dir_all(official.join("notes/.obsidian")).unwrap();
                std::fs::write(official.join("notes/.obsidian/app.json"), b"{}").unwrap();
            } else {
                st.with_lib(|l| {
                    let (_, token) = l.list_groups()?;
                    l.create_group("Reading", &token).map(|_| ())
                })
                .unwrap();
            }
            let placed = st.place_archive(&sync, &i, None, true).unwrap();
            assert_eq!(placed.old.as_deref(), Some(official.as_path()), "{case}");
            assert_ne!(placed.new, official);
            assert!(
                official.join("library.json").is_file(),
                "{case}: old one stays"
            );
            if case == "file" {
                assert!(official.join("notes/.obsidian/app.json").is_file());
            }
        }
    }

    #[test]
    fn a_bad_archive_leaves_the_current_library_open_and_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let i = info(dir.path(), true);
        let st = state(dir.path());
        st.create(Storage::Shared, &i).unwrap();
        let id = library_id(&st);
        let bad = dir.path().join("bad.zip");
        std::fs::write(&bad, b"not a zip").unwrap();
        let e = st.place_archive(&bad, &i, None, true).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Unreadable);
        assert_eq!(library_id(&st), id);
        assert_eq!(st.library_dir(), None);
        let left: Vec<String> = std::fs::read_dir(dir.path().join("Documents"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["PaperReader".to_string()]);
    }

    #[test]
    fn a_sync_package_is_not_a_backup_file() {
        let dir = tempfile::tempdir().unwrap();
        let (sync, _, _) = other_device(dir.path());
        let i = info(dir.path(), true);
        let st = state(dir.path());
        st.create(Storage::Shared, &i).unwrap();
        let e = st.place_archive(&sync, &i, None, false).unwrap_err();
        assert_eq!(e.kind, ErrorKind::RestoreGotSyncPackage);
        // 別的檔：這不是備份檔
        let other = dir.path().join("other.zip");
        std::fs::write(&other, b"not a zip").unwrap();
        let e = st.place_archive(&other, &i, None, false).unwrap_err();
        assert_eq!(e.kind, ErrorKind::NotBackupFile);
    }

    #[test]
    fn unfinished_sync_is_told_apart_from_a_newer_format() {
        let dir = tempfile::tempdir().unwrap();
        let i = info(dir.path(), false);
        let st = state(dir.path());
        st.create(Storage::Private, &i).unwrap();
        let s = st.status(&i).unwrap();
        assert!(!s.unfinished_sync);
        // 較新的格式：唯讀，但不是合併沒做完
        let lib_json = dir.path().join("private/PaperReader/library.json");
        let mut v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&lib_json).unwrap()).unwrap();
        v["format"] = serde_json::json!(99);
        std::fs::write(&lib_json, serde_json::to_vec(&v).unwrap()).unwrap();
        let s = state(dir.path()).status(&i).unwrap();
        assert_eq!(s.state, LibraryState::ReadOnly);
        assert!(!s.unfinished_sync);
    }

    #[test]
    fn failing_to_save_prefs_keeps_the_old_ones() {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(
            Settings::fresh(),
            dir.path().join("no-such-dir/settings.json"),
            false,
        );
        let changed = Prefs {
            dark_pages: true,
            ..Prefs::default()
        };
        assert!(st.set_prefs(changed).is_err());
        assert_eq!(st.prefs(), Prefs::default());
    }

    #[test]
    fn recovery_keeps_readable_prefs_and_drops_unknown_ones() {
        let dir = tempfile::tempdir().unwrap();
        let sp = dir.path().join("settings.json");
        let id = new_id();
        // 偏好是好的、但有一個這個版本不認得的 `storage`：偏好要保留
        std::fs::write(
            &sp,
            format!(
                r#"{{"device_id":"{id}","storage":"cloud","prefs":{{"sort_key":"year","dark_pages":true}}}}"#
            ),
        )
        .unwrap();
        let (s, recovered) = Settings::load_or_create(&sp).unwrap();
        assert!(recovered);
        assert_eq!(s.prefs.sort_key, SortKey::Year);
        assert!(s.prefs.dark_pages);
        assert!(!s.prefs.keep_awake, "missing fields take the default");
        // 偏好裡有不認得的值：整組退回預設，不讓啟動失敗
        std::fs::write(
            &sp,
            format!(r#"{{"device_id":"{id}","prefs":{{"sort_key":"stars"}}}}"#),
        )
        .unwrap();
        let (s, recovered) = Settings::load_or_create(&sp).unwrap();
        assert!(recovered);
        assert_eq!(s.prefs, Prefs::default());
    }

    // ---- 記得位置但那裡沒有文獻庫 ----

    #[test]
    fn missing_library_is_reported_and_not_recreated_until_asked() {
        let dir = tempfile::tempdir().unwrap();
        let i = info(dir.path(), true);
        state(dir.path()).create(Storage::Shared, &i).unwrap();
        // 使用者用檔案管理員把整個資料夾搬走
        std::fs::rename(
            dir.path().join("Documents/PaperReader"),
            dir.path().join("Documents/Moved"),
        )
        .unwrap();
        let st = state(dir.path());
        let s = st.status(&i).unwrap();
        assert_eq!(s.state, LibraryState::Missing);
        assert_eq!(s.storage, Some(Storage::Shared));
        assert!(s.path.unwrap().ends_with("PaperReader"));
        assert!(
            !dir.path().join("Documents/PaperReader").exists(),
            "must not create a new library on its own"
        );
        // 使用者確認「在這裡建一個新的」才建
        let s = st.create(Storage::Shared, &i).unwrap();
        assert_eq!(s.state, LibraryState::Ready);
        assert!(
            dir.path()
                .join("Documents/PaperReader/library.json")
                .exists()
        );
    }

    #[test]
    fn missing_private_library_is_also_reported() {
        let dir = tempfile::tempdir().unwrap();
        let i = info(dir.path(), false);
        state(dir.path()).create(Storage::Private, &i).unwrap();
        std::fs::remove_file(dir.path().join("private/PaperReader/library.json")).unwrap();
        let s = state(dir.path()).status(&i).unwrap();
        assert_eq!(s.state, LibraryState::Missing);
    }

    // ---- 應用專屬空間的文獻庫是空的時改放到共用空間 ----

    #[test]
    fn an_empty_private_library_can_move_to_shared_space() {
        let dir = tempfile::tempdir().unwrap();
        let st = state(dir.path());
        st.create(Storage::Private, &info(dir.path(), false))
            .unwrap();
        // 沒有權限：拒絕，位置不變
        let e = st.switch_to_shared(&info(dir.path(), false)).unwrap_err();
        assert_eq!(e.kind, ErrorKind::PermissionDenied);
        assert_eq!(st.storage(), Some(Storage::Private));
        // 有權限：改放共用空間，下次啟動也在那裡
        let i = info(dir.path(), true);
        let s = st.switch_to_shared(&i).unwrap();
        assert_eq!(s.storage, Some(Storage::Shared));
        assert_eq!(s.state, LibraryState::Ready);
        let st2 = state(dir.path());
        assert_eq!(st2.status(&i).unwrap().storage, Some(Storage::Shared));
    }

    #[test]
    fn a_library_with_papers_is_never_switched() {
        let dir = tempfile::tempdir().unwrap();
        let st = state(dir.path());
        st.create(Storage::Private, &info(dir.path(), false))
            .unwrap();
        let pdf = dir.path().join("a.pdf");
        std::fs::write(&pdf, b"%PDF-1.4 x").unwrap();
        st.with_lib(|l| l.add_paper(&pdf, Default::default(), None).map(|_| ()))
            .unwrap();
        let e = st.switch_to_shared(&info(dir.path(), true)).unwrap_err();
        assert_eq!(e.kind, ErrorKind::InvalidInput);
        assert_eq!(st.storage(), Some(Storage::Private));
    }

    #[test]
    fn a_library_with_only_trash_is_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        let st = state(dir.path());
        st.create(Storage::Private, &info(dir.path(), false))
            .unwrap();
        let pdf = dir.path().join("a.pdf");
        std::fs::write(&pdf, b"%PDF-1.4 x").unwrap();
        let id = st
            .with_lib(|l| match l.add_paper(&pdf, Default::default(), None)? {
                paperreader_core::local::AddOutcome::Added(id) => Ok(id),
                _ => unreachable!(),
            })
            .unwrap();
        st.with_lib(|l| l.remove_paper(&id)).unwrap();
        let e = st.switch_to_shared(&info(dir.path(), true)).unwrap_err();
        assert_eq!(e.kind, ErrorKind::InvalidInput);
    }

    // ---- 有內容的文獻庫搬到共用空間 ----

    use paperreader_core::backup::restore_backup;

    fn no_limit(_: &Path) -> Option<u64> {
        None
    }

    fn quiet(_: MoveStep, _: &Path) {}

    fn added(l: &mut LocalLibrary, body: &[u8], name: &str) -> Result<String, LibError> {
        let mut b = body;
        match l.add_paper_from_reader(&mut b, Some(name), Default::default(), None)? {
            paperreader_core::local::AddOutcome::Added(id) => Ok(id),
            _ => unreachable!(),
        }
    }

    fn shared_dir(root: &Path) -> PathBuf {
        root.join("Documents/PaperReader")
    }

    fn private_dir(root: &Path) -> PathBuf {
        root.join("private/PaperReader")
    }

    fn pending(st: &AppState) -> Option<PendingMove> {
        st.lock_settings().settings.pending_move.clone()
    }

    /// `Documents` 裡留下的工作資料夾。
    fn move_leftovers(root: &Path) -> Vec<String> {
        std::fs::read_dir(root.join("Documents"))
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with(MOVE_WORK_PREFIX))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 應用專屬空間裡有內容的文獻庫：三篇，第一篇寫了想法、放進群組「Reading」，第三篇移到回收區。
    /// 交回（狀態、有權限的 info、文獻庫代號）。
    fn filled_private(root: &Path) -> (AppState, StorageInfo, String) {
        let st = state(root);
        st.create(Storage::Private, &info(root, false)).unwrap();
        st.with_lib(|l| {
            let a = added(l, b"%PDF-1.4 first", "a.pdf")?;
            added(l, b"%PDF-1.4 second", "b.pdf")?;
            let c = added(l, b"%PDF-1.4 third", "c.pdf")?;
            l.set_thoughts(&a, "worth reading", None)?;
            let (_, token) = l.list_groups()?;
            let (g, _) = l.create_group("Reading", &token)?;
            let (_, token) = l.get_paper(&a)?;
            l.add_to_group(&a, &g.id, &token)?;
            l.remove_paper(&c)
        })
        .unwrap();
        let id = library_id(&st);
        (st, info(root, true), id)
    }

    #[test]
    fn a_library_with_content_moves_to_shared_space_and_the_old_one_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, id) = filled_private(root);
        let mut steps = Vec::new();
        let st2 = st.clone();
        let out = st
            .move_to_shared(&i, &no_limit, &mut |s, _| {
                steps.push(s);
                if s == MoveStep::Switch {
                    assert!(!st2.cancel_move(), "no cancelling once switching");
                }
            })
            .unwrap();
        assert!(!out.cancelled);
        assert!(out.old_removed);
        assert_eq!(out.path, shared_dir(root).display().to_string());
        assert_eq!(
            steps,
            vec![
                MoveStep::Prepare,
                MoveStep::Export,
                MoveStep::Copy,
                MoveStep::Verify,
                MoveStep::Switch,
                MoveStep::Clean
            ]
        );
        // 同一個文獻庫：代號不變，逐篇處理的紀錄與外掛的索引（應用資料夾裡以代號為名）照樣對得上
        assert_eq!(library_id(&st), id);
        assert_eq!(st.storage(), Some(Storage::Shared));
        assert_eq!(st.library_dir(), None);
        assert_eq!(pending(&st), None);
        let (papers, trash, groups) = st
            .with_lib(|l| Ok((l.list_papers(), l.list_trash()?.len(), l.list_groups()?.0)))
            .unwrap();
        assert_eq!(papers.len(), 2);
        assert_eq!(trash, 1);
        assert_eq!(groups.len(), 1);
        assert!(papers.iter().all(|p| p.materials.len() == 1));
        let noted = papers.iter().find(|p| p.has_notes).expect("thoughts moved");
        assert_eq!(noted.groups, vec![groups[0].id.clone()]);
        // 筆記檔在共用空間的 notes/（Obsidian 與檔案管理員看得到的地方）
        assert!(
            shared_dir(root)
                .join("notes")
                .read_dir()
                .unwrap()
                .next()
                .is_some()
        );
        // 舊的已刪，工作資料夾也清掉了
        assert!(!private_dir(root).exists());
        assert!(move_leftovers(root).is_empty());
        let p = st.move_progress();
        assert_eq!(p.step, MoveStep::Done);
        assert!(p.total > 0 && p.done == p.total);
        // 重開應用：在共用空間
        let st3 = state(root);
        let s = st3.status(&i).unwrap();
        assert_eq!(s.state, LibraryState::Ready);
        assert_eq!(s.storage, Some(Storage::Shared));
        assert_eq!(s.path, Some(shared_dir(root).display().to_string()));
        assert_eq!(library_id(&st3), id);
    }

    #[test]
    fn a_library_restored_beside_the_private_one_moves_and_the_kept_one_stays() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, _) = filled_private(root);
        // 從備份檔還原到不是空的文獻庫：新的放在旁邊（`PaperReader-restored-…`），原本的留著
        let zip = root.join("b.zip");
        st.export_backup(&zip, &mut |_, _| {}).unwrap();
        let restored = st
            .place_archive(&zip, &info(root, false), None, false)
            .unwrap()
            .new;
        assert!(st.library_dir().is_some());
        let out = st.move_to_shared(&i, &no_limit, &mut quiet).unwrap();
        assert!(out.old_removed);
        assert!(!restored.exists(), "the open one was moved");
        assert!(
            private_dir(root).join("library.json").is_file(),
            "the one the user was told is kept stays"
        );
        assert_eq!(st.library_dir(), None);
        let s = state(root).status(&i).unwrap();
        assert_eq!(
            (s.state, s.storage),
            (LibraryState::Ready, Some(Storage::Shared))
        );
    }

    #[test]
    fn the_plan_tells_where_and_how_much_without_writing_anything() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, _) = filled_private(root);
        let plan = st.move_plan(&i, &|_| Some(u64::MAX)).unwrap();
        assert!(!plan.adopt);
        assert_eq!(plan.to, shared_dir(root).display().to_string());
        assert!(plan.bytes > 0);
        assert_eq!(plan.needed, plan.bytes * 2 + MOVE_RESERVE);
        assert_eq!(plan.free, Some(u64::MAX));
        assert!(!root.join("Documents").exists());
        assert_eq!(pending(&st), None);
    }

    #[test]
    fn a_library_already_in_shared_space_is_not_touched() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, _) = filled_private(root);
        // 共用空間已經有一個文獻庫（例如之前安裝時留下的）
        let other = LocalLibrary::open(&shared_dir(root), &new_id()).unwrap();
        let other_id = other.library_file().id.clone();
        drop(other);
        assert_eq!(
            st.move_plan(&i, &no_limit).unwrap_err().kind,
            ErrorKind::SharedNotEmpty
        );
        let e = st.move_to_shared(&i, &no_limit, &mut quiet).unwrap_err();
        assert_eq!(e.kind, ErrorKind::SharedNotEmpty);
        let lf: serde_json::Value =
            serde_json::from_slice(&std::fs::read(shared_dir(root).join("library.json")).unwrap())
                .unwrap();
        assert_eq!(lf["id"], other_id.as_str());
        assert_eq!(st.storage(), Some(Storage::Private));
        assert_eq!(st.with_lib(|l| Ok(l.list_papers().len())).unwrap(), 2);
        assert!(move_leftovers(root).is_empty());
        assert_eq!(pending(&st), None);

        // 不是文獻庫、但有別的東西（使用者自己的檔）：一樣不搬、不動
        let dir2 = tempfile::tempdir().unwrap();
        let (st2, i2, _) = filled_private(dir2.path());
        std::fs::create_dir_all(shared_dir(dir2.path())).unwrap();
        std::fs::write(shared_dir(dir2.path()).join("mine.txt"), b"x").unwrap();
        let e = st2.move_to_shared(&i2, &no_limit, &mut quiet).unwrap_err();
        assert_eq!(e.kind, ErrorKind::SharedNotEmpty);
        assert!(shared_dir(dir2.path()).join("mine.txt").is_file());
    }

    #[test]
    fn an_empty_private_library_adopts_the_shared_one_instead_of_moving() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let st = state(root);
        st.create(Storage::Private, &info(root, false)).unwrap();
        let other = LocalLibrary::open(&shared_dir(root), &new_id()).unwrap();
        let other_id = other.library_file().id.clone();
        drop(other);
        let plan = st.move_plan(&info(root, true), &no_limit).unwrap();
        assert!(plan.adopt);
        // 換過去用原本的 `switch_to_shared`：接回那一個，什麼都不搬
        st.switch_to_shared(&info(root, true)).unwrap();
        assert_eq!(library_id(&st), other_id);
    }

    #[test]
    fn not_enough_space_does_not_start() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, _) = filled_private(root);
        let tight = |_: &Path| Some(1024u64);
        assert_eq!(
            st.move_plan(&i, &tight).unwrap_err().kind,
            ErrorKind::NoSpace
        );
        let mut steps = Vec::new();
        let e = st
            .move_to_shared(&i, &tight, &mut |s, _| steps.push(s))
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::NoSpace);
        assert!(!steps.contains(&MoveStep::Export));
        assert!(!root.join("Documents").exists());
        assert_eq!(st.storage(), Some(Storage::Private));
        assert_eq!(pending(&st), None);
    }

    #[test]
    fn moving_without_permission_or_from_shared_space_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, _, _) = filled_private(root);
        let no = info(root, false);
        assert_eq!(
            st.move_plan(&no, &no_limit).unwrap_err().kind,
            ErrorKind::PermissionDenied
        );
        assert_eq!(
            st.move_to_shared(&no, &no_limit, &mut quiet)
                .unwrap_err()
                .kind,
            ErrorKind::PermissionDenied
        );
        assert!(!root.join("Documents").exists());
        assert_eq!(st.storage(), Some(Storage::Private));
        // 已經在共用空間：沒有可以搬的
        let dir2 = tempfile::tempdir().unwrap();
        let st2 = state(dir2.path());
        let i2 = info(dir2.path(), true);
        st2.create(Storage::Shared, &i2).unwrap();
        assert_eq!(
            st2.move_to_shared(&i2, &no_limit, &mut quiet)
                .unwrap_err()
                .kind,
            ErrorKind::InvalidInput
        );
    }

    #[test]
    fn a_read_only_library_is_not_moved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (_, i, _) = filled_private(root);
        // 較新的版本建的：唯讀開啟
        let lib_json = private_dir(root).join("library.json");
        let mut v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&lib_json).unwrap()).unwrap();
        v["format"] = serde_json::json!(99);
        std::fs::write(&lib_json, serde_json::to_vec(&v).unwrap()).unwrap();
        let st = state(root);
        assert_eq!(st.status(&i).unwrap().state, LibraryState::ReadOnly);
        let e = st.move_to_shared(&i, &no_limit, &mut quiet).unwrap_err();
        assert_eq!(e.kind, ErrorKind::ReadOnly);
        assert!(!root.join("Documents").exists());
    }

    #[test]
    fn a_copy_that_does_not_match_is_thrown_away_and_nothing_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, _) = filled_private(root);
        // 正式位置事先是一個空資料夾：清的時候要留著
        std::fs::create_dir_all(shared_dir(root)).unwrap();
        let e = st
            .move_to_shared(&i, &no_limit, &mut |step, work| {
                if step == MoveStep::Verify {
                    // 還原出來的少了一篇
                    let papers = work.join(MOVE_LIBRARY).join("papers");
                    let one = papers
                        .read_dir()
                        .unwrap()
                        .flatten()
                        .find(|e| e.path().is_dir())
                        .unwrap();
                    std::fs::remove_dir_all(one.path()).unwrap();
                }
            })
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::MoveMismatch);
        assert_eq!(st.storage(), Some(Storage::Private));
        assert_eq!(pending(&st), None);
        assert!(move_leftovers(root).is_empty());
        assert!(
            shared_dir(root).is_dir() && absent_or_empty_dir(&shared_dir(root)),
            "the empty folder that was there stays, empty"
        );
        assert_eq!(st.with_lib(|l| Ok(l.list_papers().len())).unwrap(), 2);
        assert!(private_dir(root).join("library.json").is_file());
        let s = state(root).status(&i).unwrap();
        assert_eq!(s.storage, Some(Storage::Private));
    }

    #[test]
    fn cancelling_before_the_check_cleans_up_and_keeps_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, id) = filled_private(root);
        let st2 = st.clone();
        let out = st
            .move_to_shared(&i, &no_limit, &mut |step, _| {
                if step == MoveStep::Copy {
                    assert!(st2.cancel_move());
                }
            })
            .unwrap();
        assert!(out.cancelled);
        assert!(!out.old_removed);
        assert_eq!(out.path, private_dir(root).display().to_string());
        assert!(
            !root.join("Documents").exists(),
            "what this move made in shared space is gone"
        );
        assert_eq!(st.storage(), Some(Storage::Private));
        assert_eq!(pending(&st), None);
        assert_eq!(library_id(&st), id);
        assert_eq!(st.with_lib(|l| Ok(l.list_papers().len())).unwrap(), 2);
        // 下一次搬照常（取消不會留到下一次）
        let out = st.move_to_shared(&i, &no_limit, &mut quiet).unwrap();
        assert!(!out.cancelled);
        assert_eq!(st.storage(), Some(Storage::Shared));
    }

    #[test]
    fn the_half_left_by_an_interrupted_move_is_recognised_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, id) = filled_private(root);
        // 上次搬到換設定之前程序被結束：正式位置有一份同代號的文獻庫、工作資料夾還在、設定裡有紀錄
        let work = root
            .join("Documents")
            .join(format!("{MOVE_WORK_PREFIX}{}", new_id()));
        std::fs::create_dir_all(work.join("junk")).unwrap();
        std::fs::write(work.join(MOVE_BACKUP), b"partial").unwrap();
        let zip = root.join("half.zip");
        st.export_backup(&zip, &mut |_, _| {}).unwrap();
        restore_backup(&zip, &shared_dir(root), &st.device_id()).unwrap();
        st.update_settings(|s| {
            s.pending_move = Some(PendingMove {
                work: work.display().to_string(),
                target: shared_dir(root).display().to_string(),
                library_id: id.clone(),
                target_existed: false,
                parent_existed: false,
                placed: true,
            })
        })
        .unwrap();
        // 下次啟動：設定仍指向應用專屬空間，舊的完整
        let st = state(root);
        assert_eq!(st.status(&i).unwrap().storage, Some(Storage::Private));
        assert!(!st.move_plan(&i, &no_limit).unwrap().adopt);
        let out = st.move_to_shared(&i, &no_limit, &mut quiet).unwrap();
        assert!(!out.cancelled);
        assert!(!work.exists());
        assert!(move_leftovers(root).is_empty());
        assert_eq!(st.storage(), Some(Storage::Shared));
        assert_eq!(pending(&st), None);
        assert_eq!(library_id(&st), id);
        assert_eq!(st.with_lib(|l| Ok(l.list_papers().len())).unwrap(), 2);
    }

    #[test]
    fn a_library_at_the_target_without_a_matching_record_is_never_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, _) = filled_private(root);
        // 同一個文獻庫的另一份（例如同步工具放進來的），不是這個應用搬到一半的：不能刪
        let zip = root.join("copy.zip");
        st.export_backup(&zip, &mut |_, _| {}).unwrap();
        restore_backup(&zip, &shared_dir(root), &st.device_id()).unwrap();
        let e = st.move_to_shared(&i, &no_limit, &mut quiet).unwrap_err();
        assert_eq!(e.kind, ErrorKind::SharedNotEmpty);
        assert!(shared_dir(root).join("library.json").is_file());
        // 有紀錄，但記的是別的文獻庫：一樣不刪
        let work = root
            .join("Documents")
            .join(format!("{MOVE_WORK_PREFIX}{}", new_id()));
        st.update_settings(|s| {
            s.pending_move = Some(PendingMove {
                work: work.display().to_string(),
                target: shared_dir(root).display().to_string(),
                library_id: new_id(),
                target_existed: false,
                parent_existed: true,
                placed: true,
            })
        })
        .unwrap();
        let e = st.move_to_shared(&i, &no_limit, &mut quiet).unwrap_err();
        assert_eq!(e.kind, ErrorKind::SharedNotEmpty);
        assert!(shared_dir(root).join("library.json").is_file());
        assert_eq!(st.storage(), Some(Storage::Private));
    }

    #[test]
    fn a_record_left_before_anything_was_placed_does_not_clear_a_library_that_came_later() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, id) = filled_private(root);
        // 上次在匯出或還原途中被結束：紀錄在，但還沒放上去
        st.update_settings(|s| {
            s.pending_move = Some(PendingMove {
                work: root
                    .join("Documents")
                    .join(format!("{MOVE_WORK_PREFIX}{}", new_id()))
                    .display()
                    .to_string(),
                target: shared_dir(root).display().to_string(),
                library_id: id.clone(),
                target_existed: false,
                parent_existed: false,
                placed: false,
            })
        })
        .unwrap();
        // 之後正式位置才出現同一個文獻庫的一份（例如同步工具放進來的）：不是這次的，不能刪
        let zip = root.join("later.zip");
        st.export_backup(&zip, &mut |_, _| {}).unwrap();
        restore_backup(&zip, &shared_dir(root), &st.device_id()).unwrap();
        assert_eq!(
            st.move_plan(&i, &no_limit).unwrap_err().kind,
            ErrorKind::SharedNotEmpty
        );
        let e = st.move_to_shared(&i, &no_limit, &mut quiet).unwrap_err();
        assert_eq!(e.kind, ErrorKind::SharedNotEmpty);
        assert!(shared_dir(root).join("library.json").is_file());
        assert_eq!(st.storage(), Some(Storage::Private));
    }

    #[test]
    fn an_empty_private_library_with_nothing_in_shared_space_is_moved() {
        // 原本走 `switch_to_shared`（建一個新的、空的留在原處）；現在照樣搬，舊的空文獻庫刪掉
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let st = state(root);
        st.create(Storage::Private, &info(root, false)).unwrap();
        let id = library_id(&st);
        let i = info(root, true);
        assert!(!st.move_plan(&i, &no_limit).unwrap().adopt);
        let out = st.move_to_shared(&i, &no_limit, &mut quiet).unwrap();
        assert!(!out.cancelled && out.old_removed);
        assert_eq!(st.storage(), Some(Storage::Shared));
        assert_eq!(library_id(&st), id);
        assert!(shared_dir(root).join("library.json").is_file());
        assert!(!private_dir(root).exists());
        assert_eq!(state(root).status(&i).unwrap().state, LibraryState::Ready);
    }

    #[test]
    fn a_record_pointing_outside_the_shared_folder_deletes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, id) = filled_private(root);
        // 紀錄被改壞、指到別處：一律不碰，只清掉紀錄
        let elsewhere = root.join("elsewhere");
        std::fs::create_dir_all(elsewhere.join("keep")).unwrap();
        st.update_settings(|s| {
            s.pending_move = Some(PendingMove {
                work: elsewhere.display().to_string(),
                target: elsewhere.display().to_string(),
                library_id: id.clone(),
                target_existed: false,
                parent_existed: false,
                placed: true,
            })
        })
        .unwrap();
        st.move_to_shared(&i, &no_limit, &mut quiet).unwrap();
        assert!(elsewhere.join("keep").is_dir());
        assert_eq!(st.storage(), Some(Storage::Shared));
        assert_eq!(pending(&st), None);
    }

    #[test]
    fn a_pending_move_survives_a_broken_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let sp = dir.path().join("settings.json");
        let id = new_id();
        // 讀不懂的偏好讓整份走復原，但搬到一半的紀錄要留著
        std::fs::write(
            &sp,
            format!(
                r#"{{"device_id":"{id}","storage":"cloud","pending_move":{{"work":"w","target":"t","library_id":"{id}","target_existed":false,"parent_existed":true}}}}"#
            ),
        )
        .unwrap();
        let (s, recovered) = Settings::load_or_create(&sp).unwrap();
        assert!(recovered);
        assert_eq!(s.pending_move.map(|p| p.library_id), Some(id));
    }

    // ---- 搬移的邊界情形 ----

    fn first_paper_dir(lib_root: &Path) -> PathBuf {
        lib_root
            .join("papers")
            .read_dir()
            .unwrap()
            .flatten()
            .find(|e| e.path().is_dir())
            .unwrap()
            .path()
    }

    fn move_cleanup(st: &AppState) -> Option<MoveCleanup> {
        st.lock_settings().settings.move_cleanup.clone()
    }

    /// 在 `at` 那一步開始時程序被結束：hook 裡 panic，之後什麼都不做（不退回、不清）。
    fn killed_at(st: &AppState, i: &StorageInfo, at: MoveStep) {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = st.move_to_shared(i, &no_limit, &mut |s, _| {
                if s == at {
                    panic!("simulated: killed");
                }
            });
        }));
        assert!(r.is_err(), "{at:?}");
    }

    #[test]
    fn files_the_backup_leaves_out_move_too_and_nothing_is_lost() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, _) = filled_private(root);
        let old = private_dir(root);
        let paper = first_paper_dir(&old)
            .strip_prefix(&old)
            .unwrap()
            .to_path_buf();
        // 別的工具的隱藏檔與資料夾、使用者自己放的檔
        let extras: Vec<(PathBuf, &[u8])> = vec![
            (PathBuf::from("notes/.obsidian/app.json"), b"{}"),
            (PathBuf::from("notes/.draft.md"), b"draft"),
            (PathBuf::from(".hidden-at-root"), b"root"),
            (paper.join(".annot"), b"annot"),
            (paper.join("user-extra.txt"), b"mine"),
        ];
        for (rel, body) in &extras {
            let p = old.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        // 應用自己的暫存不搬
        std::fs::write(old.join(".pr-leftover"), b"temp").unwrap();
        let out = st.move_to_shared(&i, &no_limit, &mut quiet).unwrap();
        assert!(out.old_removed);
        for (rel, body) in &extras {
            assert_eq!(
                std::fs::read(shared_dir(root).join(rel)).ok().as_deref(),
                Some(*body),
                "{}",
                rel.display()
            );
        }
        assert!(!shared_dir(root).join(".pr-leftover").exists());
        assert!(!old.exists());
    }

    #[test]
    fn a_file_that_arrived_different_stops_the_move() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, _) = filled_private(root);
        let old = private_dir(root);
        let rel = first_paper_dir(&old)
            .strip_prefix(&old)
            .unwrap()
            .join("user-extra.txt");
        std::fs::write(old.join(&rel), b"mine").unwrap();
        // 核對之前，搬過去的那一份內容變了：整棵比對要抓到（不只比篇數與材料數）
        let e = st
            .move_to_shared(&i, &no_limit, &mut |step, work| {
                if step == MoveStep::Verify {
                    std::fs::write(work.join(MOVE_LIBRARY).join(&rel), b"changed").unwrap();
                }
            })
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::MoveMismatch);
        assert_eq!(std::fs::read(old.join(&rel)).unwrap(), b"mine");
        assert_eq!(st.storage(), Some(Storage::Private));
        assert!(move_leftovers(root).is_empty());
    }

    #[test]
    fn a_work_folder_in_the_wrong_place_or_with_the_wrong_name_is_left_alone() {
        for case in ["name", "place"] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let (st, i, id) = filled_private(root);
            let work = if case == "name" {
                root.join("Documents/not-a-move-folder")
            } else {
                root.join("elsewhere")
                    .join(format!("{MOVE_WORK_PREFIX}{}", new_id()))
            };
            std::fs::create_dir_all(work.join("keep")).unwrap();
            st.update_settings(|s| {
                s.pending_move = Some(PendingMove {
                    work: work.display().to_string(),
                    target: shared_dir(root).display().to_string(),
                    library_id: id.clone(),
                    target_existed: false,
                    parent_existed: true,
                    placed: false,
                })
            })
            .unwrap();
            st.move_to_shared(&i, &no_limit, &mut quiet).unwrap();
            assert!(work.join("keep").is_dir(), "{case}");
        }
    }

    #[test]
    fn a_placed_record_does_not_clear_a_different_library_at_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, id) = filled_private(root);
        // 紀錄記的是這個文獻庫、已經放上去了，但正式位置上是另一個文獻庫
        drop(LocalLibrary::open(&shared_dir(root), &new_id()).unwrap());
        st.update_settings(|s| {
            s.pending_move = Some(PendingMove {
                work: root
                    .join("Documents")
                    .join(format!("{MOVE_WORK_PREFIX}{}", new_id()))
                    .display()
                    .to_string(),
                target: shared_dir(root).display().to_string(),
                library_id: id.clone(),
                target_existed: false,
                parent_existed: true,
                placed: true,
            })
        })
        .unwrap();
        let e = st.move_to_shared(&i, &no_limit, &mut quiet).unwrap_err();
        assert_eq!(e.kind, ErrorKind::SharedNotEmpty);
        assert!(shared_dir(root).join("library.json").is_file());
    }

    #[test]
    fn a_failure_after_the_rename_puts_everything_back() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, id) = filled_private(root);
        st.0.moving.fail_after_rename.store(true, Ordering::SeqCst);
        let e = st.move_to_shared(&i, &no_limit, &mut quiet).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Io);
        assert!(!shared_dir(root).exists());
        assert!(move_leftovers(root).is_empty());
        assert_eq!(pending(&st), None);
        assert_eq!(st.storage(), Some(Storage::Private));
        assert_eq!(library_id(&st), id);
        assert_eq!(st.with_lib(|l| Ok(l.list_papers().len())).unwrap(), 2);
        assert_eq!(
            state(root).status(&i).unwrap().storage,
            Some(Storage::Private)
        );
    }

    #[test]
    fn being_killed_before_the_switch_is_recovered_by_the_next_move() {
        for at in [
            MoveStep::Export,
            MoveStep::Copy,
            MoveStep::Verify,
            MoveStep::Switch,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let (st, i, id) = filled_private(root);
            killed_at(&st, &i, at);
            // 鎖放開了：同一個狀態照樣用得了
            assert_eq!(
                st.with_lib(|l| Ok(l.list_papers().len())).unwrap(),
                2,
                "{at:?}"
            );
            // 重開應用：仍在應用專屬空間，舊的完整；再搬一次清掉上次留下的、搬成
            let st = state(root);
            let s = st.status(&i).unwrap();
            assert_eq!(s.storage, Some(Storage::Private), "{at:?}");
            assert_eq!(st.with_lib(|l| Ok(l.list_papers().len())).unwrap(), 2);
            st.move_to_shared(&i, &no_limit, &mut quiet).unwrap();
            assert_eq!(st.storage(), Some(Storage::Shared), "{at:?}");
            assert_eq!(library_id(&st), id);
            assert!(move_leftovers(root).is_empty(), "{at:?}");
            assert_eq!(pending(&st), None);
        }
        // 改名到正式位置之後、換設定之前
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, _) = filled_private(root);
        st.0.moving.panic_after_rename.store(true, Ordering::SeqCst);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = st.move_to_shared(&i, &no_limit, &mut quiet);
        }));
        assert!(r.is_err());
        assert!(shared_dir(root).join("library.json").is_file());
        let st = state(root);
        assert_eq!(st.status(&i).unwrap().storage, Some(Storage::Private));
        st.move_to_shared(&i, &no_limit, &mut quiet).unwrap();
        assert_eq!(st.storage(), Some(Storage::Shared));
        assert!(move_leftovers(root).is_empty());
    }

    #[test]
    fn being_killed_after_the_switch_finishes_removing_the_old_one_at_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, id) = filled_private(root);
        killed_at(&st, &i, MoveStep::Clean);
        // 換過去了，舊的與工作資料夾還在
        assert!(private_dir(root).join("library.json").is_file());
        assert!(!move_leftovers(root).is_empty());
        // 下次開應用：在共用空間，接著刪完
        let st = state(root);
        let s = st.status(&i).unwrap();
        assert_eq!(
            (s.state, s.storage),
            (LibraryState::Ready, Some(Storage::Shared))
        );
        assert_eq!(library_id(&st), id);
        assert!(!private_dir(root).exists());
        assert!(move_leftovers(root).is_empty());
        assert_eq!(move_cleanup(&st), None);
    }

    #[test]
    fn the_old_one_is_only_removed_from_the_shared_library_and_inside_the_app_space() {
        // 設定還指向應用專屬空間：不刪（那就是正在用的文獻庫）
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, _) = filled_private(root);
        st.update_settings(|s| {
            s.move_cleanup = Some(MoveCleanup {
                old: private_dir(root).display().to_string(),
                work: root
                    .join("Documents")
                    .join(format!("{MOVE_WORK_PREFIX}{}", new_id()))
                    .display()
                    .to_string(),
            })
        })
        .unwrap();
        state(root).status(&i).unwrap();
        assert!(private_dir(root).join("library.json").is_file());

        // 指向共用空間，但記的舊位置不在應用專屬空間裡：不刪，紀錄清掉
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let i = info(root, true);
        let st = state(root);
        st.create(Storage::Shared, &i).unwrap();
        let elsewhere = root.join("elsewhere");
        std::fs::create_dir_all(elsewhere.join("keep")).unwrap();
        st.update_settings(|s| {
            s.move_cleanup = Some(MoveCleanup {
                old: elsewhere.display().to_string(),
                work: elsewhere.display().to_string(),
            })
        })
        .unwrap();
        let st = state(root);
        st.status(&i).unwrap();
        assert!(elsewhere.join("keep").is_dir());
        assert_eq!(move_cleanup(&st), None);
    }

    #[test]
    fn a_panic_while_restoring_does_not_hang() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let dir = tempfile::tempdir().unwrap();
            let m = MoveCtl::default();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                watch_restore(dir.path(), 10, &m, || -> Result<_, LibError> {
                    panic!("simulated: restore broke")
                })
            }));
            let _ = tx.send(r.is_err());
        });
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(10)),
            Ok(true)
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_work_folder_that_cannot_be_removed_keeps_the_record() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (st, i, _) = filled_private(root);
        let mut held: Option<std::fs::File> = None;
        let e = st
            .move_to_shared(&i, &no_limit, &mut |step, work| {
                if step == MoveStep::Verify {
                    // 工作資料夾裡有一個檔被別的程式獨佔開著：刪不掉
                    let f = work.join("held");
                    std::fs::write(&f, b"x").unwrap();
                    held = Some(
                        std::fs::OpenOptions::new()
                            .read(true)
                            .share_mode(0)
                            .open(&f)
                            .unwrap(),
                    );
                    // 還原出來的少一篇：核對不符、退回
                    std::fs::remove_dir_all(first_paper_dir(&work.join(MOVE_LIBRARY))).unwrap();
                }
            })
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::MoveMismatch);
        assert!(
            pending(&st).is_some(),
            "the record stays while the work folder is still there"
        );
        drop(held);
        st.move_to_shared(&i, &no_limit, &mut quiet).unwrap();
        assert!(move_leftovers(root).is_empty());
        assert_eq!(pending(&st), None);
    }
}
