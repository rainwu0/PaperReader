//! 外掛在外殼一側唯一的接入處。外殼裡只有這個檔引用外掛的
//! crate（`scripts/ext-boundary.mjs` 確認）。
//!
//! - 指令：外掛的操作照指令的樣板包一層（取文獻庫、背景執行、轉錯誤）。名稱清單只寫一次，同時產生 `EXT_COMMANDS`
//!   與處理器；`lib.rs` 用 `dispatch` 把兩份清單接起來，加減外掛不必改 `lib.rs`。
//! - 共用的接法寫一次（`Slot<P>`、`refresh`、背景整理、摘要的讀寫），每個外掛在這裡寫一個 `impl ExtPlugin`：名稱、
//!   比對是否一律在背景、索引的開與建、從目錄摘要取自己要的部分、一篇更新的三步（要什麼、讀、套用）。trait 定義在這
//!   裡，外掛的 crate 不知道它，外掛之間也不互相依賴。
//! - 索引的位置：`<應用資料夾>/ext/<外掛名稱>/<文獻庫代號>/`，外掛的索引檔與外殼記的上次摘要都放這裡。每個外掛只記
//!   自己要的那部分摘要。
//! - 文獻庫有變的通知：外掛的指令執行之前，取文獻庫的計數變了（或換了文獻庫）就比一次目錄摘要，把新增、變了、
//!   移除的文獻交給外掛。文獻連結一次 50 篇以上在背景做；內文搜尋的比對與更新一律在背景。背景在跑時查詢照現有的
//!   索引交回，帶 `pending`；一次 50 篇以上、或整個重建（記下的摘要是空的）時另帶 `building`。

use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::state::AppState;
use paperreader_core::error::LibError;
use paperreader_core::store::{FileStamp, IndexRead, IndexSource, PaperDigest};
use paperreader_ext_fulltext as fulltext;
use paperreader_ext_links as links;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{Manager, State};

/// 一次變了這麼多篇以上就在背景整理，內文搜尋這時另帶 `building`。
const BACKGROUND_FROM: usize = 50;

// ---------- 指令的單一清單 ----------

macro_rules! ext_commands {
    ($($name:ident),* $(,)?) => {
        /// 外掛的指令名稱（`dispatch` 看名稱決定交給哪一份處理器）。
        pub const EXT_COMMANDS: &[&str] = &[$(stringify!($name)),*];
        fn handler<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
            tauri::generate_handler![$($name),*]
        }
    };
}
ext_commands!(
    links_for,
    links_cited_by,
    links_extract_state,
    fulltext_search,
    fulltext_hits
);

/// 應用自己的處理器加上外掛的：名稱在 `EXT_COMMANDS` 裡就交給外掛的，否則交給應用的。名稱在交出 `Invoke` 之前看。
pub fn dispatch<R: tauri::Runtime>(
    app: impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    let ext = handler::<R>();
    move |invoke| {
        if EXT_COMMANDS.contains(&invoke.message.command()) {
            ext(invoke)
        } else {
            app(invoke)
        }
    }
}

/// 啟動時登記外掛在外殼一側的狀態。
pub fn setup<R: tauri::Runtime>(app: &tauri::App<R>) {
    app.manage(Links::default());
    app.manage(Fulltext::default());
}

// ---------- 共用：每個外掛在外殼一側要交代的事 ----------

/// 一篇套用到索引失敗。
#[derive(Debug, PartialEq, Eq)]
pub enum ApplyError {
    /// 索引檔損壞：刪掉資料夾、重建。
    Corrupt,
    /// 其他：這一批不存，下次再比一次。
    Failed,
}

/// 每個外掛在外殼一側要交代的事。
pub trait ExtPlugin: Sized + 'static {
    /// 索引在 `<應用資料夾>/ext/<NAME>/<文獻庫代號>/`。
    const NAME: &'static str;
    /// 外殼記的摘要（`digests.json`）的格式版本。
    const DIGESTS_VERSION: u32;
    /// 比對與更新一律在背景（否則少於 50 篇當場做）。
    const ALWAYS_BACKGROUND: bool;
    type Index: Send + 'static;
    /// 從目錄摘要取的、這個外掛要的部分。
    type Stamp: Clone + PartialEq + Serialize + DeserializeOwned + Send + Sync + 'static;
    type Want: Send;
    type Input: Send;
    fn stamp(d: &PaperDigest) -> Self::Stamp;
    /// 讀現有的索引；沒有、讀不懂、版本不同交回 `None`。
    fn load(dir: &Path) -> Option<Self::Index>;
    /// 一份新的、空的索引。
    fn fresh(dir: &Path) -> ApiResult<Self::Index>;
    /// 索引的檔還在（被刪掉時要重建）。
    fn intact(_dir: &Path) -> bool {
        true
    }
    /// ① 拿索引的鎖：這篇要讀什麼。
    fn want(ix: &Self::Index, id: &str, now: &Self::Stamp) -> Self::Want;
    /// ② 拿文獻庫的鎖、不加計數：讀；不在目錄交回 `None`。
    fn read(s: &AppState, id: &str, want: &Self::Want) -> ApiResult<Option<Self::Input>>;
    /// ③ 拿索引的鎖：套用；`input` 是 `None` 時整篇移除。
    fn apply(
        ix: &mut Self::Index,
        id: &str,
        now: &Self::Stamp,
        input: Option<Self::Input>,
    ) -> Result<(), ApplyError>;
    fn remove(ix: &mut Self::Index, id: &str) -> Result<(), ApplyError>;
    /// 一批做完之後存（文獻連結寫分片；內文搜尋每篇一個交易，什麼都不做）。
    fn save(ix: &mut Self::Index, dir: &Path) -> ApiResult<()>;
}

#[derive(Serialize, Deserialize)]
struct DigestsFile<S> {
    version: u32,
    papers: HashMap<String, S>,
}

fn load_digests<P: ExtPlugin>(dir: &Path) -> Option<HashMap<String, P::Stamp>> {
    let bytes = std::fs::read(dir.join("digests.json")).ok()?;
    let f: DigestsFile<P::Stamp> = serde_json::from_slice(&bytes).ok()?;
    (f.version == P::DIGESTS_VERSION).then_some(f.papers)
}

fn save_digests<P: ExtPlugin>(dir: &Path, papers: &HashMap<String, P::Stamp>) -> ApiResult<()> {
    #[derive(Serialize)]
    struct Out<'a, S> {
        version: u32,
        papers: &'a HashMap<String, S>,
    }
    paperreader_core::jsonio::write_json_atomic(
        &dir.join("digests.json"),
        &Out {
            version: P::DIGESTS_VERSION,
            papers,
        },
    )?;
    Ok(())
}

/// 現在的目錄摘要裡這個外掛要的部分（不加取文獻庫的計數）。
fn current<P: ExtPlugin>(s: &AppState) -> ApiResult<HashMap<String, P::Stamp>> {
    s.with_lib_quiet(|l| {
        Ok(l.paper_digests()
            .into_iter()
            .map(|d| {
                let st = P::stamp(&d);
                (d.id, st)
            })
            .collect())
    })
}

/// 前後兩份摘要的差別：要重讀的（新增或變了）與移除的。
fn diff<S: PartialEq>(
    old: &HashMap<String, S>,
    now: &HashMap<String, S>,
) -> (Vec<String>, Vec<String>) {
    let mut changed: Vec<String> = now
        .iter()
        .filter(|(id, d)| old.get(*id) != Some(*d))
        .map(|(id, _)| id.clone())
        .collect();
    let mut removed: Vec<String> = old
        .keys()
        .filter(|id| !now.contains_key(*id))
        .cloned()
        .collect();
    changed.sort();
    removed.sort();
    (changed, removed)
}

pub struct Loaded<P: ExtPlugin> {
    library: String,
    dir: PathBuf,
    index: P::Index,
    digests: HashMap<String, P::Stamp>,
    /// 上次比到的取文獻庫的計數。
    compared: Option<u64>,
}

/// 一個外掛在外殼的狀態：載入的索引與記下的摘要（一次一個文獻庫）。
pub struct Slot<P: ExtPlugin> {
    slot: Arc<Mutex<Option<Loaded<P>>>>,
    /// 一次 50 篇以上在背景整理。
    building: Arc<AtomicBool>,
    /// 背景在比對或更新。
    pending: Arc<AtomicBool>,
}

impl<P: ExtPlugin> Default for Slot<P> {
    fn default() -> Self {
        Slot {
            slot: Arc::new(Mutex::new(None)),
            building: Arc::new(AtomicBool::new(false)),
            pending: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl<P: ExtPlugin> Clone for Slot<P> {
    fn clone(&self) -> Self {
        Slot {
            slot: self.slot.clone(),
            building: self.building.clone(),
            pending: self.pending.clone(),
        }
    }
}

impl<P: ExtPlugin> Slot<P> {
    fn lock(&self) -> MutexGuard<'_, Option<Loaded<P>>> {
        self.slot.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn building(&self) -> bool {
        self.building.load(Ordering::SeqCst)
    }

    fn pending(&self) -> bool {
        self.pending.load(Ordering::SeqCst)
    }
}

/// 載入那個文獻庫的索引與摘要；索引讀不到就建一份新的、摘要從空的起算（下一次比對整個重建）。
fn open<P: ExtPlugin>(library: String, dir: PathBuf) -> ApiResult<Loaded<P>> {
    let (index, digests) = match P::load(&dir) {
        Some(index) => (index, load_digests::<P>(&dir).unwrap_or_default()),
        None => (P::fresh(&dir)?, HashMap::new()),
    };
    Ok(Loaded {
        library,
        dir,
        index,
        digests,
        compared: None,
    })
}

/// 索引壞了：先放掉索引（關掉檔），再刪掉整個資料夾；下一次 `refresh` 建一份新的、整個重建。
fn reset<P: ExtPlugin>(g: &mut Option<Loaded<P>>) {
    if let Some(l) = g.take() {
        let dir = l.dir.clone();
        drop(l);
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// 外掛的指令執行之前：換了文獻庫（或索引的檔不見了）就載入它的索引；取文獻庫的計數變了就比一次目錄摘要、更新
/// 索引。背景已在跑時不另開。
fn refresh<P: ExtPlugin>(s: &AppState, slot: &Slot<P>) -> ApiResult<()> {
    if slot.pending() {
        return Ok(());
    }
    let library = s.with_lib_quiet(|l| Ok(l.library_file().id.clone()))?;
    let uses = s.lib_uses();
    let dir = s.app_dir().join("ext").join(P::NAME).join(&library);
    let mut g = slot.lock();
    if g.as_ref().is_none_or(|l| l.library != library) || !P::intact(&dir) {
        // 先放掉舊的（關掉索引檔），才動資料夾
        *g = None;
        *g = Some(open::<P>(library.clone(), dir)?);
    }
    let Some(loaded) = g.as_mut() else {
        return Ok(());
    };
    if loaded.compared.is_some_and(|c| c >= uses) {
        return Ok(());
    }
    if P::ALWAYS_BACKGROUND {
        // 比對目錄摘要也在背景：每篇要看幾個檔案屬性，幾千篇時不讓查詢等
        slot.pending.store(true, Ordering::SeqCst);
        // 記下的摘要是空的（新的索引、壞了或被刪掉）：整個重建，這次查詢就帶 building
        if loaded.digests.is_empty() {
            slot.building.store(true, Ordering::SeqCst);
        }
        drop(g);
        let (state, slot) = (s.clone(), slot.clone());
        std::thread::spawn(move || background::<P>(&state, &slot, &library, uses, None));
        return Ok(());
    }
    let now = current::<P>(s)?;
    let (changed, removed) = diff(&loaded.digests, &now);
    if changed.is_empty() && removed.is_empty() {
        loaded.compared = Some(uses);
        return Ok(());
    }
    if changed.len() + removed.len() < BACKGROUND_FROM {
        let failed = || ApiError::new(ErrorKind::Io, "extension index update failed");
        for id in &removed {
            P::remove(&mut loaded.index, id).map_err(|_| failed())?;
        }
        for id in &changed {
            let stamp = &now[id];
            let want = P::want(&loaded.index, id, stamp);
            let input = P::read(s, id, &want)?;
            P::apply(&mut loaded.index, id, stamp, input).map_err(|_| failed())?;
        }
        let dir = loaded.dir.clone();
        P::save(&mut loaded.index, &dir)?;
        save_digests::<P>(&dir, &now)?;
        loaded.digests = now;
        loaded.compared = Some(uses);
        return Ok(());
    }
    // 50 篇以上：在背景一篇一篇讀，讀一篇時才拿文獻庫的鎖，更新索引時才拿索引的鎖
    slot.pending.store(true, Ordering::SeqCst);
    slot.building.store(true, Ordering::SeqCst);
    drop(g);
    let (state, slot) = (s.clone(), slot.clone());
    std::thread::spawn(move || background::<P>(&state, &slot, &library, uses, Some(now)));
    Ok(())
}

/// 背景的比對與更新。每篇三步，每步只拿一把鎖：① 索引的鎖：要什麼；② 文獻庫的鎖（不加計數）：讀；③ 索引的鎖：
/// 套用。全部做完才存摘要；中途被結束，下次重送同一批（內文搜尋的 `stale` 會說沒有要重做的鍵）。換了文獻庫就不碰
/// 新載入的那一份。
fn background<P: ExtPlugin>(
    s: &AppState,
    slot: &Slot<P>,
    library: &str,
    uses: u64,
    now: Option<HashMap<String, P::Stamp>>,
) {
    let mine = |l: &Loaded<P>| l.library == library;
    let finish = |now: Option<HashMap<String, P::Stamp>>| {
        let mut g = slot.lock();
        if let Some(l) = g.as_mut().filter(|l| mine(l)) {
            match now {
                Some(now)
                    if P::save(&mut l.index, &l.dir.clone()).is_ok()
                        && save_digests::<P>(&l.dir, &now).is_ok() =>
                {
                    l.digests = now;
                    l.compared = Some(uses);
                }
                // 存不進去或中途失敗：下次再比一次
                _ => l.compared = None,
            }
        }
        slot.building.store(false, Ordering::SeqCst);
        slot.pending.store(false, Ordering::SeqCst);
    };
    let now = match now {
        Some(now) => now,
        None => match current::<P>(s) {
            Ok(now) => now,
            Err(_) => return finish(None),
        },
    };
    let (changed, removed, rebuild) = {
        let g = slot.lock();
        let Some(l) = g.as_ref().filter(|l| mine(l)) else {
            drop(g);
            return finish(None);
        };
        let (c, r) = diff(&l.digests, &now);
        // 記下的摘要是空的：整個重建（新的索引、索引壞了或被刪掉）
        (c, r, l.digests.is_empty())
    };
    if changed.len() + removed.len() >= BACKGROUND_FROM || (rebuild && !changed.is_empty()) {
        slot.building.store(true, Ordering::SeqCst);
    }
    #[cfg(test)]
    tests::wait_while_held(library);
    // 套用的結果：壞了就重建，其他失敗這一批不存
    let applied = |g: &mut Option<Loaded<P>>, r: Result<(), ApplyError>| -> bool {
        match r {
            Ok(()) => true,
            Err(ApplyError::Corrupt) => {
                reset(g);
                false
            }
            Err(ApplyError::Failed) => false,
        }
    };
    for id in &removed {
        let mut g = slot.lock();
        let r = match g.as_mut().filter(|l| mine(l)) {
            Some(l) => P::remove(&mut l.index, id),
            None => Err(ApplyError::Failed),
        };
        if !applied(&mut g, r) {
            drop(g);
            return finish(None);
        }
    }
    for id in &changed {
        let stamp = &now[id];
        let want = {
            let g = slot.lock();
            match g.as_ref().filter(|l| mine(l)) {
                Some(l) => P::want(&l.index, id, stamp),
                None => {
                    drop(g);
                    return finish(None);
                }
            }
        };
        let input = match P::read(s, id, &want) {
            Ok(i) => i,
            Err(_) => return finish(None),
        };
        let mut g = slot.lock();
        let r = match g.as_mut().filter(|l| mine(l)) {
            Some(l) => P::apply(&mut l.index, id, stamp, input),
            None => Err(ApplyError::Failed),
        };
        if !applied(&mut g, r) {
            drop(g);
            return finish(None);
        }
    }
    finish(Some(now));
}

/// 先 `refresh`，再在索引上做一件事（`building`、`pending` 一起交）。
fn with_index<P: ExtPlugin, T>(
    s: &AppState,
    slot: &Slot<P>,
    f: impl FnOnce(&P::Index, bool, bool) -> T,
) -> ApiResult<T> {
    refresh(s, slot)?;
    let (building, pending) = (slot.building(), slot.pending());
    let g = slot.lock();
    let l = g
        .as_ref()
        .ok_or_else(|| ApiError::new(ErrorKind::Io, "extension index is not loaded"))?;
    Ok(f(&l.index, building, pending))
}

// ---------- 文獻之間的連結 ----------

/// 一篇的摘要：書目的指紋、`references.json` 的大小與修改時間（`digests.json` 的格式不變）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Digest {
    bib: String,
    refs: Option<(u64, Option<i64>)>,
}

pub struct LinksExt;

/// 外掛「文獻之間的連結」在外殼的狀態。
pub type Links = Slot<LinksExt>;

/// 一篇給索引的資料，經核心公開的操作讀（每篇拿一次文獻庫的鎖、不加計數）。不在目錄了交回 `None`。
fn read_input(s: &AppState, id: &str) -> ApiResult<Option<links::PaperInput>> {
    s.with_lib_quiet(|l| {
        let paper = match l.get_paper(id) {
            Ok((p, _)) => p,
            Err(LibError::NotFound { .. }) => return Ok(None),
            Err(e) => return Err(e),
        };
        // 讀不懂的參考文獻清單當成沒有（「檢查文獻庫」會列出來）
        let references = l.read_references(id).ok().flatten();
        Ok(Some(links::PaperInput {
            id: id.to_string(),
            bib: paper.bib,
            references,
        }))
    })
}

impl ExtPlugin for LinksExt {
    const NAME: &'static str = "links";
    const DIGESTS_VERSION: u32 = 1;
    const ALWAYS_BACKGROUND: bool = false;
    type Index = links::LinkIndex;
    type Stamp = Digest;
    type Want = ();
    type Input = links::PaperInput;

    fn stamp(d: &PaperDigest) -> Digest {
        Digest {
            bib: d.bib_sha256.clone(),
            refs: d.references.map(|r| (r.bytes, r.modified_ms)),
        }
    }

    fn load(dir: &Path) -> Option<links::LinkIndex> {
        links::LinkIndex::load(dir)
    }

    fn fresh(_dir: &Path) -> ApiResult<links::LinkIndex> {
        Ok(links::LinkIndex::new())
    }

    fn want(_ix: &links::LinkIndex, _id: &str, _now: &Digest) {}

    fn read(s: &AppState, id: &str, _want: &()) -> ApiResult<Option<links::PaperInput>> {
        read_input(s, id)
    }

    fn apply(
        ix: &mut links::LinkIndex,
        id: &str,
        _now: &Digest,
        input: Option<links::PaperInput>,
    ) -> Result<(), ApplyError> {
        match input {
            Some(input) => ix.upsert(input),
            None => ix.remove(id),
        }
        Ok(())
    }

    fn remove(ix: &mut links::LinkIndex, id: &str) -> Result<(), ApplyError> {
        ix.remove(id);
        Ok(())
    }

    fn save(ix: &mut links::LinkIndex, dir: &Path) -> ApiResult<()> {
        ix.save(dir)
            .map_err(|e| paperreader_core::error::io_error(dir, e))?;
        Ok(())
    }
}

/// 那篇的每一條、每條連到哪篇；沒抽過是 `null`。
#[tauri::command]
pub async fn links_for(
    state: State<'_, AppState>,
    ext: State<'_, Links>,
    paper_id: String,
) -> ApiResult<Option<links::LinksView>> {
    let ext = ext.inner().clone();
    state
        .run("links_for", move |s| {
            with_index(s, &ext, |index, building, _| {
                index.links_for(&paper_id).map(|mut v| {
                    v.building = building;
                    v
                })
            })
        })
        .await
}

/// 庫裡引用這篇的：哪一篇、用哪幾條引的。
#[tauri::command]
pub async fn links_cited_by(
    state: State<'_, AppState>,
    ext: State<'_, Links>,
    paper_id: String,
) -> ApiResult<Vec<links::CitingPaper>> {
    let ext = ext.inner().clone();
    state
        .run("links_cited_by", move |s| {
            with_index(s, &ext, |index, _, _| index.cited_by(&paper_id))
        })
        .await
}

#[derive(Serialize)]
pub struct ExtractStates {
    /// 索引在背景整理中：逐篇處理這一輪先不判斷。
    building: bool,
    /// 每篇的 `{ source_sha256, version }`；沒抽過是 `null`。
    papers: HashMap<String, Option<links::ExtractState>>,
}

/// 逐篇處理判斷參考文獻做過沒有：一次問一批。
#[tauri::command]
pub async fn links_extract_state(
    state: State<'_, AppState>,
    ext: State<'_, Links>,
    paper_ids: Vec<String>,
) -> ApiResult<ExtractStates> {
    let ext = ext.inner().clone();
    state
        .run("links_extract_state", move |s| {
            with_index(s, &ext, |index, building, _| ExtractStates {
                building,
                papers: paper_ids
                    .into_iter()
                    .map(|id| {
                        let st = index.extract_state(&id);
                        (id, st)
                    })
                    .collect(),
            })
        })
        .await
}

// ---------- 內文搜尋 ----------

pub struct FulltextExt;

/// 一篇要重做的鍵；`false` 表示那個鍵的檔已經不在（只刪，不讀）。
type FulltextWant = Vec<(String, bool)>;

fn sql_error(e: fulltext::SqlError) -> ApiError {
    ApiError::new(ErrorKind::Io, e.to_string())
}

fn apply_error(e: &fulltext::SqlError) -> ApplyError {
    if fulltext::is_corrupt(e) {
        ApplyError::Corrupt
    } else {
        ApplyError::Failed
    }
}

impl ExtPlugin for FulltextExt {
    const NAME: &'static str = "fulltext";
    const DIGESTS_VERSION: u32 = 1;
    const ALWAYS_BACKGROUND: bool = true;
    type Index = fulltext::FulltextIndex;
    /// 全文搜尋的 `Stamp` 是目錄摘要的 `files`（不看書目）。
    type Stamp = fulltext::Stamps;
    type Want = FulltextWant;
    type Input = Vec<(String, Option<IndexSource>)>;

    fn stamp(d: &PaperDigest) -> fulltext::Stamps {
        d.files.clone()
    }

    fn load(dir: &Path) -> Option<fulltext::FulltextIndex> {
        fulltext::FulltextIndex::open(dir)
    }

    fn fresh(dir: &Path) -> ApiResult<fulltext::FulltextIndex> {
        // 打不開、版本不同：刪掉整個資料夾重建
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).map_err(|e| paperreader_core::error::io_error(dir, e))?;
        fulltext::FulltextIndex::create(dir).map_err(sql_error)
    }

    fn intact(dir: &Path) -> bool {
        dir.join(fulltext::INDEX_FILE).is_file()
    }

    fn want(ix: &fulltext::FulltextIndex, id: &str, now: &fulltext::Stamps) -> FulltextWant {
        ix.stale(id, now)
            .into_iter()
            .map(|k| {
                let present = now.contains_key(&k);
                (k, present)
            })
            .collect()
    }

    fn read(
        s: &AppState,
        id: &str,
        want: &FulltextWant,
    ) -> ApiResult<Option<Vec<(String, Option<IndexSource>)>>> {
        if want.is_empty() {
            return Ok(Some(Vec::new()));
        }
        s.with_lib_quiet(|l| {
            let mut out = Vec::with_capacity(want.len());
            for (key, present) in want {
                // 不在摘要裡的鍵（檔不見了）不讀：筆記檔不在時讀它會重掃整個筆記資料夾
                if !present {
                    out.push((key.clone(), None));
                    continue;
                }
                #[cfg(test)]
                tests::count_read(&l.library_file().id, key);
                match l.read_for_index(id, key, None) {
                    Ok(IndexRead::Read(_, src)) => out.push((key.clone(), Some(*src))),
                    Ok(_) => out.push((key.clone(), None)),
                    Err(LibError::NotFound { .. }) => return Ok(None),
                    Err(e) => return Err(e),
                }
            }
            Ok(Some(out))
        })
    }

    fn apply(
        ix: &mut fulltext::FulltextIndex,
        id: &str,
        now: &fulltext::Stamps,
        input: Option<Vec<(String, Option<IndexSource>)>>,
    ) -> Result<(), ApplyError> {
        match input {
            None => ix.remove(id).map_err(|e| apply_error(&e)),
            Some(v) if v.is_empty() => Ok(()),
            Some(v) => ix.put(id, now, v).map_err(|e| apply_error(&e)),
        }
    }

    fn remove(ix: &mut fulltext::FulltextIndex, id: &str) -> Result<(), ApplyError> {
        ix.remove(id).map_err(|e| apply_error(&e))
    }

    fn save(_ix: &mut fulltext::FulltextIndex, _dir: &Path) -> ApiResult<()> {
        Ok(())
    }
}

/// 讀檔的快取最多幾份。
const CACHE_MAX: usize = 40;
/// 一次幾篇、每篇最多列幾處。
const PAGE: usize = 10;
const SHOWN: usize = 3;
/// 「還有 K 處」展開時最多幾處。
const HITS_MAX: usize = 200;

struct Cached {
    stamp: FileStamp,
    src: Arc<IndexSource>,
    used: u64,
}

/// 查詢時讀檔的快取：（文獻庫、文獻、鍵）→（檔案屬性、內容），最多 40 份，最久沒用的先丟。
#[derive(Default)]
pub struct SourceCache {
    map: HashMap<(String, String, String), Cached>,
    tick: u64,
    /// 經核心真的讀了內容的次數（測試看）。
    reads: u64,
}

impl SourceCache {
    fn known(&self, k: &(String, String, String)) -> Option<FileStamp> {
        self.map.get(k).map(|c| c.stamp)
    }

    fn touch(&mut self, k: &(String, String, String)) -> Option<Arc<IndexSource>> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(k).map(|c| {
            c.used = tick;
            c.src.clone()
        })
    }

    fn put(&mut self, k: (String, String, String), stamp: FileStamp, src: Arc<IndexSource>) {
        self.tick += 1;
        self.reads += 1;
        self.map.insert(
            k,
            Cached {
                stamp,
                src,
                used: self.tick,
            },
        );
        while self.map.len() > CACHE_MAX {
            let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, c)| c.used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.map.remove(&oldest);
        }
    }
}

/// 外掛「內文搜尋」在外殼的狀態：索引與摘要、查詢時讀檔的快取。
#[derive(Default, Clone)]
pub struct Fulltext {
    slot: Slot<FulltextExt>,
    cache: Arc<Mutex<SourceCache>>,
}

impl Fulltext {
    fn cache(&self) -> MutexGuard<'_, SourceCache> {
        self.cache.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 經快取讀一份來源（文獻庫的鎖、不加計數；檔案屬性沒變不重讀）。沒有檔、讀不懂、不在目錄交回 `None`。
    fn source(
        &self,
        s: &AppState,
        library: &str,
        paper: &str,
        key: &str,
    ) -> ApiResult<Option<Arc<IndexSource>>> {
        let k = (library.to_string(), paper.to_string(), key.to_string());
        let known = self.cache().known(&k);
        let r = s.with_lib_quiet(|l| match l.read_for_index(paper, key, known) {
            Err(LibError::NotFound { .. }) => Ok(IndexRead::Missing),
            other => other,
        })?;
        let mut cache = self.cache();
        Ok(match r {
            IndexRead::Unchanged => cache.touch(&k),
            IndexRead::Read(stamp, src) => {
                let src = Arc::new(*src);
                cache.put(k, stamp, src.clone());
                Some(src)
            }
            IndexRead::Missing => {
                cache.map.remove(&k);
                None
            }
        })
    }

    /// 一個候選處：讀那份來源、對回那個單位、找位置。找不到交回 `None`（檔在索引之後變了）。
    fn locate(
        &self,
        s: &AppState,
        library: &str,
        q: &fulltext::Query,
        c: &fulltext::UnitHit,
    ) -> ApiResult<Option<Hit>> {
        let Some(src) = self.source(s, library, &c.paper, &c.key)? else {
            return Ok(None);
        };
        let Some(unit) = fulltext::units_of(&c.key, &src)
            .into_iter()
            .find(|u| u.same_place(c.kind, c.page, c.target.as_deref()))
        else {
            return Ok(None);
        };
        Ok(fulltext::locate(q, &unit.text).map(|l| {
            let source = c.kind == fulltext::UnitKind::Source;
            Hit {
                kind: c.kind.name(),
                material: c.material.clone(),
                page: c.page,
                target: c.target.clone(),
                bbox: c.bbox,
                context: l.context,
                cut_start: l.cut_start,
                cut_end: l.cut_end,
                marks: l.marks,
                range: source.then_some(l.range),
                quote: source.then_some(l.quote),
            }
        }))
    }
}

/// 一處。`range`、`marks` 是 UTF-16 位置。
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    kind: &'static str,
    material: Option<String>,
    page: Option<u32>,
    target: Option<String>,
    #[serde(rename = "box")]
    bbox: Option<[f64; 4]>,
    context: String,
    cut_start: bool,
    cut_end: bool,
    marks: Vec<(usize, usize)>,
    range: Option<(usize, usize)>,
    quote: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PaperHits {
    paper: String,
    total: usize,
    hits: Vec<Hit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    building: bool,
    pending: bool,
    indexed: usize,
    more: bool,
    papers: Vec<PaperHits>,
}

/// 查詢：不等比對，照現有的索引交回。
fn search(
    s: &AppState,
    ft: &Fulltext,
    query: &str,
    scope: Option<&[String]>,
    offset: usize,
) -> ApiResult<SearchResult> {
    refresh(s, &ft.slot)?;
    let mut out = SearchResult {
        building: ft.slot.building(),
        pending: ft.slot.pending(),
        indexed: 0,
        more: false,
        papers: Vec::new(),
    };
    let q = fulltext::parse_query(query);
    let (ranked, library) = {
        let mut g = ft.slot.lock();
        let Some(l) = g.as_ref() else {
            out.pending = true;
            return Ok(out);
        };
        out.indexed = l.index.len();
        let Some(q) = &q else {
            return Ok(out);
        };
        match l.index.search(q, scope, offset, PAGE) {
            Ok(r) => (r, l.library.clone()),
            Err(e) if fulltext::is_corrupt(&e) => {
                reset(&mut g);
                out.pending = true;
                return Ok(out);
            }
            Err(e) => return Err(sql_error(e)),
        }
    };
    let Some(q) = q else { return Ok(out) };
    out.more = ranked.more;
    for p in ranked.papers {
        let mut hits = Vec::new();
        let mut dropped = 0;
        for c in &p.candidates {
            if hits.len() == SHOWN {
                break;
            }
            match ft.locate(s, &library, &q, c)? {
                Some(h) => hits.push(h),
                None => dropped += 1,
            }
        }
        if !hits.is_empty() {
            out.papers.push(PaperHits {
                paper: p.paper,
                total: p.total.saturating_sub(dropped),
                hits,
            });
        }
    }
    Ok(out)
}

/// 那篇的每一處（最多 200 處）。
fn hits(s: &AppState, ft: &Fulltext, paper: &str, query: &str) -> ApiResult<Vec<Hit>> {
    refresh(s, &ft.slot)?;
    let Some(q) = fulltext::parse_query(query) else {
        return Ok(Vec::new());
    };
    let (found, library) = {
        let mut g = ft.slot.lock();
        let Some(l) = g.as_ref() else {
            return Ok(Vec::new());
        };
        match l.index.hits(&q, paper, HITS_MAX) {
            Ok(h) => (h, l.library.clone()),
            Err(e) if fulltext::is_corrupt(&e) => {
                reset(&mut g);
                return Ok(Vec::new());
            }
            Err(e) => return Err(sql_error(e)),
        }
    };
    let mut out = Vec::new();
    for c in &found {
        if let Some(h) = ft.locate(s, &library, &q, c)? {
            out.push(h);
        }
    }
    Ok(out)
}

/// 首頁搜尋的一節「內文」：一次 10 篇，`offset` 是從第幾篇起；`scope` 是群組與篩選下看得到的文獻。
#[tauri::command]
pub async fn fulltext_search(
    state: State<'_, AppState>,
    ext: State<'_, Fulltext>,
    query: String,
    scope: Option<Vec<String>>,
    offset: Option<usize>,
) -> ApiResult<SearchResult> {
    let ext = ext.inner().clone();
    state
        .run("fulltext_search", move |s| {
            search(s, &ext, &query, scope.as_deref(), offset.unwrap_or(0))
        })
        .await
}

/// 「還有 K 處」：那篇的每一處。
#[tauri::command]
pub async fn fulltext_hits(
    state: State<'_, AppState>,
    ext: State<'_, Fulltext>,
    paper_id: String,
    query: String,
) -> ApiResult<Vec<Hit>> {
    let ext = ext.inner().clone();
    state
        .run("fulltext_hits", move |s| hits(s, &ext, &paper_id, &query))
        .await
}

/// 外殼一側的接法：比目錄摘要交出的新增、變了、移除；寫入之後索引跟著變；
/// 50 篇以上在背景整理；索引與摘要存在應用資料夾；換了文獻庫各用各的。內文搜尋：兩個
/// 外掛各用各的索引與摘要；文獻連結的 `digests.json` 格式不變；內文搜尋的比對在背景、查詢不等；只讀變了的鍵；
/// 讀檔的快取；索引被刪掉時重建。
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::location::Storage;
    use crate::state::Settings;
    use paperreader_core::format::BibDraft;
    use paperreader_core::references::{EntryDraft, ReferencesDraft};
    use paperreader_core::store::AddOutcome;
    use paperreader_core::text::TextDraft;
    use std::time::{Duration, Instant};
    use tauri_plugin_paperreader::StorageInfo;

    const DETR: &str = "DETRs Beat YOLOs on Real-time Object Detection";

    // ---- 測試用的掛鉤（背景整理停在套用之前；每個文獻庫各自數讀了幾次來源） ----

    static HELD: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static READS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

    pub(crate) fn wait_while_held(library: &str) {
        while HELD.lock().unwrap().iter().any(|l| l == library) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub(crate) fn count_read(library: &str, key: &str) {
        READS
            .lock()
            .unwrap()
            .push((library.to_string(), key.to_string()));
    }

    fn reads_of(library: &str) -> Vec<String> {
        READS
            .lock()
            .unwrap()
            .iter()
            .filter(|(l, _)| l == library)
            .map(|(_, k)| k.clone())
            .collect()
    }

    fn info(root: &Path) -> StorageInfo {
        StorageInfo {
            sdk: 36,
            all_files_access: false,
            shared_dir: root.join("Documents/PaperReader").display().to_string(),
            private_dir: root.join("private/PaperReader").display().to_string(),
            cache_dir: root.join("cache").display().to_string(),
        }
    }

    /// 一個開著空文獻庫的應用狀態（設定檔與應用資料夾在 `root`）。
    fn app(root: &Path) -> AppState {
        let sp = root.join("settings.json");
        let (s, recovered) = Settings::load_or_create(&sp).unwrap();
        let st = AppState::new(s, sp, recovered);
        st.create(Storage::Private, &info(root)).unwrap();
        st
    }

    fn library_id(st: &AppState) -> String {
        st.with_lib(|l| Ok(l.library_file().id.clone())).unwrap()
    }

    fn add(st: &AppState, root: &Path, n: u64, title: &str, year: i32) -> String {
        let pdf = root.join(format!("src-{n}.pdf"));
        std::fs::write(&pdf, format!("%PDF-1.7\nsource {n}")).unwrap();
        st.with_lib(|l| {
            let draft = BibDraft {
                title: Some(title.into()),
                year: Some(year),
                ..Default::default()
            };
            match l.add_paper(&pdf, draft, Some(10))? {
                AddOutcome::Added(id) => Ok(id),
                other => panic!("{other:?}"),
            }
        })
        .unwrap()
    }

    fn cite(st: &AppState, id: &str, texts: &[&str]) {
        st.with_lib(|l| {
            let (p, _) = l.get_paper(id)?;
            let m = &p.materials[0];
            l.write_references(
                id,
                ReferencesDraft {
                    extractor: "links".into(),
                    version: 1,
                    source_material: m.id.clone(),
                    source_sha256: m.sha256.clone().unwrap(),
                    entries: texts
                        .iter()
                        .enumerate()
                        .map(|(i, t)| EntryDraft {
                            number: Some(i as u32 + 1),
                            label: None,
                            text: (*t).into(),
                            page: 9,
                            bbox: None,
                            doi: None,
                            arxiv: None,
                            year: Some(2024),
                        })
                        .collect(),
                },
            )
        })
        .unwrap();
    }

    /// 寫那篇的抽出文字（10 頁；第 0 頁是 `first`，其餘是填充）。
    fn write_text(st: &AppState, id: &str, first: &str) {
        st.with_lib(|l| {
            let (p, _) = l.get_paper(id)?;
            let m = &p.materials[0];
            let mut pages = vec![first.to_string()];
            pages.extend((1..10).map(|i| format!("page {i} filler")));
            l.write_text(
                id,
                TextDraft {
                    extractor: "pdfjs-text".into(),
                    version: 1,
                    source_material: m.id.clone(),
                    source_sha256: m.sha256.clone().unwrap(),
                    pages,
                    truncated: false,
                },
            )
        })
        .unwrap();
    }

    fn linked_to(st: &AppState, ext: &Links, id: &str) -> Vec<Option<String>> {
        with_index(st, ext, |ix, _, _| {
            ix.links_for(id)
                .map(|v| {
                    v.entries
                        .into_iter()
                        .map(|e| e.link.map(|l| l.paper))
                        .collect()
                })
                .unwrap_or_default()
        })
        .unwrap()
    }

    fn digest(bib: &str, refs: Option<(u64, Option<i64>)>) -> Digest {
        Digest {
            bib: bib.into(),
            refs,
        }
    }

    fn wait_idle<P: ExtPlugin>(slot: &Slot<P>) {
        let t0 = Instant::now();
        while slot.pending() && t0.elapsed() < Duration::from_secs(60) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!slot.pending(), "背景整理沒在時限內做完");
    }

    /// 查詢，等背景做完再查一次。
    fn search_settled(st: &AppState, ft: &Fulltext, q: &str) -> SearchResult {
        search(st, ft, q, None, 0).unwrap();
        wait_idle(&ft.slot);
        search(st, ft, q, None, 0).unwrap()
    }

    // ---------- 文獻之間的連結（原有的四個） ----------

    #[test]
    fn diff_reports_added_changed_and_removed() {
        let old: HashMap<String, Digest> = [
            ("A".to_string(), digest("a", None)),
            ("B".to_string(), digest("b", Some((10, Some(1))))),
            ("C".to_string(), digest("c", None)),
        ]
        .into();
        let now: HashMap<String, Digest> = [
            ("A".to_string(), digest("a", None)),                // 沒變
            ("B".to_string(), digest("b", Some((12, Some(2))))), // 參考文獻清單變了
            ("D".to_string(), digest("d", None)),                // 新增
        ]
        .into();
        let (changed, removed) = diff(&old, &now);
        assert_eq!(changed, vec!["B".to_string(), "D".to_string()]);
        assert_eq!(removed, vec!["C".to_string()]);
    }

    #[test]
    fn writes_add_and_remove_reach_the_index_and_it_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let st = app(dir.path());
        let ext = Links::default();
        let p = add(&st, dir.path(), 1, "Citing Paper", 2025);
        cite(&st, &p, &[&format!("Y. Zhao. {DETR}. In CVPR, 2024.")]);
        assert_eq!(linked_to(&st, &ext, &p), vec![None]);
        // 新增一篇：下一次查詢之前比摘要，連上
        let d = add(&st, dir.path(), 2, DETR, 2024);
        assert_eq!(linked_to(&st, &ext, &p), vec![Some(d.clone())]);
        // 索引與上次的摘要存在 <應用資料夾>/ext/links/<文獻庫代號>/
        let lib = library_id(&st);
        let where_ = dir.path().join("ext").join("links").join(&lib);
        assert!(where_.join("digests.json").is_file());
        assert!(where_.join("shard-00.json").is_file());
        // 移到回收區：不連了
        st.with_lib(|l| l.remove_paper(&d)).unwrap();
        assert_eq!(linked_to(&st, &ext, &p), vec![None]);
        // 放回：又連上
        st.with_lib(|l| l.restore_paper(&d)).unwrap();
        assert_eq!(linked_to(&st, &ext, &p), vec![Some(d.clone())]);
        // 重開應用（新的狀態）：從存好的索引與摘要接回，結果相同
        let ext2 = Links::default();
        assert_eq!(linked_to(&st, &ext2, &p), vec![Some(d)]);
    }

    #[test]
    fn fifty_or_more_changes_are_built_in_the_background() {
        let dir = tempfile::tempdir().unwrap();
        let st = app(dir.path());
        let mut ids = Vec::new();
        for n in 0..60 {
            ids.push(add(
                &st,
                dir.path(),
                n,
                &format!("Paper number {n} about many things"),
                2020,
            ));
        }
        let ext = Links::default();
        let building = with_index(&st, &ext, |_, b, _| b).unwrap();
        assert!(building, "60 篇第一次要在背景整理");
        let t0 = std::time::Instant::now();
        while ext.building() && t0.elapsed() < std::time::Duration::from_secs(30) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(!ext.building());
        let (len, building) = with_index(&st, &ext, |ix, b, _| (ix.len(), b)).unwrap();
        assert_eq!(len, 60);
        assert!(!building);
        let lib = library_id(&st);
        let saved =
            load_digests::<LinksExt>(&dir.path().join("ext").join("links").join(&lib)).unwrap();
        assert_eq!(saved.len(), 60);
    }

    #[test]
    fn each_library_has_its_own_index() {
        let a_dir = tempfile::tempdir().unwrap();
        let b_dir = tempfile::tempdir().unwrap();
        let a = app(a_dir.path());
        let b = app(b_dir.path());
        let ext = Links::default();
        let pa = add(&a, a_dir.path(), 1, "Only In Library A Paper", 2020);
        let pb = add(&b, b_dir.path(), 1, "Only In Library B Paper", 2021);
        assert!(with_index(&a, &ext, |ix, _, _| ix.contains(&pa)).unwrap());
        // 換到 B：載入 B 的索引，看不到 A 的
        assert!(with_index(&b, &ext, |ix, _, _| ix.contains(&pb) && !ix.contains(&pa)).unwrap());
        // 換回 A：從 A 存好的索引接回
        assert!(with_index(&a, &ext, |ix, _, _| ix.contains(&pa) && !ix.contains(&pb)).unwrap());
    }

    // ---------- 共用的接法改了之後 ----------

    /// 文獻連結的 `digests.json` 格式不變：改之前寫的檔（手寫的舊格式）照樣讀得懂、和現在的摘要相同，所以升級後
    /// 不重讀任何一篇。
    #[test]
    fn links_digests_file_keeps_its_format_and_nothing_is_reread() {
        let dir = tempfile::tempdir().unwrap();
        let st = app(dir.path());
        let p = add(&st, dir.path(), 1, "Citing Paper", 2025);
        cite(&st, &p, &[&format!("Y. Zhao. {DETR}. In CVPR, 2024.")]);
        let ext = Links::default();
        linked_to(&st, &ext, &p);
        let lib = library_id(&st);
        let where_ = dir.path().join("ext").join("links").join(&lib);
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(where_.join("digests.json")).unwrap()).unwrap();
        let now = current::<LinksExt>(&st).unwrap();
        let d = &now[&p];
        let (bytes, modified) = d.refs.unwrap();
        // 改之前的程式寫出來的樣子
        let old = serde_json::json!({
            "version": 1,
            "papers": { p.clone(): { "bib": d.bib.clone(), "refs": [bytes, modified] } }
        });
        assert_eq!(written, old);
        std::fs::write(where_.join("digests.json"), old.to_string()).unwrap();
        let loaded = load_digests::<LinksExt>(&where_).unwrap();
        let (changed, removed) = diff(&loaded, &now);
        assert!(changed.is_empty() && removed.is_empty(), "升級後不重讀");
    }

    #[test]
    fn fulltext_runs_in_the_background_and_then_finds_with_its_own_index() {
        let dir = tempfile::tempdir().unwrap();
        let st = app(dir.path());
        let p = add(&st, dir.path(), 1, "Some Paper", 2024);
        cite(&st, &p, &["A. Author. Another paper. 2020."]);
        write_text(
            &st,
            &p,
            "We propose a real-\ntime end-to-end detector named RT-DETR.",
        );
        let ft = Fulltext::default();
        // 第一次：比對在背景，查詢不等，帶 pending
        let first = search(&st, &ft, "detector", None, 0).unwrap();
        assert!(first.pending);
        assert!(first.papers.is_empty());
        wait_idle(&ft.slot);
        let r = search(&st, &ft, "detector", None, 0).unwrap();
        assert!(!r.pending && !r.building);
        assert_eq!(r.indexed, 1);
        assert_eq!(r.papers.len(), 1);
        let h = &r.papers[0].hits[0];
        assert_eq!(h.kind, "source");
        assert_eq!(h.page, Some(0));
        assert_eq!(h.quote.as_deref(), Some("detector"));
        assert_eq!(h.range, Some((35, 43)));
        // 兩個外掛各用各的索引與摘要
        linked_to(&st, &Links::default(), &p);
        let lib = library_id(&st);
        let ext_dir = dir.path().join("ext");
        assert!(
            ext_dir
                .join("fulltext")
                .join(&lib)
                .join("index.sqlite")
                .is_file()
        );
        assert!(
            ext_dir
                .join("fulltext")
                .join(&lib)
                .join("digests.json")
                .is_file()
        );
        assert!(
            ext_dir
                .join("links")
                .join(&lib)
                .join("digests.json")
                .is_file()
        );
        assert!(
            !ext_dir
                .join("links")
                .join(&lib)
                .join("index.sqlite")
                .exists()
        );
        // 內文搜尋的摘要只記 files
        let saved = load_digests::<FulltextExt>(&ext_dir.join("fulltext").join(&lib)).unwrap();
        assert_eq!(saved[&p].keys().collect::<Vec<_>>(), vec!["text"]);
        // 「還有 K 處」
        assert_eq!(hits(&st, &ft, &p, "detector").unwrap().len(), 1);
    }

    #[test]
    fn fifty_or_more_fulltext_changes_report_building_and_queries_do_not_wait() {
        let dir = tempfile::tempdir().unwrap();
        let st = app(dir.path());
        for n in 0..60 {
            let id = add(&st, dir.path(), n, &format!("Paper {n}"), 2020);
            write_text(&st, &id, &format!("common words and number{n}"));
        }
        let lib = library_id(&st);
        HELD.lock().unwrap().push(lib.clone());
        let ft = Fulltext::default();
        search(&st, &ft, "common", None, 0).unwrap();
        // 背景比完摘要、停在套用之前：查詢照現有的（空的）索引馬上交回，帶 building 與 pending
        let t0 = Instant::now();
        while !ft.slot.building() && t0.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let r = search(&st, &ft, "common", None, 0).unwrap();
        assert!(r.building && r.pending);
        assert!(r.papers.is_empty());
        HELD.lock().unwrap().retain(|l| l != &lib);
        wait_idle(&ft.slot);
        let r = search(&st, &ft, "common", None, 0).unwrap();
        assert!(!r.building && !r.pending);
        assert_eq!(r.indexed, 60);
        assert_eq!(r.papers.len(), 10);
        assert!(r.more);
        let next = search(&st, &ft, "common", None, 50).unwrap();
        assert_eq!(next.papers.len(), 10);
        assert!(!next.more);
    }

    #[test]
    fn only_changed_keys_are_read_again() {
        let dir = tempfile::tempdir().unwrap();
        let st = app(dir.path());
        let a = add(&st, dir.path(), 1, "A", 2020);
        let b = add(&st, dir.path(), 2, "B", 2020);
        write_text(&st, &a, "apple text");
        write_text(&st, &b, "banana text");
        let ft = Fulltext::default();
        search_settled(&st, &ft, "apple");
        let lib = library_id(&st);
        assert_eq!(reads_of(&lib), vec!["text", "text"]);
        // 只改 B 的抽出文字：只讀 B 的 text 一次
        write_text(&st, &b, "cherry text");
        let r = search_settled(&st, &ft, "cherry");
        assert_eq!(reads_of(&lib).len(), 3);
        assert_eq!(r.papers.len(), 1);
        assert_eq!(r.papers[0].paper, b);
        assert!(search_settled(&st, &ft, "banana").papers.is_empty());
        // 沒有變：不再讀
        search_settled(&st, &ft, "apple");
        assert_eq!(reads_of(&lib).len(), 3);
        // 移到回收區：整篇移除
        st.with_lib(|l| l.remove_paper(&a)).unwrap();
        assert!(search_settled(&st, &ft, "apple").papers.is_empty());
    }

    #[test]
    fn source_cache_rereads_only_changed_files_and_keeps_at_most_forty() {
        let dir = tempfile::tempdir().unwrap();
        let st = app(dir.path());
        let mut ids = Vec::new();
        for n in 0..45 {
            let id = add(&st, dir.path(), n, &format!("Paper {n}"), 2020);
            write_text(&st, &id, &format!("shared token and unique{n}"));
            ids.push(id);
        }
        let ft = Fulltext::default();
        search_settled(&st, &ft, "unique1");
        let before = ft.cache().reads;
        // 同一個查詢再問一次：檔案屬性沒變，不重讀
        search(&st, &ft, "unique1", None, 0).unwrap();
        assert_eq!(ft.cache().reads, before);
        // 五批（45 篇各一份）：快取最多 40 份
        for off in (0..45).step_by(10) {
            search(&st, &ft, "shared", None, off).unwrap();
        }
        assert!(ft.cache().map.len() <= CACHE_MAX);
        assert!(ft.cache().reads >= 45);
    }

    #[test]
    fn a_file_changed_after_indexing_is_not_listed_and_totals_drop() {
        let dir = tempfile::tempdir().unwrap();
        let st = app(dir.path());
        let a = add(&st, dir.path(), 1, "A", 2020);
        write_text(&st, &a, "a kiwi on the first page");
        let ft = Fulltext::default();
        assert_eq!(search_settled(&st, &ft, "kiwi").papers.len(), 1);
        HELD.lock().unwrap().push(library_id(&st));
        // 抽出文字換了，索引還沒跟上：用現在的內容找不到，不列那篇
        write_text(&st, &a, "a mango on the first page");
        let r = search(&st, &ft, "kiwi", None, 0).unwrap();
        assert!(r.pending);
        assert!(r.papers.is_empty());
        HELD.lock().unwrap().retain(|l| l != &library_id(&st));
        wait_idle(&ft.slot);
        assert_eq!(search(&st, &ft, "mango", None, 0).unwrap().papers.len(), 1);
    }

    #[test]
    fn a_deleted_index_folder_is_rebuilt_with_the_same_results() {
        let dir = tempfile::tempdir().unwrap();
        let st = app(dir.path());
        for n in 0..3 {
            let id = add(&st, dir.path(), n, &format!("Paper {n}"), 2020);
            write_text(&st, &id, &format!("transformer detection {n}"));
        }
        let ft = Fulltext::default();
        let before = search_settled(&st, &ft, "detection");
        assert_eq!(before.papers.len(), 3);
        // 刪掉索引的資料夾（先讓外殼放掉索引檔，Windows 上開著的檔刪不掉）
        let lib = library_id(&st);
        let where_ = dir.path().join("ext").join("fulltext").join(&lib);
        *ft.slot.lock() = None;
        std::fs::remove_dir_all(&where_).unwrap();
        HELD.lock().unwrap().push(lib.clone());
        let first = search(&st, &ft, "detection", None, 0).unwrap();
        assert!(first.pending);
        // 整個重建：不到 50 篇也帶 building（畫面寫「整理中」）
        let t0 = Instant::now();
        while !ft.slot.building() && t0.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let during = search(&st, &ft, "detection", None, 0).unwrap();
        assert!(during.building && during.papers.is_empty());
        HELD.lock().unwrap().retain(|l| l != &lib);
        wait_idle(&ft.slot);
        let after = search(&st, &ft, "detection", None, 0).unwrap();
        let ids = |r: &SearchResult| {
            let mut v: Vec<String> = r.papers.iter().map(|p| p.paper.clone()).collect();
            v.sort();
            v
        };
        assert_eq!(ids(&after), ids(&before));
        assert!(where_.join("index.sqlite").is_file());
    }

    #[test]
    fn each_library_has_its_own_fulltext_index() {
        let a_dir = tempfile::tempdir().unwrap();
        let b_dir = tempfile::tempdir().unwrap();
        let a = app(a_dir.path());
        let b = app(b_dir.path());
        let pa = add(&a, a_dir.path(), 1, "A", 2020);
        let pb = add(&b, b_dir.path(), 1, "B", 2020);
        write_text(&a, &pa, "only alpaca here");
        write_text(&b, &pb, "only bison here");
        let ft = Fulltext::default();
        let ids = |r: SearchResult| r.papers.into_iter().map(|p| p.paper).collect::<Vec<_>>();
        assert_eq!(ids(search_settled(&a, &ft, "alpaca")), vec![pa.clone()]);
        // 換到 B：載入 B 的索引，找不到 A 的
        assert!(search_settled(&b, &ft, "alpaca").papers.is_empty());
        assert_eq!(ids(search_settled(&b, &ft, "bison")), vec![pb]);
        // 換回 A：從 A 存好的索引接回
        assert_eq!(ids(search_settled(&a, &ft, "alpaca")), vec![pa]);
        assert!(search_settled(&a, &ft, "bison").papers.is_empty());
    }

    #[test]
    fn a_damaged_index_file_is_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        let st = app(dir.path());
        let id = add(&st, dir.path(), 1, "Paper", 2020);
        write_text(&st, &id, "zebra crossing");
        let ft = Fulltext::default();
        assert_eq!(search_settled(&st, &ft, "zebra").papers.len(), 1);
        let lib = library_id(&st);
        let where_ = dir.path().join("ext").join("fulltext").join(&lib);
        *ft.slot.lock() = None;
        for f in std::fs::read_dir(&where_).unwrap() {
            let f = f.unwrap().path();
            if f.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("index.sqlite")
            {
                std::fs::write(&f, vec![0x42u8; 4096]).unwrap();
            }
        }
        // 重開應用：打不開 → 刪掉資料夾重建
        let ft = Fulltext::default();
        assert_eq!(search_settled(&st, &ft, "zebra").papers.len(), 1);
    }
}
