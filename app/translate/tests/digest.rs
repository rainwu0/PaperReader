//! 重點筆記對假伺服器：一份或分段再合併、段落代號換成頁碼、讀不懂的回覆再要一次、
//! 停下來之後換服務接著做時不重送做好的部分。

mod common;

use common::*;
use paperreader_translate::digest::PART_CHARS;
use paperreader_translate::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn engine(base: &str) -> Engine {
    Engine::new(&EngineConfig {
        provider: Provider::OpenaiCompatible,
        model: "m".into(),
        endpoint: Some(format!("{base}/v1")),
        api_key: String::new(),
    })
    .unwrap()
    .with_retry(Duration::from_millis(1), 2)
}

/// 送出的使用者訊息（含 `<para` 或 `<notes` 的那一個字串）。
fn sent(body: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let mut found = String::new();
    fn walk(v: &serde_json::Value, found: &mut String) {
        match v {
            serde_json::Value::String(s)
                if s.contains("<para id=") || s.contains("<notes part=") =>
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

/// 假的重點筆記：拿訊息裡第一段與最後一段的代號當出處，另外放一個不存在的代號。
fn fake_notes(msg: &str) -> String {
    let ids: Vec<&str> = msg
        .split("id=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .collect();
    let first = ids.first().copied().unwrap_or("g0001");
    let last = ids.last().copied().unwrap_or(first);
    serde_json::json!({
        "question": [{"text": "問題。", "refs": [first]}],
        "method": [{"text": "方法。", "refs": [first, last]}],
        "results": [{"text": "結果 53.1 AP。", "refs": [last]}, {"text": "沒有出處。", "refs": ["g9999"]}],
        "limitations": [],
        "quotes": [{"text": "「a quote」：值得記下。", "refs": [last]}]
    })
    .to_string()
}

fn input(n: usize, len: usize) -> DigestInput {
    DigestInput {
        title: Some("Paper".into()),
        abstract_text: Some("We study things.".into()),
        segments: (0..n)
            .map(|i| DigestSegment {
                id: format!("g{:04}", i + 1),
                page: (i / 3) as u32,
                text: format!("{i} {}", "w".repeat(len)),
            })
            .collect(),
        target_lang: "zh-TW".into(),
        ..Default::default()
    }
}

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

#[tokio::test]
async fn the_preview_is_exactly_what_is_sent_for_a_part_and_a_merge() {
    // 「看完整提示詞」與實際送出的逐字相同：一部分、與分部分之後的合併
    let srv = serve(|req, _| openai_reply(&fake_notes(&sent(&req.body)))).await;
    let e = engine(&srv.base);
    let mut one = input(9, 50);
    one.glossary = vec![("w".into(), "字".into())];
    one.paper_terms = vec![("Paper".into(), "論文".into())];
    one.instructions = "Notes for {{title}} in {{to}}.".into();
    run_digest(
        &e,
        &one,
        &mut DigestState::default(),
        &Cancel::new(),
        |_| {},
    )
    .await
    .unwrap();
    let preview = digest_preview(&one);
    let (sys, user) = system_and_user(&srv.last().body);
    assert_eq!(sys, join(&preview.system));
    assert_eq!(user, join(&preview.user));
    assert!(sys.starts_with("Notes for Paper in Traditional Chinese (繁體中文).\n"));
    assert!(sys.contains("- w => 字\n"));

    let srv = serve(|req, _| openai_reply(&fake_notes(&sent(&req.body)))).await;
    let e = engine(&srv.base);
    let long = input(30, 2000);
    let mut st = DigestState::default();
    run_digest(&e, &long, &mut st, &Cancel::new(), |_| {})
        .await
        .unwrap();
    let parts: Vec<Vec<DigestItem>> = st.parts.iter().flatten().cloned().collect();
    let texts: Vec<&str> = long.segments.iter().map(|s| s.text.as_str()).collect();
    let (sys, user) = system_and_user(&srv.last().body);
    assert_eq!(
        sys,
        join(&paperreader_translate::digest::digest_parts(
            &long,
            DigestStage::Merge,
            &texts
        ))
    );
    assert_eq!(
        user,
        paperreader_translate::digest::merge_message(&long, &parts)
    );
}

#[tokio::test]
async fn an_added_section_changes_the_reply_shape_and_is_read_back() {
    let srv = serve(|req, _| {
        let m = sent(&req.body);
        let mut v: serde_json::Value = serde_json::from_str(&fake_notes(&m)).unwrap();
        v["contradictions"] = serde_json::json!([{"text": "第 1 頁與第 2 頁的數字不同.", "refs": ["g0001", "g0004"]}]);
        openai_reply(&v.to_string())
    })
    .await;
    let e = engine(&srv.base);
    let mut inp = input(9, 50);
    inp.sections = paperreader_translate::digest::standard_sections();
    inp.sections.push(SectionSpec {
        id: "contradictions".into(),
        about: "places where the paper contradicts itself".into(),
        max: 3,
    });
    let r = run_digest(
        &e,
        &inp,
        &mut DigestState::default(),
        &Cancel::new(),
        |_| {},
    )
    .await
    .unwrap();
    let c: Vec<&DigestItem> = r
        .items
        .iter()
        .filter(|i| i.section == "contradictions")
        .collect();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].text, "第 1 頁與第 2 頁的數字不同。");
    assert_eq!(c[0].pages, vec![0, 1]);
    assert!(srv.last().body.contains("\\\"contradictions\\\":[…]"));
    // 用量：每個回應都記（假伺服器每次回報輸入 1、輸出 1）
    assert_eq!(
        e.usage(),
        TokenUsage {
            input: 1,
            output: 1
        }
    );
}

#[tokio::test]
async fn a_short_paper_is_one_request_and_refs_become_pages() {
    let srv = serve(|req, _| openai_reply(&fake_notes(&sent(&req.body)))).await;
    let e = engine(&srv.base);
    let mut st = DigestState::default();
    let mut stages = Vec::new();
    let r = run_digest(&e, &input(9, 50), &mut st, &Cancel::new(), |s| {
        stages.push(s)
    })
    .await
    .unwrap();
    assert_eq!(srv.count(), 1);
    assert_eq!(r.parts, 1);
    assert_eq!(stages, vec![DigestStage::Part { n: 1, total: 1 }]);
    assert_eq!(r.dropped, 1, "the point with an unknown id is dropped");
    let sections: Vec<&str> = r.items.iter().map(|i| i.section.as_str()).collect();
    assert_eq!(sections, ["question", "method", "results", "quotes"]);
    let m = &r.items[1];
    assert_eq!(m.segments, vec!["g0001", "g0009"]);
    assert_eq!(m.pages, vec![0, 2]);
    let body = srv.last().body;
    assert!(
        body.contains("Taiwan"),
        "zh-TW output asks for Taiwan usage"
    );
    let msg = sent(&body);
    assert!(
        msg.contains("<title>Paper</title>")
            && msg.contains("<abstract>We study things.</abstract>")
    );
    assert!(msg.contains("<para id=\"g0004\" page=\"2\">"));
}

#[tokio::test]
async fn a_long_paper_is_summarized_in_parts_then_merged() {
    let srv = serve(|req, _| openai_reply(&fake_notes(&sent(&req.body)))).await;
    let e = engine(&srv.base);
    let mut st = DigestState::default();
    let mut stages = Vec::new();
    // 每段約 2000 字元：一部分放得下約 12 段
    let inp = input(30, 2000);
    let r = run_digest(&e, &inp, &mut st, &Cancel::new(), |s| stages.push(s))
        .await
        .unwrap();
    let parts = r.parts;
    assert!(parts >= 2, "{parts}");
    assert_eq!(
        srv.count(),
        parts + 1,
        "one request per part, then one merge"
    );
    assert_eq!(stages.last(), Some(&DigestStage::Merge));
    let merge = sent(&srv.last().body);
    assert!(
        merge.contains("<notes part=\"1\">")
            && merge.contains(&format!("<notes part=\"{parts}\">"))
    );
    assert!(srv.last().body.contains("Merge them"));
    // 每一部分都不超過上限
    for req in srv.all().iter().take(parts) {
        let m = sent(&req.body);
        assert!(
            m.chars().count() < PART_CHARS + 3000,
            "{}",
            m.chars().count()
        );
    }
}

#[tokio::test]
async fn an_unreadable_reply_is_asked_again_once() {
    let srv = serve(|req, n| {
        if n == 1 {
            openai_reply("Sorry, here are the notes: not json")
        } else {
            openai_reply(&fake_notes(&sent(&req.body)))
        }
    })
    .await;
    let e = engine(&srv.base);
    let r = run_digest(
        &e,
        &input(3, 20),
        &mut DigestState::default(),
        &Cancel::new(),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(srv.count(), 2);
    assert!(!r.items.is_empty());

    let srv = serve(|_, _| openai_reply("still not json")).await;
    let e = engine(&srv.base);
    let err = run_digest(
        &e,
        &input(3, 20),
        &mut DigestState::default(),
        &Cancel::new(),
        |_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ErrorKind::BadResponse);
    assert_eq!(srv.count(), 2);
}

#[tokio::test]
async fn after_a_stop_another_service_continues_without_resending_done_parts() {
    // 第一個服務：第一部分成功，之後一直回 503（停下來，可以換備用服務）
    let srv1 = serve(|req, n| {
        if n == 1 {
            openai_reply(&fake_notes(&sent(&req.body)))
        } else {
            Resp::status(503, "{}")
        }
    })
    .await;
    let inp = input(30, 2000);
    let mut st = DigestState::default();
    let err = run_digest(&engine(&srv1.base), &inp, &mut st, &Cancel::new(), |_| {})
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Server);
    assert!(st.parts[0].is_some() && st.parts[1].is_none());
    let total = st.parts.len();

    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = seen.clone();
    let srv2 = serve(move |req, _| {
        let m = sent(&req.body);
        log.lock().unwrap().push(m.clone());
        openai_reply(&fake_notes(&m))
    })
    .await;
    let r = run_digest(&engine(&srv2.base), &inp, &mut st, &Cancel::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(r.parts, total);
    assert_eq!(
        srv2.count(),
        total,
        "the parts left (total - 1) plus the merge"
    );
    assert!(
        !seen.lock().unwrap()[0].contains("<para id=\"g0001\""),
        "part 1 is not sent again"
    );
}

#[tokio::test]
async fn machine_translation_cannot_make_a_digest() {
    let e = Engine::new(&EngineConfig {
        provider: Provider::Deepl,
        model: String::new(),
        endpoint: Some("http://127.0.0.1:9/".into()),
        api_key: "k".into(),
    })
    .unwrap();
    let err = run_digest(
        &e,
        &input(3, 20),
        &mut DigestState::default(),
        &Cancel::new(),
        |_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Config);
}

#[tokio::test]
async fn cancel_stops_before_the_next_part() {
    let srv = serve(|req, _| openai_reply(&fake_notes(&sent(&req.body)))).await;
    let e = engine(&srv.base);
    let cancel = Cancel::new();
    let c2 = cancel.clone();
    let mut st = DigestState::default();
    let err = run_digest(&e, &input(30, 2000), &mut st, &cancel, move |s| {
        if matches!(s, DigestStage::Part { n: 2, .. }) {
            c2.cancel();
        }
    })
    .await
    .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Cancelled);
    assert!(st.parts[0].is_some(), "the finished part is kept");
}
