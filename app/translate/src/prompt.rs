//! 對話模型的提示詞與回覆的解析。
//!
//! 提示詞分兩部分：使用者可以挑選、自訂的「提示詞預設組」（要做什麼、怎麼寫），加上程式固定附加、
//! 使用者改不到的規則（保留公式與引用、回覆的格式、譯成繁體中文時的臺灣用語、術語）。這樣使用者改提示詞
//! 也弄不壞批次（做法參考 Readest、bilingual_book_maker：輸出格式由程式附加、變數有白名單）。
//!
//! 預設組有兩種用途：翻譯與重點筆記，共用這裡的內建組、變數白名單與術語的寫法；重點筆記的部分在
//! `digest.rs`。送出的提示詞一律由 [`PromptPart`] 接起來：「看完整提示詞」列的就是同一份，逐字相同。
//!
//! 一批段落用 `<seg id="n">…</seg>` 包起來送出，要求模型照樣包著回。不用 JSON：
//! 論文裡有大量反斜線（LaTeX），要模型逐個跳脫很容易出錯；標籤格式不必跳脫。
//! 回覆最後另附 `<terms>…</terms>`：這一批用到的術語譯法，之後的批次照用（同一篇的術語一致）。

use crate::error::{ErrorKind, Result, TranslateError};
use crate::glossary::relevant;
use crate::text::is_chinese;
use crate::{Params, Segment};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// 預設組的用途。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PresetKind {
    #[default]
    Translation,
    Digest,
    /// 選取文字的快速翻譯（`quick.rs`）。
    Quick,
}

/// 內建重點筆記預設組的一個區段：代號、要寫什麼（給模型的說明）、最多幾點。名稱跟著輸出的語言，由外殼給。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BuiltinSection {
    pub id: &'static str,
    pub about: &'static str,
    pub max: usize,
}

/// 內建的提示詞預設組。`id` 寫進設定檔，改名要同步改格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BuiltinPreset {
    pub id: &'static str,
    pub kind: PresetKind,
    pub text: &'static str,
    /// 重點筆記的區段；翻譯的是空的。
    pub sections: &'static [BuiltinSection],
}

/// 翻譯的預設組沒有指定時用的。
pub const DEFAULT_PRESET: &str = "general";
/// 重點筆記的預設組沒有指定時用的。
pub const DEFAULT_DIGEST_PRESET: &str = "standard";
/// 快速翻譯的預設組沒有指定時用的。
pub const DEFAULT_QUICK_PRESET: &str = "quick";

pub const BUILTIN_PRESETS: &[BuiltinPreset] = &[
    BuiltinPreset {
        id: "general",
        kind: PresetKind::Translation,
        text: "You are a professional translator. Translate each passage from {{from}} into {{to}}. \
Translate faithfully and completely, in natural and fluent language. \
Do not summarize, explain, add or omit anything.",
        sections: &[],
    },
    BuiltinPreset {
        id: "academic",
        kind: PresetKind::Translation,
        text: "You are a professional translator of academic papers, fluent in the terminology of the paper's field. \
The paper is titled \"{{title}}\". Translate each passage from {{from}} into {{to}}.\n\
- Use the established technical terms of the field and keep each term consistent throughout the paper.\n\
- Parenthesized English: keep proper nouns, model and method names, dataset names and acronyms in their original form \
without parentheses; write a technical term that has an established translation only in the target language, \
without the English; add the English in parentheses after the translation only at the first occurrence in the paper \
of the full form of an acronym or of a technical term without an established translation.\n\
- When translating into Traditional Chinese for Taiwan, use the wording common in Taiwan, for example \
呼叫 (not 調用), 預設 (not 默認), 設定 (not 設置), 資訊 (not 信息), 網路 (not 網絡), 最佳化 (not 優化), \
分布 (not 分佈) and 笛卡兒 (not 笛卡爾).\n\
- Keep the formal, precise register of academic writing.\n\
- Translate faithfully and completely. Do not summarize, explain, add or omit anything.",
        sections: &[],
    },
    // 「標準」：原先寫死在 `digest.rs` 的五區與寫法，加上兩條要求
    // （原文前後矛盾要標出、限制只寫這篇自己的）
    BuiltinPreset {
        id: "standard",
        kind: PresetKind::Digest,
        text: "You write structured reading notes for one research paper, so that a researcher can review the paper quickly \
and check every point against it.\n\
Use only what the given paragraphs say. Do not invent results, numbers, names or citations.\n\
Keep formulas, numbers, units, model and dataset names exactly as written.\n\
Each text is one or two sentences, at most 300 characters.\n\
When the paper contradicts itself (for example two different values for the same quantity, or a claim that its own \
results do not support), say so in the most relevant section and give the paragraphs of both places in refs.\n\
For limitations, write only what this paper states about its own work; do not add limitations of other work or \
comparisons of your own.",
        sections: &[
            BuiltinSection {
                id: "question",
                about: "the research problem and why it matters (at least 1 item)",
                max: 3,
            },
            BuiltinSection {
                id: "method",
                about: "the approach, data and experimental setup (at least 2 items)",
                max: 5,
            },
            BuiltinSection {
                id: "results",
                about: "the main findings with their key numbers (at least 2 items)",
                max: 6,
            },
            BuiltinSection {
                id: "limitations",
                about: "limitations, assumptions and open problems that the paper states about itself ([] when the paper states none)",
                max: 3,
            },
            BuiltinSection {
                id: "quotes",
                about: "sentences or numbers worth noting down; quote the original words in their original language, then add a short note",
                max: 4,
            },
        ],
    },
    // 快速翻譯：詞給詞義與在這句裡的意思，句與段只給譯文。要怎麼回（兩行、不加說明）在 `quick.rs` 固定附加
    BuiltinPreset {
        id: "quick",
        kind: PresetKind::Quick,
        text: "You help a researcher read the academic paper titled \"{{title}}\". \
Translate the selected text from {{from}} into {{to}}, using the established terms of the paper's field.\n\
- For a single word or a short phrase: give its general meaning, then what it means in this sentence.\n\
- For a sentence or a longer passage: give only a faithful, complete translation.",
        sections: &[],
    },
];

/// 提示詞裡可以用的變數（翻譯與重點筆記相同）。其他 `{{…}}` 一律不收（存設定時就拒絕）。
pub const TEMPLATE_VARS: &[&str] = &["from", "to", "title"];

/// 檢查提示詞的變數：全部在白名單裡就交回 `Ok`；否則交回第一個不認得的變數名稱。
pub fn check_template(text: &str) -> std::result::Result<(), String> {
    let mut rest = text;
    while let Some(i) = rest.find("{{") {
        let after = &rest[i + 2..];
        let Some(j) = after.find("}}") else {
            return Err(after.chars().take(20).collect());
        };
        let name = after[..j].trim();
        if !TEMPLATE_VARS.contains(&name) {
            return Err(name.to_string());
        }
        rest = &after[j + 2..];
    }
    Ok(())
}

/// 內建預設組的內容；不認得的代號交回 `None`。
pub fn builtin_preset(id: &str) -> Option<&'static BuiltinPreset> {
    BUILTIN_PRESETS.iter().find(|p| p.id == id)
}

/// 把預設組的變數代進去。
pub fn render_template(text: &str, p: &Params) -> String {
    let from = match &p.source_lang {
        Some(s) if !s.is_empty() => language_name(s),
        _ => "the original language".to_string(),
    };
    let title = p
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("(not given)");
    let to = language_name(&p.target_lang);
    let mut out = String::with_capacity(text.len() + 64);
    let mut rest = text;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let Some(j) = after.find("}}") else {
            out.push_str(&rest[i..]);
            return out;
        };
        match after[..j].trim() {
            "from" => out.push_str(&from),
            "to" => out.push_str(&to),
            "title" => out.push_str(title),
            _ => out.push_str(&rest[i..i + 2 + j + 2]),
        }
        rest = &after[j + 2..];
    }
    out.push_str(rest);
    out
}

/// 語言代碼的白話名稱，寫進提示詞。不認得的代碼原樣使用。
pub fn language_name(code: &str) -> String {
    let lower = code.to_ascii_lowercase();
    match lower.as_str() {
        "zh-tw" | "zh-hant" | "zh-hk" => "Traditional Chinese (繁體中文)".to_string(),
        "zh-cn" | "zh-hans" | "zh" => "Simplified Chinese (简体中文)".to_string(),
        "en" | "en-us" | "en-gb" => "English".to_string(),
        "ja" => "Japanese".to_string(),
        "ko" => "Korean".to_string(),
        "de" => "German".to_string(),
        "fr" => "French".to_string(),
        "es" => "Spanish".to_string(),
        "ru" => "Russian".to_string(),
        _ => code.to_string(),
    }
}

pub(crate) fn is_taiwan(code: &str) -> bool {
    matches!(
        code.to_ascii_lowercase().as_str(),
        "zh-tw" | "zh-hant" | "zh-hant-tw"
    )
}

// ---------- 提示詞的各部分 ----------

/// 提示詞的一部分。送出的提示詞就是各部分依序接起來（[`join`]）；「看完整提示詞」照同一份列出，
/// 使用者改不到的標 `fixed`。`key` 是給畫面的名稱代號：`preset`（預設組）、`sections`（區段）、
/// `rules`（固定的規則）、`format`（回覆的格式）、`refine`（第二輪潤飾的說明）、`stage`（分部分與合併的說明）、
/// `glossary`（術語）、`input`（送出的內容）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptPart {
    pub key: &'static str,
    pub fixed: bool,
    pub text: String,
}

impl PromptPart {
    pub(crate) fn new(key: &'static str, fixed: bool, text: String) -> PromptPart {
        PromptPart { key, fixed, text }
    }
}

/// 一次請求的完整內容：系統提示詞與使用者訊息，各自分成幾部分。`kind` 是哪一種請求：
/// `translate`（翻一批）、`refine`（第二輪潤飾）、`digest`（重點筆記的一部分）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptPreview {
    pub kind: &'static str,
    pub system: Vec<PromptPart>,
    pub user: Vec<PromptPart>,
}

/// 把各部分接起來（空的部分不佔位置）。
pub fn join(parts: &[PromptPart]) -> String {
    parts.iter().map(|p| p.text.as_str()).collect()
}

/// 固定附加的語言規則：譯成繁體中文時的臺灣用語、中文的全形標點（六成段落用半形逗號）。
pub(crate) fn language_rules(target_lang: &str) -> String {
    let mut s = String::new();
    if is_taiwan(target_lang) {
        s.push_str(
            "- Write Traditional Chinese as used in Taiwan: use Taiwanese vocabulary (for example 軟體, 資料, 網路, 程式, 品質, 影片), \
             not Mainland Chinese terms, and use 「」 for quotation marks (『』 inside 「」).\n",
        );
    }
    if is_taiwan(target_lang) {
        s.push_str(
            "- Cross-references: (Section 4.2) → (第 4.2 節), Fig. 3 → 圖 3, Table 2 → 表 2, Eq. (5) → 式 (5).\n",
        );
    } else if is_chinese(target_lang) {
        s.push_str(
            "- Cross-references: (Section 4.2) → (第 4.2 节), Fig. 3 → 图 3, Table 2 → 表 2, Eq. (5) → 式 (5).\n",
        );
    }
    if is_chinese(target_lang) {
        s.push_str(
            "- In Chinese text use full-width punctuation (，。：；？！（）), not half-width; keep numbers, formulas, code and English words as they are. \
             Citation markers keep their original half-width brackets and spacing, for example (Smith et al., 2020), Smith et al. (2020) and [12].\n",
        );
    }
    s
}

/// 程式固定附加的規則：保留公式與引用（自動檢查與譯文疊加靠它）、語言的規則。
pub(crate) fn fixed_rules(p: &Params) -> String {
    let mut s = String::from(
        "\n\nRules that always apply:\n\
         - Keep mathematical formulas, symbols, LaTeX, equation numbers, variable names, code, URLs and DOIs exactly as written.\n\
         - Keep citation markers unchanged, for example [12], [3, 5] and (Smith et al., 2020).\n\
         - In cross-references to sections, figures, tables and equations, translate the word into the target language and keep the number exactly as in the original.\n\
         - Do not append the whole original passage to its translation.\n",
    );
    s.push_str(&language_rules(&p.target_lang));
    s
}

/// 這一篇照用的術語最多幾條；連同候選、刪掉的，最多記幾條（同核心的 `MAX_TERMS`）。
pub const MAX_PAPER_TERMS: usize = 300;
pub const MAX_KNOWN_TERMS: usize = 600;
/// 一條術語的原文與譯法最多幾個字；學到的原文詞最少幾個字（`RL` 這種太短的不收）。
const MAX_TERM_CHARS: usize = 60;
const MIN_LEARNED_CHARS: usize = 3;

/// 這一篇的一條術語的狀態（記在那份譯文疊加檔裡）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TermState {
    /// 只在一批裡出現過：還不用。
    Candidate,
    /// 兩批以上是同一個譯法：照用。
    Learned,
    /// 使用者定的：照用，學到的不蓋掉。
    User,
    /// 使用者刪掉的：不用，也不再學。
    Removed,
}

/// 這一篇的一條術語（先看到、改得了，兩批同一個譯法才定下）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaperTerm {
    pub source: String,
    pub target: String,
    pub state: TermState,
    /// 出現過這個譯法的批數。
    pub batches: u32,
}

/// 照用的術語（學到的與使用者定的），依記下的順序。
pub fn terms_in_use(terms: &[PaperTerm]) -> Vec<(String, String)> {
    terms
        .iter()
        .filter(|t| matches!(t.state, TermState::Learned | TermState::User))
        .map(|t| (t.source.clone(), t.target.clone()))
        .collect()
}

/// `hay` 裡有沒有整個詞 `word`（都已轉小寫）：詞的兩端若是英數字，前後就不能接著英數字。
pub(crate) fn has_word(hay: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric());
    let (first, last) = (word.chars().next(), word.chars().last());
    hay.match_indices(word).any(|(i, _)| {
        let before = hay[..i].chars().last();
        let after = hay[i + word.len()..].chars().next();
        let starts_mid_word = edge(first) && edge(before);
        let ends_mid_word = edge(last) && edge(after);
        !starts_mid_word && !ends_mid_word
    })
}

/// 這一批原文用得到的這篇術語：整個詞出現在任一段裡（不分大小寫）。
fn relevant_words<'a>(terms: &'a [(String, String)], texts: &[&str]) -> Vec<&'a (String, String)> {
    let lower: Vec<String> = texts.iter().map(|t| t.to_lowercase()).collect();
    terms
        .iter()
        .filter(|(from, _)| {
            let f = from.trim().to_lowercase();
            lower.iter().any(|t| has_word(t, &f))
        })
        .collect()
}

/// 術語：使用者的術語庫在前，這篇已定的譯法在後；只放這一批原文裡出現的（做法參考 BabelDOC；這篇的術語照詞的邊界），
/// 同一個原文詞（不分大小寫）以術語庫為準。沒有就是空字串。
pub(crate) fn glossary_block(
    glossary: &[(String, String)],
    paper_terms: &[(String, String)],
    texts: &[&str],
) -> String {
    let mut lines: Vec<&(String, String)> = relevant(glossary, texts);
    let mut seen: HashSet<String> = glossary.iter().map(|(f, _)| f.to_lowercase()).collect();
    for t in relevant_words(paper_terms, texts) {
        if seen.insert(t.0.to_lowercase()) {
            lines.push(t);
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "\nGlossary (always use these translations; keep a term as is when its translation is the same):\n",
    );
    for (from, to) in lines {
        s.push_str(&format!("- {from} => {to}\n"));
    }
    s
}

/// 預設組：使用者的（`instructions`），沒有就用內建的「通用」。
fn preset_text(p: &Params) -> &str {
    if p.instructions.trim().is_empty() {
        builtin_preset(DEFAULT_PRESET).map_or("", |b| b.text)
    } else {
        &p.instructions
    }
}

/// 翻譯的系統提示詞的各部分。`terms` 是這一篇已定的譯法。
pub fn translate_parts(
    p: &Params,
    batch: &[Segment],
    terms: &[(String, String)],
) -> Vec<PromptPart> {
    let texts: Vec<&str> = batch.iter().map(|g| g.text.as_str()).collect();
    vec![
        PromptPart::new("preset", false, render_template(preset_text(p), p)),
        PromptPart::new("rules", true, fixed_rules(p)),
        PromptPart::new(
            "format",
            true,
            "- The input is a list of passages, each wrapped as <seg id=\"N\">…</seg>. Reply with exactly the same wrappers, one per passage, in the same order, with the same ids, then the <terms> block, and nothing else.\n\
             - Passages marked as context are only for understanding the text; never translate or output them.\n\
             - A note after the passages may say that a passage continues another passage or the context: one sentence was split across columns or pages. \
             Read the parts together as one sentence and translate each passage as its own part of that sentence, so that the translations read on as one sentence without repeating anything.\n\
             - After the last passage, add one <terms>…</terms> block listing the technical terms of the field that you translated in these passages, one per line as `original => translation` (at most 12 lines; write the original again when you kept a term untranslated). Write <terms></terms> when there are none.\n"
                .to_string(),
        ),
        PromptPart::new("glossary", true, glossary_block(&p.glossary, terms, &texts)),
    ]
}

/// 翻譯用的系統提示詞（[`translate_parts`] 接起來）。
pub fn system_prompt(p: &Params, batch: &[Segment], terms: &[(String, String)]) -> String {
    join(&translate_parts(p, batch, terms))
}

/// 送出的內容：前文（只供理解）加上一批段落。有接續時，段落之後另附後文與註明
/// （不放進 `<seg>` 標籤裡：模型照抄進回覆就讀不懂了）。
pub fn user_message(batch: &[Segment], before: Option<&str>) -> String {
    let mut s = String::new();
    if let Some(b) = before.filter(|b| !b.trim().is_empty()) {
        s.push_str("<context>\n");
        s.push_str(b.trim());
        s.push_str("\n</context>\n\n");
    }
    for seg in batch {
        s.push_str(&format!("<seg id=\"{}\">{}</seg>\n", seg.id, seg.text));
    }
    let after = batch.iter().find_map(|g| {
        g.after
            .as_deref()
            .filter(|a| !a.trim().is_empty())
            .map(|a| (g, a))
    });
    if let Some((_, a)) = after {
        s.push_str("\n<context>\n");
        s.push_str(a.trim());
        s.push_str("\n</context>\n");
    }
    let mut notes = Vec::new();
    for (i, seg) in batch.iter().enumerate() {
        match seg.continues.as_deref() {
            Some(m) if batch.iter().any(|g| g.id == m) => notes.push(format!(
                "Note: passage {} continues passage {m} (one sentence split across columns or pages).",
                seg.id
            )),
            // 前文只有一批的第一段的；不是第一段時，送出的前文不是它的前一段
            Some(_) if i == 0 && before.is_some_and(|b| !b.trim().is_empty()) => notes.push(format!(
                "Note: passage {} continues the sentence in the context before it.",
                seg.id
            )),
            Some(_) => {}
            None => {}
        }
    }
    if let Some((g, _)) = after {
        notes.push(format!(
            "Note: passage {} is continued by the sentence in the context after it.",
            g.id
        ));
    }
    if !notes.is_empty() {
        s.push('\n');
        for n in notes {
            s.push_str(&n);
            s.push('\n');
        }
    }
    s
}

/// 翻譯一批時送出的完整內容（「看完整提示詞」）：和實際送出的逐字相同。
pub fn translate_preview(
    p: &Params,
    batch: &[Segment],
    terms: &[(String, String)],
) -> PromptPreview {
    let before = batch.first().and_then(|s| s.before.as_deref());
    PromptPreview {
        kind: "translate",
        system: translate_parts(p, batch, terms),
        user: vec![PromptPart::new("input", true, user_message(batch, before))],
    }
}

/// 第二輪潤飾那一次請求的完整內容（潤飾開著時每一批多送一次）。`drafts` 是第一次的譯文。
pub fn refine_preview(
    p: &Params,
    batch: &[Segment],
    drafts: &[String],
    terms: &[(String, String)],
) -> PromptPreview {
    PromptPreview {
        kind: "refine",
        system: refine_parts(p, batch, terms),
        user: vec![PromptPart::new(
            "input",
            true,
            refine_message(batch, drafts),
        )],
    }
}

/// 第二輪潤飾的系統提示詞的各部分：同一個預設組，加上「修改草稿」的說明。
pub fn refine_parts(p: &Params, batch: &[Segment], terms: &[(String, String)]) -> Vec<PromptPart> {
    let texts: Vec<&str> = batch.iter().map(|g| g.text.as_str()).collect();
    vec![
        PromptPart::new("preset", false, render_template(preset_text(p), p)),
        PromptPart::new(
            "refine",
            true,
            "\n\nYou are now revising draft translations. For each passage you get the original text and a draft translation. \
             Correct mistranslations and omissions, make the wording natural and fluent, and keep terminology consistent. \
             If a draft is already good, return it unchanged."
                .to_string(),
        ),
        PromptPart::new("rules", true, fixed_rules(p)),
        PromptPart::new(
            "format",
            true,
            "- The input lists each original as <src id=\"N\">…</src> followed by its draft as <seg id=\"N\">…</seg>. \
             Reply with only the revised translations, each wrapped as <seg id=\"N\">…</seg>, one per passage, in the same order, with the same ids, and nothing else.\n"
                .to_string(),
        ),
        PromptPart::new("glossary", true, glossary_block(&p.glossary, terms, &texts)),
    ]
}

/// 第二輪潤飾的系統提示詞。
pub fn refine_prompt(p: &Params, batch: &[Segment], terms: &[(String, String)]) -> String {
    join(&refine_parts(p, batch, terms))
}

/// 第二輪潤飾送出的內容：原文用 `<src>`、草稿用 `<seg>`（回覆只有 `<seg>`，解析照舊）。
pub fn refine_message(batch: &[Segment], drafts: &[String]) -> String {
    let mut s = String::new();
    for (seg, d) in batch.iter().zip(drafts) {
        s.push_str(&format!(
            "<src id=\"{}\">{}</src>\n<seg id=\"{}\">{}</seg>\n\n",
            seg.id, seg.text, seg.id, d
        ));
    }
    s
}

// ---------- 回覆 ----------

/// 把回覆裡的 `<terms>…</terms>` 拿出來：交回（去掉它之後的回覆, 術語對）。沒有就是原樣與空的。
/// 只在最後一段 `</seg>` 之後找（原文裡的 `<terms>` 字樣、沒收尾的術語不會切掉段落）。
/// 每行 `原文 => 譯法`；讀不懂的行略過。
pub fn split_terms(reply: &str) -> (String, Vec<(String, String)>) {
    let from = reply.rfind("</seg>").map_or(0, |i| i + "</seg>".len());
    let Some(open) = reply[from..].find("<terms>").map(|i| i + from) else {
        return (reply.to_string(), Vec::new());
    };
    let body_start = open + "<terms>".len();
    let (body, end) = match reply[body_start..].find("</terms>") {
        Some(e) => (
            &reply[body_start..body_start + e],
            body_start + e + "</terms>".len(),
        ),
        None => (&reply[body_start..], reply.len()),
    };
    let mut terms = Vec::new();
    for line in body.lines() {
        let line = line.trim().trim_start_matches(['-', '*', '•']).trim();
        let Some((from, to)) = line.split_once("=>") else {
            continue;
        };
        let clean = |s: &str| s.trim().trim_matches('`').trim().to_string();
        let (from, to) = (clean(from), clean(to));
        let ok = |s: &str| {
            let n = s.chars().count();
            n > 0
                && n <= MAX_TERM_CHARS
                && !s.contains(['<', '>'])
                && !s.chars().any(char::is_control)
        };
        if ok(&from) && ok(&to) {
            terms.push((from, to));
        }
    }
    let mut rest = String::with_capacity(reply.len());
    rest.push_str(&reply[..open]);
    rest.push_str(&reply[end..]);
    (rest, terms)
}

/// 解析回覆。段數或編號不符就回報 `BadResponse`（交給上層縮小批次再試）。
/// 回覆裡的 `<terms>` 先拿掉（`split_terms`），不會被當成譯文。
pub fn parse_reply(reply: &str, batch: &[Segment]) -> Result<Vec<String>> {
    let (reply, _) = split_terms(reply);
    let reply = reply.as_str();
    let mut out = Vec::with_capacity(batch.len());
    let mut cursor = 0usize;
    for seg in batch {
        let open = format!("<seg id=\"{}\">", seg.id);
        let found = reply[cursor..].find(&open).map(|i| i + cursor);
        let Some(start) = found else {
            // 只有一段時，模型沒有包標籤也可以收（整段回覆就是譯文）
            if batch.len() == 1 {
                let t = strip_tags(reply).trim().to_string();
                if !t.is_empty() {
                    return Ok(vec![t]);
                }
            }
            return Err(TranslateError::new(
                ErrorKind::BadResponse,
                format!("segment {} is missing in the reply", seg.id),
            ));
        };
        let body_start = start + open.len();
        let Some(rel_end) = reply[body_start..].find("</seg>") else {
            return Err(TranslateError::new(
                ErrorKind::BadResponse,
                format!("segment {} is not closed", seg.id),
            ));
        };
        let body = reply[body_start..body_start + rel_end].trim().to_string();
        if body.is_empty() && !seg.text.trim().is_empty() {
            return Err(TranslateError::new(
                ErrorKind::BadResponse,
                format!("segment {} came back empty", seg.id),
            ));
        }
        out.push(body);
        cursor = body_start + rel_end + "</seg>".len();
    }
    Ok(out)
}

fn strip_tags(s: &str) -> String {
    let mut t = s.trim().to_string();
    if let Some(i) = t.find('>')
        && t.starts_with("<seg ")
    {
        t = t[i + 1..].to_string();
    }
    if let Some(stripped) = t.trim_end().strip_suffix("</seg>") {
        t = stripped.to_string();
    }
    t
}

/// 一批翻完、譯文定下之後（潤飾、單段重翻之後），把這一批回覆附的術語記進這篇的術語：
/// - 原文詞至少 3 個字、整個詞出現在這一批的原文裡，譯法出現在這一批**留下的**譯文裡，才算這一批用了這個譯法；
/// - 第一次是候選（不用），同一個譯法用滿兩批才定下（照用），兩批不必相連：每個譯法用過幾批記在 `tally`
///   （這次工作的記憶體裡），在幾種譯法之間來回換的詞也定得下來；候選只留最近的那個譯法（一個詞只有一條，
///   譯文疊加檔裡原文詞不重複），批數是那個譯法用過的批數；
/// - 術語庫已有的原文詞、已經定下的、使用者定的或刪掉的，都不動；
/// - 照用的最多 [`MAX_PAPER_TERMS`] 條，連同候選最多 [`MAX_KNOWN_TERMS`] 條。
///
/// 交回這一批新定下的條數。
pub fn learn_terms(
    known: &mut Vec<PaperTerm>,
    tally: &mut HashMap<(String, String), u32>,
    found: Vec<(String, String)>,
    sources: &[&str],
    outputs: &[&str],
    glossary: &[(String, String)],
) -> usize {
    let src: Vec<String> = sources.iter().map(|s| s.to_lowercase()).collect();
    let out: Vec<String> = outputs.iter().map(|s| s.to_lowercase()).collect();
    let in_glossary: HashSet<String> = glossary.iter().map(|(f, _)| f.to_lowercase()).collect();
    let mut seen_this_batch: HashSet<(String, String)> = HashSet::new();
    let mut learned = 0;
    for (from, to) in found {
        let (f, t) = (from.trim().to_lowercase(), to.trim().to_string());
        if f.chars().count() < MIN_LEARNED_CHARS
            || in_glossary.contains(&f)
            || !src.iter().any(|s| has_word(s, &f))
            || !out.iter().any(|s| s.contains(&t.to_lowercase()))
            || !seen_this_batch.insert((f.clone(), t.clone()))
        {
            continue;
        }
        let same_word = |k: &PaperTerm| k.source.trim().to_lowercase() == f;
        if known
            .iter()
            .any(|k| same_word(k) && k.state != TermState::Candidate)
        {
            continue;
        }
        // 這個譯法用過幾批：記憶體裡沒有時（接著翻）從候選記的批數算起
        let carried = known
            .iter()
            .find(|k| same_word(k) && k.target.trim() == t)
            .map_or(0, |k| k.batches);
        let count = tally.entry((f.clone(), t.clone())).or_insert(carried);
        *count += 1;
        let count = *count;
        // 一個詞只留一個候選（譯文疊加檔裡原文詞不重複）：換成這個譯法與它用過的批數
        match known.iter().position(&same_word) {
            Some(i) => {
                known[i].target = t.clone();
                known[i].batches = count;
                let in_use = known
                    .iter()
                    .filter(|k| matches!(k.state, TermState::Learned | TermState::User))
                    .count();
                if known[i].batches >= 2 && in_use < MAX_PAPER_TERMS {
                    known[i].state = TermState::Learned;
                    learned += 1;
                    // 同一個詞的其他候選不要了
                    let mut j = 0;
                    known.retain(|o| {
                        let keep = j == i || !same_word(o);
                        j += 1;
                        keep
                    });
                }
            }
            None if known.len() < MAX_KNOWN_TERMS => known.push(PaperTerm {
                source: from.trim().to_string(),
                target: t,
                state: TermState::Candidate,
                batches: count,
            }),
            None => {}
        }
    }
    learned
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(id: &str, text: &str) -> Segment {
        Segment {
            id: id.into(),
            text: text.into(),
            before: None,
            ..Default::default()
        }
    }

    #[test]
    fn parses_tagged_reply_in_order() {
        let batch = [seg("a", "One"), seg("b", "Two")];
        let reply = "Sure!\n<seg id=\"a\">一</seg>\n<seg id=\"b\">二 \\alpha < 3</seg>\n";
        assert_eq!(
            parse_reply(reply, &batch).unwrap(),
            vec!["一", "二 \\alpha < 3"]
        );
    }

    #[test]
    fn missing_or_unclosed_segment_is_a_bad_response() {
        let batch = [seg("a", "One"), seg("b", "Two")];
        let e = parse_reply("<seg id=\"a\">一</seg>", &batch).unwrap_err();
        assert_eq!(e.kind, ErrorKind::BadResponse);
        let e = parse_reply("<seg id=\"a\">一", &batch).unwrap_err();
        assert_eq!(e.kind, ErrorKind::BadResponse);
        let e = parse_reply("<seg id=\"a\"></seg><seg id=\"b\">二</seg>", &batch).unwrap_err();
        assert_eq!(
            e.kind,
            ErrorKind::BadResponse,
            "an empty translation of real text is not accepted"
        );
    }

    #[test]
    fn a_single_segment_may_come_back_without_tags() {
        let batch = [seg("a", "One")];
        assert_eq!(parse_reply("  一  ", &batch).unwrap(), vec!["一"]);
        assert_eq!(
            parse_reply("<seg id=\"zz\">一</seg>", &batch).unwrap(),
            vec!["一"]
        );
        // 附在後面的術語不會變成譯文
        assert_eq!(
            parse_reply("一\n<terms>\nOne => 一\n</terms>", &batch).unwrap(),
            vec!["一"]
        );
    }

    #[test]
    fn the_terms_block_is_read_and_removed() {
        let (rest, terms) = split_terms(
            "<seg id=\"1\">詞元</seg>\n<terms>\n- token => 詞元\n`stretch` => 伸展參數\nno arrow\n <b> => x\n</terms>\n",
        );
        assert_eq!(rest, "<seg id=\"1\">詞元</seg>\n\n");
        assert_eq!(
            terms,
            vec![
                ("token".to_string(), "詞元".to_string()),
                ("stretch".to_string(), "伸展參數".to_string())
            ]
        );
        assert_eq!(split_terms("no block").1, vec![]);
    }

    fn pt(source: &str, target: &str, state: TermState, batches: u32) -> PaperTerm {
        PaperTerm {
            source: source.into(),
            target: target.into(),
            state,
            batches,
        }
    }

    fn pair(s: &str, t: &str) -> (String, String) {
        (s.to_string(), t.to_string())
    }

    #[test]
    fn a_term_is_used_only_after_two_batches_agree() {
        // 第一批的譯法只是候選，第二批又是同一個譯法才定下（避免第一批的錯直接定案）
        let mut known: Vec<PaperTerm> = Vec::new();
        let mut tally = std::collections::HashMap::new();
        let src = ["The token goes in."];
        let n = learn_terms(
            &mut known,
            &mut tally,
            vec![pair("token", "權杖")],
            &src,
            &["權杖進去。"],
            &[],
        );
        assert_eq!(n, 0);
        assert_eq!(known, vec![pt("token", "權杖", TermState::Candidate, 1)]);
        assert!(terms_in_use(&known).is_empty(), "a candidate is not sent");
        // 第二批換了譯法：候選換成新的（一個詞只留一個候選：譯文疊加檔裡原文詞不重複）
        learn_terms(
            &mut known,
            &mut tally,
            vec![pair("token", "詞元")],
            &src,
            &["詞元進去。"],
            &[],
        );
        assert_eq!(known, vec![pt("token", "詞元", TermState::Candidate, 1)]);
        assert!(terms_in_use(&known).is_empty());
        // 第三批又是「詞元」：定下
        let n = learn_terms(
            &mut known,
            &mut tally,
            vec![pair("Token", "詞元")],
            &src,
            &["詞元進去。"],
            &[],
        );
        assert_eq!(n, 1);
        assert_eq!(known, vec![pt("token", "詞元", TermState::Learned, 2)]);
        assert_eq!(terms_in_use(&known), vec![pair("token", "詞元")]);
        // 定下的不再被換掉
        learn_terms(
            &mut known,
            &mut tally,
            vec![pair("token", "符記")],
            &src,
            &["符記"],
            &[],
        );
        learn_terms(
            &mut known,
            &mut tally,
            vec![pair("token", "符記")],
            &src,
            &["符記"],
            &[],
        );
        assert_eq!(terms_in_use(&known), vec![pair("token", "詞元")]);
    }

    #[test]
    fn two_batches_with_the_same_translation_count_even_when_not_in_a_row() {
        // fallback 在「後備」「回退」之間來回換：任一個譯法用滿兩批就定下
        let mut known: Vec<PaperTerm> = Vec::new();
        let mut tally = std::collections::HashMap::new();
        let src = ["The fallback is used."];
        let out = ["後備、回退"];
        for t in ["後備", "回退"] {
            learn_terms(
                &mut known,
                &mut tally,
                vec![pair("fallback", t)],
                &src,
                &out,
                &[],
            );
            assert!(terms_in_use(&known).is_empty());
        }
        assert_eq!(known, vec![pt("fallback", "回退", TermState::Candidate, 1)]);
        let n = learn_terms(
            &mut known,
            &mut tally,
            vec![pair("fallback", "後備")],
            &src,
            &out,
            &[],
        );
        assert_eq!(n, 1);
        assert_eq!(known, vec![pt("fallback", "後備", TermState::Learned, 2)]);
        // 接著翻（記憶體裡的次數沒了）：候選記的批數照算
        let mut known = vec![pt("token", "詞元", TermState::Candidate, 1)];
        let mut tally = std::collections::HashMap::new();
        let n = learn_terms(
            &mut known,
            &mut tally,
            vec![pair("token", "詞元")],
            &["A token."],
            &["詞元"],
            &[],
        );
        assert_eq!(n, 1);
    }

    #[test]
    fn each_word_is_listed_once_whatever_the_model_proposes() {
        // stretch 有六種譯法、token 三種。一批裡同一個詞兩種譯法、大小寫不同、批次之間來回換：
        // 一個詞永遠只有一條（譯文疊加檔的規則）
        let mut known: Vec<PaperTerm> = Vec::new();
        let mut tally = std::collections::HashMap::new();
        let src = ["The stretch and the Stretch factor."];
        let out = ["伸展、伸縮、拉伸、伸展參數"];
        for found in [
            vec![pair("stretch", "伸展"), pair("Stretch", "伸縮")],
            vec![pair("stretch", "拉伸")],
            vec![pair("STRETCH", "伸展參數"), pair("stretch", "伸展")],
            vec![pair("stretch", "伸展")],
        ] {
            learn_terms(&mut known, &mut tally, found, &src, &out, &[]);
            let mut words: Vec<String> = known.iter().map(|k| k.source.to_lowercase()).collect();
            words.dedup();
            assert_eq!(words.len(), known.len(), "{known:?}");
        }
    }

    #[test]
    fn the_users_decisions_and_the_glossary_are_never_overridden() {
        let mut known = vec![
            pt("policy", "策略", TermState::User, 0),
            pt("state", "", TermState::Removed, 0),
        ];
        let mut tally = std::collections::HashMap::new();
        let glossary = vec![pair("reward", "獎勵")];
        let src = ["The policy, state and reward."];
        let out = ["政策、州與回報。"];
        for _ in 0..3 {
            learn_terms(
                &mut known,
                &mut tally,
                vec![
                    pair("policy", "政策"),
                    pair("state", "州"),
                    pair("reward", "回報"),
                ],
                &src,
                &out,
                &glossary,
            );
        }
        assert_eq!(
            known,
            vec![
                pt("policy", "策略", TermState::User, 0),
                pt("state", "", TermState::Removed, 0),
            ]
        );
        assert_eq!(terms_in_use(&known), vec![pair("policy", "策略")]);
    }

    #[test]
    fn learned_terms_are_whole_words_of_three_or_more_characters() {
        // `state` 不能套到 statement、`RL` 不能套到 URL
        let mut known: Vec<PaperTerm> = Vec::new();
        let mut tally = std::collections::HashMap::new();
        for _ in 0..2 {
            learn_terms(
                &mut known,
                &mut tally,
                vec![
                    pair("state", "狀態"),
                    pair("RL", "強化學習"),
                    pair("gain", "增益"),
                ],
                &["A statement about the URL and the gain."],
                &["狀態、強化學習與增益"],
                &[],
            );
        }
        assert_eq!(terms_in_use(&known), vec![pair("gain", "增益")]);
        // 送出時也照詞的邊界挑
        let terms = vec![pair("state", "狀態"), pair("gain", "增益")];
        let b = glossary_block(&[], &terms, &["The statement has gains."]);
        assert!(b.is_empty(), "{b}");
        let b = glossary_block(&[], &terms, &["The state (gain) is high."]);
        assert!(b.contains("- state => 狀態\n- gain => 增益\n"), "{b}");
    }

    #[test]
    fn the_terms_block_is_only_read_after_the_last_passage() {
        // 原文裡有 `<terms>` 字樣、或模型先寫了術語又沒收尾，不能切掉後面的段落
        let batch = [seg("1", "a"), seg("2", "b")];
        let reply = "<seg id=\"1\">含 <terms> 字樣</seg>\n<seg id=\"2\">二</seg>\n<terms>\ngain => 增益\n</terms>";
        assert_eq!(
            parse_reply(reply, &batch).unwrap(),
            vec!["含 <terms> 字樣", "二"]
        );
        assert_eq!(split_terms(reply).1, vec![pair("gain", "增益")]);
        let reply = "<terms>\nx => y\n<seg id=\"1\">一</seg>\n<seg id=\"2\">二</seg>";
        assert_eq!(parse_reply(reply, &batch).unwrap(), vec!["一", "二"]);
    }

    fn params() -> Params {
        Params {
            source_lang: Some("en".into()),
            target_lang: "zh-TW".into(),
            glossary: vec![
                ("attention".into(), "注意力".into()),
                ("transformer".into(), "Transformer".into()),
            ],
            ..Params::default()
        }
    }

    #[test]
    fn prompt_keeps_formulas_and_lists_only_the_terms_in_the_batch() {
        let p = params();
        let s = system_prompt(&p, &[seg("1", "Self-Attention is all you need.")], &[]);
        assert!(s.contains("Traditional Chinese"));
        assert!(s.contains("formulas"));
        assert!(s.contains("[12]"));
        assert!(
            s.contains("attention => 注意力"),
            "matched case-insensitively"
        );
        assert!(
            !s.contains("transformer =>"),
            "terms that do not occur are not sent"
        );
        assert!(s.contains("Taiwan") && s.contains("「」"));
        assert!(s.contains("full-width punctuation"));
        let s = system_prompt(
            &Params {
                target_lang: "ja".into(),
                ..params()
            },
            &[seg("1", "x")],
            &[],
        );
        assert!(!s.contains("Taiwan"), "the Taiwan rule is only for zh-TW");
        assert!(
            !s.contains("full-width"),
            "the punctuation rule is only for Chinese"
        );
    }

    #[test]
    fn paper_terms_join_the_glossary_and_the_glossary_wins() {
        let p = params();
        let terms = vec![
            ("token".to_string(), "詞元".to_string()),
            ("Attention".to_string(), "關注".to_string()),
            ("unused".to_string(), "沒用到".to_string()),
        ];
        let s = system_prompt(&p, &[seg("1", "Attention over each token.")], &terms);
        assert!(
            s.contains("- attention => 注意力\n- token => 詞元\n"),
            "{s}"
        );
        assert!(!s.contains("關注") && !s.contains("沒用到"));
    }

    #[test]
    fn user_presets_cannot_drop_the_format_rules() {
        let p = Params {
            instructions: "Translate into {{to}} like a poet. Title: {{title}}".into(),
            title: Some("Deep Nets".into()),
            ..params()
        };
        let s = system_prompt(&p, &[seg("1", "x")], &[]);
        assert!(s.starts_with(
            "Translate into Traditional Chinese (繁體中文) like a poet. Title: Deep Nets"
        ));
        assert!(s.contains("<seg id=\"N\">"));
        assert!(s.contains("<terms>"));
        assert!(s.contains("formulas"));
        // 只有預設組那一部分是使用者改得到的
        let parts = translate_parts(&p, &[seg("1", "x")], &[]);
        assert_eq!(
            parts
                .iter()
                .filter(|x| !x.fixed)
                .map(|x| x.key)
                .collect::<Vec<_>>(),
            vec!["preset"]
        );
    }

    #[test]
    fn templates_only_take_known_variables() {
        assert!(check_template("from {{from}} to {{ to }} — {{title}}").is_ok());
        assert_eq!(check_template("{{text}}"), Err("text".to_string()));
        assert!(check_template("{{to").is_err());
        for b in BUILTIN_PRESETS {
            assert!(check_template(b.text).is_ok(), "{}", b.id);
            for s in b.sections {
                assert!(check_template(s.about).is_ok(), "{}", s.id);
            }
        }
        let p = Params {
            source_lang: None,
            ..params()
        };
        assert!(
            render_template("{{from}}|{{title}}", &p).contains("original language|(not given)")
        );
    }

    #[test]
    fn builtin_presets_declare_their_use() {
        assert_eq!(
            builtin_preset("general").unwrap().kind,
            PresetKind::Translation
        );
        assert_eq!(
            builtin_preset("academic").unwrap().kind,
            PresetKind::Translation
        );
        let q = builtin_preset(DEFAULT_QUICK_PRESET).unwrap();
        assert_eq!(q.kind, PresetKind::Quick);
        assert!(q.sections.is_empty());
        let s = builtin_preset(DEFAULT_DIGEST_PRESET).unwrap();
        assert_eq!(s.kind, PresetKind::Digest);
        let ids: Vec<&str> = s.sections.iter().map(|x| x.id).collect();
        assert_eq!(
            ids,
            ["question", "method", "results", "limitations", "quotes"]
        );
        assert!(s.text.contains("contradicts itself"));
        assert!(s.text.contains("states about its own work"));
    }

    #[test]
    fn refine_sends_the_source_apart_from_the_draft() {
        let batch = [seg("1", "Hello"), seg("2", "World")];
        let m = refine_message(&batch, &["你好".into(), "世界".into()]);
        assert!(m.contains("<src id=\"1\">Hello</src>\n<seg id=\"1\">你好</seg>"));
        let s = refine_prompt(&params(), &batch, &[]);
        assert!(s.contains("revising draft translations"));
        assert!(s.contains("<src id=\"N\">"));
        assert!(!s.contains("<terms>"), "refining does not ask for terms");
    }

    #[test]
    fn continuations_are_noted_outside_the_passage_tags() {
        let a = seg("g0108", "Our intuition is that the robot");
        let b = Segment {
            continues: Some("g0108".into()),
            ..seg("g0109", "should act consistently.")
        };
        let m = user_message(&[a.clone(), b.clone()], None);
        assert!(
            m.contains("<seg id=\"g0109\">should act consistently.</seg>\n"),
            "{m}"
        );
        assert!(m.ends_with("\nNote: passage g0109 continues passage g0108 (one sentence split across columns or pages).\n"), "{m}");
        // 不是一批的第一段：送出的前文不是它的前一段，不註明
        let m = user_message(&[seg("x", "Other."), b.clone()], Some("Something else"));
        assert!(!m.contains("Note:"), "{m}");
        // 前一段已翻好、不在這一批：前文就是它，註明接著前文
        let m = user_message(
            std::slice::from_ref(&b),
            Some("Our intuition is that the robot"),
        );
        assert!(
            m.contains("Note: passage g0109 continues the sentence in the context before it"),
            "{m}"
        );
        // 前一段單獨重翻：後一段當後文
        let one = Segment {
            after: Some("should act consistently.".into()),
            ..a
        };
        let m = user_message(&[one], None);
        assert!(
            m.contains("</seg>\n\n<context>\nshould act consistently.\n</context>\n"),
            "{m}"
        );
        assert!(
            m.contains("Note: passage g0108 is continued by the sentence in the context after it"),
            "{m}"
        );
        // 註明不放進標籤裡：解析照舊
        let s = system_prompt(&params(), &[seg("1", "x")], &[]);
        assert!(s.contains("A note after the passages may say"));
        assert!(!user_message(&[seg("1", "x")], None).contains("Note:"));
    }

    #[test]
    fn the_fixed_rules_cover_citations_cross_references_and_echoes() {
        let s = system_prompt(&params(), &[seg("1", "x")], &[]);
        assert!(s.contains("Citation markers keep their original half-width brackets"));
        assert!(s.contains("Do not append the whole original passage"));
        // 章節、圖、表、式的指引：字照目標語言翻，編號照原文；中文的例子只在譯成中文時給
        assert!(
            s.contains("translate the word") && s.contains("keep the number"),
            "{s}"
        );
        assert!(
            s.contains("(Section 4.2) → (第 4.2 節)") && s.contains("Fig. 3 → 圖 3"),
            "{s}"
        );
        let cn = system_prompt(
            &Params {
                target_lang: "zh-CN".into(),
                ..params()
            },
            &[seg("1", "x")],
            &[],
        );
        assert!(
            cn.contains("(Section 4.2) → (第 4.2 节)") && cn.contains("Fig. 3 → 图 3"),
            "{cn}"
        );
        let ja = system_prompt(
            &Params {
                target_lang: "ja".into(),
                ..params()
            },
            &[seg("1", "x")],
            &[],
        );
        assert!(
            ja.contains("translate the word") && !ja.contains("4.2 節") && !ja.contains("4.2 节"),
            "{ja}"
        );
        // 指引只說明要翻，不附英文的例句
        assert!(!s.contains("for example (Section 4.2), Fig. 3"), "{s}");
    }

    #[test]
    fn the_academic_preset_has_one_rule_for_parenthesized_english() {
        let a = builtin_preset("academic").unwrap().text;
        assert!(!a.contains("when a standard translation exists"), "{a}");
        assert!(a.contains("without parentheses"), "{a}");
        assert!(a.contains("only in the target language"), "{a}");
        assert!(a.contains("full form of an acronym"), "{a}");
        assert!(a.contains("分布 (not 分佈)") && a.contains("笛卡兒 (not 笛卡爾)"));
        assert!(check_template(a).is_ok());
    }

    #[test]
    fn user_message_marks_context() {
        let m = user_message(&[seg("1", "Hi")], Some("Before."));
        assert!(m.starts_with("<context>\nBefore.\n</context>"));
        assert!(m.contains("<seg id=\"1\">Hi</seg>"));
    }
}
