//! `RemoteTree`：一個照文獻庫結構放好的資料夾（同步包解開後的 `in/`），加上一份「哪些檔在、
//! 哪些檔沒放進來但知道指紋」的清單。合併層只認它，不在乎它是怎麼來的。
//! 讀不懂的照本機讀檔的規則處理：交回 `None` 或記成讀不懂，不讓整份失敗。

use super::merge::trash::Place;
use super::package::{SyncFile, SyncManifest};
use crate::format::{FORMAT_VERSION, PaperFile};
use crate::jsonio::read_json;
use crate::local::is_valid_notes_dir;
use crate::notes::read_paper_id;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct RemoteTree {
    pub root: PathBuf,
    pub manifest: SyncManifest,
    /// 路徑 → 清單裡的位置。
    index: std::collections::HashMap<String, usize>,
}

/// 匯入那邊的一篇文獻。
pub struct RemotePaper {
    pub place: Place,
    /// 在 `papers/` 或 `.trash/` 底下的資料夾名稱。
    pub folder: String,
    pub paper: PaperFile,
}

/// 讀匯入那邊的 `paper.json` 的結果。
pub struct RemotePapers {
    pub papers: BTreeMap<String, RemotePaper>,
    /// 讀不懂或 `format` 較新的：（文獻代號或資料夾的路徑, 原因 `unreadable`、`format-too-new`）。
    pub bad: Vec<(String, String)>,
}

impl RemoteTree {
    pub fn new(root: PathBuf, manifest: SyncManifest) -> RemoteTree {
        let index = manifest
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (f.path.clone(), i))
            .collect();
        RemoteTree {
            root,
            manifest,
            index,
        }
    }

    pub fn entry(&self, rel: &str) -> Option<&SyncFile> {
        self.index.get(rel).map(|&i| &self.manifest.files[i])
    }

    /// 清單列了、而且放進來了。
    pub fn has(&self, rel: &str) -> bool {
        self.entry(rel).is_some_and(|f| f.included)
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// `papers/` 與 `.trash/` 底下的每一份 `paper.json`。同一個代號在兩處時以 `papers/` 的為準；
    /// 回收區裡兩份時取資料夾名稱較大（較晚移除）的那一份。
    pub fn read_papers(&self) -> RemotePapers {
        let mut out = RemotePapers {
            papers: BTreeMap::new(),
            bad: Vec::new(),
        };
        let mut paths: Vec<&str> = self
            .manifest
            .files
            .iter()
            .filter(|f| f.included)
            .map(|f| f.path.as_str())
            .filter(|p| {
                let segs: Vec<&str> = p.split('/').collect();
                segs.len() == 3
                    && (segs[0] == "papers" || segs[0] == ".trash")
                    && segs[2] == "paper.json"
            })
            .collect();
        // papers/ 先、.trash/ 後；回收區依名稱由大到小
        paths.sort_by(|a, b| {
            let ta = a.starts_with(".trash/");
            let tb = b.starts_with(".trash/");
            ta.cmp(&tb)
                .then_with(|| if ta { b.cmp(a) } else { a.cmp(b) })
        });
        for rel in paths {
            let segs: Vec<&str> = rel.split('/').collect();
            let place = if segs[0] == "papers" {
                Place::Papers
            } else {
                Place::Trash
            };
            match read_json::<PaperFile>(&self.path(rel)) {
                Ok((p, _)) => {
                    if p.format > FORMAT_VERSION {
                        out.bad.push((p.id.clone(), "format-too-new".to_string()));
                        continue;
                    }
                    if out.papers.contains_key(&p.id) {
                        continue;
                    }
                    out.papers.insert(
                        p.id.clone(),
                        RemotePaper {
                            place,
                            folder: segs[1].to_string(),
                            paper: p,
                        },
                    );
                }
                Err(_) => out.bad.push((rel.to_string(), "unreadable".to_string())),
            }
        }
        out
    }

    /// 筆記檔：`notes_dir` 底下屬性區有 `paperreader_paper` 的 `.md`。文獻代號 →（檔名, 內容；不是 UTF-8 是 `None`）。
    /// 同一篇有兩份時依檔名第一份算數。
    pub fn read_notes(&self, notes_dir: &str) -> BTreeMap<String, (String, Option<String>)> {
        let mut out = BTreeMap::new();
        if !is_valid_notes_dir(notes_dir) {
            return out;
        }
        let prefix = format!("{notes_dir}/");
        let mut names: Vec<&str> = self
            .manifest
            .files
            .iter()
            .filter(|f| f.included)
            .filter_map(|f| f.path.strip_prefix(&prefix))
            .filter(|n| !n.contains('/') && n.ends_with(".md"))
            .collect();
        names.sort();
        for name in names {
            let Ok(bytes) = fs::read(self.root.join(notes_dir).join(name)) else {
                continue;
            };
            let head = &bytes[..bytes.len().min(crate::notes::HEAD_BYTES)];
            let Some(pid) = crate::notes::read_paper_id_from_bytes(head) else {
                continue;
            };
            let text = String::from_utf8(bytes).ok();
            if let Some(t) = &text
                && read_paper_id(t).as_deref() != Some(pid.as_str())
            {
                continue;
            }
            out.entry(pid).or_insert((name.to_string(), text));
        }
        out
    }

    /// `logs/` 底下的每一份紀錄檔：檔名 → 內容。
    pub fn read_logs(&self) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        for f in self.manifest.files.iter().filter(|f| f.included) {
            if let Some(name) = f.path.strip_prefix("logs/")
                && !name.contains('/')
                && ((name.starts_with("reading-") && name.ends_with(".jsonl"))
                    || crate::changes::is_changes_file(name))
                && let Ok(b) = fs::read(self.root.join(&f.path))
            {
                out.insert(name.to_string(), b);
            }
        }
        out
    }
}

/// `<base>/<folder>/<rel>` 的路徑（`base` 是 `papers` 或 `.trash`）。
pub fn paper_rel(place: Place, folder: &str, rel: &str) -> String {
    format!("{}/{folder}/{rel}", base_of(place))
}

pub fn base_of(place: Place) -> &'static str {
    match place {
        Place::Trash => ".trash",
        _ => "papers",
    }
}

/// 一個路徑是不是在 `dir` 底下的一般檔案。
pub fn is_file(dir: &Path, rel: &str) -> bool {
    dir.join(rel).is_file()
}
