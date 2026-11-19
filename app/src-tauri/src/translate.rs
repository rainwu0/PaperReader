//! 「產生」的設定（存在 `settings.json`）與正在進行的翻譯工作的狀態。
//!
//! 設定的形狀學 LiteLLM 的具名實例：一張「翻譯服務」清單，
//! 每一筆有代號、名稱、引擎種類、模型、網址、速率、啟用開關；另外記主要服務與依序的備用服務、
//! 提示詞預設組、第二輪潤飾。金鑰另外存，每個服務一把，名稱是服務的代號：在 Android 上由外掛用
//! Android Keystore 加密後放在應用私有的空間，不進設定檔、不進文獻庫、不進備份檔。
//!
//! 舊版的設定（一個 `provider` 加上依供應商記的 `profiles`）讀進來時自動轉成服務清單；轉出來的服務代號
//! 就是原本的供應商名稱，所以原本存的金鑰照樣對得上。

use crate::error::{ApiError, ApiResult, ErrorKind};
use paperreader_core::digest::{DigestSection, validate_sections};
use paperreader_translate::{
    BuiltinPreset, Cancel, DEFAULT_DIGEST_PRESET, DEFAULT_QUICK_PRESET, PresetKind, Provider,
    TokenUsage, builtin_preset, check_template,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// 服務的單價（美元／百萬詞元），使用者在服務的設定裡填；只用來粗估花費。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Prices {
    pub input: f64,
    pub output: f64,
}

/// 一個翻譯服務（具名的實例）。同一種引擎可以有多筆。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Service {
    /// 代號：設定檔裡互相參照、金鑰的名稱都用它。小寫英數與底線，最長 40。
    pub id: String,
    /// 使用者取的名稱；空白時畫面顯示引擎的名稱。
    #[serde(default)]
    pub name: String,
    pub provider: Provider,
    /// 對話模型的名稱；空白用引擎申報的預設。
    #[serde(default)]
    pub model: String,
    /// 服務網址；空白用官方預設。「OpenAI 相容端點」必填。
    #[serde(default)]
    pub endpoint: String,
    /// 只有啟用的出現在翻譯時的選單。
    #[serde(default = "yes")]
    pub enabled: bool,
    /// 每分鐘最多幾次請求；0 是不限。
    #[serde(default)]
    pub rpm: u32,
    /// 單價；沒填是 `None`（只顯示詞元數）。
    #[serde(default)]
    pub prices: Option<Prices>,
}

fn yes() -> bool {
    true
}

impl Service {
    /// 一種引擎的預設服務（還原預設時用）。
    pub fn default_for(id: &str, provider: Provider) -> Service {
        Service {
            id: id.to_string(),
            name: String::new(),
            provider,
            model: String::new(),
            endpoint: String::new(),
            enabled: true,
            rpm: 0,
            prices: None,
        }
    }

    /// 實際用的模型（空白用預設）與網址（空白是官方預設）。
    pub fn model_endpoint(&self) -> (String, Option<String>) {
        let model = if self.model.trim().is_empty() {
            self.provider.info().default_model.to_string()
        } else {
            self.model.trim().to_string()
        };
        let endpoint = Some(self.endpoint.trim().to_string()).filter(|e| !e.is_empty());
        (model, endpoint)
    }
}

/// 重點筆記預設組的一個區段：代號（回覆 JSON 的鍵，也寫進每一點）、名稱（`## ` 標題；空白時用內建
/// 區段的名稱，跟著輸出的語言）、要寫什麼（給模型的說明）、最多幾點。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSection {
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub about: String,
    pub max: u32,
}

/// 一個區段最多幾點；說明最多幾個字。
pub const MAX_SECTION_POINTS: u32 = 20;
const MAX_SECTION_ABOUT: usize = 1000;

/// 提示詞預設組（翻譯與重點筆記共用一套）。設定檔只存使用者的：自訂的，以及改過的內建組
/// （代號與內建的相同，「還原預設」就是拿掉它）。內建的在產生引擎（`app/translate`）裡。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptPreset {
    pub id: String,
    /// 自訂的名稱；內建的是空的（名稱由畫面的文字表提供）。
    #[serde(default)]
    pub name: String,
    /// 這一版之前存的都是翻譯用的。
    #[serde(default)]
    pub kind: PresetKind,
    pub text: String,
    /// 重點筆記的區段；翻譯的是空的。
    #[serde(default)]
    pub sections: Vec<PromptSection>,
}

impl PromptPreset {
    /// 內建組原本的內容。
    pub fn from_builtin(b: &BuiltinPreset) -> PromptPreset {
        PromptPreset {
            id: b.id.to_string(),
            name: String::new(),
            kind: b.kind,
            text: b.text.to_string(),
            sections: b
                .sections
                .iter()
                .map(|s| PromptSection {
                    id: s.id.to_string(),
                    label: String::new(),
                    about: s.about.to_string(),
                    max: u32::try_from(s.max).unwrap_or(MAX_SECTION_POINTS),
                })
                .collect(),
        }
    }

    /// 給產生引擎的區段（代號、要寫什麼、最多幾點）。
    pub fn section_specs(&self) -> Vec<paperreader_translate::SectionSpec> {
        self.sections
            .iter()
            .map(|s| paperreader_translate::SectionSpec {
                id: s.id.clone(),
                about: s.about.clone(),
                max: s.max as usize,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawSettings")]
pub struct TranslateSettings {
    pub services: Vec<Service>,
    /// 主要服務的代號。
    pub primary: String,
    /// 依序的備用服務（代號）。主要服務失敗時，先問使用者才換。
    pub fallbacks: Vec<String>,
    /// 原文語言代碼；空白讓服務自己判斷。
    pub source_lang: String,
    /// 目標語言代碼，預設繁體中文。
    pub target_lang: String,
    /// 送到雲端前不再詢問（沒勾就一律詢問）。換服務不在此限。
    pub skip_confirm: bool,
    /// 翻譯選用的提示詞預設組（內建或自訂的代號）。
    pub prompt: String,
    /// 重點筆記選用的預設組（產生面板記住上次選的）。
    pub digest_prompt: String,
    /// 自訂的與改過的內建預設組（兩種用途）。
    pub prompts: Vec<PromptPreset>,
    /// 第二輪潤飾。
    pub refine: bool,
    /// 收尾重翻：整篇翻完後，把沒用到已定術語譯法的段落再翻一次（預設開著）。
    pub final_pass: bool,
    /// 離線翻譯的語言模型只在不計流量的網路（Wi-Fi）下載。
    pub wifi_only: bool,
    /// 上次用來翻的服務（代號）：翻譯面板照它選好。不在服務清單裡的讀檔時丟掉。
    pub last_service: Option<String>,
    /// 上次用來產生重點筆記的服務（代號）：重點筆記面板照它選好。和翻譯的分開記：
    /// 重點筆記只能用對話模型類的服務。不在服務清單裡的讀檔時丟掉。
    pub last_digest_service: Option<String>,
    /// 快速翻譯用的服務：`None` 是「跟全文翻譯相同」（翻譯面板會先選的那一個，由畫面決定）。
    /// 卡片角落的小下拉與設定頁改的都是這一個。不在服務清單裡的讀檔時丟掉；服務刪掉時改回「跟全文翻譯相同」。
    pub quick_service: Option<String>,
    /// 快速翻譯選用的提示詞預設組（用途 `quick`）。
    pub quick_prompt: String,
    /// 快速翻譯「送出前詢問」已經同意的服務與主機（`服務代號@主機`）。
    /// 重新詢問是把它清空。
    pub quick_consent: Vec<String>,
}

/// 讀設定檔用：新舊兩種格式的欄位都收。
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSettings {
    services: Option<Vec<Service>>,
    primary: Option<String>,
    fallbacks: Vec<String>,
    source_lang: String,
    target_lang: Option<String>,
    skip_confirm: bool,
    prompt: Option<String>,
    digest_prompt: Option<String>,
    prompts: Vec<PromptPreset>,
    refine: bool,
    final_pass: Option<bool>,
    wifi_only: Option<bool>,
    last_service: Option<String>,
    last_digest_service: Option<String>,
    quick_service: Option<String>,
    quick_prompt: Option<String>,
    quick_consent: Vec<String>,
    // 舊版的欄位（`local_engine` 舊版也有：離線引擎現在是服務清單裡的一筆，讀到就略過）
    provider: Option<Provider>,
    profiles: BTreeMap<String, OldProfile>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OldProfile {
    model: String,
    endpoint: String,
}

/// 第一次使用時的服務清單：每種引擎一筆，代號就是引擎的名稱。Claude 與 Firefox 翻譯預設啟用。
fn default_services() -> Vec<Service> {
    Provider::ALL
        .iter()
        .map(|&p| Service {
            enabled: matches!(p, Provider::Anthropic | Provider::Bergamot),
            ..Service::default_for(p.key(), p)
        })
        .collect()
}

impl From<RawSettings> for TranslateSettings {
    fn from(r: RawSettings) -> Self {
        let (services, primary) = match r.services {
            Some(s) if !s.is_empty() => {
                let primary = r
                    .primary
                    .filter(|p| s.iter().any(|x| &x.id == p))
                    .unwrap_or_else(|| s[0].id.clone());
                (s, primary)
            }
            // 舊格式（或全新）：每種引擎一筆，帶上原本記的模型與網址；原本選的那一個啟用並當主要服務
            _ => {
                let old = r.provider.filter(|p| !p.is_on_device());
                let mut s = default_services();
                for svc in &mut s {
                    if let Some(p) = r.profiles.get(svc.provider.key()) {
                        svc.model = p.model.clone();
                        svc.endpoint = p.endpoint.clone();
                    }
                    if let Some(o) = old {
                        svc.enabled = svc.provider == o || svc.provider.is_on_device();
                    }
                }
                let primary = old.unwrap_or(Provider::Anthropic).key().to_string();
                (s, primary)
            }
        };
        let fallbacks = r
            .fallbacks
            .into_iter()
            .filter(|f| f != &primary && services.iter().any(|s| &s.id == f))
            .collect();
        let known = |l: &String| services.iter().any(|s| &s.id == l);
        let last_service = r.last_service.filter(known);
        let last_digest_service = r.last_digest_service.filter(known);
        let quick_service = r.quick_service.filter(known);
        TranslateSettings {
            services,
            primary,
            fallbacks,
            source_lang: r.source_lang,
            target_lang: r
                .target_lang
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| "zh-TW".to_string()),
            skip_confirm: r.skip_confirm,
            prompt: r
                .prompt
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| "academic".to_string()),
            digest_prompt: r
                .digest_prompt
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| DEFAULT_DIGEST_PRESET.to_string()),
            prompts: r.prompts,
            refine: r.refine,
            final_pass: r.final_pass.unwrap_or(true),
            wifi_only: r.wifi_only.unwrap_or(true),
            last_service,
            last_digest_service,
            quick_service,
            quick_prompt: r
                .quick_prompt
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| DEFAULT_QUICK_PRESET.to_string()),
            quick_consent: r.quick_consent,
        }
    }
}

impl Default for TranslateSettings {
    fn default() -> Self {
        RawSettings::default().into()
    }
}

/// 代號的規則（也是金鑰檔名的規則，外掛那邊同樣檢查）。
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// 新服務或新預設組的代號：前綴加一段隨機字。
pub fn new_id(prefix: &str, taken: impl Fn(&str) -> bool) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(7);
    loop {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let tail: String = (0..6)
            .map(|i| {
                let v = ((seed >> (i * 5 + 13)) & 31) as u8;
                if v < 10 {
                    (b'0' + v) as char
                } else {
                    (b'a' + v - 10) as char
                }
            })
            .collect();
        let id: String = format!("{prefix}_{tail}").chars().take(40).collect();
        if !taken(&id) {
            return id;
        }
    }
}

impl TranslateSettings {
    pub fn service(&self, id: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.id == id)
    }

    /// `failed` 失敗之後依序可以改用的下一個服務：主要服務之後是備用清單，跳過停用的。
    pub fn next_fallback(&self, failed: &str) -> Option<&Service> {
        let chain: Vec<&str> = std::iter::once(self.primary.as_str())
            .chain(self.fallbacks.iter().map(String::as_str))
            .collect();
        let from = chain.iter().position(|c| *c == failed).map_or(0, |i| i + 1);
        chain[from..]
            .iter()
            .filter(|c| **c != failed)
            .filter_map(|c| self.service(c))
            .find(|s| s.enabled)
    }

    /// 一組預設組實際的內容：使用者存的（自訂的、改過的內建組）優先，再來是內建的。不是這種用途的交回 `None`。
    pub fn preset(&self, kind: PresetKind, id: &str) -> Option<PromptPreset> {
        if let Some(p) = self.prompts.iter().find(|p| p.id == id) {
            return (p.kind == kind).then(|| p.clone());
        }
        builtin_preset(id)
            .filter(|b| b.kind == kind)
            .map(PromptPreset::from_builtin)
    }

    /// 選用的翻譯預設組的內容；找不到用內建的「學術論文」。
    pub fn prompt_text(&self) -> String {
        self.preset(PresetKind::Translation, &self.prompt)
            .or_else(|| self.preset(PresetKind::Translation, "academic"))
            .map(|p| p.text)
            .unwrap_or_default()
    }

    /// 選用的重點筆記預設組；找不到用內建的「標準」。
    pub fn digest_preset(&self) -> PromptPreset {
        self.preset(PresetKind::Digest, &self.digest_prompt)
            .or_else(|| self.preset(PresetKind::Digest, DEFAULT_DIGEST_PRESET))
            .expect("the standard digest preset is built in")
    }

    /// 選用的快速翻譯預設組；找不到用內建的「快速翻譯」。
    pub fn quick_preset(&self) -> PromptPreset {
        self.preset(PresetKind::Quick, &self.quick_prompt)
            .or_else(|| self.preset(PresetKind::Quick, DEFAULT_QUICK_PRESET))
            .expect("the quick preset is built in")
    }

    /// 選用的預設組不在了（刪掉了、改了用途）：改回各自的內建預設。
    pub fn fix_selection(&mut self) {
        if self.preset(PresetKind::Quick, &self.quick_prompt).is_none() {
            self.quick_prompt = DEFAULT_QUICK_PRESET.to_string();
        }
        if self.preset(PresetKind::Translation, &self.prompt).is_none() {
            self.prompt = "academic".to_string();
        }
        if self
            .preset(PresetKind::Digest, &self.digest_prompt)
            .is_none()
        {
            self.digest_prompt = DEFAULT_DIGEST_PRESET.to_string();
        }
    }

    /// 存檔前的檢查：長度、代號、參照、提示詞的變數。
    pub fn validate(&self) -> ApiResult<()> {
        let bad = |why: &str| ApiError::new(ErrorKind::InvalidInput, why.to_string());
        let lang_ok = |l: &str| l.chars().count() <= 20;
        if self.target_lang.trim().is_empty() || !lang_ok(&self.target_lang) {
            return Err(bad("target language"));
        }
        if !lang_ok(&self.source_lang) {
            return Err(bad("source language"));
        }
        if self.services.is_empty() || self.services.len() > 40 {
            return Err(bad("services"));
        }
        for (i, s) in self.services.iter().enumerate() {
            if !valid_id(&s.id) || self.services[..i].iter().any(|o| o.id == s.id) {
                return Err(bad("service id"));
            }
            if s.name.chars().count() > 40
                || s.model.chars().count() > 100
                || s.endpoint.chars().count() > 300
                || s.rpm > 6000
            {
                return Err(bad("service too long"));
            }
            let price_ok = |v: f64| v.is_finite() && (0.0..=10_000.0).contains(&v);
            if s.prices
                .is_some_and(|p| !price_ok(p.input) || !price_ok(p.output))
            {
                return Err(bad("service prices"));
            }
        }
        if self.service(&self.primary).is_none() {
            return Err(bad("primary"));
        }
        if self.fallbacks.len() > 5 {
            return Err(bad("fallbacks"));
        }
        for (i, f) in self.fallbacks.iter().enumerate() {
            if f == &self.primary || self.service(f).is_none() || self.fallbacks[..i].contains(f) {
                return Err(bad("fallbacks"));
            }
        }
        if self.prompts.len() > 40 {
            return Err(bad("prompts"));
        }
        for (i, p) in self.prompts.iter().enumerate() {
            if !valid_id(&p.id) || self.prompts[..i].iter().any(|o| o.id == p.id) {
                return Err(bad("prompt id"));
            }
            validate_preset(p)?;
        }
        if self.preset(PresetKind::Translation, &self.prompt).is_none() {
            return Err(bad("prompt"));
        }
        if self
            .preset(PresetKind::Digest, &self.digest_prompt)
            .is_none()
        {
            return Err(bad("digest prompt"));
        }
        if self.preset(PresetKind::Quick, &self.quick_prompt).is_none() {
            return Err(bad("quick prompt"));
        }
        if self
            .quick_service
            .as_deref()
            .is_some_and(|q| self.service(q).is_none())
        {
            return Err(bad("quick service"));
        }
        Ok(())
    }
}

/// 一組預設組合不合規則（存檔前、「看完整提示詞」前都檢查）：改過的內建組用途不能變；自訂的要有名稱；
/// 提示詞與區段的說明只能用白名單裡的變數；翻譯的沒有區段，重點筆記的區段照核心的規則。
pub fn validate_preset(p: &PromptPreset) -> ApiResult<()> {
    let bad = |why: &str| ApiError::new(ErrorKind::InvalidInput, why.to_string());
    match builtin_preset(&p.id) {
        Some(b) if b.kind != p.kind => return Err(bad("prompt kind")),
        Some(_) => {}
        None if p.name.trim().is_empty() || p.name.chars().count() > 40 => {
            return Err(bad("prompt name"));
        }
        None => {}
    }
    if p.text.trim().is_empty() || p.text.chars().count() > 4000 {
        return Err(bad("prompt text"));
    }
    if check_template(&p.text).is_err() {
        return Err(bad("prompt variable"));
    }
    match p.kind {
        PresetKind::Translation | PresetKind::Quick if !p.sections.is_empty() => {
            return Err(bad("prompt sections"));
        }
        PresetKind::Translation | PresetKind::Quick => {}
        PresetKind::Digest => {
            // 代號、名稱、數量照核心同一份規則；名稱空白的內建區段先補上內建的名稱
            if p.sections.is_empty() {
                return Err(bad("prompt sections"));
            }
            for s in &p.sections {
                if s.label.trim().is_empty() && builtin_label(&s.id, UI_LANG).is_none() {
                    return Err(bad("section label"));
                }
            }
            let labelled: Vec<DigestSection> = p
                .sections
                .iter()
                .map(|s| DigestSection {
                    id: s.id.clone(),
                    label: match s.label.trim() {
                        "" => builtin_label(&s.id, UI_LANG)
                            .unwrap_or_default()
                            .to_string(),
                        l => l.to_string(),
                    },
                })
                .collect();
            if let Err(e) = validate_sections(&labelled) {
                let why = e.to_string();
                return Err(bad(if why.contains("label") {
                    "section label"
                } else if why.contains(" id ") {
                    "section id"
                } else {
                    "prompt sections"
                }));
            }
            for s in &p.sections {
                if s.about.trim().is_empty()
                    || s.about.chars().count() > MAX_SECTION_ABOUT
                    || check_template(&s.about).is_err()
                {
                    return Err(bad("section about"));
                }
                if s.max == 0 || s.max > MAX_SECTION_POINTS {
                    return Err(bad("section max"));
                }
            }
        }
    }
    Ok(())
}

/// 畫面的語言（檢查名稱重複時，內建區段的名稱用它）。
const UI_LANG: &str = "zh-TW";

/// 內建五區的標題，跟著輸出的語言（不認得的語言用英文）；照代號查。不是內建區段的代號交回 `None`。
pub fn builtin_label(id: &str, lang: &str) -> Option<&'static str> {
    let table: [(&str, &str); 5] = match lang.to_ascii_lowercase().as_str() {
        "zh-tw" | "zh-hant" | "zh-hk" => [
            ("question", "研究問題"),
            ("method", "方法"),
            ("results", "主要結果"),
            ("limitations", "限制"),
            ("quotes", "值得記下的引句或數字"),
        ],
        "zh-cn" | "zh-hans" | "zh" => [
            ("question", "研究问题"),
            ("method", "方法"),
            ("results", "主要结果"),
            ("limitations", "局限"),
            ("quotes", "值得记下的引句或数字"),
        ],
        "ja" => [
            ("question", "研究課題"),
            ("method", "手法"),
            ("results", "主な結果"),
            ("limitations", "限界"),
            ("quotes", "書き留めておきたい引用・数値"),
        ],
        "ko" => [
            ("question", "연구 문제"),
            ("method", "방법"),
            ("results", "주요 결과"),
            ("limitations", "한계"),
            ("quotes", "기억해 둘 인용과 수치"),
        ],
        "de" => [
            ("question", "Forschungsfrage"),
            ("method", "Methode"),
            ("results", "Wichtigste Ergebnisse"),
            ("limitations", "Grenzen"),
            ("quotes", "Bemerkenswerte Zitate und Zahlen"),
        ],
        "fr" => [
            ("question", "Question de recherche"),
            ("method", "Méthode"),
            ("results", "Résultats principaux"),
            ("limitations", "Limites"),
            ("quotes", "Citations et chiffres à retenir"),
        ],
        "es" => [
            ("question", "Pregunta de investigación"),
            ("method", "Método"),
            ("results", "Resultados principales"),
            ("limitations", "Limitaciones"),
            ("quotes", "Citas y cifras destacables"),
        ],
        "ru" => [
            ("question", "Исследовательский вопрос"),
            ("method", "Метод"),
            ("results", "Основные результаты"),
            ("limitations", "Ограничения"),
            ("quotes", "Цитаты и числа, которые стоит запомнить"),
        ],
        _ => [
            ("question", "Research question"),
            ("method", "Method"),
            ("results", "Main results"),
            ("limitations", "Limitations"),
            ("quotes", "Quotes and numbers worth noting"),
        ],
    };
    table.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)
}

/// 一組預設組的區段在重點筆記檔裡的名稱：使用者取的名稱優先，空白的用內建的（跟著輸出的語言）。
/// 內建的名稱在某個語言和使用者取的撞名時，後面那個接上代號，寫進檔案不會因為重複被拒絕。
pub fn section_labels(p: &PromptPreset, lang: &str) -> Vec<DigestSection> {
    let mut out: Vec<DigestSection> = Vec::new();
    for s in &p.sections {
        let mut label = match s.label.trim() {
            "" => builtin_label(&s.id, lang).unwrap_or(&s.id).to_string(),
            l => l.to_string(),
        };
        if out.iter().any(|o| o.label == label) {
            label = format!("{label}（{}）", s.id);
        }
        out.push(DigestSection {
            id: s.id.clone(),
            label,
        });
    }
    out
}

// ---------- 這台裝置記下的用量（應用專屬空間，不進文獻庫） ----------

/// 用量的檔名（和設定檔放在一起）。每份工作一筆，做的途中一批一批加上去。
pub const USAGE_FILE: &str = "usage.json";
/// 最多記幾份工作的用量（多的丟掉最舊的）。
const MAX_USAGE_RECORDS: usize = 2000;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageLog {
    #[serde(default)]
    pub records: Vec<UsageRecord>,
}

/// 一份工作（翻一次、產生一次重點筆記）服務回報的用量。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageRecord {
    /// 這份工作的代號（一批一批加到同一筆）。
    pub job: String,
    pub paper: String,
    /// `translation` 或 `digest`。
    pub kind: String,
    /// 服務的代號（粗估費用照它的單價）。
    pub service: String,
    pub input: u64,
    pub output: u64,
}

impl UsageLog {
    /// 把這份工作多用的詞元加上去（沒有就新增一筆）。全是 0 的不記。
    pub fn add(&mut self, job: &str, paper: &str, kind: &str, service: &str, d: TokenUsage) {
        if d.input == 0 && d.output == 0 {
            return;
        }
        if let Some(r) = self
            .records
            .iter_mut()
            .find(|r| r.job == job && r.service == service)
        {
            r.input += d.input;
            r.output += d.output;
            return;
        }
        self.records.push(UsageRecord {
            job: job.to_string(),
            paper: paper.to_string(),
            kind: kind.to_string(),
            service: service.to_string(),
            input: d.input,
            output: d.output,
        });
        if self.records.len() > MAX_USAGE_RECORDS {
            let extra = self.records.len() - MAX_USAGE_RECORDS;
            self.records.drain(..extra);
        }
    }

    /// 一篇依服務加總的用量（畫面照各服務的單價粗估費用）。
    pub fn for_paper(&self, paper: &str) -> Vec<ServiceUsage> {
        let mut out: Vec<ServiceUsage> = Vec::new();
        for r in self.records.iter().filter(|r| r.paper == paper) {
            match out.iter_mut().find(|u| u.service == r.service) {
                Some(u) => {
                    u.input += r.input;
                    u.output += r.output;
                }
                None => out.push(ServiceUsage {
                    service: r.service.clone(),
                    input: r.input,
                    output: r.output,
                }),
            }
        }
        out
    }
}

/// 一篇用一個服務的累計用量。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServiceUsage {
    pub service: String,
    pub input: u64,
    pub output: u64,
}

/// 主要服務失敗時，可以改用的下一個服務（畫面先問使用者）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FallbackOffer {
    pub id: String,
    pub name: String,
    pub provider: Provider,
    /// 剩下的段落會送到的主機；離線引擎是空的。
    pub host: String,
}

/// 翻譯工作的狀態：畫面每隔一下來問一次。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    pub paper_id: String,
    pub material_id: String,
    /// 這次用的服務（代號）。
    pub service: String,
    /// 這份譯本全部的段落數。
    pub total: usize,
    /// 已翻好的段落數（含之前翻好的）。
    pub done: usize,
    /// 這一輪翻不出來的段落數。
    pub failed: usize,
    pub state: JobState,
    /// `Stopped` 時的原因（對應 `ErrorKind` 的名稱）。
    pub reason: Option<ErrorKind>,
    /// 因為服務連不上、額度用完、一直被限速而停下時，依序可以改用的下一個服務。
    pub fallback: Option<FallbackOffer>,
    /// 這次服務回報的用量（詞元數；服務沒有回報是 0）。
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    /// 全部處理完（可能有失敗的段落）。
    Finished,
    Cancelled,
    /// 因為錯誤停下來（金鑰不對、連不上……），沒翻的維持待翻，可以接著翻。
    Stopped,
}

/// 背景工作的狀態都有「現在是哪一種」：翻譯與重點筆記各一種狀態，共用一個工作槽（`JobSlot`）。
pub trait HasState {
    fn state(&self) -> JobState;
}

impl HasState for JobStatus {
    fn state(&self) -> JobState {
        self.state
    }
}

/// 一份背景工作：取消用的記號、畫面來問的狀態，加上這種工作接著做要留的東西（重點筆記用；翻譯沒有）。
pub struct Job<S, X = ()> {
    pub cancel: Cancel,
    pub status: S,
    pub extra: X,
}

/// 同一時間最多一份這種背景工作（翻譯與重點筆記各一個槽，互不影響）。
pub struct JobSlot<S, X = ()>(Arc<Mutex<Option<Job<S, X>>>>);

impl<S, X> Clone for JobSlot<S, X> {
    fn clone(&self) -> Self {
        JobSlot(self.0.clone())
    }
}

impl<S, X> Default for JobSlot<S, X> {
    fn default() -> Self {
        JobSlot(Arc::new(Mutex::new(None)))
    }
}

impl<S: Clone + HasState, X> JobSlot<S, X> {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Job<S, X>>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 登記一份新工作。「已經有一份在跑」的檢查與登記在同一把鎖裡；交回原本那一份
    /// （之後的準備失敗時用 `restore` 放回去）。
    pub fn begin(&self, job: Job<S, X>) -> ApiResult<Option<Job<S, X>>> {
        let mut g = self.lock();
        if g.as_ref()
            .is_some_and(|j| j.status.state() == JobState::Running)
        {
            return Err(ApiError::new(ErrorKind::TranslateBusy, "a job is running"));
        }
        Ok(g.replace(job))
    }

    /// 放回 `begin` 交回的那一份。
    pub fn restore(&self, previous: Option<Job<S, X>>) {
        *self.lock() = previous;
    }

    /// 改目前這一份（背景工作做完一批、或結束時）。
    pub fn publish(&self, f: impl FnOnce(&mut Job<S, X>)) {
        if let Some(j) = self.lock().as_mut() {
            f(j);
        }
    }

    /// 讀目前這一份；從沒有過是 `None`。
    pub fn peek<R>(&self, f: impl FnOnce(&Job<S, X>) -> R) -> Option<R> {
        self.lock().as_ref().map(f)
    }

    /// 目前（或最近一次）的狀態；從沒有過就是 `None`。
    pub fn status(&self) -> Option<S> {
        self.peek(|j| j.status.clone())
    }

    pub fn cancel(&self) {
        self.peek(|j| j.cancel.cancel());
    }
}

impl JobSlot<JobStatus> {
    /// 這份譯本是不是正在翻（比較與套用要等它翻完，免得兩邊同時寫）。
    pub fn busy_with(&self, paper_id: &str, material_id: &str) -> bool {
        self.peek(|j| {
            j.status.state == JobState::Running
                && j.status.paper_id == paper_id
                && j.status.material_id == material_id
        })
        .unwrap_or(false)
    }
}

/// 這種錯誤停下來時，可以問使用者要不要換備用服務（金鑰錯誤以外的連不上、額度用完、一直被限速）。
pub fn offers_fallback(k: paperreader_translate::ErrorKind) -> bool {
    use paperreader_translate::ErrorKind as T;
    matches!(
        k,
        T::Network | T::Server | T::RateLimited | T::QuotaExceeded
    )
}

/// 翻譯套件的錯誤種類對應到畫面的錯誤種類。
pub fn api_kind(k: paperreader_translate::ErrorKind) -> ErrorKind {
    use paperreader_translate::ErrorKind as T;
    match k {
        T::Auth => ErrorKind::TranslateAuth,
        T::Config => ErrorKind::TranslateConfig,
        T::RateLimited => ErrorKind::TranslateRateLimited,
        T::Network => ErrorKind::TranslateNetwork,
        T::Server => ErrorKind::TranslateServer,
        T::BadResponse | T::Cancelled => ErrorKind::TranslateBadResponse,
        T::UnsupportedLanguage => ErrorKind::TranslateLanguage,
        T::ModelMissing => ErrorKind::TranslateNoModel,
        T::QuotaExceeded => ErrorKind::TranslateQuota,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_settings_have_one_service_per_engine() {
        let s: TranslateSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.target_lang, "zh-TW");
        assert!(!s.skip_confirm);
        assert_eq!(s.primary, "anthropic");
        assert_eq!(s.services.len(), Provider::ALL.len());
        assert!(s.service("anthropic").unwrap().enabled);
        assert!(s.service("bergamot").unwrap().enabled);
        assert!(!s.service("deepl").unwrap().enabled);
        assert!(s.wifi_only);
        assert!(s.final_pass, "the final pass is on unless turned off");
        assert_eq!(s.prompt, "academic");
        assert!(s.validate().is_ok());
        assert_eq!(s.last_service, None);
    }

    /// 上次用的服務存進設定檔、讀回來；已經不在清單裡的丟掉。
    #[test]
    fn the_last_used_service_survives_a_save_and_a_deleted_one_is_dropped() {
        let mut s: TranslateSettings = serde_json::from_str("{}").unwrap();
        s.last_service = Some("bergamot".into());
        let back: TranslateSettings =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.last_service.as_deref(), Some("bergamot"));
        let mut v = serde_json::to_value(&s).unwrap();
        v["last_service"] = "gone".into();
        let back: TranslateSettings = serde_json::from_value(v).unwrap();
        assert_eq!(back.last_service, None);
    }

    /// 重點筆記上次用的服務和翻譯的分開記：存得回來、刪掉的丟掉，不動到翻譯的。
    #[test]
    fn the_last_digest_service_is_kept_apart_from_the_translation_one() {
        let mut s: TranslateSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.last_digest_service, None);
        s.last_service = Some("bergamot".into());
        s.last_digest_service = Some("anthropic".into());
        let back: TranslateSettings =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.last_service.as_deref(), Some("bergamot"));
        assert_eq!(back.last_digest_service.as_deref(), Some("anthropic"));
        let mut v = serde_json::to_value(&s).unwrap();
        v["last_digest_service"] = "gone".into();
        let back: TranslateSettings = serde_json::from_value(v).unwrap();
        assert_eq!(back.last_digest_service, None);
        assert_eq!(back.last_service.as_deref(), Some("bergamot"));
    }

    /// 快速翻譯用的服務：預設是「跟全文翻譯相同」（`None`）；存得回來、刪掉的服務改回預設；
    /// 指到不存在的服務不收。預設組預設是內建的「快速翻譯」，刪掉選用的那一組改回它。
    #[test]
    fn the_quick_translation_service_defaults_to_the_full_translation_one() {
        let mut s: TranslateSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.quick_service, None);
        assert_eq!(s.quick_prompt, "quick");
        assert_eq!(s.quick_preset().kind, PresetKind::Quick);
        s.quick_service = Some("deepl".into());
        assert!(s.validate().is_ok());
        let back: TranslateSettings =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.quick_service.as_deref(), Some("deepl"));
        let mut v = serde_json::to_value(&s).unwrap();
        v["quick_service"] = "gone".into();
        let back: TranslateSettings = serde_json::from_value(v).unwrap();
        assert_eq!(back.quick_service, None);
        s.quick_service = Some("gone".into());
        assert!(s.validate().is_err());
        s.quick_service = None;
        // 選用的是一組翻譯的預設組：用途不對，改回內建的
        s.quick_prompt = "academic".into();
        assert!(s.validate().is_err());
        s.fix_selection();
        assert_eq!(s.quick_prompt, "quick");
        assert!(s.validate().is_ok());
    }

    /// 快速翻譯的同意：存在設定檔、讀得回來；沒有這一欄的舊設定檔是空清單（會再問）。
    #[test]
    fn quick_consent_is_kept_in_the_settings_file() {
        let mut s: TranslateSettings = serde_json::from_str("{}").unwrap();
        assert!(s.quick_consent.is_empty());
        s.quick_consent = vec!["anthropic@api.anthropic.com".into()];
        let back: TranslateSettings =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.quick_consent, vec!["anthropic@api.anthropic.com"]);
    }

    #[test]
    fn old_settings_are_converted_once_and_keep_their_keys() {
        // 舊版（服務清單以前）的設定檔
        let old = r#"{"provider":"openai_compatible","target_lang":"ja","skip_confirm":true,
            "profiles":{"openai_compatible":{"model":"llama3","endpoint":"http://10.0.2.2:11434/v1/"},
                        "anthropic":{"model":"claude-x","endpoint":""}},
            "local_engine":"bergamot","wifi_only":false}"#;
        let s: TranslateSettings = serde_json::from_str(old).unwrap();
        assert_eq!(
            s.primary, "openai_compatible",
            "the old choice is the primary service"
        );
        let c = s.service("openai_compatible").unwrap();
        assert_eq!(c.provider, Provider::OpenaiCompatible);
        assert!(c.enabled);
        assert_eq!(c.model, "llama3");
        assert_eq!(c.endpoint, "http://10.0.2.2:11434/v1/");
        let a = s.service("anthropic").unwrap();
        assert_eq!(a.model, "claude-x");
        assert!(!a.enabled, "only the chosen service is switched on");
        assert!(s.service("bergamot").unwrap().enabled);
        assert_eq!(s.target_lang, "ja");
        assert!(s.skip_confirm);
        assert!(!s.wifi_only);
        // 代號就是原本的供應商名稱：原本依供應商名稱存的金鑰照樣對得上
        for p in Provider::ALL {
            assert!(s.service(p.key()).is_some());
        }
        // 離線引擎是清單裡的一筆，可以當主要或備用；沒有另一個 `local_engine` 欄位
        let b = s.service("bergamot").unwrap();
        assert_eq!(b.provider, Provider::Bergamot);
        let mut s2 = s.clone();
        s2.primary = "bergamot".into();
        s2.fallbacks = vec!["openai_compatible".into()];
        assert!(s2.validate().is_ok());
        // 存回去是新格式，再讀一次不變
        let again: TranslateSettings =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(again, s);
        let v = serde_json::to_value(&s).unwrap();
        assert!(
            v.get("profiles").is_none()
                && v.get("provider").is_none()
                && v.get("local_engine").is_none()
        );
    }

    #[test]
    fn fallbacks_follow_the_order_and_skip_disabled_services() {
        let mut s = TranslateSettings::default();
        let mut extra = Service::default_for("anthropic_2", Provider::Anthropic);
        extra.enabled = false;
        s.services.push(extra);
        s.service_mut("deepl").enabled = true;
        s.fallbacks = vec!["anthropic_2".into(), "deepl".into(), "bergamot".into()];
        assert!(s.validate().is_ok());
        assert_eq!(s.next_fallback("anthropic").unwrap().id, "deepl");
        assert_eq!(s.next_fallback("deepl").unwrap().id, "bergamot");
        assert!(
            s.next_fallback("bergamot").is_none(),
            "nothing after the last one"
        );
        // 不在順序裡的服務（例如這次手動選了別的）失敗：從頭找
        assert_eq!(s.next_fallback("openai").unwrap().id, "anthropic");
    }

    #[test]
    fn validation_rejects_bad_references_and_prompts() {
        let s = TranslateSettings {
            fallbacks: vec!["anthropic".into()],
            ..Default::default()
        };
        assert!(
            s.validate().is_err(),
            "the primary cannot be its own fallback"
        );
        let s = TranslateSettings {
            primary: "nope".into(),
            ..Default::default()
        };
        assert!(s.validate().is_err());
        let mut s = TranslateSettings::default();
        s.prompts.push(PromptPreset {
            id: "mine".into(),
            name: "我的".into(),
            kind: PresetKind::Translation,
            text: "Translate {{text}}".into(),
            sections: vec![],
        });
        assert!(s.validate().is_err(), "unknown template variable");
        s.prompts[0].text = "Translate into {{to}}.".into();
        s.prompt = "mine".into();
        assert!(s.validate().is_ok());
        assert_eq!(s.prompt_text(), "Translate into {{to}}.");
        let mut s = TranslateSettings::default();
        s.services
            .push(Service::default_for("Bad-Id", Provider::Openai));
        assert!(s.validate().is_err());
    }

    fn digest_copy() -> PromptPreset {
        PromptPreset {
            id: "notes_1".into(),
            name: "我的重點筆記".into(),
            ..PromptPreset::from_builtin(builtin_preset("standard").unwrap())
        }
    }

    #[test]
    fn old_saved_presets_are_translation_presets_and_the_digest_default_is_standard() {
        // 這一版之前的設定檔：預設組沒有用途與區段
        let s: TranslateSettings = serde_json::from_str(
            r#"{"prompt":"mine","prompts":[{"id":"mine","name":"我的","text":"Into {{to}}."}]}"#,
        )
        .unwrap();
        assert_eq!(s.prompts[0].kind, PresetKind::Translation);
        assert_eq!(s.prompt_text(), "Into {{to}}.");
        assert_eq!(s.digest_prompt, "standard");
        assert_eq!(s.digest_preset().sections.len(), 5);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn a_builtin_preset_can_be_edited_and_reset() {
        // 改過的內建組存成同代號的一筆；拿掉就是「還原預設」
        let mut s = TranslateSettings::default();
        let mut edited = PromptPreset::from_builtin(builtin_preset("academic").unwrap());
        edited.text = "My academic rules for {{to}}.".into();
        s.prompts.push(edited);
        assert!(s.validate().is_ok());
        assert_eq!(s.prompt_text(), "My academic rules for {{to}}.");
        // 用途不能改
        s.prompts[0].kind = PresetKind::Digest;
        assert!(s.validate().is_err());
        s.prompts.clear();
        assert!(
            s.prompt_text()
                .starts_with("You are a professional translator of academic papers")
        );
    }

    #[test]
    fn digest_presets_check_their_sections() {
        let mut s = TranslateSettings::default();
        let mut p = digest_copy();
        p.sections.push(PromptSection {
            id: "contradictions".into(),
            label: "原文前後矛盾的地方".into(),
            about: "places where the paper contradicts itself".into(),
            max: 3,
        });
        s.prompts.push(p.clone());
        s.digest_prompt = "notes_1".into();
        assert!(s.validate().is_ok());
        assert_eq!(s.digest_preset().sections.len(), 6);
        // 選了重點筆記的預設組當翻譯用：不收
        let mut wrong = s.clone();
        wrong.prompt = "notes_1".into();
        assert!(wrong.validate().is_err());
        type Edit = fn(&mut PromptPreset);
        let cases: [Edit; 11] = [
            |p| p.sections[5].label = String::new(), // 新的區段要有名稱
            |p| p.sections[5].id = "Bad-Id".into(),
            |p| p.sections[5].id = "question".into(), // 重複
            |p| p.sections[5].about = "{{text}}".into(),
            |p| p.sections[5].max = 0,
            |p| p.sections[5].max = MAX_SECTION_POINTS + 1,
            |p| p.sections[5].label = "a --> b".into(),
            |p| p.sections.clear(),
            |p| p.kind = PresetKind::Translation, // 翻譯的不能有區段
            // 名稱不重複，和內建區段的名稱撞也不行
            |p| p.sections[5].label = "研究問題".into(),
            |p| {
                for i in 0..7 {
                    p.sections.push(PromptSection {
                        id: format!("more_{i}"),
                        label: format!("多的 {i}"),
                        about: "x".into(),
                        max: 1,
                    });
                }
            },
        ];
        for (i, f) in cases.iter().enumerate() {
            let mut q = p.clone();
            f(&mut q);
            assert!(validate_preset(&q).is_err(), "case {i}");
        }
        // 內建區段的名稱可以空白（跟著輸出的語言）
        assert!(p.sections[0].label.is_empty());
        // 原因的代號（設定畫面照它說是哪裡不對）
        let why = |f: Edit| {
            let mut q = p.clone();
            f(&mut q);
            validate_preset(&q).unwrap_err().detail
        };
        assert_eq!(
            why(|p| p.sections[5].label = "研究問題".into()),
            "section label"
        );
        assert_eq!(why(|p| p.sections[5].id = "question".into()), "section id");
        assert_eq!(why(|p| p.sections[5].id = "Bad-Id".into()), "section id");
        assert_eq!(why(|p| p.sections[5].max = 0), "section max");
        assert_eq!(why(|p| p.sections.clear()), "prompt sections");
    }

    #[test]
    fn section_names_follow_the_output_language_and_never_repeat() {
        assert_eq!(builtin_label("question", "zh-TW"), Some("研究問題"));
        assert_eq!(builtin_label("limitations", "zh-CN"), Some("局限"));
        assert_eq!(builtin_label("results", "xx"), Some("Main results"));
        assert_eq!(builtin_label("contradictions", "zh-TW"), None);
        let mut p = digest_copy();
        p.sections[1].label = "做法".into();
        // 日文的內建名稱剛好和使用者取的撞名：接上代號，不讓寫檔失敗
        p.sections.push(PromptSection {
            id: "extra".into(),
            label: "研究課題".into(),
            about: "x".into(),
            max: 2,
        });
        let s = section_labels(&p, "ja");
        assert_eq!(s[0].label, "研究課題");
        assert_eq!(s[1].label, "做法");
        assert_eq!(s[5].label, "研究課題（extra）");
        assert!(validate_sections(&s).is_ok());
    }

    #[test]
    fn a_deleted_selection_falls_back_to_the_builtin() {
        let mut s = TranslateSettings::default();
        s.prompts.push(digest_copy());
        s.digest_prompt = "notes_1".into();
        s.prompts.clear();
        s.fix_selection();
        assert_eq!(s.digest_prompt, "standard");
        assert!(s.validate().is_ok());
    }

    #[test]
    fn usage_adds_up_per_job_and_per_paper() {
        let mut log = UsageLog::default();
        let u = |i, o| TokenUsage {
            input: i,
            output: o,
        };
        log.add("j1", "p1", "translation", "anthropic", u(100, 50));
        log.add("j1", "p1", "translation", "anthropic", u(10, 5));
        log.add("j1", "p1", "translation", "anthropic", u(0, 0));
        log.add("j2", "p1", "digest", "openai", u(7, 3));
        log.add("j3", "p2", "digest", "openai", u(1, 1));
        assert_eq!(log.records.len(), 3);
        let p1 = log.for_paper("p1");
        assert_eq!(
            p1,
            vec![
                ServiceUsage {
                    service: "anthropic".into(),
                    input: 110,
                    output: 55
                },
                ServiceUsage {
                    service: "openai".into(),
                    input: 7,
                    output: 3
                },
            ]
        );
        // 存回去讀得回來
        let back: UsageLog = serde_json::from_str(&serde_json::to_string(&log).unwrap()).unwrap();
        assert_eq!(back, log);
    }

    #[test]
    fn prices_are_checked() {
        let mut s = TranslateSettings::default();
        s.services[0].prices = Some(Prices {
            input: 2.0,
            output: 10.0,
        });
        assert!(s.validate().is_ok());
        s.services[0].prices = Some(Prices {
            input: -1.0,
            output: 10.0,
        });
        assert!(s.validate().is_err());
        s.services[0].prices = Some(Prices {
            input: f64::NAN,
            output: 1.0,
        });
        assert!(s.validate().is_err());
    }

    #[test]
    fn new_ids_are_valid_and_unique() {
        let id = new_id("openai_compatible", |_| false);
        assert!(valid_id(&id), "{id}");
        assert!(id.starts_with("openai_compatible_"));
        let first = new_id("x", |_| false);
        let second = new_id("x", |c| c == first);
        assert_ne!(first, second);
    }

    #[test]
    fn model_falls_back_to_the_default() {
        let mut s = Service::default_for("anthropic", Provider::Anthropic);
        assert_eq!(s.model_endpoint(), ("claude-sonnet-5-5".to_string(), None));
        s.model = " claude-x ".into();
        s.endpoint = " ".into();
        assert_eq!(s.model_endpoint(), ("claude-x".to_string(), None));
    }

    #[test]
    fn only_outages_offer_a_fallback() {
        use paperreader_translate::ErrorKind as T;
        assert!(offers_fallback(T::Network));
        assert!(offers_fallback(T::QuotaExceeded));
        assert!(offers_fallback(T::RateLimited));
        assert!(
            !offers_fallback(T::Auth),
            "a wrong key must be fixed, not skipped"
        );
        assert!(!offers_fallback(T::Config));
    }

    impl TranslateSettings {
        fn service_mut(&mut self, id: &str) -> &mut Service {
            self.services.iter_mut().find(|s| s.id == id).unwrap()
        }
    }
}
