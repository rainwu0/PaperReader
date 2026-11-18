//! 文獻庫的操作交換的共用型別。操作本身是 `local::LocalLibrary` 的公開方法。
//!
//! 閱讀事件只能經由三個操作寫入（`record_open`、`record_position`、`set_read`），它們會檢查
//! 欄位。`LocalLibrary` 沒有可以寫任意 `type` 與 `data` 的公開方法，畫面無法繞過檢查。
//!
//! 下面的 `compile_fail` 只要編譯失敗就算通過，所以要說明它為什麼會失敗：`append_event` 這個方法
//! 不在 `LocalLibrary` 的公開方法上（核心內部用的 `LocalLibrary::append_raw` 是 `pub(crate)`），
//! 而這段程式碼除了呼叫它之外沒有別的錯（`LocalLibrary`、`serde_json` 都有引用，其餘用法都合法）。
//! 若有人把任意寫入的操作加成公開方法，這段程式碼會編譯成功，測試就會失敗。
//!
//! ```compile_fail
//! use paperreader_core::local::LocalLibrary;
//! fn write_anything(store: &mut LocalLibrary) {
//!     store.append_event("paper", "whatever", serde_json::json!({})).unwrap();
//! }
//! ```

use crate::format::Bib;
use crate::jsonio::VersionToken;
use crate::notes::{Highlight, Quote, TextPosition, UnreadableBlock};
use crate::progress::ReadingProgress;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MaterialStatus {
    Ok,
    /// 清單列了，但檔案不存在。
    Missing,
    /// 檔案存在，大小與記錄不符。
    SizeMismatch,
    /// `path` 是絕對路徑、含 `..` 或以 `@` 開頭；不去碰那個路徑。
    InvalidPath,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MaterialSummary {
    pub id: String,
    pub role: String,
    pub form: String,
    pub pages: Option<u32>,
    pub status: MaterialStatus,
    /// `paper.json` 記的指紋（逐篇處理判斷原文換了沒有）；沒有是 `None`。
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PaperSummary {
    pub id: String,
    /// 資料夾名稱（只給人看）。
    pub folder: String,
    pub bib: Bib,
    pub materials: Vec<MaterialSummary>,
    /// `format` 比應用認得的新：只能讀，不寫回。
    pub needs_newer_app: bool,
    /// 由閱讀事件算出的閱讀狀況；畫面不必一篇一篇另外問。
    pub progress: ReadingProgress,
    /// `paper.json` 的 `added`（加入時間），原樣；沒有時是 `None`。
    pub added: Option<String>,
    /// 這篇所屬的群組代號（含已不存在的懸空代號，畫面自己略過）。
    pub groups: Vec<String>,
    /// 筆記檔裡有劃線、整篇的想法或讀不懂但留著的段落；從記憶體裡的筆記索引來。
    pub has_notes: bool,
}

/// 畫面交來的一條新劃線。核心填代號、種類、檔案指紋、時間、顏色。
#[derive(Debug, Clone, PartialEq)]
pub struct HighlightDraft {
    pub material: String,
    /// 頁序，從 0 起算。
    pub page_index: u32,
    /// 頁面上印的頁碼；不合規則時核心當成 `None`。
    pub page_label: Option<String>,
    /// 頁面座標的矩形 `[左, 下, 右, 上]`，單位是點，至少一個。
    pub rects: Vec<[f64; 4]>,
    pub next_page_rects: Vec<[f64; 4]>,
    pub position: Option<TextPosition>,
    pub quote: Quote,
    /// PDF.js 讀出的文件識別碼，核心不解讀。
    pub fingerprint: Option<String>,
    pub comment: String,
}

/// 畫面交來的一筆筆跡。核心填代號、建立時間。
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeDraft {
    pub material: String,
    /// 頁序，從 0 起算。
    pub page_index: u32,
    /// `#rrggbb`。
    pub color: String,
    /// 線寬，單位是 PDF 的點。
    pub width: f64,
    /// 大於 0、最多 1。
    pub opacity: f64,
    /// PDF 頁面座標 `[x, y]`，至少一個。
    pub points: Vec<[f64; 2]>,
}

/// 一篇文獻的筆記。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NotesDoc {
    /// 相對於根目錄、用 `/` 分隔，例如 `notes/Zhao 2024 - DETRs.md`；還沒有筆記檔時是 `None`。
    pub path: Option<String>,
    pub modified: Option<String>,
    pub modified_by: Option<String>,
    pub thoughts: String,
    /// 整篇的想法有開始標記卻找不到結束標記時是 `false`；畫面這時不提供編輯。
    pub thoughts_complete: bool,
    pub highlights: Vec<Highlight>,
    pub unreadable: Vec<UnreadableBlock>,
}

/// 匯出時沒有放進備份檔的一個檔。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedFile {
    /// 名稱不是 UTF-8 時用 `to_string_lossy`。
    pub path: String,
    /// `non-utf8-name`、`unsafe-path`、`case-duplicate`、`symlink`、`not-a-file`、`unreadable`。
    pub reason: String,
}

/// 匯出備份檔的結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportReport {
    pub files: usize,
    pub bytes: u64,
    /// 開始時列出、匯出途中大小或修改時間變了的檔案（放進去的是讀到的那一份，備份清單與它一致）。
    pub changed_during_export: Vec<String>,
    /// 沒有放進去的檔。
    pub skipped: Vec<SkippedFile>,
}

/// 從備份檔還原的結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RestoreReport {
    pub library_id: String,
    pub files: usize,
    pub bytes: u64,
    /// 還原後重新讀每個檔、與備份清單比對不符的（正常應該是空的）。
    pub manifest_mismatches: Vec<String>,
    /// 還原後快速檢查找到的問題。原本的文獻庫就有的問題也會出現在這裡。
    pub problems: Vec<Problem>,
}

/// 加入一項附加材料（譯本）的結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum AddMaterialOutcome {
    /// 已加入：新材料的代號與 `paper.json` 新的版本記號。
    Added {
        material_id: String,
        token: VersionToken,
    },
    /// 同一份檔案（指紋相同）已經是這篇的材料（任何角色，包括原文）：那項材料的代號。
    AlreadyPresent { material_id: String },
}

/// 存下產生的譯本 PDF 的結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum PutGenerated {
    /// 第一次產生：新材料的代號與 `paper.json` 新的版本記號。
    Created {
        material_id: String,
        token: VersionToken,
    },
    /// 再次產生：同一份材料原地更新（代號不變，檔案與指紋換成新的）。
    Updated {
        material_id: String,
        token: VersionToken,
    },
    /// 內容與現有的完全相同，沒有寫任何東西。
    Unchanged { material_id: String },
}

/// 建立譯文疊加檔的結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreatedOverlay {
    /// 新材料的代號。
    pub material_id: String,
    /// `paper.json` 新的版本記號。
    pub paper_token: VersionToken,
    /// 譯文疊加檔的版本記號（接著更新各段時帶這個）。
    pub overlay_token: VersionToken,
}

/// 刪除群組的結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeleteOutcome {
    /// `library.json` 新的版本記號。
    pub token: VersionToken,
    /// 沒能移出這個群組的文獻代號（`format` 較新、沒有權限等）；其餘都已移出。
    pub skipped: Vec<String>,
}

/// 回收區裡的一篇文獻。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrashEntry {
    pub paper_id: String,
    /// 名稱：`.trash/` 裡資料夾名稱去掉時間前綴之後的部分（通常就是移除前的資料夾名稱）。
    pub name: String,
    /// 移除的時間（`2026-11-13T08:30:11.000Z`）；資料夾名稱沒有時間前綴時是 `None`（不知道）。
    pub removed_at: Option<String>,
    /// 它在 `.trash/` 裡的資料夾名稱。
    pub folder: String,
    pub bib: Bib,
}

/// 回收區裡一項移除的材料（`remove_material` 移過來的）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemovedMaterialEntry {
    /// 它在 `.trash/` 裡的資料夾名稱；永久刪除時用它指定。
    pub folder: String,
    pub paper_id: String,
    /// 移除時那篇的資料夾名稱（只給人看）。
    pub paper_folder: String,
    /// 移除的時間；說明檔讀不出合法的時間時是 `None`。
    pub removed_at: Option<String>,
    /// 移除前材料清單裡的那一項，原樣。
    pub material: crate::format::Material,
    /// 這個資料夾裡的檔案實際占的位元組數。
    pub bytes: u64,
}

/// 清空回收區的結果。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PurgeReport {
    /// 永久刪除的文獻數。
    pub papers: usize,
    /// 永久刪除的移除材料數。
    pub materials: usize,
}

/// 重新掃描與上次相比的差異，內容都是文獻代號。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ScanDiff {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
}

/// 加入一篇文獻的三種結果，都帶文獻代號。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddOutcome {
    /// 已加入（新代號）。
    Added(String),
    /// 原文的指紋與目錄裡某一篇相同（那一篇的代號）。
    AlreadyPresent(String),
    /// 原文的指紋與 `.trash/` 裡某一篇相同（那一篇的代號）。
    InTrash(String),
}

/// 讀取閱讀事件的篩選條件。`from` 含、`to` 不含；時間用 `2026-11-13T09:02:44.310Z` 這種格式的字串。
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub paper: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

// ---- 同步包----

/// 同步包的模式：`full` 全部的檔都放進來；`changes` 只帶變更。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    Full,
    Changes,
}

/// 匯出同步包的結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncExportReport {
    pub package: String,
    /// 實際的模式：要求 `Changes` 但找不到起算點時是 `Full`。
    pub mode: SyncMode,
    pub since: Option<String>,
    /// 清單裡的檔數、放進來的檔數與位元組數。
    pub listed: usize,
    pub included: usize,
    pub bytes: u64,
    pub changed_during_export: Vec<String>,
    pub skipped: Vec<SkippedFile>,
}

/// 預覽與回報裡彙總的種類（不含紀錄檔）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, serde::Deserialize,
)]
pub enum ItemKind {
    Paper,
    Material,
    Bib,
    Groups,
    Pages,
    Highlight,
    Thoughts,
    Stroke,
    Overlay,
    NotesFile,
    Group,
    TrashState,
    /// 參考文獻清單（`references.json`）：一篇一份。
    References,
    /// 抽出的文字（`text.json`）：一篇一份。
    Text,
}

/// 依文獻與種類彙總的一項。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct SyncItem {
    pub kind: ItemKind,
    pub paper: Option<String>,
    pub title: Option<String>,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum ConflictKind {
    Bib,
    HighlightEdited,
    HighlightDeletedButEdited,
    Thoughts,
    MaterialFile,
    Overlay,
    TrashState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum ConflictChoice {
    Local,
    Incoming,
    Both,
}

/// 衝突的一邊。
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct ConflictSide {
    pub exists: bool,
    pub modified: Option<String>,
    pub device: Option<String>,
    /// 給使用者比對的文字：書目的篇名、短評、想法；其他種類是 `None`。
    pub text: Option<String>,
    /// 書目衝突：兩邊不同的那幾欄（`title`、`authors`、`year`、`venue`、`doi`、`url`，依這個順序），值是給人看的文字，
    /// 空的是 `None`。其他種類是空的。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<ConflictField>,
}

/// 衝突一邊的一欄。
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct ConflictField {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct SyncConflict {
    /// 預覽裡唯一（`c1`、`c2` …），寫入時用它對應選擇。
    pub id: String,
    pub kind: ConflictKind,
    pub paper: Option<String>,
    pub title: Option<String>,
    /// 劃線代號、材料代號等；整篇層級的是 `None`。
    pub item: Option<String>,
    pub local: ConflictSide,
    pub incoming: ConflictSide,
    pub options: Vec<ConflictChoice>,
    pub default: ConflictChoice,
    /// `Overlay` 時是兩邊都翻過、譯文不同的段數；其他是 1。
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum NoticeKind {
    SameSource,
    SameGroupName,
    ConflictCopy,
    NotesOutsideText,
    IncomingUnreadable,
    NewerFormat,
    LocalUnreadable,
    UnmergedWrite,
    /// 本機的材料檔還在、但大小與 `paper.json` 記的不符（在應用以外被改過）：不補回、不覆寫。
    MaterialChangedOutside,
    /// 只帶變更的同步包省略了某項材料、本機也找不到相符的檔：只略過那一項材料。
    /// 畫面請使用者在另一台匯出完整的同步包。
    MissingFile,
    /// 重點筆記的同一點兩邊文字不同：留本機的，匯入的那一份另存衝突副本。
    DigestPointConflict,
    /// 這篇在某一台永久刪除了，但在這台或匯入那邊的目錄裡（之後放回、或一直在用）：目錄裡的一律不刪。
    PurgedButRestored,
    /// 一篇的參考文獻清單合併後會超過上限（判斷 5000 項或 2 MB、抽出的部分 6 MB、整份 9 MB）：不合併，本機的不動
    /// （各項上限如上）。
    TooLargeToMerge,
}

/// 提醒：不用選，寫在預覽與回報裡。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct SyncNotice {
    pub kind: NoticeKind,
    pub paper: Option<String>,
    pub path: Option<String>,
    pub other: Option<String>,
    /// 這則提醒只因為這些衝突照預設的選擇（「兩個都留」）才會出現：預覽裡是衝突的代號（`c1` …）。
    /// 畫面在列出的衝突目前都不是預設的選擇時隱藏它。空的表示和選擇無關、一定會出現。
    #[serde(default)]
    pub conflicts: Vec<String>,
}

/// 一份檔是哪一種（只看清單的 `kind`，不驗整份）：畫面用來分辨選到的是同步包、備份檔還是都不是。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum ArchiveKind {
    SyncPackage,
    Backup,
    /// 不是 zip、沒有清單、清單讀不懂或不是這兩種。
    Other,
}

/// 整篇不合併的文獻。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct SkippedPaper {
    pub paper: String,
    pub title: Option<String>,
    /// `missing-file`、`format-too-new`、`unreadable`
    pub reason: String,
}

/// 同步包的資訊。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PackageInfo {
    pub package: String,
    pub created: String,
    pub device: String,
    pub device_label: Option<String>,
    pub mode: SyncMode,
    /// 這份同步包已經匯入過（有它的 `sync-imported` 記錄）。
    pub already_imported: bool,
}

/// 匯入的預覽。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncPreview {
    /// 這一份預覽的代號；寫入與丟棄都帶它。
    pub preview_id: String,
    pub package: PackageInfo,
    pub added: Vec<SyncItem>,
    pub updated: Vec<SyncItem>,
    pub removed: Vec<SyncItem>,
    pub conflicts: Vec<SyncConflict>,
    pub notices: Vec<SyncNotice>,
    pub skipped_papers: Vec<SkippedPaper>,
}

/// 匯入寫完的回報。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncImportReport {
    pub package: String,
    pub added: Vec<SyncItem>,
    pub updated: Vec<SyncItem>,
    pub removed: Vec<SyncItem>,
    pub conflict_copies: Vec<String>,
    pub skipped_papers: Vec<SkippedPaper>,
    /// 計畫之後被別處改過、沒有覆寫的檔（`UnmergedWrite`），以及其他提醒。
    pub notices: Vec<SyncNotice>,
    /// 寫完之後快速檢查找到的問題（原本就有的也會出現）。
    pub problems: Vec<Problem>,
}

/// 讀取變更紀錄的篩選條件。
#[derive(Debug, Clone, Default)]
pub struct ChangeFilter {
    pub paper: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckLevel {
    /// `paper.json` 讀不懂、代號重複、材料檔不存在或大小不符、不認得的項目、清單沒列的檔案、
    /// 讀不了的紀錄檔。
    Quick,
    /// 快速檢查的全部，加上重算每個材料檔的指紋並比對。
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ProblemKind {
    UnreadablePaperFile,
    DuplicateId,
    MaterialMissing,
    MaterialSizeMismatch,
    MaterialInvalidPath,
    MaterialHashMismatch,
    UnknownEntry,
    UnlistedFile,
    /// 殘留的暫存項目（`.pr-` 開頭）：應用被中斷留下的，或開啟時清不掉的。
    LeftoverTemp,
    /// `logs/` 裡讀不了的紀錄檔（權限不足、是資料夾等）：開啟、重新掃描、讀寫事件都略過它。
    UnreadableLogFile,
    /// `library.json` 的 `groups` 或 `deleted_groups` 讀不懂（路徑 `library.json`）。
    UnreadableGroups,
    /// 一篇文獻的 `groups` 裡有不存在（或已刪除）的群組代號；一篇只列一次。
    UnknownGroupRef,
    /// `notes_dir` 不合規則（路徑 `library.json`）；這時不檢查它底下的東西。
    InvalidNotesDir,
    /// `notes_dir` 裡的 `.md` 檔讀不了（權限）或不是 UTF-8。
    UnreadableNotesFile,
    /// 兩份以上的筆記檔屬性區的文獻代號相同：依檔名排序第一份以外的每一份各一項。
    DuplicateNotesFile,
    /// 一份筆記檔裡有讀不懂或不完整的劃線、或不完整的想法：一份檔只列一項。
    UnreadableNoteBlock,
    /// 文獻資料夾裡的 `ink.json` 讀不懂（格式錯誤、不是 UTF-8、讀不了）：筆跡不會被覆寫，也畫不出來。
    UnreadableInkFile,
    /// 材料清單裡的譯文疊加檔（`form` 是 `overlay`）讀不懂（格式錯誤、不是 UTF-8、讀不了）：不會被覆寫，也看不到譯文。
    UnreadableOverlayFile,
    /// 有寫到一半的合併（路徑 `.pr-sync-<代號>`）：文獻庫因此唯讀，直到計畫做完。
    UnfinishedSyncImport,
    /// 材料清單裡的重點筆記（`role` `digest`）讀不懂（不是 UTF-8、不是重點筆記的格式）：不會被覆寫，也看不到。
    UnreadableDigestFile,
    /// 文獻資料夾裡的 `references.json` 讀不懂（格式錯誤、超過 9 MB、不是這個格式的同名檔）：不會被覆寫，
    /// 逐篇處理也不重抽。
    UnreadableReferencesFile,
    /// 文獻資料夾裡的 `text.json` 讀不懂（格式錯誤、超過 32 MB、不是這個格式的同名檔）：不會被覆寫，
    /// 逐篇處理也不重抽。
    UnreadableTextFile,
}

/// 目錄摘要的一篇（`paper_digests`）：外殼比對前後兩份，得出新增、變了、移除的文獻。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaperDigest {
    pub id: String,
    /// 記憶體裡的書目寫成 JSON 的 SHA-256（不讀檔）。
    pub bib_sha256: String,
    /// `references.json` 的檔案屬性；沒有檔是 `None`。
    pub references: Option<FileStamp>,
    /// 全文搜尋要的來源檔的檔案屬性：鍵是 `text`、`notes`、每份疊加檔 `overlay:<材料代號>`、
    /// 每份重點筆記 `digest:<材料代號>`；沒有的檔不列。每個外掛只取自己要的部分。
    pub files: BTreeMap<String, FileStamp>,
}

/// 一個檔的大小與修改時間（毫秒；系統給不出時間是 `None`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct FileStamp {
    pub bytes: u64,
    pub modified_ms: Option<i64>,
}

/// `read_for_index` 的結果。
#[derive(Debug, Clone, PartialEq)]
pub enum IndexRead {
    /// 檔案屬性等於交來的 `known`：沒有讀內容。
    Unchanged,
    /// 沒有檔、讀不懂、`format` 較新。
    Missing,
    /// 讀到的內容與讀之前取的檔案屬性。
    Read(FileStamp, Box<IndexSource>),
}

/// 給全文搜尋的索引讀的一份來源。
#[derive(Debug, Clone, PartialEq)]
pub enum IndexSource {
    Text(crate::text::TextFile),
    Overlay(crate::overlay::OverlayFile),
    Digest(crate::digest::DigestDoc),
    Notes(NotesDoc),
}

/// 檢查文獻庫回報的一項問題。只回報，不修、不刪。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub kind: ProblemKind,
    /// 相對於根目錄、用 `/` 分隔。材料路徑不合法（`MaterialInvalidPath`）時不指向那個路徑，
    /// 而是指向記載它的 `paper.json`。重複代號指向沒有列入目錄的那個資料夾。
    pub path: String,
    pub paper: Option<String>,
}
