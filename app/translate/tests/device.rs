//! 裝置上的引擎（Firefox 翻譯）經外殼的轉接接進同一條工作迴圈：分批、自動檢查、停下來的錯誤。

use paperreader_translate::*;
use std::sync::{Arc, Mutex};

struct Fake {
    calls: Mutex<Vec<Vec<String>>>,
    fail: Option<ErrorKind>,
}

impl DeviceTranslator for Fake {
    fn translate<'a>(&'a self, _params: &'a Params, texts: Vec<String>) -> DeviceFuture<'a> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(texts.clone());
            if let Some(k) = self.fail {
                return Err(TranslateError::new(k, "fake"));
            }
            // 模型把引用編號弄丟的那一段，讓自動檢查標出來
            Ok(texts
                .iter()
                .map(|t| format!("譯文：{}", t.replace(" [7]", "")))
                .collect())
        })
    }
}

fn params() -> Params {
    Params {
        source_lang: Some("en".into()),
        target_lang: "zh-TW".into(),
        ..Params::default()
    }
}

fn segs(n: usize) -> Vec<Segment> {
    (0..n)
        .map(|i| Segment {
            id: format!("s{i}"),
            text: if i == 2 {
                "As shown in [7], the model converges.".into()
            } else {
                format!("Paragraph number {i} of the paper.")
            },
            before: None,
            ..Default::default()
        })
        .collect()
}

#[tokio::test]
async fn on_device_engine_runs_the_same_job_loop() {
    let fake = Arc::new(Fake {
        calls: Mutex::new(Vec::new()),
        fail: None,
    });
    let e = Engine::on_device(Provider::Bergamot, fake.clone()).unwrap();
    let mut items: Vec<BatchItem> = Vec::new();
    let sum = run_job(&e, &params(), segs(20), &Cancel::new(), |ev| {
        items.extend(ev.items);
        true
    })
    .await;
    assert_eq!(sum.done, 20);
    assert_eq!(sum.failed, 0);
    // 一批最多 8 段；裝置上的引擎不重翻（同一段會翻出一樣的結果）
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|c| c.len() <= 8));
    assert_eq!(sum.retried, 0);
    assert_eq!(sum.flagged, 1, "the lost citation is flagged");
    let s2 = items.iter().find(|i| i.id == "s2").unwrap();
    assert!(!s2.issues.is_empty());
}

#[tokio::test]
async fn missing_model_stops_the_job() {
    let fake = Arc::new(Fake {
        calls: Mutex::new(Vec::new()),
        fail: Some(ErrorKind::ModelMissing),
    });
    let e = Engine::on_device(Provider::Bergamot, fake.clone()).unwrap();
    let sum = run_job(&e, &params(), segs(5), &Cancel::new(), |_| true).await;
    assert_eq!(sum.stopped, Some(ErrorKind::ModelMissing));
    assert_eq!(sum.done, 0);
    assert_eq!(
        fake.calls.lock().unwrap().len(),
        1,
        "no retry when the model is missing"
    );
}

/// 原文抽出的控制字元（公式括號的 `\u{0}`、`\u{1}`）讓 Firefox 翻譯回不出東西：
/// 送出前就濾掉，引擎收到的是乾淨的文字。
struct NoControls {
    calls: Mutex<Vec<Vec<String>>>,
}

impl DeviceTranslator for NoControls {
    fn translate<'a>(&'a self, _params: &'a Params, texts: Vec<String>) -> DeviceFuture<'a> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(texts.clone());
            if texts.iter().any(|t| t.chars().any(char::is_control)) {
                return Ok(texts.iter().map(|_| String::new()).collect());
            }
            Ok(texts.iter().map(|t| format!("譯：{t}")).collect())
        })
    }
}

#[tokio::test]
async fn control_characters_are_removed_before_sending() {
    let fake = Arc::new(NoControls {
        calls: Mutex::new(Vec::new()),
    });
    let e = Engine::on_device(Provider::Bergamot, fake.clone()).unwrap();
    let segs = vec![
        Segment {
            id: "g0102".into(),
            text: "\u{0}x\u{1} where the loss is minimal".into(),
            before: Some("prev\u{1}".into()),
            ..Default::default()
        },
        Segment {
            id: "g0103".into(),
            text: "Plain text.".into(),
            before: None,
            ..Default::default()
        },
    ];
    let mut out: Vec<BatchItem> = Vec::new();
    let sum = run_job(&e, &params(), segs, &Cancel::new(), |ev| {
        out.extend(ev.items);
        true
    })
    .await;
    assert_eq!((sum.done, sum.failed), (2, 0));
    assert_eq!(
        out[0].result.as_deref().unwrap(),
        "譯：x where the loss is minimal"
    );
    let calls = fake.calls.lock().unwrap();
    assert!(
        calls
            .iter()
            .flatten()
            .all(|t| !t.chars().any(char::is_control))
    );
}

#[test]
fn engine_kinds_are_not_mixed_up() {
    let fake: Arc<dyn DeviceTranslator> = Arc::new(Fake {
        calls: Mutex::new(Vec::new()),
        fail: None,
    });
    assert!(Engine::on_device(Provider::Anthropic, fake).is_err());
    let cfg = EngineConfig {
        provider: Provider::Bergamot,
        model: String::new(),
        endpoint: None,
        api_key: String::new(),
    };
    assert!(Engine::new(&cfg).is_err());
    assert!(Provider::Bergamot.is_on_device() && !Provider::Bergamot.is_chat_model());
}
