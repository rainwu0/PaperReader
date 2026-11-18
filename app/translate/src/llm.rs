//! 對話模型：用現成的多供應商用戶端函式庫 genai，不自己逐一寫各家的請求格式。
//!
//! 使用者明確選了供應商，所以不靠模型名稱推測供應商：用 `ServiceTargetResolver` 指定供應商、網址與金鑰。
//! 測試時把網址指到本機的假伺服器，走的是同一條路。

use crate::error::{ErrorKind, Result, TranslateError};
use crate::prompt::{
    parse_reply, refine_message, refine_prompt, split_terms, system_prompt, user_message,
};
use crate::{EngineConfig, Params, Provider, Segment, TokenUsage};
use genai::ServiceTarget;
use genai::adapter::AdapterKind;
use genai::chat::{ChatMessage, ChatOptions, ChatRequest};
use genai::resolver::{AuthData, Endpoint, ServiceTargetResolver};
use genai::{Client, ModelIden};
use std::sync::Mutex;

pub(crate) struct Llm {
    client: Client,
    model: String,
    /// 服務回報的用量，每收到一個回應就加上（讀不懂而再要一次的、潤飾、重翻、合併都算）。
    usage: Mutex<TokenUsage>,
}

impl Llm {
    pub(crate) fn new(cfg: &EngineConfig, http: reqwest::Client) -> Result<Llm> {
        let adapter = match cfg.provider {
            Provider::Anthropic => AdapterKind::Anthropic,
            Provider::Openai | Provider::OpenaiCompatible => AdapterKind::OpenAI,
            Provider::Gemini => AdapterKind::Gemini,
            _ => unreachable!("not a chat model provider"),
        };
        if cfg.model.trim().is_empty() {
            return Err(TranslateError::new(ErrorKind::Config, "no model name"));
        }
        let endpoint = endpoint_for(cfg)?;
        // 本機的相容端點（Ollama、LM Studio）不需要金鑰，但 genai 要有值才肯送出
        let key = if cfg.api_key.is_empty() {
            "none".to_string()
        } else {
            cfg.api_key.clone()
        };
        let resolver = ServiceTargetResolver::from_resolver_fn(
            move |t: ServiceTarget| -> std::result::Result<ServiceTarget, genai::resolver::Error> {
                Ok(ServiceTarget {
                    endpoint: Endpoint::from_owned(endpoint.clone()),
                    auth: AuthData::from_single(key.clone()),
                    model: ModelIden::new(adapter, t.model.model_name),
                })
            },
        );
        let client = Client::builder()
            .with_reqwest(http)
            .with_service_target_resolver(resolver)
            .build()
            .map_err(|_| TranslateError::new(ErrorKind::Config, "cannot build the client"))?;
        Ok(Llm {
            client,
            model: cfg.model.trim().to_string(),
            usage: Mutex::new(TokenUsage::default()),
        })
    }

    /// 翻譯一批：交回譯文與回覆裡附的術語（`<terms>`）。`terms` 是這一篇已定的譯法。
    pub(crate) async fn translate(
        &self,
        params: &Params,
        batch: &[Segment],
        before: Option<&str>,
        terms: &[(String, String)],
    ) -> Result<(Vec<String>, Vec<(String, String)>)> {
        let reply = self
            .complete(
                system_prompt(params, batch, terms),
                user_message(batch, before),
            )
            .await?;
        let (body, found) = split_terms(&reply);
        Ok((parse_reply(&body, batch)?, found))
    }

    /// 第二輪潤飾：原文與草稿一起送出，交回修過的譯文。
    pub(crate) async fn refine(
        &self,
        params: &Params,
        batch: &[Segment],
        drafts: &[String],
        terms: &[(String, String)],
    ) -> Result<Vec<String>> {
        let reply = self
            .complete(
                refine_prompt(params, batch, terms),
                refine_message(batch, drafts),
            )
            .await?;
        parse_reply(&reply, batch)
    }

    /// 一次完整回覆的原文（格式由呼叫的一方讀）。服務有回報用量就記下。
    pub(crate) async fn complete(&self, system: String, user: String) -> Result<String> {
        let req = ChatRequest::default()
            .with_system(system)
            .append_message(ChatMessage::user(user));
        let opts = ChatOptions::default()
            .with_temperature(0.0)
            .with_max_tokens(8192);
        let res = self
            .client
            .exec_chat(&self.model, req, Some(&opts))
            .await
            .map_err(map_error)?;
        {
            let mut u = self.usage.lock().unwrap_or_else(|p| p.into_inner());
            let n = |v: Option<i32>| v.and_then(|x| u64::try_from(x).ok()).unwrap_or(0);
            u.input += n(res.usage.prompt_tokens);
            u.output += n(res.usage.completion_tokens);
        }
        res.first_text()
            .map(str::to_string)
            .ok_or_else(|| TranslateError::new(ErrorKind::BadResponse, "the reply has no text"))
    }

    /// 到目前為止服務回報的用量。
    pub(crate) fn usage(&self) -> TokenUsage {
        *self.usage.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// 網址（必須以 `/` 結尾，genai 直接接上路徑）。
fn endpoint_for(cfg: &EngineConfig) -> Result<String> {
    let base = match (&cfg.endpoint, cfg.provider) {
        (Some(e), _) if !e.trim().is_empty() => e.trim().to_string(),
        (_, Provider::Anthropic) => "https://api.anthropic.com/v1/".to_string(),
        (_, Provider::Openai) => "https://api.openai.com/v1/".to_string(),
        (_, Provider::Gemini) => "https://generativelanguage.googleapis.com/v1beta/".to_string(),
        _ => return Err(TranslateError::new(ErrorKind::Config, "no service address")),
    };
    if !(base.starts_with("https://") || base.starts_with("http://")) {
        return Err(TranslateError::new(
            ErrorKind::Config,
            "the service address must start with http:// or https://",
        ));
    }
    Ok(if base.ends_with('/') {
        base
    } else {
        format!("{base}/")
    })
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
}

fn map_error(e: genai::Error) -> TranslateError {
    use genai::Error as E;
    match e {
        E::HttpError {
            status,
            body,
            headers,
            ..
        } => TranslateError::from_status(status.as_u16(), &body, retry_after(&headers)),
        E::WebAdapterCall { webc_error, .. } | E::WebModelCall { webc_error, .. } => {
            map_webc(webc_error)
        }
        E::RequiresApiKey { .. } | E::NoAuthResolver { .. } | E::NoAuthData { .. } => {
            TranslateError::new(ErrorKind::Config, "no API key")
        }
        E::NoChatResponse { .. } => {
            TranslateError::new(ErrorKind::BadResponse, "the service returned no answer")
        }
        _ => TranslateError::new(ErrorKind::BadResponse, "unexpected response"),
    }
}

fn map_webc(e: genai::webc::Error) -> TranslateError {
    use genai::webc::Error as W;
    match e {
        W::ResponseFailedStatus {
            status,
            body,
            headers,
        } => TranslateError::from_status(status.as_u16(), &body, retry_after(&headers)),
        W::Reqwest(r) => match r.status() {
            Some(s) => TranslateError::from_status(s.as_u16(), "", None),
            None => TranslateError::network(&r),
        },
        _ => TranslateError::new(ErrorKind::BadResponse, "unreadable response"),
    }
}
