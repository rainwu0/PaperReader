#![allow(dead_code)]
//! 外掛測試共用：直接做核心的資料型別（書目、參考文獻清單），不經文獻庫的檔。

use paperreader_core::format::{Author, Bib, Map};
use paperreader_core::jsonio::VersionToken;
use paperreader_core::references::{Extraction, Judgment, RefEntry, ReferencesFile};
use paperreader_ext_links::{LinkIndex, PaperInput};

pub fn bib(title: &str, year: Option<i32>) -> Bib {
    Bib {
        title: Some(title.into()),
        year,
        ..Default::default()
    }
}

pub fn with_author(mut b: Bib, family: &str) -> Bib {
    b.authors = vec![Author {
        family: Some(family.into()),
        ..Default::default()
    }];
    b
}

pub fn entry(text: &str, year: Option<i32>) -> RefEntry {
    RefEntry {
        number: None,
        label: None,
        text: text.into(),
        page: 0,
        bbox: None,
        doi: None,
        arxiv: None,
        year,
        extra: Map::new(),
    }
}

pub fn numbered(n: u32, text: &str, year: Option<i32>) -> RefEntry {
    RefEntry {
        number: Some(n),
        label: Some(format!("[{n}]")),
        ..entry(text, year)
    }
}

pub fn refs(entries: Vec<RefEntry>, judgments: Vec<Judgment>) -> ReferencesFile {
    ReferencesFile {
        format: 1,
        modified: None,
        modified_by: None,
        extraction: Extraction {
            extractor: "links".into(),
            version: 1,
            source_material: "M".into(),
            source_sha256: "a".repeat(64),
            created: "2026-11-17T08:00:00.000Z".into(),
            created_by: "D".into(),
            extra: Map::new(),
        },
        entries,
        judgments,
        extra: Map::new(),
    }
}

/// 一項判斷（找回鍵照核心的第 1 版算）。
pub fn judgment(id: &str, kind: &str, paper: &str, text: &str, at: &str) -> Judgment {
    Judgment {
        id: id.into(),
        kind: kind.into(),
        paper: paper.into(),
        entry_key: paperreader_core::references::entry_key(text, 1).unwrap(),
        key_version: 1,
        entry_text: text.into(),
        entry_doi: None,
        entry_arxiv: None,
        created: at.into(),
        created_by: "D".into(),
        extra: Map::new(),
    }
}

pub fn input(id: &str, b: Bib, r: Option<ReferencesFile>) -> PaperInput {
    PaperInput {
        id: id.into(),
        bib: b,
        references: r.map(|f| (f, VersionToken(format!("tok-{id}")))),
    }
}

/// 一個索引，加入幾篇。
pub fn index_of(papers: Vec<PaperInput>) -> LinkIndex {
    let mut ix = LinkIndex::new();
    for p in papers {
        ix.upsert(p);
    }
    ix
}

/// 那篇第 `i` 條連到哪一篇與依據（`Debug` 的小寫，例如 `title`）。
pub fn link_of(ix: &LinkIndex, paper: &str, i: usize) -> Option<(String, String)> {
    let v = ix.links_for(paper)?;
    v.entries[i]
        .link
        .as_ref()
        .map(|l| (l.paper.clone(), format!("{:?}", l.by).to_lowercase()))
}

pub fn pending_of(ix: &LinkIndex, paper: &str, i: usize) -> Vec<String> {
    let v = ix.links_for(paper).unwrap();
    let mut out: Vec<String> = v.entries[i]
        .pending
        .iter()
        .map(|c| c.paper.clone())
        .collect();
    out.sort();
    out
}
