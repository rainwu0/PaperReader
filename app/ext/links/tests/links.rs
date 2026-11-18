//! 外掛「文獻之間的連結」的 Rust 一側：比對的三種規則與順序、判斷優先、索引的
//! 增改刪與重建。3000 篇的量測在 `scale.rs`。

mod common;

use common::*;
use paperreader_core::references::{KIND_CONFIRM, KIND_MANUAL, KIND_REJECT};
use paperreader_ext_links::LinkIndex;
use paperreader_ext_links::matching::{
    arxiv_from_doi, arxiv_from_url, arxiv_year, entry_keys, paper_keys, title_rule,
};

const DETR: &str = "DETRs Beat YOLOs on Real-time Object Detection";

// ---------- 三種比對與順序 ----------

#[test]
fn doi_then_arxiv_then_title_first_rule_that_holds_wins() {
    let mut by_doi = bib("Some Other Title Entirely Here", Some(2024));
    by_doi.doi = Some("10.1109/CVPR.2024.01605".into());
    let mut by_arxiv = bib("Yet Another Unrelated Long Title", Some(2023));
    by_arxiv.url = Some("https://arxiv.org/abs/2304.08069v3".into());
    let by_title = bib(DETR, Some(2024));
    let mut e = entry(
        &format!("Y. Zhao et al. {DETR}. In CVPR, 2024."),
        Some(2024),
    );
    e.doi = Some("10.1109/cvpr.2024.01605".into());
    e.arxiv = Some("2304.08069".into());
    let mut only_arxiv = e.clone();
    only_arxiv.doi = None;
    let mut only_title = e.clone();
    only_title.doi = None;
    only_title.arxiv = None;
    let ix = index_of(vec![
        input("A", by_doi, None),
        input("B", by_arxiv, None),
        input("C", by_title, None),
        input(
            "P",
            bib("Citing Paper", Some(2025)),
            Some(refs(vec![e, only_arxiv, only_title], vec![])),
        ),
    ]);
    assert_eq!(link_of(&ix, "P", 0), Some(("A".into(), "doi".into())));
    assert_eq!(link_of(&ix, "P", 1), Some(("B".into(), "arxiv".into())));
    assert_eq!(link_of(&ix, "P", 2), Some(("C".into(), "title".into())));
}

#[test]
fn title_containment_does_not_link_to_the_shorter_title() {
    let short = bib("A Survey on Deep Learning", Some(2017));
    let e = entry(
        "G. Litjens et al. A Survey on Deep Learning in Medical Image Analysis. Medical Image Analysis, 2017.",
        Some(2017),
    );
    let ix = index_of(vec![
        input("S", short, None),
        input("P", bib("Citing", None), Some(refs(vec![e], vec![]))),
    ]);
    assert_eq!(link_of(&ix, "P", 0), None);
    assert!(pending_of(&ix, "P", 0).is_empty());
    // 篇名後面接標點才算：同一個篇名、後面是句點
    let ok = entry(
        "G. Litjens. A Survey on Deep Learning. arXiv, 2017.",
        Some(2017),
    );
    let ix = index_of(vec![
        input("S", bib("A Survey on Deep Learning", Some(2017)), None),
        input("P", bib("Citing", None), Some(refs(vec![ok], vec![]))),
    ]);
    assert_eq!(link_of(&ix, "P", 0), Some(("S".into(), "title".into())));
}

#[test]
fn title_needs_four_words_and_a_word_boundary_before_it() {
    let three = bib("Attention Is Everything", Some(2017));
    let e = entry(
        "A. Vaswani. Attention Is Everything. In NeurIPS, 2017.",
        Some(2017),
    );
    let glued = bib("Learning Transferable Visual Models", Some(2021));
    let e2 = entry(
        "X. Y. PreLearning Transferable Visual Models. 2021.",
        Some(2021),
    );
    let ix = index_of(vec![
        input("T", three, None),
        input("G", glued, None),
        input("P", bib("Citing", None), Some(refs(vec![e, e2], vec![]))),
    ]);
    assert_eq!(link_of(&ix, "P", 0), None, "篇名少於 4 個詞不連");
    assert_eq!(link_of(&ix, "P", 1), None, "篇名前一個字是字母時不連");
}

#[test]
fn year_must_be_within_one_of_bib_or_arxiv_year() {
    let mut p = bib(DETR, Some(2024));
    p.url = Some("https://arxiv.org/abs/2304.08069".into()); // arXiv 年份 2023
    let entries = vec![
        entry(&format!("Zhao. {DETR}. In CVPR, 2024."), Some(2024)),
        entry(&format!("Zhao. {DETR}. arXiv, 2022."), Some(2022)),
        entry(&format!("Zhao. {DETR}. Somewhere, 2020."), Some(2020)),
    ];
    let ix = index_of(vec![
        input("D", p, None),
        input("P", bib("Citing", None), Some(refs(entries, vec![]))),
    ]);
    assert_eq!(link_of(&ix, "P", 0), Some(("D".into(), "title".into())));
    assert_eq!(
        link_of(&ix, "P", 1),
        Some(("D".into(), "title".into())),
        "arXiv 年份 2023 差 1"
    );
    assert_eq!(link_of(&ix, "P", 2), None);
}

#[test]
fn no_year_needs_six_words_or_first_author_and_is_always_pending() {
    let six = bib("Deep Residual Learning for Image Recognition", Some(2016));
    let five = with_author(
        bib("Batch Normalization Accelerates Deep Training", Some(2015)),
        "Ioffe",
    );
    let five_other = bib("Layer Normalization Speeds Recurrent Training", Some(2016));
    let entries = vec![
        entry(
            "K. He et al. Deep Residual Learning for Image Recognition. CVPR.",
            None,
        ),
        entry(
            "S. Ioffe and C. Szegedy. Batch Normalization Accelerates Deep Training. ICML.",
            None,
        ),
        entry(
            "J. Ba. Layer Normalization Speeds Recurrent Training. arXiv.",
            None,
        ),
    ];
    let ix = index_of(vec![
        input("R", six, None),
        input("B", five, None),
        input("L", five_other, None),
        input("P", bib("Citing", None), Some(refs(entries, vec![]))),
    ]);
    assert_eq!(link_of(&ix, "P", 0), None);
    assert_eq!(pending_of(&ix, "P", 0), vec!["R".to_string()]);
    assert_eq!(
        pending_of(&ix, "P", 1),
        vec!["B".to_string()],
        "第一作者的姓在篇名之前"
    );
    assert!(
        pending_of(&ix, "P", 2).is_empty(),
        "5 個詞、沒有作者：不成立"
    );
}

#[test]
fn two_candidates_are_not_linked_automatically() {
    // 預印本與正式版都在庫裡
    let ix = index_of(vec![
        input("V1", bib(DETR, Some(2023)), None),
        input("V2", bib(DETR, Some(2024)), None),
        input(
            "P",
            bib("Citing", None),
            Some(refs(
                vec![entry(&format!("Zhao. {DETR}. CVPR, 2024."), Some(2024))],
                vec![],
            )),
        ),
    ]);
    assert_eq!(link_of(&ix, "P", 0), None);
    assert_eq!(
        pending_of(&ix, "P", 0),
        vec!["V1".to_string(), "V2".to_string()]
    );
    // 同一個 DOI 加了兩次也一樣
    let mut a = bib("Alpha Beta Gamma Delta", Some(2020));
    a.doi = Some("10.1000/same".into());
    let b = a.clone();
    let mut e = entry("Someone. Something. 2020.", Some(2020));
    e.doi = Some("10.1000/SAME".into());
    let ix = index_of(vec![
        input("A", a, None),
        input("B", b, None),
        input("P", bib("Citing", None), Some(refs(vec![e], vec![]))),
    ]);
    assert_eq!(link_of(&ix, "P", 0), None);
    assert_eq!(
        pending_of(&ix, "P", 0),
        vec!["A".to_string(), "B".to_string()]
    );
}

#[test]
fn never_links_to_itself() {
    let me = bib(DETR, Some(2024));
    let e = entry(&format!("Zhao. {DETR}. CVPR, 2024."), Some(2024));
    let ix = index_of(vec![input("P", me, Some(refs(vec![e], vec![])))]);
    assert_eq!(link_of(&ix, "P", 0), None);
    assert!(pending_of(&ix, "P", 0).is_empty());
}

#[test]
fn arxiv_id_of_a_library_paper_from_url_or_doi_and_of_an_entry_from_corr_or_doi() {
    assert_eq!(
        arxiv_from_url("https://arxiv.org/abs/2304.08069v2").as_deref(),
        Some("2304.08069")
    );
    assert_eq!(
        arxiv_from_url("http://arxiv.org/pdf/2304.08069v1.pdf").as_deref(),
        Some("2304.08069")
    );
    assert_eq!(
        arxiv_from_url("https://arxiv.org/abs/hep-th/9901001v3").as_deref(),
        Some("hep-th/9901001")
    );
    assert_eq!(arxiv_from_url("https://example.org/abs/2304.08069"), None);
    assert_eq!(
        arxiv_from_doi("10.48550/arXiv.2304.08069").as_deref(),
        Some("2304.08069")
    );
    assert_eq!(
        arxiv_from_doi("https://doi.org/10.48550/arxiv.math.AG/0309136").as_deref(),
        Some("math.AG/0309136")
    );
    assert_eq!(arxiv_year("2304.08069"), Some(2023));
    assert_eq!(arxiv_year("hep-th/9901001"), Some(1999));
    assert_eq!(arxiv_year("cs/0112017"), Some(2001));

    let mut by_doi = bib("Some Title With Many Words", Some(2023));
    by_doi.doi = Some("10.48550/arXiv.2304.08069".into());
    assert_eq!(paper_keys(&by_doi).arxiv.as_deref(), Some("2304.08069"));
    // 條目的 DOI 是 arXiv 的 DOI：也當成 arXiv 編號
    let mut e = entry("Zhao. A paper. CoRR abs/2304.08069, 2023.", Some(2023));
    e.doi = Some("10.48550/arxiv.2304.08069".into());
    let mut lib = bib("Different Title Than The Entry", Some(2023));
    lib.url = Some("https://arxiv.org/abs/2304.08069".into());
    let ix = index_of(vec![
        input("X", lib, None),
        input("P", bib("Citing", None), Some(refs(vec![e], vec![]))),
    ]);
    assert_eq!(link_of(&ix, "P", 0), Some(("X".into(), "arxiv".into())));
}

#[test]
fn title_rule_is_a_pure_function() {
    let k = paper_keys(&bib(
        "Mip-NeRF 360: Unbounded Anti-Aliased Neural Radiance Fields",
        Some(2022),
    ));
    let e = entry_keys(
        "J. T. Barron et al. Mip-nerf 360: Unbounded anti-aliased neural radiance fields. In CVPR, 2022.",
        None,
        None,
        Some(2022),
    );
    assert_eq!(title_rule(&k, &e), Some(false));
    let hyphen = entry_keys(
        "Barron. Mip-nerf 360: Unbounded anti-aliased neural ra- diance fields. CVPR, 2022.",
        None,
        None,
        Some(2022),
    );
    assert_eq!(title_rule(&k, &hyphen), Some(false), "斷行的連字號接回");
}

// ---------- 判斷優先 ----------

const T1: &str = "2026-11-17T08:00:00.000Z";
const T2: &str = "2026-11-17T09:00:00.000Z";

#[test]
fn reject_is_not_linked_manual_and_confirm_are() {
    let text = format!("Zhao. {DETR}. CVPR, 2024.");
    let other = "Someone. A totally unrelated reference without matches. 2020.";
    let lib = vec![
        input("D", bib(DETR, Some(2024)), None),
        input("M", bib("Manually Chosen Paper Title", Some(2020)), None),
    ];
    // 否決自動連上的那篇
    let mut papers = lib.clone_inputs();
    papers.push(input(
        "P",
        bib("Citing", None),
        Some(refs(
            vec![entry(&text, Some(2024))],
            vec![judgment("j-1", KIND_REJECT, "D", &text, T1)],
        )),
    ));
    let ix = index_of(papers);
    assert_eq!(link_of(&ix, "P", 0), None);
    let v = ix.links_for("P").unwrap();
    assert_eq!(v.entries[0].rejected.len(), 1);
    assert_eq!(v.entries[0].rejected[0].judgment, "j-1");
    // 指定別篇、是這篇
    let mut papers = lib.clone_inputs();
    papers.push(input(
        "P",
        bib("Citing", None),
        Some(refs(
            vec![entry(&text, Some(2024)), entry(other, Some(2020))],
            vec![
                judgment("j-2", KIND_MANUAL, "M", &text, T1),
                judgment("j-3", KIND_CONFIRM, "D", other, T1),
            ],
        )),
    ));
    let ix = index_of(papers);
    assert_eq!(link_of(&ix, "P", 0), Some(("M".into(), "manual".into())));
    assert_eq!(link_of(&ix, "P", 1), Some(("D".into(), "confirm".into())));
}

#[test]
fn rejecting_a_then_c_keeps_a_rejected() {
    // 兩篇同名：先否決 A（自動改成只剩 C 待確認……這裡只剩 C 一篇時自動連 C），再否決 C：A 不會回來
    let text = format!("Zhao. {DETR}. CVPR, 2024.");
    let papers = vec![
        input("A", bib(DETR, Some(2024)), None),
        input("C", bib(DETR, Some(2024)), None),
        input(
            "P",
            bib("Citing", None),
            Some(refs(
                vec![entry(&text, Some(2024))],
                vec![
                    judgment("j-a", KIND_REJECT, "A", &text, T1),
                    judgment("j-c", KIND_REJECT, "C", &text, T2),
                ],
            )),
        ),
    ];
    let ix = index_of(papers);
    assert_eq!(link_of(&ix, "P", 0), None);
    assert!(pending_of(&ix, "P", 0).is_empty());
}

#[test]
fn link_to_a_paper_not_in_the_catalog_counts_as_unlinked_and_comes_back() {
    let text = format!("Zhao. {DETR}. CVPR, 2024.");
    let mut ix = index_of(vec![
        input("M", bib("Manually Chosen Paper Title", Some(2020)), None),
        input(
            "P",
            bib("Citing", None),
            Some(refs(
                vec![entry(&text, Some(2024))],
                vec![judgment("j-1", KIND_MANUAL, "M", &text, T1)],
            )),
        ),
    ]);
    assert_eq!(link_of(&ix, "P", 0), Some(("M".into(), "manual".into())));
    ix.remove("M"); // 移到回收區
    assert_eq!(link_of(&ix, "P", 0), None);
    assert!(ix.cited_by("M").is_empty());
    ix.upsert(input(
        "M",
        bib("Manually Chosen Paper Title", Some(2020)),
        None,
    )); // 放回
    assert_eq!(link_of(&ix, "P", 0), Some(("M".into(), "manual".into())));
    assert_eq!(ix.cited_by("M").len(), 1);
}

#[test]
fn lost_judgments_are_listed() {
    let ix = index_of(vec![
        input("D", bib(DETR, Some(2024)), None),
        input(
            "P",
            bib("Citing", None),
            Some(refs(
                vec![entry("Some entry that stays. 2020.", Some(2020))],
                vec![judgment(
                    "j-gone",
                    KIND_REJECT,
                    "D",
                    "An entry that was removed by re-extraction",
                    T1,
                )],
            )),
        ),
    ]);
    let v = ix.links_for("P").unwrap();
    assert_eq!(v.lost.len(), 1);
    assert_eq!(v.lost[0].judgment, "j-gone");
    assert_eq!(v.lost[0].kind, "reject");
}

// ---------- cited_by 和 links_for 同一個結論 ----------

#[test]
fn cited_by_agrees_with_links_for() {
    let text = format!("Zhao. {DETR}. CVPR, 2024.");
    let mut doi_entry = entry("Other. Something with a DOI. 2021.", Some(2021));
    doi_entry.doi = Some("10.1000/xyz".into());
    let mut with_doi = bib("Some Paper Found By Its DOI", Some(2021));
    with_doi.doi = Some("10.1000/XYZ".into());
    let ix = index_of(vec![
        input("D", bib(DETR, Some(2024)), None),
        input("X", with_doi, None),
        input(
            "P1",
            bib("First Citing", Some(2025)),
            Some(refs(
                vec![numbered(3, &text, Some(2024)), doi_entry],
                vec![],
            )),
        ),
        input(
            "P2",
            bib("Second Citing", Some(2026)),
            Some(refs(
                vec![numbered(7, &text, Some(2024))],
                vec![judgment("j", KIND_REJECT, "D", &text, T1)],
            )),
        ),
    ]);
    let c = ix.cited_by("D");
    assert_eq!(c.len(), 1, "P2 否決了，不列");
    assert_eq!(c[0].paper, "P1");
    assert_eq!(c[0].numbers, vec![Some(3)]);
    assert_eq!(ix.cited_by("X")[0].paper, "P1");
    // 再加一篇同名：P1 有兩個候選，不自動連、cited_by 也不列；P2 否決了 D，只剩 D2 一個候選，連到 D2
    let mut ix = ix;
    ix.upsert(input("D2", bib(DETR, Some(2024)), None));
    assert!(ix.cited_by("D").is_empty());
    let c2 = ix.cited_by("D2");
    assert_eq!(c2.len(), 1);
    assert_eq!(c2[0].paper, "P2");
    assert_eq!(link_of(&ix, "P2", 0), Some(("D2".into(), "title".into())));
}

// ---------- 索引的增改刪、存與重建 ----------

#[test]
fn upsert_change_and_remove_update_the_links() {
    let text = format!("Zhao. {DETR}. CVPR, 2024.");
    let mut ix = index_of(vec![input(
        "P",
        bib("Citing", None),
        Some(refs(vec![entry(&text, Some(2024))], vec![])),
    )]);
    assert_eq!(link_of(&ix, "P", 0), None);
    ix.upsert(input("D", bib(DETR, Some(2024)), None)); // 新增
    assert_eq!(link_of(&ix, "P", 0), Some(("D".into(), "title".into())));
    ix.upsert(input(
        "D",
        bib("Renamed To Something Else Now", Some(2024)),
        None,
    )); // 改了篇名
    assert_eq!(link_of(&ix, "P", 0), None);
    ix.upsert(input("D", bib(DETR, Some(2024)), None));
    ix.remove("D"); // 移除
    assert_eq!(link_of(&ix, "P", 0), None);
    assert_eq!(ix.len(), 1);
    // 重抽：條目換了
    ix.upsert(input("D", bib(DETR, Some(2024)), None));
    ix.upsert(input(
        "P",
        bib("Citing", None),
        Some(refs(vec![entry("Nothing here. 2020.", Some(2020))], vec![])),
    ));
    assert_eq!(link_of(&ix, "P", 0), None);
    assert!(ix.cited_by("D").is_empty());
    assert_eq!(ix.extract_state("P").unwrap().version, 1);
    assert_eq!(ix.extract_state("D"), None);
}

#[test]
fn save_load_and_rebuild_when_broken_or_other_version() {
    let dir = tempfile::tempdir().unwrap();
    let text = format!("Zhao. {DETR}. CVPR, 2024.");
    let mut ix = index_of(vec![
        input("D", bib(DETR, Some(2024)), None),
        input(
            "P",
            bib("Citing", None),
            Some(refs(vec![numbered(1, &text, Some(2024))], vec![])),
        ),
    ]);
    assert!(LinkIndex::load(dir.path()).is_none(), "沒有索引檔");
    ix.save(dir.path()).unwrap();
    let back = LinkIndex::load(dir.path()).unwrap();
    assert_eq!(back.links_for("P"), ix.links_for("P"));
    assert_eq!(back.cited_by("D"), ix.cited_by("D"));
    // 只改一篇：只重寫它所在的那一份
    let before: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (e.file_name(), e.metadata().unwrap().modified().unwrap())
        })
        .collect();
    std::thread::sleep(std::time::Duration::from_millis(20));
    ix.upsert(input("D", bib(DETR, Some(2023)), None));
    ix.save(dir.path()).unwrap();
    let changed = std::fs::read_dir(dir.path())
        .unwrap()
        .filter(|e| {
            let e = e.as_ref().unwrap();
            let m = e.metadata().unwrap().modified().unwrap();
            before.iter().any(|(n, t)| *n == e.file_name() && *t != m)
        })
        .count();
    assert_eq!(changed, 1);
    // 一份壞了：整個重建
    let shard = std::fs::read_dir(dir.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&shard, b"{ not json").unwrap();
    assert!(LinkIndex::load(dir.path()).is_none());
    // 版本不同：整個重建
    let dir2 = tempfile::tempdir().unwrap();
    let mut fresh = index_of(vec![input("D", bib(DETR, Some(2024)), None)]);
    fresh.save(dir2.path()).unwrap();
    assert!(LinkIndex::load(dir2.path()).is_some());
    let shard = dir2.path().join("shard-00.json");
    let text = std::fs::read_to_string(&shard).unwrap();
    assert!(text.contains("\"version\": 1"));
    std::fs::write(
        &shard,
        text.replacen("\"version\": 1", "\"version\": 99", 1),
    )
    .unwrap();
    assert!(LinkIndex::load(dir2.path()).is_none());
}

trait CloneInputs {
    fn clone_inputs(&self) -> Vec<paperreader_ext_links::PaperInput>;
}

impl CloneInputs for Vec<paperreader_ext_links::PaperInput> {
    fn clone_inputs(&self) -> Vec<paperreader_ext_links::PaperInput> {
        self.iter()
            .map(|p| paperreader_ext_links::PaperInput {
                id: p.id.clone(),
                bib: p.bib.clone(),
                references: p.references.clone(),
            })
            .collect()
    }
}
