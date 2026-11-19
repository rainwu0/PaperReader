//! 翻譯：服務清單與金鑰、提示詞、術語庫、建立譯本、背景翻譯、進度與取消、換備用服務、
//! 幾個服務並排比較、匯出給翻譯軟體。
//!
//! 資料規則在核心（譯文疊加檔），引擎在 `paperreader-translate`；這裡只轉接。
//! 金鑰只在這裡從外掛取出、交給引擎，不進任何回傳值與記錄。

use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::interchange::{self, Format};
use crate::ondevice::{self, BergamotDevice, Bridge};
use crate::state::AppState;
use crate::translate::{
    FallbackOffer, Job, JobSlot, JobState, JobStatus, PromptPreset, PromptSection, Service,
    ServiceUsage, TranslateSettings, USAGE_FILE, UsageLog, api_kind, new_id, offers_fallback,
    validate_preset,
};
use paperreader_core::overlay::{
    OVERLAY_FORM, OverlayDraft, OverlayFile, OverlaySegment, OverlayTerm, STATUS_DONE,
    STATUS_FAILED, SegmentDraft, SegmentService, SegmentUpdate, TERM_CANDIDATE, TERM_LEARNED,
    TERM_REMOVED, TERM_USER,
};
use paperreader_translate::continuation::{Placed, continuations};
use paperreader_translate::glossary::{self, Term};
use paperreader_translate::text::{drop_repeated_glosses, glosses};
use paperreader_translate::{
    BUILTIN_PRESETS, BatchEvent, Cancel, DigestInput, DigestSegment, Engine, EngineConfig,
    FinalPassItem, Need, PaperTerm, Params, PresetKind, PromptPreview, Provider, QuickInput,
    Segment, TermState, TokenUsage, digest_preview, final_pass_targets, quick_preview,
    refine_preview, run_final_pass, run_job, terms_in_use, translate_preview,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_paperreader::PaperReaderExt;

// ---------- 設定 ----------

/// 畫面看到的一個服務：設定加上有沒有金鑰、文字實際會送到哪個主機。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceView {
    #[serde(flatten)]
    pub service: Service,
    pub has_key: bool,
    /// 論文的文字會送到的主機（離線引擎是空的）。DeepL 免費版的金鑰送到 `api-free.deepl.com`：
    /// 畫面讀不到金鑰，所以由這裡告知。
    pub host: String,
}

/// 一組提示詞預設組（實際的內容；內建的名稱由畫面的文字表提供）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetView {
    pub id: String,
    pub name: String,
    pub kind: PresetKind,
    pub text: String,
    pub sections: Vec<PromptSection>,
    pub builtin: bool,
    /// 內建的、使用者改過（可以「還原預設」）。
    pub modified: bool,
}

/// 畫面看到的設定：不含金鑰。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub services: Vec<ServiceView>,
    pub primary: String,
    pub fallbacks: Vec<String>,
    pub source_lang: String,
    pub target_lang: String,
    pub skip_confirm: bool,
    pub prompt: String,
    /// 重點筆記選用的預設組。
    pub digest_prompt: String,
    /// 內建的在前（改過的是改過的內容），自訂的在後；兩種用途都在。
    pub prompts: Vec<PresetView>,
    pub refine: bool,
    /// 收尾重翻。
    pub final_pass: bool,
    /// 離線引擎的語言模型只在 Wi-Fi 下載（在 Firefox 翻譯那一筆的頁面裡設）。
    pub wifi_only: bool,
    /// 這個平台能不能安全地存金鑰（Android 以外不能）。
    pub secrets_supported: bool,
    /// 上次用來翻的服務。
    pub last_service: Option<String>,
    /// 上次用來產生重點筆記的服務。
    pub last_digest_service: Option<String>,
    /// 快速翻譯用的服務；`null` 是「跟全文翻譯相同」。
    pub quick_service: Option<String>,
    /// 快速翻譯選用的預設組。
    pub quick_prompt: String,
    /// 快速翻譯已經同意送出的服務與主機（`服務代號@主機`）。
    pub quick_consent: Vec<String>,
}

/// 文字實際會送到的主機。
pub(crate) async fn host_of(app: &AppHandle, s: &Service) -> String {
    if s.provider.is_on_device() {
        return String::new();
    }
    let e = s.endpoint.trim();
    if !e.is_empty() {
        return e
            .split("://")
            .nth(1)
            .unwrap_or(e)
            .split('/')
            .next()
            .unwrap_or(e)
            .to_string();
    }
    // DeepL 免費版的金鑰以 `:fx` 結尾，會送到另一個主機
    if s.provider == Provider::Deepl
        && let Ok(Some(k)) = app.paperreader().secret_get(&s.id).await
        && k.ends_with(":fx")
    {
        return "api-free.deepl.com".to_string();
    }
    s.provider.info().official_host.to_string()
}

async fn settings_view(app: &AppHandle, t: TranslateSettings) -> SettingsView {
    let (keys, supported) = match app.paperreader().secret_names().await {
        Ok(names) => (names, true),
        Err(_) => (Vec::new(), false),
    };
    let mut services = Vec::with_capacity(t.services.len());
    for s in &t.services {
        services.push(ServiceView {
            has_key: keys.contains(&s.id),
            host: host_of(app, s).await,
            service: s.clone(),
        });
    }
    let view = |p: PromptPreset, builtin: bool, modified: bool| PresetView {
        id: p.id,
        name: if builtin { String::new() } else { p.name },
        kind: p.kind,
        text: p.text,
        sections: p.sections,
        builtin,
        modified,
    };
    let prompts = BUILTIN_PRESETS
        .iter()
        .map(|b| match t.prompts.iter().find(|p| p.id == b.id) {
            Some(p) => view(p.clone(), true, true),
            None => view(PromptPreset::from_builtin(b), true, false),
        })
        .chain(
            t.prompts
                .iter()
                .filter(|p| paperreader_translate::builtin_preset(&p.id).is_none())
                .map(|p| view(p.clone(), false, false)),
        )
        .collect();
    SettingsView {
        services,
        primary: t.primary,
        fallbacks: t.fallbacks,
        source_lang: t.source_lang,
        target_lang: t.target_lang,
        skip_confirm: t.skip_confirm,
        prompt: t.prompt,
        digest_prompt: t.digest_prompt,
        prompts,
        refine: t.refine,
        final_pass: t.final_pass,
        wifi_only: t.wifi_only,
        secrets_supported: supported,
        last_service: t.last_service,
        last_digest_service: t.last_digest_service,
        quick_service: t.quick_service,
        quick_prompt: t.quick_prompt,
        quick_consent: t.quick_consent,
    }
}

#[tauri::command]
pub async fn translate_settings_get(
    app: AppHandle,
    state: State<'_, AppState>,
) -> ApiResult<SettingsView> {
    Ok(settings_view(&app, state.translate_settings()).await)
}

/// 改設定的一部分：只改給了的欄位。服務本身用 `translate_service_*`。
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct SettingsPatch {
    pub primary: Option<String>,
    pub fallbacks: Option<Vec<String>>,
    pub source_lang: Option<String>,
    pub target_lang: Option<String>,
    pub skip_confirm: Option<bool>,
    pub prompt: Option<String>,
    pub digest_prompt: Option<String>,
    pub prompts: Option<Vec<PromptPreset>>,
    pub refine: Option<bool>,
    pub final_pass: Option<bool>,
    pub wifi_only: Option<bool>,
    /// 這次用來翻的服務。不在服務清單裡的不記。
    pub last_service: Option<String>,
    /// 這次用來產生重點筆記的服務。不在服務清單裡的不記。
    pub last_digest_service: Option<String>,
    /// 快速翻譯用的服務：服務的代號，空字串是改回「跟全文翻譯相同」。不在服務清單裡的不收。
    pub quick_service: Option<String>,
    /// 快速翻譯選用的預設組。
    pub quick_prompt: Option<String>,
    /// 快速翻譯已經同意的服務與主機：整份取代（空清單是「重新詢問」）。
    pub quick_consent: Option<Vec<String>>,
}

#[tauri::command]
pub async fn translate_settings_patch(
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> ApiResult<()> {
    state
        .run("translate_settings_patch", move |s| {
            s.update_translate(|t| {
                if let Some(v) = patch.primary {
                    t.fallbacks.retain(|f| f != &v);
                    t.primary = v;
                }
                if let Some(v) = patch.fallbacks {
                    t.fallbacks = v;
                }
                if let Some(v) = patch.source_lang {
                    t.source_lang = v;
                }
                if let Some(v) = patch.target_lang {
                    t.target_lang = v;
                }
                if let Some(v) = patch.skip_confirm {
                    t.skip_confirm = v;
                }
                if let Some(v) = patch.prompts {
                    t.prompts = v;
                    // 選用的那一組被刪掉了：改回內建的
                    t.fix_selection();
                }
                if let Some(v) = patch.prompt {
                    t.prompt = v;
                }
                if let Some(v) = patch.digest_prompt {
                    t.digest_prompt = v;
                }
                if let Some(v) = patch.refine {
                    t.refine = v;
                }
                if let Some(v) = patch.final_pass {
                    t.final_pass = v;
                }
                if let Some(v) = patch.wifi_only {
                    t.wifi_only = v;
                }
                if let Some(v) = patch.last_service
                    && t.service(&v).is_some()
                {
                    t.last_service = Some(v);
                }
                if let Some(v) = patch.last_digest_service
                    && t.service(&v).is_some()
                {
                    t.last_digest_service = Some(v);
                }
                if let Some(v) = patch.quick_service {
                    // 空字串：跟全文翻譯相同；指到不存在的服務由 `validate` 擋下
                    t.quick_service = Some(v).filter(|v| !v.is_empty());
                }
                if let Some(v) = patch.quick_prompt {
                    t.quick_prompt = v;
                }
                if let Some(mut v) = patch.quick_consent {
                    v.retain(|k| !k.is_empty());
                    v.sort();
                    v.dedup();
                    t.quick_consent = v;
                }
                Ok(())
            })
            .map(|_| ())
        })
        .await
}

/// 存一組提示詞預設組（依代號取代，沒有就加在後面）。代號是內建的：存成改過的內建組（「還原預設」用
/// `translate_prompt_delete`）。只改這一組，不拿畫面上的整份清單蓋回去。
#[tauri::command]
pub async fn translate_prompt_save(
    state: State<'_, AppState>,
    preset: PromptPreset,
) -> ApiResult<()> {
    validate_preset(&preset)?;
    state
        .run("translate_prompt_save", move |s| {
            s.update_translate(|t| {
                match t.prompts.iter_mut().find(|p| p.id == preset.id) {
                    Some(slot) if slot.kind != preset.kind => {
                        return Err(ApiError::new(ErrorKind::InvalidInput, "prompt kind"));
                    }
                    Some(slot) => *slot = preset,
                    None => t.prompts.push(preset),
                }
                Ok(())
            })
            .map(|_| ())
        })
        .await
}

/// 刪一組自訂的預設組，或把改過的內建組還原預設。選用的那一組被刪掉時改回內建的。
#[tauri::command]
pub async fn translate_prompt_delete(state: State<'_, AppState>, id: String) -> ApiResult<()> {
    state
        .run("translate_prompt_delete", move |s| {
            s.update_translate(|t| {
                t.prompts.retain(|p| p.id != id);
                t.fix_selection();
                Ok(())
            })
            .map(|_| ())
        })
        .await
}

/// 「看完整提示詞」用的範例原文（實際送出時換成論文的段落）。
const SAMPLE_BEFORE: &str = "The previous paragraph is sent as context only.";

/// 看完整提示詞：用 `preset`（可以是還沒存的草稿）與這篇的篇名、術語庫、這篇的術語，替一段範例原文
/// 組出實際會送出的每一次請求。和翻譯、重點筆記實際送出的用同一組函式產生，逐字相同；固定的部分標 `fixed`。
/// `refine` 開著時多列第二輪潤飾那一次；`material_id`（接著翻的那份譯本）給了就用它自己的語言與術語，
/// 沒給用目前的設定與新的一份建立時會帶過去的術語（使用者的決定）；重點筆記用這篇最新那份同語言譯本照用的術語。
#[tauri::command]
pub async fn prompt_preview(
    state: State<'_, AppState>,
    preset: PromptPreset,
    paper_id: Option<String>,
    material_id: Option<String>,
    refine: Option<bool>,
) -> ApiResult<Vec<PromptPreview>> {
    validate_preset(&preset)?;
    let t = state.translate_settings();
    let (title, overlay) = match paper_id.clone() {
        Some(pid) => {
            let mid = material_id.clone();
            state
                .lib("prompt_preview", move |l| {
                    let (p, _) = l.get_paper(&pid)?;
                    let overlay = match mid {
                        Some(m) => Some(l.read_overlay(&pid, &m)?.0),
                        None => None,
                    };
                    Ok((p.bib.title.filter(|t| !t.trim().is_empty()), overlay))
                })
                .await?
        }
        None => (None, None),
    };
    let (source_lang, target_lang) = match &overlay {
        Some(d) => (d.source_lang.clone(), d.target_lang.clone()),
        None => (
            Some(t.source_lang.clone()).filter(|l| !l.is_empty()),
            t.target_lang.clone(),
        ),
    };
    // 翻譯：接著翻用那份譯本的；新的一份用建立時會帶過去的（使用者的決定，`inherited_terms`）。
    // 重點筆記：這篇最新那份同語言譯本照用的（同 `digest_run`）
    let terms = match (&overlay, &paper_id, preset.kind) {
        (Some(d), _, _) => terms_in_use(&to_paper_terms(&d.terms)),
        (None, Some(p), PresetKind::Translation) => terms_in_use(&to_paper_terms(
            &inherited_terms(&latest_overlay_entries(&state, p, &target_lang).await),
        )),
        (None, Some(p), PresetKind::Digest | PresetKind::Quick) => {
            latest_overlay_terms(&state, p, &target_lang).await
        }
        (None, None, _) => Vec::new(),
    };
    let sample = Segment {
        id: "g0001".to_string(),
        text: TEST_SENTENCE.to_string(),
        before: Some(SAMPLE_BEFORE.to_string()),
        ..Default::default()
    };
    Ok(match preset.kind {
        PresetKind::Translation => {
            let params = Params {
                instructions: preset.text,
                refine: false,
                ..params_for(&state, &t, source_lang, target_lang, title)
            };
            let batch = std::slice::from_ref(&sample);
            let mut out = vec![translate_preview(&params, batch, &terms)];
            if refine.unwrap_or(false) {
                out.push(refine_preview(
                    &params,
                    batch,
                    &[SAMPLE_DRAFT.to_string()],
                    &terms,
                ));
            }
            out
        }
        PresetKind::Digest => vec![digest_preview(&DigestInput {
            title,
            abstract_text: None,
            segments: vec![DigestSegment {
                id: "g0001".to_string(),
                page: 0,
                text: TEST_SENTENCE.to_string(),
            }],
            target_lang,
            instructions: preset.text.clone(),
            sections: preset.section_specs(),
            glossary: user_glossary(&state),
            paper_terms: terms,
        })],
        // 快速翻譯：選一句的範例（選一個詞時「回覆的格式」換成詞義與在這句裡的意思）
        PresetKind::Quick => {
            let params = Params {
                instructions: preset.text,
                refine: false,
                ..params_for(&state, &t, source_lang, target_lang, title)
            };
            let input = QuickInput {
                text: TEST_SENTENCE.to_string(),
                before: Some(SAMPLE_BEFORE.to_string()),
                after: Some(SAMPLE_AFTER.to_string()),
            };
            vec![quick_preview(&params, &input, &terms)]
        }
    })
}

/// 「看完整提示詞」快速翻譯的範例後文（實際送出時是選取後面的一句）。
const SAMPLE_AFTER: &str = "The next sentence is sent as context only.";

/// 「看完整提示詞」第二輪潤飾那一次的範例草稿（實際送出時是第一次請求的譯文）。
const SAMPLE_DRAFT: &str = "（第一次請求的譯文）";

/// 一組預設組哪裡不合規則（設定的編輯畫面用；規則只有外殼這一份，`validate_preset`）：合規則交回 `None`，
/// 否則交回原因的代號（`prompt name`、`section label` 等）。
#[tauri::command]
pub async fn prompt_check(preset: PromptPreset) -> ApiResult<Option<String>> {
    Ok(validate_preset(&preset).err().map(|e| e.detail))
}

// ---------- 這份譯本的術語（記在譯文疊加檔） ----------

/// 疊加檔記的術語 → 引擎用的（狀態不認得的略過）。
pub(crate) fn to_paper_terms(terms: &[OverlayTerm]) -> Vec<PaperTerm> {
    terms
        .iter()
        .filter_map(|t| {
            let state = match t.state.as_str() {
                TERM_CANDIDATE => TermState::Candidate,
                TERM_LEARNED => TermState::Learned,
                TERM_USER => TermState::User,
                TERM_REMOVED => TermState::Removed,
                _ => return None,
            };
            Some(PaperTerm {
                source: t.source.clone(),
                target: t.target.clone(),
                state,
                batches: t.batches,
            })
        })
        .collect()
}

/// 引擎的術語 → 寫進疊加檔的。原本就有的那一條（原文詞相同）留著它不認得的欄位；
/// 狀態這一版不認得的原樣留在後面。
pub(crate) fn to_overlay_terms(terms: &[PaperTerm], old: &[OverlayTerm]) -> Vec<OverlayTerm> {
    let mut out: Vec<OverlayTerm> = terms
        .iter()
        .map(|t| {
            let extra = old
                .iter()
                .find(|o| o.source.eq_ignore_ascii_case(&t.source))
                .map(|o| o.extra.clone())
                .unwrap_or_default();
            OverlayTerm {
                source: t.source.clone(),
                target: t.target.clone(),
                state: match t.state {
                    TermState::Candidate => TERM_CANDIDATE,
                    TermState::Learned => TERM_LEARNED,
                    TermState::User => TERM_USER,
                    TermState::Removed => TERM_REMOVED,
                }
                .to_string(),
                batches: t.batches,
                extra,
            }
        })
        .collect();
    for o in old {
        let known =
            [TERM_CANDIDATE, TERM_LEARNED, TERM_USER, TERM_REMOVED].contains(&o.state.as_str());
        if !known && !out.iter().any(|t| t.source.eq_ignore_ascii_case(&o.source)) {
            out.push(o.clone());
        }
    }
    out
}

/// 新的一份譯本（整篇重翻）從這篇最新那份同語言譯本帶過來的術語：只帶使用者的決定（改的、刪的）。
/// 學到的不帶：整篇重翻常是為了換掉不好的譯法，新的一份重新學。
pub(crate) fn inherited_terms(old: &[OverlayTerm]) -> Vec<OverlayTerm> {
    old.iter()
        .filter(|t| t.state == TERM_USER || t.state == TERM_REMOVED)
        .cloned()
        .collect()
}

/// 這篇最新那份同語言譯本記的術語（全部）；沒有是空的。讀不到不擋事情，只是沒有術語。
async fn latest_overlay_entries(state: &AppState, paper_id: &str, lang: &str) -> Vec<OverlayTerm> {
    let (pid, lang) = (paper_id.to_string(), lang.to_string());
    state
        .lib("overlay_terms_latest", move |l| {
            let (paper, _) = l.get_paper(&pid)?;
            for m in paper.materials.iter().rev().filter(|m| {
                m.role == paperreader_core::format::ROLE_TRANSLATION && m.form == OVERLAY_FORM
            }) {
                if let Ok((d, _)) = l.read_overlay(&pid, &m.id)
                    && d.target_lang.eq_ignore_ascii_case(&lang)
                {
                    return Ok(d.terms);
                }
            }
            Ok(Vec::new())
        })
        .await
        .unwrap_or_default()
}

/// 這篇最新那份同語言譯本照用的術語（重點筆記、預覽用）。
pub(crate) async fn latest_overlay_terms(
    state: &AppState,
    paper_id: &str,
    lang: &str,
) -> Vec<(String, String)> {
    terms_in_use(&to_paper_terms(
        &latest_overlay_entries(state, paper_id, lang).await,
    ))
}

/// 使用者在「檢查譯文」改、刪這份譯本的術語：整份換掉。改了的之後的批次與重點筆記照新的用；
/// 已翻的段落不自動重翻。這份正在翻的時候不收（翻譯工作會把它記的蓋回去）。
#[tauri::command]
pub async fn overlay_terms_set(
    state: State<'_, AppState>,
    slot: State<'_, JobSlot<JobStatus>>,
    paper_id: String,
    material_id: String,
    terms: Vec<OverlayTerm>,
) -> ApiResult<()> {
    if slot.busy_with(&paper_id, &material_id) {
        return Err(ApiError::new(ErrorKind::TranslateBusy, "a job is running"));
    }
    state
        .lib("overlay_terms_set", move |l| {
            let (_, token) = l.read_overlay(&paper_id, &material_id)?;
            l.update_overlay(&paper_id, &material_id, &[], Some(&terms), &token)
                .map(|_| ())
        })
        .await
}

/// 這篇在這台裝置上用過的詞元數，依服務加總（畫面照各服務的單價粗估費用）。
#[tauri::command]
pub async fn usage_paper(
    state: State<'_, AppState>,
    paper_id: String,
) -> ApiResult<Vec<ServiceUsage>> {
    let log: UsageLog = state.read_side(USAGE_FILE);
    Ok(log.for_paper(&paper_id))
}

/// 一份工作的用量：一批一批記進 `usage.json`（同一份工作加到同一筆），也放進畫面看的狀態。
pub(crate) struct UsageMeter {
    job: String,
    paper: String,
    kind: &'static str,
    service: String,
    flushed: TokenUsage,
}

impl UsageMeter {
    pub(crate) fn new(paper: &str, kind: &'static str, service: &str) -> UsageMeter {
        Self::for_job(&new_id(kind, |_| false), paper, kind, service)
    }

    /// 記到指定的那一筆（同一個 `job` 與服務加在一起）：快速翻譯每一篇、每個服務只占一筆，
    /// 不會因為查很多次而把翻譯與重點筆記的紀錄擠出上限。
    pub(crate) fn for_job(job: &str, paper: &str, kind: &'static str, service: &str) -> UsageMeter {
        UsageMeter {
            job: job.to_string(),
            paper: paper.to_string(),
            kind,
            service: service.to_string(),
            flushed: TokenUsage::default(),
        }
    }

    /// 把引擎到現在多出來的用量記下（寫不成只記一行，不擋翻譯）。交回到現在的總數。
    pub(crate) fn flush(&mut self, state: &AppState, engine: &Engine) -> TokenUsage {
        let now = engine.usage();
        let d = now.since(self.flushed);
        if d != TokenUsage::default() {
            let (job, paper, kind, service) = (&self.job, &self.paper, self.kind, &self.service);
            match state.update_side(USAGE_FILE, |log: &mut UsageLog| {
                log.add(job, paper, kind, service, d)
            }) {
                Ok(()) => self.flushed = now,
                Err(e) => {
                    let _ = e.log("usage_record");
                }
            }
        }
        now
    }
}

/// 新增一個服務（預設啟用），交回它的代號。
#[tauri::command]
pub async fn translate_service_add(
    state: State<'_, AppState>,
    provider: Provider,
) -> ApiResult<String> {
    state
        .run("translate_service_add", move |s| {
            let mut id = String::new();
            s.update_translate(|t| {
                id = new_id(provider.key(), |c| t.service(c).is_some());
                t.services.push(Service::default_for(&id, provider));
                Ok(())
            })?;
            Ok(id)
        })
        .await
}

/// 存一個服務（依代號取代）。引擎種類不能改：要換引擎就新增一個。
#[tauri::command]
pub async fn translate_service_save(state: State<'_, AppState>, service: Service) -> ApiResult<()> {
    state
        .run("translate_service_save", move |s| {
            s.update_translate(|t| {
                let slot = t
                    .services
                    .iter_mut()
                    .find(|x| x.id == service.id)
                    .ok_or_else(|| ApiError::new(ErrorKind::NotFound, "service"))?;
                if slot.provider != service.provider {
                    return Err(ApiError::new(ErrorKind::InvalidInput, "engine kind"));
                }
                *slot = service;
                Ok(())
            })
            .map(|_| ())
        })
        .await
}

/// 還原預設：名稱、模型、網址、速率回到引擎的預設（金鑰與開關不動）。
#[tauri::command]
pub async fn translate_service_reset(state: State<'_, AppState>, id: String) -> ApiResult<()> {
    state
        .run("translate_service_reset", move |s| {
            s.update_translate(|t| {
                let slot = t
                    .services
                    .iter_mut()
                    .find(|x| x.id == id)
                    .ok_or_else(|| ApiError::new(ErrorKind::NotFound, "service"))?;
                *slot = Service {
                    enabled: slot.enabled,
                    ..Service::default_for(&slot.id, slot.provider)
                };
                Ok(())
            })
            .map(|_| ())
        })
        .await
}

/// 刪除一個服務與它的金鑰。主要服務被刪時，改由第一個還啟用的服務當主要服務。
#[tauri::command]
pub async fn translate_service_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> ApiResult<()> {
    let gone = id.clone();
    state
        .run("translate_service_delete", move |s| {
            s.update_translate(|t| {
                if t.services.len() <= 1 {
                    return Err(ApiError::new(ErrorKind::InvalidInput, "last service"));
                }
                t.services.retain(|x| x.id != gone);
                t.fallbacks.retain(|f| f != &gone);
                // 快速翻譯用的是它：改回「跟全文翻譯相同」
                if t.quick_service.as_deref() == Some(gone.as_str()) {
                    t.quick_service = None;
                }
                if t.primary == gone {
                    t.primary = t
                        .services
                        .iter()
                        .find(|x| x.enabled)
                        .unwrap_or(&t.services[0])
                        .id
                        .clone();
                    let p = t.primary.clone();
                    t.fallbacks.retain(|f| f != &p);
                }
                Ok(())
            })
            .map(|_| ())
        })
        .await?;
    // 金鑰刪不掉（例如電腦上本來就沒有）不擋刪除
    let _ = app.paperreader().secret_delete(&id).await;
    Ok(())
}

#[tauri::command]
pub async fn translate_key_set(
    app: AppHandle,
    state: State<'_, AppState>,
    service: String,
    key: String,
) -> ApiResult<()> {
    let key = key.trim().to_string();
    if key.is_empty() || key.chars().count() > 500 {
        return Err(ApiError::new(ErrorKind::InvalidInput, "key"));
    }
    if state.translate_settings().service(&service).is_none() {
        return Err(ApiError::new(ErrorKind::NotFound, "service"));
    }
    app.paperreader()
        .secret_set(&service, &key)
        .await
        .map_err(|e| ApiError::from(e).log("translate_key_set"))
}

#[tauri::command]
pub async fn translate_key_clear(app: AppHandle, service: String) -> ApiResult<()> {
    app.paperreader()
        .secret_delete(&service)
        .await
        .map_err(|e| ApiError::from(e).log("translate_key_clear"))
}

// ---------- 引擎 ----------

pub(crate) fn find_service(t: &TranslateSettings, id: &str) -> ApiResult<Service> {
    t.service(id)
        .cloned()
        .ok_or_else(|| ApiError::new(ErrorKind::NotFound, "service"))
}

/// 一個服務的引擎設定。缺金鑰、缺模型時回報 `TranslateConfig`。
async fn engine_config(app: &AppHandle, s: &Service) -> ApiResult<EngineConfig> {
    let (model, endpoint) = s.model_endpoint();
    let key = app
        .paperreader()
        .secret_get(&s.id)
        .await
        .map_err(|e| ApiError::from(e).log("secret_get"))?
        .unwrap_or_default();
    if key.is_empty() && s.provider.info().key == Need::Required {
        return Err(ApiError::new(ErrorKind::TranslateConfig, "no API key"));
    }
    Ok(EngineConfig {
        provider: s.provider,
        model,
        endpoint,
        api_key: key,
    })
}

/// 裝置上的引擎（Firefox 翻譯）：先確認模型已經下載（沒下載就說明，不要開始了才一段一段失敗）。
async fn device_engine(
    app: &AppHandle,
    provider: Provider,
    target_lang: &str,
) -> ApiResult<Engine> {
    let pair = ondevice::bergamot_pair(Some("en"), target_lang)
        .ok_or_else(|| ApiError::new(ErrorKind::TranslateLanguage, "unsupported language"))?;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|_| ApiError::new(ErrorKind::Io, "app data dir"))?;
    if !ondevice::bergamot_ready(&data, pair) {
        return Err(ApiError::new(
            ErrorKind::TranslateNoModel,
            "model not downloaded",
        ));
    }
    let bridge = app.state::<Arc<Bridge>>().inner().clone();
    Engine::on_device(provider, Arc::new(BergamotDevice { bridge }))
        .map_err(|e| ApiError::new(api_kind(e.kind), "engine").log("translate_engine"))
}

/// 全部引擎的申報：要填什麼、會不會送出裝置、批次、支援的目標語言。
/// 畫面照它顯示欄位、過濾目標語言、決定送出前要不要詢問。
#[tauri::command]
pub async fn translate_engines() -> ApiResult<Vec<paperreader_translate::EngineInfo>> {
    Ok(paperreader_translate::engines())
}

/// 一個服務的引擎（照它的速率）。
pub(crate) async fn engine_for(
    app: &AppHandle,
    s: &Service,
    target_lang: &str,
) -> ApiResult<Engine> {
    engine_for_with(app, s, target_lang, false).await
}

/// `quick` 是快速翻譯用的：連線逾時短、不重試；只影響送出裝置的服務。
pub(crate) async fn engine_for_with(
    app: &AppHandle,
    s: &Service,
    target_lang: &str,
    quick: bool,
) -> ApiResult<Engine> {
    if s.provider.info().target(target_lang).is_none() {
        return Err(ApiError::new(
            ErrorKind::TranslateLanguage,
            "unsupported language",
        ));
    }
    if s.provider.is_on_device() {
        return device_engine(app, s.provider, target_lang).await;
    }
    let cfg = engine_config(app, s).await?;
    let built = if quick {
        Engine::new_quick(&cfg)
    } else {
        Engine::new(&cfg)
    };
    built
        .map(|e| e.with_rate(s.rpm))
        .map_err(|e| ApiError::new(api_kind(e.kind), "engine").log("translate_engine"))
}

/// 使用者的術語庫（原文詞 → 譯法），翻譯與重點筆記共用。
pub(crate) fn user_glossary(state: &AppState) -> Vec<(String, String)> {
    state
        .glossary()
        .into_iter()
        .map(|g| (g.source, g.target))
        .collect()
}

/// 一次翻譯共同的參數：語言、選用的提示詞、術語庫、潤飾。
fn params_for(
    state: &AppState,
    t: &TranslateSettings,
    source_lang: Option<String>,
    target_lang: String,
    title: Option<String>,
) -> Params {
    Params {
        source_lang,
        target_lang,
        glossary: user_glossary(state),
        instructions: t.prompt_text(),
        title,
        refine: t.refine,
    }
}

/// 測試服務時送出的句子（畫面把送出與收到的並排列出）。
const TEST_SENTENCE: &str = "The model is trained on a large corpus.";

/// 測試一個服務的結果：送出什麼、收到什麼、用哪個模型、花了多久。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub sent: String,
    pub received: String,
    /// 對話模型的名稱；機器翻譯與離線引擎是空的（畫面改寫引擎的名稱）。
    pub model: String,
    pub ms: u64,
}

/// 測試一個服務：翻一句短句。金鑰不對、連不上、模型名稱不對都會在這裡看到。
#[tauri::command]
pub async fn translate_test(
    app: AppHandle,
    state: State<'_, AppState>,
    service: String,
) -> ApiResult<TestResult> {
    let t = state.translate_settings();
    let s = find_service(&t, &service)?;
    let engine = engine_for(&app, &s, &t.target_lang)
        .await?
        .with_retry(Duration::from_millis(500), 2);
    let params = Params {
        refine: false,
        ..params_for(&state, &t, Some("en".into()), t.target_lang.clone(), None)
    };
    let seg = Segment {
        id: "1".to_string(),
        text: TEST_SENTENCE.to_string(),
        before: None,
        ..Default::default()
    };
    let started = std::time::Instant::now();
    let mut out = engine
        .translate_batch(&params, std::slice::from_ref(&seg), &Cancel::new())
        .await
        .map_err(|e| ApiError::new(api_kind(e.kind), "test").log("translate_test"))?;
    Ok(TestResult {
        sent: TEST_SENTENCE.to_string(),
        received: out.pop().unwrap_or_default(),
        model: if s.provider.is_chat_model() {
            s.model_endpoint().0
        } else {
            String::new()
        },
        ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

// ---------- 譯本與翻譯工作 ----------

/// 畫面抽好的一段（版面與原文）。
#[derive(Deserialize)]
pub struct SegmentInput {
    pub page: u32,
    pub bbox: [f64; 4],
    pub kind: String,
    pub size: Option<f64>,
    pub source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayCreated {
    pub material_id: String,
}

/// 建立一份譯本（版面紀錄，每段待翻）。語言用目前的設定，記下要用的服務的引擎與模型。
#[tauri::command]
pub async fn overlay_create(
    state: State<'_, AppState>,
    paper_id: String,
    source_material: String,
    segments: Vec<SegmentInput>,
    layout_model_pages: Option<Vec<u32>>,
    service: String,
) -> ApiResult<OverlayCreated> {
    let t = state.translate_settings();
    let svc = find_service(&t, &service)?;
    if svc.provider.info().target(&t.target_lang).is_none() {
        return Err(ApiError::new(
            ErrorKind::TranslateLanguage,
            "unsupported language",
        ));
    }
    let (model, _) = svc.model_endpoint();
    // 這篇最新那份同語言譯本裡使用者的決定（改的、刪的術語）帶到新的一份
    let inherit = inherited_terms(&latest_overlay_entries(&state, &paper_id, &t.target_lang).await);
    state
        .run("overlay_create", move |s| {
            let draft = OverlayDraft {
                source_material,
                provider: svc.provider.key().to_string(),
                model: if svc.provider.is_chat_model() {
                    model
                } else {
                    String::new()
                },
                source_lang: Some(t.source_lang.clone()).filter(|l| !l.is_empty()),
                target_lang: t.target_lang.clone(),
                segments: segments
                    .into_iter()
                    .map(|g| SegmentDraft {
                        page: g.page,
                        bbox: g.bbox,
                        kind: g.kind,
                        size: g.size,
                        source: g.source,
                    })
                    .collect(),
                layout_model_pages: layout_model_pages.unwrap_or_default(),
            };
            s.with_lib(|l| {
                let (_, token) = l.get_paper(&paper_id)?;
                let c = l.create_overlay(&paper_id, draft, &token)?;
                if !inherit.is_empty() {
                    // 帶不過去不擋建立：只是從空的開始
                    let _ = l.update_overlay(
                        &paper_id,
                        &c.material_id,
                        &[],
                        Some(&inherit),
                        &c.overlay_token,
                    );
                }
                Ok(OverlayCreated {
                    material_id: c.material_id,
                })
            })
        })
        .await
}

/// 讀一份譯本的全部內容（版面、原文、譯文、狀態）。
#[tauri::command]
pub async fn overlay_read(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
) -> ApiResult<OverlayFile> {
    state
        .lib("overlay_read", move |l| {
            l.read_overlay(&paper_id, &material_id).map(|(d, _)| d)
        })
        .await
}

/// 一份譯本裡跨欄、跨頁被切成前後兩段的句子（接續）：每一對是（前一段, 後一段）的代號。譯文檢視用它把兩段接成一段。
#[tauri::command]
pub async fn overlay_continuations(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
) -> ApiResult<Vec<(String, String)>> {
    let doc = state
        .lib("overlay_continuations", move |l| {
            l.read_overlay(&paper_id, &material_id).map(|(d, _)| d)
        })
        .await?;
    Ok(continuation_pairs(&doc)
        .into_iter()
        .map(|(a, b)| (doc.segments[a].id.clone(), doc.segments[b].id.clone()))
        .collect())
}

/// 接續的（前一段, 後一段）位置（`paperreader_translate::continuation`）：只在建立譯本時用版面模型分段的頁上認
/// （譯文疊加檔的 `layout_model_pages`）。
fn continuation_pairs(doc: &OverlayFile) -> Vec<(usize, usize)> {
    let by_model: HashSet<u32> = doc.layout_model_pages.iter().copied().collect();
    let placed: Vec<Placed> = doc
        .segments
        .iter()
        .map(|g| Placed {
            page: g.page,
            bbox: g.bbox,
            kind: &g.kind,
            size: g.size,
            by_model: by_model.contains(&g.page),
            source: &g.source,
        })
        .collect();
    continuations(&placed)
}

/// 前後文最多幾個字。
const CONTEXT_CHARS: usize = 400;

/// 送去翻的段落（`pick` 挑位置）：前一段原文的結尾當前文。跨欄、跨頁的接續記下它接著哪一段，前文用那一段的結尾。
fn job_segments(doc: &OverlayFile, pick: impl Fn(usize) -> bool) -> Vec<Segment> {
    let follows: HashMap<usize, usize> = continuation_pairs(doc)
        .into_iter()
        .map(|(a, b)| (b, a))
        .collect();
    // 篇名：首頁的第一個標題
    let title = doc
        .segments
        .iter()
        .position(|g| g.page == 0 && g.kind == "heading");
    doc.segments
        .iter()
        .enumerate()
        .filter(|(i, _)| pick(*i))
        .map(|(i, g)| {
            let first_half = follows.get(&i).copied();
            // 前文是前一段（接續的話是它接著的那一段）的最後一部分：離這一段最近的字
            let before = first_half
                .or_else(|| i.checked_sub(1))
                .map_or(String::new(), |p| {
                    let v: Vec<char> = doc.segments[p].source.chars().collect();
                    v[v.len().saturating_sub(CONTEXT_CHARS)..].iter().collect()
                });
            Segment {
                id: g.id.clone(),
                text: g.source.clone(),
                before: (!before.is_empty()).then_some(before),
                continues: first_half.map(|a| doc.segments[a].id.clone()),
                after: None,
                title: title == Some(i),
            }
        })
        .collect()
}

/// 同一個括號英文在一篇裡只留第一次（依原文的順序）：記著每一段現在的譯文附了哪些括號英文。
struct Glosses {
    at: HashMap<String, usize>,
    per_segment: Vec<Vec<String>>,
}

impl Glosses {
    fn new(doc: &OverlayFile) -> Glosses {
        Glosses {
            at: doc
                .segments
                .iter()
                .enumerate()
                .map(|(i, g)| (g.id.clone(), i))
                .collect(),
            per_segment: doc
                .segments
                .iter()
                .map(|g| match (&g.target, g.status == STATUS_DONE) {
                    (Some(t), true) => glosses(t),
                    _ => Vec::new(),
                })
                .collect(),
        }
    }

    /// 這一段新的譯文：前面的段落附過的括號英文拿掉，記下這一段留下的。
    fn apply(&mut self, id: &str, text: &str) -> String {
        let Some(&i) = self.at.get(id) else {
            return text.to_string();
        };
        let mut seen: HashSet<String> = self.per_segment[..i].iter().flatten().cloned().collect();
        let out = drop_repeated_glosses(text, &mut seen);
        self.per_segment[i] = glosses(&out);
        out
    }
}

/// 讀譯本與篇名（`command` 是記錄用的指令名稱）。
async fn read_doc(
    state: &AppState,
    command: &'static str,
    paper_id: &str,
    material_id: &str,
) -> ApiResult<(OverlayFile, Option<String>)> {
    let (pid, mid) = (paper_id.to_string(), material_id.to_string());
    state
        .lib(command, move |l| {
            let (doc, _) = l.read_overlay(&pid, &mid)?;
            let (paper, _) = l.get_paper(&pid)?;
            Ok((doc, paper.bib.title.filter(|t| !t.trim().is_empty())))
        })
        .await
}

/// 停下來時讓畫面問的「改用下一個服務」：`next` 是依序的下一個（翻譯與重點筆記各有各的挑法）。
pub(crate) async fn fallback_offer(
    app: &AppHandle,
    next: Option<Service>,
) -> Option<FallbackOffer> {
    let next = next?;
    Some(FallbackOffer {
        host: host_of(app, &next).await,
        id: next.id,
        name: next.name,
        provider: next.provider,
    })
}

/// 開始（或接著）翻：只翻還沒翻好的段落，用 `service` 這個服務。背景進行，立刻交回；
/// 進度用 `translate_status` 問。服務連不上、額度用完、一直被限速而停下時，狀態裡帶著依序的
/// 下一個服務，畫面先問使用者，同意了再用那個服務呼叫這裡接著翻。
/// 原地更新同一份譯本：只送還沒翻好的段落（`pending`、`failed`），已翻的（含使用者決定的）
/// 不動；這一輪翻不出來的段落，原本是已翻的就不寫（留著舊的譯文）。整篇重翻是另做一份新的譯本（畫面先建一份
/// 新的疊加檔再呼叫這裡）。
#[tauri::command]
pub async fn translate_run(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: State<'_, JobSlot<JobStatus>>,
    paper_id: String,
    material_id: String,
    service: String,
) -> ApiResult<()> {
    let blank = |paper_id: &str, material_id: &str| JobStatus {
        paper_id: paper_id.to_string(),
        material_id: material_id.to_string(),
        service: service.clone(),
        total: 0,
        done: 0,
        failed: 0,
        state: JobState::Running,
        reason: None,
        fallback: None,
        input_tokens: 0,
        output_tokens: 0,
    };
    // 先占位（檢查「已有工作」與登記在同一把鎖裡），後面任何一步失敗再還原
    let cancel = Cancel::new();
    let previous = slot.begin(Job {
        cancel: cancel.clone(),
        status: blank(&paper_id, &material_id),
        extra: (),
    })?;
    let t = state.translate_settings();
    let prepared = async {
        let svc = find_service(&t, &service)?;
        let (doc, title) = read_doc(&state, "translate_run", &paper_id, &material_id).await?;
        let engine = engine_for(&app, &svc, &doc.target_lang).await?;
        Ok::<_, ApiError>((engine, doc, title, segment_service(&svc)))
    }
    .await;
    let (engine, doc, title, by) = match prepared {
        Ok(v) => v,
        Err(e) => {
            slot.restore(previous);
            return Err(e);
        }
    };
    // 這份譯本記的術語（在譯文疊加檔裡）：接著翻沿用，使用者在「檢查譯文」改的照新的用
    let old_terms = doc.terms.clone();
    let engine = engine.with_paper_terms(to_paper_terms(&old_terms));
    let mut meter = UsageMeter::new(&paper_id, "translation", &service);
    let total = doc.segments.len();
    let done0 = doc
        .segments
        .iter()
        .filter(|g| !needs_translation(g))
        .count();
    // 已翻的段落：這一輪翻不出來也不寫
    let done_before: std::collections::HashSet<String> = doc
        .segments
        .iter()
        .filter(|g| g.status == STATUS_DONE)
        .map(|g| g.id.clone())
        .collect();
    // 前一段的原文當前後文（不管前一段翻了沒有）；接續的後一段用它接著的那一段
    let pending: Vec<Segment> = job_segments(&doc, |i| needs_translation(&doc.segments[i]));
    let mut glossed = Glosses::new(&doc);
    let final_pass = t.final_pass;
    let params = params_for(
        &state,
        &t,
        doc.source_lang.clone(),
        doc.target_lang.clone(),
        title,
    );

    let mut status = JobStatus {
        total,
        done: done0,
        ..blank(&paper_id, &material_id)
    };
    let publish =
        |slot: &JobSlot<JobStatus>, st: &JobStatus| slot.publish(|j| j.status = st.clone());
    publish(&slot, &status);

    let slot = slot.inner().clone();
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let mut written = engine.paper_terms();
        // 一批結果寫進譯本；`final_pass` 是收尾重翻換掉的段落（不算進進度）
        let mut sink = |mut ev: BatchEvent, final_pass: bool| {
            // 同一個括號英文在這篇前面出現過就拿掉
            for it in &mut ev.items {
                if let Ok(text) = &it.result {
                    it.result = Ok(glossed.apply(&it.id, text));
                }
            }
            // 用量：一批一批記下（中途停下也留著）
            let used = meter.flush(&state, &engine);
            status.input_tokens = used.input;
            status.output_tokens = used.output;
            // 這份譯本的術語變了：和這一批的段落同一次寫進疊加檔
            let now = engine.paper_terms();
            // 術語不合規則時只寫段落、記一行：不能讓付過費的譯文因為術語寫不進去
            let terms = (now != written)
                .then(|| to_overlay_terms(&now, &old_terms))
                .filter(|t| match paperreader_core::overlay::validate_terms(t) {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("paperreader: paper_terms skipped: {}", e.kind_name());
                        false
                    }
                });
            let updates = batch_updates(&ev.items, &done_before, &by);
            let wrote = if updates.is_empty() && terms.is_none() {
                Ok(())
            } else {
                state
                    .with_lib(|l| {
                        let (_, token) = l.read_overlay(&paper_id, &material_id)?;
                        l.update_overlay(
                            &paper_id,
                            &material_id,
                            &updates,
                            terms.as_deref(),
                            &token,
                        )
                    })
                    .map(|_| ())
            };
            if wrote.is_ok() && terms.is_some() {
                written = now;
            }
            if wrote.is_err() {
                return false;
            }
            if !final_pass {
                for it in &ev.items {
                    match it.result {
                        Ok(_) => status.done += 1,
                        Err(_) => status.failed += 1,
                    }
                }
            }
            publish(&slot, &status);
            true
        };
        let summary = run_job(&engine, &params, pending, &cancel, |ev| sink(ev, false)).await;
        let mut final_summary = None;
        // 收尾重翻：翻完（沒有取消、沒有停下）才做，只對對話模型
        if final_pass
            && engine.provider().is_chat_model()
            && !summary.cancelled
            && summary.stopped.is_none()
            && !summary.sink_failed
            && let Ok((doc, _)) = state.with_lib(|l| l.read_overlay(&paper_id, &material_id))
        {
            let items = final_pass_items(&doc, &terms_in_use(&engine.paper_terms()));
            if !items.is_empty() {
                final_summary = Some(
                    run_final_pass(&engine, &params, items, &cancel, |ev| sink(ev, true)).await,
                );
            }
        }
        // 最後一批之後還有用量（讀不懂而縮小批次再試、停下前的那一次）
        let used = meter.flush(&state, &engine);
        status.input_tokens = used.input;
        status.output_tokens = used.output;
        (status.state, status.reason) = final_state(&summary, final_summary.as_ref());
        let stopped = summary
            .stopped
            .or(final_summary.as_ref().and_then(|f| f.stopped));
        // 服務停擺（不是金鑰或設定的問題）：找依序的下一個服務，讓畫面問使用者要不要換
        if stopped.is_some_and(offers_fallback) {
            let next = state
                .translate_settings()
                .next_fallback(&status.service)
                .cloned();
            status.fallback = fallback_offer(&app, next).await;
        }
        publish(&slot, &status);
    });
    Ok(())
}

/// 目前（或最近一次）翻譯工作的狀態；從沒有過就是 `null`。
#[tauri::command]
pub async fn translate_status(slot: State<'_, JobSlot<JobStatus>>) -> ApiResult<Option<JobStatus>> {
    Ok(slot.status())
}

#[tauri::command]
pub async fn translate_cancel(slot: State<'_, JobSlot<JobStatus>>) -> ApiResult<()> {
    slot.cancel();
    Ok(())
}

// ---------- 並排比較與套用 ----------

/// 一個服務對選的那幾段交回的譯文。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareColumn {
    pub service: String,
    /// 和要求的段落一一對應；翻不出來的是 `null`。
    pub texts: Vec<Option<String>>,
    /// 整個服務都沒翻成時的原因。
    pub error: Option<ErrorKind>,
}

/// 讓幾個服務（最多 3 個）各翻一次選的段落，並排比較。不寫進譯本：使用者挑了再用 `overlay_apply` 套用。
#[tauri::command]
pub async fn translate_compare(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: State<'_, JobSlot<JobStatus>>,
    paper_id: String,
    material_id: String,
    segment_ids: Vec<String>,
    services: Vec<String>,
) -> ApiResult<Vec<CompareColumn>> {
    if services.is_empty()
        || services.len() > 3
        || segment_ids.is_empty()
        || segment_ids.len() > 200
    {
        return Err(ApiError::new(ErrorKind::InvalidInput, "compare"));
    }
    if slot.busy_with(&paper_id, &material_id) {
        return Err(ApiError::new(ErrorKind::TranslateBusy, "a job is running"));
    }
    let t = state.translate_settings();
    let (doc, title) = read_doc(&state, "translate_compare", &paper_id, &material_id).await?;
    let wanted: HashSet<&String> = segment_ids.iter().collect();
    let segs: Vec<Segment> = job_segments(&doc, |i| wanted.contains(&doc.segments[i].id));
    let params = Params {
        refine: false,
        ..params_for(
            &state,
            &t,
            doc.source_lang.clone(),
            doc.target_lang.clone(),
            title,
        )
    };
    let mut tasks = Vec::new();
    for id in services {
        let app = app.clone();
        let svc = find_service(&t, &id)?;
        let (segs, params, ids) = (segs.clone(), params.clone(), segment_ids.clone());
        tasks.push(tauri::async_runtime::spawn(async move {
            let engine = match engine_for(&app, &svc, &params.target_lang).await {
                Ok(e) => e,
                Err(e) => {
                    return CompareColumn {
                        service: svc.id,
                        texts: vec![None; ids.len()],
                        error: Some(e.kind),
                    };
                }
            };
            let mut got: Vec<(String, String)> = Vec::new();
            let sum = run_job(&engine, &params, segs, &Cancel::new(), |ev| {
                got.extend(
                    ev.items
                        .into_iter()
                        .filter_map(|i| i.result.ok().map(|t| (i.id, t))),
                );
                true
            })
            .await;
            CompareColumn {
                service: svc.id,
                texts: ids
                    .iter()
                    .map(|id| got.iter().find(|(g, _)| g == id).map(|(_, t)| t.clone()))
                    .collect(),
                error: summary_error(&sum),
            }
        }));
    }
    let mut out = Vec::new();
    for task in tasks {
        out.push(
            task.await
                .map_err(|_| ApiError::new(ErrorKind::Io, "compare task"))?,
        );
    }
    Ok(out)
}

fn summary_error(sum: &paperreader_translate::JobSummary) -> Option<ErrorKind> {
    sum.stopped.map(api_kind)
}

/// 收尾重翻要送的段落：原文有這篇已定的術語、譯文卻沒用那個譯法的已翻段落（使用者決定的除外），
/// 接續的另一段一起送；最多全篇段數的一成。
fn final_pass_items(doc: &OverlayFile, terms: &[(String, String)]) -> Vec<FinalPassItem> {
    let segs: Vec<(&str, Option<&str>, bool)> = doc
        .segments
        .iter()
        .map(|g| {
            let done = g.status == STATUS_DONE;
            (
                g.source.as_str(),
                g.target.as_deref().filter(|_| done),
                g.by_user,
            )
        })
        .collect();
    let limit = (doc.segments.len() / 10).max(1);
    let pairs = continuation_pairs(doc);
    let picked: HashSet<usize> = final_pass_targets(&segs, terms, limit, &pairs)
        .into_iter()
        .collect();
    job_segments(doc, |i| picked.contains(&i))
        .into_iter()
        .filter_map(|segment| {
            let g = doc.segments.iter().find(|g| g.id == segment.id)?;
            Some(FinalPassItem {
                current: g.target.clone()?,
                segment,
            })
        })
        .collect()
}

/// 工作最後的狀態與原因：翻譯本身與收尾重翻（有做的話）的取消、停下、寫入失敗都算。
fn final_state(
    job: &paperreader_translate::JobSummary,
    fp: Option<&paperreader_translate::FinalPassSummary>,
) -> (JobState, Option<ErrorKind>) {
    let cancelled = job.cancelled || fp.is_some_and(|f| f.cancelled);
    let sink_failed = job.sink_failed || fp.is_some_and(|f| f.sink_failed);
    let stopped = job.stopped.or(fp.and_then(|f| f.stopped));
    let state = if cancelled {
        JobState::Cancelled
    } else if stopped.is_some() || sink_failed {
        JobState::Stopped
    } else {
        JobState::Finished
    };
    let reason = if sink_failed {
        Some(ErrorKind::Io)
    } else {
        stopped.map(api_kind)
    };
    (state, reason)
}

/// 這一段要不要送翻：只有還沒翻好的（`pending`、`failed`）。已翻的、使用者決定的都不送。
fn needs_translation(g: &OverlaySegment) -> bool {
    g.status != STATUS_DONE
}

/// 一批翻譯結果要寫進譯本的更新。翻出來的寫成已翻、記服務；翻不出來的寫成 `failed`，但這一輪開始時
/// 已翻的段落（`done_before`）不寫，原本的譯文留著。
fn batch_updates(
    items: &[paperreader_translate::BatchItem],
    done_before: &std::collections::HashSet<String>,
    by: &SegmentService,
) -> Vec<SegmentUpdate> {
    items
        .iter()
        .filter_map(|it| match &it.result {
            Err(_) if done_before.contains(&it.id) => None,
            Ok(text) => Some(SegmentUpdate {
                id: it.id.clone(),
                status: STATUS_DONE.to_string(),
                target: Some(text.clone()),
                error: None,
                issues: paperreader_translate::check::issue_codes(&it.issues),
                service: Some(by.clone()),
                by_user: false,
            }),
            Err(kind) => Some(SegmentUpdate {
                id: it.id.clone(),
                status: STATUS_FAILED.to_string(),
                target: None,
                error: Some(format!("{kind:?}")),
                issues: Vec::new(),
                service: Some(by.clone()),
                by_user: false,
            }),
        })
        .collect()
}

/// 記在每一段上的服務：代號、引擎種類、模型（對話模型以外是空字串，同譯本檔頭的 `model`）。
fn segment_service(svc: &Service) -> SegmentService {
    let (model, _) = svc.model_endpoint();
    SegmentService {
        id: svc.id.clone(),
        provider: svc.provider.key().to_string(),
        model: if svc.provider.is_chat_model() {
            model
        } else {
            String::new()
        },
        extra: Default::default(),
    }
}

/// 把一段譯文套用到譯本（並排比較時挑的那一個）。重新跑自動檢查。`service` 是翻出這段的服務的代號：
/// 找得到就記在那一段上；已經不在設定裡時不記。
#[tauri::command]
pub async fn overlay_apply(
    state: State<'_, AppState>,
    slot: State<'_, JobSlot<JobStatus>>,
    paper_id: String,
    material_id: String,
    segment_id: String,
    text: String,
    service: Option<String>,
) -> ApiResult<()> {
    if text.trim().is_empty() || text.chars().count() > 20_000 {
        return Err(ApiError::new(ErrorKind::InvalidInput, "text"));
    }
    if slot.busy_with(&paper_id, &material_id) {
        return Err(ApiError::new(ErrorKind::TranslateBusy, "a job is running"));
    }
    let by = service.and_then(|id| {
        let t = state.translate_settings();
        t.service(&id).map(segment_service)
    });
    state
        .lib("overlay_apply", move |l| {
            let (doc, token) = l.read_overlay(&paper_id, &material_id)?;
            let source = doc
                .segments
                .iter()
                .find(|g| g.id == segment_id)
                .map(|g| g.source.clone())
                .unwrap_or_default();
            let issues =
                paperreader_translate::check::check_segment(&source, &text, &doc.target_lang);
            let update = SegmentUpdate {
                id: segment_id,
                status: STATUS_DONE.to_string(),
                target: Some(text),
                error: None,
                issues: paperreader_translate::check::issue_codes(&issues),
                service: by,
                // 使用者挑的：之後任何整批的翻譯都不蓋掉
                by_user: true,
            };
            l.update_overlay_segments(&paper_id, &material_id, &[update], &token)
                .map(|_| ())
        })
        .await
}

// ---------- 選取文字的快速翻譯 ----------

/// 快速翻譯的快取檔（和設定檔放在一起：應用專屬空間，不進文獻庫、不進備份、不同步）。
pub const QUICK_CACHE_FILE: &str = "quick-cache.json";
/// 快取最多幾筆（多的丟掉最舊的）。
const QUICK_CACHE_MAX: usize = 300;

#[derive(Default, Serialize, Deserialize)]
struct QuickCacheFile {
    #[serde(default)]
    entries: Vec<QuickCacheEntry>,
}

#[derive(Clone, Serialize, Deserialize)]
struct QuickCacheEntry {
    /// 這一篇、這個服務（引擎與模型）、目標語言、提示詞、選取與前後文的雜湊（不存原文）。
    key: String,
    text: String,
    service: String,
}

/// 查過的快速翻譯：記憶體一份（第一次用時從快取檔讀進來），寫入時記憶體與檔案一起改。
#[derive(Default)]
pub struct QuickCache(std::sync::Mutex<Option<Vec<QuickCacheEntry>>>);

impl QuickCache {
    fn get(&self, state: &AppState, key: &str) -> Option<QuickCacheEntry> {
        let mut g = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let entries =
            g.get_or_insert_with(|| state.read_side::<QuickCacheFile>(QUICK_CACHE_FILE).entries);
        entries.iter().find(|e| e.key == key).cloned()
    }

    /// 記下一筆。寫不進檔案只記一行：這次查到的照樣顯示，記憶體裡也留著。
    fn put(&self, state: &AppState, entry: QuickCacheEntry) {
        let add = |list: &mut Vec<QuickCacheEntry>| {
            list.retain(|e| e.key != entry.key);
            list.push(entry.clone());
            if list.len() > QUICK_CACHE_MAX {
                let extra = list.len() - QUICK_CACHE_MAX;
                list.drain(..extra);
            }
        };
        if let Err(e) = state.update_side(QUICK_CACHE_FILE, |f: &mut QuickCacheFile| {
            add(&mut f.entries)
        }) {
            let _ = e.log("quick_cache");
        }
        let mut g = self.0.lock().unwrap_or_else(|p| p.into_inner());
        add(g.get_or_insert_with(Vec::new));
    }
}

/// 快取的鍵：同一篇、同一段文字、同一個服務（含模型）、同一個目標語言與提示詞才算查過。
/// 機器翻譯與離線引擎不收提示詞與前後文，鍵也不算它們。
fn quick_key(
    paper_id: &str,
    svc: &Service,
    target_lang: &str,
    preset: &str,
    input: &QuickInput,
) -> String {
    use sha2::{Digest, Sha256};
    let chat = svc.provider.is_chat_model();
    let (model, endpoint) = svc.model_endpoint();
    let parts = serde_json::json!([
        paper_id,
        svc.id,
        svc.provider.key(),
        if chat { model } else { String::new() },
        endpoint.unwrap_or_default(),
        target_lang,
        if chat { preset } else { "" },
        input.text,
        if chat { input.before.clone() } else { None },
        if chat { input.after.clone() } else { None },
    ]);
    let digest = Sha256::digest(parts.to_string().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// 一次快速翻譯的結果。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickResult {
    pub text: String,
    /// 實際翻的服務（代號）。沒有網路改用離線引擎時是離線引擎那一筆。
    pub service: String,
    pub provider: Provider,
    /// 原本的服務連不上、改用了離線的 Firefox 翻譯。
    pub offline: bool,
    /// 查過的，這次沒有送出。
    pub cached: bool,
    /// 選的是詞（給詞義與在這句裡的意思）。
    pub word: bool,
}

/// 用 `svc` 翻一次（先看快取）。交回譯文與是不是查過的。
#[allow(clippy::too_many_arguments)]
async fn quick_once(
    app: &AppHandle,
    state: &AppState,
    cache: &QuickCache,
    paper_id: &str,
    svc: &Service,
    params: &Params,
    input: &QuickInput,
    paper_terms: &[OverlayTerm],
) -> Result<(String, bool), paperreader_translate::TranslateError> {
    let key = quick_key(
        paper_id,
        svc,
        &params.target_lang,
        &params.instructions,
        input,
    );
    if let Some(hit) = cache.get(state, &key) {
        return Ok((hit.text, true));
    }
    let engine = engine_for_with(app, svc, &params.target_lang, true)
        .await
        .map_err(|e| {
            paperreader_translate::TranslateError::new(translate_kind(e.kind), "engine")
        })?;
    // 送出裝置的服務：連線 5 秒、不重試（引擎已設）；裝置上的離線引擎照舊等 0.4 秒、最多兩次
    let engine = if svc.provider.is_on_device() {
        engine.with_retry(Duration::from_millis(400), 2)
    } else {
        engine
    }
    .with_paper_terms(to_paper_terms(paper_terms));
    let mut meter = UsageMeter::for_job(&format!("quick-{paper_id}"), paper_id, "quick", &svc.id);
    let out = engine.quick(params, input, &Cancel::new()).await;
    meter.flush(state, &engine);
    let text = out?;
    cache.put(
        state,
        QuickCacheEntry {
            key,
            text: text.clone(),
            service: svc.id.clone(),
        },
    );
    Ok((text, false))
}

/// 外殼的錯誤種類換回翻譯套件的（`quick_once` 統一用翻譯套件的錯誤，才分得出「連不上」）。
fn translate_kind(k: ErrorKind) -> paperreader_translate::ErrorKind {
    use paperreader_translate::ErrorKind as T;
    match k {
        ErrorKind::TranslateAuth => T::Auth,
        ErrorKind::TranslateRateLimited => T::RateLimited,
        ErrorKind::TranslateNetwork => T::Network,
        ErrorKind::TranslateServer => T::Server,
        ErrorKind::TranslateBadResponse => T::BadResponse,
        ErrorKind::TranslateLanguage => T::UnsupportedLanguage,
        ErrorKind::TranslateNoModel => T::ModelMissing,
        _ => T::Config,
    }
}

/// 選取文字的快速翻譯：用 `service`（畫面照「快速翻譯用的服務」決定，預設和全文翻譯相同）翻 `text`。
/// 對話模型另收前後各一句（只用來決定詞義）；機器翻譯與離線引擎只收選取的文字。
/// 同一篇、同一段、同一個服務查過的直接交回，不再送出。送出前的詢問由畫面做。
/// 會送出裝置的服務連不上、而這台裝置有離線的 Firefox 翻譯模型時，改用它（卡片上註明）；不會自動改用另一個雲端服務。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn translate_quick(
    app: AppHandle,
    state: State<'_, AppState>,
    cache: State<'_, QuickCache>,
    paper_id: String,
    text: String,
    before: Option<String>,
    after: Option<String>,
    service: String,
) -> ApiResult<QuickResult> {
    let input = QuickInput {
        text,
        before,
        after,
    }
    .normalized()
    .map_err(|_| ApiError::new(ErrorKind::InvalidInput, "selection length"))?;
    let t = state.translate_settings();
    let svc = find_service(&t, &service)?;
    let pid = paper_id.clone();
    let title = state
        .lib("translate_quick", move |l| {
            let (p, _) = l.get_paper(&pid)?;
            Ok(p.bib.title.filter(|t| !t.trim().is_empty()))
        })
        .await?;
    let params = Params {
        instructions: t.quick_preset().text,
        refine: false,
        ..params_for(
            &state,
            &t,
            Some(t.source_lang.clone()).filter(|l| !l.is_empty()),
            t.target_lang.clone(),
            title,
        )
    };
    let word = paperreader_translate::is_word(&input.text);
    // 這篇最新那份同語言譯本照用的術語：快速翻譯和全文譯本的用詞一致
    let terms = latest_overlay_entries(&state, &paper_id, &t.target_lang).await;
    let first = quick_once(
        &app, &state, &cache, &paper_id, &svc, &params, &input, &terms,
    )
    .await;
    let err = match first {
        Ok((text, cached)) => {
            return Ok(QuickResult {
                text,
                service: svc.id.clone(),
                provider: svc.provider,
                offline: false,
                cached,
                word,
            });
        }
        Err(e) => e,
    };
    // 連不上：有下載離線模型就改用它（沒有網路時自動用）
    if err.kind == paperreader_translate::ErrorKind::Network
        && svc.provider.info().leaves_device
        && let Some(local) = offline_service(&app, &t)
        && let Ok((text, cached)) = quick_once(
            &app, &state, &cache, &paper_id, &local, &params, &input, &terms,
        )
        .await
    {
        return Ok(QuickResult {
            text,
            service: local.id.clone(),
            provider: local.provider,
            offline: true,
            cached,
            word,
        });
    }
    Err(ApiError::new(api_kind(err.kind), "quick").log("translate_quick"))
}

/// 這台裝置上可以用的離線翻譯（Firefox 翻譯）：支援目前的目標語言、模型已經下載。服務清單裡有那一筆就用它
/// （名稱照使用者取的），刪掉了也照樣可以用（用引擎的預設）。
fn offline_service(app: &AppHandle, t: &TranslateSettings) -> Option<Service> {
    let pair = ondevice::bergamot_pair(Some("en"), &t.target_lang)?;
    let data = app.path().app_data_dir().ok()?;
    if !ondevice::bergamot_ready(&data, pair) {
        return None;
    }
    Some(
        t.services
            .iter()
            .find(|s| s.provider == Provider::Bergamot)
            .cloned()
            .unwrap_or_else(|| Service::default_for(Provider::Bergamot.key(), Provider::Bergamot)),
    )
}

// ---------- 術語庫 ----------

const GLOSSARY_MAX: usize = 5000;

fn check_terms(terms: &[Term]) -> ApiResult<()> {
    let bad = || ApiError::new(ErrorKind::InvalidInput, "glossary");
    if terms.len() > GLOSSARY_MAX {
        return Err(bad());
    }
    for t in terms {
        if t.source.trim().is_empty()
            || t.target.trim().is_empty()
            || t.source.chars().count() > 200
            || t.target.chars().count() > 200
            || t.note.chars().count() > 500
        {
            return Err(bad());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn glossary_get(state: State<'_, AppState>) -> ApiResult<Vec<Term>> {
    state.run("glossary_get", |s| Ok(s.glossary())).await
}

#[tauri::command]
pub async fn glossary_set(state: State<'_, AppState>, terms: Vec<Term>) -> ApiResult<()> {
    check_terms(&terms)?;
    state
        .run("glossary_set", move |s| s.set_glossary(&terms))
        .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryImport {
    pub cancelled: bool,
    /// 新加的條數。
    pub added: usize,
    /// 原文詞已經有、改成檔案裡的譯法的條數。
    pub updated: usize,
    /// 讀不出原文或譯法而略過的行數。
    pub skipped: usize,
}

/// 從使用者選的三欄 TSV 檔匯入術語：原文詞相同的取代，其餘加在後面。
#[tauri::command]
pub async fn glossary_import(
    app: AppHandle,
    state: State<'_, AppState>,
) -> ApiResult<GlossaryImport> {
    let picked = app.paperreader().pick_any_file().await?;
    let Some(path) = picked.into_iter().next().and_then(|p| p.path) else {
        return Ok(GlossaryImport {
            cancelled: true,
            added: 0,
            updated: 0,
            skipped: 0,
        });
    };
    state
        .run("glossary_import", move |s| {
            let file = std::path::PathBuf::from(&path);
            let bytes =
                std::fs::read(&file).map_err(|e| paperreader_core::error::io_error(&file, e));
            let _ = std::fs::remove_file(&file);
            let text = String::from_utf8(bytes?)
                .map_err(|_| ApiError::new(ErrorKind::Unreadable, "not utf-8"))?;
            let parsed = glossary::parse_tsv(&text);
            if parsed.terms.is_empty() {
                return Err(ApiError::new(ErrorKind::Unreadable, "no terms"));
            }
            let mut list = s.glossary();
            let (mut added, mut updated) = (0, 0);
            for t in parsed.terms {
                if glossary::merge(&mut list, t) {
                    updated += 1;
                } else {
                    added += 1;
                }
            }
            check_terms(&list)?;
            s.set_glossary(&list)?;
            Ok(GlossaryImport {
                cancelled: false,
                added,
                updated,
                skipped: parsed.skipped,
            })
        })
        .await
}

/// 術語庫寫成三欄 TSV（畫面接著用匯出的流程存檔或分享）。
#[tauri::command]
pub async fn glossary_tsv(state: State<'_, AppState>) -> ApiResult<String> {
    state
        .run("glossary_tsv", |s| Ok(glossary::to_tsv(&s.glossary())))
        .await
}

// ---------- 匯出給翻譯軟體 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterchangeFile {
    pub name: String,
    pub text: String,
}

/// 逐段對照寫成 XLIFF 2.1 或 TMX 1.4b（`format`：`xliff`、`tmx`）。狀態只在這裡對到標準的狀態值。
#[tauri::command]
pub async fn overlay_interchange(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
    format: Format,
) -> ApiResult<InterchangeFile> {
    let (doc, title) = read_doc(&state, "overlay_interchange", &paper_id, &material_id).await?;
    let title = title.unwrap_or_else(|| "paper".to_string());
    let text = match format {
        Format::Xliff => interchange::xliff(&doc, &title, &material_id),
        Format::Tmx => interchange::tmx(&doc),
    };
    Ok(InterchangeFile {
        name: interchange::file_name(&title, format),
        text,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn service_ids_of_converted_settings_are_valid_secret_names() {
        // 服務代號就是金鑰的檔名（外掛那邊的規則是 `^[a-z0-9_]{1,40}$`）
        for p in paperreader_translate::Provider::ALL {
            assert!(crate::translate::valid_id(p.key()));
        }
    }

    fn seg(status: &str, modified: Option<&str>) -> super::OverlaySegment {
        super::OverlaySegment {
            id: "g0001".into(),
            page: 0,
            bbox: [1.0, 1.0, 2.0, 2.0],
            kind: String::new(),
            size: None,
            source: "s".into(),
            target: (status == "done").then(|| "t".into()),
            status: status.into(),
            error: None,
            issues: vec![],
            modified: modified.map(String::from),
            modified_by: None,
            service: None,
            by_user: false,
            extra: Default::default(),
        }
    }

    /// 接著翻只送還沒翻好的段落；已翻的（含使用者決定的）不送。
    #[test]
    fn only_unfinished_segments_are_sent() {
        use super::needs_translation;
        assert!(needs_translation(&seg("pending", None)));
        assert!(needs_translation(&seg(
            "failed",
            Some("2020-01-01T09:00:00.000Z")
        )));
        assert!(!needs_translation(&seg("done", None)));
        let mut chosen = seg("done", Some("2020-01-01T07:00:00.000Z"));
        chosen.by_user = true;
        assert!(!needs_translation(&chosen));
    }

    fn ot(source: &str, target: &str, state: &str) -> super::OverlayTerm {
        super::OverlayTerm {
            source: source.into(),
            target: target.into(),
            state: state.into(),
            batches: 2,
            extra: Default::default(),
        }
    }

    /// 術語記在譯文疊加檔裡；疊加檔與引擎之間來回不丟東西，不認得的欄位與狀態留著。
    #[test]
    fn overlay_terms_round_trip_through_the_engine_form() {
        use super::{to_overlay_terms, to_paper_terms};
        let mut first = ot("token", "詞元", "learned");
        first.extra.insert("later".into(), serde_json::json!(1));
        let old = vec![
            first,
            ot("policy", "策略", "user"),
            ot("state", "", "removed"),
            ot("gain", "增益", "candidate"),
            ot("odd", "怪", "from_a_newer_version"),
        ];
        let engine_form = to_paper_terms(&old);
        assert_eq!(engine_form.len(), 4, "an unknown state is not used");
        let back = to_overlay_terms(&engine_form, &old);
        assert_eq!(back, old);
        assert!(paperreader_core::overlay::validate_terms(&back).is_ok());
    }

    /// 引擎學到的術語不管模型怎麼提，寫進疊加檔前都合核心的規則（一個詞一條）：不合就整批寫不進去、付過費的譯文沒存。
    #[test]
    fn learned_terms_always_fit_the_overlay_rules() {
        use paperreader_translate::learn_terms;
        let p = |s: &str, t: &str| (s.to_string(), t.to_string());
        let mut known = Vec::new();
        let mut tally = std::collections::HashMap::new();
        let src = ["The token, the Token and the stretch."];
        let out = ["權杖、符記、詞元、伸展、拉伸"];
        for found in [
            vec![p("token", "權杖"), p("Token", "符記"), p("stretch", "伸展")],
            vec![p("token", "詞元"), p("stretch", "拉伸")],
            vec![p("TOKEN", "詞元"), p("stretch", "伸展")],
        ] {
            learn_terms(&mut known, &mut tally, found, &src, &out, &[]);
            let overlay = super::to_overlay_terms(&known, &[]);
            assert!(
                paperreader_core::overlay::validate_terms(&overlay).is_ok(),
                "{overlay:?}"
            );
        }
    }

    /// 整篇重翻另做一份：只帶使用者的決定，學到的與候選重新來。
    #[test]
    fn a_new_translation_inherits_only_the_users_decisions() {
        let old = vec![
            ot("token", "權杖", "learned"),
            ot("policy", "策略", "user"),
            ot("state", "", "removed"),
            ot("gain", "增益", "candidate"),
        ];
        let kept: Vec<String> = super::inherited_terms(&old)
            .into_iter()
            .map(|t| t.source)
            .collect();
        assert_eq!(kept, vec!["policy", "state"]);
    }

    /// 這一輪翻不出來的段落，原本已翻的不寫（不清掉舊的譯文）；原本沒翻好的照樣記成 failed。
    #[test]
    fn a_failure_does_not_wipe_a_finished_segment() {
        use paperreader_translate::{BatchItem, ErrorKind};
        let by = super::SegmentService {
            id: "svc".into(),
            provider: "deepl".into(),
            model: String::new(),
            extra: Default::default(),
        };
        let items = vec![
            BatchItem {
                id: "g0001".into(),
                result: Err(ErrorKind::BadResponse),
                issues: vec![],
            },
            BatchItem {
                id: "g0002".into(),
                result: Err(ErrorKind::BadResponse),
                issues: vec![],
            },
            BatchItem {
                id: "g0003".into(),
                result: Ok("譯".into()),
                issues: vec![],
            },
        ];
        let done_before: std::collections::HashSet<String> = ["g0001".to_string()].into();
        let ups = super::batch_updates(&items, &done_before, &by);
        let ids: Vec<&str> = ups.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(ids, vec!["g0002", "g0003"], "{ups:?}");
        assert_eq!(ups[0].status, "failed");
        assert_eq!(ups[1].status, "done");
        assert!(ups.iter().all(|u| !u.by_user));
    }

    fn doc(segs: Vec<super::OverlaySegment>, model_pages: Vec<u32>) -> super::OverlayFile {
        serde_json::from_value::<super::OverlayFile>(serde_json::json!({
            "format": 1, "source_material": "m", "provider": "p", "model": "", "target_lang": "zh-TW",
            "segments": []
        }))
        .map(|mut d| {
            d.segments = segs;
            d.layout_model_pages = model_pages;
            d
        })
        .unwrap()
    }

    fn at(
        id: &str,
        page: u32,
        x0: f64,
        kind: &str,
        source: &str,
        status: &str,
    ) -> super::OverlaySegment {
        super::OverlaySegment {
            id: id.into(),
            page,
            bbox: [x0, 100.0, x0 + 200.0, 300.0],
            kind: kind.into(),
            size: Some(10.0),
            source: source.into(),
            ..seg(status, None)
        }
    }

    /// 首頁的第一個標題是篇名；接續只在記下用版面模型分段的頁上認。
    #[test]
    fn the_title_and_continuations_are_marked_on_the_segments() {
        let segs = vec![
            at(
                "g0001",
                0,
                50.0,
                "heading",
                "A Title of the Paper Here",
                "pending",
            ),
            at("g0002", 0, 50.0, "heading", "1 Introduction", "pending"),
            at(
                "g0003",
                0,
                50.0,
                "body",
                "Our intuition is that the robot",
                "done",
            ),
            at(
                "g0004",
                0,
                320.0,
                "body",
                "should act consistently.",
                "pending",
            ),
        ];
        let d = doc(segs.clone(), vec![0]);
        let out = super::job_segments(&d, |_| true);
        assert!(out[0].title && !out[1].title && !out[2].title);
        assert_eq!(out[3].continues.as_deref(), Some("g0003"));
        let rules = super::job_segments(&doc(segs, vec![]), |_| true);
        assert_eq!(rules[3].continues, None);
    }

    /// 收尾重翻挑到接續的一段，另一段一起送。
    #[test]
    fn the_final_pass_sends_both_halves() {
        let mut segs = vec![
            at("g0001", 0, 50.0, "body", "Each token of the", "done"),
            at("g0002", 0, 320.0, "body", "sentence is kept.", "done"),
        ];
        segs[0].target = Some("每個符元的".into());
        segs[1].target = Some("句子都保留。".into());
        // 一成的上限要容得下兩段
        for i in 3..=20 {
            segs.push(at(
                &format!("g{i:04}"),
                1,
                50.0,
                "body",
                "Other text.",
                "done",
            ));
        }
        let items =
            super::final_pass_items(&doc(segs, vec![0]), &[("token".into(), "token".into())]);
        let ids: Vec<&str> = items.iter().map(|i| i.segment.id.as_str()).collect();
        assert_eq!(ids, vec!["g0001", "g0002"]);
    }

    /// 收尾重翻時取消或寫入失敗：最後的狀態照實顯示，不是完成。
    #[test]
    fn the_final_state_reflects_the_final_pass() {
        use crate::translate::JobState;
        use paperreader_translate::{FinalPassSummary, JobSummary};
        let ok = JobSummary::default();
        assert_eq!(super::final_state(&ok, None).0, JobState::Finished);
        let cancelled = FinalPassSummary {
            cancelled: true,
            ..Default::default()
        };
        assert_eq!(
            super::final_state(&ok, Some(&cancelled)).0,
            JobState::Cancelled
        );
        let failed = FinalPassSummary {
            sink_failed: true,
            ..Default::default()
        };
        assert_eq!(
            super::final_state(&ok, Some(&failed)),
            (JobState::Stopped, Some(crate::error::ErrorKind::Io))
        );
    }

    /// 前文是前一段的最後 400 字（接續與一般段落都一樣）。
    #[test]
    fn the_context_is_the_end_of_the_previous_segment() {
        let long = format!("{} the last words.", "x".repeat(1000));
        let segs = vec![
            at("g0001", 0, 50.0, "body", &long, "done"),
            at(
                "g0002",
                1,
                50.0,
                "body",
                "A new paragraph starts here.",
                "pending",
            ),
        ];
        let out = super::job_segments(&doc(segs, vec![]), |i| i == 1);
        let before = out[0].before.as_deref().unwrap();
        assert_eq!(before.chars().count(), 400);
        assert!(before.ends_with("the last words."), "{before}");
    }
}
