//! 匯入同步包的寫入：收斂、冪等、選擇、過期的預覽、中斷與重做、唯讀。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::BibDraft;
use paperreader_core::local::LocalLibrary;
use paperreader_core::overlay::{OverlayDraft, SegmentDraft, SegmentUpdate};
use paperreader_core::store::{
    ChangeFilter, CheckLevel, ConflictChoice, ConflictKind, EventFilter, NoticeKind, ProblemKind,
};
use serde_json::json;
use std::fs;
use std::path::Path;

fn find(lib: &LocalLibrary, title: &str) -> (String, String) {
    let p = lib
        .list_papers()
        .into_iter()
        .find(|p| p.bib.title.as_deref() == Some(title))
        .unwrap_or_else(|| panic!("no paper {title}"));
    (p.id.clone(), p.materials[0].id.clone())
}

fn set_title(lib: &mut LocalLibrary, id: &str, title: &str) {
    let (paper, tok) = lib.get_paper(id).unwrap();
    let d = BibDraft {
        title: Some(title.into()),
        authors: paper.bib.authors.clone(),
        year: paper.bib.year,
        ..Default::default()
    };
    lib.update_bib(id, d, &tok).unwrap();
}

/// 兩台各做每一種修改，而且沒有衝突。
fn diverge(p: &mut Pair) {
    let (alpha, am) = find(&p.a, "Alpha Paper");
    let (beta, bm) = find(&p.a, "Beta Paper");
    let gamma = p.a.list_trash().unwrap()[0].paper_id.clone();

    // B 先刪群組 Vision（Alpha 在裡面）
    let (groups, gt) = p.b.list_groups().unwrap();
    p.b.delete_group(&groups[0].id, &gt).unwrap();

    // ---- A ----
    add_paper(&mut p.a, 201, "A New", Some(3));
    set_title(&mut p.a, &beta, "Beta Paper"); // 書目改過（時間換新）
    let (_, gt) = p.a.list_groups().unwrap();
    let (ga, _) = p.a.create_group("GA", &gt).unwrap();
    let tok = p.a.get_paper(&alpha).unwrap().1;
    p.a.add_to_group(&alpha, &ga.id, &tok).unwrap();
    // 加入又移出群組
    let (_, gt) = p.a.list_groups().unwrap();
    let (gb, _) = p.a.create_group("GB", &gt).unwrap();
    let tok = p.a.get_paper(&beta).unwrap().1;
    let tok = p.a.add_to_group(&beta, &gb.id, &tok).unwrap();
    p.a.remove_from_group(&beta, &gb.id, &tok).unwrap();
    let (notes, nt) = p.a.read_notes(&alpha).unwrap();
    p.a.delete_highlight(&alpha, &notes.highlights[0].id, nt.as_ref().unwrap())
        .unwrap();
    p.a.set_thoughts(&beta, "A thought on beta", None).unwrap();
    let (_, t) = p.a.add_stroke(&alpha, stroke_draft(&am, 1), None).unwrap();
    let (s2, t) =
        p.a.add_stroke(&alpha, stroke_draft(&am, 2), Some(&t))
            .unwrap();
    p.a.delete_stroke(&alpha, &s2.id, &t).unwrap();
    let tok = p.a.get_paper(&alpha).unwrap().1;
    let ov =
        p.a.create_overlay(
            &alpha,
            OverlayDraft {
                source_material: am.clone(),
                provider: "x".into(),
                model: "y".into(),
                source_lang: None,
                target_lang: "zh-TW".into(),
                segments: (0..3)
                    .map(|i| SegmentDraft {
                        page: 0,
                        bbox: [1.0, 1.0, 2.0, 2.0],
                        kind: "body".into(),
                        size: None,
                        source: format!("s{i}"),
                    })
                    .collect(),
            },
            &tok,
        )
        .unwrap();
    p.a.update_overlay_segments(
        &alpha,
        &ov.material_id,
        &[SegmentUpdate {
            id: "g0001".into(),
            status: "done".into(),
            target: Some("譯一".into()),
            error: None,
            issues: vec![],
            service: None,
            by_user: false,
        }],
        &ov.overlay_token,
    )
    .unwrap();
    p.a.set_material_pages(&beta, &bm, 5).unwrap();
    p.a.record_open(&alpha, &am).unwrap();

    // ---- B ----
    add_paper(&mut p.b, 202, "B New", None);
    let (bnotes, bnt) = p.b.read_notes(&alpha).unwrap();
    let _ = bnotes;
    let (bh, bt) =
        p.b.add_highlight(
            &alpha,
            highlight_draft(&am, 3, "B draft comment"),
            bnt.as_ref(),
        )
        .unwrap();
    // 改短評
    p.b.set_highlight_comment(&alpha, &bh.id, "B comment", &bt)
        .unwrap();
    let nt = p.b.read_notes(&alpha).unwrap().1;
    p.b.set_thoughts(&alpha, "B idea on alpha", nt.as_ref())
        .unwrap();
    p.b.restore_paper(&gamma).unwrap();
    let (_, t) = p.b.add_stroke(&beta, stroke_draft(&bm, 0), None).unwrap();
    let (_, t) =
        p.b.add_stroke(&beta, stroke_draft(&bm, 1), Some(&t))
            .unwrap();
    p.b.clear_ink_page(&beta, &bm, 0, &t).unwrap();
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf_seeded(src.path(), "t.pdf", 500, 303);
    let tok = p.b.get_paper(&beta).unwrap().1;
    p.b.add_translation(&beta, &pdf, None, &tok).unwrap();
    p.b.record_open(&beta, &bm).unwrap();

    // A 最後把 Beta 移到回收區（同一篇 B 改了內容）
    p.a.remove_paper(&beta).unwrap();
}

fn assert_converged(p: &Pair) {
    let sa = sync_state(&p.a, &p.a_root());
    let sb = sync_state(&p.b, &p.b_root());
    assert_eq!(
        sa.keys().collect::<Vec<_>>(),
        sb.keys().collect::<Vec<_>>(),
        "同樣的文獻與筆記"
    );
    for (k, v) in &sa {
        assert_eq!(Some(v), sb.get(k), "{k}");
    }
    assert_eq!(
        p.a.read_events(&EventFilter::default()).unwrap(),
        p.b.read_events(&EventFilter::default()).unwrap()
    );
}

fn round_trip(p: &mut Pair) {
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    sync_once(&mut p.b, &mut p.a, &out);
}

#[test]
fn two_devices_converge() {
    let mut p = pair();
    diverge(&mut p);
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(preview.conflicts.is_empty(), "{:?}", preview.conflicts);
    let r = p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert!(r.skipped_papers.is_empty(), "{:?}", r.skipped_papers);
    sync_once(&mut p.b, &mut p.a, &out);
    assert_converged(&p);

    // 經由接口讀出的也相同
    let strip = |lib: &LocalLibrary| {
        let mut v: Vec<_> = lib
            .list_papers()
            .into_iter()
            .map(|mut x| {
                x.folder.clear();
                x
            })
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        format!("{v:?}")
    };
    assert_eq!(strip(&p.a), strip(&p.b));
    let trash = |lib: &LocalLibrary| {
        let mut v: Vec<_> = lib
            .list_trash()
            .unwrap()
            .into_iter()
            .map(|t| (t.paper_id, t.name))
            .collect();
        v.sort();
        v
    };
    assert_eq!(trash(&p.a), trash(&p.b));
    assert_eq!(p.a.list_groups().unwrap().0, p.b.list_groups().unwrap().0);
    for x in p.a.list_papers() {
        let mut na = p.a.read_notes(&x.id).unwrap().0;
        let mut nb = p.b.read_notes(&x.id).unwrap().0;
        na.path = None;
        nb.path = None;
        na.modified = None;
        nb.modified = None;
        na.modified_by = None;
        nb.modified_by = None;
        assert_eq!(na, nb);
    }

    // 具體的結果：刪掉的劃線、筆跡沒有回來；B 的想法到了 A；Beta 在回收區；Gamma 回到目錄
    let (alpha, _) = find(&p.a, "Alpha Paper");
    let notes = p.a.read_notes(&alpha).unwrap().0;
    assert_eq!(notes.highlights.len(), 1);
    assert_eq!(notes.highlights[0].comment, "B comment");
    assert_eq!(notes.thoughts, "B idea on alpha");
    assert!(
        p.a.list_papers()
            .iter()
            .any(|x| x.bib.title.as_deref() == Some("Gamma Trash"))
    );
    assert!(
        p.a.list_trash()
            .unwrap()
            .iter()
            .any(|t| t.bib.title.as_deref() == Some("Beta Paper"))
    );
    let (ink, _) = p.a.read_ink(&alpha).unwrap();
    assert_eq!(ink.strokes.len(), 1);
    let g: Vec<_> =
        p.a.list_groups()
            .unwrap()
            .0
            .into_iter()
            .filter(|g| g.name == "GA")
            .collect();
    assert_eq!(g.len(), 1);
    assert!(
        p.a.list_groups()
            .unwrap()
            .0
            .iter()
            .all(|g| g.name != "Vision")
    );
    let (beta, _) = (
        p.a.list_trash()
            .unwrap()
            .into_iter()
            .find(|t| t.bib.title.as_deref() == Some("Beta Paper"))
            .unwrap()
            .paper_id,
        (),
    );
    let beta_paper: serde_json::Value = serde_json::from_slice(
        &fs::read(
            p.a_root()
                .join(".trash")
                .join(
                    &p.a.list_trash()
                        .unwrap()
                        .into_iter()
                        .find(|t| t.paper_id == beta)
                        .unwrap()
                        .folder,
                )
                .join("paper.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(beta_paper["groups"], json!([]), "移出群組也同步了");
    assert_eq!(
        p.a.get_paper(&alpha).unwrap().0.groups,
        vec![g[0].id.clone()]
    );
}

#[test]
fn third_round_is_noop() {
    let mut p = pair();
    diverge(&mut p);
    round_trip(&mut p);
    // 可同步的檔（不含紀錄檔）逐位元組
    let files = |root: &Path| -> std::collections::BTreeMap<String, Option<Vec<u8>>> {
        snapshot(root)
            .into_iter()
            .filter(|(k, _)| !k.starts_with("logs"))
            .collect()
    };
    let before_a = files(&p.a_root());
    let before_b = files(&p.b_root());
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(preview.added.is_empty(), "{:?}", preview.added);
    assert!(preview.updated.is_empty(), "{:?}", preview.updated);
    assert!(preview.removed.is_empty(), "{:?}", preview.removed);
    assert!(preview.conflicts.is_empty());
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    sync_once(&mut p.b, &mut p.a, &out);
    assert_eq!(files(&p.a_root()), before_a);
    assert_eq!(files(&p.b_root()), before_b);
}

/// A 與 B 各改同一篇的書目，時間相同（直接寫檔）：書目衝突；兩邊各改想法：想法衝突。
fn conflicting(p: &mut Pair) -> String {
    let (alpha, _) = find(&p.a, "Alpha Paper");
    for (lib, root, title) in [
        (&mut p.a, p.a_dir.path().to_path_buf(), "Title A"),
        (&mut p.b, p.b_dir.path().join("lib"), "Title B"),
    ] {
        let folder = lib
            .list_papers()
            .into_iter()
            .find(|x| x.id == alpha)
            .unwrap()
            .folder;
        let path = root.join("papers").join(folder).join("paper.json");
        let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        v["bib"]["title"] = json!(title);
        v["bib"]["modified"] = json!("2026-11-20T00:00:00.000Z");
        v["bib"]["modified_by"] = json!(DEVICE);
        fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        lib.rescan().unwrap();
    }
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "A version", nt.as_ref()).unwrap();
    let nt = p.b.read_notes(&alpha).unwrap().1;
    p.b.set_thoughts(&alpha, "B version", nt.as_ref()).unwrap();
    alpha
}

#[test]
fn every_choice_required() {
    let mut p = pair();
    conflicting(&mut p);
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert_eq!(preview.conflicts.len(), 2, "{:?}", preview.conflicts);
    let before = snapshot(&p.b_root());
    let all: Vec<(String, ConflictChoice)> = preview
        .conflicts
        .iter()
        .map(|c| (c.id.clone(), c.default))
        .collect();
    // 少一個
    let r = p.b.apply_sync_import(&preview.preview_id, &all[..1]);
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    // 多一個不存在的
    let mut more = all.clone();
    more.push(("c99".into(), ConflictChoice::Local));
    assert!(matches!(
        p.b.apply_sync_import(&preview.preview_id, &more),
        Err(LibError::InvalidInput { .. })
    ));
    // 選了不提供的選項（書目衝突沒有 both）
    let bib = preview
        .conflicts
        .iter()
        .find(|c| c.kind == ConflictKind::Bib)
        .unwrap();
    let bad: Vec<_> = all
        .iter()
        .map(|(id, c)| {
            if *id == bib.id {
                (id.clone(), ConflictChoice::Both)
            } else {
                (id.clone(), *c)
            }
        })
        .collect();
    assert!(matches!(
        p.b.apply_sync_import(&preview.preview_id, &bad),
        Err(LibError::InvalidInput { .. })
    ));
    assert_eq!(snapshot(&p.b_root()), before, "什麼都沒寫");
    // 預覽還在
    p.b.apply_sync_import(&preview.preview_id, &all).unwrap();
}

#[test]
fn choices_applied_and_stamped() {
    let mut p = pair();
    let alpha = conflicting(&mut p);
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let choices: Vec<_> = preview
        .conflicts
        .iter()
        .map(|c| {
            let ch = match c.kind {
                ConflictKind::Bib => ConflictChoice::Incoming,
                _ => ConflictChoice::Both,
            };
            (c.id.clone(), ch)
        })
        .collect();
    let r =
        p.b.apply_sync_import(&preview.preview_id, &choices)
            .unwrap();
    let (paper, _) = p.b.get_paper(&alpha).unwrap();
    assert_eq!(paper.bib.title.as_deref(), Some("Title A"));
    assert_eq!(paper.bib.modified_by.as_deref(), Some(DEVICE_B));
    assert_ne!(
        paper.bib.modified.as_deref(),
        Some("2026-11-20T00:00:00.000Z")
    );
    assert_eq!(r.conflict_copies.len(), 1, "{r:?}");
    assert!(p.b_root().join(&r.conflict_copies[0]).is_file());
    assert_eq!(p.b.read_notes(&alpha).unwrap().0.thoughts, "B version");
    let resolved =
        p.b.read_changes(&ChangeFilter {
            paper: Some(alpha.clone()),
            kind: Some("thoughts-written".into()),
        })
        .unwrap();
    let last = resolved.iter().find(|r| r.prev().len() == 2);
    assert!(last.is_some(), "{resolved:?}");
    // 反向匯入：A 不再出現衝突
    let pkg = export_pkg(&mut p.b, &out);
    let preview = p.a.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(preview.conflicts.is_empty(), "{:?}", preview.conflicts);
    p.a.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert_eq!(
        p.a.get_paper(&alpha).unwrap().0.bib.title.as_deref(),
        Some("Title A")
    );
    assert_eq!(p.a.read_notes(&alpha).unwrap().0.thoughts, "B version");
}

#[test]
fn stale_preview_conflict() {
    let mut p = pair();
    let (alpha, _) = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "From A");
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    set_title(&mut p.b, &alpha, "Changed after preview");
    let before = snapshot(&p.b_root());
    let r = p.b.apply_sync_import(&preview.preview_id, &[]);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    let after = snapshot(&p.b_root());
    assert_eq!(
        before
            .into_iter()
            .filter(|(k, _)| !k.starts_with(".pr-sync"))
            .collect::<Vec<_>>(),
        after.into_iter().collect::<Vec<_>>()
    );
    // 預覽已丟棄
    assert!(matches!(
        p.b.apply_sync_import(&preview.preview_id, &[]),
        Err(LibError::NotFound { .. })
    ));
}

#[test]
fn reading_after_prepare_is_fine() {
    let mut p = pair();
    let (alpha, am) = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "From A");
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let e =
        p.b.record_position(&alpha, &am, 3, 0.5, None)
            .unwrap()
            .unwrap();
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert!(
        p.b.read_events(&EventFilter::default())
            .unwrap()
            .iter()
            .any(|x| x.id == e.id)
    );
}

fn stop_at(lib: &mut LocalLibrary, at: usize) {
    lib.set_sync_apply_hook(Some(Box::new(move |k| k != at)));
}

#[test]
fn interrupted_before_plan_leaves_library_untouched() {
    let mut p = pair();
    diverge(&mut p);
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let before = snapshot(&p.b_root());
    stop_at(&mut p.b, 0);
    assert!(p.b.apply_sync_import(&preview.preview_id, &[]).is_err());
    let root = p.b_root();
    drop(p.b);
    let b = LocalLibrary::open(&root, DEVICE_B).unwrap();
    let after = snapshot(&root);
    let strip = |m: std::collections::BTreeMap<String, Option<Vec<u8>>>| {
        m.into_iter()
            .filter(|(k, _)| !k.starts_with(".pr-sync"))
            .collect::<Vec<_>>()
    };
    assert_eq!(strip(before), strip(after.clone()));
    assert!(!after.keys().any(|k| k.starts_with(".pr-sync")));
    assert!(!b.is_read_only());
}

#[test]
fn staging_in_removed_before_plan() {
    let mut p = pair();
    add_paper(&mut p.a, 333, "New Big", None);
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    // 停在寫計畫檔之前，暫存資料夾留著（不清掉）看裡面
    let root = p.b_root();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let s2 = seen.clone();
    let r2 = root.clone();
    p.b.set_sync_apply_hook(Some(Box::new(move |k| {
        if k == 0 {
            let d = fs::read_dir(&r2)
                .unwrap()
                .flatten()
                .find(|e| e.file_name().to_string_lossy().starts_with(".pr-sync-"))
                .unwrap()
                .path();
            *s2.lock().unwrap() = Some((d.join("in").exists(), snapshot(&d.join("out"))));
        }
        true
    })));
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    let (in_exists, out_files) = seen.lock().unwrap().take().unwrap();
    assert!(!in_exists, "in/ 在寫計畫檔之前已刪");
    assert!(
        out_files.keys().any(|k| k.ends_with("source.pdf")),
        "{:?}",
        out_files.keys()
    );
    assert!(
        !fs::read_dir(&root)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with(".pr-sync-"))
    );
}

/// 同一篇的內容有改、又要搬到回收區；同一篇的內容有改、又要從回收區搬回。
fn diverge_with_moves(p: &mut Pair) {
    let (alpha, am) = find(&p.a, "Alpha Paper");
    let gamma = p.a.list_trash().unwrap()[0].paper_id.clone();
    set_title(&mut p.a, &alpha, "Alpha moved");
    p.a.add_stroke(&alpha, stroke_draft(&am, 0), None).unwrap();
    p.a.remove_paper(&alpha).unwrap();
    p.a.restore_paper(&gamma).unwrap();
    set_title(&mut p.a, &gamma, "Gamma back");
    add_paper(&mut p.a, 444, "Fresh", None);
}

/// 中斷重做的比對：可同步的檔的完整內容（`sync_state`），加上每份紀錄檔的每一行；
/// `sync-imported` 那一行的代號與時間在兩次執行之間本來就不同，比之前拿掉。
type FullState = (
    std::collections::BTreeMap<String, serde_json::Value>,
    std::collections::BTreeMap<String, Vec<serde_json::Value>>,
);

fn full_state(lib: &LocalLibrary, root: &Path) -> FullState {
    let mut logs = std::collections::BTreeMap::new();
    for (name, bytes) in logs_snapshot(root) {
        let Some(bytes) = bytes else { continue };
        let lines: Vec<serde_json::Value> = String::from_utf8(bytes)
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
                if v["type"] == json!("sync-imported") {
                    let o = v.as_object_mut().unwrap();
                    o.remove("id");
                    o.remove("t");
                }
                v
            })
            .collect();
        logs.insert(name, lines);
    }
    (sync_state(lib, root), logs)
}

#[test]
fn interrupted_after_plan_rolls_forward_on_open() {
    // 同一份起點與同一份同步包：每次從 B 的複本重來
    let mut p = pair();
    diverge_with_moves(&mut p);
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let seed = out.join("b-seed");
    copy_dir(&p.b_root(), &seed);
    let run = |name: &str, stop: Option<usize>| -> (FullState, usize) {
        let root = out.join(name);
        copy_dir(&seed, &root);
        let mut b = LocalLibrary::open(&root, DEVICE_B).unwrap();
        let preview = b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c2 = count.clone();
        b.set_sync_apply_hook(Some(Box::new(move |k| {
            c2.fetch_max(k, std::sync::atomic::Ordering::SeqCst);
            Some(k) != stop
        })));
        let r = b.apply_sync_import(&preview.preview_id, &[]);
        assert_eq!(r.is_err(), stop.is_some(), "{name}");
        drop(b);
        let b = LocalLibrary::open(&root, DEVICE_B).unwrap();
        assert!(!b.is_read_only(), "{name}");
        assert!(
            !fs::read_dir(&root)
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with(".pr-sync-"))
        );
        (
            full_state(&b, &root),
            count.load(std::sync::atomic::Ordering::SeqCst),
        )
    };
    let (want, n_items) = run("whole", None);
    assert!(n_items >= 6, "{n_items}");
    for at in 1..=n_items {
        let (got, _) = run(&format!("stop{at}"), Some(at));
        assert_eq!(got, want, "stop at {at}");
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            fs::copy(e.path(), t).unwrap();
        }
    }
}

#[test]
fn roll_forward_twice_is_idempotent() {
    let mut p = pair();
    diverge_with_moves(&mut p);
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    // 第 1 項（附加記錄）之前停：計畫資料夾留一份複本
    let root = p.b_root();
    let keep = p.out.path().join("plan-copy");
    let r2 = root.clone();
    let k2 = keep.clone();
    p.b.set_sync_apply_hook(Some(Box::new(move |k| {
        if k == 1 {
            let d = fs::read_dir(&r2)
                .unwrap()
                .flatten()
                .find(|e| e.file_name().to_string_lossy().starts_with(".pr-sync-"))
                .unwrap()
                .path();
            copy_dir(&d, &k2.join(d.file_name().unwrap()));
        }
        true
    })));
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    let state = sync_state(&p.b, &root);
    let logs = logs_snapshot(&root);
    drop(p.b);
    // 放回去，重新開啟 → 重做一遍
    for e in fs::read_dir(&keep).unwrap().flatten() {
        copy_dir(&e.path(), &root.join(e.file_name()));
    }
    let b = LocalLibrary::open(&root, DEVICE_B).unwrap();
    assert_eq!(sync_state(&b, &root), state);
    assert_eq!(logs_snapshot(&root), logs, "紀錄檔沒有重複的行");
    let imported = b
        .read_changes(&ChangeFilter {
            paper: None,
            kind: Some("sync-imported".into()),
        })
        .unwrap();
    assert_eq!(imported.iter().filter(|r| r.device == DEVICE_B).count(), 1);
}

#[test]
fn roll_forward_does_not_overwrite_later_edits() {
    let mut p = pair();
    let (alpha, am) = find(&p.a, "Alpha Paper");
    let nt = p.a.read_notes(&alpha).unwrap().1;
    p.a.set_thoughts(&alpha, "from A", nt.as_ref()).unwrap();
    p.a.add_stroke(&alpha, stroke_draft(&am, 0), None).unwrap();
    // B 也先有 ink.json，好讓合併寫的是「改」而不是「建」
    p.b.add_stroke(&alpha, stroke_draft(&am, 1), None).unwrap();
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    // 停在第一項（附加記錄）之前：計畫已寫成
    stop_at(&mut p.b, 1);
    assert!(p.b.apply_sync_import(&preview.preview_id, &[]).is_err());
    let notes_rel = p.b.read_notes(&alpha).unwrap().0.path.unwrap();
    let root = p.b_root();
    drop(p.b);
    let notes = root.join(&notes_rel);
    let edited = fs::read_to_string(&notes).unwrap() + "\nmy own line\n";
    fs::write(&notes, &edited).unwrap();
    let ink = root.join("papers/Alpha Paper/ink.json");
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&ink).unwrap()).unwrap();
    v["edited_outside"] = json!(true);
    let ink_text = serde_json::to_string_pretty(&v).unwrap();
    fs::write(&ink, &ink_text).unwrap();
    let mut b = LocalLibrary::open(&root, DEVICE_B).unwrap();
    assert_eq!(fs::read_to_string(&notes).unwrap(), edited);
    assert_eq!(fs::read_to_string(&ink).unwrap(), ink_text);
    let report = b.resume_sync_import().unwrap();
    let unmerged: Vec<_> = report
        .notices
        .iter()
        .filter(|n| n.kind == NoticeKind::UnmergedWrite)
        .collect();
    assert_eq!(unmerged.len(), 2, "{:?}", report.notices);
    assert!(
        report
            .conflict_copies
            .iter()
            .any(|c| root.join(c).is_file())
    );
    assert!(!b.is_read_only());
}

#[test]
fn unfinished_plan_makes_library_read_only() {
    let mut p = pair();
    let (alpha, am) = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "From A");
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    stop_at(&mut p.b, 2);
    assert!(p.b.apply_sync_import(&preview.preview_id, &[]).is_err());
    assert!(p.b.is_read_only());
    let tok = p.b.get_paper(&alpha).unwrap().1;
    assert!(matches!(
        p.b.update_bib(&alpha, BibDraft::default(), &tok),
        Err(LibError::ReadOnly)
    ));
    assert!(matches!(
        p.b.add_highlight(&alpha, highlight_draft(&am, 0, "x"), None),
        Err(LibError::ReadOnly)
    ));
    let problems = p.b.check(CheckLevel::Quick).unwrap();
    assert!(
        problems
            .iter()
            .any(|x| x.kind == ProblemKind::UnfinishedSyncImport)
    );
    p.b.set_sync_apply_hook(None);
    p.b.resume_sync_import().unwrap();
    assert!(!p.b.is_read_only());
    assert_eq!(
        p.b.get_paper(&alpha).unwrap().0.bib.title.as_deref(),
        Some("From A")
    );
    let tok = p.b.get_paper(&alpha).unwrap().1;
    set_title(&mut p.b, &alpha, "writable again");
    let _ = tok;
}

#[cfg(unix)]
#[test]
fn roll_forward_failure_keeps_plan() {
    use std::os::unix::fs::PermissionsExt;
    let mut p = pair();
    let (alpha, _) = find(&p.a, "Alpha Paper");
    set_title(&mut p.a, &alpha, "From A");
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    stop_at(&mut p.b, 1);
    assert!(p.b.apply_sync_import(&preview.preview_id, &[]).is_err());
    let root = p.b_root();
    drop(p.b);
    let dir = root.join("papers/Alpha Paper");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let b = LocalLibrary::open(&root, DEVICE_B).unwrap();
    assert!(b.is_read_only());
    assert!(
        b.check(CheckLevel::Quick)
            .unwrap()
            .iter()
            .any(|x| x.kind == ProblemKind::UnfinishedSyncImport)
    );
    drop(b);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    let b = LocalLibrary::open(&root, DEVICE_B).unwrap();
    assert!(!b.is_read_only());
    assert_eq!(
        b.get_paper(&alpha).unwrap().0.bib.title.as_deref(),
        Some("From A")
    );
}

#[test]
fn quick_check_clean_after_merge() {
    let mut p = pair();
    diverge(&mut p);
    round_trip(&mut p);
    assert_eq!(p.a.check(CheckLevel::Full).unwrap(), vec![]);
    assert_eq!(p.b.check(CheckLevel::Full).unwrap(), vec![]);
}

#[test]
fn tokens_are_stale_after_apply() {
    let mut p = pair();
    let (alpha, _) = find(&p.a, "Alpha Paper");
    let (beta, _) = find(&p.a, "Beta Paper");
    set_title(&mut p.a, &alpha, "From A");
    let alpha_tok = p.b.get_paper(&alpha).unwrap().1;
    let beta_tok = p.b.get_paper(&beta).unwrap().1;
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    assert!(matches!(
        p.b.update_bib(&alpha, BibDraft::default(), &alpha_tok),
        Err(LibError::Conflict)
    ));
    p.b.update_bib(&beta, BibDraft::default(), &beta_tok)
        .unwrap();
}

#[test]
fn resume_after_auto_resume_finishes_the_new_plan() {
    let mut p = pair();
    let (alpha, _) = find(&p.a, "Alpha Paper");
    let (beta, _) = find(&p.a, "Beta Paper");
    set_title(&mut p.a, &alpha, "First");
    let out = p.out.path().to_path_buf();
    let pkg = export_pkg(&mut p.a, &out);
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    stop_at(&mut p.b, 2);
    assert!(p.b.apply_sync_import(&preview.preview_id, &[]).is_err());
    let root = p.b_root();
    drop(p.b);
    // 重開時自動做完第一份；回報留在記憶體裡
    let mut b = LocalLibrary::open(&root, DEVICE_B).unwrap();
    set_title(&mut p.a, &beta, "Second");
    let pkg2 = export_pkg(&mut p.a, &out);
    let preview = b.prepare_sync_import(&pkg2, &mut |_, _| {}).unwrap();
    stop_at(&mut b, 2);
    assert!(b.apply_sync_import(&preview.preview_id, &[]).is_err());
    b.set_sync_apply_hook(None);
    let r = b.resume_sync_import().unwrap();
    assert_eq!(r.package, preview.package.package, "做完的是新的那一份");
    assert!(!b.is_read_only());
    assert_eq!(
        b.get_paper(&beta).unwrap().0.bib.title.as_deref(),
        Some("Second")
    );
}

/// 同步包的操作走一遍（匯出、準備、丟棄、再準備、寫入、沒有寫到一半的合併時做完回報錯誤、
/// 讀變更紀錄）。
#[test]
fn sync_operations_round_trip() {
    use paperreader_core::store::{ChangeFilter, SyncMode};
    let mut p = pair();
    common::add_paper(&mut p.a, 808, "Through trait", None);
    let a = &mut p.a;
    let pkg = p.out.path().join("t.zip");
    let r = a
        .export_sync_package(&pkg, SyncMode::Changes, Some("x"), &mut |_, _| {})
        .unwrap();
    assert_eq!(r.mode, SyncMode::Full);
    let b = &mut p.b;
    let preview = b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    b.discard_sync_import(&preview.preview_id).unwrap();
    let preview = b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    let report = b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert_eq!(report.package, r.package);
    assert!(b.resume_sync_import().is_err());
    let imported = b
        .read_changes(&ChangeFilter {
            paper: None,
            kind: Some("sync-imported".into()),
        })
        .unwrap();
    assert_eq!(imported.len(), 1);
}
