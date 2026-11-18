//! 書目的 CSL-JSON 與引用格式。
//!
//! CSL-JSON 是引用格式工具通用的書目資料格式（Zotero、Pandoc 讀得懂；結構描述以 MIT 授權發布，
//! <https://github.com/citation-style-language/schema>）。引用格式用 CSL（Citation Style Language）：
//! 書目先轉成 CSL-JSON 的條目，交給 hayagriva 依 `csl/` 裡的格式檔排版。格式檔與語系檔是 CSL 專案的，
//! CC BY-SA 3.0，原樣內含、不修改（見 `app/core/csl/README.md`）。
//! 純計算：不讀不寫檔案。和 BibTeX、RIS（`bibexport`）一樣是書目的排版規則，所以放在核心，
//! 外殼只轉接。

use crate::bibexport::cite_keys;
use crate::format::{Author, Bib};
use hayagriva::citationberg::json::Item;
use hayagriva::citationberg::{
    FontStyle, FontVariant, FontWeight, IndependentStyle, Locale, LocaleFile, VerticalAlign,
};
use hayagriva::{
    BibliographyDriver, BibliographyRequest, CitationItem, CitationRequest, ElemChild, Formatting,
};
use serde_json::{Map, Value, json};

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn csl_name(a: &Author) -> Option<Value> {
    let family = non_empty(a.family.as_deref());
    let given = non_empty(a.given.as_deref());
    let mut m = Map::new();
    match (family, given) {
        (None, None) => {
            m.insert("literal".into(), non_empty(a.literal.as_deref())?.into());
        }
        (f, g) => {
            if let Some(f) = f {
                m.insert("family".into(), f.into());
            }
            if let Some(g) = g {
                m.insert("given".into(), g.into());
            }
        }
    }
    Some(Value::Object(m))
}

/// 一批書目寫成 CSL-JSON（一個陣列），順序照輸入；`id` 與 BibTeX 的鍵值相同。沒有的欄位不寫。
pub fn csl_json(entries: &[&Bib]) -> String {
    let mut s = serde_json::to_string_pretty(&Value::Array(csl_items(entries)))
        .unwrap_or_else(|_| "[]".into());
    s.push('\n');
    s
}

fn csl_items(entries: &[&Bib]) -> Vec<Value> {
    let keys = cite_keys(entries);
    entries
        .iter()
        .zip(keys)
        .map(|(bib, id)| {
            let venue = non_empty(bib.venue.as_deref());
            let mut m = Map::new();
            m.insert("id".into(), id.into());
            m.insert(
                "type".into(),
                // 沒有期刊就是一般的「文件」：`article` 在 CSL 是預印本，Chicago 會印出 Preprint
                if venue.is_some() {
                    "article-journal"
                } else {
                    "document"
                }
                .into(),
            );
            if let Some(t) = non_empty(bib.title.as_deref()) {
                m.insert("title".into(), t.into());
            }
            let authors: Vec<Value> = bib.authors.iter().filter_map(csl_name).collect();
            if !authors.is_empty() {
                m.insert("author".into(), authors.into());
            }
            if let Some(y) = bib.year {
                m.insert("issued".into(), json!({ "date-parts": [[y]] }));
            }
            if let Some(v) = venue {
                m.insert("container-title".into(), v.into());
            }
            if let Some(d) = non_empty(bib.doi.as_deref()) {
                m.insert("DOI".into(), d.into());
            }
            if let Some(u) = non_empty(bib.url.as_deref()) {
                m.insert("URL".into(), u.into());
            }
            Value::Object(m)
        })
        .collect()
}

/// 至少有一位作者（有姓、名或整串姓名）。
pub fn has_author(bib: &Bib) -> bool {
    bib.authors.iter().any(|a| csl_name(a).is_some())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CitationStyle {
    Apa,
    Ieee,
    Chicago,
    Mla,
}

impl CitationStyle {
    pub fn from_name(name: &str) -> Option<CitationStyle> {
        match name {
            "apa" => Some(CitationStyle::Apa),
            "ieee" => Some(CitationStyle::Ieee),
            "chicago" => Some(CitationStyle::Chicago),
            "mla" => Some(CitationStyle::Mla),
            _ => None,
        }
    }

    /// 檔名裡用的名稱。
    pub fn label(self) -> &'static str {
        match self {
            CitationStyle::Apa => "APA",
            CitationStyle::Ieee => "IEEE",
            CitationStyle::Chicago => "Chicago",
            CitationStyle::Mla => "MLA",
        }
    }

    /// 格式檔（CSL 專案原樣的檔案，見 `csl/README.md`）。
    fn source(self) -> &'static str {
        match self {
            CitationStyle::Apa => include_str!("../csl/apa.csl"),
            CitationStyle::Ieee => include_str!("../csl/ieee.csl"),
            CitationStyle::Chicago => include_str!("../csl/chicago-author-date.csl"),
            CitationStyle::Mla => include_str!("../csl/modern-language-association.csl"),
        }
    }
}

fn locales() -> Vec<Locale> {
    [
        include_str!("../csl/locales-en-US.xml"),
        include_str!("../csl/locales-zh-TW.xml"),
    ]
    .iter()
    .filter_map(|x| LocaleFile::from_xml(x).ok().map(Into::into))
    .collect()
}

/// 排好的引用清單：每一條一份純文字、一份 HTML 片段（斜體等用標籤）。
#[derive(Debug, Default)]
pub struct Rendered {
    pub items: Vec<(String, String)>,
}

impl Rendered {
    /// 純文字：一條一行。
    pub fn text(&self) -> String {
        let lines: Vec<&str> = self.items.iter().map(|(t, _)| t.as_str()).collect();
        lines.join("\n")
    }

    /// 放進剪貼簿的 HTML：一條一段。
    pub fn html(&self) -> String {
        self.items
            .iter()
            .map(|(_, h)| format!("<p>{h}</p>"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 一份完整的 HTML 檔（瀏覽器、文書軟體都打得開）。
    pub fn html_document(&self, title: &str) -> String {
        format!(
            "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{}</title>\n\
             <style>p{{padding-left:2em;text-indent:-2em;margin:0 0 .6em}}</style></head>\n\
             <body>\n{}\n</body></html>\n",
            escape(title),
            self.html()
        )
    }
}

/// 依格式排出引用清單。順序照格式的規則（APA、Chicago、MLA 依作者排序；IEEE 照輸入的順序編號）。
/// 缺的欄位由格式自己略過（例如沒有年份寫 n.d.）。
pub fn render_citations(entries: &[&Bib], style: CitationStyle) -> Rendered {
    let Ok(style) = IndependentStyle::from_xml(style.source()) else {
        return Rendered::default();
    };
    let locales = locales();
    let items: Vec<Item> = csl_items(entries)
        .into_iter()
        .map(keep_title_case)
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    let mut driver = BibliographyDriver::new();
    for item in &items {
        driver.citation(CitationRequest::from_items(
            vec![CitationItem::with_entry(item)],
            &style,
            &locales,
        ));
    }
    let result = driver.finish(BibliographyRequest::new(&style, None, &locales));
    let Some(bib) = result.bibliography else {
        return Rendered::default();
    };
    let items = bib
        .items
        .iter()
        .map(|it| {
            let mut text = String::new();
            let mut html = String::new();
            if let Some(first) = &it.first_field {
                write_child(first, &mut text, &mut html);
                text.push(' ');
                html.push(' ');
            }
            for c in &it.content.0 {
                write_child(c, &mut text, &mut html);
            }
            (tidy(&text), tidy(&html))
        })
        .collect();
    Rendered { items }
}

/// 篇名裡有大小寫混合的詞（DETRs、YOLOs、LoRA、iPhone）時，不讓格式改它的大小寫。
/// CSL 規格要這類詞原樣保留，hayagriva 的標題式大小寫卻只保留全大寫的詞（CNN 不受影響，「DETRs」會變「Detrs」）。
/// 做法是把這一條標成非英文（`language: und`）：hayagriva 只對英文條目做標題式大小寫。
fn keep_title_case(mut v: Value) -> Value {
    let mixed = v["title"].as_str().is_some_and(|t| {
        t.split(|c: char| !c.is_alphanumeric())
            .any(|w| w.chars().skip(1).any(char::is_uppercase) && w.chars().any(char::is_lowercase))
    });
    if mixed && let Some(m) = v.as_object_mut() {
        m.insert("language".into(), "und".into());
    }
    v
}

/// 欄位空著時 hayagriva 會留下前一個分隔號，收成一個標點：Chicago 沒有卷期頁時的「學刊,.」、
/// 一般文件（`document`）在 APA 的「(2024,)」、在 Chicago 篇名後的「.”.」。
fn tidy(s: &str) -> String {
    s.trim()
        .replace(",.", ".")
        .replace(",,", ",")
        .replace(",)", ")")
        .replace(".\u{201d}.", ".\u{201d}")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 斜體、粗體、小型大寫、上下標寫成 HTML 標籤。
fn tags(f: &Formatting) -> (String, String) {
    let mut open = String::new();
    let mut close: Vec<&str> = Vec::new();
    if f.font_style == FontStyle::Italic {
        open.push_str("<i>");
        close.push("</i>");
    }
    if f.font_weight == FontWeight::Bold {
        open.push_str("<b>");
        close.push("</b>");
    }
    if f.font_variant == FontVariant::SmallCaps {
        open.push_str("<span style=\"font-variant:small-caps\">");
        close.push("</span>");
    }
    match f.vertical_align {
        VerticalAlign::Sup => {
            open.push_str("<sup>");
            close.push("</sup>");
        }
        VerticalAlign::Sub => {
            open.push_str("<sub>");
            close.push("</sub>");
        }
        _ => {}
    }
    close.reverse();
    (open, close.concat())
}

/// 一段排好的內容寫成純文字與 HTML。hayagriva 內建的 HTML 輸出不跳脫文字，所以自己寫。
fn write_child(c: &ElemChild, text: &mut String, html: &mut String) {
    match c {
        ElemChild::Text(t) => {
            let (o, e) = tags(&t.formatting);
            text.push_str(&t.text);
            html.push_str(&o);
            html.push_str(&escape(&t.text));
            html.push_str(&e);
        }
        ElemChild::Elem(e) => {
            for c in &e.children.0 {
                write_child(c, text, html);
            }
        }
        ElemChild::Markup(m) => {
            text.push_str(m);
            html.push_str(&escape(m));
        }
        ElemChild::Link { text: t, url } => {
            let (o, e) = tags(&t.formatting);
            text.push_str(&t.text);
            html.push_str(&format!("<a href=\"{}\">", escape(url)));
            html.push_str(&o);
            html.push_str(&escape(&t.text));
            html.push_str(&e);
            html.push_str("</a>");
        }
        ElemChild::Transparent { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn author(family: Option<&str>, given: Option<&str>, literal: Option<&str>) -> Author {
        Author {
            family: family.map(Into::into),
            given: given.map(Into::into),
            literal: literal.map(Into::into),
            ..Default::default()
        }
    }

    fn vaswani() -> Bib {
        Bib {
            title: Some("Attention is all you need".into()),
            authors: vec![
                author(Some("Vaswani"), Some("Ashish"), None),
                author(Some("Shazeer"), Some("Noam"), None),
            ],
            year: Some(2017),
            venue: Some("Advances in Neural Information Processing Systems".into()),
            doi: Some("10.5555/3295222.3295349".into()),
            ..Default::default()
        }
    }

    fn chinese() -> Bib {
        Bib {
            title: Some("深度學習於電路板瑕疵檢測之研究".into()),
            authors: vec![
                author(Some("王"), Some("小明"), None),
                author(None, None, Some("李大華")),
            ],
            year: Some(2021),
            venue: Some("電機工程學刊".into()),
            ..Default::default()
        }
    }

    fn title_only() -> Bib {
        Bib {
            title: Some("A paper with only a title".into()),
            ..Default::default()
        }
    }

    fn render(style: CitationStyle) -> Rendered {
        let (a, b, c) = (vaswani(), chinese(), title_only());
        let r = render_citations(&[&a, &b, &c], style);
        assert_eq!(r.items.len(), 3, "{style:?}: {:?}", r.items);
        r
    }

    fn find<'a>(r: &'a Rendered, needle: &str) -> &'a (String, String) {
        r.items
            .iter()
            .find(|(t, _)| t.contains(needle))
            .unwrap_or_else(|| panic!("找不到 {needle}：{:?}", r.items))
    }

    #[test]
    fn csl_json_full_and_missing_fields() {
        let full = Bib {
            title: Some("Attention Is All You Need".into()),
            authors: vec![
                author(Some("Vaswani"), Some("Ashish"), None),
                author(None, None, Some("王小明")),
            ],
            year: Some(2017),
            venue: Some("NeurIPS".into()),
            doi: Some("10.1/x".into()),
            url: Some("https://example.org/a".into()),
            ..Default::default()
        };
        let bare = Bib {
            title: Some("只有篇名".into()),
            ..Default::default()
        };
        let v: Value = serde_json::from_str(&csl_json(&[&full, &bare])).unwrap();
        let a = &v[0];
        assert_eq!(a["id"], "vaswani2017attention");
        assert_eq!(a["type"], "article-journal");
        assert_eq!(
            a["author"][0],
            json!({"family": "Vaswani", "given": "Ashish"})
        );
        assert_eq!(a["author"][1], json!({"literal": "王小明"}));
        assert_eq!(a["issued"], json!({"date-parts": [[2017]]}));
        assert_eq!(a["container-title"], "NeurIPS");
        assert_eq!(a["DOI"], "10.1/x");
        assert_eq!(a["URL"], "https://example.org/a");
        let b = v[1].as_object().unwrap();
        assert_eq!(b["type"], "document");
        assert_eq!(b["title"], "只有篇名");
        assert!(!b.contains_key("author") && !b.contains_key("issued") && !b.contains_key("DOI"));
    }

    #[test]
    fn csl_json_empty_list() {
        assert_eq!(csl_json(&[]).trim(), "[]");
    }

    #[test]
    fn has_author_ignores_blank_names() {
        let mut b = title_only();
        assert!(!has_author(&b));
        b.authors = vec![author(Some(" "), None, Some(""))];
        assert!(!has_author(&b));
        b.authors = vec![author(None, None, Some("王小明"))];
        assert!(has_author(&b));
    }

    /// 只有篇名的文獻不被說成預印本（CSL 的 `article` 會讓 Chicago 印出 Preprint）。
    #[test]
    fn title_only_is_not_called_a_preprint() {
        for style in [
            CitationStyle::Apa,
            CitationStyle::Ieee,
            CitationStyle::Chicago,
            CitationStyle::Mla,
        ] {
            let r = render_citations(&[&title_only()], style);
            let t = &r.items[0].0;
            assert!(
                t.contains("only a title") || t.contains("Only a Title"),
                "{style:?}: {t}"
            );
            assert!(!t.to_lowercase().contains("preprint"), "{style:?}: {t}");
        }
        // 有作者與年份、沒有期刊的一般文件：不留多餘的標點
        let b = Bib {
            title: Some("DETRs-Beat-YOLOs".into()),
            authors: vec![author(Some("Zhao"), Some("Yian"), None)],
            year: Some(2024),
            ..Default::default()
        };
        for style in [
            CitationStyle::Apa,
            CitationStyle::Ieee,
            CitationStyle::Chicago,
            CitationStyle::Mla,
        ] {
            let t = &render_citations(&[&b], style).items[0].0;
            for bad in [",)", ",.", ".\u{201d}.", ",,"] {
                assert!(!t.contains(bad), "{style:?}: {t}");
            }
        }
    }

    #[test]
    fn apa() {
        let r = render(CitationStyle::Apa);
        let (t, h) = find(&r, "Vaswani");
        assert!(t.starts_with("Vaswani, A., & Shazeer, N. (2017)."), "{t}");
        assert!(
            h.contains("<i>Advances in Neural Information Processing Systems</i>"),
            "{h}"
        );
        assert!(t.contains("https://doi.org/10.5555/3295222.3295349"), "{t}");
        let (t, _) = find(&r, "電路板");
        assert!(
            t.contains("王小明") && t.contains("李大華") && t.contains("(2021)"),
            "{t}"
        );
        // 缺作者與年份：篇名移到前面、寫 n.d.
        let (t, _) = find(&r, "only a title");
        assert!(t.starts_with("A paper with only a title"), "{t}");
        assert!(t.contains("n.d."), "{t}");
    }

    #[test]
    fn ieee() {
        let (a, b, c) = (vaswani(), chinese(), title_only());
        let r = render_citations(&[&a, &b, &c], CitationStyle::Ieee);
        // 照輸入的順序編號
        assert!(
            r.items[0].0.starts_with("[1] A. Vaswani and N. Shazeer"),
            "{:?}",
            r.items
        );
        assert!(
            r.items[1].0.starts_with("[2]") && r.items[1].0.contains("王小明"),
            "{:?}",
            r.items
        );
        assert!(
            r.items[2].0.starts_with("[3]") && !r.items[2].0.contains("n.d."),
            "{:?}",
            r.items
        );
        assert!(r.items[0].1.contains("<i>"), "{:?}", r.items[0].1);
    }

    #[test]
    fn chicago() {
        let r = render(CitationStyle::Chicago);
        let (t, h) = find(&r, "Vaswani");
        assert!(
            t.starts_with("Vaswani, Ashish, and Noam Shazeer. 2017."),
            "{t}"
        );
        assert!(
            h.contains("<i>Advances in Neural Information Processing Systems</i>"),
            "{h}"
        );
        let (t, _) = find(&r, "電路板");
        assert!(t.contains("王小明") && t.contains("2021"), "{t}");
        // 有 DOI、沒有卷期頁的期刊文章寫 ahead of print（CMOS18 14.75）；不留「,.」
        assert!(
            find(&r, "Vaswani")
                .0
                .contains("Systems, ahead of print. https://doi.org/")
        );
        assert!(
            !r.items.iter().any(|(t, _)| t.contains(",.")),
            "{:?}",
            r.items
        );
        let (t, _) = find(&r, "Only a Title");
        assert!(t.contains("n.d."), "{t}");
    }

    #[test]
    fn mla() {
        let r = render(CitationStyle::Mla);
        let (t, h) = find(&r, "Vaswani");
        assert!(t.starts_with("Vaswani, Ashish, and Noam Shazeer."), "{t}");
        assert!(
            h.contains("<i>Advances in Neural Information Processing Systems</i>"),
            "{h}"
        );
        let (t, _) = find(&r, "電路板");
        assert!(t.contains("王小明"), "{t}");
        let (t, _) = find(&r, "Only a Title");
        assert!(t.starts_with("\u{201c}A Paper with Only a Title"), "{t}");
    }

    #[test]
    fn mixed_case_words_survive_title_case() {
        let b = Bib {
            title: Some("DETRs beat YOLOs on real-time object detection".into()),
            authors: vec![author(Some("Zhao"), Some("Yian"), None)],
            year: Some(2024),
            ..Default::default()
        };
        for style in [CitationStyle::Chicago, CitationStyle::Mla] {
            let r = render_citations(&[&b], style);
            assert!(
                r.items[0].0.contains("DETRs beat YOLOs"),
                "{style:?}: {:?}",
                r.items
            );
        }
        // 只有全大寫縮寫（CNN）的照常改成標題式大小寫，縮寫保留
        let cnn = Bib {
            title: Some("a survey of CNN architectures".into()),
            authors: vec![author(Some("Doe"), Some("Jane"), None)],
            year: Some(2020),
            ..Default::default()
        };
        let r = render_citations(&[&cnn], CitationStyle::Mla);
        assert!(
            r.items[0].0.contains("A Survey of CNN Architectures"),
            "{:?}",
            r.items
        );
        // 沒有這類詞的照常改成標題式大小寫
        let r = render_citations(&[&vaswani()], CitationStyle::Mla);
        assert!(
            r.items[0].0.contains("Attention Is All You Need"),
            "{:?}",
            r.items
        );
    }

    #[test]
    fn html_escapes_text() {
        let b = Bib {
            title: Some("Fish & <chips>".into()),
            authors: vec![author(Some("Doe"), Some("Jane"), None)],
            year: Some(2020),
            ..Default::default()
        };
        let r = render_citations(&[&b], CitationStyle::Apa);
        assert!(
            r.items[0].1.contains("&amp;") && r.items[0].1.contains("&lt;chips&gt;"),
            "{:?}",
            r.items
        );
        assert!(r.items[0].0.contains("Fish & <chips>"), "{:?}", r.items);
        assert!(r.html_document("清單").starts_with("<!doctype html>"));
    }
}
