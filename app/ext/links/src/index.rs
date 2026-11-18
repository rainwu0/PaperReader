//! 連結的索引：每篇的比對鍵、每篇的條目與判斷、DOI 與 arXiv 編號的對照、篇名的詞 → 文獻、
//! 條目的詞 → 條目。查的時候才套比對規則與判斷（核心的 `resolve_judgments`）。
//!
//! 索引能重建：檔壞了、版本不同就整個重建。存成 64 份分片，一篇的更新只重寫它所在的那一份。

use crate::matching::{
    By, EntryKeys, PaperKeys, entry_ids, entry_keys, paper_keys, title_rule, words_of,
};
use paperreader_core::format::Bib;
use paperreader_core::jsonio::{VersionToken, write_json_atomic};
use paperreader_core::references::{
    Judgment, KIND_CONFIRM, KIND_MANUAL, KIND_REJECT, RefEntry, ReferencesFile, fold_text,
    resolve_judgments,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// 索引檔的版本；沒有、讀不懂、版本不同就整個重建。
pub const INDEX_VERSION: u32 = 1;
/// 分片的份數。
const SHARDS: u32 = 64;
/// 條目在全域的代號：文獻的格位左移這麼多位，加上條目的位置（一篇最多 2000 條，`MAX_ENTRIES`）。
const ENTRY_BITS: u32 = 11;
/// 條目的詞 → 條目只收至少這麼長、不全是數字的詞（給「庫裡引用這篇的」找候選用）。
const MIN_ENTRY_WORD: usize = 4;

/// 一篇給索引的資料：書目來自目錄，參考文獻清單來自核心的 `read_references`。
pub struct PaperInput {
    pub id: String,
    pub bib: Bib,
    pub references: Option<(ReferencesFile, VersionToken)>,
}

// ---------- 交給畫面的資料 ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LinkTo {
    pub paper: String,
    pub title: String,
    pub by: By,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Candidate {
    pub paper: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rejected {
    pub paper: String,
    pub title: String,
    /// 那一項 `reject` 判斷的代號（「撤銷」刪它）。
    pub judgment: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntryView {
    /// 在 `entries` 裡的位置（加判斷時帶）。
    pub index: usize,
    pub number: Option<u32>,
    pub label: Option<String>,
    pub text: String,
    pub page: u32,
    pub bbox: Option<[f64; 4]>,
    pub link: Option<LinkTo>,
    /// 待確認的候選。
    pub pending: Vec<Candidate>,
    pub rejected: Vec<Rejected>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LostJudgment {
    pub judgment: String,
    pub kind: String,
    pub text: String,
    pub paper: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LinksView {
    /// `references.json` 的版本記號，加刪判斷時帶。
    pub token: String,
    /// 索引在背景整理中（外殼填）。
    pub building: bool,
    pub entries: Vec<EntryView>,
    /// 找不回的判斷。
    pub lost: Vec<LostJudgment>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CitingPaper {
    pub paper: String,
    pub title: String,
    pub year: Option<i32>,
    /// 用哪幾條引的（條目的編號；沒有編號的樣式是 `null`）。
    pub numbers: Vec<Option<u32>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractState {
    pub source_sha256: String,
    pub version: u32,
}

// ---------- 存在記憶體與分片裡的樣子 ----------

/// 一條（只留索引用得到的欄位）。存檔用短的鍵名。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    #[serde(rename = "n", default)]
    number: Option<u32>,
    #[serde(rename = "l", default)]
    label: Option<Box<str>>,
    #[serde(rename = "t")]
    text: Box<str>,
    #[serde(rename = "p")]
    page: u32,
    #[serde(rename = "b", default)]
    bbox: Option<[f32; 4]>,
    #[serde(rename = "d", default)]
    doi: Option<Box<str>>,
    #[serde(rename = "a", default)]
    arxiv: Option<Box<str>>,
    #[serde(rename = "y", default)]
    year: Option<i32>,
}

impl Entry {
    fn from_ref(e: &RefEntry) -> Entry {
        Entry {
            number: e.number,
            label: e.label.as_deref().map(Box::from),
            text: Box::from(e.text.as_str()),
            page: e.page,
            bbox: e.bbox.map(|b| b.map(|v| v as f32)),
            doi: e.doi.as_deref().map(Box::from),
            arxiv: e.arxiv.as_deref().map(Box::from),
            year: e.year,
        }
    }

    /// 給核心的 `resolve_judgments`（只用到原文、DOI、arXiv 編號）。
    fn to_ref(&self) -> RefEntry {
        RefEntry {
            number: self.number,
            label: self.label.as_deref().map(String::from),
            text: self.text.to_string(),
            page: self.page,
            bbox: None,
            doi: self.doi.as_deref().map(String::from),
            arxiv: self.arxiv.as_deref().map(String::from),
            year: self.year,
            extra: Default::default(),
        }
    }

    fn keys(&self) -> EntryKeys {
        entry_keys(
            &self.text,
            self.doi.as_deref(),
            self.arxiv.as_deref(),
            self.year,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredRefs {
    token: String,
    extractor: String,
    version: u32,
    source_sha256: String,
    entries: Vec<Entry>,
    #[serde(default)]
    judgments: Vec<Judgment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    id: String,
    bib: Bib,
    #[serde(default)]
    refs: Option<StoredRefs>,
}

#[derive(Serialize, Deserialize)]
struct ShardFile {
    version: u32,
    papers: Vec<Stored>,
}

/// 判斷套用的結果（只記有判斷的條）。
#[derive(Debug, Clone, Default)]
struct Judged {
    /// 條目的位置 → 連到哪一篇與依據（`manual` 或 `confirm`）。
    link: HashMap<usize, (String, By)>,
    /// 條目的位置 → 被否決的每一篇（文獻代號、判斷代號）。
    rejected: HashMap<usize, Vec<(String, String)>>,
    /// 找不回的判斷（在 `judgments` 裡的位置）。
    lost: Vec<usize>,
}

struct Paper {
    stored: Stored,
    keys: PaperKeys,
    /// 篇名的錨詞（篇名的詞裡最長的一個）：條目要含篇名的每一個詞，所以一定也含它。
    anchor: Option<u32>,
    judged: Judged,
}

/// 文獻之間的連結的索引。
#[derive(Default)]
pub struct LinkIndex {
    slots: Vec<Option<Paper>>,
    free: Vec<u32>,
    slot_of: HashMap<String, u32>,
    words: HashMap<Box<str>, u32>,
    by_doi: HashMap<String, Vec<u32>>,
    by_arxiv: HashMap<String, Vec<u32>>,
    title_anchor: HashMap<u32, Vec<u32>>,
    entry_word: HashMap<u32, Vec<u32>>,
    entry_doi: HashMap<String, Vec<u32>>,
    entry_arxiv: HashMap<String, Vec<u32>>,
    /// 被判斷「連到」的那一篇 → 下判斷的條目。
    judged_target: HashMap<String, Vec<u32>>,
    dirty: HashSet<u32>,
    /// 還沒存過：存的時候每一份分片都寫。
    never_saved: bool,
}

fn gid(slot: u32, idx: usize) -> u32 {
    (slot << ENTRY_BITS) | idx as u32
}

fn split_gid(g: u32) -> (u32, usize) {
    (g >> ENTRY_BITS, (g & ((1 << ENTRY_BITS) - 1)) as usize)
}

/// 文獻代號所在的分片（FNV-1a）。
fn shard_of(id: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in id.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h % SHARDS
}

fn shard_path(dir: &Path, n: u32) -> std::path::PathBuf {
    dir.join(format!("shard-{n:02}.json"))
}

fn push_unique(map: &mut HashMap<String, Vec<u32>>, k: &str, v: u32) {
    let list = map.entry(k.to_string()).or_default();
    if !list.contains(&v) {
        list.push(v);
    }
}

fn drop_from(map: &mut HashMap<String, Vec<u32>>, k: &str, keep: impl Fn(u32) -> bool) {
    if let Some(list) = map.get_mut(k) {
        list.retain(|v| keep(*v));
        if list.is_empty() {
            map.remove(k);
        }
    }
}

/// 條目的詞 → 條目收的詞：至少 `MIN_ENTRY_WORD` 個字、不全是數字。
fn indexable(w: &str) -> bool {
    w.chars().count() >= MIN_ENTRY_WORD && !w.chars().all(|c| c.is_ascii_digit())
}

/// 自動比對的結果。
struct Auto {
    by: By,
    papers: Vec<u32>,
    pending: bool,
}

impl LinkIndex {
    pub fn new() -> Self {
        LinkIndex {
            never_saved: true,
            ..Default::default()
        }
    }

    /// 文獻的篇數。
    pub fn len(&self) -> usize {
        self.slot_of.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slot_of.is_empty()
    }

    pub fn contains(&self, paper_id: &str) -> bool {
        self.slot_of.contains_key(paper_id)
    }

    /// 沒有、讀不懂、版本不同交回 `None`，外殼就整個重建。
    pub fn load(dir: &Path) -> Option<Self> {
        let mut index = LinkIndex::default();
        for n in 0..SHARDS {
            let bytes = std::fs::read(shard_path(dir, n)).ok()?;
            let shard: ShardFile = serde_json::from_slice(&bytes).ok()?;
            if shard.version != INDEX_VERSION {
                return None;
            }
            for s in shard.papers {
                index.insert(s);
            }
        }
        index.dirty.clear();
        index.compact();
        Some(index)
    }

    /// 反向的對照放掉多留的容量（一次加入很多篇之後；`save` 也做）。
    fn compact(&mut self) {
        for list in self.entry_word.values_mut() {
            list.shrink_to_fit();
        }
        for list in self.title_anchor.values_mut() {
            list.shrink_to_fit();
        }
        self.entry_word.shrink_to_fit();
        self.words.shrink_to_fit();
    }

    /// 原子寫入改過的分片（第一次存時全部）。
    pub fn save(&mut self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        if self.never_saved || self.dirty.len() > 1 {
            self.compact();
        }
        let shards: Vec<u32> = if self.never_saved {
            (0..SHARDS).collect()
        } else {
            let mut v: Vec<u32> = self.dirty.iter().copied().collect();
            v.sort_unstable();
            v
        };
        for n in shards {
            let mut papers: Vec<&Stored> = self
                .slots
                .iter()
                .flatten()
                .filter(|p| shard_of(&p.stored.id) == n)
                .map(|p| &p.stored)
                .collect();
            papers.sort_by(|a, b| a.id.cmp(&b.id));
            #[derive(Serialize)]
            struct Out<'a> {
                version: u32,
                papers: Vec<&'a Stored>,
            }
            write_json_atomic(
                &shard_path(dir, n),
                &Out {
                    version: INDEX_VERSION,
                    papers,
                },
            )
            .map_err(|e| std::io::Error::other(format!("{e:?}")))?;
            self.dirty.remove(&n);
        }
        self.never_saved = false;
        Ok(())
    }

    /// 加入或換掉一篇。
    pub fn upsert(&mut self, paper: PaperInput) {
        let refs = paper.references.map(|(file, token)| StoredRefs {
            token: token.0,
            extractor: file.extraction.extractor.clone(),
            version: file.extraction.version,
            source_sha256: file.extraction.source_sha256.clone(),
            entries: file.entries.iter().map(Entry::from_ref).collect(),
            judgments: file.judgments,
        });
        self.remove(&paper.id);
        self.insert(Stored {
            id: paper.id,
            bib: paper.bib,
            refs,
        });
    }

    fn word_id(&mut self, w: &str) -> u32 {
        if let Some(&id) = self.words.get(w) {
            return id;
        }
        let id = self.words.len() as u32;
        self.words.insert(Box::from(w), id);
        id
    }

    fn insert(&mut self, stored: Stored) {
        let slot = self.free.pop().unwrap_or_else(|| {
            self.slots.push(None);
            (self.slots.len() - 1) as u32
        });
        let keys = paper_keys(&stored.bib);
        if let Some(d) = &keys.doi {
            push_unique(&mut self.by_doi, d, slot);
        }
        if let Some(a) = &keys.arxiv {
            push_unique(&mut self.by_arxiv, a, slot);
        }
        let anchor = (keys.title_words.len() >= crate::matching::MIN_TITLE_WORDS)
            .then(|| {
                keys.title_words
                    .iter()
                    .max_by(|a, b| {
                        a.chars()
                            .count()
                            .cmp(&b.chars().count())
                            .then_with(|| b.cmp(a))
                    })
                    .cloned()
            })
            .flatten()
            .map(|w| self.word_id(&w));
        if let Some(w) = anchor {
            let list = self.title_anchor.entry(w).or_default();
            if !list.contains(&slot) {
                list.push(slot);
            }
        }
        let mut judged = Judged::default();
        if let Some(r) = &stored.refs {
            for (i, e) in r.entries.iter().enumerate() {
                let g = gid(slot, i);
                let k = e.keys();
                let mut seen = HashSet::new();
                for w in k.words() {
                    if indexable(&w) && seen.insert(w.clone()) {
                        let id = self.word_id(&w);
                        self.entry_word.entry(id).or_default().push(g);
                    }
                }
                if let Some(d) = &k.doi {
                    push_unique(&mut self.entry_doi, d, g);
                }
                if let Some(a) = &k.arxiv {
                    push_unique(&mut self.entry_arxiv, a, g);
                }
            }
            if !r.judgments.is_empty() {
                let refs: Vec<RefEntry> = r.entries.iter().map(Entry::to_ref).collect();
                let res = resolve_judgments(&refs, &r.judgments);
                for (i, re) in res.entries.iter().enumerate() {
                    if let Some(k) = re.link {
                        let j = &r.judgments[k];
                        let by = if j.kind == KIND_MANUAL {
                            By::Manual
                        } else {
                            By::Confirm
                        };
                        judged.link.insert(i, (j.paper.clone(), by));
                        push_unique(&mut self.judged_target, &j.paper, gid(slot, i));
                    }
                    if !re.rejected.is_empty() {
                        judged.rejected.insert(
                            i,
                            re.rejected
                                .iter()
                                .map(|&k| (r.judgments[k].paper.clone(), r.judgments[k].id.clone()))
                                .collect(),
                        );
                    }
                }
                judged.lost = res.lost;
            }
        }
        self.dirty.insert(shard_of(&stored.id));
        self.slot_of.insert(stored.id.clone(), slot);
        self.slots[slot as usize] = Some(Paper {
            stored,
            keys,
            anchor,
            judged,
        });
    }

    /// 拿掉一篇（移到回收區、永久刪除）。
    pub fn remove(&mut self, paper_id: &str) {
        let Some(slot) = self.slot_of.remove(paper_id) else {
            return;
        };
        let Some(p) = self.slots[slot as usize].take() else {
            return;
        };
        if let Some(d) = &p.keys.doi {
            drop_from(&mut self.by_doi, d, |s| s != slot);
        }
        if let Some(a) = &p.keys.arxiv {
            drop_from(&mut self.by_arxiv, a, |s| s != slot);
        }
        if let Some(w) = p.anchor
            && let Some(list) = self.title_anchor.get_mut(&w)
        {
            list.retain(|s| *s != slot);
        }
        let mine = |g: u32| split_gid(g).0 != slot;
        if let Some(r) = &p.stored.refs {
            let mut words = HashSet::new();
            for e in &r.entries {
                let k = e.keys();
                for w in k.words() {
                    if indexable(&w) {
                        words.insert(w);
                    }
                }
                if let Some(d) = &k.doi {
                    drop_from(&mut self.entry_doi, d, mine);
                }
                if let Some(a) = &k.arxiv {
                    drop_from(&mut self.entry_arxiv, a, mine);
                }
            }
            for w in words {
                if let Some(&id) = self.words.get(w.as_str())
                    && let Some(list) = self.entry_word.get_mut(&id)
                {
                    list.retain(|g| mine(*g));
                }
            }
        }
        for (target, _) in p.judged.link.values() {
            drop_from(&mut self.judged_target, target, mine);
        }
        self.dirty.insert(shard_of(paper_id));
        self.free.push(slot);
    }

    fn paper(&self, slot: u32) -> Option<&Paper> {
        self.slots.get(slot as usize).and_then(Option::as_ref)
    }

    fn title_of(&self, paper_id: &str) -> Option<String> {
        let slot = self.slot_of.get(paper_id)?;
        self.paper(*slot).map(|p| p.keys.title.clone())
    }

    /// 自動比對一條：DOI、arXiv、篇名依序，成立就停。`skip` 是不算候選的文獻（這篇自己、被否決的）。
    fn auto_match(&self, e: &Entry, skip: &[u32]) -> Option<Auto> {
        let (doi, arxiv) = entry_ids(e.doi.as_deref(), e.arxiv.as_deref());
        let pick = |list: Option<&Vec<u32>>| -> Vec<u32> {
            list.map(|l| l.iter().copied().filter(|s| !skip.contains(s)).collect())
                .unwrap_or_default()
        };
        if let Some(d) = &doi {
            let c = pick(self.by_doi.get(d));
            if !c.is_empty() {
                return Some(Auto {
                    by: By::Doi,
                    pending: c.len() > 1,
                    papers: c,
                });
            }
        }
        if let Some(a) = &arxiv {
            let c = pick(self.by_arxiv.get(a));
            if !c.is_empty() {
                return Some(Auto {
                    by: By::Arxiv,
                    pending: c.len() > 1,
                    papers: c,
                });
            }
        }
        let keys = e.keys();
        let mut seen = HashSet::new();
        let mut found: Vec<(u32, bool)> = Vec::new();
        for w in keys.words() {
            if !seen.insert(w.clone()) {
                continue;
            }
            let Some(id) = self.words.get(w.as_str()) else {
                continue;
            };
            for &slot in self.title_anchor.get(id).map(Vec::as_slice).unwrap_or(&[]) {
                if skip.contains(&slot) || found.iter().any(|(s, _)| *s == slot) {
                    continue;
                }
                if let Some(p) = self.paper(slot)
                    && let Some(pending) = title_rule(&p.keys, &keys)
                {
                    found.push((slot, pending));
                }
            }
        }
        if found.is_empty() {
            return None;
        }
        found.sort_unstable();
        let pending = found.len() > 1 || found[0].1;
        Some(Auto {
            by: By::Title,
            papers: found.into_iter().map(|(s, _)| s).collect(),
            pending,
        })
    }

    /// 一條現在的結果：連到哪一篇（文獻代號、依據）、待確認的候選（文獻代號）。
    fn resolve_entry(&self, slot: u32, idx: usize) -> (Option<(String, By)>, Vec<String>) {
        let Some(p) = self.paper(slot) else {
            return (None, Vec::new());
        };
        let Some(e) = p.stored.refs.as_ref().and_then(|r| r.entries.get(idx)) else {
            return (None, Vec::new());
        };
        if let Some((target, by)) = p.judged.link.get(&idx) {
            // 連到的那篇不在目錄（回收區、永久刪除）：當成沒連上，判斷留著
            return if self.slot_of.contains_key(target) {
                (Some((target.clone(), *by)), Vec::new())
            } else {
                (None, Vec::new())
            };
        }
        let mut skip = vec![slot];
        for (paper, _) in p
            .judged
            .rejected
            .get(&idx)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            if let Some(s) = self.slot_of.get(paper) {
                skip.push(*s);
            }
        }
        match self.auto_match(e, &skip) {
            None => (None, Vec::new()),
            Some(a) => {
                let ids: Vec<String> = a
                    .papers
                    .iter()
                    .filter_map(|s| self.paper(*s).map(|q| q.stored.id.clone()))
                    .collect();
                if a.pending {
                    (None, ids)
                } else {
                    (ids.into_iter().next().map(|id| (id, a.by)), Vec::new())
                }
            }
        }
    }

    /// 那篇的每一條、每條連到哪篇；沒抽過是 `None`。
    pub fn links_for(&self, paper_id: &str) -> Option<LinksView> {
        let slot = *self.slot_of.get(paper_id)?;
        let p = self.paper(slot)?;
        let r = p.stored.refs.as_ref()?;
        let title = |id: &str| self.title_of(id).unwrap_or_default();
        let entries = r
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let (link, pending) = self.resolve_entry(slot, i);
                EntryView {
                    index: i,
                    number: e.number,
                    label: e.label.as_deref().map(String::from),
                    text: e.text.to_string(),
                    page: e.page,
                    bbox: e
                        .bbox
                        .map(|b| b.map(|v| (f64::from(v) * 100.0).round() / 100.0)),
                    link: link.map(|(paper, by)| LinkTo {
                        title: title(&paper),
                        paper,
                        by,
                    }),
                    pending: pending
                        .into_iter()
                        .map(|paper| Candidate {
                            title: title(&paper),
                            paper,
                        })
                        .collect(),
                    rejected: p
                        .judged
                        .rejected
                        .get(&i)
                        .map(Vec::as_slice)
                        .unwrap_or(&[])
                        .iter()
                        .filter(|(paper, _)| self.slot_of.contains_key(paper))
                        .map(|(paper, j)| Rejected {
                            paper: paper.clone(),
                            title: title(paper),
                            judgment: j.clone(),
                        })
                        .collect(),
                }
            })
            .collect();
        let lost = p
            .judged
            .lost
            .iter()
            .filter_map(|&k| r.judgments.get(k))
            .filter(|j| j.kind == KIND_CONFIRM || j.kind == KIND_MANUAL || j.kind == KIND_REJECT)
            .map(|j| LostJudgment {
                judgment: j.id.clone(),
                kind: j.kind.clone(),
                text: j.entry_text.clone(),
                paper: j.paper.clone(),
            })
            .collect();
        Some(LinksView {
            token: r.token.clone(),
            building: false,
            entries,
            lost,
        })
    }

    /// 庫裡引用這篇的：哪一篇、用哪幾條引的。先用反向的對照找候選的條，再對每一條跑和 `links_for` 相同的判斷與比對，
    /// 連到這篇的才列。
    pub fn cited_by(&self, paper_id: &str) -> Vec<CitingPaper> {
        let Some(&target) = self.slot_of.get(paper_id) else {
            return Vec::new();
        };
        let Some(t) = self.paper(target) else {
            return Vec::new();
        };
        let mut cands: HashSet<u32> = HashSet::new();
        if let Some(d) = &t.keys.doi {
            cands.extend(self.entry_doi.get(d).into_iter().flatten());
        }
        if let Some(a) = &t.keys.arxiv {
            cands.extend(self.entry_arxiv.get(a).into_iter().flatten());
        }
        cands.extend(self.judged_target.get(paper_id).into_iter().flatten());
        if t.keys.title_words.len() >= crate::matching::MIN_TITLE_WORDS {
            // 候選的條：篇名的詞（收進「條目的詞 → 條目」的那些）裡條目最少的那一份。有一個詞沒有任何條目含它，
            // 就沒有條目含整個篇名。篇名沒有收進去的詞（都很短）時逐條看。
            let indexed: Vec<&String> =
                t.keys.title_words.iter().filter(|w| indexable(w)).collect();
            if indexed.is_empty() {
                for (s, p) in self.slots.iter().enumerate() {
                    if let Some(r) = p.as_ref().and_then(|p| p.stored.refs.as_ref()) {
                        cands.extend((0..r.entries.len()).map(|i| gid(s as u32, i)));
                    }
                }
            } else {
                let mut best: Option<&Vec<u32>> = None;
                let mut missing = false;
                for w in indexed {
                    match self
                        .words
                        .get(w.as_str())
                        .and_then(|id| self.entry_word.get(id))
                    {
                        None => {
                            missing = true;
                            break;
                        }
                        Some(l) => {
                            if best.is_none_or(|b| l.len() < b.len()) {
                                best = Some(l);
                            }
                        }
                    }
                }
                if !missing && let Some(b) = best {
                    cands.extend(b.iter().copied());
                }
            }
        }
        let mut cands: Vec<u32> = cands.into_iter().collect();
        cands.sort_unstable();
        let mut out: Vec<CitingPaper> = Vec::new();
        for g in cands {
            let (slot, idx) = split_gid(g);
            if slot == target {
                continue;
            }
            let (link, _) = self.resolve_entry(slot, idx);
            if link.as_ref().is_none_or(|(id, _)| id != paper_id) {
                continue;
            }
            let Some(p) = self.paper(slot) else { continue };
            let number = p
                .stored
                .refs
                .as_ref()
                .and_then(|r| r.entries.get(idx))
                .and_then(|e| e.number);
            match out.iter_mut().find(|c| c.paper == p.stored.id) {
                Some(c) => c.numbers.push(number),
                None => out.push(CitingPaper {
                    paper: p.stored.id.clone(),
                    title: p.keys.title.clone(),
                    year: p.stored.bib.year,
                    numbers: vec![number],
                }),
            }
        }
        out.sort_by(|a, b| b.year.cmp(&a.year).then_with(|| a.title.cmp(&b.title)));
        out
    }

    /// 那篇的參考文獻清單是從哪一份原文、哪一個版本的規則抽的；沒抽過是 `None`。
    pub fn extract_state(&self, paper_id: &str) -> Option<ExtractState> {
        let p = self.paper(*self.slot_of.get(paper_id)?)?;
        p.stored.refs.as_ref().map(|r| ExtractState {
            source_sha256: r.source_sha256.clone(),
            version: r.version,
        })
    }
}

/// 篇名或條目原文的詞（給測試與外殼的量測用）。
pub fn folded_words(s: &str) -> Vec<String> {
    words_of(&fold_text(s))
}
