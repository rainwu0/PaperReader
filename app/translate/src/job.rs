//! 一整份翻譯工作：把待翻的段落分批、送出、把結果交回，直到翻完、取消、或遇到停下來的錯誤。
//!
//! 這裡不碰檔案：每一批的結果交給呼叫端的 `on_batch`（外殼用它寫進譯本檔），所以中途停下來
//! 或程式被關掉，已經交出的結果都已存好，下次只翻還沒翻的（接著翻）。

use crate::check::{Issue, better_of, check_segment, has_critical};
use crate::error::ErrorKind;
use crate::text::{fullwidth_punct, is_chinese, strip_controls};
use crate::{Cancel, Engine, Params, Segment};
use std::collections::VecDeque;

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
/// 譯成中文時，譯文在自動檢查與交出之前先把中文旁邊的半形標點換成全形（`text.rs`）。
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
    let pending: Vec<Segment> = pending
        .into_iter()
        .map(|s| Segment {
            text: strip_controls(&s.text),
            before: s.before.as_deref().map(strip_controls),
            id: s.id,
        })
        .collect();
    let zh = is_chinese(&params.target_lang);
    let polish = |t: String| if zh { fullwidth_punct(&t) } else { t };
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
                let drafts: Vec<String> = drafts.into_iter().map(polish).collect();
                // 第二輪潤飾：整批再送一次；失敗、段數不符或檢查變差，就留草稿
                let mut refined: Vec<Option<String>> = vec![None; drafts.len()];
                if params.refine
                    && engine.can_refine()
                    && !cancel.is_cancelled()
                    && let Ok(v) = engine.refine_batch(params, &batch, &drafts, cancel).await
                    && v.len() == drafts.len()
                {
                    refined = v.into_iter().map(|r| Some(polish(r))).collect();
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
                        if let Ok(mut again) = engine
                            .translate_batch(params, std::slice::from_ref(s), cancel)
                            .await
                            && let Some(t2) = again.pop().map(polish)
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
                    let mid = batch.len() / 2;
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

fn group(pending: Vec<Segment>, max_segs: usize, max_chars: usize) -> Vec<Vec<Segment>> {
    let mut out: Vec<Vec<Segment>> = Vec::new();
    let mut cur: Vec<Segment> = Vec::new();
    let mut chars = 0usize;
    for s in pending {
        let n = s.text.chars().count();
        if !cur.is_empty() && (cur.len() >= max_segs || chars + n > max_chars) {
            out.push(std::mem::take(&mut cur));
            chars = 0;
        }
        chars += n;
        cur.push(s);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(id: &str, n: usize) -> Segment {
        Segment {
            id: id.to_string(),
            text: "x".repeat(n),
            before: None,
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
}
