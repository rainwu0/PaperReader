//! 書目匯出（BibTeX、RIS）與譯本的逐段對照 Markdown：純計算，不碰檔案。

use paperreader_core::bibexport::{BibFormat, cite_keys, export_bibliography};
use paperreader_core::format::{Author, Bib};
use paperreader_core::overlay::OverlayFile;
use paperreader_core::translation_md::{translation_markdown, translation_text_markdown};

fn author(family: &str, given: &str) -> Author {
    Author {
        family: Some(family.into()),
        given: Some(given.into()),
        ..Author::default()
    }
}

fn full() -> Bib {
    Bib {
        title: Some("Deep Learning for Vision".into()),
        authors: vec![author("Smith", "Ann"), author("Lee", "Bo")],
        year: Some(2020),
        venue: Some("Journal of Things".into()),
        doi: Some("10.1000/xyz123".into()),
        url: Some("https://example.org/p".into()),
        ..Bib::default()
    }
}

#[test]
fn bibtex_writes_every_field_of_a_full_entry() {
    let b = full();
    let out = export_bibliography(&[&b], BibFormat::Bibtex);
    assert_eq!(
        out,
        "@article{smith2020deep,\n  author = {Smith, Ann and Lee, Bo},\n  title = {Deep Learning for Vision},\n  \
         year = {2020},\n  journal = {Journal of Things},\n  doi = {10.1000/xyz123},\n  url = {https://example.org/p},\n}\n\n"
    );
}

#[test]
fn bibtex_without_venue_is_misc_and_missing_fields_are_not_written() {
    let b = Bib {
        title: Some("Only a title".into()),
        ..Bib::default()
    };
    let out = export_bibliography(&[&b], BibFormat::Bibtex);
    assert_eq!(out, "@misc{anonndonly,\n  title = {Only a title},\n}\n\n");
    assert!(!out.contains("author"));
    assert!(!out.contains("year"));
    assert!(!out.contains("doi"));
}

#[test]
fn bibtex_escapes_special_characters_and_collapses_whitespace() {
    let b = Bib {
        title: Some("R&D: 50% of $x_1 #1 {a}\nsecond ~line^ \\ end".into()),
        authors: vec![Author {
            literal: Some("Research & Co, Inc.".into()),
            ..Author::default()
        }],
        year: Some(2021),
        ..Bib::default()
    };
    let out = export_bibliography(&[&b], BibFormat::Bibtex);
    assert!(
        out.contains(
            "title = {R\\&D: 50\\% of \\$x\\_1 \\#1 \\{a\\} second \\textasciitilde{}line\\textasciicircum{} \\textbackslash{} end}"
        ),
        "{out}"
    );
    // 整串姓名用大括號包起來，不會被拆成姓與名
    assert!(out.contains("author = {{Research \\& Co, Inc.}}"), "{out}");
}

#[test]
fn urls_stay_raw_but_cannot_break_the_braces() {
    let b = Bib {
        title: Some("T".into()),
        url: Some("https://e.org/a_b%20c#x?q={1}".into()),
        ..Bib::default()
    };
    let out = export_bibliography(&[&b], BibFormat::Bibtex);
    assert!(
        out.contains("url = {https://e.org/a_b%20c#x?q=%7B1%7D}"),
        "{out}"
    );
}

#[test]
fn keys_are_first_author_family_year_first_title_word_lowercase() {
    let a = full();
    let literal = Bib {
        title: Some("The Quick Test".into()),
        authors: vec![Author {
            literal: Some("Jane Q. Public".into()),
            ..Author::default()
        }],
        year: Some(1999),
        ..Bib::default()
    };
    let comma = Bib {
        title: Some("Another".into()),
        authors: vec![Author {
            literal: Some("Doe, John".into()),
            ..Author::default()
        }],
        year: Some(2001),
        ..Bib::default()
    };
    let cjk = Bib {
        title: Some("深度學習綜述".into()),
        authors: vec![author("王", "小明")],
        year: Some(2022),
        ..Bib::default()
    };
    let keys = cite_keys(&[&a, &literal, &comma, &cjk]);
    assert_eq!(
        keys,
        [
            "smith2020deep",
            "public1999the",
            "doe2001another",
            "王2022深"
        ]
    );
}

#[test]
fn duplicate_keys_get_letters_in_input_order() {
    let a = full();
    let b = full();
    let c = full();
    let other = Bib {
        year: Some(2019),
        ..full()
    };
    let keys = cite_keys(&[&a, &other, &b, &c]);
    assert_eq!(
        keys,
        [
            "smith2020deepa",
            "smith2019deep",
            "smith2020deepb",
            "smith2020deepc"
        ]
    );
}

#[test]
fn a_letter_suffix_never_collides_with_another_entrys_key() {
    // 「Smith 2020 A…」的鍵值本來就是 smith2020a；兩篇 smith2020 沒有篇名也會產生 smith2020a
    let titled_a = Bib {
        title: Some("A study".into()),
        year: Some(2020),
        authors: vec![author("Smith", "X")],
        ..Bib::default()
    };
    let t1 = Bib {
        title: None,
        ..titled_a.clone()
    };
    let t2 = t1.clone();
    let keys = cite_keys(&[&titled_a, &t1, &t2]);
    let unique: std::collections::HashSet<_> = keys.iter().collect();
    assert_eq!(unique.len(), 3, "{keys:?}");
}

#[test]
fn ris_writes_tagged_lines_and_skips_missing_fields() {
    let b = full();
    let bare = Bib {
        title: Some("Bare\ntitle".into()),
        ..Bib::default()
    };
    let out = export_bibliography(&[&b, &bare], BibFormat::Ris);
    assert_eq!(
        out,
        "TY  - JOUR\nID  - smith2020deep\nAU  - Smith, Ann\nAU  - Lee, Bo\nTI  - Deep Learning for Vision\nPY  - 2020\n\
         JO  - Journal of Things\nDO  - 10.1000/xyz123\nUR  - https://example.org/p\nER  - \n\n\
         TY  - GEN\nID  - anonndbare\nTI  - Bare title\nER  - \n\n"
    );
}

#[test]
fn empty_list_gives_an_empty_file() {
    assert_eq!(export_bibliography(&[], BibFormat::Bibtex), "");
    assert_eq!(export_bibliography(&[], BibFormat::Ris), "");
}

fn overlay(segments: &str) -> OverlayFile {
    serde_json::from_str(&format!(
        r#"{{"format":1,"source_material":"m-1","provider":"anthropic","model":"claude-x","target_lang":"zh-TW","segments":{segments}}}"#
    ))
    .unwrap()
}

#[test]
fn translation_markdown_groups_by_page_and_marks_untranslated() {
    let o = overlay(
        r#"[
        {"id":"g-1","page":1,"bbox":[0,0,1,1],"source":"Second page text","target":"第二頁","status":"done"},
        {"id":"g-2","page":0,"bbox":[0,0,1,1],"source":"First line\nsecond line","target":"第一頁的段落","status":"done"},
        {"id":"g-3","page":0,"bbox":[0,0,1,1],"source":"Pending one","target":null,"status":"pending"},
        {"id":"g-4","page":1,"bbox":[0,0,1,1],"source":"Broken one","target":null,"status":"failed"}
    ]"#,
    );
    let md = translation_markdown("My Paper", &o);
    assert_eq!(
        md,
        "# My Paper\n\n翻譯：anthropic claude-x；目標語言：zh-TW\n\n\
         ## 第 1 頁\n\n> First line\n> second line\n\n第一頁的段落\n\n> Pending one\n\n（尚未翻譯）\n\n\
         ## 第 2 頁\n\n> Second page text\n\n第二頁\n\n> Broken one\n\n（翻譯失敗）\n\n"
    );
}

#[test]
fn translation_markdown_without_title_or_segments_still_has_a_header() {
    let o = overlay("[]");
    let md = translation_markdown("  ", &o);
    assert!(md.starts_with("# 譯本對照\n\n"), "{md}");
    assert!(!md.contains("## 第"));
}

// ---- 只有譯文 ----

#[test]
fn translation_text_has_only_the_translation_in_reading_order() {
    let o = overlay(
        r#"[
        {"id":"g-1","page":1,"bbox":[0,0,1,1],"source":"Second page text","target":"第二頁","status":"done"},
        {"id":"g-2","page":0,"bbox":[0,0,1,1],"kind":"heading","source":"1 Introduction","target":"一、引言","status":"done"},
        {"id":"g-3","page":0,"bbox":[0,0,1,1],"source":"First line\nsecond line","target":"第一頁的段落\n第二行","status":"done"},
        {"id":"g-4","page":1,"bbox":[0,0,1,1],"kind":"caption","source":"Figure 1. A cat.","target":"圖 1：一隻貓。","status":"done"}
    ]"#,
    );
    let md = translation_text_markdown("My Paper", &o);
    assert_eq!(
        md,
        "# My Paper\n\n翻譯：anthropic claude-x；目標語言：zh-TW\n\n\
         ## 一、引言\n\n第一頁的段落\n第二行\n\n第二頁\n\n圖 1：一隻貓。\n"
    );
    // 不夾原文、不分頁
    for no in [
        "Introduction",
        "First line",
        "Second page text",
        "Figure 1",
        "> ",
        "## 第",
    ] {
        assert!(!md.contains(no), "不該有 {no:?}：{md}");
    }
}

#[test]
fn translation_text_lists_untranslated_segments_as_the_source_text() {
    let o = overlay(
        r#"[
        {"id":"g-1","page":0,"bbox":[0,0,1,1],"source":"Translated one","target":"翻好的","status":"done"},
        {"id":"g-2","page":0,"bbox":[0,0,1,1],"source":"Pending one","target":null,"status":"pending"},
        {"id":"g-3","page":0,"bbox":[0,0,1,1],"kind":"heading","source":"2 Method","target":null,"status":"failed"},
        {"id":"g-4","page":0,"bbox":[0,0,1,1],"source":"Blank target","target":"  ","status":"done"}
    ]"#,
    );
    let md = translation_text_markdown("P", &o);
    assert_eq!(
        md,
        "# P\n\n翻譯：anthropic claude-x；目標語言：zh-TW\n\n\
         翻好的\n\nPending one\n\n## 2 Method\n\nBlank target\n"
    );
    assert!(!md.contains("Translated one"));
}

#[test]
fn translation_text_without_title_or_segments_still_has_a_header() {
    let o = overlay("[]");
    let md = translation_text_markdown("  ", &o);
    assert_eq!(md, "# 譯文\n\n翻譯：anthropic claude-x；目標語言：zh-TW\n");
}

#[test]
fn the_bilingual_version_is_unchanged_by_the_translation_only_one() {
    // 同一份疊加檔：對照版仍有原文的引用區塊與頁的標題
    let o = overlay(
        r#"[{"id":"g-1","page":0,"bbox":[0,0,1,1],"source":"Hello","target":"你好","status":"done"}]"#,
    );
    assert_eq!(
        translation_markdown("T", &o),
        "# T\n\n翻譯：anthropic claude-x；目標語言：zh-TW\n\n## 第 1 頁\n\n> Hello\n\n你好\n\n"
    );
    assert_eq!(
        translation_text_markdown("T", &o),
        "# T\n\n翻譯：anthropic claude-x；目標語言：zh-TW\n\n你好\n"
    );
}
