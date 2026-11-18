//! 裝置上的引擎（離線翻譯）的轉接：Rust 的翻譯工作迴圈（`run_job`）照常推進，
//! 每一批字經由這裡交給真正做翻譯的地方，拿回譯文。
//!
//! - **Firefox 翻譯（Bergamot）**：WASM 引擎在網頁元件的背景工作者裡跑。Rust 不能直接呼叫網頁，
//!   所以畫面一直留著一個「等工作」的請求（`ondevice_next`，最長等 20 秒、交回後立刻再問）；
//!   Rust 把一批字放進佇列、喚醒那個請求，畫面翻完用 `ondevice_reply` 交回。
//!   畫面太久沒來問（應用剛啟動、網頁還沒載入完），這一批算連不上，由引擎的重試處理。
//!
//! 模型檔：Bergamot 的模型由這裡下載到應用私有的空間
//! （`<app_data>/models/bergamot/`），不進文獻庫、不進備份。下載的只有模型（資料）；
//! Bergamot 的 WASM 引擎是程式碼，跟著安裝檔走（`public/bergamot/`）。

use paperreader_translate::download::RemoteFile;
use paperreader_translate::{
    DeviceFuture, DeviceTranslator, ErrorKind as TErr, Params, Provider, TranslateError,
};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, oneshot};

// ---------- Bergamot 的模型目錄 ----------

const CDN: &str = "https://firefox-settings-attachments.cdn.mozilla.net/";

/// 一個語言組合要用的檔：名稱是存在裝置上的檔名，也是畫面讀檔時用的名稱。
pub struct BergamotFile {
    pub name: &'static str,
    pub remote: RemoteFile,
}

macro_rules! rf {
    ($loc:literal, $size:literal, $sha:literal) => {
        RemoteFile {
            url: concat!(
                "https://firefox-settings-attachments.cdn.mozilla.net/",
                $loc
            ),
            size: $size,
            zstd: true,
            sha256: $sha,
        }
    };
}

/// 舊版放在模型資料夾裡的引擎檔名（現在引擎跟著安裝檔走；刪最後一個模型時順便清掉舊檔）。
pub const OLD_ENGINE_FILE: &str = "bergamot-translator.wasm";

/// 一個語言組合的模型（Remote Settings `translations-models-v2`，3.0 版，`base-memory`，MPL-2.0）。
pub struct BergamotPair {
    /// 存放的資料夾名稱與畫面看到的代號，例如 `en-zh-Hant`。
    pub id: &'static str,
    pub source: &'static str,
    pub target: &'static str,
    /// model、lex、srcvocab、trgvocab。
    pub files: [BergamotFile; 4],
}

pub const BERGAMOT_PAIRS: [BergamotPair; 2] = [
    BergamotPair {
        id: "en-zh-Hant",
        source: "en",
        target: "zh-Hant",
        files: [
            BergamotFile {
                name: "model.bin",
                remote: rf!(
                    "main-workspace/translations-models-v2/c21ec952-90bb-42dc-b245-7b73e90df282.zst",
                    32_758_465,
                    "559ab90d723a58c1f1e2ab7cc12137bc667af5ba3e325e3eb30b5cdc930db520"
                ),
            },
            BergamotFile {
                name: "lex.bin",
                remote: rf!(
                    "main-workspace/translations-models-v2/2fbf2d3e-9917-4564-8348-0bc0356de46c.zst",
                    1_726_096,
                    "d891404d1436a7334df12539fe30a26f9e9f2b80bd42fdb8b5f8849e8a1e942b"
                ),
            },
            BergamotFile {
                name: "srcvocab.spm",
                remote: rf!(
                    "main-workspace/translations-models-v2/103d9764-f969-4c25-9ac9-ac08bc628e07.zst",
                    346_110,
                    "2266df70492162a249ab1c0154f929bd6098b246544c666c1a0d5a24dde7d2ea"
                ),
            },
            BergamotFile {
                name: "trgvocab.spm",
                remote: rf!(
                    "main-workspace/translations-models-v2/349beae6-80c3-4e87-bfcb-4963d8f58882.zst",
                    350_477,
                    "22b91a4436d70b91ab8777c677252ab5fae2bc284d71f977df5206c110e3444c"
                ),
            },
        ],
    },
    BergamotPair {
        id: "en-zh-Hans",
        source: "en",
        target: "zh-Hans",
        files: [
            BergamotFile {
                name: "model.bin",
                remote: rf!(
                    "main-workspace/translations-models-v2/321191b6-2c42-4bca-84ec-b7b074b67bd6.zst",
                    32_923_790,
                    "4e5accc141373565ddc8fa1565bceaa8d0c3482a82cab8131c719ebcc6c2157c"
                ),
            },
            BergamotFile {
                name: "lex.bin",
                remote: rf!(
                    "main-workspace/translations-models-v2/900beaed-2dba-4a88-ad7c-852ec9e73f6a.zst",
                    2_499_275,
                    "4a5e5827788060f1d718a8132b69440929387514a045796e9b77f935db68c055"
                ),
            },
            BergamotFile {
                name: "srcvocab.spm",
                remote: rf!(
                    "main-workspace/translations-models-v2/83278466-fec4-4944-a99d-c2372fa9de88.zst",
                    348_078,
                    "bd9b65504acc6d9726dd281f7defc2adb7c2c22d0688fe2f84697de25197c8c5"
                ),
            },
            BergamotFile {
                name: "trgvocab.spm",
                remote: rf!(
                    "main-workspace/translations-models-v2/84236654-1b66-42fc-9ba2-74c37f27e5eb.zst",
                    354_994,
                    "aded6993c36e440284d11cec3f6b8aef9c0e43188a772d80be342a713adf223d"
                ),
            },
        ],
    },
];

/// 模型檔的來源主機（給使用者看「會連到哪裡」）。
pub fn bergamot_host() -> &'static str {
    CDN.trim_start_matches("https://").trim_end_matches('/')
}

/// 目標語言代碼對到 Bergamot 的語言組合（照引擎申報的語言對照表）；沒有對應的模型交回 `None`。
pub fn bergamot_pair(source: Option<&str>, target: &str) -> Option<&'static BergamotPair> {
    let src = source.unwrap_or("en").to_ascii_lowercase();
    let src = src.split(['-', '_']).next().unwrap_or("");
    let tgt = Provider::Bergamot.info().target(target)?.engine_code;
    BERGAMOT_PAIRS
        .iter()
        .find(|p| p.source == src && p.target == tgt)
}

pub fn bergamot_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join("bergamot")
}

/// 這個語言組合的模型是不是都下載好了。
pub fn bergamot_ready(data_dir: &Path, pair: &BergamotPair) -> bool {
    let dir = bergamot_dir(data_dir);
    pair.files
        .iter()
        .all(|f| dir.join(pair.id).join(f.name).is_file())
}

/// 一個資料夾（或檔）的大小。
pub fn disk_size(p: &Path) -> u64 {
    if p.is_file() {
        return p.metadata().map(|m| m.len()).unwrap_or(0);
    }
    std::fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| disk_size(&e.path())).sum())
        .unwrap_or(0)
}

/// 下載的大小（壓縮後）。
pub fn bergamot_download_size(pair: &BergamotPair) -> u64 {
    pair.files.iter().map(|f| f.remote.size).sum()
}

// ---------- 下載的進度（同一時間一份） ----------

#[derive(Default)]
pub struct Downloads {
    pub running: AtomicBool,
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: Mutex<Option<paperreader_translate::Cancel>>,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub running: bool,
    pub done: u64,
    pub total: u64,
}

// ---------- 畫面一側的引擎（Bergamot）的橋 ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRequest {
    pub id: u64,
    /// 語言組合（`BergamotPair::id`），畫面依它載入模型。
    pub pair: String,
    pub texts: Vec<String>,
}

type Reply = std::result::Result<Vec<String>, String>;

#[derive(Default)]
struct BridgeInner {
    next_id: u64,
    queue: VecDeque<BridgeRequest>,
    pending: HashMap<u64, oneshot::Sender<Reply>>,
    /// 畫面最後一次來問工作的時間。
    last_poll: Option<Instant>,
}

#[derive(Default)]
pub struct Bridge {
    inner: Mutex<BridgeInner>,
    notify: Notify,
}

/// 畫面多久沒來問就當作它不在（網頁沒在跑）。
const POLL_STALE: Duration = Duration::from_secs(45);
/// 一批最長等多久（第一批要載入引擎與模型）。
const REPLY_TIMEOUT: Duration = Duration::from_secs(600);

impl Bridge {
    fn lock(&self) -> std::sync::MutexGuard<'_, BridgeInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 畫面來拿下一件工作；沒有就等到有、或 `wait` 到了交回 `None`。
    pub async fn next(&self, wait: Duration) -> Option<BridgeRequest> {
        let deadline = Instant::now() + wait;
        loop {
            let notified = self.notify.notified();
            {
                let mut g = self.lock();
                g.last_poll = Some(Instant::now());
                if let Some(r) = g.queue.pop_front() {
                    return Some(r);
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || tokio::time::timeout(left, notified).await.is_err() {
                self.lock().last_poll = Some(Instant::now());
                return None;
            }
        }
    }

    pub fn reply(&self, id: u64, result: Reply) {
        if let Some(tx) = self.lock().pending.remove(&id) {
            let _ = tx.send(result);
        }
    }

    async fn request(
        &self,
        pair: &str,
        texts: Vec<String>,
    ) -> paperreader_translate::Result<Vec<String>> {
        let (tx, rx) = oneshot::channel();
        let id = {
            let mut g = self.lock();
            let alive = g.last_poll.is_some_and(|t| t.elapsed() < POLL_STALE);
            if !alive {
                return Err(TranslateError::new(TErr::Network, "page is not listening"));
            }
            g.next_id += 1;
            let id = g.next_id;
            g.pending.insert(id, tx);
            g.queue.push_back(BridgeRequest {
                id,
                pair: pair.to_string(),
                texts,
            });
            id
        };
        self.notify.notify_waiters();
        match tokio::time::timeout(REPLY_TIMEOUT, rx).await {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(code))) if code == "no_model" => Err(TranslateError::new(
                TErr::ModelMissing,
                "model not downloaded",
            )),
            Ok(Ok(Err(_))) => Err(TranslateError::new(TErr::BadResponse, "engine failed")),
            // 取消或逾時：把這一件拿掉，免得畫面之後還來拿
            _ => {
                let mut g = self.lock();
                g.pending.remove(&id);
                g.queue.retain(|r| r.id != id);
                Err(TranslateError::new(TErr::Network, "no reply from the page"))
            }
        }
    }
}

pub struct BergamotDevice {
    pub bridge: Arc<Bridge>,
}

impl DeviceTranslator for BergamotDevice {
    fn translate<'a>(&'a self, params: &'a Params, texts: Vec<String>) -> DeviceFuture<'a> {
        Box::pin(async move {
            let pair = bergamot_pair(params.source_lang.as_deref(), &params.target_lang)
                .ok_or_else(|| TranslateError::new(TErr::UnsupportedLanguage, "language"))?;
            self.bridge.request(pair.id, texts).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_codes_map_to_pairs() {
        assert_eq!(bergamot_pair(Some("en"), "zh-TW").unwrap().id, "en-zh-Hant");
        assert_eq!(bergamot_pair(None, "zh-CN").unwrap().id, "en-zh-Hans");
        assert!(bergamot_pair(Some("en"), "ja").is_none());
        assert!(bergamot_pair(Some("de"), "zh-TW").is_none());
    }

    #[test]
    fn catalog_urls_are_on_the_mozilla_cdn() {
        for p in &BERGAMOT_PAIRS {
            for f in &p.files {
                assert!(f.remote.url.starts_with(CDN));
                assert_eq!(f.remote.sha256.len(), 64);
            }
        }
        assert_eq!(
            bergamot_host(),
            "firefox-settings-attachments.cdn.mozilla.net"
        );
    }

    #[tokio::test]
    async fn bridge_hands_work_to_the_page_and_back() {
        let b = Arc::new(Bridge::default());
        // 畫面還沒來問過：當作不在
        let e = b.request("en-zh-Hant", vec!["a".into()]).await.unwrap_err();
        assert_eq!(e.kind, TErr::Network);
        // 畫面在等工作
        let page = {
            let b = b.clone();
            tokio::spawn(async move {
                let r = b.next(Duration::from_secs(5)).await.unwrap();
                assert_eq!(r.pair, "en-zh-Hant");
                let out = r.texts.iter().map(|t| format!("譯{t}")).collect();
                b.reply(r.id, Ok(out));
            })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        let out = b
            .request("en-zh-Hant", vec!["a".into(), "b".into()])
            .await
            .unwrap();
        assert_eq!(out, vec!["譯a", "譯b"]);
        page.await.unwrap();
        // 畫面說模型不在：整個工作要停
        let page = {
            let b = b.clone();
            tokio::spawn(async move {
                let r = b.next(Duration::from_secs(5)).await.unwrap();
                b.reply(r.id, Err("no_model".into()));
            })
        };
        let e = b.request("en-zh-Hant", vec!["a".into()]).await.unwrap_err();
        assert_eq!(e.kind, TErr::ModelMissing);
        page.await.unwrap();
    }
}
