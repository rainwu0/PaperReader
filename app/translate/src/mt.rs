//! 機器翻譯服務：DeepL、Google Cloud Translation（基本版 v2，API 金鑰）。照官方的 REST API。
//! 不接非官方的免費 Google 翻譯介面。
//!
//! 術語：DeepL 用它的術語表功能（v2 `/v2/glossaries`；術語表要指定原文語言）。術語表依內容取名
//! （`PaperReader <雜湊>`），同樣的內容沿用帳號裡已有的那一份，不重複建立。建不起來（例如這個語言組合
//! 不支援術語表）就照常翻、不用術語表。Google 基本版 v2 沒有術語表，不用。

use crate::error::{ErrorKind, Result, TranslateError};
use crate::{EngineConfig, Params, Provider, Segment};
use serde::Deserialize;
use serde_json::json;
use std::sync::Mutex;

pub(crate) struct Mt {
    http: reqwest::Client,
    provider: Provider,
    url: String,
    key: String,
    /// 這次工作用的 DeepL 術語表：（名稱，代號）。代號是 `None` 表示試過但建不起來。
    glossary: Mutex<Option<(String, Option<String>)>>,
}

impl Mt {
    pub(crate) fn new(cfg: &EngineConfig, http: reqwest::Client) -> Result<Mt> {
        if cfg.api_key.is_empty() {
            return Err(TranslateError::new(ErrorKind::Config, "no API key"));
        }
        let url = match (&cfg.endpoint, cfg.provider) {
            (Some(e), _) if !e.trim().is_empty() => e.trim().trim_end_matches('/').to_string(),
            // 免費方案的金鑰以 `:fx` 結尾，用另一個網址
            (_, Provider::Deepl) if cfg.api_key.ends_with(":fx") => {
                "https://api-free.deepl.com".to_string()
            }
            (_, Provider::Deepl) => "https://api.deepl.com".to_string(),
            (_, Provider::GoogleTranslate) => "https://translation.googleapis.com".to_string(),
            _ => unreachable!("not a machine translation provider"),
        };
        Ok(Mt {
            http,
            provider: cfg.provider,
            url,
            key: cfg.api_key.clone(),
            glossary: Mutex::new(None),
        })
    }

    pub(crate) async fn translate(
        &self,
        params: &Params,
        batch: &[Segment],
    ) -> Result<Vec<String>> {
        let texts: Vec<&str> = batch.iter().map(|s| s.text.as_str()).collect();
        // 送給服務的語言代碼照引擎申報的對照表（`engines.rs`）；不支援的語言在 `Engine` 就擋下了
        let target = self
            .provider
            .info()
            .target(&params.target_lang)
            .map(|l| l.engine_code.to_string())
            .ok_or_else(|| TranslateError::new(ErrorKind::UnsupportedLanguage, "target"))?;
        let (req, parse): (reqwest::RequestBuilder, ParseFn) = match self.provider {
            Provider::Deepl => {
                let mut body = json!({
                    "text": texts,
                    "target_lang": target,
                });
                if let Some(s) = params.source_lang.as_deref().filter(|s| !s.is_empty()) {
                    body["source_lang"] = json!(deepl_source(s));
                    if let Some(id) = self.deepl_glossary(params, s, &target).await {
                        body["glossary_id"] = json!(id);
                    }
                }
                (
                    self.http
                        .post(format!("{}/v2/translate", self.url))
                        .header("Authorization", format!("DeepL-Auth-Key {}", self.key))
                        .json(&body),
                    parse_deepl,
                )
            }
            _ => {
                let mut body = json!({
                    "q": texts,
                    "target": target,
                    "format": "text",
                });
                if let Some(s) = params.source_lang.as_deref().filter(|s| !s.is_empty()) {
                    body["source"] = json!(google_code(s));
                }
                (
                    self.http
                        .post(format!("{}/language/translate/v2", self.url))
                        // 金鑰放標頭、不放網址：錯誤訊息與記錄才不會帶出金鑰
                        .header("x-goog-api-key", &self.key)
                        .json(&body),
                    parse_google,
                )
            }
        };
        let resp = req.send().await.map_err(|e| TranslateError::network(&e))?;
        let status = resp.status().as_u16();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok());
        let text = resp.text().await.map_err(|e| TranslateError::network(&e))?;
        if !(200..300).contains(&status) {
            // DeepL 用 456 表示額度用完
            let mut e = TranslateError::from_status(status, &text, retry_after);
            if status == 456 {
                e.kind = ErrorKind::QuotaExceeded;
            }
            return Err(e);
        }
        let out = parse(&text)
            .ok_or_else(|| TranslateError::new(ErrorKind::BadResponse, "unreadable response"))?;
        if out.len() != batch.len() {
            return Err(TranslateError::new(
                ErrorKind::BadResponse,
                format!("expected {} translations, got {}", batch.len(), out.len()),
            ));
        }
        Ok(out)
    }
}

impl Mt {
    /// 這次工作要用的 DeepL 術語表的代號（沒有術語、或建不起來時是 `None`）。
    async fn deepl_glossary(&self, params: &Params, source: &str, target: &str) -> Option<String> {
        if params.glossary.is_empty() {
            return None;
        }
        let src = deepl_source(source).to_ascii_lowercase();
        // 術語表的語言不分地區（`ZH-HANT` 的術語表是 `zh`）
        let tgt = target
            .split(['-', '_'])
            .next()
            .unwrap_or(target)
            .to_ascii_lowercase();
        let entries: String = params
            .glossary
            .iter()
            .map(|(a, b)| {
                let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ").trim().to_string();
                format!("{}\t{}\n", clean(a), clean(b))
            })
            .collect();
        let name = format!(
            "PaperReader {:016x}",
            fnv(&format!("{src}|{tgt}|{entries}"))
        );
        if let Some((n, id)) = self
            .glossary
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            && n == name
        {
            return id;
        }
        let id = self.find_or_create(&name, &src, &tgt, &entries).await;
        *self.glossary.lock().unwrap_or_else(|p| p.into_inner()) = Some((name, id.clone()));
        id
    }

    async fn find_or_create(
        &self,
        name: &str,
        src: &str,
        tgt: &str,
        entries: &str,
    ) -> Option<String> {
        let auth = format!("DeepL-Auth-Key {}", self.key);
        #[derive(Deserialize)]
        struct List {
            glossaries: Vec<Item>,
        }
        #[derive(Deserialize)]
        struct Item {
            glossary_id: String,
            name: String,
            source_lang: String,
            target_lang: String,
        }
        if let Ok(r) = self
            .http
            .get(format!("{}/v2/glossaries", self.url))
            .header("Authorization", &auth)
            .send()
            .await
            && r.status().is_success()
            && let Ok(list) = r.json::<List>().await
            && let Some(g) = list.glossaries.into_iter().find(|g| {
                g.name == name
                    && g.source_lang.eq_ignore_ascii_case(src)
                    && g.target_lang.eq_ignore_ascii_case(tgt)
            })
        {
            return Some(g.glossary_id);
        }
        #[derive(Deserialize)]
        struct Created {
            glossary_id: String,
        }
        let r = self
            .http
            .post(format!("{}/v2/glossaries", self.url))
            .header("Authorization", &auth)
            .json(&json!({
                "name": name,
                "source_lang": src,
                "target_lang": tgt,
                "entries": entries,
                "entries_format": "tsv",
            }))
            .send()
            .await
            .ok()?;
        if !r.status().is_success() {
            return None;
        }
        r.json::<Created>().await.ok().map(|c| c.glossary_id)
    }
}

/// 術語表名稱用的雜湊（FNV-1a，不必是密碼學的）。
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[derive(Deserialize)]
struct DeeplResp {
    translations: Vec<DeeplItem>,
}
#[derive(Deserialize)]
struct DeeplItem {
    text: String,
}

fn parse_deepl(s: &str) -> Option<Vec<String>> {
    let r: DeeplResp = serde_json::from_str(s).ok()?;
    Some(r.translations.into_iter().map(|t| t.text).collect())
}

#[derive(Deserialize)]
struct GoogleResp {
    data: GoogleData,
}
#[derive(Deserialize)]
struct GoogleData {
    translations: Vec<GoogleItem>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleItem {
    translated_text: String,
}

fn parse_google(s: &str) -> Option<Vec<String>> {
    let r: GoogleResp = serde_json::from_str(s).ok()?;
    Some(
        r.data
            .translations
            .into_iter()
            .map(|t| t.translated_text)
            .collect(),
    )
}

/// DeepL 的目標語言代碼（官方文件：繁體中文 `ZH-HANT`、簡體 `ZH-HANS`，英文要分 `EN-US`／`EN-GB`）。
pub fn deepl_target(code: &str) -> String {
    match code.to_ascii_lowercase().as_str() {
        "zh-tw" | "zh-hant" | "zh-hk" => "ZH-HANT".to_string(),
        "zh-cn" | "zh-hans" | "zh" => "ZH-HANS".to_string(),
        "en" | "en-us" => "EN-US".to_string(),
        "en-gb" => "EN-GB".to_string(),
        "pt" | "pt-br" => "PT-BR".to_string(),
        "pt-pt" => "PT-PT".to_string(),
        other => other.to_ascii_uppercase(),
    }
}

/// 來源語言只收不含地區的代碼。
fn deepl_source(code: &str) -> String {
    code.split(['-', '_'])
        .next()
        .unwrap_or(code)
        .to_ascii_uppercase()
}

/// Google 的語言代碼：繁體中文 `zh-TW`、簡體 `zh-CN`，其餘照原樣。
pub fn google_code(code: &str) -> String {
    match code.to_ascii_lowercase().as_str() {
        "zh-tw" | "zh-hant" | "zh-hk" => "zh-TW".to_string(),
        "zh-cn" | "zh-hans" | "zh" => "zh-CN".to_string(),
        other => other.to_string(),
    }
}

type ParseFn = fn(&str) -> Option<Vec<String>>;
