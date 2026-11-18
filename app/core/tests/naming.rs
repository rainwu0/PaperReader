use paperreader_core::format::{Author, BibDraft};
use paperreader_core::naming::{folder_base_name, unique_name};

fn family(f: &str) -> Author {
    Author {
        family: Some(f.into()),
        ..Default::default()
    }
}
fn literal(l: &str) -> Author {
    Author {
        literal: Some(l.into()),
        ..Default::default()
    }
}
fn draft(author: Option<Author>, year: Option<i32>, title: Option<&str>) -> BibDraft {
    BibDraft {
        title: title.map(String::from),
        authors: author.into_iter().collect(),
        year,
        ..Default::default()
    }
}
fn only_title(t: &str) -> BibDraft {
    draft(None, None, Some(t))
}

#[test]
fn full() {
    let d = draft(
        Some(family("Zhao")),
        Some(2024),
        Some("DETRs Beat YOLOs on Real-time Object Detection"),
    );
    assert_eq!(
        folder_base_name(&d, None),
        "Zhao 2024 - DETRs Beat YOLOs on Real-time Object Detection"
    );
}

#[test]
fn literal_author_and_illegal_chars() {
    let d = draft(Some(literal("RT-DETR Team")), None, Some("A: B/C?"));
    assert_eq!(folder_base_name(&d, None), "RT-DETR Team - A BC");
}

#[test]
fn no_author() {
    let d = draft(None, Some(2025), Some("Survey"));
    assert_eq!(folder_base_name(&d, None), "2025 - Survey");
}

#[test]
fn no_title() {
    let d = draft(Some(family("Lema")), Some(2025), None);
    assert_eq!(folder_base_name(&d, None), "Lema 2025");
}

#[test]
fn origin_name_fallback() {
    let d = BibDraft::default();
    assert_eq!(
        folder_base_name(&d, Some("2304.08069v3.pdf")),
        "2304.08069v3"
    );
}

#[test]
fn untitled() {
    assert_eq!(folder_base_name(&BibDraft::default(), None), "untitled");
}

#[test]
fn strip_dots() {
    assert_eq!(folder_base_name(&only_title("..hidden. "), None), "hidden");
}

#[test]
fn truncate_chars() {
    let t = "a".repeat(100);
    assert_eq!(folder_base_name(&only_title(&t), None), "a".repeat(60));
}

#[test]
fn truncate_cjk() {
    let t = "論".repeat(100);
    assert_eq!(folder_base_name(&only_title(&t), None), "論".repeat(60));
}

#[test]
fn truncate_bytes() {
    let t = "𠮷".repeat(100);
    let got = folder_base_name(&only_title(&t), None);
    assert_eq!(got, "𠮷".repeat(45));
    assert!(got.len() <= 180);
}

#[test]
fn truncate_then_strip() {
    let t = format!("{} {}", "a".repeat(59), "b".repeat(10));
    assert_eq!(folder_base_name(&only_title(&t), None), "a".repeat(59));
}

#[test]
fn collapse_whitespace() {
    assert_eq!(folder_base_name(&only_title("A \t  B"), None), "A B");
}

#[test]
fn reserved_name() {
    assert_eq!(folder_base_name(&only_title("con"), None), "_con");
    assert_eq!(folder_base_name(&only_title("COM1"), None), "_COM1");
    assert_eq!(folder_base_name(&only_title("console"), None), "console");
}

#[test]
fn unique_first() {
    assert_eq!(unique_name(&[], "Paper"), "Paper");
}

#[test]
fn unique_second() {
    assert_eq!(unique_name(&["Paper".to_string()], "Paper"), "Paper (2)");
}

#[test]
fn unique_case_insensitive() {
    assert_eq!(unique_name(&["paper".to_string()], "Paper"), "Paper (2)");
}

#[test]
fn unique_third() {
    let ex = vec!["Paper".to_string(), "Paper (2)".to_string()];
    assert_eq!(unique_name(&ex, "Paper"), "Paper (3)");
}

#[test]
fn truncate_keeps_combining_accent_with_its_letter() {
    // 59 個 a，再接 e 加組合用的重音（U+0065 U+0301），再接 bbb：第 60 個「字」是完整的 é
    let t = format!("{}e\u{301}bbb", "a".repeat(59));
    let got = folder_base_name(&only_title(&t), None);
    assert_eq!(got, format!("{}e\u{301}", "a".repeat(59)));
}

#[test]
fn truncate_never_splits_a_zwj_emoji() {
    // 「家庭」表情符號：用零寬連接符組合，25 個位元組、使用者看到的是一個字
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
    assert_eq!(family.len(), 25);
    let t = family.repeat(10);
    let got = folder_base_name(&only_title(&t), None);
    // 位元組上限 180：最多放 7 個（175 位元組），第 8 個放不下就整個捨去
    assert_eq!(got, family.repeat(7));
}

#[test]
fn truncate_counts_graphemes_not_code_points() {
    // 100 個 e 加重音：每個是一個字（3 位元組）；60 個字 = 180 位元組，剛好
    let t = "e\u{301}".repeat(100);
    let got = folder_base_name(&only_title(&t), None);
    assert_eq!(got, "e\u{301}".repeat(60));
}

#[test]
fn truncate_korean_syllables_decomposed() {
    // 韓文音節用組合字母（NFD）寫：한 = ᄒ ᅡ ᆫ，三個碼位（9 位元組）、一個字。
    // 位元組上限 180 先到：放 20 個，不會停在一個音節的中間。
    let t = "\u{1112}\u{1161}\u{11AB}".repeat(100);
    let got = folder_base_name(&only_title(&t), None);
    assert_eq!(got, "\u{1112}\u{1161}\u{11AB}".repeat(20));
}
