//! 各家引擎對假伺服器的行為：請求長什麼樣、回應怎麼讀、錯誤怎麼分、重試與取消。

mod common;

use common::*;
use paperreader_translate::*;
use std::time::Duration;

fn params() -> Params {
    Params {
        source_lang: Some("en".into()),
        target_lang: "zh-TW".into(),
        ..Params::default()
    }
}

fn seg(id: &str, text: &str) -> Segment {
    Segment {
        id: id.into(),
        text: text.into(),
        before: None,
    }
}

fn cfg(provider: Provider, base: &str, model: &str, key: &str) -> EngineConfig {
    EngineConfig {
        provider,
        model: model.into(),
        endpoint: Some(base.into()),
        api_key: key.into(),
    }
}

fn fast(e: Engine) -> Engine {
    e.with_retry(Duration::from_millis(1), 3)
}

#[tokio::test]
async fn openai_compatible_request_and_reply() {
    let srv = serve(|req, _| openai_reply(&fake_translate(&user_text(&req.body)))).await;
    let e = Engine::new(&cfg(
        Provider::OpenaiCompatible,
        &format!("{}/v1", srv.base),
        "gpt-x",
        "sk-test",
    ))
    .unwrap();
    let out = e
        .translate_batch(
            &params(),
            &[seg("a", "Hello [3] $x^2$"), seg("b", "World")],
            &Cancel::new(),
        )
        .await
        .unwrap();
    assert_eq!(out, vec!["譯：Hello [3] $x^2$", "譯：World"]);
    let r = srv.last();
    assert_eq!(r.path, "/v1/chat/completions");
    assert_eq!(r.headers["authorization"], "Bearer sk-test");
    let body: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(body["model"], "gpt-x");
    assert!(r.body.contains("Traditional Chinese"));
    assert!(r.body.contains("citation markers"));
}

#[tokio::test]
async fn compatible_endpoint_needs_no_key() {
    let srv = serve(|req, _| openai_reply(&fake_translate(&user_text(&req.body)))).await;
    let e = Engine::new(&cfg(
        Provider::OpenaiCompatible,
        &format!("{}/v1/", srv.base),
        "llama3",
        "",
    ))
    .unwrap();
    let out = e
        .translate_batch(&params(), &[seg("1", "Hi")], &Cancel::new())
        .await
        .unwrap();
    assert_eq!(out, vec!["譯：Hi"]);
}

#[tokio::test]
async fn anthropic_request_and_reply() {
    let srv = serve(|req, _| anthropic_reply(&fake_translate(&user_text(&req.body)))).await;
    let e = Engine::new(&cfg(
        Provider::Anthropic,
        &format!("{}/v1", srv.base),
        "claude-test",
        "sk-ant-test",
    ))
    .unwrap();
    let out = e
        .translate_batch(&params(), &[seg("1", "Hi")], &Cancel::new())
        .await
        .unwrap();
    assert_eq!(out, vec!["譯：Hi"]);
    let r = srv.last();
    assert_eq!(r.path, "/v1/messages");
    assert_eq!(r.headers["x-api-key"], "sk-ant-test");
    assert!(r.headers.contains_key("anthropic-version"));
}

#[tokio::test]
async fn gemini_request_and_reply() {
    let srv = serve(|req, _| gemini_reply(&fake_translate(&user_text(&req.body)))).await;
    let e = Engine::new(&cfg(
        Provider::Gemini,
        &format!("{}/v1beta", srv.base),
        "gemini-test",
        "g-key",
    ))
    .unwrap();
    let out = e
        .translate_batch(&params(), &[seg("1", "Hi")], &Cancel::new())
        .await
        .unwrap();
    assert_eq!(out, vec!["譯：Hi"]);
    let r = srv.last();
    assert_eq!(r.path, "/v1beta/models/gemini-test:generateContent");
    assert_eq!(r.headers["x-goog-api-key"], "g-key");
}

#[tokio::test]
async fn deepl_uses_official_api_shape() {
    let srv = serve(|req, _| {
        let v: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        let tr: Vec<_> = v["text"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| serde_json::json!({"detected_source_language": "EN", "text": format!("譯：{}", t.as_str().unwrap())}))
            .collect();
        Resp::json(serde_json::json!({ "translations": tr }).to_string())
    })
    .await;
    let e = Engine::new(&cfg(Provider::Deepl, &srv.base, "", "abc:fx")).unwrap();
    let out = e
        .translate_batch(
            &params(),
            &[seg("1", "One"), seg("2", "Two")],
            &Cancel::new(),
        )
        .await
        .unwrap();
    assert_eq!(out, vec!["譯：One", "譯：Two"]);
    let r = srv.last();
    assert_eq!(r.path, "/v2/translate");
    assert_eq!(r.headers["authorization"], "DeepL-Auth-Key abc:fx");
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(v["target_lang"], "ZH-HANT");
    assert_eq!(v["source_lang"], "EN");
}

#[tokio::test]
async fn google_v2_puts_the_key_in_a_header_not_the_url() {
    let srv = serve(|req, _| {
        let v: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        let tr: Vec<_> = v["q"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| serde_json::json!({"translatedText": format!("譯：{}", t.as_str().unwrap())}))
            .collect();
        Resp::json(serde_json::json!({ "data": { "translations": tr } }).to_string())
    })
    .await;
    let e = Engine::new(&cfg(Provider::GoogleTranslate, &srv.base, "", "g-key")).unwrap();
    let out = e
        .translate_batch(&params(), &[seg("1", "One")], &Cancel::new())
        .await
        .unwrap();
    assert_eq!(out, vec!["譯：One"]);
    let r = srv.last();
    assert_eq!(r.path, "/language/translate/v2");
    assert_eq!(r.headers["x-goog-api-key"], "g-key");
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(v["target"], "zh-TW");
    assert_eq!(v["format"], "text");
}

#[tokio::test]
async fn auth_error_is_not_retried() {
    let srv = serve(|_, _| Resp::status(401, r#"{"error":{"message":"invalid x-api-key"}}"#)).await;
    let e = fast(
        Engine::new(&cfg(
            Provider::Anthropic,
            &format!("{}/v1", srv.base),
            "m",
            "bad",
        ))
        .unwrap(),
    );
    let err = e
        .translate_batch(&params(), &[seg("1", "Hi")], &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Auth);
    assert_eq!(srv.count(), 1);
}

#[tokio::test]
async fn rate_limit_is_retried_then_succeeds() {
    let srv = serve(|req, n| {
        if n == 1 {
            let mut r = Resp::status(429, "slow down");
            r.headers.push(("retry-after".into(), "0".into()));
            r
        } else {
            openai_reply(&fake_translate(&user_text(&req.body)))
        }
    })
    .await;
    let e = fast(
        Engine::new(&cfg(
            Provider::Openai,
            &format!("{}/v1", srv.base),
            "m",
            "k",
        ))
        .unwrap(),
    );
    let out = e
        .translate_batch(&params(), &[seg("1", "Hi")], &Cancel::new())
        .await
        .unwrap();
    assert_eq!(out, vec!["譯：Hi"]);
    assert_eq!(srv.count(), 2);
}

#[tokio::test]
async fn server_errors_give_up_after_the_attempt_limit() {
    let srv = serve(|_, _| Resp::status(503, "down")).await;
    let e = fast(Engine::new(&cfg(Provider::Deepl, &srv.base, "", "k")).unwrap());
    let err = e
        .translate_batch(&params(), &[seg("1", "Hi")], &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Server);
    assert_eq!(srv.count(), 3);
}

#[tokio::test]
async fn unreachable_service_is_a_network_error() {
    // 沒有人在聽的埠
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let e = fast(
        Engine::new(&cfg(
            Provider::Openai,
            &format!("http://127.0.0.1:{port}/v1"),
            "m",
            "k",
        ))
        .unwrap(),
    );
    let err = e
        .translate_batch(&params(), &[seg("1", "Hi")], &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Network);
}

#[tokio::test]
async fn cancel_interrupts_a_slow_request() {
    // 假伺服器收到後永遠不回；取消要能中斷等待
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((s, _)) = l.accept().await {
            held.push(s);
        }
    });
    let e = Engine::new(&cfg(
        Provider::Openai,
        &format!("http://127.0.0.1:{port}/v1"),
        "m",
        "k",
    ))
    .unwrap();
    let cancel = Cancel::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        c2.cancel();
    });
    let started = std::time::Instant::now();
    let err = e
        .translate_batch(&params(), &[seg("1", "Hi")], &cancel)
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn missing_key_or_model_is_a_config_error() {
    let no_key = Engine::new(&cfg(Provider::Anthropic, "https://x.test/v1", "m", ""));
    assert_eq!(no_key.err().unwrap().kind, ErrorKind::Config);
    let no_model = Engine::new(&cfg(Provider::Openai, "https://x.test/v1", " ", "k"));
    assert_eq!(no_model.err().unwrap().kind, ErrorKind::Config);
    let no_url = Engine::new(&EngineConfig {
        provider: Provider::OpenaiCompatible,
        model: "m".into(),
        endpoint: None,
        api_key: String::new(),
    });
    assert_eq!(no_url.err().unwrap().kind, ErrorKind::Config);
    let no_mt_key = Engine::new(&cfg(Provider::Deepl, "https://x.test", "", ""));
    assert_eq!(no_mt_key.err().unwrap().kind, ErrorKind::Config);
}

#[test]
fn debug_output_never_shows_the_key() {
    let c = cfg(Provider::Openai, "https://x.test/v1", "m", "sk-secret-123");
    assert!(!format!("{c:?}").contains("sk-secret-123"));
}

#[tokio::test]
async fn job_splits_bad_batches_marks_the_hopeless_one_failed_and_continues() {
    // 超過 3 段的批次，模型就少回一段；內容含 "POISON" 的段落永遠回不出來
    let srv = serve(|req, _| {
        let text = user_text(&req.body);
        if text.contains("POISON") && text.matches("<seg id=").count() == 1 {
            return openai_reply("<seg id=\"s4\"></seg>");
        }
        if text.matches("<seg id=").count() > 3 || text.contains("POISON") {
            let reply = fake_translate(&text);
            let cut = reply.rfind("<seg").unwrap();
            return openai_reply(&reply[..cut]);
        }
        openai_reply(&fake_translate(&text))
    })
    .await;
    let e = fast(
        Engine::new(&cfg(
            Provider::OpenaiCompatible,
            &format!("{}/v1", srv.base),
            "m",
            "",
        ))
        .unwrap(),
    );
    let mut segs: Vec<Segment> = (0..9)
        .map(|i| seg(&format!("s{i}"), &format!("para {i}")))
        .collect();
    segs[4].text = "POISON".into();
    let mut results: Vec<(String, std::result::Result<String, ErrorKind>)> = Vec::new();
    let sum = run_job(&e, &params(), segs, &Cancel::new(), |ev| {
        for it in ev.items {
            results.push((it.id, it.result));
        }
        true
    })
    .await;
    assert_eq!(sum.done, 8);
    assert_eq!(sum.failed, 1);
    assert!(!sum.cancelled && sum.stopped.is_none());
    let failed: Vec<_> = results.iter().filter(|(_, r)| r.is_err()).collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, "s4");
    let ok_ids: std::collections::BTreeSet<_> = results
        .iter()
        .filter(|(_, r)| r.is_ok())
        .map(|(i, _)| i.clone())
        .collect();
    assert_eq!(ok_ids.len(), 8);
}

#[tokio::test]
async fn job_retries_a_segment_alone_when_the_check_finds_a_critical_issue() {
    // 一批多段時，模型把引用編號 [7] 弄丟；單獨送那一段時就保留
    let srv = serve(|req, _| {
        let text = user_text(&req.body);
        let reply = fake_translate(&text);
        if text.matches("<seg id=").count() > 1 {
            return openai_reply(&reply.replace(" [7]", ""));
        }
        openai_reply(&reply)
    })
    .await;
    let e = fast(
        Engine::new(&cfg(
            Provider::OpenaiCompatible,
            &format!("{}/v1", srv.base),
            "m",
            "",
        ))
        .unwrap(),
    );
    let mut p = params();
    p.target_lang = "de".into(); // 假服務不會真的翻成中文，用非中日韓的目標語言避開「漏翻」判斷
    let segs = vec![
        seg("a", "First paragraph."),
        seg("b", "As shown in prior work [7], it works."),
        seg("c", "Third paragraph."),
    ];
    let mut items: Vec<BatchItem> = Vec::new();
    let sum = run_job(&e, &p, segs, &Cancel::new(), |ev| {
        items.extend(ev.items);
        true
    })
    .await;
    assert_eq!(sum.done, 3);
    assert_eq!(
        sum.retried, 1,
        "only the segment that lost its citation is retried"
    );
    assert_eq!(sum.flagged, 0, "the retry fixed it");
    let b = items.iter().find(|i| i.id == "b").unwrap();
    assert!(b.result.as_ref().unwrap().contains("[7]"));
    assert!(b.issues.is_empty());
}

#[tokio::test]
async fn job_keeps_the_translation_and_flags_it_when_the_retry_does_not_help() {
    // 不論怎麼送，模型都把 [7] 弄丟
    let srv = serve(|req, _| {
        let text = user_text(&req.body);
        openai_reply(&fake_translate(&text).replace(" [7]", ""))
    })
    .await;
    let e = fast(
        Engine::new(&cfg(
            Provider::OpenaiCompatible,
            &format!("{}/v1", srv.base),
            "m",
            "",
        ))
        .unwrap(),
    );
    let mut p = params();
    p.target_lang = "de".into();
    let segs = vec![seg("a", "One."), seg("b", "Prior work [7] says so.")];
    let mut items: Vec<BatchItem> = Vec::new();
    let sum = run_job(&e, &p, segs, &Cancel::new(), |ev| {
        items.extend(ev.items);
        true
    })
    .await;
    assert_eq!(sum.done, 2);
    assert_eq!(sum.retried, 1);
    assert_eq!(sum.flagged, 1);
    let b = items.iter().find(|i| i.id == "b").unwrap();
    assert!(b.result.is_ok(), "the translation is kept, not thrown away");
    assert_eq!(
        b.issues.iter().map(|i| i.code()).collect::<Vec<_>>(),
        vec!["missing_citation"]
    );
}

#[tokio::test]
async fn job_stops_on_auth_error_and_keeps_the_rest_pending() {
    let srv = serve(|_, _| Resp::status(401, "no")).await;
    let e = fast(Engine::new(&cfg(Provider::Deepl, &srv.base, "", "bad")).unwrap());
    let segs: Vec<Segment> = (0..5).map(|i| seg(&format!("s{i}"), "x")).collect();
    let mut called = false;
    let sum = run_job(&e, &params(), segs, &Cancel::new(), |_| {
        called = true;
        true
    })
    .await;
    assert_eq!(sum.stopped, Some(ErrorKind::Auth));
    assert_eq!(sum.done + sum.failed, 0);
    assert!(!called);
}

#[tokio::test]
async fn job_can_be_cancelled_between_batches_and_resumed() {
    let srv = serve(|req, _| openai_reply(&fake_translate(&user_text(&req.body)))).await;
    let e = fast(
        Engine::new(&cfg(
            Provider::OpenaiCompatible,
            &format!("{}/v1", srv.base),
            "m",
            "",
        ))
        .unwrap(),
    );
    let segs: Vec<Segment> = (0..25)
        .map(|i| seg(&format!("s{i:02}"), &format!("p{i}")))
        .collect();
    let cancel = Cancel::new();
    let mut done_ids: Vec<String> = Vec::new();
    // 對話模型一批 10 段：第一批做完就取消
    let sum = run_job(&e, &params(), segs.clone(), &cancel, |ev| {
        done_ids.extend(ev.items.into_iter().map(|i| i.id));
        cancel.cancel();
        true
    })
    .await;
    assert!(sum.cancelled);
    assert_eq!(done_ids.len(), 10);
    // 接著翻：只送還沒翻的
    let rest: Vec<Segment> = segs
        .into_iter()
        .filter(|s| !done_ids.contains(&s.id))
        .collect();
    assert_eq!(rest.len(), 15);
    let before = srv.count();
    let sum2 = run_job(&e, &params(), rest, &Cancel::new(), |ev| {
        done_ids.extend(ev.items.into_iter().map(|i| i.id));
        true
    })
    .await;
    assert_eq!(sum2.done, 15);
    assert_eq!(done_ids.len(), 25);
    assert_eq!(srv.count() - before, 2, "15 segments go out as two batches");
}

#[tokio::test]
async fn context_before_is_sent_but_marked_as_context() {
    let srv = serve(|req, _| openai_reply(&fake_translate(&user_text(&req.body)))).await;
    let e = Engine::new(&cfg(
        Provider::OpenaiCompatible,
        &format!("{}/v1", srv.base),
        "m",
        "",
    ))
    .unwrap();
    let s = Segment {
        id: "1".into(),
        text: "Second".into(),
        before: Some("First paragraph.".into()),
    };
    e.translate_batch(&params(), &[s], &Cancel::new())
        .await
        .unwrap();
    let body = srv.last().body;
    assert!(body.contains("<context>"));
    assert!(body.contains("First paragraph."));
}

#[tokio::test]
async fn test_connection_returns_a_translation() {
    let srv = serve(|req, _| openai_reply(&fake_translate(&user_text(&req.body)))).await;
    let e = Engine::new(&cfg(
        Provider::OpenaiCompatible,
        &format!("{}/v1", srv.base),
        "m",
        "",
    ))
    .unwrap();
    let t = e.test(&params(), &Cancel::new()).await.unwrap();
    assert!(t.starts_with("譯："));
}

#[tokio::test]
async fn machine_translation_is_never_retried_but_still_flagged() {
    // DeepL 把 [7] 弄丟：同一段再送一次也會得到同樣的結果，所以不重翻，只標出來
    let srv = serve(|req, _| {
        let v: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        let tr: Vec<_> = v["text"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| serde_json::json!({"text": t.as_str().unwrap().replace(" [7]", "")}))
            .collect();
        Resp::json(serde_json::json!({ "translations": tr }).to_string())
    })
    .await;
    let e = fast(Engine::new(&cfg(Provider::Deepl, &srv.base, "", "k")).unwrap());
    let mut p = params();
    p.target_lang = "de".into();
    let segs = vec![seg("1", "One."), seg("2", "Prior work [7] says so.")];
    let sum = run_job(&e, &p, segs, &Cancel::new(), |_| true).await;
    assert_eq!(sum.done, 2);
    assert_eq!(sum.retried, 0);
    assert_eq!(sum.flagged, 1);
    assert_eq!(srv.count(), 1, "one request for the whole batch, no retry");
}

#[tokio::test]
async fn an_empty_machine_translation_fails_only_that_segment() {
    // DeepL 對第二段回空字串：其餘兩段照存，第二段記為失敗
    let srv = serve(|req, _| {
        let v: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        let tr: Vec<_> = v["text"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let s = t.as_str().unwrap();
                let out = if s == "Two" {
                    String::new()
                } else {
                    format!("譯：{s}")
                };
                serde_json::json!({"text": out})
            })
            .collect();
        Resp::json(serde_json::json!({ "translations": tr }).to_string())
    })
    .await;
    let e = fast(Engine::new(&cfg(Provider::Deepl, &srv.base, "", "k")).unwrap());
    let segs = vec![seg("1", "One"), seg("2", "Two"), seg("3", "Three")];
    let mut results: Vec<(String, std::result::Result<String, ErrorKind>)> = Vec::new();
    let sum = run_job(&e, &params(), segs, &Cancel::new(), |ev| {
        for it in ev.items {
            results.push((it.id, it.result));
        }
        true
    })
    .await;
    assert_eq!((sum.done, sum.failed), (2, 1));
    assert!(sum.stopped.is_none() && !sum.sink_failed);
    assert_eq!(results[0], ("1".to_string(), Ok("譯：One".to_string())));
    assert_eq!(results[1], ("2".to_string(), Err(ErrorKind::BadResponse)));
    assert_eq!(results[2], ("3".to_string(), Ok("譯：Three".to_string())));
}

#[tokio::test]
async fn deepl_quota_is_not_reported_as_a_bad_key() {
    // DeepL 用 456 表示額度用完：要等額度重置或加購，不是金鑰不對
    let srv = serve(|_, _| Resp::status(456, "Quota exceeded")).await;
    let e = fast(Engine::new(&cfg(Provider::Deepl, &srv.base, "", "k")).unwrap());
    let err = e
        .translate_batch(&params(), &[seg("1", "Hello.")], &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::QuotaExceeded);
    assert!(err.is_fatal_for_job());
}

#[tokio::test]
async fn unsupported_target_stops_before_anything_is_sent() {
    let srv = serve(|_, _| Resp::status(500, "should not be called")).await;
    let e = fast(Engine::new(&cfg(Provider::Deepl, &srv.base, "", "k")).unwrap());
    let mut p = params();
    p.target_lang = "tlh".into();
    let err = e
        .translate_batch(&p, &[seg("1", "Hello.")], &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::UnsupportedLanguage);
    assert_eq!(
        srv.count(),
        0,
        "nothing is sent for an unsupported language"
    );
}

#[tokio::test]
async fn refine_sends_drafts_back_and_keeps_the_polished_text() {
    let srv = serve(|req, _| openai_reply(&fake_translate(&user_text(&req.body)))).await;
    let e = fast(
        Engine::new(&cfg(
            Provider::OpenaiCompatible,
            &format!("{}/v1", srv.base),
            "m",
            "",
        ))
        .unwrap(),
    );
    let p = Params {
        refine: true,
        ..params()
    };
    let mut got: Vec<String> = Vec::new();
    let sum = run_job(
        &e,
        &p,
        vec![seg("a", "Hello"), seg("b", "World")],
        &Cancel::new(),
        |ev| {
            got.extend(ev.items.into_iter().map(|i| i.result.unwrap()));
            true
        },
    )
    .await;
    assert_eq!(srv.count(), 2, "one translation call and one refine call");
    assert_eq!(got, vec!["譯：譯：Hello", "譯：譯：World"]);
    assert_eq!(sum.refined, 2);
    let body = srv.last().body;
    assert!(body.contains("<src id=\\\"a\\\">Hello</src>"), "{body}");
}

#[tokio::test]
async fn refine_failure_keeps_the_draft() {
    let srv = serve(|req, n| {
        if n == 1 {
            openai_reply(&fake_translate(&user_text(&req.body)))
        } else {
            Resp::status(400, "bad")
        }
    })
    .await;
    let e = fast(
        Engine::new(&cfg(
            Provider::OpenaiCompatible,
            &format!("{}/v1", srv.base),
            "m",
            "",
        ))
        .unwrap(),
    );
    let p = Params {
        refine: true,
        ..params()
    };
    let mut got: Vec<String> = Vec::new();
    let sum = run_job(&e, &p, vec![seg("a", "Hello")], &Cancel::new(), |ev| {
        got.extend(ev.items.into_iter().map(|i| i.result.unwrap()));
        true
    })
    .await;
    assert_eq!(got, vec!["譯：Hello"]);
    assert_eq!(sum.refined, 0);
    assert_eq!(sum.done, 1);
}

#[tokio::test]
async fn the_rate_limit_spaces_requests_out() {
    let srv = serve(|req, _| openai_reply(&fake_translate(&user_text(&req.body)))).await;
    let e = Engine::new(&cfg(
        Provider::OpenaiCompatible,
        &format!("{}/v1", srv.base),
        "m",
        "",
    ))
    .unwrap()
    .with_rate(600); // 每 100 毫秒一次
    let start = std::time::Instant::now();
    for i in 0..3 {
        e.translate_batch(&params(), &[seg(&i.to_string(), "x")], &Cancel::new())
            .await
            .unwrap();
    }
    assert!(start.elapsed() >= Duration::from_millis(200));
    assert_eq!(srv.count(), 3);
}

#[tokio::test]
async fn the_glossary_reaches_the_chat_model() {
    let srv = serve(|req, _| openai_reply(&fake_translate(&user_text(&req.body)))).await;
    let e = Engine::new(&cfg(
        Provider::OpenaiCompatible,
        &format!("{}/v1", srv.base),
        "m",
        "",
    ))
    .unwrap();
    let p = Params {
        glossary: vec![("LoRA".into(), "低秩適應".into())],
        ..params()
    };
    e.translate_batch(&p, &[seg("1", "We use LoRA.")], &Cancel::new())
        .await
        .unwrap();
    assert!(srv.last().body.contains("LoRA => 低秩適應"));
}

#[tokio::test]
async fn deepl_creates_a_glossary_once_and_uses_it() {
    let srv = serve(|req, _| match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/v2/glossaries") => Resp::json(r#"{"glossaries":[]}"#),
        ("POST", "/v2/glossaries") => Resp::json(r#"{"glossary_id":"g-1"}"#),
        _ => {
            let v: serde_json::Value = serde_json::from_str(&req.body).unwrap();
            let n = v["text"].as_array().unwrap().len();
            let tr: Vec<_> = (0..n).map(|_| serde_json::json!({"text": "譯"})).collect();
            Resp::json(serde_json::json!({ "translations": tr }).to_string())
        }
    })
    .await;
    let e = Engine::new(&cfg(Provider::Deepl, &srv.base, "", "k")).unwrap();
    let p = Params {
        glossary: vec![("LoRA".into(), "低秩適應".into())],
        ..params()
    };
    for _ in 0..2 {
        e.translate_batch(&p, &[seg("1", "x")], &Cancel::new())
            .await
            .unwrap();
    }
    let reqs = srv.all();
    let create: Vec<_> = reqs
        .iter()
        .filter(|r| r.method == "POST" && r.path == "/v2/glossaries")
        .collect();
    assert_eq!(create.len(), 1, "the glossary is created once per job");
    let body: serde_json::Value = serde_json::from_str(&create[0].body).unwrap();
    assert_eq!(body["entries"], "LoRA\t低秩適應\n");
    assert_eq!(body["entries_format"], "tsv");
    assert_eq!(body["source_lang"], "en");
    assert_eq!(body["target_lang"], "zh");
    let last: serde_json::Value = serde_json::from_str(&srv.last().body).unwrap();
    assert_eq!(last["glossary_id"], "g-1");
}

#[tokio::test]
async fn deepl_translates_without_a_glossary_it_cannot_create() {
    let srv = serve(|req, _| match req.path.as_str() {
        "/v2/glossaries" => Resp::status(400, "unsupported language pair"),
        _ => Resp::json(r#"{"translations":[{"text":"譯"}]}"#),
    })
    .await;
    let e = Engine::new(&cfg(Provider::Deepl, &srv.base, "", "k")).unwrap();
    let p = Params {
        glossary: vec![("a".into(), "甲".into())],
        ..params()
    };
    let out = e
        .translate_batch(&p, &[seg("1", "x")], &Cancel::new())
        .await
        .unwrap();
    assert_eq!(out, vec!["譯"]);
    let last: serde_json::Value = serde_json::from_str(&srv.last().body).unwrap();
    assert!(last.get("glossary_id").is_none());
}

// ---------- 提示詞自訂、術語一致、用量----------

/// 送出的系統提示詞與使用者訊息（OpenAI 相容格式的 `messages`）。
fn system_and_user(body: &str) -> (String, String) {
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let mut sys = String::new();
    let mut user = String::new();
    for m in v["messages"].as_array().unwrap() {
        let text = m["content"].as_str().unwrap_or_default();
        match m["role"].as_str() {
            Some("system" | "developer") => sys.push_str(text),
            Some("user") => user.push_str(text),
            _ => {}
        }
    }
    (sys, user)
}

fn compat(base: &str) -> Engine {
    fast(
        Engine::new(&cfg(
            Provider::OpenaiCompatible,
            &format!("{base}/v1"),
            "m",
            "",
        ))
        .unwrap(),
    )
}

#[tokio::test]
async fn the_preview_is_exactly_what_is_sent_for_translate_and_refine() {
    let srv = serve(|req, _| openai_reply(&fake_translate(&user_text(&req.body)))).await;
    let e = compat(&srv.base).with_paper_terms(vec![
        PaperTerm {
            source: "token".into(),
            target: "詞元".into(),
            state: TermState::Learned,
            batches: 2,
        },
        PaperTerm {
            source: "gain".into(),
            target: "增益".into(),
            state: TermState::Candidate,
            batches: 1,
        },
    ]);
    let p = Params {
        glossary: vec![("LoRA".into(), "低秩適應".into())],
        instructions: "Translate \"{{title}}\" into {{to}}.".into(),
        title: Some("T".into()),
        ..params()
    };
    let batch = vec![
        Segment {
            id: "a".into(),
            text: "LoRA uses a token.".into(),
            before: Some("Before.".into()),
        },
        seg("b", "World"),
    ];
    e.translate_batch(&p, &batch, &Cancel::new()).await.unwrap();
    let terms = terms_in_use(&e.paper_terms());
    let pv = translate_preview(&p, &batch, &terms);
    let (sys, user) = system_and_user(&srv.last().body);
    assert_eq!(sys, join(&pv.system));
    assert_eq!(user, join(&pv.user));
    assert!(sys.starts_with("Translate \"T\" into Traditional Chinese (繁體中文)."));
    assert!(
        sys.contains("- LoRA => 低秩適應\n- token => 詞元\n"),
        "{sys}"
    );
    assert!(!sys.contains("增益"), "candidates are not sent");

    // 第二輪潤飾那一次請求也列得出來
    let drafts = vec!["一".to_string(), "二".to_string()];
    e.refine_batch(&p, &batch, &drafts, &Cancel::new())
        .await
        .unwrap();
    let rv = refine_preview(&p, &batch, &drafts, &terms);
    assert_eq!(rv.kind, "refine");
    let (sys, user) = system_and_user(&srv.last().body);
    assert_eq!(sys, join(&rv.system));
    assert_eq!(user, join(&rv.user));
}

#[tokio::test]
async fn terms_agreed_by_two_batches_reach_the_next_one_and_the_glossary_wins() {
    // 同一篇的術語一致：兩批附了同一個譯法才定下，之後的批次帶進術語；
    // 術語庫有的以術語庫為準
    let srv = serve(|req, _| {
        let out = fake_translate(&user_text(&req.body))
            .replace("token", "詞元")
            .replace("policy", "政策");
        openai_reply(&format!(
            "{out}<terms>\ntoken => 詞元\npolicy => 政策\n</terms>\n"
        ))
    })
    .await;
    let e = compat(&srv.base);
    let p = Params {
        glossary: vec![("policy".into(), "策略".into())],
        ..params()
    };
    // 21 段：三批（10、10、1）
    let segs: Vec<Segment> = (0..21)
        .map(|i| seg(&format!("s{i}"), "The token and the policy."))
        .collect();
    let mut got: Vec<String> = Vec::new();
    let sum = run_job(&e, &p, segs, &Cancel::new(), |ev| {
        got.extend(ev.items.into_iter().map(|i| i.result.unwrap()));
        true
    })
    .await;
    assert_eq!(sum.done, 21);
    let reqs = srv.all();
    assert_eq!(reqs.len(), 3);
    for r in &reqs[..2] {
        let (s, _) = system_and_user(&r.body);
        assert!(!s.contains("token =>"), "only a candidate after one batch");
    }
    let (third, _) = system_and_user(&reqs[2].body);
    assert!(
        third.contains("- policy => 策略\n- token => 詞元\n"),
        "{third}"
    );
    assert_eq!(
        terms_in_use(&e.paper_terms()),
        vec![("token".to_string(), "詞元".to_string())]
    );
    assert!(
        got.iter()
            .all(|t| !t.contains("terms") && !t.contains("=>"))
    );
    // 中文的半形句號換成全形
    assert_eq!(got[0], "譯：The 詞元 and the 政策。");
    // 用量：每個回應都記（假伺服器每次回報輸入 1、輸出 1）
    assert_eq!(
        e.usage(),
        TokenUsage {
            input: 3,
            output: 3
        }
    );
}

#[tokio::test]
async fn terms_are_learned_from_the_text_that_is_kept_not_the_draft() {
    // 潤飾換掉了草稿的譯法，草稿附的術語不能算數
    let srv = serve(|req, _| {
        let u = user_text(&req.body);
        if u.contains("<src id=") {
            // 潤飾：「權杖」改成「詞元」
            openai_reply(&fake_translate(&u).replace("權杖", "詞元"))
        } else {
            let out = fake_translate(&u).replace("token", "權杖");
            openai_reply(&format!("{out}<terms>\ntoken => 權杖\n</terms>"))
        }
    })
    .await;
    let e = compat(&srv.base);
    let p = Params {
        refine: true,
        ..params()
    };
    let segs: Vec<Segment> = (0..20)
        .map(|i| seg(&format!("s{i}"), "The token moves."))
        .collect();
    run_job(&e, &p, segs, &Cancel::new(), |_| true).await;
    assert!(
        e.paper_terms().iter().all(|t| t.target != "權杖"),
        "{:?}",
        e.paper_terms()
    );
}
