//! 翻譯引擎：給一批段落，交回譯文。
//!
//! 兩類引擎：對話模型（Anthropic、OpenAI、Gemini、OpenAI 相容端點含 Ollama；用現成的 genai 函式庫）
//! 與機器翻譯服務（DeepL、Google Cloud Translation v2；照官方 REST API）；
//! 另外是裝置上的引擎（Firefox 翻譯），由外殼實作 [`DeviceTranslator`] 交進來（`device.rs`）。
//!
//! 這個套件會連網，所以不放進核心（核心不連網）。它不知道文獻庫與檔案格式：
//! 外殼把要翻的段落交進來、把譯文拿去寫進核心。金鑰只存在 `EngineConfig` 裡，不進任何記錄。

mod cancel;
pub mod check;
mod device;
pub mod digest;
pub mod download;
mod engines;
mod error;
pub mod glossary;
/// 共用的 HTTP 用戶端（外殼的「檢查更新」也用它）。
pub mod http;
mod job;
#[cfg(feature = "layout-model")]
pub mod layout_model;
mod llm;
mod mt;
mod prompt;
mod quick;
pub mod text;

pub use cancel::Cancel;
pub use device::{DeviceFuture, DeviceTranslator};
pub use digest::{
    DigestInput, DigestItem, DigestResult, DigestSegment, DigestStage, DigestState, SectionSpec,
    digest_preview, run_digest,
};
pub use engines::{EngineInfo, EngineKind, LangSupport, Need, engines};
pub use error::{ErrorKind, Result, TranslateError};
pub use job::{BatchEvent, BatchItem, JobSummary, run_job};
pub use mt::{deepl_target, google_code};
pub use prompt::{
    BUILTIN_PRESETS, BuiltinPreset, BuiltinSection, DEFAULT_DIGEST_PRESET, DEFAULT_PRESET,
    DEFAULT_QUICK_PRESET, MAX_KNOWN_TERMS, MAX_PAPER_TERMS, PaperTerm, PresetKind, PromptPart,
    PromptPreview, TEMPLATE_VARS, TermState, builtin_preset, check_template, join, language_name,
    learn_terms, parse_reply, refine_message, refine_parts, refine_preview, refine_prompt,
    render_template, split_terms, system_prompt, terms_in_use, translate_parts, translate_preview,
    user_message,
};
pub use quick::{
    MAX_CONTEXT_CHARS, MAX_QUICK_CHARS, QuickInput, clean_reply, is_word, quick_message,
    quick_parts, quick_preview,
};

/// 服務回報的用量（詞元數）。服務沒有回報的是 0。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
}

impl TokenUsage {
    /// 從 `earlier` 到現在多出來的部分。
    pub fn since(self, earlier: TokenUsage) -> TokenUsage {
        TokenUsage {
            input: self.input.saturating_sub(earlier.input),
            output: self.output.saturating_sub(earlier.output),
        }
    }
}

use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 供應商。名稱會寫進譯本檔與設定檔，改名要同步改格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Anthropic,
    Openai,
    Gemini,
    /// 照 OpenAI 的格式提供服務的網址（Ollama、LM Studio、OpenRouter、DeepSeek……）。
    OpenaiCompatible,
    Deepl,
    GoogleTranslate,
    /// Firefox 翻譯（Bergamot，離線，在網頁元件裡跑）。
    Bergamot,
}

impl Provider {
    pub fn is_chat_model(self) -> bool {
        self.info().kind == EngineKind::Chat
    }

    /// 在這台裝置上翻、不連網的引擎。
    pub fn is_on_device(self) -> bool {
        self.info().kind == EngineKind::OnDevice
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    pub provider: Provider,
    /// 對話模型的名稱；機器翻譯服務不用。
    #[serde(default)]
    pub model: String,
    /// 服務網址。空白用官方的預設網址；相容端點必填。
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub api_key: String,
}

// 不用 derive：`Debug` 不可以印出金鑰
impl fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineConfig")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("endpoint", &self.endpoint)
            .field(
                "api_key",
                &if self.api_key.is_empty() { "" } else { "<set>" },
            )
            .finish()
    }
}

/// 一次翻譯工作共同的參數。
#[derive(Debug, Clone, Default)]
pub struct Params {
    /// 原文語言代碼（例如 `en`）；`None` 讓服務自己判斷。
    pub source_lang: Option<String>,
    /// 譯文語言代碼（例如 `zh-TW`）。
    pub target_lang: String,
    /// 術語：原文詞 → 指定的譯法。對話模型只收到這一批原文裡出現的；DeepL 用它的術語表功能。
    pub glossary: Vec<(String, String)>,
    /// 提示詞預設組的內容（可含 `{{from}}`、`{{to}}`、`{{title}}`）；空白用內建的「通用」。只給對話模型。
    pub instructions: String,
    /// 文獻的篇名（提示詞的 `{{title}}`）。
    pub title: Option<String>,
    /// 第二輪潤飾：對話模型翻完的段落再送一次請它潤飾。其他引擎不理會。
    pub refine: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub id: String,
    pub text: String,
    /// 這一段前面的原文（只給對話模型當前後文，不會被翻譯）。一批只用第一段的。
    pub before: Option<String>,
}

/// 快速翻譯的連線逾時（全文翻譯是 15 秒）。
pub const QUICK_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 快速翻譯整個請求的逾時（全文翻譯是 180 秒）：選取最多 2,000 字，等不到就改用離線的。
pub const QUICK_TOTAL_TIMEOUT: Duration = Duration::from_secs(20);

enum Backend {
    Llm(llm::Llm),
    Mt(mt::Mt),
    Device(Arc<dyn DeviceTranslator>),
}

pub struct Engine {
    provider: Provider,
    backend: Backend,
    retry_base: Duration,
    max_attempts: u32,
    /// 兩次請求之間至少隔多久（使用者給這個服務設的速率）；零是不限。
    min_interval: Duration,
    last_request: Mutex<Option<Instant>>,
    /// 這一篇的術語（`prompt::learn_terms`）：照用的帶進之後的批次，和使用者的術語庫同一個寫法（術語庫優先）。
    /// 外殼開始時放進譯文疊加檔記的、每批之後拿去存。
    terms: Mutex<Vec<PaperTerm>>,
    /// 這一批到目前為止回覆附的 `<terms>`（含單段重翻的）；`run_job` 定下譯文之後交給 `learn_terms`。
    proposed: Mutex<Vec<(String, String)>>,
}

impl Engine {
    pub fn new(cfg: &EngineConfig) -> Result<Engine> {
        Engine::build(cfg, http::build_client(), 4)
    }

    /// 快速翻譯用的引擎：連線逾時 5 秒、整個請求 20 秒、不重試，連不上就快點讓畫面改用離線的。
    /// 全文翻譯照舊用 `Engine::new`（連線 15 秒、整個請求 180 秒、最多四次）。
    pub fn new_quick(cfg: &EngineConfig) -> Result<Engine> {
        Engine::new_quick_with(cfg, QUICK_CONNECT_TIMEOUT, QUICK_TOTAL_TIMEOUT)
    }

    /// `new_quick`，逾時自己訂（測試用）。
    pub fn new_quick_with(
        cfg: &EngineConfig,
        connect: Duration,
        total: Duration,
    ) -> Result<Engine> {
        Engine::build(cfg, http::build_client_with(connect, total), 1)
    }

    fn build(cfg: &EngineConfig, http: reqwest::Client, max_attempts: u32) -> Result<Engine> {
        if cfg.provider.is_on_device() {
            return Err(TranslateError::new(
                ErrorKind::Config,
                "on-device engines are built with Engine::on_device",
            ));
        }
        let backend = if cfg.provider.is_chat_model() {
            if cfg.api_key.is_empty() && cfg.provider.info().key == Need::Required {
                return Err(TranslateError::new(ErrorKind::Config, "no API key"));
            }
            Backend::Llm(llm::Llm::new(cfg, http)?)
        } else {
            Backend::Mt(mt::Mt::new(cfg, http)?)
        };
        Ok(Engine {
            provider: cfg.provider,
            backend,
            retry_base: Duration::from_secs(2),
            max_attempts,
            min_interval: Duration::ZERO,
            last_request: Mutex::new(None),
            terms: Mutex::new(Vec::new()),
            proposed: Mutex::new(Vec::new()),
        })
    }

    /// 裝置上的引擎：翻譯交給外殼提供的 `device`。
    pub fn on_device(provider: Provider, device: Arc<dyn DeviceTranslator>) -> Result<Engine> {
        if !provider.is_on_device() {
            return Err(TranslateError::new(
                ErrorKind::Config,
                "not an on-device engine",
            ));
        }
        Ok(Engine {
            provider,
            backend: Backend::Device(device),
            retry_base: Duration::from_secs(1),
            max_attempts: 3,
            min_interval: Duration::ZERO,
            last_request: Mutex::new(None),
            terms: Mutex::new(Vec::new()),
            proposed: Mutex::new(Vec::new()),
        })
    }

    /// 這一篇已定的術語譯法（接著翻時放進上次存的）。超過上限的部分不收。
    pub fn with_paper_terms(self, mut terms: Vec<PaperTerm>) -> Engine {
        terms.truncate(MAX_KNOWN_TERMS);
        *self.terms.lock().unwrap_or_else(|p| p.into_inner()) = terms;
        self
    }

    /// 這一篇的術語（候選、學到的、使用者定的、刪掉的都在）。
    pub fn paper_terms(&self) -> Vec<PaperTerm> {
        self.terms.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// 丟掉還沒定下的術語提議（`run_job` 每一批開始時）。
    pub fn discard_proposed_terms(&self) {
        self.proposed
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    /// 一批的譯文定下之後（`run_job`）：這一批回覆附的術語照 `learn_terms` 的規則記下。交回新定下的條數。
    pub fn learn_from_batch(
        &self,
        sources: &[&str],
        outputs: &[&str],
        glossary: &[(String, String)],
    ) -> usize {
        let found = std::mem::take(&mut *self.proposed.lock().unwrap_or_else(|p| p.into_inner()));
        if found.is_empty() {
            return 0;
        }
        let mut t = self.terms.lock().unwrap_or_else(|p| p.into_inner());
        learn_terms(&mut t, found, sources, outputs, glossary)
    }

    /// 到目前為止服務回報的用量（對話模型才有；其他引擎是 0）。
    pub fn usage(&self) -> TokenUsage {
        match &self.backend {
            Backend::Llm(l) => l.usage(),
            _ => TokenUsage::default(),
        }
    }

    /// 重試的等待時間起點（每次加倍）與次數上限。測試用。
    pub fn with_retry(mut self, base: Duration, max_attempts: u32) -> Engine {
        self.retry_base = base;
        self.max_attempts = max_attempts.max(1);
        self
    }

    /// 速率：每分鐘最多幾次請求（0 是不限）。
    pub fn with_rate(mut self, per_minute: u32) -> Engine {
        self.min_interval = if per_minute == 0 {
            Duration::ZERO
        } else {
            Duration::from_millis(60_000 / u64::from(per_minute))
        };
        self
    }

    pub fn provider(&self) -> Provider {
        self.provider
    }

    /// 一批最多幾段、最多幾個字元（照引擎申報）。
    pub fn batch_limits(&self) -> (usize, usize) {
        let i = self.provider.info();
        (i.max_segments, i.max_chars)
    }

    /// 翻譯一批（不重試）。
    async fn once(&self, params: &Params, batch: &[Segment]) -> Result<Vec<String>> {
        match &self.backend {
            Backend::Llm(l) => {
                let before = batch.first().and_then(|s| s.before.as_deref());
                let known = terms_in_use(&self.paper_terms());
                let (out, found) = l.translate(params, batch, before, &known).await?;
                // 先記著，等這一批的譯文定下（潤飾、單段重翻之後）才學
                self.proposed
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .extend(found);
                Ok(out)
            }
            Backend::Mt(m) => m.translate(params, batch).await,
            Backend::Device(d) => {
                let texts: Vec<String> = batch.iter().map(|s| s.text.clone()).collect();
                let out = d.translate(params, texts).await?;
                if out.len() != batch.len() {
                    return Err(TranslateError::new(
                        ErrorKind::BadResponse,
                        format!("expected {} translations, got {}", batch.len(), out.len()),
                    ));
                }
                Ok(out)
            }
        }
    }

    /// 照使用者設的速率等到可以送下一個請求（可以取消）。
    async fn pace(&self, cancel: &Cancel) -> Result<()> {
        if self.min_interval.is_zero() {
            return Ok(());
        }
        let wait = {
            let mut last = self.last_request.lock().unwrap_or_else(|p| p.into_inner());
            let now = Instant::now();
            let at = last.map_or(now, |l| (l + self.min_interval).max(now));
            *last = Some(at);
            at - now
        };
        if wait.is_zero() {
            return Ok(());
        }
        tokio::select! {
            () = tokio::time::sleep(wait) => Ok(()),
            () = cancel.cancelled() => Err(TranslateError::new(ErrorKind::Cancelled, "cancelled")),
        }
    }

    /// 送出一個請求；網路、太頻繁、服務端出錯時等一下再試。可以隨時取消。
    async fn retrying<'a, F, Fut>(&'a self, cancel: &Cancel, call: F) -> Result<Vec<String>>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<String>>> + 'a,
    {
        let mut attempt = 0u32;
        loop {
            if cancel.is_cancelled() {
                return Err(TranslateError::new(ErrorKind::Cancelled, "cancelled"));
            }
            self.pace(cancel).await?;
            let result = tokio::select! {
                r = call() => r,
                () = cancel.cancelled() => {
                    return Err(TranslateError::new(ErrorKind::Cancelled, "cancelled"));
                }
            };
            match result {
                Ok(v) => return Ok(v),
                Err(e) if e.retryable() && attempt + 1 < self.max_attempts => {
                    let backoff = self.retry_base * 2u32.pow(attempt);
                    let wait = e
                        .retry_after_secs
                        .map(|s| Duration::from_secs(s.min(60)))
                        .unwrap_or(backoff)
                        .max(backoff.min(Duration::from_secs(60)));
                    attempt += 1;
                    tokio::select! {
                        () = tokio::time::sleep(wait) => {}
                        () = cancel.cancelled() => {
                            return Err(TranslateError::new(ErrorKind::Cancelled, "cancelled"));
                        }
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// 翻譯一批；網路、太頻繁、服務端出錯時等一下再試。可以隨時取消。
    pub async fn translate_batch(
        &self,
        params: &Params,
        batch: &[Segment],
        cancel: &Cancel,
    ) -> Result<Vec<String>> {
        if self.provider.info().target(&params.target_lang).is_none() {
            return Err(TranslateError::new(
                ErrorKind::UnsupportedLanguage,
                "target language",
            ));
        }
        self.retrying(cancel, || self.once(params, batch)).await
    }

    /// 對話模型的一次完整回覆（重點筆記用）：系統提示詞加一則使用者訊息，交回模型的原文。
    /// 速率、重試、取消與錯誤的分類和翻譯相同。不是對話模型回報 `Config`。
    pub async fn complete(&self, system: &str, user: &str, cancel: &Cancel) -> Result<String> {
        let Backend::Llm(l) = &self.backend else {
            return Err(TranslateError::new(ErrorKind::Config, "not a chat model"));
        };
        let mut out = self
            .retrying(cancel, || {
                let (s, u) = (system.to_string(), user.to_string());
                async move { l.complete(s, u).await.map(|t| vec![t]) }
            })
            .await?;
        Ok(out.pop().unwrap_or_default())
    }

    /// 選取文字的快速翻譯。對話模型照快速翻譯的提示詞送出選取與前後各一句（`quick.rs`），
    /// 回覆整理過才交回；機器翻譯與離線引擎只收到選取的文字。速率、重試、取消與錯誤的分類和翻譯相同。
    /// `params.instructions` 是快速翻譯的預設組（空白用內建的）。
    pub async fn quick(
        &self,
        params: &Params,
        input: &QuickInput,
        cancel: &Cancel,
    ) -> Result<String> {
        if self.provider.info().target(&params.target_lang).is_none() {
            return Err(TranslateError::new(
                ErrorKind::UnsupportedLanguage,
                "target language",
            ));
        }
        let input = input.normalized()?;
        match &self.backend {
            Backend::Llm(l) => {
                let known = terms_in_use(&self.paper_terms());
                let (system, user) = quick::quick_request(params, &input, &known);
                let mut out = self
                    .retrying(cancel, || {
                        let (s, u) = (system.clone(), user.clone());
                        async move { l.complete(s, u).await.map(|t| vec![t]) }
                    })
                    .await?;
                clean_reply(&out.pop().unwrap_or_default())
            }
            _ => {
                // 沒有提示詞的引擎：只送選取的文字，不送前後文
                let seg = Segment {
                    id: "1".to_string(),
                    text: input.text,
                    before: None,
                };
                let mut out = self
                    .retrying(cancel, || self.once(params, std::slice::from_ref(&seg)))
                    .await?;
                let t = out.pop().unwrap_or_default();
                if t.trim().is_empty() {
                    return Err(TranslateError::new(ErrorKind::BadResponse, "empty reply"));
                }
                Ok(t.trim().to_string())
            }
        }
    }

    /// 這個引擎能不能做第二輪潤飾（只有對話模型）。
    pub fn can_refine(&self) -> bool {
        matches!(self.backend, Backend::Llm(_))
    }

    /// 第二輪潤飾：把一批的草稿譯文連同原文再送一次，請模型修順。不是對話模型就原樣交回草稿。
    pub async fn refine_batch(
        &self,
        params: &Params,
        batch: &[Segment],
        drafts: &[String],
        cancel: &Cancel,
    ) -> Result<Vec<String>> {
        let Backend::Llm(l) = &self.backend else {
            return Ok(drafts.to_vec());
        };
        let known = terms_in_use(&self.paper_terms());
        self.retrying(cancel, || l.refine(params, batch, drafts, &known))
            .await
    }

    /// 測試連線：翻一句短句。金鑰不對、連不上、模型名稱不對都會在這裡看到。
    pub async fn test(&self, params: &Params, cancel: &Cancel) -> Result<String> {
        let seg = Segment {
            id: "1".to_string(),
            text: "The model is trained on a large corpus.".to_string(),
            before: None,
        };
        let mut out = self
            .translate_batch(params, std::slice::from_ref(&seg), cancel)
            .await?;
        Ok(out.pop().unwrap_or_default())
    }
}
