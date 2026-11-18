//! 3000 篇的合成文獻庫（時間目標）：重建、存與載入、一篇的更新、`links_for`、
//! `cited_by`、記憶體，以及判斷的套用（核心的 `resolve_judgments`）佔多少。
//!
//! 平常的 `cargo test` 不跑（要一兩分鐘建文獻庫）。量的時候：
//!   cargo test -p paperreader-ext-links --release --test scale -- --ignored --nocapture
//! 篇數與每篇條數可用環境變數 `PR_SCALE_PAPERS`、`PR_SCALE_ENTRIES` 改。
//!
//! 文獻庫是真的（核心的 `LocalLibrary`，寫在暫存資料夾），讀法和外殼的 `ext.rs` 相同：每篇 `get_paper` 加
//! `read_references`，再交給索引。

mod common;

use paperreader_core::format::{Author, BibDraft};
use paperreader_core::local::LocalLibrary;
use paperreader_core::references::{EntryDraft, JudgmentKind, ReferencesDraft, resolve_judgments};
use paperreader_core::store::AddOutcome;
use paperreader_ext_links::{LinkIndex, PaperInput};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// 數著配置了多少位元組的配置器（量索引佔的記憶體）。
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: 照原樣交給系統的配置器
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            LIVE.fetch_add(l.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: 照原樣交給系統的配置器
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        // SAFETY: 照原樣交給系統的配置器
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            LIVE.fetch_add(new, Ordering::Relaxed);
            LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// 可重現的亂數。
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
    /// 偏向小的數（常用詞較常出現）。
    fn skewed(&mut self, n: usize) -> usize {
        let r = (self.next() % 10_000) as f64 / 10_000.0;
        ((r * r) * n as f64) as usize
    }
}

fn word(r: &mut Rng) -> String {
    const SYL: [&str; 24] = [
        "ra", "di", "an", "ce", "ne", "ur", "al", "fi", "el", "ds", "ten", "sor", "trans", "for",
        "mer", "ge", "o", "met", "ric", "lin", "ear", "vi", "sion", "mod",
    ];
    let n = 2 + r.below(3);
    (0..n).map(|_| SYL[r.below(SYL.len())]).collect()
}

struct Lib {
    vocab: Vec<String>,
    names: Vec<String>,
}

impl Lib {
    fn title(&self, r: &mut Rng) -> String {
        let n = 5 + r.below(8);
        let mut w: Vec<String> = (0..n)
            .map(|_| self.vocab[r.skewed(self.vocab.len())].clone())
            .collect();
        if let Some(f) = w.first_mut() {
            let mut c = f.chars();
            *f = c
                .next()
                .map(|x| x.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default();
        }
        w.join(" ")
    }
    fn authors(&self, r: &mut Rng) -> String {
        let n = 1 + r.below(5);
        (0..n)
            .map(|_| {
                format!(
                    "{}. {}",
                    (b'A' + r.below(26) as u8) as char,
                    self.names[r.below(self.names.len())]
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

struct Made {
    id: String,
    title: String,
    year: i32,
    doi: Option<String>,
    arxiv: Option<String>,
}

fn env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

fn read_input(lib: &LocalLibrary, id: &str) -> PaperInput {
    let (paper, _) = lib.get_paper(id).unwrap();
    PaperInput {
        id: id.to_string(),
        bib: paper.bib,
        references: lib.read_references(id).unwrap(),
    }
}

fn stats(times: &mut [Duration]) -> (f64, f64, f64) {
    times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let avg = times.iter().map(|d| ms(*d)).sum::<f64>() / times.len().max(1) as f64;
    let p95 = times
        .get(times.len() * 95 / 100)
        .map(|d| ms(*d))
        .unwrap_or(0.0);
    (avg, p95, times.last().map(|d| ms(*d)).unwrap_or(0.0))
}

#[test]
#[ignore]
fn scale_3000_papers() {
    let papers = env("PR_SCALE_PAPERS", 3000);
    let per = env("PR_SCALE_ENTRIES", 60);
    let mut r = Rng(20261117);
    let synth = Lib {
        vocab: (0..6000).map(|_| word(&mut r)).collect(),
        names: (0..3000)
            .map(|_| {
                let w = word(&mut r);
                let mut c = w.chars();
                c.next()
                    .map(|x| x.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            })
            .collect(),
    };

    // ---- 建文獻庫：先加入每一篇（書目），再寫每篇的參考文獻清單 ----
    let dir = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    let mut lib = LocalLibrary::open(dir.path(), common_device()).unwrap();
    let t = Instant::now();
    let mut made: Vec<Made> = Vec::with_capacity(papers);
    for i in 0..papers {
        let title = synth.title(&mut r);
        let year = 2000 + r.below(27) as i32;
        let doi = (r.below(10) < 4).then(|| format!("10.{}/synth.{i}", 1000 + r.below(9000)));
        let arxiv =
            (r.below(10) < 4).then(|| format!("{:02}{:02}.{:05}", year % 100, 1 + r.below(12), i));
        let pdf = src.path().join(format!("{i}.pdf"));
        let mut bytes = b"%PDF-1.7\n".to_vec();
        bytes.extend_from_slice(format!("synthetic {i}").as_bytes());
        std::fs::write(&pdf, bytes).unwrap();
        let draft = BibDraft {
            title: Some(title.clone()),
            authors: vec![Author {
                family: Some(synth.names[r.below(synth.names.len())].clone()),
                ..Default::default()
            }],
            year: Some(year),
            doi: doi.clone(),
            url: arxiv.as_ref().map(|a| format!("https://arxiv.org/abs/{a}")),
            ..Default::default()
        };
        let id = match lib.add_paper(&pdf, draft, Some(20)).unwrap() {
            AddOutcome::Added(id) => id,
            other => panic!("{other:?}"),
        };
        made.push(Made {
            id,
            title,
            year,
            doi,
            arxiv,
        });
    }
    let added = t.elapsed();
    let t = Instant::now();
    let mut judged = 0;
    for i in 0..papers {
        let mut entries = Vec::with_capacity(per);
        for n in 0..per {
            let cite = r.below(4) == 0;
            let (text, year, doi, arxiv) = if cite {
                let m = &made[r.below(papers)];
                let doi = m.doi.clone().filter(|_| r.below(3) == 0);
                let arxiv = m.arxiv.clone().filter(|_| r.below(3) == 0);
                (
                    format!(
                        "{}. {}. In Proc. of something, {}.",
                        synth.authors(&mut r),
                        m.title,
                        m.year
                    ),
                    m.year,
                    doi,
                    arxiv,
                )
            } else {
                let y = 1990 + r.below(37) as i32;
                (
                    format!(
                        "{}. {}. Journal of {}, {}.",
                        synth.authors(&mut r),
                        synth.title(&mut r),
                        synth.vocab[r.below(100)],
                        y
                    ),
                    y,
                    None,
                    None,
                )
            };
            entries.push(EntryDraft {
                number: Some(n as u32 + 1),
                label: Some(format!("[{}]", n + 1)),
                text,
                page: 15 + r.below(5) as u32,
                bbox: Some([72.0, 100.0 + n as f64, 300.0, 120.0 + n as f64]),
                doi,
                arxiv,
                year: Some(year),
            });
        }
        let (paper, _) = lib.get_paper(&made[i].id).unwrap();
        let m = &paper.materials[0];
        let token = lib
            .write_references(
                &made[i].id,
                ReferencesDraft {
                    extractor: "links".into(),
                    version: 1,
                    source_material: m.id.clone(),
                    source_sha256: m.sha256.clone().unwrap(),
                    entries,
                },
            )
            .unwrap();
        // 每篇兩項判斷：否決第 1 條的某篇、第 2 條指定一篇
        let target = &made[(i + 1) % papers].id;
        let (_, token) = lib
            .add_link_judgment(&made[i].id, 0, JudgmentKind::Reject, target, &token)
            .unwrap();
        lib.add_link_judgment(&made[i].id, 1, JudgmentKind::Manual, target, &token)
            .unwrap();
        judged += 2;
    }
    let written = t.elapsed();
    println!(
        "文獻庫：{papers} 篇、每篇 {per} 條、判斷 {judged} 項；加入 {added:?}、寫參考文獻清單 {written:?}"
    );

    // ---- 目錄摘要 ----
    let t = Instant::now();
    let digests = lib.paper_digests();
    println!(
        "目錄摘要（paper_digests）：{} 篇 {:?}",
        digests.len(),
        t.elapsed()
    );

    // ---- 整個重建（外殼的背景整理：一篇一篇讀、交給索引）與記憶體 ----
    let ids: Vec<String> = made.iter().map(|m| m.id.clone()).collect();
    let before = LIVE.load(Ordering::SeqCst);
    let t = Instant::now();
    let mut ix = LinkIndex::new();
    let mut read_time = Duration::ZERO;
    for id in &ids {
        let tr = Instant::now();
        let input = read_input(&lib, id);
        read_time += tr.elapsed();
        ix.upsert(input);
    }
    let rebuild = t.elapsed();
    let held = LIVE.load(Ordering::SeqCst).saturating_sub(before);
    println!(
        "重建：{rebuild:?}（其中經核心讀每篇 {read_time:?}、建索引 {:?}）；索引佔記憶體 {:.1} MB",
        rebuild - read_time,
        mb(held)
    );

    // ---- 判斷的套用（核心的 resolve_judgments）單獨量 ----
    let docs: Vec<_> = ids
        .iter()
        .map(|id| lib.read_references(id).unwrap().unwrap().0)
        .collect();
    let t = Instant::now();
    for d in &docs {
        let res = resolve_judgments(&d.entries, &d.judgments);
        assert_eq!(res.entries.len(), d.entries.len());
    }
    println!("判斷的套用：{} 篇共 {:?}", docs.len(), t.elapsed());
    // 一篇判斷很多（上限 5000 的一半）時
    let mut big = docs[0].clone();
    let j0 = big.judgments[0].clone();
    for k in 0..2500 {
        let mut j = j0.clone();
        j.id = format!("j-{k:026}");
        j.created = format!("2026-11-17T08:{:02}:{:02}.000Z", (k / 60) % 60, k % 60);
        big.judgments.push(j);
    }
    let t = Instant::now();
    let _ = resolve_judgments(&big.entries, &big.judgments);
    println!(
        "判斷的套用：一篇 {} 項判斷 {:?}",
        big.judgments.len(),
        t.elapsed()
    );
    drop(big);
    drop(docs);

    // ---- 存、載入 ----
    let store = tempfile::tempdir().unwrap();
    let t = Instant::now();
    ix.save(store.path()).unwrap();
    let saved = t.elapsed();
    let size: u64 = std::fs::read_dir(store.path())
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    let compacted = LIVE.load(Ordering::SeqCst).saturating_sub(before);
    let t = Instant::now();
    let loaded = LinkIndex::load(store.path()).unwrap();
    println!(
        "存：{saved:?}（{:.1} MB；存的時候放掉多留的容量，索引佔 {:.1} MB）；載入：{:?}",
        size as f64 / 1024.0 / 1024.0,
        mb(compacted),
        t.elapsed()
    );
    drop(loaded);

    // ---- 查詢 ----
    let sample: Vec<&String> = (0..200).map(|k| &ids[(k * 7919) % ids.len()]).collect();
    let mut times = Vec::new();
    let mut linked = 0;
    for id in &sample {
        let t = Instant::now();
        let v = ix.links_for(id).unwrap();
        times.push(t.elapsed());
        linked += v.entries.iter().filter(|e| e.link.is_some()).count();
    }
    let (avg, p95, max) = stats(&mut times);
    println!(
        "links_for：200 篇，平均 {avg:.2} ms、95% {p95:.2} ms、最長 {max:.2} ms（連上 {linked} 條）"
    );
    let mut times = Vec::new();
    let mut citing = 0;
    for id in &sample {
        let t = Instant::now();
        citing += ix.cited_by(id).len();
        times.push(t.elapsed());
    }
    let (avg, p95, max) = stats(&mut times);
    println!(
        "cited_by：200 篇，平均 {avg:.2} ms、95% {p95:.2} ms、最長 {max:.2} ms（共 {citing} 篇引用）"
    );

    // ---- 一篇的更新：重讀一篇、換掉、存改過的那一份 ----
    let mut times = Vec::new();
    for id in sample.iter().take(50) {
        let t = Instant::now();
        let input = read_input(&lib, id);
        ix.upsert(input);
        ix.save(store.path()).unwrap();
        times.push(t.elapsed());
    }
    let (avg, p95, max) = stats(&mut times);
    println!(
        "一篇的更新（讀、換、存）：50 次，平均 {avg:.2} ms、95% {p95:.2} ms、最長 {max:.2} ms"
    );
}

fn common_device() -> &'static str {
    "01JCK3T1A2B3C4D5E6F7G8H9JK"
}
