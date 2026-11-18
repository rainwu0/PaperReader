//! 重點筆記：把一篇論文的段落交給對話模型，依預設組的區段整理（內建「標準」是
//! 研究問題、方法、主要結果、限制、值得記下的引句或數字）；**每一點帶它依據的段落代號**，由這裡換成頁碼。
//!
//! - 輸入是畫面用 `layout.ts` 分好的段落（不含參考文獻區與被略過的文字），每段有代號與頁序；
//!   篇名、摘要另外帶給模型當前後文。送出前濾掉控制字元（`text.rs`）。
//! - 提示詞：預設組的指示與區段是使用者改得到的；語言的規則、回覆的 JSON 形狀（照區段產生）、
//!   分部分與合併的說明、術語是固定的。各部分由 [`digest_parts`] 產生，「看完整提示詞」列的是同一份。
//! - 長論文分段：一部分最多 `PART_CHARS` 個字元，每部分各做一份，再請模型合併成一份（只有一部分就不合併）。
//!   做好的部分記在 [`DigestState`]，停下來換服務接著做時不重送。
//! - 回覆是 JSON（每一點 `{"text", "refs"}`）。不認得的段落代號拿掉；一個都對不上的那一點整點不要
//!   （不能寫出沒有頁碼的一點）。讀不懂的回覆再要一次，仍讀不懂就停下來（`BadResponse`）。
//! - 輸出的語言跟著翻譯的目標語言；繁體中文用臺灣用語，中文的半形標點換成全形。術語和翻譯同一個寫法：
//!   使用者的術語庫與這篇譯本已定的譯法（術語庫優先），重點筆記與譯本的用詞一致。
//!
//! 這裡不碰檔案與文獻庫：結果交給外殼，由外殼交給核心檢查、寫入。

use crate::error::{ErrorKind, Result, TranslateError};
use crate::prompt::{
    DEFAULT_DIGEST_PRESET, PromptPart, PromptPreview, builtin_preset, glossary_block, join,
    language_name, language_rules, render_template,
};
use crate::text::{fullwidth_punct, is_chinese, strip_controls};
use crate::{Cancel, Engine, Params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

/// 一個區段：代號（回覆 JSON 的鍵）、要寫什麼（給模型的說明，可含變數）、最多幾點。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionSpec {
    pub id: String,
    pub about: String,
    pub max: usize,
}

/// 內建「標準」的區段。
pub fn standard_sections() -> Vec<SectionSpec> {
    builtin_preset(DEFAULT_DIGEST_PRESET)
        .map(|b| {
            b.sections
                .iter()
                .map(|s| SectionSpec {
                    id: s.id.to_string(),
                    about: s.about.to_string(),
                    max: s.max,
                })
                .collect()
        })
        .unwrap_or_default()
}
/// 一部分最多幾個字元（約 1.2 萬詞元：十頁上下的論文一次送完，十五頁以上才分部分再合併）。
pub const PART_CHARS: usize = 48_000;
/// 一點的文字最多幾個字（超過截斷）、最多幾個段落代號、幾個頁碼。
const MAX_TEXT: usize = 600;
const MAX_REFS: usize = 6;
const MAX_PAGES: usize = 8;
/// 摘要最多帶多少字元。
const MAX_ABSTRACT: usize = 3000;

/// 一段（畫面分好的）。`page` 是原文的頁序，從 0 起算。
#[derive(Debug, Clone, PartialEq)]
pub struct DigestSegment {
    pub id: String,
    pub page: u32,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct DigestInput {
    pub title: Option<String>,
    pub abstract_text: Option<String>,
    pub segments: Vec<DigestSegment>,
    /// 輸出的語言（翻譯的目標語言）。
    pub target_lang: String,
    /// 預設組的指示（可含 `{{from}}`、`{{to}}`、`{{title}}`）；空白用內建的「標準」。
    pub instructions: String,
    /// 區段，依輸出的順序；空的用內建「標準」的五區。
    pub sections: Vec<SectionSpec>,
    /// 使用者的術語庫（原文詞 → 譯法）。
    pub glossary: Vec<(String, String)>,
    /// 這篇譯本已定的譯法（翻譯時記下的，`learn_terms`）；同一個原文詞以術語庫為準。
    pub paper_terms: Vec<(String, String)>,
}

impl DigestInput {
    /// 實際用的區段。
    pub fn resolved_sections(&self) -> Vec<SectionSpec> {
        if self.sections.is_empty() {
            standard_sections()
        } else {
            self.sections.clone()
        }
    }

    /// 送出的內容一律是去掉控制字元之後的。
    fn cleaned(&self) -> DigestInput {
        DigestInput {
            title: self.title.as_deref().map(strip_controls),
            abstract_text: self.abstract_text.as_deref().map(strip_controls),
            segments: self
                .segments
                .iter()
                .map(|s| DigestSegment {
                    text: strip_controls(&s.text),
                    ..s.clone()
                })
                .collect(),
            sections: self.resolved_sections(),
            ..self.clone()
        }
    }
}

/// 整理好的一點：依據的段落代號與頁序（從 0 起算，排好、不重複）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DigestItem {
    pub section: String,
    pub text: String,
    pub segments: Vec<String>,
    pub pages: Vec<u32>,
}

/// 做到哪裡：每一部分做好的點。停下來之後換服務接著做時沿用。
#[derive(Debug, Clone, Default)]
pub struct DigestState {
    pub parts: Vec<Option<Vec<DigestItem>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestStage {
    /// 正在做第 `n` 部分（從 1 起算），共 `total` 部分。
    Part { n: usize, total: usize },
    /// 正在合併各部分。
    Merge,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DigestResult {
    pub items: Vec<DigestItem>,
    /// 段落代號對不上而拿掉的點數。
    pub dropped: usize,
    /// 分成幾部分。
    pub parts: usize,
}

/// 依閱讀順序把段落分成幾部分（每部分最多 `max_chars` 個字元；一段本身太長就自己一部分）。
pub fn split_parts(segments: &[DigestSegment], max_chars: usize) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = 0;
    for (i, s) in segments.iter().enumerate() {
        let n = s.text.chars().count();
        if i > start && chars + n > max_chars {
            out.push(start..i);
            start = i;
            chars = 0;
        }
        chars += n;
    }
    if start < segments.len() {
        out.push(start..segments.len());
    }
    out
}

/// 系統提示詞的各部分（`stage` 是第幾部分或合併；`texts` 是這次送出的原文，術語只放其中出現的）。
/// 送出的就是 [`join`] 接起來的；「看完整提示詞」照同一份列出。
pub fn digest_parts(input: &DigestInput, stage: DigestStage, texts: &[&str]) -> Vec<PromptPart> {
    let lang = input.target_lang.as_str();
    let params = Params {
        target_lang: lang.to_string(),
        title: input.title.clone(),
        ..Params::default()
    };
    let instructions = if input.instructions.trim().is_empty() {
        builtin_preset(DEFAULT_DIGEST_PRESET).map_or("", |b| b.text)
    } else {
        input.instructions.as_str()
    };
    let sections = input.resolved_sections();

    let mut rules = format!("Write every note in {}.\n", language_name(lang));
    rules.push_str(&language_rules(lang));
    let mut list = String::from("Sections:\n");
    for s in &sections {
        list.push_str(&format!(
            "- {}: {} (at most {} items)\n",
            s.id,
            render_template(s.about.trim(), &params),
            s.max
        ));
    }
    let shape: Vec<String> = sections
        .iter()
        .map(|s| format!("\"{}\":[…]", s.id))
        .collect();
    let format = format!(
        "Reply with JSON only (no explanation, no code fence) in exactly this shape:\n{{{}}}\n\
Every item is {{\"text\":\"…\",\"refs\":[\"g0012\"]}}. refs are the ids of the paragraphs that support the item, \
1 to 4 of them, copied exactly from the input.\n\
Write each text on one line. Do not use LaTeX or backslashes in text: write symbols in Unicode (for example α, ≤, ²) or in words.\n",
        shape.join(",")
    );
    let stage = match stage {
        DigestStage::Part { n, total } if total > 1 => format!(
            "The paper is long, so it is sent in {total} parts. This is part {n} of {total}: note only what this part says; \
any of the lists may be empty when this part says nothing about it.\n"
        ),
        DigestStage::Part { .. } => String::new(),
        DigestStage::Merge => "You get notes that were made separately from consecutive parts of the same paper. Merge them into one set of \
notes for the whole paper: remove duplicates, keep the most important items within the limits above, and keep the refs \
of every item you keep (when you combine items, combine their refs, at most 4).\n"
            .to_string(),
    };
    vec![
        PromptPart::new(
            "preset",
            false,
            format!("{}\n", render_template(instructions.trim_end(), &params)),
        ),
        PromptPart::new("rules", true, rules),
        PromptPart::new("sections", false, list),
        PromptPart::new("format", true, format),
        PromptPart::new("stage", true, stage),
        PromptPart::new(
            "glossary",
            true,
            glossary_block(&input.glossary, &input.paper_terms, texts),
        ),
    ]
}

/// 一部分的系統提示詞。
pub fn part_prompt(
    input: &DigestInput,
    n: usize,
    total: usize,
    range: std::ops::Range<usize>,
) -> String {
    let texts: Vec<&str> = input.segments[range]
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    join(&digest_parts(input, DigestStage::Part { n, total }, &texts))
}

/// 合併的系統提示詞（術語照整篇的原文挑）。
pub fn merge_prompt(input: &DigestInput) -> String {
    let texts: Vec<&str> = input.segments.iter().map(|s| s.text.as_str()).collect();
    join(&digest_parts(input, DigestStage::Merge, &texts))
}

/// 整篇一次送出時的完整內容（「看完整提示詞」）：和實際送出的第一部分逐字相同（只有一部分時就是全部）。
pub fn digest_preview(input: &DigestInput) -> PromptPreview {
    let input = input.cleaned();
    let ranges = split_parts(&input.segments, PART_CHARS);
    let total = ranges.len().max(1);
    let range = ranges.first().cloned().unwrap_or(0..0);
    let texts: Vec<&str> = input.segments[range.clone()]
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    PromptPreview {
        kind: "digest",
        system: digest_parts(&input, DigestStage::Part { n: 1, total }, &texts),
        user: vec![PromptPart::new("input", true, part_message(&input, range))],
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn context(input: &DigestInput) -> String {
    let mut s = String::new();
    if let Some(t) = input.title.as_deref().filter(|t| !t.trim().is_empty()) {
        s.push_str(&format!("<title>{}</title>\n", escape(t.trim())));
    }
    if let Some(a) = input
        .abstract_text
        .as_deref()
        .filter(|a| !a.trim().is_empty())
    {
        let a: String = a.trim().chars().take(MAX_ABSTRACT).collect();
        s.push_str(&format!("<abstract>{}</abstract>\n", escape(&a)));
    }
    s
}

/// 一部分的使用者訊息：篇名、摘要、`<para id="…" page="…">` 包起來的段落（頁碼從 1 起算）。
pub fn part_message(input: &DigestInput, range: std::ops::Range<usize>) -> String {
    let mut s = context(input);
    s.push_str("<paragraphs>\n");
    for g in &input.segments[range] {
        s.push_str(&format!(
            "<para id=\"{}\" page=\"{}\">{}</para>\n",
            g.id,
            g.page + 1,
            escape(&g.text)
        ));
    }
    s.push_str("</paragraphs>\n");
    s
}

/// 合併的使用者訊息：每一部分的點（含段落代號）。
pub fn merge_message(input: &DigestInput, parts: &[Vec<DigestItem>]) -> String {
    let mut s = context(input);
    for (i, items) in parts.iter().enumerate() {
        let mut obj = serde_json::Map::new();
        for sec in input.resolved_sections() {
            let arr: Vec<Value> = items
                .iter()
                .filter(|it| it.section == sec.id)
                .map(|it| serde_json::json!({ "text": it.text, "refs": it.segments }))
                .collect();
            obj.insert(sec.id, Value::Array(arr));
        }
        s.push_str(&format!(
            "<notes part=\"{}\">{}</notes>\n",
            i + 1,
            escape(&Value::Object(obj).to_string())
        ));
    }
    s
}

/// 一點的文字：換行與控制字元換成空白、去掉 `<!--` 與 `-->`（重複去到一個都不剩：只去一次時，
/// 拼起來又是一個）、連續空白收成一個、太長就截斷。
pub fn clean_text(s: &str) -> String {
    let mut t: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    loop {
        let n = t.replace("<!--", "").replace("-->", "");
        if n == t {
            break;
        }
        t = n;
    }
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    t.chars().take(MAX_TEXT).collect()
}

/// 模型照抄了 LaTeX（例如 `\alpha`）時，JSON 不認得的跳脫（反斜線後面不是 `"`、`\`、`/`、`b`、`f`、`n`、`r`、`t`、`u`）
/// 改成兩個反斜線，讓整份回覆還讀得懂。`\beta`、`\frac` 這種剛好像合法跳脫的救不回來（提示詞已要求不用反斜線）。
fn fix_backslashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u') => {
                out.push(c);
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            _ => out.push_str("\\\\"),
        }
    }
    out
}

/// 讀回覆：去掉程式碼框、取第一個 `{` 到最後一個 `}`。只讀 `sections` 裡的區段（每區最多 `max` 的兩倍）；
/// 每一點的段落代號只留 `pages` 裡有的，一個都沒有的那一點不要（計入第二個值）。`lang` 是中文時，
/// 中文旁邊的半形標點換成全形。整個讀不懂是 `BadResponse`。
pub fn parse_notes(
    reply: &str,
    pages: &HashMap<String, u32>,
    sections: &[SectionSpec],
    lang: &str,
) -> Result<(Vec<DigestItem>, usize)> {
    let zh = is_chinese(lang);
    let bad = |why: &str| TranslateError::new(ErrorKind::BadResponse, why.to_string());
    let (Some(a), Some(b)) = (reply.find('{'), reply.rfind('}')) else {
        return Err(bad("the reply has no JSON object"));
    };
    if b < a {
        return Err(bad("the reply has no JSON object"));
    }
    let body = &reply[a..=b];
    let v: Value = serde_json::from_str(body)
        .or_else(|_| serde_json::from_str(&fix_backslashes(body)))
        .map_err(|_| bad("the reply is not JSON"))?;
    let Value::Object(obj) = v else {
        return Err(bad("the reply is not a JSON object"));
    };
    if !sections.iter().any(|s| obj.contains_key(&s.id)) {
        return Err(bad("the reply has none of the sections"));
    }
    let mut items = Vec::new();
    let mut dropped = 0;
    for sec in sections {
        let Some(Value::Array(arr)) = obj.get(&sec.id) else {
            continue;
        };
        for it in arr.iter().take(sec.max.max(1) * 2) {
            let (text, refs) = match it {
                Value::Object(o) => (
                    o.get("text").and_then(Value::as_str).unwrap_or(""),
                    o.get("refs")
                        .and_then(Value::as_array)
                        .map(|r| r.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                        .unwrap_or_default(),
                ),
                _ => ("", Vec::new()),
            };
            let text = clean_text(text);
            let text = if zh { fullwidth_punct(&text) } else { text };
            if text.is_empty() {
                continue;
            }
            let mut segments: Vec<String> = Vec::new();
            for r in refs {
                let r = r.trim();
                if pages.contains_key(r) && !segments.iter().any(|s| s == r) {
                    segments.push(r.to_string());
                }
            }
            segments.truncate(MAX_REFS);
            if segments.is_empty() {
                dropped += 1;
                continue;
            }
            let pg: BTreeSet<u32> = segments.iter().map(|s| pages[s]).collect();
            items.push(DigestItem {
                section: sec.id.clone(),
                text,
                segments,
                pages: pg.into_iter().take(MAX_PAGES).collect(),
            });
        }
    }
    Ok((items, dropped))
}

/// 一次請求（讀不懂的回覆再要一次）。
async fn ask(
    engine: &Engine,
    (system, user): (&str, &str),
    pages: &HashMap<String, u32>,
    input: &DigestInput,
    cancel: &Cancel,
) -> Result<(Vec<DigestItem>, usize)> {
    let mut last = None;
    for _ in 0..2 {
        let reply = engine.complete(system, user, cancel).await?;
        match parse_notes(&reply, pages, &input.sections, &input.target_lang) {
            Ok(v) => return Ok(v),
            Err(e) => last = Some(e),
        }
    }
    Err(last.expect("two attempts"))
}

/// 做一份重點筆記。`state` 記做好的部分（失敗後換服務接著做時沿用）；`on_stage` 每開始一個步驟呼叫一次。
pub async fn run_digest(
    engine: &Engine,
    input: &DigestInput,
    state: &mut DigestState,
    cancel: &Cancel,
    mut on_stage: impl FnMut(DigestStage),
) -> Result<DigestResult> {
    if !engine.provider().is_chat_model() {
        return Err(TranslateError::new(
            ErrorKind::Config,
            "digests need a chat model",
        ));
    }
    if input.segments.is_empty() {
        return Err(TranslateError::new(ErrorKind::Config, "no paragraphs"));
    }
    let input = &input.cleaned();
    let pages: HashMap<String, u32> = input
        .segments
        .iter()
        .map(|s| (s.id.clone(), s.page))
        .collect();
    let ranges = split_parts(&input.segments, PART_CHARS);
    let total = ranges.len();
    if state.parts.len() != total {
        state.parts = vec![None; total];
    }
    let mut dropped = 0;
    for (i, range) in ranges.iter().enumerate() {
        if state.parts[i].is_some() {
            continue;
        }
        if cancel.is_cancelled() {
            return Err(TranslateError::new(ErrorKind::Cancelled, "cancelled"));
        }
        on_stage(DigestStage::Part { n: i + 1, total });
        let (items, d) = ask(
            engine,
            (
                &part_prompt(input, i + 1, total, range.clone()),
                &part_message(input, range.clone()),
            ),
            &pages,
            input,
            cancel,
        )
        .await?;
        dropped += d;
        state.parts[i] = Some(items);
    }
    let parts: Vec<Vec<DigestItem>> = state.parts.iter().flatten().cloned().collect();
    let items = if total == 1 {
        parts.into_iter().next().unwrap_or_default()
    } else {
        if cancel.is_cancelled() {
            return Err(TranslateError::new(ErrorKind::Cancelled, "cancelled"));
        }
        on_stage(DigestStage::Merge);
        let (items, d) = ask(
            engine,
            (&merge_prompt(input), &merge_message(input, &parts)),
            &pages,
            input,
            cancel,
        )
        .await?;
        dropped += d;
        items
    };
    if items.is_empty() {
        return Err(TranslateError::new(
            ErrorKind::BadResponse,
            "no usable points in the reply",
        ));
    }
    Ok(DigestResult {
        items,
        dropped,
        parts: total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(id: &str, page: u32, n: usize) -> DigestSegment {
        DigestSegment {
            id: id.into(),
            page,
            text: "x".repeat(n),
        }
    }

    #[test]
    fn parts_follow_the_reading_order_and_the_size_limit() {
        let s = vec![
            seg("a", 0, 10),
            seg("b", 0, 10),
            seg("c", 1, 10),
            seg("d", 2, 50),
        ];
        assert_eq!(split_parts(&s, 25), vec![0..2, 2..3, 3..4]);
        assert_eq!(split_parts(&s, 1000), vec![0..4]);
        assert_eq!(split_parts(&[seg("a", 0, 500)], 10), vec![0..1]);
    }

    fn pages() -> HashMap<String, u32> {
        [("g0001", 0), ("g0002", 2), ("g0003", 2)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    fn std() -> Vec<SectionSpec> {
        standard_sections()
    }

    #[test]
    fn notes_keep_only_known_refs_and_drop_points_without_any() {
        let reply = "```json\n{\"question\":[{\"text\":\"問題\\n一。\",\"refs\":[\"g0001\",\"g9999\"]}],\
\"results\":[{\"text\":\"結果 <!-- x --> 二。\",\"refs\":[\"g0003\",\"g0002\",\"g0003\"]},{\"text\":\"沒有出處\",\"refs\":[\"zz\"]}],\
\"limitations\":[]}\n```";
        let (items, dropped) = parse_notes(reply, &pages(), &std(), "zh-TW").unwrap();
        assert_eq!(dropped, 1);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].section, "question");
        assert_eq!(items[0].text, "問題 一。");
        assert_eq!(items[0].segments, vec!["g0001"]);
        assert_eq!(items[0].pages, vec![0]);
        assert_eq!(items[1].text, "結果 x 二。");
        assert_eq!(items[1].segments, vec!["g0003", "g0002"]);
        assert_eq!(items[1].pages, vec![2]);
    }

    #[test]
    fn comment_markers_are_cleaned_until_none_is_left() {
        // 只取代一次時，拼起來又是一個標記
        for s in ["<!<!----", "x ---->> y", "<!-<!---->-", "a<!--b-->c"] {
            let t = clean_text(s);
            assert!(!t.contains("<!--") && !t.contains("-->"), "{s:?} → {t:?}");
        }
    }

    #[test]
    fn latex_backslashes_do_not_break_the_reply() {
        // 模型照抄的 `\alpha` 不是合法的 JSON 跳脫；已經跳脫好的 `\\` 照常
        let reply = r#"{"results":[{"text":"誤差 \alpha < 0.1，\\ 照常","refs":["g0001"]}]}"#;
        let (items, _) = parse_notes(reply, &pages(), &std(), "zh-TW").unwrap();
        assert_eq!(items[0].text, r"誤差 \alpha < 0.1，\ 照常");
    }

    #[test]
    fn unreadable_replies_are_bad_responses() {
        for r in ["", "no json here", "[1,2]", "{\"other\":[]}", "{broken"] {
            let e = parse_notes(r, &pages(), &std(), "zh-TW").unwrap_err();
            assert_eq!(e.kind, ErrorKind::BadResponse, "{r}");
        }
    }

    fn input(lang: &str) -> DigestInput {
        DigestInput {
            title: Some("T".into()),
            segments: vec![seg("g0001", 0, 10)],
            target_lang: lang.into(),
            ..Default::default()
        }
    }

    #[test]
    fn prompts_follow_the_target_language() {
        let p = part_prompt(&input("zh-TW"), 1, 1, 0..1);
        assert!(p.contains("Traditional Chinese") && p.contains("Taiwan"));
        assert!(p.contains("full-width punctuation"));
        assert!(!p.contains("part 1 of"));
        let p = part_prompt(&input("ja"), 2, 3, 0..1);
        assert!(p.contains("Japanese") && !p.contains("Taiwan"));
        assert!(p.contains("part 2 of 3"));
    }

    #[test]
    fn the_reply_shape_follows_the_sections() {
        // 加一個區段，回覆的 JSON 形狀與說明跟著改；讀回覆只認得這些區段
        let mut i = input("zh-TW");
        i.sections = std();
        i.sections.insert(
            2,
            SectionSpec {
                id: "contradictions".into(),
                about: "places where {{title}} contradicts itself".into(),
                max: 2,
            },
        );
        let p = part_prompt(&i, 1, 1, 0..1);
        assert!(
            p.contains(
                "{\"question\":[…],\"method\":[…],\"contradictions\":[…],\"results\":[…],\"limitations\":[…],\"quotes\":[…]}"
            ),
            "{p}"
        );
        assert!(
            p.contains("- contradictions: places where T contradicts itself (at most 2 items)\n")
        );
        let reply = r#"{"contradictions":[{"text":"第 3 頁說 4 層,第 7 頁說 6 層.","refs":["g0001","g0002"]},{"text":"b","refs":["g0001"]},{"text":"c","refs":["g0001"]},{"text":"d","refs":["g0001"]},{"text":"e","refs":["g0001"]}],"unknown":[{"text":"x","refs":["g0001"]}]}"#;
        let (items, _) = parse_notes(reply, &pages(), &i.sections, "zh-TW").unwrap();
        assert_eq!(
            items.len(),
            4,
            "at most twice the limit, unknown sections ignored"
        );
        assert!(items.iter().all(|x| x.section == "contradictions"));
        assert_eq!(items[0].text, "第 3 頁說 4 層，第 7 頁說 6 層。");
        assert_eq!(items[0].pages, vec![0, 2]);
        // 只有使用者改得到的兩部分不是固定的
        let parts = digest_parts(&i, DigestStage::Part { n: 1, total: 1 }, &[]);
        let editable: Vec<&str> = parts.iter().filter(|x| !x.fixed).map(|x| x.key).collect();
        assert_eq!(editable, ["preset", "sections"]);
    }

    #[test]
    fn the_glossary_and_the_paper_terms_go_into_the_digest_prompt() {
        let mut i = input("zh-TW");
        i.segments[0].text = "The policy maps each state.".into();
        i.glossary = vec![("state".into(), "狀態".into())];
        i.paper_terms = vec![
            ("policy".into(), "策略".into()),
            ("State".into(), "州".into()),
        ];
        let p = part_prompt(&i, 1, 1, 0..1);
        assert!(p.contains("- state => 狀態\n- policy => 策略\n"), "{p}");
        assert!(!p.contains("州"));
    }

    #[test]
    fn control_characters_are_not_sent() {
        let mut i = input("zh-TW");
        i.segments[0].text = "a\u{0}b\u{1}".into();
        let preview = digest_preview(&i);
        assert!(join(&preview.user).contains(">ab</para>"));
    }

    #[test]
    fn messages_carry_ids_pages_title_and_abstract() {
        let input = DigestInput {
            title: Some("A <b> title".into()),
            abstract_text: Some("We study x.".into()),
            segments: vec![
                DigestSegment {
                    id: "g0001".into(),
                    page: 0,
                    text: "First & <x>".into(),
                },
                DigestSegment {
                    id: "g0002".into(),
                    page: 4,
                    text: "Second".into(),
                },
            ],
            target_lang: "zh-TW".into(),
            ..Default::default()
        };
        let m = part_message(&input, 0..2);
        assert!(m.contains("<title>A &lt;b&gt; title</title>"));
        assert!(m.contains("<abstract>We study x.</abstract>"));
        assert!(m.contains("<para id=\"g0001\" page=\"1\">First &amp; &lt;x&gt;</para>"));
        assert!(m.contains("<para id=\"g0002\" page=\"5\">Second</para>"));
    }
}
