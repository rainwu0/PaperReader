//! 重點筆記：開始（背景工作）、進度、取消、換備用服務接著做；
//! 讀、刪一點、標為已校對；匯出與用其他應用打開。
//!
//! 資料規則在核心（`paperreader_core::digest`），模型的部分在 `paperreader_translate::digest`；這裡只轉接。
//! 服務照「翻譯服務」的清單（辰），只能用對話模型類的。工作在背景進行，做完由這裡交給核心寫進文獻庫，
//! 所以離開畫面不會丟掉已經付費做好的結果。

use crate::commands::export::{ExportFile, copy_out, exports_dir, file_stem, paper_title};
use crate::commands::translate::{
    UsageMeter, engine_for, fallback_offer, find_service, latest_overlay_terms, user_glossary,
};
use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::state::AppState;
use crate::translate::{
    FallbackOffer, HasState, Job, JobSlot, JobState, Service, TranslateSettings, api_kind,
    offers_fallback, section_labels,
};
use paperreader_core::digest::{DigestDoc, DigestDraft, DigestSection, PointDraft};
use paperreader_core::jsonio::VersionToken;
use paperreader_translate::{
    Cancel, DigestInput, DigestResult, DigestSegment, DigestStage, DigestState, run_digest,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_paperreader::PaperReaderExt;

/// 服務沒有取名字時，寫進重點筆記屬性區的名稱：和服務清單上顯示的名稱相同
/// （畫面的 `settings.generate.provider.*`），重點筆記的頁頂照它顯示。
fn provider_label(p: paperreader_translate::Provider) -> &'static str {
    use paperreader_translate::Provider as P;
    match p {
        P::Anthropic => "Claude（Anthropic）",
        P::Openai => "OpenAI",
        P::Gemini => "Gemini（Google）",
        P::OpenaiCompatible => "自訂端點（OpenAI 相容，含 Ollama）",
        _ => p.key(),
    }
}

/// 失敗之後依序可以改用的下一個**對話模型**服務（機器翻譯服務與離線引擎做不了重點筆記）。
pub fn next_chat_fallback<'a>(t: &'a TranslateSettings, failed: &str) -> Option<&'a Service> {
    let mut cur = failed.to_string();
    for _ in 0..=t.services.len() {
        let next = t.next_fallback(&cur)?;
        if next.provider.is_chat_model() {
            return Some(next);
        }
        cur = next.id.clone();
    }
    None
}

// ---------- 工作 ----------

/// 重點筆記工作的狀態：畫面每隔一下來問一次。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DigestStatus {
    pub paper_id: String,
    pub service: String,
    pub state: JobState,
    /// 正在做第幾部分（從 1 起算）、共幾部分；`merging` 是正在合併。
    pub part: usize,
    pub parts: usize,
    pub merging: bool,
    pub reason: Option<ErrorKind>,
    pub fallback: Option<FallbackOffer>,
    /// 完成時寫好的材料代號。
    pub material_id: Option<String>,
    pub points: usize,
    /// 段落代號對不上而拿掉的點數。
    pub dropped: usize,
    /// 這次服務回報的用量（詞元數；服務沒有回報是 0）。
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl HasState for DigestStatus {
    fn state(&self) -> JobState {
        self.state
    }
}

/// 重點筆記工作接著做（換服務、再寫一次）要留的東西。
#[derive(Clone)]
pub struct DigestCarry {
    source_material: String,
    /// 要送的內容：開始時就定好預設組、區段與術語（停下來之後改了設定，接著做的部分仍照同一份）。
    input: Arc<DigestInput>,
    /// 區段在檔案裡的名稱（同上，開始時定好）。
    sections: Vec<DigestSection>,
    /// 做好的部分（換服務接著做時不重送）。
    progress: DigestState,
    /// 模型做完、但寫進文獻庫失敗的結果（接著做時直接再寫一次，不再送出）。
    result: Option<(DigestResult, String, String, String)>,
    /// 寫好新的一份之後要移到回收區的舊重點筆記（重跑預設更新同一份）。
    replaces: Option<String>,
}

/// 同一時間最多一份重點筆記工作（和翻譯工作分開）。
pub type DigestSlot = JobSlot<DigestStatus, DigestCarry>;

/// 畫面抽好的一段（`layout.ts` 的段落，依閱讀順序）。
#[derive(Deserialize)]
pub struct DigestSegmentInput {
    pub page: u32,
    pub source: String,
}

fn bad(why: &str) -> ApiError {
    ApiError::new(ErrorKind::InvalidInput, why.to_string())
}

/// 開始做一份重點筆記：用 `service` 這個服務（要是對話模型）。背景進行，立刻交回；進度用 `digest_status` 問。
/// `replaces` 是這篇目前的重點筆記：新的一份寫好之後，舊的移到回收區（重跑預設更新同一份）；
/// 沒寫成就不動舊的。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn digest_run(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: State<'_, DigestSlot>,
    paper_id: String,
    source_material: String,
    segments: Vec<DigestSegmentInput>,
    abstract_text: Option<String>,
    service: String,
    replaces: Option<String>,
) -> ApiResult<()> {
    if segments.is_empty() || segments.len() > 20_000 {
        return Err(bad("segments"));
    }
    let (pid, mid) = (paper_id.clone(), source_material.clone());
    let (pages, title) = state
        .lib("digest_run", move |l| {
            let (p, _) = l.get_paper(&pid)?;
            let pages = p.materials.iter().find(|m| m.id == mid).map(|m| m.pages);
            Ok((pages, p.bib.title.filter(|t| !t.trim().is_empty())))
        })
        .await?;
    let pages = match pages {
        None => return Err(ApiError::new(ErrorKind::NotFound, "source material")),
        Some(None) => return Err(bad("page count unknown")),
        Some(Some(n)) => n,
    };
    if segments
        .iter()
        .any(|g| g.page >= pages || g.source.trim().is_empty())
    {
        return Err(bad("segment page"));
    }
    let t = state.translate_settings();
    // 選用的預設組、術語庫與這篇譯本已定的譯法，開始時就定好
    let preset = t.digest_preset();
    // 這篇最新那份同語言譯本的術語（跟著譯文疊加檔）
    let paper_terms = latest_overlay_terms(&state, &paper_id, &t.target_lang).await;
    let input = DigestInput {
        title,
        abstract_text: abstract_text.filter(|a| !a.trim().is_empty()),
        segments: segments
            .into_iter()
            .enumerate()
            .map(|(i, g)| DigestSegment {
                id: format!("g{:04}", i + 1),
                page: g.page,
                text: g.source,
            })
            .collect(),
        target_lang: t.target_lang.clone(),
        instructions: preset.text.clone(),
        sections: preset.section_specs(),
        glossary: user_glossary(&state),
        paper_terms,
    };
    let carry = DigestCarry {
        source_material,
        sections: section_labels(&preset, &t.target_lang),
        input: Arc::new(input),
        progress: DigestState::default(),
        result: None,
        replaces,
    };
    start(app, &state, &slot, paper_id, carry, service).await
}

/// 停下來之後用另一個服務（或同一個）接著做：沿用做好的部分。
#[tauri::command]
pub async fn digest_continue(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: State<'_, DigestSlot>,
    service: String,
) -> ApiResult<()> {
    let (paper_id, carry) = slot
        .peek(|j| match j.status.state {
            JobState::Running => Err(ApiError::new(ErrorKind::TranslateBusy, "a job is running")),
            JobState::Finished => Err(bad("already finished")),
            _ => Ok((j.status.paper_id.clone(), j.extra.clone())),
        })
        .ok_or_else(|| ApiError::new(ErrorKind::NotFound, "no digest job"))??;
    start(app, &state, &slot, paper_id, carry, service).await
}

/// 用 `service` 開始（或接著）做：`carry` 是原文、要送的內容、做好的部分與要取代的舊的一份。
async fn start(
    app: AppHandle,
    state: &AppState,
    slot: &DigestSlot,
    paper_id: String,
    carry: DigestCarry,
    service: String,
) -> ApiResult<()> {
    let t = state.translate_settings();
    let svc = find_service(&t, &service)?;
    if !svc.provider.is_chat_model() {
        return Err(ApiError::new(
            ErrorKind::TranslateConfig,
            "not a chat model",
        ));
    }
    let engine = engine_for(&app, &svc, &carry.input.target_lang).await?;
    let (model, _) = svc.model_endpoint();
    let label = if svc.name.trim().is_empty() {
        provider_label(svc.provider).to_string()
    } else {
        svc.name.trim().to_string()
    };
    let cancel = Cancel::new();
    let status = DigestStatus {
        paper_id: paper_id.clone(),
        service: service.clone(),
        state: JobState::Running,
        part: 0,
        parts: carry.progress.parts.len(),
        merging: false,
        reason: None,
        fallback: None,
        material_id: None,
        points: 0,
        dropped: 0,
        input_tokens: 0,
        output_tokens: 0,
    };
    slot.begin(Job {
        cancel: cancel.clone(),
        status,
        extra: carry.clone(),
    })?;

    let (state, slot) = (state.clone(), slot.clone());
    let DigestCarry {
        source_material,
        input,
        sections,
        mut progress,
        result,
        replaces,
    } = carry;
    let mut meter = UsageMeter::new(&paper_id, "digest", &service);
    tauri::async_runtime::spawn(async move {
        let publish = |f: &dyn Fn(&mut DigestStatus)| slot.publish(|j| f(&mut j.status));
        // 模型那一段：已經有沒寫成的結果就不再送出。每開始一步就把到現在的用量記下
        let outcome = match result {
            Some(r) => Ok(r),
            None => run_digest(&engine, &input, &mut progress, &cancel, |stage| {
                let used = meter.flush(&state, &engine);
                match stage {
                    DigestStage::Part { n, total } => publish(&|s| {
                        s.part = n;
                        s.parts = total;
                        s.merging = false;
                        s.input_tokens = used.input;
                        s.output_tokens = used.output;
                    }),
                    DigestStage::Merge => publish(&|s| {
                        s.merging = true;
                        s.input_tokens = used.input;
                        s.output_tokens = used.output;
                    }),
                }
            })
            .await
            .map(|r| (r, label, svc.provider.key().to_string(), model)),
        };
        let used = meter.flush(&state, &engine);
        publish(&|s| {
            s.input_tokens = used.input;
            s.output_tokens = used.output;
        });
        let mut keep_result = None;
        let mut fallback_for = None;
        let final_state: Box<dyn Fn(&mut DigestStatus) + Send> = match outcome {
            Ok((r, label, provider, model)) => {
                let written = write_digest(
                    &state,
                    &paper_id,
                    (&source_material, &sections),
                    &input,
                    &r,
                    (&label, &provider, &model),
                    replaces.as_deref(),
                )
                .await;
                match written {
                    Ok(material_id) => {
                        let (points, dropped) = (r.items.len(), r.dropped);
                        Box::new(move |s: &mut DigestStatus| {
                            s.state = JobState::Finished;
                            s.material_id = Some(material_id.clone());
                            s.points = points;
                            s.dropped = dropped;
                            s.merging = false;
                        })
                    }
                    Err(e) => {
                        keep_result = Some((r, label, provider, model));
                        let kind = e.kind;
                        Box::new(move |s: &mut DigestStatus| {
                            s.state = JobState::Stopped;
                            s.reason = Some(kind);
                        })
                    }
                }
            }
            Err(e) if e.kind == paperreader_translate::ErrorKind::Cancelled => {
                Box::new(|s: &mut DigestStatus| s.state = JobState::Cancelled)
            }
            Err(e) => {
                if offers_fallback(e.kind) {
                    fallback_for = Some(service.clone());
                }
                let kind = api_kind(e.kind);
                Box::new(move |s: &mut DigestStatus| {
                    s.state = JobState::Stopped;
                    s.reason = Some(kind);
                })
            }
        };
        // 服務停擺（不是金鑰或設定的問題）：找依序的下一個對話模型服務，讓畫面問使用者要不要換
        let next = fallback_for
            .and_then(|failed| next_chat_fallback(&state.translate_settings(), &failed).cloned());
        let offer: Option<FallbackOffer> = fallback_offer(&app, next).await;
        slot.publish(|j| {
            final_state(&mut j.status);
            j.status.fallback = offer;
            j.extra.progress = progress;
            j.extra.result = keep_result;
        });
    });
    Ok(())
}

/// 把模型做好的結果交給核心：核心檢查過結構、大小、頁碼範圍才寫（架構 5.3 規則 4）。
/// 寫好之後，`replaces`（這篇原本的重點筆記）還在就移到回收區；移不成只記下，新的一份照樣算寫好。
async fn write_digest(
    state: &AppState,
    paper_id: &str,
    (source_material, sections): (&str, &[DigestSection]),
    input: &DigestInput,
    r: &DigestResult,
    (label, provider, model): (&str, &str, &str),
    replaces: Option<&str>,
) -> ApiResult<String> {
    let pid = paper_id.to_string();
    let (source, input, r) = (source_material.to_string(), input.clone(), r.clone());
    let sections = sections.to_vec();
    let (label, provider, model) = (label.to_string(), provider.to_string(), model.to_string());
    let replaces = replaces.map(String::from);
    state
        .lib("digest_write", move |l| {
            write_digest_into(
                l,
                &pid,
                (&source, &sections),
                &input,
                &r,
                (&label, &provider, &model),
                replaces.as_deref(),
            )
        })
        .await
}

/// 寫一份新的重點筆記；`replaces` 是要取代的那一份（重點筆記才算）：新的寫好之後把它移到回收區（可以還原）。
/// 移不成時只記下，新的一份照樣算寫好。
fn write_digest_into(
    l: &mut paperreader_core::local::LocalLibrary,
    pid: &str,
    (source, sections): (&str, &[DigestSection]),
    input: &DigestInput,
    r: &DigestResult,
    (label, provider, model): (&str, &str, &str),
    replaces: Option<&str>,
) -> Result<String, paperreader_core::error::LibError> {
    let (paper, token) = l.get_paper(pid)?;
    let pages = paper
        .materials
        .iter()
        .find(|m| m.id == source)
        .and_then(|m| m.pages)
        .unwrap_or(0);
    let draft = build_draft(
        (source, sections),
        input,
        r,
        (label, provider, model),
        pages,
    );
    let created = l.create_digest(pid, draft, &token)?.material_id;
    if let Some(old) = replaces {
        let (paper, token) = l.get_paper(pid)?;
        let is_digest = paper
            .materials
            .iter()
            .any(|m| m.id == old && m.role == paperreader_core::digest::ROLE_DIGEST);
        if is_digest && let Err(e) = l.remove_material(pid, old, &token) {
            eprintln!("paperreader: digest_replace failed: {}", e.kind_name());
        }
    }
    Ok(created)
}

/// 給核心的內容：點照核心同一套規則先濾過，不合的那幾點丟掉，不讓整份失敗；
/// 服務與模型的名稱去掉控制字元、截長度。
fn build_draft(
    (source_material, sections): (&str, &[DigestSection]),
    input: &DigestInput,
    r: &DigestResult,
    (label, provider, model): (&str, &str, &str),
    source_pages: u32,
) -> DigestDraft {
    let plain = |s: &str, max: usize| -> String {
        s.chars()
            .filter(|c| !c.is_control())
            .take(max)
            .collect::<String>()
            .trim()
            .to_string()
    };
    DigestDraft {
        source_material: source_material.to_string(),
        service: plain(label, 100),
        provider: provider.to_string(),
        model: plain(model, 200),
        lang: input.target_lang.clone(),
        title: input.title.clone().unwrap_or_default(),
        sections: sections.to_vec(),
        points: r
            .items
            .iter()
            .map(|it| PointDraft {
                section: it.section.clone(),
                text: it.text.clone(),
                pages: it.pages.clone(),
                segments: it.segments.clone(),
            })
            .filter(|p| paperreader_core::digest::validate_point(p, sections, source_pages).is_ok())
            .take(paperreader_core::digest::MAX_POINTS)
            .collect(),
    }
}

/// 目前（或最近一次）重點筆記工作的狀態；從沒有過就是 `null`。
#[tauri::command]
pub async fn digest_status(slot: State<'_, DigestSlot>) -> ApiResult<Option<DigestStatus>> {
    Ok(slot.status())
}

#[tauri::command]
pub async fn digest_cancel(slot: State<'_, DigestSlot>) -> ApiResult<()> {
    slot.cancel();
    Ok(())
}

// ---------- 讀、刪一點、標為已校對 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestView {
    pub doc: DigestDoc,
    /// 版本記號：刪一點、標已校對時帶回來。
    pub token: String,
}

#[tauri::command]
pub async fn digest_read(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
) -> ApiResult<DigestView> {
    state
        .lib("digest_read", move |l| {
            l.read_digest(&paper_id, &material_id)
                .map(|(doc, token)| DigestView {
                    doc,
                    token: token.0,
                })
        })
        .await
}

#[tauri::command]
pub async fn digest_delete_point(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
    point_id: String,
    token: String,
) -> ApiResult<String> {
    state
        .lib("digest_delete_point", move |l| {
            l.delete_digest_point(&paper_id, &material_id, &point_id, &VersionToken(token))
                .map(|t| t.0)
        })
        .await
}

#[tauri::command]
pub async fn digest_set_proofread(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
    proofread: bool,
    token: String,
) -> ApiResult<String> {
    state
        .lib("digest_set_proofread", move |l| {
            l.set_digest_proofread(&paper_id, &material_id, proofread, &VersionToken(token))
                .map(|t| t.0)
        })
        .await
}

// ---------- 匯出、用其他應用打開 ----------

/// 複製一份到匯出的暫存資料夾（檔名「篇名-重點筆記.md」）。文獻庫裡的檔不動。
async fn digest_copy(
    app: &AppHandle,
    state: &AppState,
    command: &'static str,
    paper_id: &str,
    material_id: &str,
) -> ApiResult<ExportFile> {
    let dir = exports_dir(app).await?;
    let (pid, mid) = (paper_id.to_string(), material_id.to_string());
    state
        .run(command, move |s| {
            let (src, title) = s.with_lib(|l| {
                // 讀得懂才交出去
                l.read_digest(&pid, &mid)?;
                Ok((l.material_path(&pid, &mid)?, paper_title(l, &pid)?))
            })?;
            copy_out(
                &dir,
                &src,
                format!("{}-重點筆記.md", file_stem(&title, "paper")),
            )
        })
        .await
}

/// 匯出重點筆記：交回暫存檔，畫面接著用 `deliver_export` 分享或存到別處。
#[tauri::command]
pub async fn export_digest(
    app: AppHandle,
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
) -> ApiResult<ExportFile> {
    digest_copy(&app, &state, "export_digest", &paper_id, &material_id).await
}

/// 用其他應用打開（唯讀的一份複本）。交回 `opened`、`no_app` 或 `not_shareable`。
#[tauri::command]
pub async fn digest_open_external(
    app: AppHandle,
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
) -> ApiResult<String> {
    let f = digest_copy(
        &app,
        &state,
        "digest_open_external",
        &paper_id,
        &material_id,
    )
    .await?;
    Ok(app.paperreader().open_external(&f.path, false).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::PromptPreset;
    use paperreader_translate::Provider;

    fn std_sections() -> Vec<DigestSection> {
        let p =
            PromptPreset::from_builtin(paperreader_translate::builtin_preset("standard").unwrap());
        section_labels(&p, "zh-TW")
    }

    #[test]
    fn points_the_core_would_refuse_are_dropped_not_the_whole_digest() {
        // 一點不合核心的規則（或服務名稱帶控制字元）不讓整份失敗、付過的錢白費
        let input = DigestInput {
            title: Some("T".into()),
            segments: vec![DigestSegment {
                id: "g0001".into(),
                page: 0,
                text: "x".into(),
            }],
            target_lang: "zh-TW".into(),
            ..Default::default()
        };
        let item = |text: &str, pages: Vec<u32>| paperreader_translate::DigestItem {
            section: "results".into(),
            text: text.into(),
            segments: vec!["g0001".into()],
            pages,
        };
        let r = DigestResult {
            items: vec![
                item("好的一點。", vec![0]),
                item("壞的 --> 一點", vec![0]),
                item("超出頁數", vec![9]),
            ],
            dropped: 0,
            parts: 1,
        };
        let ss = std_sections();
        let d = build_draft(
            ("S", &ss),
            &input,
            &r,
            ("Name\u{7}\n", "anthropic", "m\u{0}x"),
            3,
        );
        assert_eq!(d.points.len(), 1);
        assert_eq!(d.points[0].text, "好的一點。");
        assert!(paperreader_core::digest::validate_draft(&d, 3).is_ok());
    }

    /// 重跑重點筆記取代舊的一份：新的寫好、舊的移到回收區（可以放回）；`replaces` 指的不是重點筆記時不動。
    #[test]
    fn a_new_digest_replaces_the_old_one_which_goes_to_the_trash() {
        use paperreader_core::local::LocalLibrary;
        let dir = tempfile::tempdir().unwrap();
        let mut l = LocalLibrary::open(dir.path(), "01JCK3T1A2B3C4D5E6F7G8H9JK").unwrap();
        let pdf = dir.path().join("in.pdf");
        let mut bytes = b"%PDF-1.4\n".to_vec();
        bytes.extend((0..400u32).map(|i| (i * 7 % 251) as u8));
        std::fs::write(&pdf, bytes).unwrap();
        let id = match l.add_paper(&pdf, Default::default(), Some(3)).unwrap() {
            paperreader_core::store::AddOutcome::Added(id) => id,
            other => panic!("{other:?}"),
        };
        let source = l.get_paper(&id).unwrap().0.materials[0].id.clone();
        let input = DigestInput {
            title: Some("T".into()),
            segments: vec![DigestSegment {
                id: "g0001".into(),
                page: 0,
                text: "x".into(),
            }],
            target_lang: "zh-TW".into(),
            ..Default::default()
        };
        let r = DigestResult {
            items: vec![paperreader_translate::DigestItem {
                section: "results".into(),
                text: "一點。".into(),
                segments: vec!["g0001".into()],
                pages: vec![0],
            }],
            dropped: 0,
            parts: 1,
        };
        let svc = ("Svc", "anthropic", "m");
        let ss = std_sections();
        let src = (source.as_str(), ss.as_slice());
        let first = write_digest_into(&mut l, &id, src, &input, &r, svc, None).unwrap();
        let second = write_digest_into(&mut l, &id, src, &input, &r, svc, Some(&first)).unwrap();
        let digests: Vec<String> = l
            .get_paper(&id)
            .unwrap()
            .0
            .materials
            .into_iter()
            .filter(|m| m.role == paperreader_core::digest::ROLE_DIGEST)
            .map(|m| m.id)
            .collect();
        assert_eq!(digests, vec![second.clone()]);
        let removed = l.list_removed_materials().unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].material.id, first);
        // 放得回去
        l.restore_material(&removed[0].folder).unwrap();
        assert_eq!(
            l.get_paper(&id)
                .unwrap()
                .0
                .materials
                .iter()
                .filter(|m| m.role == paperreader_core::digest::ROLE_DIGEST)
                .count(),
            2
        );
        // 指的是原文：不動
        let third = write_digest_into(&mut l, &id, src, &input, &r, svc, Some(&source)).unwrap();
        let p = l.get_paper(&id).unwrap().0;
        assert!(p.materials.iter().any(|m| m.id == source));
        assert!(p.materials.iter().any(|m| m.id == third));
    }

    #[test]
    fn only_chat_models_are_offered_as_fallback() {
        let mut t = TranslateSettings::default();
        for id in ["deepl", "openai"] {
            t.services.iter_mut().find(|s| s.id == id).unwrap().enabled = true;
        }
        t.fallbacks = vec!["deepl".into(), "bergamot".into(), "openai".into()];
        assert!(t.validate().is_ok());
        let next = next_chat_fallback(&t, "anthropic").unwrap();
        assert_eq!(next.provider, Provider::Openai);
        assert!(next_chat_fallback(&t, "openai").is_none());
    }
}
