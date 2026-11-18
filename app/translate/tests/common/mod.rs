//! 測試用的假伺服器：在本機一個隨機的埠聽，依路徑回假的各家服務回應。不連真的服務。

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone)]
pub struct Req {
    pub method: String,
    pub path: String,
    /// 標頭名稱一律小寫。
    pub headers: HashMap<String, String>,
    pub body: String,
}

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Resp {
    pub fn json(body: impl Into<String>) -> Resp {
        Resp {
            status: 200,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body.into(),
        }
    }
    pub fn status(status: u16, body: &str) -> Resp {
        Resp {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body.to_string(),
        }
    }
}

pub struct Server {
    pub base: String,
    pub requests: Arc<Mutex<Vec<Req>>>,
}

impl Server {
    pub fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
    pub fn all(&self) -> Vec<Req> {
        self.requests.lock().unwrap().clone()
    }

    pub fn last(&self) -> Req {
        self.requests
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a request")
    }
}

pub async fn serve<F>(handler: F) -> Server
where
    F: Fn(&Req, usize) -> Resp + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests: Arc<Mutex<Vec<Req>>> = Arc::default();
    let log = requests.clone();
    let handler = Arc::new(handler);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let log = log.clone();
            let handler = handler.clone();
            tokio::spawn(async move {
                let Some(req) = read_request(&mut sock).await else {
                    return;
                };
                let n = {
                    let mut g = log.lock().unwrap();
                    g.push(req.clone());
                    g.len()
                };
                let resp = handler(&req, n);
                let mut out = format!("HTTP/1.1 {} X\r\n", resp.status);
                for (k, v) in &resp.headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str(&format!(
                    "content-length: {}\r\nconnection: close\r\n\r\n",
                    resp.body.len()
                ));
                out.push_str(&resp.body);
                let _ = sock.write_all(out.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    Server {
        base: format!("http://127.0.0.1:{port}"),
        requests,
    }
}

async fn read_request(sock: &mut tokio::net::TcpStream) -> Option<Req> {
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let first = lines.next()?;
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut headers = HashMap::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    while buf.len() < head_end + len {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
    Some(Req {
        method,
        path,
        headers,
        body,
    })
}

/// 從送出的內容裡取出各段（`<seg id="x">…</seg>`），回「譯：…」包同樣的標籤。
pub fn fake_translate(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("<seg id=\"") {
        rest = &rest[i + 9..];
        let Some(q) = rest.find("\">") else { break };
        let id = &rest[..q];
        let body_start = q + 2;
        let Some(end) = rest[body_start..].find("</seg>") else {
            break;
        };
        let src = &rest[body_start..body_start + end];
        out.push_str(&format!("<seg id=\"{id}\">譯：{src}</seg>\n"));
        rest = &rest[body_start + end..];
    }
    out
}

/// 找出請求裡所有的使用者文字（JSON 字串值裡含 `<seg` 的那一個），其餘結構不管。
pub fn user_text(body: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let mut found = String::new();
    fn walk(v: &serde_json::Value, found: &mut String) {
        match v {
            serde_json::Value::String(s)
                if s.contains("<seg id=") && !s.contains("Rules that always apply") =>
            {
                found.push_str(s)
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, found)),
            serde_json::Value::Object(o) => o.values().for_each(|x| walk(x, found)),
            _ => {}
        }
    }
    walk(&v, &mut found);
    found
}

pub fn openai_reply(text: &str) -> Resp {
    Resp::json(
        serde_json::json!({
            "id": "chatcmpl-1", "object": "chat.completion", "created": 1, "model": "m",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        })
        .to_string(),
    )
}

pub fn anthropic_reply(text: &str) -> Resp {
    Resp::json(
        serde_json::json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "m",
            "content": [{"type": "text", "text": text}],
            "stop_reason": "end_turn", "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        })
        .to_string(),
    )
}

pub fn gemini_reply(text: &str) -> Resp {
    Resp::json(
        serde_json::json!({
            "candidates": [{"content": {"role": "model", "parts": [{"text": text}]}, "finishReason": "STOP", "index": 0}],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 2}
        })
        .to_string(),
    )
}
