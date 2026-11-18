//! 3000 篇的合成文獻庫（時間目標）：整個重建、一次查詢（一批 10 篇含前後文、
//! 快取是空的）、一篇的更新、比一次目錄摘要的時間、索引檔的大小；100 個查詢逐筆比「每一筆 FTS5 命中都找得到位置」。
//!
//! 平常的 `cargo test` 不跑。量的時候先把公開組每篇逐頁的文字抽成 `<資料夾>/<篇>.json`（字串陣列，照 `pageText`
//! 加 `wellFormed`），再：
//!   $env:PR_FULLTEXT_PAGES = "<資料夾>"
//!   cargo test -p paperreader-ext-fulltext --release --test scale -- --ignored --nocapture
//! 篇數可用 `PR_SCALE_PAPERS` 改（預設 3000）。論文的文字不進版本庫，所以這裡只讀環境變數指的資料夾。
//!
//! 文獻庫是真的（核心的 `LocalLibrary`，寫在暫存資料夾），每篇輪流用那幾篇的文字；讀法和外殼的 `ext.rs` 相同：
//! `paper_digests` 的 `files` → `stale` → 逐鍵 `read_for_index` → `put`；查詢時 `search` → 逐篇 `read_for_index`
//! → `units_of` → `locate`。

use paperreader_core::format::BibDraft;
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{AddOutcome, IndexRead};
use paperreader_core::text::TextDraft;
use paperreader_ext_fulltext::{FulltextIndex, Query, UnitHit, locate, parse_query, units_of};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn dir_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok()?.metadata().ok())
        .map(|m| m.len())
        .sum()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// 從語料挑 100 個查詢：單字、字首、不加引號的兩個詞、加引號的片語、帶連字號的詞。
fn queries(texts: &[Vec<String>], r: &mut Rng) -> Vec<String> {
    let mut out = Vec::new();
    while out.len() < 100 {
        let page = &texts[r.below(texts.len())];
        let p = &page[r.below(page.len())];
        let words: Vec<&str> = p
            .split_whitespace()
            .filter(|w| w.chars().count() >= 3)
            .collect();
        if words.len() < 3 {
            continue;
        }
        let i = r.below(words.len() - 2);
        let q = match out.len() % 5 {
            0 => words[i].to_string(),
            1 => words[i].chars().take(4).collect(),
            2 => format!("{} {}", words[i], words[i + 2]),
            3 => format!("\"{} {}\"", words[i], words[i + 1]),
            _ => match words.iter().find(|w| w.contains('-')) {
                Some(w) => (*w).to_string(),
                None => continue,
            },
        };
        if parse_query(&q).is_some() {
            out.push(q);
        }
    }
    out
}

/// 照外殼的做法找一處的位置：讀那份來源、對回單位、`locate`。
fn place(lib: &LocalLibrary, q: &Query, c: &UnitHit) -> bool {
    let IndexRead::Read(_, src) = lib.read_for_index(&c.paper, &c.key, None).unwrap() else {
        return false;
    };
    units_of(&c.key, &src)
        .into_iter()
        .find(|u| u.same_place(c.kind, c.page, c.target.as_deref()))
        .and_then(|u| locate(q, &u.text))
        .is_some()
}

#[test]
#[ignore = "量測：要一兩分鐘，另外用 release 跑"]
fn three_thousand_papers() {
    let Ok(pages_dir) = std::env::var("PR_FULLTEXT_PAGES") else {
        eprintln!("沒有設 PR_FULLTEXT_PAGES，略過");
        return;
    };
    let papers: usize = std::env::var("PR_SCALE_PAPERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);
    let mut texts: Vec<Vec<String>> = std::fs::read_dir(&pages_dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.ok()?.path();
            (p.extension()? == "json")
                .then(|| serde_json::from_slice(&std::fs::read(&p).ok()?).ok())?
        })
        .collect();
    texts.sort_by_key(|t: &Vec<String>| t.len());
    assert!(!texts.is_empty(), "{pages_dir} 裡沒有 .json");

    // ---- 建文獻庫：每篇一份假的原文（頁數同那篇）加抽出文字 ----
    let dir = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    let mut lib = LocalLibrary::open(dir.path(), "01JCK3T1A2B3C4D5E6F7G8H9JK").unwrap();
    let t = Instant::now();
    let mut ids = Vec::with_capacity(papers);
    let (mut total_pages, mut total_chars) = (0usize, 0usize);
    for i in 0..papers {
        let pages = &texts[i % texts.len()];
        let pdf = src.path().join(format!("{i}.pdf"));
        std::fs::write(&pdf, format!("%PDF-1.7\nsynthetic {i}")).unwrap();
        let draft = BibDraft {
            title: Some(format!("Synthetic paper {i}")),
            year: Some(2024),
            ..Default::default()
        };
        let AddOutcome::Added(id) = lib
            .add_paper(&pdf, draft, Some(pages.len() as u32))
            .unwrap()
        else {
            panic!("加不進去");
        };
        let (p, _) = lib.get_paper(&id).unwrap();
        let m = &p.materials[0];
        lib.write_text(
            &id,
            TextDraft {
                extractor: "pdfjs-text".into(),
                version: 1,
                source_material: m.id.clone(),
                source_sha256: m.sha256.clone().unwrap(),
                pages: pages.clone(),
                truncated: false,
            },
        )
        .unwrap();
        total_pages += pages.len();
        total_chars += pages.iter().map(|p| p.chars().count()).sum::<usize>();
        ids.push(id);
    }
    println!(
        "建文獻庫：{papers} 篇、{total_pages} 頁、{total_chars} 字，{:.1} 秒",
        t.elapsed().as_secs_f64()
    );

    // ---- 比一次目錄摘要（背景比對的第一步） ----
    let t = Instant::now();
    let digests = lib.paper_digests();
    let digest_time = t.elapsed();
    let t = Instant::now();
    let _ = lib.paper_digests();
    println!(
        "目錄摘要：{:.0} ms（第二次 {:.0} ms）",
        ms(digest_time),
        ms(t.elapsed())
    );

    // ---- 整個重建 ----
    let ix_dir = tempfile::tempdir().unwrap();
    let mut ix = FulltextIndex::create(ix_dir.path()).unwrap();
    let (mut read_t, mut put_t) = (Duration::ZERO, Duration::ZERO);
    let t = Instant::now();
    for d in &digests {
        let want = ix.stale(&d.id, &d.files);
        let r0 = Instant::now();
        let sources = want
            .into_iter()
            .map(|k| {
                let s = match lib.read_for_index(&d.id, &k, None).unwrap() {
                    IndexRead::Read(_, s) => Some(*s),
                    _ => None,
                };
                (k, s)
            })
            .collect();
        read_t += r0.elapsed();
        let p0 = Instant::now();
        ix.put(&d.id, &d.files, sources).unwrap();
        put_t += p0.elapsed();
    }
    let rebuild = t.elapsed();
    ix.checkpoint().unwrap();
    println!(
        "整個重建：{:.1} 秒（經核心讀檔 {:.1} 秒、寫索引 {:.1} 秒）；索引 {:.1} MB（{} 篇）",
        rebuild.as_secs_f64(),
        read_t.as_secs_f64(),
        put_t.as_secs_f64(),
        dir_bytes(ix_dir.path()) as f64 / 1_048_576.0,
        ix.len()
    );
    assert_eq!(ix.len(), papers);
    drop(ix);
    let t = Instant::now();
    let mut ix = FulltextIndex::open(ix_dir.path()).expect("重開");
    println!("重開索引：{:.1} ms", ms(t.elapsed()));

    // ---- 一次查詢：一批 10 篇含前後文（每篇最多 3 處），快取是空的 ----
    let mut r = Rng(42);
    let qs = queries(&texts, &mut r);
    let mut times = Vec::new();
    let fixed = ["transformer", "detection", "the", "zzzznotfound"];
    for qs in qs
        .iter()
        .map(String::as_str)
        .take(20)
        .chain(fixed.iter().copied())
    {
        let q = parse_query(qs).unwrap();
        let t = Instant::now();
        let ranked = ix.search(&q, None, 0, 10).unwrap();
        let fts = t.elapsed();
        for p in &ranked.papers {
            let mut shown = 0;
            for c in &p.candidates {
                if shown == 3 {
                    break;
                }
                if place(&lib, &q, c) {
                    shown += 1;
                }
            }
        }
        let all = t.elapsed();
        times.push(all);
        println!(
            "  查詢 {qs:?}：{} 篇（第一批 {}），FTS5 {:.1} ms、含讀檔與前後文 {:.1} ms",
            ranked.papers.len(),
            ranked.papers.len(),
            ms(fts),
            ms(all)
        );
    }
    times.sort();
    println!(
        "查詢（{} 個）：中位數 {:.1} ms、最長 {:.1} ms",
        times.len(),
        ms(times[times.len() / 2]),
        ms(*times.last().unwrap())
    );

    // ---- 一篇的更新：改一頁、比、讀、寫 ----
    let mut upd = Vec::new();
    for k in 0..20 {
        let id = &ids[k * (papers / 20)];
        let (p, _) = lib.get_paper(id).unwrap();
        let m = &p.materials[0];
        let mut pages = texts[(k * (papers / 20)) % texts.len()].clone();
        pages[0] = format!("updated page number {k} kumquat");
        lib.write_text(
            id,
            TextDraft {
                extractor: "pdfjs-text".into(),
                version: 2,
                source_material: m.id.clone(),
                source_sha256: m.sha256.clone().unwrap(),
                pages,
                truncated: false,
            },
        )
        .unwrap();
        let files = lib
            .paper_digests()
            .into_iter()
            .find(|d| &d.id == id)
            .unwrap()
            .files;
        let t = Instant::now();
        let want = ix.stale(id, &files);
        let sources = want
            .into_iter()
            .map(|k| match lib.read_for_index(id, &k, None).unwrap() {
                IndexRead::Read(_, s) => (k, Some(*s)),
                _ => (k, None),
            })
            .collect();
        ix.put(id, &files, sources).unwrap();
        upd.push(t.elapsed());
    }
    upd.sort();
    println!(
        "一篇的更新（20 篇）：中位數 {:.1} ms、最長 {:.1} ms",
        ms(upd[upd.len() / 2]),
        ms(*upd.last().unwrap())
    );
    assert_eq!(
        ix.search(&parse_query("kumquat").unwrap(), None, 0, 100)
            .unwrap()
            .papers
            .len(),
        20
    );

    // ---- 100 個查詢：每一筆 FTS5 命中都找得到位置（候選每篇最多 10 處；同一份文字的同一頁只算一次） ----
    let base: HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i % texts.len()))
        .collect();
    let updated: std::collections::HashSet<&str> =
        (0..20).map(|k| ids[k * (papers / 20)].as_str()).collect();
    let (mut checked, mut hits_total) = (0usize, 0usize);
    let t = Instant::now();
    for qs in &qs {
        let q = parse_query(qs).unwrap();
        let ranked = ix.search(&q, None, 0, papers).unwrap();
        let mut seen: HashMap<(usize, Option<u32>), bool> = HashMap::new();
        for p in &ranked.papers {
            hits_total += p.total;
            for c in &p.candidates {
                let ok = if updated.contains(c.paper.as_str()) {
                    place(&lib, &q, c)
                } else {
                    *seen
                        .entry((base[c.paper.as_str()], c.page))
                        .or_insert_with(|| place(&lib, &q, c))
                };
                assert!(ok, "查詢 {qs:?}：{} 第 {:?} 頁找不到位置", c.paper, c.page);
                checked += 1;
            }
        }
    }
    println!(
        "100 個查詢：FTS5 命中 {hits_total} 處，逐筆比了 {checked} 處候選，全部找得到位置（{:.1} 秒）",
        t.elapsed().as_secs_f64()
    );
}
