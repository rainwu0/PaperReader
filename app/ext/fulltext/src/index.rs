//! 索引：`<應用資料夾>/ext/fulltext/<文獻庫代號>/index.sqlite`。不進文獻庫、備份與同步；
//! 打不開、版本不同、查詢或寫入時交回資料庫損壞，由外殼刪掉整個資料夾重建。

use crate::query::Query;
use crate::tokenize::tokens;
use crate::units::{UnitKind, units_of};
use paperreader_core::references::{FOLD_VERSION, fold_text_mapped};
use paperreader_core::store::{FileStamp, IndexSource};
use rusqlite::{Connection, ErrorCode, OpenFlags, params};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// 外掛自己的索引規則版本：切詞、單位的規則改了加一。
pub const INDEX_VERSION: u32 = 1;
/// 索引檔的名稱。
pub const INDEX_FILE: &str = "index.sqlite";
/// 鍵同 `PaperDigest.files`（`text`、`notes`、`overlay:<材料>`、`digest:<材料>`）。
pub type Stamps = BTreeMap<String, FileStamp>;

/// 索引檔的 `PRAGMA user_version`：`INDEX_VERSION × 1000 + FOLD_VERSION`。
pub fn user_version() -> i64 {
    i64::from(INDEX_VERSION) * 1000 + i64::from(FOLD_VERSION)
}

/// SQLite 交回資料庫損壞（`SQLITE_CORRUPT`、`SQLITE_NOTADB`）：外殼刪掉資料夾重建。
pub fn is_corrupt(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
    )
}

/// 命中的一個單位（還沒找位置）。
#[derive(Debug, Clone, PartialEq)]
pub struct UnitHit {
    pub paper: String,
    pub key: String,
    pub kind: UnitKind,
    pub material: Option<String>,
    pub page: Option<u32>,
    pub target: Option<String>,
    pub bbox: Option<[f64; 4]>,
}

/// 一篇：命中的單位數與排名最前的幾個候選（依分數）。
#[derive(Debug, Clone, PartialEq)]
pub struct RankedPaper {
    pub paper: String,
    pub total: usize,
    pub candidates: Vec<UnitHit>,
}

/// 這一批的篇（依每篇最好的一處排序），`more` 表示還有下一批。
#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub papers: Vec<RankedPaper>,
    pub more: bool,
}

/// 每篇最多帶幾個候選（找不到位置時換下一處）。
pub const CANDIDATES_PER_PAPER: usize = 10;

pub struct FulltextIndex {
    conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE units(
  id INTEGER PRIMARY KEY,
  paper TEXT NOT NULL,
  key TEXT NOT NULL,
  kind INTEGER NOT NULL,
  material TEXT,
  page INTEGER,
  target TEXT,
  x0 REAL, y0 REAL, x1 REAL, y1 REAL
);
CREATE INDEX units_by_key ON units(paper, key);
CREATE VIRTUAL TABLE fts USING fts5(t, j, tokenize='ascii', content='', contentless_delete=1);
CREATE TABLE stamps(
  paper TEXT NOT NULL,
  key TEXT NOT NULL,
  bytes INTEGER NOT NULL,
  modified INTEGER,
  PRIMARY KEY(paper, key)
) WITHOUT ROWID;
";

const HIT_COLUMNS: &str =
    "u.paper, u.key, u.kind, u.material, u.page, u.target, u.x0, u.y0, u.x1, u.y1";

fn hit_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<UnitHit> {
    let bbox = match (
        r.get::<_, Option<f64>>(6)?,
        r.get::<_, Option<f64>>(7)?,
        r.get::<_, Option<f64>>(8)?,
        r.get::<_, Option<f64>>(9)?,
    ) {
        (Some(a), Some(b), Some(c), Some(d)) => Some([a, b, c, d]),
        _ => None,
    };
    Ok(UnitHit {
        paper: r.get(0)?,
        key: r.get(1)?,
        kind: UnitKind::from_code(r.get(2)?).unwrap_or(UnitKind::Source),
        material: r.get(3)?,
        page: r.get(4)?,
        target: r.get(5)?,
        bbox,
    })
}

impl FulltextIndex {
    fn tune(conn: &Connection) -> rusqlite::Result<()> {
        // 每篇一個交易，3000 篇就是 3000 次提交：WAL 加 NORMAL，提交不必每次都寫到磁碟上
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(())
    }

    /// 開現有的索引。沒有檔、打不開、`user_version` 不同交回 `None`，外殼刪掉資料夾再 `create`。
    /// 載入時不做 `quick_check`：上百 MB 的索引每次開都要整份讀過。
    pub fn open(dir: &Path) -> Option<Self> {
        let path = dir.join(INDEX_FILE);
        if !path.is_file() {
            return None;
        }
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()?;
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .ok()?;
        if v != user_version() {
            return None;
        }
        Self::tune(&conn).ok()?;
        Some(Self { conn })
    }

    /// 建一份新的索引（資料夾要先在；裡面原本的索引檔由外殼先刪掉）。
    pub fn create(dir: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(dir.join(INDEX_FILE))?;
        Self::tune(&conn)?;
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", user_version())?;
        Ok(Self { conn })
    }

    /// 和 `stamps` 表比：新增、變了、不見了的鍵（由小到大）。
    pub fn stale(&self, paper: &str, now: &Stamps) -> Vec<String> {
        let old: Stamps = self
            .conn
            .prepare("SELECT key, bytes, modified FROM stamps WHERE paper = ?1")
            .and_then(|mut st| {
                st.query_map([paper], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        FileStamp {
                            bytes: r.get::<_, i64>(1)? as u64,
                            modified_ms: r.get(2)?,
                        },
                    ))
                })?
                .collect()
            })
            .unwrap_or_default();
        let mut out: Vec<String> = now
            .iter()
            .filter(|(k, s)| old.get(*k) != Some(*s))
            .map(|(k, _)| k.clone())
            .chain(old.keys().filter(|k| !now.contains_key(*k)).cloned())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    fn delete_key(tx: &rusqlite::Transaction<'_>, paper: &str, key: &str) -> rusqlite::Result<()> {
        tx.execute(
            "DELETE FROM fts WHERE rowid IN (SELECT id FROM units WHERE paper = ?1 AND key = ?2)",
            params![paper, key],
        )?;
        tx.execute(
            "DELETE FROM units WHERE paper = ?1 AND key = ?2",
            params![paper, key],
        )?;
        Ok(())
    }

    /// 一篇一個交易：每個鍵先刪那個鍵的單位再加；來源是 `None`（檔不見或讀不懂）時只刪。記下 `now` 裡那個鍵的檔案
    /// 屬性（`now` 沒有那個鍵時刪掉記下的）。
    pub fn put(
        &mut self,
        paper: &str,
        now: &Stamps,
        sources: Vec<(String, Option<IndexSource>)>,
    ) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut add_unit = tx.prepare(
                "INSERT INTO units(paper, key, kind, material, page, target, x0, y0, x1, y1)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            let mut add_fts = tx.prepare("INSERT INTO fts(rowid, t, j) VALUES (?1, ?2, ?3)")?;
            for (key, src) in &sources {
                Self::delete_key(&tx, paper, key)?;
                if let Some(src) = src {
                    for u in units_of(key, src) {
                        let (t, j) = tokens(&fold_text_mapped(&u.text)).columns();
                        if t.is_empty() && j.is_empty() {
                            continue;
                        }
                        let b = u.bbox;
                        add_unit.execute(params![
                            paper,
                            key,
                            u.kind.code(),
                            u.material,
                            u.page,
                            u.target,
                            b.map(|b| b[0]),
                            b.map(|b| b[1]),
                            b.map(|b| b[2]),
                            b.map(|b| b[3]),
                        ])?;
                        add_fts.execute(params![tx.last_insert_rowid(), t, j])?;
                    }
                }
                match now.get(key) {
                    Some(s) => tx.execute(
                        "INSERT INTO stamps(paper, key, bytes, modified) VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(paper, key) DO UPDATE SET bytes = excluded.bytes, modified = excluded.modified",
                        params![paper, key, s.bytes as i64, s.modified_ms],
                    )?,
                    None => tx.execute(
                        "DELETE FROM stamps WHERE paper = ?1 AND key = ?2",
                        params![paper, key],
                    )?,
                };
            }
        }
        tx.commit()
    }

    /// 整篇移除。
    pub fn remove(&mut self, paper: &str) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM fts WHERE rowid IN (SELECT id FROM units WHERE paper = ?1)",
            [paper],
        )?;
        tx.execute("DELETE FROM units WHERE paper = ?1", [paper])?;
        tx.execute("DELETE FROM stamps WHERE paper = ?1", [paper])?;
        tx.commit()
    }

    /// 依文獻分組，每篇以最好的一處排序（bm25），交回第 `offset` 篇起的 `limit` 篇；每篇帶總處數與最多
    /// `CANDIDATES_PER_PAPER` 個候選。`scope` 是 `None` 時不限。
    pub fn search(
        &self,
        q: &Query,
        scope: Option<&[String]>,
        offset: usize,
        limit: usize,
    ) -> rusqlite::Result<Ranked> {
        let scope: Option<HashSet<&str>> = scope.map(|s| s.iter().map(String::as_str).collect());
        let mut st = self.conn.prepare(&format!(
            "SELECT {HIT_COLUMNS} FROM fts JOIN units u ON u.id = fts.rowid
             WHERE fts MATCH ?1 ORDER BY fts.rank"
        ))?;
        let mut order: Vec<String> = Vec::new();
        let mut by_paper: HashMap<String, RankedPaper> = HashMap::new();
        let mut rows = st.query([q.fts()])?;
        while let Some(r) = rows.next()? {
            let paper: String = r.get(0)?;
            if scope.as_ref().is_some_and(|s| !s.contains(paper.as_str())) {
                continue;
            }
            let entry = by_paper.entry(paper.clone()).or_insert_with(|| {
                order.push(paper.clone());
                RankedPaper {
                    paper,
                    total: 0,
                    candidates: Vec::new(),
                }
            });
            entry.total += 1;
            if entry.candidates.len() < CANDIDATES_PER_PAPER {
                entry.candidates.push(hit_from_row(r)?);
            }
        }
        let more = order.len() > offset + limit;
        let papers = order
            .into_iter()
            .skip(offset)
            .take(limit)
            .filter_map(|p| by_paper.remove(&p))
            .collect();
        Ok(Ranked { papers, more })
    }

    /// 一篇的每一處（依分數，最多 `limit` 處）。
    pub fn hits(&self, q: &Query, paper: &str, limit: usize) -> rusqlite::Result<Vec<UnitHit>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {HIT_COLUMNS} FROM fts JOIN units u ON u.id = fts.rowid
             WHERE fts MATCH ?1 AND u.paper = ?2 ORDER BY fts.rank LIMIT ?3"
        ))?;
        st.query_map(params![q.fts(), paper, limit as i64], hit_from_row)?
            .collect()
    }

    /// 已索引的篇數（有記下檔案屬性的篇）。
    pub fn len(&self) -> usize {
        self.conn
            .query_row("SELECT COUNT(DISTINCT paper) FROM stamps", [], |r| {
                r.get::<_, i64>(0)
            })
            .map_or(0, |n| n as usize)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 整個索引檔（含 WAL）的位元組數，給量測用。
    pub fn checkpoint(&self) -> rusqlite::Result<()> {
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
    }
}
