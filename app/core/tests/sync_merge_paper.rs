//! `paper.json` 與材料的合併。純計算（另有幾項走兩台文獻庫）。

mod common;

use common::*;
use paperreader_core::format::{Material, PaperFile};
use paperreader_core::store::{ConflictChoice, ConflictKind, MaterialStatus, NoticeKind};
use paperreader_core::sync::merge::paper::{LocalFolder, merge_paper, prepare_new_paper};
use serde_json::json;
use std::collections::HashSet;

fn base() -> PaperFile {
    let mut p = sample_paper("01JCK3W2A2B3C4D5E6F7G8H9JK");
    p.materials[0].id = "M1".into();
    p.materials[0].sha256 = Some("a".repeat(64));
    p.materials[0].bytes = Some(10);
    p
}

fn run(
    local: &PaperFile,
    remote: &PaperFile,
    ok: bool,
) -> (
    paperreader_core::sync::merge::paper::PaperMerge,
    paperreader_core::sync::merge::MergeCtx,
) {
    run_with(local, remote, ok, &[], &[], None)
}

fn run_with(
    local: &PaperFile,
    remote: &PaperFile,
    ok: bool,
    names: &[&str],
    choices: &[(ConflictKind, Option<&str>, Option<&str>, ConflictChoice)],
    deleted: Option<&HashSet<String>>,
) -> (
    paperreader_core::sync::merge::paper::PaperMerge,
    paperreader_core::sync::merge::MergeCtx,
) {
    let mut ctx = mctx(choices);
    let names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
    let f = move |_: &Material| {
        if ok {
            MaterialStatus::Ok
        } else {
            MaterialStatus::Missing
        }
    };
    let m = merge_paper(
        local,
        &LocalFolder {
            names: &names,
            file_state: &f,
        },
        remote,
        deleted,
        &[],
        Some("t"),
        &mut ctx,
    );
    (m, ctx)
}

#[test]
fn bib_newer_wins_whole_block_with_extra() {
    let l = base();
    let mut r = base();
    r.bib.title = Some("Newer".into());
    r.bib.modified = Some("2026-11-20T00:00:00.000Z".into());
    r.bib.extra.insert("note".into(), json!("x"));
    let (m, ctx) = run(&l, &r, true);
    assert_eq!(m.paper.bib, r.bib);
    assert!(m.changed);
    assert!(ctx.conflicts.is_empty());
}

#[test]
fn bib_same_time_different_content_conflict() {
    let l = base();
    let mut r = base();
    r.bib.title = Some("Other".into());
    let (m, ctx) = run(&l, &r, true);
    assert_eq!(ctx.conflicts.len(), 1);
    assert_eq!(ctx.conflicts[0].1.kind, ConflictKind::Bib);
    assert_eq!(ctx.conflicts[0].1.default, ConflictChoice::Local);
    assert_eq!(m.paper.bib.title.as_deref(), Some("Sample"));
    assert_eq!(
        m.paper.bib.modified.as_deref(),
        Some(NOW),
        "解決就是一次新的修改"
    );
    let (m, _) = run_with(
        &l,
        &r,
        true,
        &[],
        &[(
            ConflictKind::Bib,
            Some(&l.id),
            None,
            ConflictChoice::Incoming,
        )],
        None,
    );
    assert_eq!(m.paper.bib.title.as_deref(), Some("Other"));
    assert_eq!(m.paper.bib.modified_by.as_deref(), Some(DEVICE_B));
}

// ---- 書目的空欄----

/// 一台的逐篇處理補了 DOI 與網址（不改書目的時間），另一台沒補：時間相同，互相補上之後相同，不問。
#[test]
fn bib_filled_on_one_side_same_time_merges_without_asking() {
    let l = base();
    let mut r = base();
    r.bib.doi = Some("10.1/x".into());
    r.bib.url = Some("https://arxiv.org/abs/2304.08069".into());
    let (m, ctx) = run(&l, &r, true);
    assert!(ctx.conflicts.is_empty());
    assert_eq!(m.paper.bib.doi.as_deref(), Some("10.1/x"));
    assert_eq!(m.paper.bib.url, r.bib.url);
    assert_eq!(m.paper.bib.modified, l.bib.modified, "補空欄不改書目的時間");
    assert!(m.changed);
    // 反過來（補了的在本機）：不必寫
    let (m, ctx) = run(&r, &l, true);
    assert!(ctx.conflicts.is_empty());
    assert_eq!(m.paper.bib, r.bib);
    assert!(!m.changed);
}

/// 一台改了篇名（較新），另一台補了 DOI：篇名與 DOI 都在，不問。
#[test]
fn bib_newer_title_and_older_fill_both_kept() {
    let mut l = base();
    l.bib.doi = Some("10.1/filled".into());
    let mut r = base();
    r.bib.title = Some("Renamed".into());
    r.bib.modified = Some("2026-11-20T00:00:00.000Z".into());
    let (m, ctx) = run(&l, &r, true);
    assert!(ctx.conflicts.is_empty());
    assert_eq!(m.paper.bib.title.as_deref(), Some("Renamed"));
    assert_eq!(m.paper.bib.doi.as_deref(), Some("10.1/filled"));
    assert_eq!(m.paper.bib.modified, r.bib.modified);
    // 本機較新、匯入的補了：本機的空欄也補（原本這種情況什麼都不做）
    let (m, ctx) = run(&r, &l, true);
    assert!(ctx.conflicts.is_empty());
    assert_eq!(m.paper.bib.title.as_deref(), Some("Renamed"));
    assert_eq!(m.paper.bib.doi.as_deref(), Some("10.1/filled"));
    assert!(m.changed);
}

/// 時間相同而兩邊補了不同的值：`Bib` 衝突。
#[test]
fn bib_same_time_different_fills_conflict() {
    let mut l = base();
    l.bib.doi = Some("10.1/one".into());
    let mut r = base();
    r.bib.doi = Some("10.1/two".into());
    let (_, ctx) = run(&l, &r, true);
    assert_eq!(ctx.conflicts.len(), 1);
    assert_eq!(ctx.conflicts[0].1.kind, ConflictKind::Bib);
}

/// `Bib` 衝突的兩邊帶上不同的那幾欄：只差自動補的欄時，預覽看得出差在哪。
#[test]
fn bib_conflict_sides_show_the_differing_fields() {
    let mut l = base();
    l.bib.year = Some(2023);
    l.bib.doi = Some("10.1000/same".into());
    let mut r = base();
    r.bib.year = Some(2024);
    r.bib.doi = Some("10.1000/same".into());
    r.bib.url = Some("https://arxiv.org/abs/2304.08069".into());
    let (_, ctx) = run(&l, &r, true);
    assert_eq!(ctx.conflicts.len(), 1);
    let c = &ctx.conflicts[0].1;
    let show = |side: &paperreader_core::store::ConflictSide| -> Vec<(String, Option<String>)> {
        side.fields
            .iter()
            .map(|f| (f.name.clone(), f.value.clone()))
            .collect()
    };
    // 網址：本機空著、匯入的有值，互相補空欄之後相同，不算差別；只差年份
    assert_eq!(
        show(&c.local),
        vec![("year".to_string(), Some("2023".to_string()))]
    );
    assert_eq!(
        show(&c.incoming),
        vec![("year".to_string(), Some("2024".to_string()))]
    );
    // 篇名與作者不同時也列
    let mut r2 = r.clone();
    r2.bib.title = Some("Other".into());
    r2.bib.authors[0].family = Some("Lee".into());
    let (_, ctx) = run(&l, &r2, true);
    let names: Vec<String> = ctx.conflicts[0]
        .1
        .incoming
        .fields
        .iter()
        .map(|f| f.name.clone())
        .collect();
    assert_eq!(names, ["title", "authors", "year"]);
    assert_eq!(
        ctx.conflicts[0].1.incoming.fields[1].value.as_deref(),
        Some("Lee, Yian")
    );
}

/// 較新那份 `cleared` 記下的欄不補；只補 `year`、`doi`、`url` 三欄。
#[test]
fn bib_fill_skips_cleared_fields_and_other_fields() {
    let mut l = base();
    l.bib.doi = Some("10.1/wrong".into());
    l.bib.venue = Some("Old venue".into());
    let mut r = base();
    r.bib.year = None;
    r.bib.cleared = vec!["doi".into()];
    r.bib.modified = Some("2026-11-20T00:00:00.000Z".into());
    let (m, ctx) = run(&l, &r, true);
    assert!(ctx.conflicts.is_empty());
    assert_eq!(m.paper.bib.doi, None);
    assert_eq!(m.paper.bib.venue, None, "出處不是自動補的欄");
    assert_eq!(m.paper.bib.year, Some(2024), "年份照樣補");
    assert_eq!(m.paper.bib.cleared, vec!["doi".to_string()]);
}

/// 本機較新、本機 `cleared` 記下的欄：匯入那邊有值也不補（`Ordering::Less` 的方向）。
#[test]
fn bib_local_newer_with_cleared_is_not_filled() {
    let mut l = base();
    l.bib.year = None;
    l.bib.doi = None;
    l.bib.cleared = vec!["year".into()];
    l.bib.modified = Some("2026-11-20T00:00:00.000Z".into());
    let mut r = base();
    r.bib.year = Some(2023);
    r.bib.doi = Some("10.1000/r".into());
    let (m, ctx) = run(&l, &r, true);
    assert!(ctx.conflicts.is_empty());
    assert_eq!(m.paper.bib.year, None);
    assert_eq!(
        m.paper.bib.doi.as_deref(),
        Some("10.1000/r"),
        "沒記下的照補"
    );
    assert_eq!(m.paper.bib.cleared, vec!["year".to_string()]);
}

/// 較舊那份 `cleared` 記下、較新那份仍空著的欄：併進結果的 `cleared`，那一欄也不補；兩個方向算出同一個結果。
#[test]
fn bib_older_cleared_carries_into_the_newer_and_is_symmetric() {
    // A：清掉了年份（較舊）；B：改了篇名（較新），年份空著，但補過網址
    let mut a = base();
    a.bib.year = None;
    a.bib.cleared = vec!["year".into()];
    a.bib.modified = Some("2026-11-18T00:00:00.000Z".into());
    let mut b = base();
    b.bib.title = Some("Renamed".into());
    b.bib.year = None;
    b.bib.url = Some("https://arxiv.org/abs/2304.08069".into());
    b.bib.modified = Some("2026-11-19T00:00:00.000Z".into());
    let (on_a, ca) = run(&a, &b, true);
    let (on_b, cb) = run(&b, &a, true);
    assert!(ca.conflicts.is_empty() && cb.conflicts.is_empty());
    assert_eq!(on_a.paper.bib, on_b.paper.bib, "兩個方向同一個結果");
    let bib = &on_a.paper.bib;
    assert_eq!(bib.title.as_deref(), Some("Renamed"));
    assert_eq!(bib.year, None);
    assert_eq!(bib.cleared, vec!["year".to_string()]);
    assert_eq!(bib.modified, b.bib.modified);
    // 較新那份那一欄有值時不併：較新的值留著
    let mut b2 = b.clone();
    b2.bib.year = Some(2024);
    let (on_a, _) = run(&a, &b2, true);
    assert_eq!(on_a.paper.bib.year, Some(2024));
    assert!(on_a.paper.bib.cleared.is_empty());
}

/// 時間相同：兩邊各自記下的 `cleared` 互相併進來，結果相同、不問。
#[test]
fn bib_same_time_cleared_merge_without_conflict() {
    let mut l = base();
    l.bib.year = None;
    l.bib.cleared = vec!["year".into()];
    let mut r = base();
    r.bib.year = None;
    r.bib.url = None;
    r.bib.doi = None;
    r.bib.cleared = vec!["doi".into()];
    let (m, ctx) = run(&l, &r, true);
    let (m2, _) = run(&r, &l, true);
    assert!(ctx.conflicts.is_empty());
    assert_eq!(m.paper.bib, m2.paper.bib);
    assert_eq!(
        m.paper.bib.cleared,
        vec!["year".to_string(), "doi".to_string()]
    );
}

#[test]
fn tie_breaks_by_device() {
    let mut l = base();
    let mut r = base();
    l.bib.modified_by = Some(DEVICE.into()); // 01JCK…
    r.bib.modified_by = Some(DEVICE_B.into()); // 01JD9… 較大
    r.bib.title = Some("Bigger device".into());
    let (m, ctx) = run(&l, &r, true);
    assert!(ctx.conflicts.is_empty());
    assert_eq!(m.paper.bib.title.as_deref(), Some("Bigger device"));
    // 時間不合格式的算最舊
    let mut r2 = base();
    r2.bib.modified = Some("2026-13-01".into());
    r2.bib.title = Some("bad time".into());
    let (m, _) = run(&base(), &r2, true);
    assert_eq!(m.paper.bib.title.as_deref(), Some("Sample"));
}

#[test]
fn added_takes_earlier_and_aliases_union() {
    let mut l = base();
    let mut r = base();
    l.added = Some("2026-11-10T00:00:00.000Z".into());
    r.added = Some("2026-11-05T00:00:00.000Z".into());
    l.aliases = vec!["b".into(), "a".into()];
    r.aliases = vec!["a".into(), "c".into()];
    let (m, _) = run(&l, &r, true);
    assert_eq!(m.paper.added.as_deref(), Some("2026-11-05T00:00:00.000Z"));
    assert_eq!(m.paper.aliases, ["b", "a", "c"]);
}

#[test]
fn short_name_rules() {
    let mut l = base();
    let mut r = base();
    r.short_name = Some("R".into());
    let (m, _) = run(&l, &r, true);
    assert_eq!(m.paper.short_name.as_deref(), Some("R"));
    l.short_name = Some("L".into());
    let (m, ctx) = run(&l, &r, true);
    assert_eq!(m.paper.short_name.as_deref(), Some("L"));
    assert!(ctx.conflicts.is_empty());
}

#[test]
fn groups_block_newer_wins_and_deleted_ids_stripped_time_kept() {
    let mut l = base();
    let mut r = base();
    l.groups = vec!["G1".into()];
    l.groups_modified = Some("2026-11-01T00:00:00.000Z".into());
    r.groups = vec!["G1".into(), "G2".into(), "G3".into()];
    r.groups_modified = Some("2026-11-02T00:00:00.000Z".into());
    r.groups_modified_by = Some(DEVICE.into());
    let deleted: HashSet<String> = ["G3".to_string()].into();
    let (m, _) = run_with(&l, &r, true, &[], &[], Some(&deleted));
    assert_eq!(m.paper.groups, ["G1", "G2"]);
    assert_eq!(
        m.paper.groups_modified, r.groups_modified,
        "時間與裝置保留原樣"
    );
    // 群組讀不懂：留本機的所屬
    let (m, _) = run_with(&l, &r, true, &[], &[], None);
    assert_eq!(m.paper.groups, ["G1"]);
    // 只在匯入那邊的文獻也拿掉
    let n = prepare_new_paper(&r, Some(&deleted), &[]);
    assert_eq!(n.groups, ["G1", "G2"]);
}

fn material(id: &str, path: &str, sha: Option<&str>, t: &str) -> Material {
    let mut m = base().materials[0].clone();
    m.id = id.into();
    m.path = path.into();
    m.sha256 = sha.map(String::from);
    m.role = "translation".into();
    m.provenance.time = Some(t.into());
    m
}

#[test]
fn materials_union_by_id_and_name_collision_renamed() {
    let mut l = base();
    let mut r = base();
    l.materials.push(material(
        "OV-L",
        "translation-overlay.json",
        None,
        "2026-11-01T00:00:00.000Z",
    ));
    r.materials.push(material(
        "OV-R",
        "translation-overlay.json",
        None,
        "2026-11-01T00:00:00.000Z",
    ));
    let (m, _) = run_with(
        &l,
        &r,
        true,
        &["source.pdf", "translation-overlay.json", "paper.json"],
        &[],
        None,
    );
    let paths: Vec<&str> = m.paper.materials.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "source.pdf",
            "translation-overlay.json",
            "translation-overlay-2.json"
        ]
    );
    assert_eq!(m.files.len(), 1);
    assert_eq!(m.files[0].remote_path, "translation-overlay.json");
    assert_eq!(m.files[0].local_path, "translation-overlay-2.json");
}

#[test]
fn same_material_same_hash_keeps_local_path_and_pages_block_newer_wins() {
    let mut l = base();
    let mut r = base();
    r.materials[0].path = "elsewhere.pdf".into();
    r.materials[0].pages = Some(12);
    r.materials[0].pages_modified = Some("2026-11-03T00:00:00.000Z".into());
    r.materials[0].pages_modified_by = Some(DEVICE.into());
    l.materials[0].pages = Some(10);
    l.materials[0].pages_modified = Some("2026-11-01T00:00:00.000Z".into());
    let (m, _) = run(&l, &r, true);
    assert_eq!(m.paper.materials[0].path, "source.pdf");
    assert_eq!(m.paper.materials[0].pages, Some(12));
    assert!(m.files.is_empty());
}

#[test]
fn material_newer_provenance_wins_and_file_follows() {
    let mut l = base();
    let mut r = base();
    l.materials.push(material(
        "GEN",
        "translation-generated.pdf",
        Some(&"1".repeat(64)),
        "2026-11-01T00:00:00.000Z",
    ));
    r.materials.push(material(
        "GEN",
        "translation-generated.pdf",
        Some(&"2".repeat(64)),
        "2026-11-02T00:00:00.000Z",
    ));
    let (m, _) = run(&l, &r, true);
    assert_eq!(m.paper.materials[1].sha256, Some("2".repeat(64)));
    assert_eq!(m.files.len(), 1);
    assert_eq!(m.files[0].sha256, Some("2".repeat(64)));
}

#[test]
fn material_same_time_different_hash_conflict_and_resolution_stamps_time() {
    let mut l = base();
    let mut r = base();
    l.materials.push(material(
        "GEN",
        "g.pdf",
        Some(&"1".repeat(64)),
        "2026-11-01T00:00:00.000Z",
    ));
    r.materials.push(material(
        "GEN",
        "g.pdf",
        Some(&"2".repeat(64)),
        "2026-11-01T00:00:00.000Z",
    ));
    let (m, ctx) = run(&l, &r, true);
    assert_eq!(ctx.conflicts[0].1.kind, ConflictKind::MaterialFile);
    assert_eq!(m.paper.materials[1].sha256, Some("1".repeat(64)));
    assert_eq!(m.paper.materials[1].provenance.time.as_deref(), Some(NOW));
    // 反向合併：本機的較新，不再衝突
    let (m2, ctx2) = run(&r, &m.paper, true);
    assert!(ctx2.conflicts.is_empty());
    assert_eq!(m2.paper.materials[1].sha256, Some("1".repeat(64)));
}

#[test]
fn missing_local_file_refilled() {
    let l = base();
    let r = base();
    let (m, _) = run(&l, &r, false);
    assert_eq!(m.files.len(), 1);
    assert_eq!(m.files[0].local_path, "source.pdf");
}

#[test]
fn paper_format_newer_skipped_and_same_source_notice() {
    let mut p = pair();
    // 匯入那邊的 paper.json 格式較新
    let alpha = p.a.list_papers()[0].id.clone();
    let folder = p.a.list_papers()[0].folder.clone();
    let path = p.a_root().join("papers").join(&folder).join("paper.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    v["format"] = json!(2);
    std::fs::write(&path, v.to_string()).unwrap();
    p.a.rescan().unwrap();
    // 兩台各加同一份 PDF
    add_paper(&mut p.a, 900, "Same PDF", None);
    add_paper(&mut p.b, 900, "Same PDF", None);
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    assert!(
        preview
            .skipped_papers
            .iter()
            .any(|s| s.paper == alpha && s.reason == "format-too-new")
    );
    assert!(
        preview
            .notices
            .iter()
            .any(|n| n.kind == NoticeKind::SameSource)
    );
    p.b.apply_sync_import(&preview.preview_id, &[]).unwrap();
    assert_eq!(
        p.b.list_papers()
            .iter()
            .filter(|x| x.bib.title.as_deref() == Some("Same PDF"))
            .count(),
        2,
        "兩篇都留"
    );
}

#[test]
fn incoming_only_paper_folder_name_collision() {
    let mut p = pair();
    let (a_id, _) = add_paper(&mut p.a, 910, "Clash", None);
    add_paper(&mut p.b, 911, "Clash", None);
    sync_once(&mut p.a, &mut p.b, p.out.path());
    let folder =
        p.b.list_papers()
            .into_iter()
            .find(|x| x.id == a_id)
            .unwrap()
            .folder;
    assert_eq!(folder, "Clash (2)");
}

#[test]
fn paper_in_both_papers_and_trash_uses_papers() {
    let mut p = pair();
    let alpha = p.b.list_papers()[0].clone();
    // B 的回收區放一份同代號的舊複本
    let src = p.b_root().join("papers").join(&alpha.folder);
    let dst = p.b_root().join(".trash/20260101T000000Z-Old copy");
    std::fs::create_dir_all(&dst).unwrap();
    for e in std::fs::read_dir(&src).unwrap().flatten() {
        std::fs::copy(e.path(), dst.join(e.file_name())).unwrap();
    }
    let before = snapshot(&dst);
    let (paper, tok) = p.a.get_paper(&alpha.id).unwrap();
    let d = paperreader_core::format::BibDraft {
        title: Some("Edited".into()),
        authors: paper.bib.authors.clone(),
        ..Default::default()
    };
    p.a.update_bib(&alpha.id, d, &tok).unwrap();
    sync_once(&mut p.a, &mut p.b, p.out.path());
    assert_eq!(
        p.b.get_paper(&alpha.id).unwrap().0.bib.title.as_deref(),
        Some("Edited")
    );
    assert_eq!(snapshot(&dst), before, "回收區那一份不合併、不移動");
}
