//! 一整份翻譯工作：把待翻的段落分批、送出、把結果交回，直到翻完、取消、或遇到停下來的錯誤。
//!
//! 這裡不碰檔案：每一批的結果交給呼叫端的 `on_batch`（外殼用它寫進譯本檔），所以中途停下來
//! 或程式被關掉，已經交出的結果都已存好，下次只翻還沒翻的（接著翻）。

use crate::check::{Issue, better_of, check_segment, has_critical};
use crate::error::ErrorKind;
use crate::prompt::has_word;
use crate::text::{
    drop_echoed_source, fullwidth_glosses, fullwidth_punct, is_chinese, restore_citations,
    strip_controls,
};
use crate::{Cancel, Engine, Params, Segment};
use std::collections::{HashMap, VecDeque};

/// 一批裡每一段的結果。
#[derive(Debug, Clone)]
pub struct BatchItem {
    pub id: String,
    /// `Ok` 是譯文；`Err` 是這一段翻不出來的原因（其他段不受影響）。
    pub result: std::result::Result<String, ErrorKind>,
    /// 自動檢查留下的問題（`check.rs`）。有嚴重問題的段落已經單獨重翻過一次，這裡是重翻後仍在的問題。
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone)]
pub struct BatchEvent {
    pub items: Vec<BatchItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobSummary {
    pub done: usize,
    pub failed: usize,
    /// 使用者取消。
    pub cancelled: bool,
    /// 因為這個錯誤停下來（沒翻的段落維持待翻，可以接著翻）。
    pub stopped: Option<ErrorKind>,
    /// `on_batch` 回報寫入失敗而停下來。
    pub sink_failed: bool,
    /// 自動檢查發現嚴重問題、單獨重翻過的段數。
    pub retried: usize,
    /// 翻好了但仍留有檢查問題的段數（使用者可以在逐段對照裡看到並單段重翻）。
    pub flagged: usize,
    /// 第二輪潤飾採用了潤飾後譯文的段數。
    pub refined: usize,
}

/// 照順序翻 `pending`。`on_batch` 回 `false` 表示存不下去（例如寫檔失敗），工作停止。
///
/// 送出前先濾掉原文與前文裡的控制字元（三種引擎、分批、空白譯文的判斷、自動檢查看到的都是同一份文字）；
/// 譯文在自動檢查與交出之前先整理（[`polish`]）。
///
/// 接續（跨欄、跨頁被切開的同一句，`Segment::continues`）：後一段排到前一段正後面、同一批送出；
/// 讀不懂而拆批時也不拆開，單段重翻時另一半當前後文帶上。
pub async fn run_job<F>(
    engine: &Engine,
    params: &Params,
    pending: Vec<Segment>,
    cancel: &Cancel,
    mut on_batch: F,
) -> JobSummary
where
    F: FnMut(BatchEvent) -> bool,
{
    let pending: Vec<Segment> = pending.into_iter().map(clean).collect();
    let zh = is_chinese(&params.target_lang);
    let (max_segs, max_chars) = engine.batch_limits();
    // 重翻只對對話模型：機器翻譯服務對同一段會翻出一樣的結果，重翻只是多付一次錢。
    // 每份工作最多重翻 max(5, 段數的一成) 段，避免檢查誤判時費用失控
    let retry_budget = if engine.provider().is_chat_model() {
        (pending.len() / 10).max(5)
    } else {
        0
    };
    let mut queue: VecDeque<Vec<Segment>> = group(pending, max_segs, max_chars).into();
    let mut sum = JobSummary::default();

    while let Some(batch) = queue.pop_front() {
        if cancel.is_cancelled() {
            sum.cancelled = true;
            break;
        }
        // 前一批沒有定下的術語提議（例如那一批讀不懂而拆開重送）不帶到這一批
        engine.discard_proposed_terms();
        match engine.translate_batch(params, &batch, cancel).await {
            Ok(drafts) => {
                let drafts: Vec<String> = drafts
                    .into_iter()
                    .zip(&batch)
                    .map(|(d, s)| polish(zh, s, d))
                    .collect();
                // 第二輪潤飾：整批再送一次；失敗、段數不符或檢查變差，就留草稿
                let mut refined: Vec<Option<String>> = vec![None; drafts.len()];
                if params.refine
                    && engine.can_refine()
                    && !cancel.is_cancelled()
                    && let Ok(v) = engine.refine_batch(params, &batch, &drafts, cancel).await
                    && v.len() == drafts.len()
                {
                    refined = v
                        .into_iter()
                        .zip(&batch)
                        .map(|(r, s)| Some(polish(zh, s, r)))
                        .collect();
                }
                let texts: Vec<String> = drafts
                    .into_iter()
                    .zip(refined)
                    .zip(&batch)
                    .map(|((d, r), s)| match r {
                        Some(r) if !r.trim().is_empty() => {
                            let a = check_segment(&s.text, &d, &params.target_lang);
                            let b = check_segment(&s.text, &r, &params.target_lang);
                            // 潤飾後的問題比草稿多才不採用
                            if !better_of(&b, &a) {
                                sum.refined += 1;
                                r
                            } else {
                                d
                            }
                        }
                        _ => d,
                    })
                    .collect();
                // 服務對一段回了空白譯文（機器翻譯服務可能這樣）：只有那一段記為失敗，
                // 其餘照存；接著翻時失敗的段落會再試一次
                let mut items: Vec<BatchItem> = Vec::with_capacity(batch.len());
                for (s, t) in batch.iter().zip(texts) {
                    if t.trim().is_empty() && !s.text.trim().is_empty() {
                        items.push(BatchItem {
                            id: s.id.clone(),
                            result: Err(ErrorKind::BadResponse),
                            issues: Vec::new(),
                        });
                        continue;
                    }
                    let mut text = t;
                    let mut issues = check_segment(&s.text, &text, &params.target_lang);
                    // 有嚴重問題（漏翻、引用編號或公式不見、被截斷、亂碼）：這一段單獨再翻一次，
                    // 取問題較少的那一份。只重翻一次，避免費用失控
                    if has_critical(&issues) && batch.len() > 1 && sum.retried < retry_budget {
                        sum.retried += 1;
                        let one = alone(s, &batch);
                        if let Ok(mut again) = engine
                            .translate_batch(params, std::slice::from_ref(&one), cancel)
                            .await
                            && let Some(t2) = again.pop().map(|t| polish(zh, s, t))
                            && !t2.trim().is_empty()
                        {
                            let issues2 = check_segment(&s.text, &t2, &params.target_lang);
                            if better_of(&issues, &issues2) {
                                text = t2;
                                issues = issues2;
                            }
                        }
                    }
                    if !issues.is_empty() {
                        sum.flagged += 1;
                    }
                    items.push(BatchItem {
                        id: s.id.clone(),
                        result: Ok(text),
                        issues,
                    });
                }
                // 這一批留下的譯文定了：回覆附的術語照它學（不從被潤飾或重翻換掉的草稿學）
                let sources: Vec<&str> = batch.iter().map(|s| s.text.as_str()).collect();
                let kept: Vec<&str> = items
                    .iter()
                    .filter_map(|i| i.result.as_deref().ok())
                    .collect();
                engine.learn_from_batch(&sources, &kept, &params.glossary);
                // 重翻途中取消：已拿到的這一批照存，下一輪迴圈開頭會結束工作
                sum.failed += items.iter().filter(|i| i.result.is_err()).count();
                sum.done += items.iter().filter(|i| i.result.is_ok()).count();
                if !on_batch(BatchEvent { items }) {
                    sum.sink_failed = true;
                    break;
                }
            }
            Err(e) if e.kind == ErrorKind::Cancelled => {
                sum.cancelled = true;
                break;
            }
            // 回覆讀不懂：縮小批次再試；縮到一段還是不行，這一段記為失敗，繼續下一批
            Err(e) if e.kind == ErrorKind::BadResponse => {
                if batch.len() > 1 {
                    let mid = split_point(&batch);
                    let (a, b) = batch.split_at(mid);
                    queue.push_front(b.to_vec());
                    queue.push_front(a.to_vec());
                } else {
                    sum.failed += 1;
                    let items = vec![BatchItem {
                        id: batch[0].id.clone(),
                        result: Err(ErrorKind::BadResponse),
                        issues: Vec::new(),
                    }];
                    if !on_batch(BatchEvent { items }) {
                        sum.sink_failed = true;
                        break;
                    }
                }
            }
            // 金鑰不對、設定不全、重試用完仍連不上：整個工作停下來，沒翻的維持待翻
            Err(e) => {
                sum.stopped = Some(e.kind);
                break;
            }
        }
    }
    sum
}

/// 濾掉控制字元（原文、前後文）。
fn clean(s: Segment) -> Segment {
    Segment {
        text: strip_controls(&s.text),
        before: s.before.as_deref().map(strip_controls),
        after: s.after.as_deref().map(strip_controls),
        ..s
    }
}

/// 譯文的整理：譯成中文時把中文旁邊的半形標點換成全形、中文詞後面半形括號附的英文換成全形括號、
/// 作者年份引用的括號換回原文的半形；
/// 篇名的譯文後面用括號附上的整段原文拿掉（`text.rs`）。
fn polish(zh: bool, s: &Segment, t: String) -> String {
    let t = if zh {
        restore_citations(&s.text, &fullwidth_glosses(&s.text, &fullwidth_punct(&t)))
    } else {
        t
    };
    if s.title {
        drop_echoed_source(&s.text, &t)
    } else {
        t
    }
}

/// 單段重翻用的一段：接續的另一半在這一批裡時，當前後文帶上。
fn alone(s: &Segment, batch: &[Segment]) -> Segment {
    let mut one = s.clone();
    if let Some(a) = s
        .continues
        .as_deref()
        .and_then(|m| batch.iter().find(|g| g.id == m))
    {
        one.before = Some(a.text.clone());
    }
    if let Some(b) = batch
        .iter()
        .find(|g| g.continues.as_deref() == Some(s.id.as_str()))
    {
        one.after = Some(b.text.clone());
    }
    one
}

/// 這一段接著前一段（接續）：兩段要在同一批。
fn joined(prev: &Segment, s: &Segment) -> bool {
    s.continues.as_deref() == Some(prev.id.as_str())
}

/// 讀不懂而拆批的位置：離中間最近、不拆開接續的地方；整批是同一串接續時照中間拆。
fn split_point(batch: &[Segment]) -> usize {
    let mid = batch.len() / 2;
    (1..batch.len())
        .filter(|&m| !joined(&batch[m - 1], &batch[m]))
        .min_by_key(|&m| m.abs_diff(mid))
        .unwrap_or(mid)
}

/// 接續的後一段排到前一段的正後面（夾在中間的圖說排到它們後面）；其餘照原本的順序。
fn order_continuations(pending: Vec<Segment>) -> Vec<Segment> {
    let follower: HashMap<&str, usize> = pending
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.continues.as_deref().map(|m| (m, i)))
        .collect();
    let mut order = Vec::with_capacity(pending.len());
    let mut used = vec![false; pending.len()];
    for i in 0..pending.len() {
        let mut k = i;
        while !used[k] {
            used[k] = true;
            order.push(k);
            match follower.get(pending[k].id.as_str()) {
                Some(&j) if j > k => k = j,
                _ => break,
            }
        }
    }
    let mut slots: Vec<Option<Segment>> = pending.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

/// 分批：每批不超過引擎的段數與字數；接續的前後段不拆在兩批（一串接續本身超過上限時照樣同一批）。
/// 接續的前一段不在這次要送的段落裡時，後一段自成一批（只有一批的第一段帶前文，前文才會是它的前一段）。
fn group(pending: Vec<Segment>, max_segs: usize, max_chars: usize) -> Vec<Vec<Segment>> {
    let ids: std::collections::HashSet<String> = pending.iter().map(|s| s.id.clone()).collect();
    let alone = |s: &Segment| s.continues.as_deref().is_some_and(|m| !ids.contains(m));
    let mut units: Vec<Vec<Segment>> = Vec::new();
    for s in order_continuations(pending) {
        match units.last_mut() {
            Some(u) if u.last().is_some_and(|l| joined(l, &s)) => u.push(s),
            _ => units.push(vec![s]),
        }
    }
    let mut out: Vec<Vec<Segment>> = Vec::new();
    let mut cur: Vec<Segment> = Vec::new();
    let mut chars = 0usize;
    for u in units {
        let n: usize = u.iter().map(|s| s.text.chars().count()).sum();
        let by_itself = u.first().is_some_and(&alone);
        if !cur.is_empty() && (by_itself || cur.len() + u.len() > max_segs || chars + n > max_chars)
        {
            out.push(std::mem::take(&mut cur));
            chars = 0;
        }
        chars += n;
        cur.extend(u);
        if by_itself {
            out.push(std::mem::take(&mut cur));
            chars = 0;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ---------- 收尾重翻 ----------

/// 一段原文裡出現、譯文卻沒用到的術語數（術語的原文詞依詞的邊界比對，不分大小寫）。
fn term_misses(source: &str, target: &str, terms: &[(String, String)]) -> usize {
    let src = source.to_lowercase();
    let tgt = target.to_lowercase();
    terms
        .iter()
        .filter(|(from, to)| {
            let (f, t) = (from.trim().to_lowercase(), to.trim().to_lowercase());
            !t.is_empty() && has_word(&src, &f) && !tgt.contains(&t)
        })
        .count()
}

/// 收尾重翻要挑的段落：原文有這篇已定的術語（`terms`）、譯文卻沒用那個譯法的；沒翻好的與使用者決定的不挑。
/// `segs` 依原文順序，每段是（原文, 譯文, 使用者決定的）；`pairs` 是接續的（前一段, 後一段）位置。
/// 挑到接續的其中一段，整串一起挑（一串裡有沒翻好或使用者決定的就整串不挑），整串都算進上限。
/// 依順序最多挑 `limit` 段，交回由小到大的位置。
pub fn final_pass_targets(
    segs: &[(&str, Option<&str>, bool)],
    terms: &[(String, String)],
    limit: usize,
    pairs: &[(usize, usize)],
) -> Vec<usize> {
    if terms.is_empty() {
        return Vec::new();
    }
    // 每一段所在的那一串接續
    let mut root: Vec<usize> = (0..segs.len()).collect();
    fn find(root: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while root[r] != r {
            r = root[r];
        }
        root[i] = r;
        r
    }
    for &(a, b) in pairs {
        if a < segs.len() && b < segs.len() {
            let (ra, rb) = (find(&mut root, a), find(&mut root, b));
            root[ra.max(rb)] = ra.min(rb);
        }
    }
    let usable = |(_, tgt, by_user): &(&str, Option<&str>, bool)| {
        !by_user && tgt.is_some_and(|t| !t.trim().is_empty())
    };
    let mut picked = vec![false; segs.len()];
    let mut count = 0;
    for i in 0..segs.len() {
        let (src, tgt, _) = segs[i];
        if picked[i] || !usable(&segs[i]) || term_misses(src, tgt.unwrap_or(""), terms) == 0 {
            continue;
        }
        let r = find(&mut root, i);
        let unit: Vec<usize> = (0..segs.len())
            .filter(|&k| find(&mut root, k) == r)
            .collect();
        if unit.iter().all(|&k| usable(&segs[k])) && count + unit.len() <= limit {
            count += unit.len();
            unit.into_iter().for_each(|k| picked[k] = true);
        }
    }
    (0..segs.len()).filter(|&i| picked[i]).collect()
}

/// 收尾重翻的一段：要送出的段落（含前後文與接續）與現在的譯文。
#[derive(Debug, Clone)]
pub struct FinalPassItem {
    pub segment: Segment,
    pub current: String,
}

/// 收尾重翻的結果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FinalPassSummary {
    /// 採用了新譯文的段數。
    pub adopted: usize,
    /// 使用者取消。
    pub cancelled: bool,
    /// 因為這個錯誤停下來（已採用的照樣寫好了）。
    pub stopped: Option<ErrorKind>,
    /// `on_batch` 回報寫入失敗而停下來。
    pub sink_failed: bool,
}

/// 收尾重翻：整篇翻完後，把 [`final_pass_targets`] 挑出的段落帶著這篇已定的術語再翻一次。
/// 新的譯文少用錯的術語、自動檢查的問題又沒有變多才採用；一串接續一起採用或一起不採用（前後兩段的切法要配得上）。
/// 只交出採用的段落（`on_batch`）。取消、遇到停下來的錯誤、寫入失敗就結束；讀不懂的那一批略過（原本的譯文留著）。
/// 這一輪回覆附的術語不學。
pub async fn run_final_pass<F>(
    engine: &Engine,
    params: &Params,
    items: Vec<FinalPassItem>,
    cancel: &Cancel,
    mut on_batch: F,
) -> FinalPassSummary
where
    F: FnMut(BatchEvent) -> bool,
{
    let zh = is_chinese(&params.target_lang);
    let terms = crate::prompt::terms_in_use(&engine.paper_terms());
    let mut current: HashMap<String, String> = HashMap::new();
    let segs: Vec<Segment> = items
        .into_iter()
        .map(|it| {
            current.insert(it.segment.id.clone(), it.current);
            clean(it.segment)
        })
        .collect();
    let (max_segs, max_chars) = engine.batch_limits();
    let mut sum = FinalPassSummary::default();
    for batch in group(segs, max_segs, max_chars) {
        if cancel.is_cancelled() {
            sum.cancelled = true;
            break;
        }
        engine.discard_proposed_terms();
        let out = engine.translate_batch(params, &batch, cancel).await;
        engine.discard_proposed_terms();
        let texts = match out {
            Ok(t) => t,
            Err(e) if e.kind == ErrorKind::Cancelled => {
                sum.cancelled = true;
                break;
            }
            Err(e) if e.kind == ErrorKind::BadResponse => continue,
            Err(e) => {
                sum.stopped = Some(e.kind);
                break;
            }
        };
        // 每一段的候選：（新譯文, 新的問題, 問題沒有變多, 原本與新的少用術語數）
        let mut cands = Vec::with_capacity(batch.len());
        for (s, t) in batch.iter().zip(texts) {
            let Some(old) = current.get(&s.id) else {
                cands.push(None);
                continue;
            };
            let new = polish(zh, s, t);
            if new.trim().is_empty() {
                cands.push(None);
                continue;
            }
            let old_issues = check_segment(&s.text, old, &params.target_lang);
            let new_issues = check_segment(&s.text, &new, &params.target_lang);
            let ok = !better_of(&new_issues, &old_issues);
            let misses = (
                term_misses(&s.text, old, &terms),
                term_misses(&s.text, &new, &terms),
            );
            cands.push(Some((new, new_issues, ok, misses)));
        }
        // 一串接續（相連、後一段接著前一段）一起決定
        let mut items = Vec::new();
        let mut start = 0;
        while start < batch.len() {
            let mut end = start + 1;
            while end < batch.len() && joined(&batch[end - 1], &batch[end]) {
                end += 1;
            }
            let unit = &cands[start..end];
            let adopt = unit.iter().all(|c| c.as_ref().is_some_and(|c| c.2))
                && unit.iter().flatten().map(|c| c.3.1).sum::<usize>()
                    < unit.iter().flatten().map(|c| c.3.0).sum::<usize>();
            if adopt {
                for (s, c) in batch[start..end].iter().zip(unit) {
                    if let Some((new, issues, _, _)) = c {
                        items.push(BatchItem {
                            id: s.id.clone(),
                            result: Ok(new.clone()),
                            issues: issues.clone(),
                        });
                    }
                }
            }
            start = end;
        }
        sum.adopted += items.len();
        if !items.is_empty() && !on_batch(BatchEvent { items }) {
            sum.sink_failed = true;
            break;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(id: &str, n: usize) -> Segment {
        Segment {
            id: id.to_string(),
            text: "x".repeat(n),
            before: None,
            ..Default::default()
        }
    }

    #[test]
    fn grouping_respects_both_limits() {
        let v = vec![seg("1", 10), seg("2", 10), seg("3", 10), seg("4", 50)];
        let g = group(v, 2, 100);
        let ids: Vec<Vec<&str>> = g
            .iter()
            .map(|b| b.iter().map(|s| s.id.as_str()).collect())
            .collect();
        assert_eq!(ids, vec![vec!["1", "2"], vec!["3", "4"]]);
        let g = group(vec![seg("a", 60), seg("b", 60)], 10, 100);
        assert_eq!(
            g.len(),
            2,
            "a long segment is not merged past the char limit"
        );
        let g = group(vec![seg("a", 500)], 10, 100);
        assert_eq!(
            g.len(),
            1,
            "a single over-long segment still goes out alone"
        );
    }

    fn cont(id: &str, n: usize, of: &str) -> Segment {
        Segment {
            continues: Some(of.to_string()),
            ..seg(id, n)
        }
    }

    fn ids(g: &[Vec<Segment>]) -> Vec<Vec<&str>> {
        g.iter()
            .map(|b| b.iter().map(|s| s.id.as_str()).collect())
            .collect()
    }

    #[test]
    fn a_continuation_follows_its_first_half_into_the_same_batch() {
        // b 接著 a 讀，中間夾著圖說 cap：b 排到 a 後面，圖說排到 b 後面；a 是一批的最後一段時，兩段一起進下一批
        let v = vec![
            seg("x", 10),
            seg("a", 10),
            seg("cap", 10),
            cont("b", 10, "a"),
            seg("y", 10),
        ];
        assert_eq!(
            ids(&group(v.clone(), 2, 1000)),
            vec![vec!["x"], vec!["a", "b"], vec!["cap", "y"]]
        );
        assert_eq!(
            ids(&group(v, 10, 1000)),
            vec![vec!["x", "a", "b", "cap", "y"]]
        );
        // 一串接續本身超過上限：照樣同一批
        let v = vec![seg("a", 10), cont("b", 10, "a"), cont("c", 10, "b")];
        assert_eq!(ids(&group(v, 2, 15)), vec![vec!["a", "b", "c"]]);
        // 前一段不在這次要翻的段落裡（已翻好）：照原本的順序，後一段自成一批
        let v = vec![seg("cap", 10), cont("b", 10, "a"), seg("y", 10)];
        assert_eq!(
            ids(&group(v, 10, 1000)),
            vec![vec!["cap"], vec!["b"], vec!["y"]]
        );
    }

    #[test]
    fn splitting_a_bad_batch_keeps_continuations_together() {
        let b = vec![seg("1", 1), seg("2", 1), cont("3", 1, "2"), seg("4", 1)];
        assert_eq!(split_point(&b), 1, "the middle would split 2 and 3");
        let b = vec![seg("1", 1), cont("2", 1, "1")];
        assert_eq!(
            split_point(&b),
            1,
            "a single pair is split as a last resort"
        );
    }

    #[test]
    fn a_half_retried_alone_carries_the_other_half_as_context() {
        let a = Segment {
            text: "the robot".into(),
            ..seg("a", 0)
        };
        let b = Segment {
            text: "should act consistently.".into(),
            ..cont("b", 0, "a")
        };
        let batch = vec![seg("x", 3), a.clone(), b.clone()];
        let one = alone(&b, &batch);
        assert_eq!(one.before.as_deref(), Some("the robot"));
        assert_eq!(one.continues.as_deref(), Some("a"));
        let one = alone(&a, &batch);
        assert_eq!(one.after.as_deref(), Some("should act consistently."));
        assert_eq!(alone(&batch[0], &batch), batch[0]);
    }

    #[test]
    fn the_final_pass_picks_segments_that_miss_a_settled_term() {
        let terms = vec![
            ("token".to_string(), "token".to_string()),
            ("fallback".to_string(), "後備".to_string()),
        ];
        let segs = [
            ("Each token is kept.", Some("每個 Token 都保留。"), false),
            ("Each token is kept.", Some("每個符元都保留。"), false),
            ("The fallback path.", Some("回退路徑。"), false),
            ("The fallback path.", Some("回退路徑。"), true),
            ("The fallback path.", None, false),
            ("A statement about tokens.", Some("關於符元的陳述。"), false),
            ("The fallback path.", Some("後備路徑。"), false),
        ];
        assert_eq!(final_pass_targets(&segs, &terms, 10, &[]), vec![1, 2]);
        assert_eq!(final_pass_targets(&segs, &terms, 1, &[]), vec![1]);
        assert!(final_pass_targets(&segs, &[], 10, &[]).is_empty());
    }

    #[test]
    fn polishing_restores_citations_and_drops_an_echoed_source() {
        let src = "Spectral Smoothing for Policy Learning (Wang et al., 2025)";
        assert_eq!(
            polish(true, &Segment { text: src.into(), title: true, ..seg("t", 0) }, "策略學習的頻譜平滑,見（Wang et al.，2025）（Spectral Smoothing for Policy Learning (Wang et al., 2025)）".into()),
            "策略學習的頻譜平滑，見(Wang et al., 2025)"
        );
    }

    #[test]
    fn a_continuation_without_its_first_half_goes_out_alone_and_first() {
        // 前一段不在這次要送的段落裡：後一段自成一批，前文才會是前一段；同批的其他段落照常
        let v = vec![seg("x", 10), cont("b", 10, "a"), seg("y", 10), seg("z", 10)];
        assert_eq!(
            ids(&group(v, 10, 1000)),
            vec![vec!["x"], vec!["b"], vec!["y", "z"]]
        );
    }

    #[test]
    fn the_final_pass_takes_both_halves_of_a_continuation() {
        let terms = vec![("token".to_string(), "token".to_string())];
        let segs = [
            ("Each token of the", Some("每個符元的"), false),
            ("sentence is kept.", Some("句子都保留。"), false),
            ("A token.", Some("一個符元。"), false),
            ("Another token.", Some("另一個符元。"), false),
        ];
        // 挑到前一段，後一段一起挑，兩段都算進上限
        assert_eq!(
            final_pass_targets(&segs, &terms, 10, &[(0, 1)]),
            vec![0, 1, 2, 3]
        );
        assert_eq!(
            final_pass_targets(&segs, &terms, 3, &[(0, 1)]),
            vec![0, 1, 2]
        );
        assert_eq!(final_pass_targets(&segs, &terms, 1, &[(0, 1)]), vec![2]);
        // 另一半是使用者決定的：整對不挑
        let mut user = segs;
        user[1].2 = true;
        assert_eq!(final_pass_targets(&user, &terms, 10, &[(0, 1)]), vec![2, 3]);
    }

    #[test]
    fn only_the_paper_title_drops_an_echoed_source() {
        let src = "Proximal Policy Optimization";
        let t = "近端策略最佳化（Proximal Policy Optimization）";
        let title = Segment {
            text: src.into(),
            title: true,
            ..seg("t", 0)
        };
        let heading = Segment {
            text: src.into(),
            ..seg("h", 0)
        };
        assert_eq!(polish(true, &title, t.into()), "近端策略最佳化");
        assert_eq!(polish(true, &heading, t.into()), t);
    }

    #[test]
    fn polishing_puts_english_glosses_in_full_width_parentheses() {
        let s = Segment {
            text: "In behavior cloning the policy (Smith et al., 2020) is copied.".into(),
            ..seg("a", 0)
        };
        assert_eq!(
            polish(
                true,
                &s,
                "在行為複製 (behavior cloning) 中，策略被複製 (Smith et al., 2020)。".into()
            ),
            "在行為複製（behavior cloning）中，策略被複製 (Smith et al., 2020)。"
        );
        let t = "Im Behavior Cloning (behavior cloning) wird kopiert.";
        assert_eq!(polish(false, &s, t.into()), t);
    }
}
