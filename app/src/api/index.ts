// 畫面唯一呼叫指令的地方。
// 指令一對一轉給核心的操作；這裡只負責呼叫與把錯誤包成 `AppError`。

import { invoke } from "@tauri-apps/api/core";
import type {
  AddBatch,
  AddedHighlight,
  DeleteGroupResult,
  ExportResult,
  Group,
  HighlightInput,
  InkFile,
  NotesView,
  Stroke,
  StrokeInput,
  RestoreResult,
  TranslationResult,
  JobStatus,
  OverlayFile,
  SegmentInput,
  TranslateProvider,
  TranslateSettingsView,
  TranslateSettingsPatch,
  TranslateService,
  PromptPreset,
  PromptPreview,
  OverlayTerm,
  ServiceUsage,
  GlossaryTerm,
  GlossaryImport,
  CompareColumn,
  QuickResult,
  TranslateTestResult,
  InterchangeFormat,
  LocalEngine,
  EngineInfo,
  OnDeviceRequest,
  OnDeviceStatus,
  LayoutModelStatus,
  AppInfo,
  Platform,
  BibDraft,
  BibFill,
  JudgmentAdded,
  JudgmentKind,
  ReferencesDraft,
  TextDraft,
  TextState,
  BibExport,
  BibFormat,
  CitationExport,
  CitationStyle,
  DeliverAction,
  DeliverResult,
  ExportFile,
  GeneratedPut,
  ErrorKind,
  DigestStatus,
  DigestView,
  HistoryEntry,
  Insets,
  LibraryStatus,
  MoveOutcome,
  MovePlan,
  MoveProgress,
  PaperDetail,
  PaperSummary,
  Prefs,
  Problem,
  ReadingProgress,
  ScanDiff,
  StartupNotice,
  Storage,
  ConflictChoice,
  SyncExportResult,
  SyncImportReport,
  SyncPrepared,
  ImportPreview,
  ImportReport,
  ImportScanned,
  ImportStep,
  Theme,
  TrashEntry,
  RemovedMaterialEntry,
  PurgeReport,
  UpdateAvailable,
  InstallResult,
  UpdatePatch,
  UpdateStatus,
} from "./types";

export type * from "./types";
// 版面模型交回的區塊：形狀定在分段的純函式那裡（Node 的測試也用），這裡只借用型別
import type { RegionBox } from "../translate/regions";

// 用 `Record<ErrorKind, …>` 列：`ErrorKind` 多了一種而這裡沒列，型別檢查就不過（漏列的種類會被當成 Unknown）
const KNOWN_KIND_TABLE: Record<Exclude<ErrorKind, "Unknown">, true> = {
  PermissionDenied: true,
  NoSpace: true,
  NotFound: true,
  Conflict: true,
  FormatTooNew: true,
  ReadOnly: true,
  Unreadable: true,
  InvalidInput: true,
  Io: true,
  NoLibrary: true,
  NotPdf: true,
  Unsupported: true,
  Platform: true,
  TranslateAuth: true,
  TranslateConfig: true,
  TranslateRateLimited: true,
  TranslateNetwork: true,
  TranslateServer: true,
  TranslateBadResponse: true,
  TranslateBusy: true,
  TranslateQuota: true,
  TranslateNoModel: true,
  TranslateLanguage: true,
  NotOnWifi: true,
  NoFont: true,
  SyncOtherLibrary: true,
  SyncOwnPackage: true,
  SyncBackupFile: true,
  SyncIncomplete: true,
  NotBackupFile: true,
  RestoreGotSyncPackage: true,
  ImportZipBackslash: true,
  SharedNotEmpty: true,
  MoveMismatch: true,
  UpdateNetwork: true,
  UpdateRateLimited: true,
  UpdateBadRelease: true,
  UpdateChecksum: true,
  UpdateSignature: true,
  UpdateUnverifiable: true,
  UpdateGone: true,
  UpdateInstall: true,
};
const KNOWN_KINDS: ReadonlySet<string> = new Set(Object.keys(KNOWN_KIND_TABLE));

/** 指令失敗。畫面依 `kind` 從文字表取白話訊息，不顯示 `detail`。 */
export class AppError extends Error {
  readonly kind: ErrorKind;

  constructor(kind: ErrorKind, detail: string) {
    super(detail);
    this.name = "AppError";
    this.kind = kind;
  }
}

/** 接到的錯誤的種類；不是指令的錯誤（程式的問題）是 `Unknown`。 */
export function kindOf(e: unknown): ErrorKind {
  return e instanceof AppError ? e.kind : "Unknown";
}

function toAppError(e: unknown): AppError {
  if (e instanceof AppError) return e;
  if (typeof e === "object" && e !== null && "kind" in e) {
    const { kind, detail } = e as { kind: unknown; detail?: unknown };
    if (typeof kind === "string" && KNOWN_KINDS.has(kind)) {
      return new AppError(kind as ErrorKind, typeof detail === "string" ? detail : "");
    }
  }
  return new AppError("Unknown", "");
}

/** 呼叫一個指令，失敗時包成 `AppError`。外掛呼叫自己的指令也用它，不另寫一份。 */
export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toAppError(e);
  }
}

let platformOnce: Promise<Platform> | null = null;

export const api = {
  // 文獻庫
  libraryStatus: () => call<LibraryStatus>("library_status"),
  /** `storage` 為 `null`：剛從授權頁回來，由外殼依權限決定放哪裡。 */
  libraryCreate: (storage: Storage | null) => call<LibraryStatus>("library_create", { storage }),
  /** 應用專屬空間的文獻庫是空的、共用空間已經有文獻庫時，改用那一個（要先有「所有檔案存取」）。 */
  librarySwitchToShared: () => call<LibraryStatus>("library_switch_to_shared"),
  /** 搬到共用空間之前的估計：搬去哪裡、要多少空間、是不是改用那裡已有的。 */
  libraryMovePlan: () => call<MovePlan>("library_move_plan"),
  /** 把應用專屬空間的文獻庫搬到共用空間。做完才交回；進度另外問。 */
  libraryMoveToShared: () => call<MoveOutcome>("library_move_to_shared"),
  libraryMoveProgress: () => call<MoveProgress>("library_move_progress"),
  /** 取消搬移：核對之前才收，交回收了沒有。 */
  libraryMoveCancel: () => call<boolean>("library_move_cancel"),
  rescan: () => call<ScanDiff>("library_rescan"),
  check: (full: boolean) => call<Problem[]>("library_check", { full }),
  openAllFilesSettings: () => call<void>("open_all_files_settings"),

  // 目錄
  listPapers: () => call<PaperSummary[]>("list_papers"),
  continueReading: () => call<string | null>("continue_reading"),

  // 文獻
  getPaper: (id: string) => call<PaperDetail>("get_paper", { id }),
  updateBib: (id: string, draft: BibDraft, token: string) =>
    call<string>("update_bib", { id, draft, token }),
  removePaper: (id: string) => call<void>("remove_paper", { id }),
  restorePaper: (id: string) => call<void>("restore_paper", { id }),
  listTrash: () => call<TrashEntry[]>("list_trash"),
  /** 把一項材料（原文以外）移到回收區；交回 `paper.json` 新的版本記號。 */
  removeMaterial: (paperId: string, materialId: string, token: string) =>
    call<string>("remove_material", { paperId, materialId, token }),
  listRemovedMaterials: () => call<RemovedMaterialEntry[]>("list_removed_materials"),
  /** 永久刪除回收區裡的一項（回收區裡的資料夾名稱）。 */
  purgeTrashEntry: (folder: string) => call<void>("purge_trash_entry", { folder }),
  /** 把回收區裡移除的材料放回那篇（回收區裡的資料夾名稱）；交回那篇的代號。 */
  restoreMaterial: (folder: string) => call<string>("restore_material", { folder }),
  emptyTrash: () => call<PurgeReport>("empty_trash"),

  /** 記下一項材料的總頁數（PDF 載入、知道頁數時呼叫；已經對了就不寫）。 */
  setMaterialPages: (paperId: string, materialId: string, pages: number) =>
    call<boolean>("set_material_pages", { paperId, materialId, pages }),
  /** 自動補書目：只補空著的年份、DOI、網址，不改書目的修改時間；補到了交回 `true`。 */
  fillBibBlanks: (paperId: string, fill: BibFill) => call<boolean>("fill_bib_blanks", { paperId, fill }),

  // 參考文獻清單（`references.json`）
  /** 寫入或取代抽出的部分；交回新的版本記號。 */
  referencesWrite: (paperId: string, draft: ReferencesDraft) =>
    call<string>("references_write", { paperId, draft }),
  linkJudgmentAdd: (paperId: string, index: number, kind: JudgmentKind, target: string, token: string) =>
    call<JudgmentAdded>("link_judgment_add", { paperId, index, kind, target, token }),
  linkJudgmentsRemove: (paperId: string, ids: string[], token: string) =>
    call<string>("link_judgments_remove", { paperId, ids, token }),

  // 抽出的文字（`text.json`）
  /** 寫入或整份取代一篇的抽出的文字；交回版本記號（內容相同、現有的版本較高時沒寫，交回現有的）。 */
  textWrite: (paperId: string, draft: TextDraft) => call<string>("text_write", { paperId, draft }),
  /** 一批文獻的檔頭（逐篇處理判斷做過沒有）；沒有檔、讀不懂、不在目錄是 `null`。 */
  textStates: (paperIds: string[]) => call<Record<string, TextState | null>>("text_states", { paperIds }),

  // 逐篇處理的紀錄（應用空間，每個文獻庫一份；內容由 `perPaper.ts` 定）
  perPaperStateGet: () => call<unknown>("per_paper_state_get"),
  perPaperStatePut: (record: unknown) => call<void>("per_paper_state_put", { record }),

  // 加入：系統選檔畫面可以一次選多份，逐一加入，結果逐份回報
  addFromPicker: () => call<AddBatch>("add_papers_from_picker"),
  /** 別的應用分享進來、還沒加入的份數（非 Android 是 0）。 */
  sharedPending: () => call<number>("shared_pending"),
  /** 領取分享進來的 PDF，逐一加入（和一次選多份同一條路）。沒有可以領的時 `cancelled` 為真。 */
  addShared: () => call<AddBatch>("add_shared_papers"),
  /** 選一份 PDF，加成這篇的譯本。 */
  addTranslationFromPicker: (paperId: string) =>
    call<TranslationResult>("add_translation_from_picker", { paperId }),

  // 群組（版本記號由外殼在每次呼叫前讀最新的，畫面不管）
  listGroups: () => call<Group[]>("list_groups"),
  createGroup: (name: string) => call<Group>("create_group", { name }),
  renameGroup: (id: string, name: string) => call<void>("rename_group", { id, name }),
  /** 刪除群組不會刪文獻；`skipped` 是沒能移出這個群組的文獻。 */
  deleteGroup: (id: string) => call<DeleteGroupResult>("delete_group", { id }),
  setGroupMember: (paperId: string, groupId: string, member: boolean) =>
    call<void>("set_group_member", { paperId, groupId, member }),

  // 筆記（版本記號由畫面保管：讀到的原樣交回寫入；`Conflict` 時重新讀）
  readNotes: (paperId: string) => call<NotesView>("read_notes", { paperId }),
  /** 有筆記（劃線、想法）的文獻代號（首頁的「有筆記」篩選）。 */
  addHighlight: (paperId: string, draft: HighlightInput, token: string | null) =>
    call<AddedHighlight>("add_highlight", { paperId, draft, token }),
  setHighlightComment: (paperId: string, highlightId: string, comment: string, token: string) =>
    call<string>("set_highlight_comment", { paperId, highlightId, comment, token }),
  deleteHighlight: (paperId: string, highlightId: string, token: string) =>
    call<string>("delete_highlight", { paperId, highlightId, token }),
  setThoughts: (paperId: string, text: string, token: string | null) =>
    call<string>("set_thoughts", { paperId, text, token }),

  /** 把這篇的筆記檔交給別的應用：`obsidian` 為真用 Obsidian 的連結開，否則開系統的選單。交回 `opened` 或 `no_app`。 */
  openNotesExternal: (paperId: string, obsidian: boolean) =>
    call<"opened" | "no_app" | "not_shareable">("open_notes_external", { paperId, obsidian }),

  // 筆跡（外殼自己讀最新的版本記號，畫面不管記號）
  readInk: (paperId: string) => call<InkFile>("read_ink", { paperId }),
  addStroke: (paperId: string, draft: StrokeInput) => call<Stroke>("add_stroke", { paperId, draft }),
  deleteStroke: (paperId: string, strokeId: string) => call<void>("delete_stroke", { paperId, strokeId }),
  clearInkPage: (paperId: string, materialId: string, pageIndex: number) =>
    call<void>("clear_ink_page", { paperId, materialId, pageIndex }),

  // 翻譯（產生）：設定、金鑰（只能寫入與清除，讀不回來）、譯文疊加檔、背景翻譯
  translateSettings: () => call<TranslateSettingsView>("translate_settings_get"),
  /** 只改給了的欄位（其他畫面剛改的不會被蓋掉）。 */
  patchTranslateSettings: (patch: TranslateSettingsPatch) => call<void>("translate_settings_patch", { patch }),
  addTranslateService: (provider: TranslateProvider) => call<string>("translate_service_add", { provider }),
  saveTranslateService: (service: TranslateService) => call<void>("translate_service_save", { service }),
  resetTranslateService: (id: string) => call<void>("translate_service_reset", { id }),
  deleteTranslateService: (id: string) => call<void>("translate_service_delete", { id }),
  setTranslateKey: (service: string, key: string) => call<void>("translate_key_set", { service, key }),
  clearTranslateKey: (service: string) => call<void>("translate_key_clear", { service }),
  /** 用這個服務翻一句短句：交回送出的、收到的、模型與花了多久。 */
  testTranslate: (service: string) => call<TranslateTestResult>("translate_test", { service }),
  /** 存一組提示詞預設組（依代號取代；代號是內建的就存成改過的內建組）。 */
  savePrompt: (preset: PromptPreset) => call<void>("translate_prompt_save", { preset }),
  /** 刪一組自訂的預設組，或把改過的內建組還原預設。 */
  deletePrompt: (id: string) => call<void>("translate_prompt_delete", { id }),
  /**
   * 看完整提示詞：用這組（可以是還沒存的草稿）替一段範例原文組出實際送出的每一次請求。`materialId` 是接著翻的那份譯本
   * （用它的語言與術語）；`refine` 開著時多一次第二輪潤飾。
   */
  previewPrompt: (preset: PromptPreset, paperId: string | null, materialId: string | null = null, refine = false) =>
    call<PromptPreview[]>("prompt_preview", { preset, paperId, materialId, refine }),
  /** 一組預設組哪裡不合規則（規則在外殼）：合規則是 `null`，否則是原因的代號。 */
  checkPrompt: (preset: PromptPreset) => call<string | null>("prompt_check", { preset }),
  /** 改、刪這份譯本的術語（整份換掉）。正在翻這份時不收。 */
  setOverlayTerms: (paperId: string, materialId: string, terms: OverlayTerm[]) =>
    call<void>("overlay_terms_set", { paperId, materialId, terms }),
  /** 這篇在這台裝置上用過的詞元數，依服務加總。 */
  usagePaper: (paperId: string) => call<ServiceUsage[]>("usage_paper", { paperId }),
  glossaryGet: () => call<GlossaryTerm[]>("glossary_get"),
  glossarySet: (terms: GlossaryTerm[]) => call<void>("glossary_set", { terms }),
  /** 開系統選檔畫面，匯入三欄 TSV。 */
  glossaryImport: () => call<GlossaryImport>("glossary_import"),
  glossaryTsv: () => call<string>("glossary_tsv"),
  /** 幾個服務各翻一次選的段落（不寫進譯本）。 */
  /**
   * 選取文字的快速翻譯：只送選取的文字與前後各一句；查過的不再送出；連不上而有離線模型時改用它。
   * 送出前的詢問由呼叫的一方先做。
   */
  translateQuick: (paperId: string, text: string, before: string | null, after: string | null, service: string) =>
    call<QuickResult>("translate_quick", { paperId, text, before, after, service }),
  translateCompare: (paperId: string, materialId: string, segmentIds: string[], services: string[]) =>
    call<CompareColumn[]>("translate_compare", { paperId, materialId, segmentIds, services }),
  /**
   * 一段譯文由使用者決定（`by_user`）：並排比較挑的記下是哪個服務翻的；
   * 就地改的 `service` 給 `null`（譯文是使用者寫的）。
   */
  overlayApply: (paperId: string, materialId: string, segmentId: string, text: string, service: string | null) =>
    call<void>("overlay_apply", { paperId, materialId, segmentId, text, service }),
  /** 逐段對照寫成 XLIFF 2.1 或 TMX 1.4b 的內容（接著用匯出的流程存檔）。 */
  overlayInterchange: (paperId: string, materialId: string, format: InterchangeFormat) =>
    call<{ name: string; text: string }>("overlay_interchange", { paperId, materialId, format }),
  /** `layoutModelPages`：依版面模型分段的頁（頁序）；只在這些頁上認跨欄、跨頁的接續。 */
  createOverlay: (paperId: string, sourceMaterial: string, segments: SegmentInput[], layoutModelPages: number[], service: string) =>
    call<{ materialId: string }>("overlay_create", { paperId, sourceMaterial, segments, layoutModelPages, service }),
  readOverlay: (paperId: string, materialId: string) =>
    call<OverlayFile>("overlay_read", { paperId, materialId }),
  /** 跨欄、跨頁被切成前後兩段的句子（接續）：每一對是（前一段, 後一段）的代號。 */
  overlayContinuations: (paperId: string, materialId: string) =>
    call<Array<[string, string]>>("overlay_continuations", { paperId, materialId }),
  /** 開始（或接著）翻：只翻還沒翻好的段落，已翻的與使用者挑的不動。背景進行，立刻交回。 */
  translateRun: (paperId: string, materialId: string, service: string) =>
    call<void>("translate_run", { paperId, materialId, service }),

  /** 全部引擎的申報。 */
  translateEngines: () => call<EngineInfo[]>("translate_engines"),

  // 應用內檢查更新（只有 Android）
  updateStatus: () => call<UpdateStatus>("update_status"),
  updateConfigure: (patch: UpdatePatch) => call<void>("update_configure", { patch }),
  /** `manual` 為假（打開應用時）：自動檢查關著或一天內查過就不連網，交回已知的結果。沒同意過一律不連網。 */
  updateCheck: (manual: boolean) => call<UpdateAvailable | null>("update_check", { manual }),
  /** 下載並核對；完成才交回那一版的版號。進度看 `updateStatus().download`。 */
  updateDownload: () => call<string>("update_download"),
  updateDownloadCancel: () => call<void>("update_download_cancel"),
  /** 交給系統安裝：`needs_permission`（要先允許「安裝不明應用程式」）或 `committed`。 */
  updateInstall: () => call<"needs_permission" | "committed">("update_install"),
  /** 系統對這一份下載的最近一次結果（存在應用私有空間，畫面重開也讀得到）。 */
  updateInstallStatus: () => call<InstallResult>("update_install_status"),
  /** 退路：把核對過的安裝檔交給系統的安裝程式。結果收不到；`opened` 或 `no_app`。 */
  updateOpenInstaller: () => call<"opened" | "no_app">("update_open_installer"),
  /** 「複製錯誤資訊」：只放到剪貼簿。 */
  updateCopyReport: (text: string) => call<void>("update_copy_report", { text }),
  updateOpenInstallSettings: () => call<void>("update_open_install_settings"),
  updateDiscard: () => call<void>("update_discard"),

  // 離線翻譯：狀態、下載與刪除模型、畫面一側引擎（Firefox 翻譯）的橋
  onDeviceStatus: () => call<OnDeviceStatus>("ondevice_status"),
  onDeviceDownload: (engine: LocalEngine, wifiOnly: boolean) =>
    call<void>("ondevice_download", { engine, wifiOnly }),
  onDeviceDownloadCancel: () => call<void>("ondevice_download_cancel"),
  /** Firefox 翻譯給語言組合（例如 `en-zh-Hant`）。 */
  onDeviceDelete: (engine: LocalEngine, model: string) => call<void>("ondevice_delete", { engine, model }),
  onDeviceNext: () => call<OnDeviceRequest | null>("ondevice_next"),
  onDeviceReply: (id: number, texts: string[] | null, error: string | null) =>
    call<void>("ondevice_reply", { id, texts, error }),
  /** Firefox 翻譯某個語言組合的模型檔。 */
  bergamotFile: (pair: string, name: string) => call<ArrayBuffer>("bergamot_file", { pair, name }),

  // 版面模型（只用在翻譯的分段）：狀態、下載與刪除模型檔、一頁的偵測
  layoutStatus: () => call<LayoutModelStatus>("layout_status"),
  layoutDownload: (wifiOnly: boolean) => call<void>("layout_download", { wifiOnly }),
  layoutDownloadCancel: () => call<void>("layout_download_cancel"),
  layoutDelete: () => call<void>("layout_delete"),
  /** `image` 是 800×800 的 RGB 以 Base64 編成的字串；`width`、`height` 是頁面的寬高（PDF 點）。 */
  layoutDetect: (image: string, width: number, height: number) =>
    call<RegionBox[]>("layout_detect", { image, width, height }),
  translateStatus: () => call<JobStatus | null>("translate_status"),

  // 重點筆記：開始（背景）、進度、取消、換服務接著做；讀、刪一點、標為已校對；匯出、用其他應用打開
  /** `segments` 是 `layout.ts` 分好的段落（依閱讀順序）；`abstractText` 另外帶給模型當前後文。 */
  digestRun: (
    paperId: string,
    sourceMaterial: string,
    segments: Array<{ page: number; source: string }>,
    abstractText: string | null,
    service: string,
    /** 新的一份寫好之後移到回收區的舊重點筆記（重跑預設更新同一份）。 */
    replaces: string | null = null,
  ) => call<void>("digest_run", { paperId, sourceMaterial, segments, abstractText, service, replaces }),
  /** 停下來之後用 `service` 接著做（做好的部分不重送）。 */
  digestContinue: (service: string) => call<void>("digest_continue", { service }),
  digestStatus: () => call<DigestStatus | null>("digest_status"),
  digestCancel: () => call<void>("digest_cancel"),
  digestRead: (paperId: string, materialId: string) => call<DigestView>("digest_read", { paperId, materialId }),
  /** 刪一點；交回新的版本記號。 */
  digestDeletePoint: (paperId: string, materialId: string, pointId: string, token: string) =>
    call<string>("digest_delete_point", { paperId, materialId, pointId, token }),
  digestSetProofread: (paperId: string, materialId: string, proofread: boolean, token: string) =>
    call<string>("digest_set_proofread", { paperId, materialId, proofread, token }),
  exportDigest: (paperId: string, materialId: string) => call<ExportFile>("export_digest", { paperId, materialId }),
  /** 用其他應用打開一份唯讀的複本：`opened`、`no_app` 或 `not_shareable`。 */
  digestOpenExternal: (paperId: string, materialId: string) =>
    call<string>("digest_open_external", { paperId, materialId }),
  translateCancel: () => call<void>("translate_cancel"),

  // 產生的譯本 PDF
  /** 系統內建的中日韓字型檔（唯讀；找不到是 `NoFont`）。 */
  systemCjkFont: async (): Promise<Uint8Array> => {
    try {
      return new Uint8Array(await invoke<ArrayBuffer>("system_cjk_font"));
    } catch (e) {
      throw toAppError(e);
    }
  },
  /**
   * 把原文 PDF 裡落在蓋住範圍內的文字指令刪掉（外殼用 lopdf 做）。
   * 交回新檔與刪掉的指令數；讀不了（例如加密）是 `Unreadable`。
   */
  stripCoveredText: async (
    pdf: Uint8Array,
    covers: Record<string, Array<[number, number, number, number]>>,
  ): Promise<{ bytes: Uint8Array; removed: number }> => {
    const json = new TextEncoder().encode(JSON.stringify(covers));
    const body = new Uint8Array(4 + json.length + pdf.length);
    new DataView(body.buffer).setUint32(0, json.length);
    body.set(json, 4);
    body.set(pdf, 4 + json.length);
    try {
      const r = new Uint8Array(await invoke<ArrayBuffer>("pdf_strip_text", body));
      return { removed: new DataView(r.buffer, r.byteOffset).getUint32(0), bytes: r.subarray(4) };
    } catch (e) {
      throw toAppError(e);
    }
  },
  /** 把組好的譯本 PDF 存成這篇的譯本材料（第一次新增，之後原地更新）。`overlayId` 是由哪一份譯文疊加檔產生。 */
  putGeneratedTranslation: async (
    paperId: string,
    overlayId: string,
    bytes: Uint8Array,
    pages: number,
  ): Promise<GeneratedPut> => {
    try {
      return await invoke<GeneratedPut>("translation_pdf_put", bytes, {
        headers: { "paper-id": paperId, "overlay-id": overlayId, pages: String(pages) },
      });
    } catch (e) {
      throw toAppError(e);
    }
  },

  // 備份
  /** 匯出：把整個文獻庫寫成備份檔放在暫存；耗時。接著用 `deliverExport` 交給使用者。 */
  exportBackupFile: () => call<ExportResult>("export_backup_file"),
  /** 選備份檔、還原、換過去（目前的文獻庫是空的就放在原位，否則放在旁邊）；成功後要重新整理目錄。 */
  restoreBackupFromPicker: () => call<RestoreResult>("restore_backup_from_picker"),
  /**
   * 從另一台加入：選同步包或備份檔，照種類加入或還原。還沒有文獻庫時放在正式位置；
   * `storage` 是放哪一種空間（`null`：剛從授權頁回來，有權限放共用空間）。
   */
  libraryJoinFromPicker: (storage: Storage | null) =>
    call<RestoreResult>("library_join_from_picker", { storage }),

  // 整批匯入：選資料夾或 zip → 掃描 → 預覽 → 一篇一步寫入。
  /** 開選擇畫面、掃描（不鎖文獻庫，`importCancel` 取消）。掃描好的留在外殼，接著 `prepareImport`。 */
  scanImport: (zip: boolean) => call<ImportScanned>("scan_import", { zip }),
  /** 和文獻庫比對成預覽；除了記憶體裡的計畫，什麼都不寫。 */
  prepareImport: () => call<ImportPreview>("prepare_import"),
  /** 寫下一篇。 */
  importStep: (previewId: string) => call<ImportStep>("import_step", { previewId }),
  /** 取消寫入、放棄預覽：已加入的保留。 */
  finishImport: (previewId: string) => call<ImportReport>("finish_import", { previewId }),
  /** 掃描的進度 `[已讀位元組, 總位元組]`；還不知道總量時是 `[0, 0]`。 */
  importProgress: () => call<[number, number]>("import_progress"),
  importCancel: () => call<void>("import_cancel"),
  /** 用其他應用打開一份 Markdown 材料（交出唯讀的複本）：`opened`、`no_app`、`not_shareable`。 */
  materialOpenExternal: (paperId: string, materialId: string) =>
    call<string>("material_open_external", { paperId, materialId }),

  // 同步包。匯出好的用 `deliverExport` 交出。
  /** `full` 假：只帶變更（另一台還沒匯入過時自動完整）。`stamp` 是檔名裡的當地時間（`20261116-1030`）。 */
  syncExport: (full: boolean, stamp: string) => call<SyncExportResult>("sync_export", { full, stamp }),
  /** 選檔、解到暫存、算出預覽；不寫文獻庫（補記的想法記錄除外）。 */
  syncImportPrepare: () => call<SyncPrepared>("sync_import_prepare"),
  /** 照預覽與每個衝突的選擇合併。 */
  syncImportApply: (previewId: string, choices: Array<[string, ConflictChoice]>) =>
    call<SyncImportReport>("sync_import_apply", { previewId, choices }),
  syncImportDiscard: (previewId: string) => call<void>("sync_import_discard", { previewId }),
  /** 把上一次沒做完的合併做完；沒有時是 `NotFound`。 */
  syncResume: () => call<SyncImportReport>("sync_resume"),
  /** 畫面已經說了「上一次沒做完的合併已經做完」：之後的狀況不再帶 `resumedSync`。 */
  syncResumedSeen: () => call<void>("sync_resumed_seen"),
  /** 匯出或匯入準備的進度 `[已處理, 全部]`。 */
  syncProgress: () => call<[number, number]>("sync_progress"),
  /** 這台裝置的代號。 */
  thisDevice: () => call<string>("this_device"),

  // 匯出與列印：先在暫存資料夾準備一份檔案，再交給系統的分享、「存到…」或列印
  exportOriginal: (paperId: string, materialId: string) =>
    call<ExportFile>("export_original", { paperId, materialId }),
  /** 這篇的 Markdown 筆記檔；還沒有筆記檔是 `NotFound`。 */
  exportNotes: (paperId: string) => call<ExportFile>("export_notes", { paperId }),
  /** 譯文疊加檔寫成 Markdown：只有譯文；`bilingual` 為真是原文與譯文逐段對照。 */
  exportTranslation: (paperId: string, materialId: string, bilingual: boolean) =>
    call<ExportFile>("export_translation", { paperId, materialId, bilingual }),
  /** 幾篇的書目一次寫成一份 BibTeX 或 RIS 檔；`name` 是檔名的前半（群組的名稱）。 */
  exportBibliography: (paperIds: string[], format: BibFormat, name: string | null) =>
    call<BibExport>("export_bibliography_file", { paperIds, format, name }),
  /** 幾篇的書目排成引用格式：`copy` 放到剪貼簿（純文字加 HTML），`file` 寫成一份 HTML 引用清單。 */
  exportCitations: (paperIds: string[], style: CitationStyle, action: "copy" | "file", name: string | null) =>
    call<CitationExport>("export_citations", { paperIds, style, action, name }),
  /** 把畫面組好的檔案（帶標註的 PDF）放進暫存資料夾。 */
  exportPutBytes: async (name: string, bytes: Uint8Array): Promise<ExportFile> => {
    try {
      return await invoke<ExportFile>("export_put_bytes", bytes, {
        headers: { "export-name": encodeURIComponent(name) },
      });
    } catch (e) {
      throw toAppError(e);
    }
  },
  /** 分享、存到…或列印準備好的檔案（列印只限 PDF）。備份檔與同步包也從這裡交出（存好之後外殼刪暫存）。 */
  deliverExport: (path: string, name: string, action: DeliverAction) =>
    call<DeliverResult>("deliver_export", { path, name, action }),

  // 封面縮圖（存在應用的快取，不進文獻庫；空的 ArrayBuffer 表示還沒有）
  thumbGet: (paperId: string) => call<ArrayBuffer>("thumb_get", { paperId }),
  thumbPut: async (paperId: string, bytes: ArrayBuffer): Promise<void> => {
    try {
      await invoke<void>("thumb_put", bytes, { headers: { "paper-id": paperId } });
    } catch (e) {
      throw toAppError(e);
    }
  },

  /** 別的應用開了 `paperreader://` 連結：交回還沒處理的那一條（只交一次）。 */
  takeLink: () => call<string | null>("take_link"),

  // 閱讀
  recordOpen: (paperId: string, materialId: string) =>
    call<void>("record_open", { paperId, materialId }),
  recordPosition: (
    paperId: string,
    materialId: string,
    page: number,
    offset: number,
    zoom: number | null,
    /** 畫面左緣在那一頁寬度的幾分之幾（放大讀一欄時）；不知道給 `null`。 */
    left: number | null,
  ) => call<boolean>("record_position", { paperId, materialId, page, offset, zoom, left }),
  setRead: (paperId: string, read: boolean) => call<void>("set_read", { paperId, read }),
  readingProgress: (paperId: string) => call<ReadingProgress>("reading_progress", { paperId }),
  /** 閱讀紀錄：每次打開一列，最近的在前。 */
  readingHistory: () => call<HistoryEntry[]>("reading_history"),
  materialSize: (paperId: string, materialId: string) =>
    call<number>("material_size", { paperId, materialId }),
  /** 分段讀材料的位元組（給 PDF 元件用，只讀）。 */
  readMaterialRange: (paperId: string, materialId: string, offset: number, len: number) =>
    call<ArrayBuffer>("read_material_range", { paperId, materialId, offset, len }),

  // 應用
  appInfo: () => call<AppInfo>("app_info"),
  /** 這個建置的作業系統（問一次就留著）。 */
  platform: (): Promise<Platform> => {
    platformOnce ??= call<AppInfo>("app_info").then((i) => i.platform);
    platformOnce.catch(() => {
      platformOnce = null;
    });
    return platformOnce;
  },
  /** 用系統的瀏覽器打開網頁（只接受 https）。 */
  openUrl: (url: string) => call<void>("open_url", { url }),
  startupNotice: () => call<StartupNotice>("startup_notice"),
  getTheme: () => call<Theme>("get_theme"),
  setTheme: (theme: Theme) => call<void>("set_theme", { theme }),
  getPrefs: () => call<Prefs>("get_prefs"),
  setPrefs: (prefs: Prefs) => call<void>("set_prefs", { prefs }),
  /** 閱讀時螢幕不自動關閉（離開閱讀要關掉）。 */
  setKeepAwake: (on: boolean) => call<void>("set_keep_awake", { on }),
  systemInsets: () => call<Insets>("system_insets"),
  systemBars: (dark: boolean) => call<void>("system_bars", { dark }),
};
