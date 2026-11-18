// 指令交回的資料的型別。與 `src-tauri` 與核心（`app/core`）的 Serialize 結構一一對應。
// 畫面只讀這些資料，不推算資料規則。

export interface Author {
  family?: string | null;
  given?: string | null;
  literal?: string | null;
}

export interface Bib {
  title?: string | null;
  authors: Author[];
  year?: number | null;
  venue?: string | null;
  doi?: string | null;
  url?: string | null;
  /** 使用者清空過的 `year`、`doi`、`url`：自動補書目與同步都不補回。核心維護，畫面只讀。 */
  cleared?: string[];
  modified?: string | null;
  modified_by?: string | null;
}

/** 自動補書目交來的值（`fill_bib_blanks`）；`null` 表示沒抽到。 */
export interface BibFill {
  year: number | null;
  doi: string | null;
  url: string | null;
}

/** 參考文獻清單的一條草稿（`references_write`）。 */
export interface ReferenceEntryDraft {
  number: number | null;
  label: string | null;
  text: string;
  /** 條目起頭所在的頁序，從 0 起。 */
  page: number;
  bbox: [number, number, number, number] | null;
  doi: string | null;
  arxiv: string | null;
  year: number | null;
}

/** 寫入參考文獻清單的抽出的部分。 */
export interface ReferencesDraft {
  extractor: string;
  version: number;
  source_material: string;
  source_sha256: string;
  entries: ReferenceEntryDraft[];
}

/** 寫入抽出的文字（`text.json`）。`pages` 的第 i 項是第 i 頁的文字。 */
export interface TextDraft {
  extractor: string;
  version: number;
  source_material: string;
  source_sha256: string;
  pages: string[];
  /** 到了上限沒抽完。 */
  truncated: boolean;
}

/** 一篇的抽出的文字的檔頭（`text_states`）。 */
export interface TextState {
  extractor: string;
  version: number;
  source_sha256: string;
}

export type JudgmentKind = "confirm" | "reject" | "manual";

/** 參考文獻清單裡使用者的一項判斷。 */
export interface LinkJudgment {
  id: string;
  kind: string;
  paper: string;
  entry_key: string;
  key_version: number;
  entry_text: string;
  entry_doi: string | null;
  entry_arxiv: string | null;
  created: string;
  created_by: string;
}

export interface JudgmentAdded {
  judgment: LinkJudgment;
  /** `references.json` 新的版本記號。 */
  token: string;
}

export type MaterialStatus = "Ok" | "Missing" | "SizeMismatch" | "InvalidPath";

export interface MaterialSummary {
  id: string;
  role: string;
  form: string;
  pages: number | null;
  status: MaterialStatus;
  /** `paper.json` 記的指紋（譯文疊加檔、重點筆記沒有）。 */
  sha256: string | null;
}

export type ReadStatus = "Unread" | "Reading" | "Read";

export interface ReadPosition {
  material: string;
  page: number;
  offset: number;
  zoom: number | null;
  /** 畫面左緣在那一頁寬度的幾分之幾；舊的紀錄沒有，是 `null`。 */
  left: number | null;
  t: string;
  device: string;
}

export interface ReadingProgress {
  status: ReadStatus;
  read_at: string | null;
  position: ReadPosition | null;
  device_positions: ReadPosition[];
  furthest_pages: Record<string, number>;
  last_read_at: string | null;
}

export interface PaperSummary {
  id: string;
  folder: string;
  bib: Bib;
  materials: MaterialSummary[];
  needs_newer_app: boolean;
  progress: ReadingProgress;
  /** 加入時間（世界協調時間）；舊的文獻沒有。 */
  added: string | null;
  /** 所屬的群組代號（可能含已不存在的，畫面略過）。 */
  groups: string[];
  /** 筆記檔裡有劃線、整篇的想法或讀不懂但留著的段落（核心算好，首頁的「有筆記」篩選用）。 */
  has_notes: boolean;
}

/** 材料的出處（`paper.json` 的 `provenance`）。 */
export interface Provenance {
  /** `added-by-user`（使用者加入的）或 `generated`（產生的）。 */
  method?: string | null;
  /** 加入或（再次）產生的時間。 */
  time?: string | null;
  /** 產生的譯本 PDF：由哪一份譯文疊加檔產生。 */
  generated_from?: string | null;
  /** 是否已校對（產生的材料一律從 `false` 開始）。 */
  proofread?: boolean | null;
  /** 加入或匯入時原本的檔名（只作紀錄；匯入之後讀 PDF 自帶的篇名時用）。 */
  origin_name?: string | null;
}

export interface Material {
  id: string;
  role: string;
  form: string;
  path: string;
  bytes: number | null;
  pages: number | null;
  provenance?: Provenance;
}

export interface PaperFile {
  format: number;
  id: string;
  short_name: string | null;
  added: string | null;
  bib: Bib;
  materials: Material[];
}

export interface PaperDetail {
  paper: PaperFile;
  /** 版本記號：改書目時原樣交回。 */
  token: string;
}

export interface BibDraft {
  title: string | null;
  authors: Author[];
  year: number | null;
  venue: string | null;
  doi: string | null;
  url: string | null;
}

export interface TrashEntry {
  paper_id: string;
  name: string;
  removed_at: string | null;
  folder: string;
  bib: Bib;
}

/** 回收區裡一項移除的材料（`remove_material` 移過來的）。 */
export interface RemovedMaterialEntry {
  /** 它在回收區裡的資料夾名稱；永久刪除時用它指定。 */
  folder: string;
  paper_id: string;
  /** 移除時那篇的資料夾名稱。 */
  paper_folder: string;
  removed_at: string | null;
  /** 移除前材料清單裡的那一項。 */
  material: Material;
  /** 占的位元組數。 */
  bytes: number;
}

/** 清空回收區的結果。 */
export interface PurgeReport {
  papers: number;
  materials: number;
}

/**
 * 檢查找到的問題種類。核心之後會新增種類（例如群組讀不懂），所以畫面不能假設只有這幾種：
 * 不認得的種類要有一句通用的說法（`strings` 的 `problemMessage`）。
 */
export type ProblemKind = string;

export interface Problem {
  kind: ProblemKind;
  path: string;
  paper: string | null;
}

export interface ScanDiff {
  added: string[];
  changed: string[];
  removed: string[];
}

export type Storage = "shared" | "private";
export type LibraryState = "not_created" | "ready" | "read_only" | "access_lost" | "missing";

export interface LibraryStatus {
  state: LibraryState;
  storage: Storage | null;
  path: string | null;
  allFilesAccess: boolean;
  /** 唯讀是因為上一次匯入同步包的合併沒做完（不是較新的格式）：給「重試」。 */
  unfinishedSync: boolean;
  /** 這次開啟時把上一次沒做完的合併做完了：只在做完之後的第一次狀況裡是 true（畫面說一次）。 */
  resumedSync: boolean;
}

/** 搬到共用空間之前的估計。位元組。 */
export interface MovePlan {
  /** 搬過去的位置。 */
  to: string;
  /** 文獻庫的大小。 */
  bytes: number;
  /** 需要的空間（暫存的備份檔加搬過去的，再加一點餘裕）。 */
  needed: number;
  /** 共用空間可用的空間；量不到是 `null`。 */
  free: number | null;
  /** 共用空間已經有文獻庫、這裡的是空的：不搬，改用那一個（`librarySwitchToShared`）。 */
  adopt: boolean;
}

/** 搬到共用空間做到哪一步。從 `switch` 起不能取消。 */
export type MoveStep = "idle" | "prepare" | "export" | "copy" | "verify" | "switch" | "clean" | "done";

export interface MoveProgress {
  step: MoveStep;
  /** 匯出加寫到共用空間的位元組（兩段各算一次文獻庫的大小）。 */
  done: number;
  total: number;
  /** 已經按了取消。 */
  cancelling: boolean;
}

export interface MoveOutcome {
  /** 在核對之前取消了：什麼都沒換。 */
  cancelled: boolean;
  /** 文獻庫現在的位置。 */
  path: string;
  /** 應用專屬空間的舊文獻庫刪掉了（刪不成不算失敗）。 */
  oldRemoved: boolean;
}

/** 一份檔案的加入結果。 */
export type AddOutcome = "added" | "already_present" | "in_trash" | "failed";

export interface AddItem {
  /** 使用者選的檔案的顯示名稱。 */
  name: string;
  outcome: AddOutcome;
  paperId: string | null;
  /** `outcome` 是 `failed` 時的原因（錯誤的種類）。 */
  error: ErrorKind | null;
}

/** 一次選檔的結果：使用者取消時 `cancelled` 為真、`items` 是空的。 */
export interface AddBatch {
  cancelled: boolean;
  items: AddItem[];
}

/** 閱讀紀錄的一列：某一次打開某篇文獻，那一次讀到第幾頁（沒記下是 `null`）。 */
export interface HistoryEntry {
  paper_id: string;
  /** 打開的時間（世界協調時間）。 */
  t: string;
  page: number | null;
}

/** 群組：使用者自己建的一組文獻，一篇可以在好幾個群組。 */
export interface Group {
  id: string;
  name: string;
  created?: string | null;
  modified?: string | null;
}

export interface DeleteGroupResult {
  /** 沒能移出這個群組的文獻代號（其餘都已移出）。 */
  skipped: string[];
}

/** 加入譯本的結果。 */
export interface TranslationResult {
  cancelled: boolean;
  outcome: "added" | "already_present" | "failed";
  name: string;
  error: ErrorKind | null;
}

/** 材料的角色（`paper.json` 的 `role`）。 */
export const ROLE_SOURCE = "source";
export const ROLE_TRANSLATION = "translation";

// ---------- 筆記 ----------

export interface PageRef {
  /** 頁序，從 0 起算。 */
  index: number;
  /** 頁面上印的頁碼；沒有就是 `null`。 */
  label: string | null;
}

export interface TextPosition {
  start: number;
  end: number;
  basis: string;
}

export interface Quote {
  exact: string;
  prefix?: string | null;
  suffix?: string | null;
}

/** 頁面座標的矩形 `[左, 下, 右, 上]`，單位是點（PDF 本身的座標）。 */
export type Rect = [number, number, number, number];

/**
 * 劃線的黃：閱讀畫面（`pdfView.ts` 的劃線層）與匯出帶標註的 PDF（`export/annotate.ts`）共用這一個值，
 * 兩邊都以「色彩增值」疊在頁面上，白紙上看起來就是這個顏色。
 */
export const HIGHLIGHT_COLOR = "#ffe58a";

export interface Highlight {
  id: string;
  kind: string;
  material: string;
  page: PageRef;
  rects: Rect[];
  /** 劃線跨頁時，接在下一頁的那一半。舊的筆記檔沒有。 */
  next_page_rects?: Rect[];
  position: TextPosition | null;
  quote: Quote;
  /** `#rrggbb`；沒記（`null`）是 `HIGHLIGHT_COLOR`。 */
  color: string | null;
  created: string;
  modified: string;
  /** 短評。 */
  comment: string;
  /** 筆記檔裡這一條的結尾標記還在；`false` 時只顯示、不能改。 */
  complete: boolean;
}

export interface UnreadableBlock {
  line: number;
  text: string;
  reason: string;
}

export interface NotesDoc {
  path: string | null;
  modified: string | null;
  thoughts: string;
  /** 整篇的想法的結尾標記不見了：只顯示、不能改。 */
  thoughts_complete: boolean;
  highlights: Highlight[];
  unreadable: UnreadableBlock[];
}

/** 一篇的筆記，加上筆記檔的版本記號（還沒有筆記檔是 `null`）：寫的時候原樣交回。 */
export interface NotesView {
  doc: NotesDoc;
  token: string | null;
}

/** 畫面交給核心的一條新劃線。 */
export interface HighlightInput {
  material: string;
  /** 頁序，從 0 起算。 */
  page_index: number;
  page_label: string | null;
  rects: Rect[];
  position: TextPosition | null;
  quote: Quote;
  fingerprint: string | null;
  comment: string;
}

export interface AddedHighlight {
  highlight: Highlight;
  token: string;
}

// ---------- 筆跡（ink.json） ----------

/** 一筆筆跡：點是 PDF 頁面座標 `[x, y]`，線寬是 PDF 的點，與縮放無關。 */
export interface Stroke {
  id: string;
  material: string;
  /** 頁序，從 0 起算。 */
  page: number;
  color: string;
  width: number;
  opacity: number;
  created: string;
  points: Array<[number, number]>;
}

export interface InkFile {
  strokes: Stroke[];
}

/** 畫面交給核心的一筆新筆跡。 */
export interface StrokeInput {
  material: string;
  page_index: number;
  color: string;
  width: number;
  opacity: number;
  points: Array<[number, number]>;
}

// ---------- 備份 ----------

export interface SkippedFile {
  path: string;
  reason: string;
}

export interface ExportResult {
  /** 備份檔的檔名。 */
  name: string;
  /** 暫存的備份檔，交給 `deliverExport`。 */
  path: string;
  files: number;
  bytes: number;
  skipped: SkippedFile[];
  changedDuringExport: string[];
}

export interface RestoreResult {
  cancelled: boolean;
  /** 讀進來的是備份檔還是同步包（從另一台加入兩種都收）。取消時是 `null`。 */
  kind: "backup" | "sync" | null;
  files: number;
  bytes: number;
  problems: number;
  mismatches: number;
  /** 原本的文獻庫還在這裡，沒有動；放在原位（原本是空的或還沒有文獻庫）時是 `null`。 */
  oldPath: string | null;
  newPath: string;
}

// ---------- 同步包（核心 `store.rs` 的型別原樣交來：欄位是 snake_case，列舉是大寫開頭，只有 `SyncMode` 是小寫） ----------

export type SyncMode = "full" | "changes";

/** 匯出同步包的結果（外殼的結構，camelCase）。 */
export interface SyncExportResult {
  name: string;
  /** 暫存的同步包，交給 `deliverExport`。 */
  path: string;
  /** 實際的模式：要求只帶變更、但另一台還沒匯入過這台的同步包時是完整的。 */
  mode: SyncMode;
  listed: number;
  included: number;
  bytes: number;
  skipped: SkippedFile[];
  changedDuringExport: string[];
}

export type ItemKind =
  | "Paper"
  | "Material"
  | "Bib"
  | "Groups"
  | "Pages"
  | "Highlight"
  | "Thoughts"
  | "Stroke"
  | "Overlay"
  | "NotesFile"
  | "Group"
  | "TrashState"
  | "References"
  | "Text";

/** 依文獻與種類彙總的一項。 */
export interface SyncItem {
  kind: ItemKind;
  paper: string | null;
  title: string | null;
  count: number;
}

export type ConflictKind =
  | "Bib"
  | "HighlightEdited"
  | "HighlightDeletedButEdited"
  | "Thoughts"
  | "MaterialFile"
  | "Overlay"
  | "TrashState";

export type ConflictChoice = "Local" | "Incoming" | "Both";

/** 衝突的一邊。`text`：書目是篇名、劃線是短評、想法是全文、回收區是 `papers` 或 `trash`；其他是 `null`。 */
export interface ConflictSide {
  exists: boolean;
  modified: string | null;
  device: string | null;
  text: string | null;
  /** 書目衝突：兩邊不同的那幾欄（`title`、`authors`、`year`、`venue`、`doi`、`url`）；其他種類沒有。 */
  fields?: ConflictField[];
}

/** 衝突一邊的一欄；`value` 是給人看的文字，空的是 `null`。 */
export interface ConflictField {
  name: string;
  value: string | null;
}

export interface SyncConflict {
  /** 預覽裡唯一（`c1`、`c2` …），寫入時用它對應選擇。 */
  id: string;
  kind: ConflictKind;
  paper: string | null;
  title: string | null;
  item: string | null;
  local: ConflictSide;
  incoming: ConflictSide;
  options: ConflictChoice[];
  default: ConflictChoice;
  /** `Overlay` 是兩邊都翻過、譯文不同的段數；其他是 1。 */
  count: number;
}

export type NoticeKind =
  | "SameSource"
  | "SameGroupName"
  | "ConflictCopy"
  | "NotesOutsideText"
  | "IncomingUnreadable"
  | "NewerFormat"
  | "LocalUnreadable"
  | "UnmergedWrite"
  | "MaterialChangedOutside"
  | "MissingFile"
  | "DigestPointConflict"
  | "PurgedButRestored"
  | "TooLargeToMerge";

export interface SyncNotice {
  kind: NoticeKind;
  paper: string | null;
  path: string | null;
  other: string | null;
  /** 只因為這些衝突（代號）選了「兩個都留」才會出現；空的或沒有表示一定會出現。 */
  conflicts?: string[];
}

/** 整篇不合併的文獻。`reason`：`missing-file`、`format-too-new`、`unreadable`。 */
export interface SkippedPaper {
  paper: string;
  title: string | null;
  reason: string;
}

export interface PackageInfo {
  package: string;
  created: string;
  device: string;
  device_label: string | null;
  mode: SyncMode;
  /** 這份同步包已經匯入過。 */
  already_imported: boolean;
}

/** 匯入的預覽：寫入之前列出會新增、更新、移除什麼與有哪些衝突。 */
export interface SyncPreview {
  preview_id: string;
  package: PackageInfo;
  added: SyncItem[];
  updated: SyncItem[];
  removed: SyncItem[];
  conflicts: SyncConflict[];
  notices: SyncNotice[];
  skipped_papers: SkippedPaper[];
}

// ---- 整批匯入：核心的型別，欄位原樣（snake_case） ----

/** 選資料夾或 zip、掃描之後。 */
export interface ImportScanned {
  cancelled: boolean;
  /** 選到的資料夾不在手機內部儲存空間（記憶卡、雲端）。 */
  unsupported: boolean;
}

/** 一則提醒：`same-doi`（detail 是那篇的代號）、`bib-fields-dropped`（detail 是沒收的欄位名稱）。 */
export interface ImportNotice {
  kind: string;
  detail: string | null;
}

/** 會加入的一篇。 */
export interface ImportItem {
  /** 在清單 `papers` 裡的序號，從 0 起。 */
  index: number;
  title: string | null;
  files: number;
  notices: ImportNotice[];
}

/** 略過（或寫入失敗）的一篇。`reason` 是核心的原因代碼（`bad-entry`、`missing-file` …）。 */
export interface ImportSkip {
  index: number;
  reason: string;
  /** 欄位位置、清單裡的路徑，或相關文獻的代號。 */
  detail: string | null;
}

export interface ImportPreview {
  preview_id: string;
  /** 清單裡的篇數。 */
  total: number;
  accept: ImportItem[];
  skip: ImportSkip[];
  /** 會加入的檔案總大小。 */
  bytes: number;
}

export interface ImportReport {
  /** （序號, 新代號）。 */
  added: Array<[number, string]>;
  skipped: ImportSkip[];
  failed: ImportSkip[];
  /** 取消或空間不足而沒做的序號。 */
  not_done: number[];
  /** 加入的篇裡有提醒的篇數。 */
  notices: number;
}

/** 寫一篇之後：還有下一篇，或全部做完。 */
export type ImportStep = { Progress: { done: number; total: number } } | { Finished: ImportReport };

/** 選檔之後的準備：取消時 `preview` 是 `null`。 */
export interface SyncPrepared {
  cancelled: boolean;
  preview: SyncPreview | null;
}

/** 匯入寫完的回報。 */
export interface SyncImportReport {
  package: string;
  added: SyncItem[];
  updated: SyncItem[];
  removed: SyncItem[];
  /** 衝突副本（相對於文獻庫的路徑）。 */
  conflict_copies: string[];
  skipped_papers: SkippedPaper[];
  /** 沒有覆寫的檔（`UnmergedWrite`）與其他提醒。 */
  notices: SyncNotice[];
  /** 寫完之後快速檢查找到的問題。 */
  problems: Problem[];
}

export type Theme = "system" | "light" | "dark";

export type SortKey = "recent" | "added" | "title" | "author" | "year";
export type OpenTo = "home" | "last";

/** 使用者的偏好（存在應用的設定檔）。 */
export type ToolboxMode = "mini" | "floating" | "docked";
export type ToolboxDock = "top" | "bottom" | "left" | "right";

export interface Prefs {
  sort_key: SortKey;
  sort_reverse: boolean;
  /** 閱讀時頁面跟著深色；只在深色外觀時作用。 */
  dark_pages: boolean;
  /** 閱讀時螢幕不自動關閉。 */
  keep_awake: boolean;
  open_to: OpenTo;
  /** 閱讀畫面的工具箱：狀態、停靠的邊、位置（畫面寬高的千分比）。 */
  toolbox_mode: ToolboxMode;
  toolbox_dock: ToolboxDock;
  toolbox_x: number;
  toolbox_y: number;
  /** 設定的「整理文獻庫」（逐篇處理）暫停了；這台裝置記住。 */
  organize_paused: boolean;
}

/** 應用的版本。組件與授權的清單不經指令，畫面直接讀建置時產生的 `licenses.json`（`screens/settings/licenses.ts`）。 */
export interface AppInfo {
  version: string;
  /** 這個建置的作業系統。「關於」頁依它只列這個安裝檔內含的組件。 */
  platform: Platform;
}

export type Platform = "android" | "windows" | "other";

/** 啟動時要告知使用者的事（每次啟動只回報一次）。 */
export interface StartupNotice {
  settingsRecovered: boolean;
}

export interface Insets {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

/** 錯誤的種類（`src-tauri/src/error.rs`）。 */
export type ErrorKind =
  | "PermissionDenied"
  | "NoSpace"
  | "NotFound"
  | "Conflict"
  | "FormatTooNew"
  | "ReadOnly"
  | "Unreadable"
  | "InvalidInput"
  | "Io"
  | "NoLibrary"
  | "NotPdf"
  | "Unsupported"
  | "Platform"
  | "TranslateAuth"
  | "TranslateConfig"
  | "TranslateRateLimited"
  | "TranslateNetwork"
  | "TranslateServer"
  | "TranslateBadResponse"
  | "TranslateBusy"
  | "TranslateQuota"
  | "TranslateNoModel"
  | "TranslateLanguage"
  | "NotOnWifi"
  | "NoFont"
  | "SyncOtherLibrary"
  | "SyncOwnPackage"
  | "SyncBackupFile"
  | "SyncIncomplete"
  | "NotBackupFile"
  | "RestoreGotSyncPackage"
  | "ImportZipBackslash"
  | "SharedNotEmpty"
  | "MoveMismatch"
  | "UpdateNetwork"
  | "UpdateRateLimited"
  | "UpdateBadRelease"
  | "UpdateChecksum"
  | "UpdateSignature"
  | "UpdateUnverifiable"
  | "UpdateGone"
  | "UpdateInstall"
  | "Unknown";

// ---------- 應用內檢查更新（`src-tauri/src/commands/update.rs`） ----------

/** 查到的新版本。 */
export interface UpdateAvailable {
  /** 對外的版號（`規格/05`）。 */
  version: string;
  code: number;
  /** 那一版的變更摘要（GitHub Release 的說明，原樣的文字）。 */
  notes: string;
  /** 安裝檔的大小（位元組）。 */
  size: number;
}

export interface UpdateStatus {
  /** 這台能用應用內更新（Android）。電腦上為假，畫面不顯示這一節。 */
  supported: boolean;
  /** 已安裝的版本。 */
  current: string;
  /** 看過「連到哪裡、送出什麼」並同意。 */
  consented: boolean;
  autoCheck: boolean;
  /** 首頁問過「要不要在有新版時通知」。 */
  invited: boolean;
  lastCheck: number | null;
  /** 首頁提示被關掉的那一版。 */
  dismissed: string | null;
  available: UpdateAvailable | null;
  /** 已下載、核對過、還沒裝的那一版。 */
  downloaded: string | null;
  download: { running: boolean; done: number; total: number };
  /** 連到哪裡（同意的說明用）。 */
  host: string;
  /** 現在是不是不計流量的網路；查不到時 `null`。 */
  unmetered: boolean | null;
  /** 小米而且「MIUI 最佳化」開著：按「更新」前先說明可能被系統中止。 */
  miuiOptimized: boolean;
}

export interface UpdatePatch {
  consented?: boolean;
  autoCheck?: boolean;
  invited?: boolean;
  dismissed?: string;
}

/**
 * 系統安裝程式對這一份下載的最近一次結果（`commands/update.rs` 的 `outcome`）。裝成功時應用會被系統結束，所以沒有「成功」。
 * `waiting`：交給系統了、還沒有結果；`none`：沒有要說的；`stalled`：系統一直沒有回報（報告的狀態是 -1）；
 * `cancelled`：使用者在確認畫面按了取消；其餘是各種失敗。
 */
export type InstallOutcome =
  | "waiting"
  | "none"
  | "stalled"
  | "cancelled"
  | "aborted"
  | "invalid"
  | "conflict"
  | "storage"
  | "incompatible"
  | "blocked"
  | "timeout"
  | "failed";

/** 裝置與系統（讀不到的欄位是 null 或沒有）。 */
export interface DeviceInfo {
  manufacturer: string;
  brand: string;
  model: string;
  display: string;
  release: string;
  sdk: number;
  incremental: string;
  miuiVersion?: string | null;
  hyperOsVersion?: string | null;
  miuiOptimization?: string | null;
}

/** 一次安裝的系統原話與環境資訊（平台轉接器存在應用私有空間，不送出）。時間是 Unix 毫秒。 */
export interface InstallReport {
  status: number;
  statusMessage?: string | null;
  legacyStatus?: number | null;
  sessionId?: number | null;
  commitAt?: number | null;
  confirmAt?: number | null;
  at: number;
  targetCode?: number | null;
  confirmPackage?: string | null;
  confirmComponent?: string | null;
  confirmAction?: string | null;
  installedName?: string | null;
  installedCode?: number | null;
  installer?: string | null;
  initiator?: string | null;
  canRequestInstalls?: boolean | null;
  device: DeviceInfo;
}

export interface InstallResult {
  outcome: InstallOutcome;
  report: InstallReport | null;
}

// ---------- 翻譯（產生） ----------

export type TranslateProvider =
  | "anthropic"
  | "openai"
  | "gemini"
  | "openai_compatible"
  | "deepl"
  | "google_translate";

/** 離線（在這支手機上翻）的引擎：目前只有 Firefox 翻譯。 */
export type LocalEngine = "bergamot";

/** 引擎申報（外殼 `translate_engines`，`app/translate/src/engines.rs`）：畫面照它顯示欄位、過濾語言、決定要不要詢問。 */
export interface EngineInfo {
  provider: TranslateProvider | LocalEngine;
  kind: "chat" | "mt" | "on_device";
  key: "no" | "optional" | "required";
  model: "no" | "optional" | "required";
  endpoint: "no" | "optional" | "required";
  defaultModel: string;
  officialHost: string;
  /** 論文的文字會不會離開這台裝置（會的話送出前要詢問）。 */
  leavesDevice: boolean;
  maxSegments: number;
  maxChars: number;
  glossary: boolean;
  prompt: boolean;
  downloadsModels: boolean;
  targets: { code: string; engineCode: string }[];
}

/** 一個翻譯服務（具名的實例，`src-tauri/src/translate.rs`）。同一種引擎可以有多筆。 */
export interface TranslateService {
  /** 代號：金鑰的名稱也是它。 */
  id: string;
  /** 使用者取的名稱；空白時顯示引擎的名稱。 */
  name: string;
  provider: TranslateProvider | LocalEngine;
  model: string;
  endpoint: string;
  enabled: boolean;
  /** 每分鐘最多幾次請求；0 是不限。 */
  rpm: number;
  /** 單價（美元／百萬詞元），只用來粗估花費；沒填是 `null`（只顯示詞元數）。 */
  prices: { input: number; output: number } | null;
}

/** 畫面看到的一個服務：設定加上有沒有金鑰、文字實際送到哪個主機（離線引擎是空的）。 */
export interface TranslateServiceView extends TranslateService {
  hasKey: boolean;
  host: string;
}

/** 預設組的用途。 */
export type PresetKind = "translation" | "digest";

/** 重點筆記預設組的一個區段：代號（回覆 JSON 的鍵）、名稱（空白時用內建的名稱）、要寫什麼、最多幾點。 */
export interface PromptSection {
  id: string;
  label: string;
  about: string;
  max: number;
}

/** 提示詞預設組。內建的名稱由文字表提供（`name` 是空的）。 */
export interface PromptPreset {
  id: string;
  name: string;
  kind: PresetKind;
  text: string;
  /** 重點筆記的區段；翻譯的是空的。 */
  sections: PromptSection[];
}

export interface PromptPresetView extends PromptPreset {
  builtin: boolean;
  /** 內建的、改過（可以還原預設）。 */
  modified: boolean;
}

/** 看完整提示詞：送出的各部分，固定的標 `fixed`（`translate/src/prompt.rs` 的 `PromptPart`）。 */
export interface PromptPart {
  key: "preset" | "sections" | "rules" | "format" | "refine" | "stage" | "glossary" | "input";
  fixed: boolean;
  text: string;
}

export interface PromptPreview {
  /** 哪一種請求：翻一批、第二輪潤飾、重點筆記的一部分。 */
  kind: "translate" | "refine" | "digest";
  system: PromptPart[];
  user: PromptPart[];
}

/** 這篇用一個服務的累計用量（詞元數）。 */
export interface ServiceUsage {
  service: string;
  input: number;
  output: number;
}

/** 畫面看到的翻譯設定：不含金鑰。 */
export interface TranslateSettingsView {
  services: TranslateServiceView[];
  primary: string;
  fallbacks: string[];
  /** 空白讓服務自己判斷。 */
  sourceLang: string;
  targetLang: string;
  skipConfirm: boolean;
  prompt: string;
  /** 重點筆記選用的預設組。 */
  digestPrompt: string;
  /** 內建的在前（改過的是改過的內容），自訂的在後；兩種用途都在。 */
  prompts: PromptPresetView[];
  refine: boolean;
  /** 離線引擎的語言模型只在 Wi-Fi 下載（在 Firefox 翻譯那一筆的頁面裡設）。 */
  wifiOnly: boolean;
  secretsSupported: boolean;
  /** 上次用來翻的服務（代號）；翻譯面板照它選好。沒有是 `null`。 */
  lastService: string | null;
  /** 上次用來產生重點筆記的服務（代號）；重點筆記面板照它選好。沒有是 `null`。 */
  lastDigestService: string | null;
}

/** 只改給了的欄位（`translate_settings_patch`）。 */
export interface TranslateSettingsPatch {
  primary?: string;
  fallbacks?: string[];
  sourceLang?: string;
  targetLang?: string;
  skipConfirm?: boolean;
  prompt?: string;
  digestPrompt?: string;
  prompts?: PromptPreset[];
  refine?: boolean;
  wifiOnly?: boolean;
  /** 這次用來翻的服務（翻譯面板開始時記下）。 */
  lastService?: string;
  /** 這次用來產生重點筆記的服務（重點筆記面板開始時記下）。 */
  lastDigestService?: string;
}

/** 術語庫的一條。 */
export interface GlossaryTerm {
  source: string;
  target: string;
  note: string;
}

export interface GlossaryImport {
  cancelled: boolean;
  added: number;
  updated: number;
  skipped: number;
}

/** 並排比較：一個服務對選的段落交回的譯文（和要求的段落一一對應）。 */
/** 測試一個服務：送出的句子、收到的譯文、模型（對話模型以外是空的）、花了幾毫秒。 */
export interface TranslateTestResult {
  sent: string;
  received: string;
  model: string;
  ms: number;
}

export interface CompareColumn {
  service: string;
  texts: (string | null)[];
  error: ErrorKind | null;
}

export type InterchangeFormat = "xliff" | "tmx";

/** Firefox 翻譯的一個語言組合的模型。 */
export interface BergamotModel {
  id: string;
  target: string;
  downloaded: boolean;
  bytes: number;
  downloadBytes: number;
}

/** 離線引擎（Firefox 翻譯）的模型狀態。 */
export interface OnDeviceStatus {
  targetLang: string;
  bergamot: BergamotModel[];
  /** `null`：沒有這個語言組合的模型。 */
  bergamotReady: boolean | null;
  bergamotHost: string;
  download: { running: boolean; done: number; total: number };
  unmetered: boolean | null;
}

/** 版面模型（只用在翻譯的分段）的狀態。 */
export interface LayoutModelStatus {
  /** 這個建置有沒有版面模型；沒有時畫面不顯示這一項。 */
  supported: boolean;
  downloaded: boolean;
  /** 已下載的大小（沒下載是 0）。 */
  bytes: number;
  /** 要下載的大小。 */
  downloadBytes: number;
  host: string;
  download: { running: boolean; done: number; total: number };
  unmetered: boolean | null;
}

/** 外殼交給畫面一側引擎（Firefox 翻譯）的一批字。 */
export interface OnDeviceRequest {
  id: number;
  pair: string;
  texts: string[];
}

/** 畫面抽好的一段（版面與原文）。 */
export interface SegmentInput {
  page: number;
  bbox: [number, number, number, number];
  kind: string;
  size: number | null;
  source: string;
}

export type SegmentStatus = "pending" | "done" | "failed";

/** 譯文疊加檔裡的一段（`core/src/overlay.rs`）。 */
export interface OverlaySegment {
  id: string;
  /** 頁序，從 0 起算。 */
  page: number;
  /** PDF 頁面座標 `[x0, y0, x1, y1]`（原點在左下）。 */
  bbox: [number, number, number, number];
  kind: string;
  size: number | null;
  source: string;
  target: string | null;
  status: SegmentStatus;
  error: string | null;
  /** 已翻的段落經自動檢查留下的問題代號（`translate/src/check.rs`）；沒有問題時不存在。 */
  issues?: string[];
  /** 翻這一段的服務；待翻的、或這一版之前翻的沒有。 */
  service?: SegmentService | null;
  /** 這一段最後一次更新的時間；建立後還沒更新過是 `null`。 */
  modified?: string | null;
  /** 譯文是使用者決定的（並排比較按「用這個」）：任何整批的翻譯都不蓋掉。沒有時不存在。 */
  by_user?: boolean;
}

/** 翻一段的服務：這台設定裡那一筆的代號，加上引擎種類與模型（別台、或那一筆刪掉了時看這兩個）。 */
export interface SegmentService {
  id: string;
  provider: string;
  model: string;
}

export interface OverlayFile {
  format: number;
  source_material: string;
  provider: string;
  model: string;
  source_lang: string | null;
  target_lang: string;
  created: string | null;
  modified: string | null;
  segments: OverlaySegment[];
  /** 這份譯本的術語（翻譯時學的、使用者改的）。這一版之前的檔沒有。 */
  terms?: OverlayTerm[];
}

/** 術語的狀態：候選（還不用）、學到的、使用者定的（照用）、使用者刪掉的（不用也不再學）。 */
export type TermState = "candidate" | "learned" | "user" | "removed";

/** 這份譯本的一條術語。 */
export interface OverlayTerm {
  source: string;
  target: string;
  /** 這一版不認得的狀態原樣保留。 */
  state: TermState | string;
  batches: number;
}

export type JobState = "running" | "finished" | "cancelled" | "stopped";

/** 主要服務停擺時，依序可以改用的下一個服務（畫面先問使用者）。 */
export interface FallbackOffer {
  id: string;
  name: string;
  provider: TranslateProvider | LocalEngine;
  host: string;
}

export interface JobStatus {
  paperId: string;
  materialId: string;
  /** 這次用的服務（代號）。 */
  service: string;
  total: number;
  done: number;
  failed: number;
  state: JobState;
  reason: ErrorKind | null;
  fallback: FallbackOffer | null;
  /** 這次服務回報的用量（詞元數；服務沒有回報是 0）。 */
  inputTokens: number;
  outputTokens: number;
}

/** 材料的形式：`pdf`，或疊加用的譯文疊加檔 `overlay`。 */
export const FORM_OVERLAY = "overlay";

// ---------- 重點筆記（核心的 `digest`） ----------

export const ROLE_DIGEST = "digest";
export const FORM_MARKDOWN = "markdown";
/** 重點筆記的一個區段（檔案記下的；舊檔是內建五區、名稱空白，核心給）。 */
export interface DigestSection {
  id: string;
  label: string;
}

export interface DigestPoint {
  id: string;
  section: string;
  text: string;
  /** 原文的頁序，從 0 起算。 */
  pages: number[];
  segments: string[];
  created: string;
}

export interface DigestDoc {
  format: number;
  paper: string | null;
  source_material: string | null;
  source_sha256: string | null;
  service: string | null;
  provider: string | null;
  model: string | null;
  lang: string | null;
  created: string | null;
  created_by: string | null;
  modified: string | null;
  modified_by: string | null;
  proofread: boolean;
  /** 區段，依顯示的順序。名稱空白的用畫面的文字表。 */
  sections: DigestSection[];
  /** 屬性區的區段讀不懂（那一行也在 `unreadable`）：區段照每一點前的標題。 */
  sections_unreadable: boolean;
  title: string | null;
  points: DigestPoint[];
  unreadable: UnreadableBlock[];
}

export interface DigestView {
  doc: DigestDoc;
  /** 版本記號：刪一點、標已校對時帶回去。 */
  token: string;
}

/** 重點筆記工作的狀態。 */
export interface DigestStatus {
  paperId: string;
  service: string;
  state: JobState;
  /** 正在做第幾部分（從 1 起算）、共幾部分；`merging` 是正在合併。 */
  part: number;
  parts: number;
  merging: boolean;
  reason: ErrorKind | null;
  fallback: FallbackOffer | null;
  materialId: string | null;
  points: number;
  dropped: number;
  /** 這次服務回報的用量（詞元數；服務沒有回報是 0）。 */
  inputTokens: number;
  outputTokens: number;
}

/** 這篇的原文 PDF（沒有是 `undefined`）。 */
export function sourcePdf<T extends { role: string; form: string }>(materials: T[]): T | undefined {
  return materials.find((m) => m.role === ROLE_SOURCE && m.form === "pdf");
}

/** 這篇最後做好的那份重點筆記（沒有是 `undefined`）。 */
export function lastDigest<T extends { role: string; form: string }>(materials: T[]): T | undefined {
  const all = materials.filter((m) => m.role === ROLE_DIGEST && m.form === FORM_MARKDOWN);
  return all[all.length - 1];
}

/** 存下產生的譯本 PDF 的結果。 */
export interface GeneratedPut {
  materialId: string;
  outcome: "created" | "updated" | "unchanged";
}

/** 這份材料是不是由譯文疊加檔產生的譯本 PDF。 */
export function isGeneratedPdf(m: Material): boolean {
  return m.role === ROLE_TRANSLATION && m.form === "pdf" && m.provenance?.method === "generated";
}

// ---------- 匯出與列印 ----------

/** 準備好、等著交給使用者的一份檔案（`export_*` 指令的結果）。 */
export interface ExportFile {
  /** 檔名。 */
  name: string;
  /** 暫存檔的路徑，交給 `deliverExport`。 */
  path: string;
}

/** 引用格式：APA 第 7 版、IEEE、Chicago 作者－年份、MLA 第 9 版。 */
export type CitationStyle = "apa" | "ieee" | "chicago" | "mla";

export interface CitationExport {
  /** 複製到剪貼簿時沒有檔案。 */
  file: ExportFile | null;
  count: number;
  skipped: number;
  /** 缺作者或年份的幾篇（那一段照格式略過）。 */
  missing: Array<{ id: string; title: string }>;
}

export interface BibExport {
  file: ExportFile;
  /** 寫進檔案的篇數。 */
  count: number;
  /** 讀不出書目、沒有寫進去的篇數。 */
  skipped: number;
}

export type BibFormat = "bibtex" | "ris" | "csljson";
export type DeliverAction = "share" | "save" | "print";
export type DeliverResult = "shared" | "saved" | "cancelled" | "printing";
