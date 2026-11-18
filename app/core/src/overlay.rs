//! 譯文疊加檔：一份 JSON，同時是「版面紀錄」與「譯本」。
//!
//! # 在文獻庫裡的位置
//!
//! 檔案放在那一篇的資料夾裡，檔名 `translation-overlay.json`（被占用時 `translation-overlay-2.json` …），
//! 並在 `paper.json` 的材料清單加一項：`role` 是 `translation`、`form` 是 `overlay`。
//! 這一項的 `sha256` 與 `bytes` 一律是 `null`：檔案每翻完一批就會改，指紋與大小沒有意義；
//! 所以 `check` 對它只確認檔案存在、讀得懂，不比對指紋與大小。
//! 一份譯本的來源（哪一份原文）記在檔案的 `source_material`，並在材料的 `provenance.source_sha256`
//! 留下當時那份原文的指紋，原文被換掉時看得出來。
//!
//! # 檔案內容
//!
//! ```json
//! {
//!   "format": 1,
//!   "source_material": "<材料代號>",
//!   "provider": "anthropic",         // 引擎（自由字串，核心不解讀）
//!   "model": "claude-…",             // 模型名稱；機器翻譯服務是空字串
//!   "source_lang": "en",             // 沒有指定時 null
//!   "target_lang": "zh-TW",
//!   "created": "…", "created_by": "<裝置代號>",
//!   "modified": "…", "modified_by": "<裝置代號>",
//!   "segments": [
//!     { "id": "g0001", "page": 0, "bbox": [x0, y0, x1, y1], "kind": "body",
//!       "size": 9.96, "source": "原文", "target": null, "status": "pending", "error": null }
//!   ]
//! }
//! ```
//!
//! - 段落的順序就是閱讀順序；`id` 由核心依順序編成 `g0001`、`g0002` …，建立之後不變。
//! - `page` 是原文那份材料的頁序（從 0 起算）；`bbox` 是 PDF 頁面座標（點，原點在左下），
//!   `[x0, y0, x1, y1]` 且 `x0 < x1`、`y0 < y1`。
//! - `status`：`pending`（待翻）、`done`（已翻，`target` 有字）、`failed`（翻不出來，`error` 記原因的種類）。
//!   `pending` 與 `failed` 的 `target` 是 `null`。
//! - 重新翻譯、接著翻：把 `pending` 與 `failed` 的段落再送翻，已 `done` 的不重翻。
//! - 每一段的 `issues`（自動檢查的問題代號）、`modified`、`modified_by`、`service`（翻這一段的服務）
//!   寫在譯文疊加檔裡。
//!
//! # 寫入的規則（與 `ink.json` 相同）
//!
//! 原子寫入；每次寫入帶檔案自己的版本記號，不符回報 `Conflict`、什麼都不寫；讀不懂的檔案不覆寫；
//! `format` 比認得的新就拒絕寫入；每個結構都保留不認得的欄位。
//! 更新一批段落只寫這一個檔案，不碰 `paper.json`。

use crate::error::{LibError, Result};
use crate::format::{Map, format_or_current, null_as_default, one};
use serde::{Deserialize, Serialize};

pub const OVERLAY_FORM: &str = "overlay";
pub const METHOD_GENERATED: &str = "generated";

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";

/// 一份譯本最多幾段。
pub const MAX_SEGMENTS: usize = 20_000;
/// 一段原文或譯文最多幾個字元。
pub const MAX_TEXT_CHARS: usize = 20_000;
/// 座標的絕對值上限（PDF 的點）。
pub const MAX_COORD: f64 = 1_000_000.0;
const MAX_PAGE_INDEX: u32 = 100_000;
const MAX_SHORT: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlayFile {
    #[serde(default = "one", deserialize_with = "format_or_current")]
    pub format: u32,
    #[serde(default)]
    pub source_material: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub source_lang: Option<String>,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub modified: Option<String>,
    #[serde(default)]
    pub modified_by: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub segments: Vec<OverlaySegment>,
    /// 這份譯本翻譯時定下的術語：之後的批次與同一篇的重點筆記照用，
    /// 使用者可以在「檢查譯文」改、刪。沒有時不寫出；這一版之前的檔沒有它。
    #[serde(
        default,
        deserialize_with = "null_as_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub terms: Vec<OverlayTerm>,
    #[serde(flatten)]
    pub extra: Map,
}

/// 術語的狀態：只在一批裡出現過（還不用）、兩批以上同一個譯法（照用）、使用者定的（照用，學到的不蓋掉）、
/// 使用者刪掉的（不用，也不再學）。
pub const TERM_CANDIDATE: &str = "candidate";
pub const TERM_LEARNED: &str = "learned";
pub const TERM_USER: &str = "user";
pub const TERM_REMOVED: &str = "removed";
/// 一份譯本最多記幾條術語、一條的原文與譯法最多幾個字。
pub const MAX_TERMS: usize = 600;
pub const MAX_TERM_CHARS: usize = 100;

/// 一條術語：原文詞 → 譯法。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlayTerm {
    pub source: String,
    /// 譯法；`removed` 的可以是空字串。
    #[serde(default)]
    pub target: String,
    pub state: String,
    /// 出現過這個譯法的批數（`candidate`、`learned` 用；其餘是 0）。
    #[serde(default)]
    pub batches: u32,
    #[serde(flatten)]
    pub extra: Map,
}

fn term_rank(state: &str) -> u8 {
    match state {
        TERM_USER | TERM_REMOVED => 3,
        TERM_LEARNED => 2,
        _ => 1,
    }
}

/// 術語合不合規則：最多 `MAX_TERMS` 條；狀態是 1 到 20 個 `a`–`z` 與 `_`（這一版不認得的狀態原樣保留、不用，
/// 較新版本加的狀態不擋寫入）；原文 1 到 `MAX_TERM_CHARS` 個字、同一份裡（不分大小寫）不重複；
/// 譯法最多 `MAX_TERM_CHARS` 個字，除了 `removed` 都要有字；都不含控制字元。
pub fn validate_terms(terms: &[OverlayTerm]) -> Result<()> {
    if terms.len() > MAX_TERMS {
        return Err(bad(format!("at most {MAX_TERMS} terms")));
    }
    let mut seen = std::collections::HashSet::new();
    for (i, t) in terms.iter().enumerate() {
        if !(1..=20).contains(&t.state.len())
            || !t.state.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        {
            return Err(bad(format!("term {i}: bad state {:?}", t.state)));
        }
        let text_ok = |s: &str, may_be_empty: bool| {
            let n = s.trim().chars().count();
            (may_be_empty || n > 0) && n <= MAX_TERM_CHARS && !s.chars().any(char::is_control)
        };
        if !text_ok(&t.source, false) || !text_ok(&t.target, t.state == TERM_REMOVED) {
            return Err(bad(format!(
                "term {i}: source and target must be 1 to {MAX_TERM_CHARS} characters on one line"
            )));
        }
        if !seen.insert(t.source.trim().to_lowercase()) {
            return Err(bad(format!("term {i}: {:?} is listed twice", t.source)));
        }
    }
    Ok(())
}

/// 兩台的術語合併（同步）：依原文詞（不分大小寫）；只在一邊的照收；兩邊都有時使用者的決定（改、刪）勝過學到的，
/// 學到的勝過還在候選的；同一級留本機的。順序是本機的在前、只在對方的接在後面。
pub fn merge_terms(local: &[OverlayTerm], remote: &[OverlayTerm]) -> Vec<OverlayTerm> {
    let key = |t: &OverlayTerm| t.source.trim().to_lowercase();
    let mut out: Vec<OverlayTerm> = local
        .iter()
        .map(|l| match remote.iter().find(|r| key(r) == key(l)) {
            Some(r) if term_rank(&r.state) > term_rank(&l.state) => r.clone(),
            _ => l.clone(),
        })
        .collect();
    for r in remote {
        if !local.iter().any(|l| key(l) == key(r)) {
            out.push(r.clone());
        }
    }
    out.truncate(MAX_TERMS);
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlaySegment {
    pub id: String,
    /// 頁序，從 0 起算（原文那份材料的頁序）。
    pub page: u32,
    /// `[x0, y0, x1, y1]`，PDF 頁面座標。
    pub bbox: [f64; 4],
    /// 種類：`body`、`caption`、`heading` 等；核心不解讀，空字串當作 `body`。
    #[serde(default)]
    pub kind: String,
    /// 主要的字級（點），畫疊加時用來決定譯文的大小；不知道是 `null`。
    #[serde(default)]
    pub size: Option<f64>,
    pub source: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    /// 已翻的段落經自動檢查留下的問題代號（例如 `missing_citation`），沒有問題時不寫出這個欄位。
    /// 只記代號，不記原文內容。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<String>,
    /// 這一段最後一次更新的時間與裝置；建立時是 `null`，`null` 時照樣寫出。
    #[serde(default)]
    pub modified: Option<String>,
    #[serde(default)]
    pub modified_by: Option<String>,
    /// 這一段是哪個翻譯服務翻的。已翻與翻不出來的段落記下；待翻的沒有，沒有時不寫出這個欄位。
    /// 這一版之前寫的檔沒有它。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<SegmentService>,
    /// 這一段的譯文是使用者決定的（在並排比較按「用這個」、日後的就地修改）：任何整批的翻譯都不蓋掉它，
    /// 只有使用者在那一段明確重翻才換。只有已翻的段落會是 `true`；`false` 時不寫出。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub by_user: bool,
    #[serde(flatten)]
    pub extra: Map,
}

/// 翻一段的服務。`id` 是使用者在翻譯服務設定裡的那一筆的代號，只在那台裝置的設定查得到，
/// 所以同時記引擎種類與模型：別台、或那一筆已經刪掉時，看這兩個欄位。網址與主機不記。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentService {
    pub id: String,
    /// 引擎種類，與檔頭的 `provider` 同一套值（核心不解讀）。
    pub provider: String,
    /// 模型名稱；機器翻譯服務是空字串。
    #[serde(default)]
    pub model: String,
    #[serde(flatten)]
    pub extra: Map,
}

/// 一段最多記幾個問題代號、每個代號多長。
pub const MAX_ISSUES: usize = 16;
pub const MAX_ISSUE_CHARS: usize = 40;

/// 建立時畫面交來的一段（只有版面與原文）。
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentDraft {
    pub page: u32,
    pub bbox: [f64; 4],
    pub kind: String,
    pub size: Option<f64>,
    pub source: String,
}

/// 建立譯文疊加檔要的資料。
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayDraft {
    /// 翻的是哪一份原文材料。
    pub source_material: String,
    pub provider: String,
    pub model: String,
    pub source_lang: Option<String>,
    pub target_lang: String,
    pub segments: Vec<SegmentDraft>,
}

/// 更新一段的結果。`status` 是 `done` 時 `target` 必須有字；`failed` 時 `error` 記原因的種類；
/// `pending` 把這一段清回待翻。`issues` 只對 `done` 有意義（自動檢查留下的問題代號），其餘狀態要是空的。
/// `service` 是翻這一段的服務（`done`、`failed` 時記下；`pending` 要是 `None`）。
/// `by_user` 是「這段譯文是使用者決定的」（只有 `done` 可以是 `true`）；寫入時照它設定那一段。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SegmentUpdate {
    pub id: String,
    pub status: String,
    pub target: Option<String>,
    pub error: Option<String>,
    pub issues: Vec<String>,
    pub service: Option<SegmentService>,
    pub by_user: bool,
}

fn bad(reason: impl Into<String>) -> LibError {
    LibError::InvalidInput {
        reason: reason.into(),
    }
}

fn coord_ok(x: f64) -> bool {
    x.is_finite() && x.abs() <= MAX_COORD
}

pub(crate) fn validate_draft(d: &OverlayDraft, pages: Option<u32>) -> Result<()> {
    if d.segments.len() > MAX_SEGMENTS {
        return Err(bad(format!(
            "too many segments: {} (max {MAX_SEGMENTS})",
            d.segments.len()
        )));
    }
    if d.target_lang.trim().is_empty() || d.target_lang.chars().count() > MAX_SHORT {
        return Err(bad("target language must be 1 to 200 characters"));
    }
    if d.provider.trim().is_empty() || d.provider.chars().count() > MAX_SHORT {
        return Err(bad("provider must be 1 to 200 characters"));
    }
    if d.model.chars().count() > MAX_SHORT
        || d.source_lang
            .as_deref()
            .is_some_and(|s| s.chars().count() > MAX_SHORT)
    {
        return Err(bad(
            "model and source language are limited to 200 characters",
        ));
    }
    for (i, s) in d.segments.iter().enumerate() {
        if s.page >= MAX_PAGE_INDEX {
            return Err(bad(format!("segment {i}: page index out of range")));
        }
        if let Some(n) = pages
            && s.page >= n
        {
            return Err(bad(format!(
                "segment {i}: page index {} is beyond the {n} pages of the material",
                s.page
            )));
        }
        let [x0, y0, x1, y1] = s.bbox;
        if !(coord_ok(x0) && coord_ok(y0) && coord_ok(x1) && coord_ok(y1) && x0 < x1 && y0 < y1) {
            return Err(bad(format!(
                "segment {i}: bbox must be finite with x0<x1 and y0<y1"
            )));
        }
        if let Some(z) = s.size
            && !(z.is_finite() && z > 0.0 && z <= 1000.0)
        {
            return Err(bad(format!("segment {i}: size must be in (0, 1000]")));
        }
        if s.source.trim().is_empty() || s.source.chars().count() > MAX_TEXT_CHARS {
            return Err(bad(format!(
                "segment {i}: source must be 1 to {MAX_TEXT_CHARS} characters"
            )));
        }
        if s.kind.chars().count() > 40 {
            return Err(bad(format!("segment {i}: kind is too long")));
        }
    }
    Ok(())
}

pub(crate) fn validate_update(u: &SegmentUpdate) -> Result<()> {
    if u.by_user && u.status != STATUS_DONE {
        return Err(bad(format!(
            "segment {}: only a done segment can be marked as chosen by the user",
            u.id
        )));
    }
    if let Some(s) = &u.service {
        if u.status == STATUS_PENDING {
            return Err(bad(format!(
                "segment {}: a pending segment has no service",
                u.id
            )));
        }
        let short = |x: &str| !x.trim().is_empty() && x.chars().count() <= MAX_SHORT;
        if !short(&s.id) || !short(&s.provider) || s.model.chars().count() > MAX_SHORT {
            return Err(bad(format!(
                "segment {}: service id and provider must be 1 to {MAX_SHORT} characters, model at most {MAX_SHORT}",
                u.id
            )));
        }
    }
    if !u.issues.is_empty() && u.status != STATUS_DONE {
        return Err(bad(format!(
            "segment {}: only a done segment has issues",
            u.id
        )));
    }
    if u.issues.len() > MAX_ISSUES
        || u.issues.iter().any(|c| {
            c.is_empty()
                || c.chars().count() > MAX_ISSUE_CHARS
                || !c.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_')
        })
    {
        return Err(bad(format!(
            "segment {}: issues must be at most {MAX_ISSUES} codes of [a-z_], 1 to {MAX_ISSUE_CHARS} characters",
            u.id
        )));
    }
    match u.status.as_str() {
        STATUS_DONE => match &u.target {
            Some(t) if !t.trim().is_empty() && t.chars().count() <= MAX_TEXT_CHARS * 3 => Ok(()),
            _ => Err(bad(format!(
                "segment {}: a done segment needs a target of 1 to {} characters",
                u.id,
                MAX_TEXT_CHARS * 3
            ))),
        },
        STATUS_FAILED | STATUS_PENDING => {
            if u.target.is_some() {
                return Err(bad(format!(
                    "segment {}: only a done segment has a target",
                    u.id
                )));
            }
            if u.error
                .as_deref()
                .is_some_and(|e| e.chars().count() > MAX_SHORT)
            {
                return Err(bad(format!("segment {}: error is too long", u.id)));
            }
            Ok(())
        }
        other => Err(bad(format!("segment {}: unknown status {other:?}", u.id))),
    }
}

/// 依順序給的代號：`g0001`、`g0002` …（超過一萬段時位數自然增加）。
pub(crate) fn segment_id(index: usize) -> String {
    format!("g{:04}", index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(source: &str, target: &str, state: &str, batches: u32) -> OverlayTerm {
        OverlayTerm {
            source: source.into(),
            target: target.into(),
            state: state.into(),
            batches,
            extra: Map::new(),
        }
    }

    #[test]
    fn terms_merge_by_source_and_the_users_decisions_win() {
        let local = vec![
            term("token", "詞元", TERM_LEARNED, 2),
            term("policy", "策略", TERM_USER, 0),
            term("state", "", TERM_REMOVED, 0),
            term("gain", "增益", TERM_CANDIDATE, 1),
        ];
        let remote = vec![
            term("Token", "符記", TERM_USER, 0),
            term("policy", "政策", TERM_USER, 0),
            term("state", "狀態", TERM_LEARNED, 3),
            term("stretch", "伸展參數", TERM_LEARNED, 2),
            term("gain", "增益", TERM_CANDIDATE, 1),
        ];
        let merged = merge_terms(&local, &remote);
        assert_eq!(
            merged,
            vec![
                term("Token", "符記", TERM_USER, 0),
                term("policy", "策略", TERM_USER, 0),
                term("state", "", TERM_REMOVED, 0),
                term("gain", "增益", TERM_CANDIDATE, 1),
                term("stretch", "伸展參數", TERM_LEARNED, 2),
            ]
        );
        assert_eq!(merge_terms(&local, &local), local);
        assert!(validate_terms(&merged).is_ok());
    }
}
