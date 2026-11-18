//! 引擎申報：每種引擎自己說明它要什麼、能做什麼。
//!
//! 主程式（外殼、畫面）照申報行事，不再各自寫一份 `match` 或常數表：要不要金鑰、一批多大、
//! 支援哪些目標語言（以及送給引擎時用的代碼）、文字會不會送出裝置
//! （送出前的詢問照它判斷）、有沒有要下載的模型。做法參考 Bob、kiss-translator 的
//! 「引擎申報能力」，程式自己寫。

use crate::Provider;
use serde::Serialize;

/// 引擎的種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    /// 對話模型（照提示詞翻，可以用術語與前後文）。
    Chat,
    /// 機器翻譯服務。
    Mt,
    /// 在這台裝置上翻（離線）。
    OnDevice,
}

/// 一個設定欄位要不要填。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Need {
    No,
    Optional,
    Required,
}

/// 支援的一種目標語言。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LangSupport {
    /// 內部用的 BCP-47 代碼（設定檔、譯本檔都用它），例如 `zh-TW`。
    pub code: &'static str,
    /// 送給引擎時用的代碼，例如 DeepL 的 `ZH-HANT`、Firefox 翻譯的 `zh-Hant`。
    pub engine_code: &'static str,
}

const fn lang(code: &'static str, engine_code: &'static str) -> LangSupport {
    LangSupport { code, engine_code }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub provider: Provider,
    pub kind: EngineKind,
    pub key: Need,
    pub model: Need,
    pub endpoint: Need,
    /// 沒填模型時用的預設（不用模型的引擎是空的）。
    pub default_model: &'static str,
    /// 官方主機（沒填網址時文字送到這裡）；送不出裝置或一定要自己填網址的是空的。
    pub official_host: &'static str,
    /// 論文的文字會不會離開這台裝置。會的話送出前要詢問。
    pub leaves_device: bool,
    /// 一批最多幾段。
    pub max_segments: usize,
    /// 一批最多幾個字元。
    pub max_chars: usize,
    pub glossary: bool,
    pub prompt: bool,
    /// 第一次用之前要下載語言模型。
    pub downloads_models: bool,
    /// 支援的目標語言。原文目前一律是英文或讓服務自己判斷。
    pub targets: &'static [LangSupport],
}

/// 對話模型：任何語言都行，送出的是給模型看的語言名稱（`prompt::language_name`）。
const CHAT_TARGETS: &[LangSupport] = &[
    lang("zh-TW", "Traditional Chinese (繁體中文)"),
    lang("zh-CN", "Simplified Chinese (简体中文)"),
    lang("en", "English"),
    lang("ja", "Japanese"),
    lang("ko", "Korean"),
    lang("de", "German"),
    lang("fr", "French"),
    lang("es", "Spanish"),
    lang("ru", "Russian"),
];
/// DeepL 的目標語言代碼（官方文件：繁中 `ZH-HANT`、簡中 `ZH-HANS`、英文要分 `EN-US`／`EN-GB`）。
const DEEPL_TARGETS: &[LangSupport] = &[
    lang("zh-TW", "ZH-HANT"),
    lang("zh-CN", "ZH-HANS"),
    lang("en", "EN-US"),
    lang("ja", "JA"),
    lang("ko", "KO"),
    lang("de", "DE"),
    lang("fr", "FR"),
    lang("es", "ES"),
    lang("ru", "RU"),
];
const GOOGLE_TARGETS: &[LangSupport] = &[
    lang("zh-TW", "zh-TW"),
    lang("zh-CN", "zh-CN"),
    lang("en", "en"),
    lang("ja", "ja"),
    lang("ko", "ko"),
    lang("de", "de"),
    lang("fr", "fr"),
    lang("es", "es"),
    lang("ru", "ru"),
];
/// Firefox 翻譯：這次只接了英→繁中、英→簡中兩個模型（`src-tauri/src/ondevice.rs` 的目錄）。
const BERGAMOT_TARGETS: &[LangSupport] = &[lang("zh-TW", "zh-Hant"), lang("zh-CN", "zh-Hans")];

impl Provider {
    pub const ALL: [Provider; 7] = [
        Provider::Anthropic,
        Provider::Openai,
        Provider::Gemini,
        Provider::OpenaiCompatible,
        Provider::Deepl,
        Provider::GoogleTranslate,
        Provider::Bergamot,
    ];

    /// 這種引擎的申報。
    pub fn info(self) -> EngineInfo {
        use Need::*;
        let chat = |default_model, official_host, key| EngineInfo {
            provider: self,
            kind: EngineKind::Chat,
            key,
            model: Optional,
            endpoint: Optional,
            default_model,
            official_host,
            leaves_device: true,
            max_segments: 10,
            max_chars: 4_000,
            glossary: true,
            prompt: true,
            downloads_models: false,
            targets: CHAT_TARGETS,
        };
        let mt = |official_host, max_segments, targets| EngineInfo {
            provider: self,
            kind: EngineKind::Mt,
            key: Required,
            model: No,
            endpoint: Optional,
            default_model: "",
            official_host,
            leaves_device: true,
            max_segments,
            max_chars: 20_000,
            glossary: false,
            prompt: false,
            downloads_models: false,
            targets,
        };
        // 裝置上的引擎一段一段翻，批次只決定多久寫一次檔、多久更新一次進度
        let device = |targets| EngineInfo {
            provider: self,
            kind: EngineKind::OnDevice,
            key: No,
            model: No,
            endpoint: No,
            default_model: "",
            official_host: "",
            leaves_device: false,
            max_segments: 8,
            max_chars: 6_000,
            glossary: false,
            prompt: false,
            downloads_models: true,
            targets,
        };
        match self {
            Provider::Anthropic => chat("claude-sonnet-5-5", "api.anthropic.com", Required),
            Provider::Openai => chat("gpt-4o-mini", "api.openai.com", Required),
            Provider::Gemini => chat(
                "gemini-2.5-flash",
                "generativelanguage.googleapis.com",
                Required,
            ),
            Provider::OpenaiCompatible => EngineInfo {
                model: Required,
                endpoint: Required,
                ..chat("", "", Optional)
            },
            // DeepL 有術語表功能（要指定原文語言才用得上，`mt.rs`）
            Provider::Deepl => EngineInfo {
                glossary: true,
                ..mt("api.deepl.com", 40, DEEPL_TARGETS)
            },
            Provider::GoogleTranslate => mt("translation.googleapis.com", 60, GOOGLE_TARGETS),
            Provider::Bergamot => device(BERGAMOT_TARGETS),
        }
    }

    /// 設定檔、金鑰檔、譯本檔用的名稱（與 serde 的名稱相同）。
    pub fn key(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::Openai => "openai",
            Provider::Gemini => "gemini",
            Provider::OpenaiCompatible => "openai_compatible",
            Provider::Deepl => "deepl",
            Provider::GoogleTranslate => "google_translate",
            Provider::Bergamot => "bergamot",
        }
    }
}

impl EngineInfo {
    /// 這個目標語言（BCP-47，不分大小寫）怎麼送給引擎；不支援交回 `None`。
    pub fn target(&self, code: &str) -> Option<&'static LangSupport> {
        self.targets
            .iter()
            .find(|l| l.code.eq_ignore_ascii_case(code))
    }
}

/// 全部引擎的申報（畫面的引擎清單）。
pub fn engines() -> Vec<EngineInfo> {
    Provider::ALL.iter().map(|p| p.info()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_declares_itself_consistently() {
        for p in Provider::ALL {
            let i = p.info();
            assert_eq!(i.provider, p);
            assert_eq!(serde_json::to_value(p).unwrap(), serde_json::json!(p.key()));
            assert!(i.max_segments > 0 && i.max_chars > 0);
            assert!(!i.targets.is_empty(), "{p:?} supports no language");
            // 不送出裝置的就是裝置上的引擎，而且不用金鑰
            assert_eq!(!i.leaves_device, i.kind == EngineKind::OnDevice);
            if !i.leaves_device {
                assert_eq!(i.key, Need::No);
                assert!(i.downloads_models);
            }
            // 會送出裝置又沒有官方主機的，一定要自己填網址
            if i.leaves_device && i.official_host.is_empty() {
                assert_eq!(i.endpoint, Need::Required);
            }
        }
    }

    #[test]
    fn language_tables_map_to_engine_codes() {
        let d = Provider::Deepl.info();
        assert_eq!(d.target("zh-tw").unwrap().engine_code, "ZH-HANT");
        let b = Provider::Bergamot.info();
        // 繁中是 Firefox 翻譯的專用模型（en→zh-Hant），不是簡體再轉
        assert_eq!(b.target("zh-TW").unwrap().engine_code, "zh-Hant");
        assert!(
            b.target("en").is_none(),
            "Firefox Translations here translates from English"
        );
        assert!(Provider::Bergamot.info().target("ja").is_none());
        assert_eq!(
            Provider::Bergamot
                .info()
                .target("zh-CN")
                .unwrap()
                .engine_code,
            "zh-Hans"
        );
    }
}
