//! 選取文字的快速翻譯：讀原文時選一個詞、一句、一段，當場看譯文。
//!
//! 送出的只有選取的文字與前後各一句（前後文只用來決定詞義，不翻、不回）。
//! 提示詞照翻譯的做法分兩部分（`prompt.rs`）：使用者改得到的預設組（用途 `quick`），加上程式固定附加的規則、
//! 回覆的格式與術語。詞與句的回法不同（詞：詞義與在這句裡的意思；句與段：只給譯文），由 [`is_word`] 決定，
//! 寫在固定的「回覆的格式」裡。「看完整提示詞」列的是同一份（[`quick_preview`]），逐字相同。
//!
//! 機器翻譯與離線引擎沒有提示詞，只收到選取的文字（不送前後文）。

use crate::Params;
use crate::error::{ErrorKind, Result, TranslateError};
use crate::prompt::{
    DEFAULT_QUICK_PRESET, PromptPart, PromptPreview, builtin_preset, fixed_rules, glossary_block,
    join, language_name, render_template,
};

/// 選取的文字最多幾個字；更長的請使用者改用全文翻譯（整頁的快速翻譯不做）。
pub const MAX_QUICK_CHARS: usize = 2000;
/// 前文、後文各最多幾個字（一句；超過的截掉，留靠近選取的那一端）。
pub const MAX_CONTEXT_CHARS: usize = 400;

/// 一次快速翻譯要送的東西。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuickInput {
    /// 選取的文字。
    pub text: String,
    /// 選取前面的一句（同一頁）；沒有是 `None`。
    pub before: Option<String>,
    /// 選取後面的一句（同一頁）；沒有是 `None`。
    pub after: Option<String>,
}

impl QuickInput {
    /// 整理過的版本：空白收成一個、前後文截到上限（前文留後段、後文留前段）。選取是空的或超過上限交回錯誤。
    pub fn normalized(&self) -> Result<QuickInput> {
        let text = squeeze(&self.text);
        let n = text.chars().count();
        if n == 0 || n > MAX_QUICK_CHARS {
            return Err(TranslateError::new(ErrorKind::Config, "selection length"));
        }
        let clip = |s: &Option<String>, keep_end: bool| {
            let s = squeeze(s.as_deref().unwrap_or(""));
            let c = s.chars().count();
            if c == 0 {
                return None;
            }
            Some(if c <= MAX_CONTEXT_CHARS {
                s
            } else if keep_end {
                s.chars().skip(c - MAX_CONTEXT_CHARS).collect()
            } else {
                s.chars().take(MAX_CONTEXT_CHARS).collect()
            })
        };
        Ok(QuickInput {
            before: clip(&self.before, true),
            after: clip(&self.after, false),
            text,
        })
    }
}

fn squeeze(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 選取的是一個詞或短的詞組（最多三個詞、40 字以內、沒有句子的標點）。不是的就當成句或段。
pub fn is_word(text: &str) -> bool {
    let t = text.trim();
    let n = t.chars().count();
    n > 0
        && n <= 40
        && t.split_whitespace().count() <= 3
        && !t
            .trim_end_matches(['.', '!', '?', ';', ':', ','])
            .contains(['.', '!', '?', ';', '。', '！', '？', '；'])
}

/// 預設組：使用者的（`instructions`），沒有就用內建的「快速翻譯」。
fn preset_text(p: &Params) -> &str {
    if p.instructions.trim().is_empty() {
        builtin_preset(DEFAULT_QUICK_PRESET).map_or("", |b| b.text)
    } else {
        &p.instructions
    }
}

/// 回覆的格式（固定）：詞與句各一種回法。
fn format_part(p: &Params, word: bool) -> String {
    let to = language_name(&p.target_lang);
    let mut s = String::from(
        "- The selected text is wrapped in <selection>…</selection>. The sentences in <before> and <after> come right before and after it in the paper; \
         they are given only to decide the meaning. Never translate or output them.\n",
    );
    if word {
        s.push_str(&format!(
            "- The selection is a word or a short phrase. Reply in {to} with exactly two lines: \
             line 1, its general meaning (one to three short senses separated by \u{FF1B}); \
             line 2, what it means in this sentence. No labels, no quotation of the original.\n"
        ));
    } else {
        s.push_str(&format!(
            "- The selection is a sentence or a passage. Reply with only its translation in {to}.\n"
        ));
    }
    s.push_str("- Reply in plain text: no tags, no quotation marks around the reply, no preamble or explanation.\n");
    s
}

/// 系統提示詞的各部分。`terms` 是這一篇已定的譯法（最新那份同語言譯本照用的）。
pub fn quick_parts(p: &Params, input: &QuickInput, terms: &[(String, String)]) -> Vec<PromptPart> {
    vec![
        PromptPart::new("preset", false, render_template(preset_text(p), p)),
        PromptPart::new("rules", true, fixed_rules(p)),
        PromptPart::new("format", true, format_part(p, is_word(&input.text))),
        PromptPart::new(
            "glossary",
            true,
            glossary_block(&p.glossary, terms, &[input.text.as_str()]),
        ),
    ]
}

/// 送出的內容：前一句、選取的文字、後一句（沒有前後文的不佔位置）。
pub fn quick_message(input: &QuickInput) -> String {
    let mut s = String::new();
    if let Some(b) = input.before.as_deref().filter(|b| !b.trim().is_empty()) {
        s.push_str(&format!("<before>{}</before>\n", b.trim()));
    }
    s.push_str(&format!("<selection>{}</selection>\n", input.text.trim()));
    if let Some(a) = input.after.as_deref().filter(|a| !a.trim().is_empty()) {
        s.push_str(&format!("<after>{}</after>\n", a.trim()));
    }
    s
}

/// 一次快速翻譯送出的完整內容（「看完整提示詞」）：和實際送出的逐字相同。
pub fn quick_preview(p: &Params, input: &QuickInput, terms: &[(String, String)]) -> PromptPreview {
    PromptPreview {
        kind: "quick",
        system: quick_parts(p, input, terms),
        user: vec![PromptPart::new("input", true, quick_message(input))],
    }
}

/// 系統提示詞與使用者訊息（實際送出的）。
pub(crate) fn quick_request(
    p: &Params,
    input: &QuickInput,
    terms: &[(String, String)],
) -> (String, String) {
    (join(&quick_parts(p, input, terms)), quick_message(input))
}

/// 模型的回覆整理成要顯示的譯文：去掉前後空白、包起來的程式碼框、照抄回來的標籤，空行收成一行。空的交回錯誤。
pub fn clean_reply(reply: &str) -> Result<String> {
    let mut t = reply.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // 第一行是語言名稱（可能是空的）
        t = rest.split_once('\n').map_or("", |(_, r)| r);
        t = t.trim_end().trim_end_matches("```").trim();
    }
    let mut out = t.to_string();
    for tag in ["<selection>", "</selection>"] {
        out = out.replace(tag, "");
    }
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    let mut joined = String::new();
    let mut blank = false;
    for l in lines {
        if l.trim().is_empty() {
            blank = true;
            continue;
        }
        if !joined.is_empty() {
            joined.push('\n');
            if blank {
                joined.push('\n');
            }
        }
        blank = false;
        joined.push_str(l);
    }
    let joined = joined.trim().to_string();
    if joined.is_empty() {
        return Err(TranslateError::new(ErrorKind::BadResponse, "empty reply"));
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Params {
        Params {
            source_lang: Some("en".into()),
            target_lang: "zh-TW".into(),
            title: Some("Attention".into()),
            ..Params::default()
        }
    }

    fn input(text: &str) -> QuickInput {
        QuickInput {
            text: text.into(),
            before: Some("The previous sentence.".into()),
            after: Some("The next sentence.".into()),
        }
    }

    #[test]
    fn words_and_sentences_are_told_apart() {
        assert!(is_word("convolution"));
        assert!(is_word("self-attention layer"));
        assert!(is_word("state of the"));
        assert!(is_word("ablation,"));
        assert!(!is_word("We propose a new method"));
        assert!(!is_word("It works. Really"));
        assert!(!is_word(""));
        assert!(!is_word(&"x".repeat(41)));
    }

    #[test]
    fn the_reply_format_follows_the_kind_of_selection() {
        let p = params();
        let w = join(&quick_parts(&p, &input("convolution"), &[]));
        assert!(w.contains("exactly two lines"));
        assert!(!w.contains("only its translation"));
        let s = join(&quick_parts(&p, &input("We propose a new method."), &[]));
        assert!(s.contains("only its translation"));
        // 前後文只用來決定詞義
        assert!(s.contains("Never translate or output them"));
        // 預設組代入篇名與語言；臺灣用語與全形標點照翻譯的固定規則
        assert!(s.contains("\"Attention\""));
        assert!(s.contains("Traditional Chinese"));
        assert!(s.contains("full-width punctuation"));
    }

    #[test]
    fn only_the_selection_and_one_sentence_each_side_are_sent() {
        let m = quick_message(&input("convolution"));
        assert_eq!(
            m,
            "<before>The previous sentence.</before>\n<selection>convolution</selection>\n<after>The next sentence.</after>\n"
        );
        let bare = QuickInput {
            text: "convolution".into(),
            ..QuickInput::default()
        };
        assert_eq!(quick_message(&bare), "<selection>convolution</selection>\n");
    }

    #[test]
    fn the_preview_is_what_is_sent() {
        let p = params();
        let i = input("We propose a new method.");
        let terms = vec![("method".to_string(), "方法".to_string())];
        let (system, user) = quick_request(&p, &i, &terms);
        let pv = quick_preview(&p, &i, &terms);
        assert_eq!(pv.kind, "quick");
        assert_eq!(join(&pv.system), system);
        assert_eq!(join(&pv.user), user);
        // 這一篇已定的譯法出現在選取裡就帶上
        assert!(system.contains("- method => 方法"));
        // 使用者自訂的預設組照用
        let mine = Params {
            instructions: "Just translate into {{to}}.".into(),
            ..params()
        };
        assert!(
            join(&quick_parts(&mine, &i, &[]))
                .starts_with("Just translate into Traditional Chinese")
        );
    }

    #[test]
    fn input_is_tidied_and_limited() {
        let i = QuickInput {
            text: "  a\n  b  ".into(),
            before: Some("x".repeat(MAX_CONTEXT_CHARS + 10) + "END"),
            after: Some("START".to_string() + &"y".repeat(MAX_CONTEXT_CHARS + 10)),
        }
        .normalized()
        .unwrap();
        assert_eq!(i.text, "a b");
        assert!(i.before.as_deref().unwrap().ends_with("END"));
        assert_eq!(
            i.before.as_deref().unwrap().chars().count(),
            MAX_CONTEXT_CHARS
        );
        assert!(i.after.as_deref().unwrap().starts_with("START"));
        assert!(QuickInput::default().normalized().is_err());
        let long = QuickInput {
            text: "a".repeat(MAX_QUICK_CHARS + 1),
            ..QuickInput::default()
        };
        assert!(long.normalized().is_err());
        let blank_context = QuickInput {
            text: "a".into(),
            before: Some("  ".into()),
            after: None,
        };
        assert_eq!(blank_context.normalized().unwrap().before, None);
    }

    #[test]
    fn replies_are_cleaned() {
        assert_eq!(
            clean_reply("  卷積\n\n\n此處指運算  ").unwrap(),
            "卷積\n\n此處指運算"
        );
        assert_eq!(clean_reply("```text\n譯文\n```").unwrap(), "譯文");
        assert_eq!(clean_reply("<selection>譯文</selection>").unwrap(), "譯文");
        assert!(clean_reply("  \n ").is_err());
    }
}
