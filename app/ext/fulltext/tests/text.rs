//! 找位置：切詞、查詢的解析、找位置。

use paperreader_core::references::fold_text_mapped;
use paperreader_ext_fulltext::{Term, locate, parse_query, tokens};

fn main_and_joined(s: &str) -> (Vec<String>, Vec<String>) {
    let t = tokens(&fold_text_mapped(s));
    (
        t.main.into_iter().map(|t| t.text).collect(),
        t.joined.into_iter().map(|t| t.text).collect(),
    )
}

fn check(s: &str, main: &[&str], joined: &[&str]) {
    let (m, j) = main_and_joined(s);
    assert_eq!(m, main, "主列：{s:?}");
    assert_eq!(j, joined, "接回列：{s:?}");
}

// ---------- 切詞 ----------

#[test]
fn tokens_appendix_c2() {
    check(
        "Real-time ob-\njects",
        &["real", "time", "ob", "jects"],
        &["objects"],
    );
    check("\u{FB01}ne-tuning", &["fine", "tuning"], &[]);
    check("Ａｐｐｌｅ　ＡＰ５０", &["apple", "ap50"], &[]);
    check("naïve", &["naive"], &[]);
    check(
        "RT-DETR 在 COCO 上達到 53.1 AP",
        &[
            "rt", "detr", "在", "coco", "上達", "達到", "到", "53", "1", "ap",
        ],
        &[],
    );
    check(
        "非極大值抑制（NMS）",
        &["非極", "極大", "大值", "值抑", "抑制", "制", "nms"],
        &[],
    );
    check(
        "我們在COCO上達到",
        &["我們", "們在", "在", "coco", "上達", "達到", "到"],
        &[],
    );
    check(
        "Transformer模型的表現",
        &["transformer", "模型", "型的", "的表", "表現", "現"],
        &[],
    );
    // 範例寫「スー ーパ パー ー モデ デル ル」；fold_text 的去重音（NFD 後拿掉附加符號）連濁音、半濁音的記號
    // 也拿掉（パ → ハ、デ → テ）。以 fold_text 的輸出為準
    check(
        "スーパー・モデル",
        &["スー", "ーハ", "ハー", "ー", "モテ", "テル", "ル"],
        &[],
    );
    check(
        "self-\nsupervised learning",
        &["self", "supervised", "learning"],
        &["selfsupervised"],
    );
    check("end-to-\nend", &["end", "to", "end"], &["toend"]);
    check(
        "state-of-the-\nart",
        &["state", "of", "the", "art"],
        &["theart"],
    );
    check(
        "object detec-\ntion",
        &["object", "detec", "tion"],
        &["detection"],
    );
    check("Let \u{1D465} denote", &["let", "x", "denote"], &[]);
}

#[test]
fn token_ranges_are_char_positions_in_folded_text() {
    let f = fold_text_mapped("模型 ab");
    let t = tokens(&f);
    let r: Vec<(usize, usize)> = t.main.iter().map(|t| (t.start, t.end)).collect();
    assert_eq!(r, vec![(0, 2), (1, 2), (3, 5)]);
}

#[test]
fn joins_that_are_not_inside_a_word_make_no_joined_token() {
    // 句中的「 - 」也會被 fold_text 接起來，接回點前一個字是空白：沒有接回列
    check("a - b", &["a", "b"], &[]);
    // 連字號在開頭
    check("-\nfoo", &["foo"], &[]);
    // 一串裡兩個接回點：主列切三段，接回列一個整串
    check(
        "detec-\ntion-\nbased",
        &["detec", "tion", "based"],
        &["detectionbased"],
    );
    // 一串中文的單字
    check("圖", &["圖"], &[]);
}

// ---------- 查詢的解析 ----------

fn fts(q: &str) -> String {
    parse_query(q).map(|q| q.fts()).unwrap_or_default()
}

#[test]
fn queries_appendix_c3() {
    assert_eq!(fts("極大值"), r#""極大 大值 值" *"#);
    assert_eq!(fts("detect"), r#""detect" *"#);
    assert_eq!(fts("抑制 NMS"), r#""抑制 制" * AND "nms" *"#);
    assert_eq!(fts("\"near field\""), r#""near field" *"#);
    assert_eq!(fts("在COCO上"), r#""在 coco 上" *"#);
    assert_eq!(fts("Transformer模型"), r#""transformer 模型 型" *"#);
    assert_eq!(fts("self-supervised"), r#""self supervised" *"#);
    assert_eq!(fts("selfsupervised"), r#""selfsupervised" *"#);
    assert_eq!(fts("end-to-end"), r#""end to end" *"#);
    assert_eq!(fts("state-of-the-art"), r#""state of the art" *"#);
    assert_eq!(fts("\"real-time detection\""), r#""real time detection" *"#);
    assert_eq!(fts("\"for real-time\""), r#""for real time" *"#);
    assert_eq!(fts("detection"), r#""detection" *"#);
    assert_eq!(fts("\"object detection\""), r#""object detection" *"#);
    assert_eq!(fts("圖 3"), r#""圖" * AND "3""#);
    assert_eq!(fts("\u{1D465} denote"), r#""x" AND "denote" *"#);
}

#[test]
fn queries_shorter_than_two_chars_or_without_tokens_are_not_run() {
    assert!(parse_query("").is_none());
    assert!(parse_query(" a ").is_none());
    assert!(parse_query("圖").is_none());
    assert!(parse_query("--").is_none());
    assert!(parse_query("「」 ！").is_none());
    // 切不出記號的詞丟掉，其餘的照樣查
    assert_eq!(fts("-- detect"), r#""detect" *"#);
}

#[test]
fn only_the_last_token_is_a_prefix_and_not_for_one_letter_or_digits() {
    let q = parse_query("a 12 ab x1").unwrap();
    let flags: Vec<bool> = q.terms.iter().map(|t| t.prefix_last).collect();
    assert_eq!(flags, vec![false, false, true, true]);
    let t: &Term = &parse_query("real time detect").unwrap().terms[2];
    assert_eq!(t.tokens, vec!["detect"]);
}

#[test]
fn quotes_pair_up_and_an_unpaired_one_is_a_space() {
    let q = parse_query("\"deep learning\" model \"open").unwrap();
    let terms: Vec<Vec<String>> = q.terms.iter().map(|t| t.tokens.clone()).collect();
    assert_eq!(
        terms,
        vec![vec!["deep", "learning"], vec!["model"], vec!["open"]]
    );
    // 彎引號、全形引號
    assert_eq!(fts("“near field”"), r#""near field" *"#);
    assert_eq!(fts("＂near field＂"), r#""near field" *"#);
}

#[test]
fn syntax_characters_are_only_characters() {
    assert_eq!(
        fts("a AND b NEAR"),
        r#""a" AND "and" * AND "b" AND "near" *"#
    );
    assert_eq!(fts("-foo* bar"), r#""foo" * AND "bar" *"#);
    assert_eq!(fts("\"x\" OR y"), r#""x" AND "or" * AND "y""#);
}

// ---------- 找位置 ----------

#[test]
fn locate_range_is_utf16_and_quote_is_the_original_text() {
    // 最後一列：range 是 UTF-16 的 [4,6)
    let q = parse_query("\u{1D465} denote").unwrap();
    let l = locate(&q, "Let \u{1D465} denote the input").unwrap();
    assert_eq!(l.range, (4, 6));
    assert_eq!(l.quote, "\u{1D465}");
    // 其餘的詞（denote）在前後文裡標出；context 從頭開始，前面沒截
    assert!(!l.cut_start);
    assert_eq!(l.marks, vec![(4, 6), (7, 13)]);
}

#[test]
fn prefix_marks_whole_word_but_only_first_cjk_char() {
    let q = parse_query("detect").unwrap();
    let l = locate(&q, "object detection works").unwrap();
    assert_eq!(l.quote, "detection");
    let q = parse_query("極大值").unwrap();
    let l = locate(&q, "使用非極大值抑制").unwrap();
    assert_eq!(l.quote, "極大值");
}

#[test]
fn locate_falls_back_to_the_joined_row_and_quotes_across_the_hyphen() {
    let q = parse_query("detection").unwrap();
    let l = locate(&q, "object detec-\ntion here").unwrap();
    assert_eq!(l.quote, "detec-\ntion");
    // 引號片語遇到行尾斷開的詞找不到（已知的限制）
    assert!(
        locate(
            &parse_query("\"object detection\"").unwrap(),
            "object detec-\ntion"
        )
        .is_none()
    );
    // 不加引號的兩個詞找得到
    assert!(
        locate(
            &parse_query("object detection").unwrap(),
            "object detec-\ntion"
        )
        .is_some()
    );
    // 三段以上的複合詞
    let l = locate(&parse_query("end-to-end").unwrap(), "an end-to-\nend model").unwrap();
    assert_eq!(l.quote, "end-to-\nend");
}

#[test]
fn first_term_first_occurrence_and_context_window() {
    let text = format!("{}alpha beta {}alpha", "x ".repeat(40), "y ".repeat(50));
    let q = parse_query("alpha beta").unwrap();
    let l = locate(&q, &text).unwrap();
    assert_eq!(l.range, (80, 85));
    assert!(l.cut_start);
    assert!(l.cut_end);
    assert_eq!(l.context.chars().count(), 30 + 5 + 60);
    // 前後文裡 alpha 與 beta 各標一次（第二個 alpha 在 60 字之外）
    assert_eq!(l.marks, vec![(30, 35), (36, 40)]);
}

#[test]
fn locate_finds_mixed_cjk_without_spaces() {
    for text in ["我們在COCO上達到", "我們在 COCO 上達到"] {
        let l = locate(&parse_query("在COCO上").unwrap(), text).unwrap();
        assert!(
            l.quote.starts_with('在') && l.quote.ends_with('上'),
            "{text}"
        );
    }
    // 一串中文最後一個字
    let l = locate(&parse_query("示意 圖").unwrap(), "見示意圖。").unwrap();
    assert_eq!(l.quote, "示意");
}
