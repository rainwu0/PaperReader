//! 合併層。純計算：輸入是本機與匯入兩邊讀好的結構加上合起來的紀錄，
//! 輸出是合併後的內容、衝突、提醒。不碰檔案、不取現在的時間、不產生代號（都由 `MergeCtx` 交進來），
//! 所以預覽與寫入用同一組函式算得出同一份結果。

pub mod ink;
pub mod library;
pub mod logs;
pub mod notes;
pub mod paper;
pub mod references;
pub mod text;
pub mod trash;

use crate::changes::ChangeRecord;
use crate::ids::parse_utc_millis;
use crate::store::{
    ConflictChoice, ConflictKind, ConflictSide, ItemKind, NoticeKind, SyncConflict, SyncItem,
    SyncNotice,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

/// 比（時間, 裝置代號）：時間是 `null` 或格式不對的算最舊；時間相同比裝置代號的位元組順序。
pub fn cmp_stamp(
    a_time: Option<&str>,
    a_dev: Option<&str>,
    b_time: Option<&str>,
    b_dev: Option<&str>,
) -> Ordering {
    let ta = a_time.and_then(parse_utc_millis);
    let tb = b_time.and_then(parse_utc_millis);
    match (ta, tb) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| {
            a_dev
                .unwrap_or("")
                .as_bytes()
                .cmp(b_dev.unwrap_or("").as_bytes())
        }),
    }
}

/// 只比時間（材料的出處沒有裝置欄位）。
pub fn cmp_time(a: Option<&str>, b: Option<&str>) -> Ordering {
    cmp_stamp(a, None, b, None)
}

/// 合併時共用的狀態：衝突的選擇、累積的衝突與提醒、解決衝突要寫的記錄。
pub struct MergeCtx {
    /// 現在的時間（寫入時蓋的時間）。
    pub now: String,
    /// 衝突副本檔名用的時間（`now_compact()` 的格式）。
    pub now_compact: String,
    /// 這台（匯入的）裝置。
    pub device: String,
    /// 匯出那台裝置。
    pub remote_device: String,
    /// 衝突的選擇，以內部的鍵（`conflict_key`）對應；沒有的用預設。
    pub choices: HashMap<String, ConflictChoice>,
    pub conflicts: Vec<(String, SyncConflict)>,
    pub notices: Vec<SyncNotice>,
    /// 解決衝突要附加到這台變更紀錄檔的記錄。
    pub records: Vec<ChangeRecord>,
    /// 產生記錄的代號。
    pub id_gen: Box<dyn FnMut() -> String>,
    /// 會新增、更新、移除的項目的彙總。
    pub summary: Summary,
    /// 合併之前，本機、匯入那邊的紀錄裡有想法記錄的文獻（空的想法沒有節點時，只有那一邊從沒記過想法才算起點）。
    pub local_thoughts: HashSet<String>,
    pub remote_thoughts: HashSet<String>,
}

/// （種類, 文獻）→（篇名, 數目）。
pub type Tally = BTreeMap<(ItemKind, Option<String>), (Option<String>, usize)>;

/// 依（種類, 文獻）彙總的數目。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub added: Tally,
    pub updated: Tally,
    pub removed: Tally,
}

#[derive(Debug, Clone, Copy)]
pub enum Change {
    Added,
    Updated,
    Removed,
}

impl Summary {
    pub fn note(
        &mut self,
        change: Change,
        kind: ItemKind,
        paper: Option<&str>,
        title: Option<&str>,
        n: usize,
    ) {
        if n == 0 {
            return;
        }
        let map = match change {
            Change::Added => &mut self.added,
            Change::Updated => &mut self.updated,
            Change::Removed => &mut self.removed,
        };
        let e = map
            .entry((kind, paper.map(String::from)))
            .or_insert((title.map(String::from), 0));
        e.1 += n;
    }

    pub fn absorb(&mut self, other: Summary) {
        for (map, from) in [
            (&mut self.added, other.added),
            (&mut self.updated, other.updated),
            (&mut self.removed, other.removed),
        ] {
            for (k, (t, n)) in from {
                let e = map.entry(k).or_insert((t, 0));
                e.1 += n;
            }
        }
    }

    /// 交給畫面的清單。
    pub fn items(map: &Tally) -> Vec<SyncItem> {
        map.iter()
            .map(|((kind, paper), (title, count))| SyncItem {
                kind: *kind,
                paper: paper.clone(),
                title: title.clone(),
                count: *count,
            })
            .collect()
    }
}

impl MergeCtx {
    pub fn new(
        now: String,
        now_compact: String,
        device: String,
        remote_device: String,
        choices: HashMap<String, ConflictChoice>,
        id_gen: Box<dyn FnMut() -> String>,
    ) -> MergeCtx {
        MergeCtx {
            now,
            now_compact,
            device,
            remote_device,
            choices,
            conflicts: Vec::new(),
            notices: Vec::new(),
            records: Vec::new(),
            id_gen,
            summary: Summary::default(),
            local_thoughts: HashSet::new(),
            remote_thoughts: HashSet::new(),
        }
    }

    /// 登記一個衝突，交回這次要用的選擇（使用者給的，沒有就是預設）。
    #[allow(clippy::too_many_arguments)]
    pub fn conflict(
        &mut self,
        kind: ConflictKind,
        paper: Option<&str>,
        title: Option<&str>,
        item: Option<&str>,
        local: ConflictSide,
        incoming: ConflictSide,
        options: Vec<ConflictChoice>,
        default: ConflictChoice,
        count: usize,
    ) -> ConflictChoice {
        let key = conflict_key(kind, paper, item);
        let choice = self
            .choices
            .get(&key)
            .copied()
            .filter(|c| options.contains(c))
            .unwrap_or(default);
        self.conflicts.push((
            key,
            SyncConflict {
                id: String::new(),
                kind,
                paper: paper.map(String::from),
                title: title.map(String::from),
                item: item.map(String::from),
                local,
                incoming,
                options,
                default,
                count,
            },
        ));
        choice
    }

    pub fn notice(
        &mut self,
        kind: NoticeKind,
        paper: Option<&str>,
        path: Option<&str>,
        other: Option<&str>,
    ) {
        self.notice_for(kind, paper, path, other, Vec::new());
    }

    /// 同 `notice`，但這則提醒只因為 `conflicts`（衝突的內部鍵，`conflict_key`）照預設的選擇才出現；
    /// 預覽交出去之前換成衝突的代號（`c1` …）。同一則提醒（種類、文獻、路徑、其他都相同）只留一則，鍵併在一起。
    pub fn notice_for(
        &mut self,
        kind: NoticeKind,
        paper: Option<&str>,
        path: Option<&str>,
        other: Option<&str>,
        conflicts: Vec<String>,
    ) {
        self.push_notice(SyncNotice {
            kind,
            paper: paper.map(String::from),
            path: path.map(String::from),
            other: other.map(String::from),
            conflicts,
        });
    }

    /// 加一則提醒；已有同一則（種類、文獻、路徑、其他相同）時併進那一則：衝突的鍵取聯集，其中一則沒有鍵
    /// （和選擇無關、一定出現）時合起來也沒有鍵。
    fn push_notice(&mut self, n: SyncNotice) {
        let same = self.notices.iter_mut().find(|o| {
            o.kind == n.kind && o.paper == n.paper && o.path == n.path && o.other == n.other
        });
        match same {
            None => self.notices.push(n),
            Some(o) if o.conflicts.is_empty() => {}
            Some(o) if n.conflicts.is_empty() => o.conflicts.clear(),
            Some(o) => {
                for k in n.conflicts {
                    if !o.conflicts.contains(&k) {
                        o.conflicts.push(k);
                    }
                }
            }
        }
    }

    /// 一筆這台的新記錄（解決衝突時寫的），時間是 `now`。
    pub fn record(
        &mut self,
        paper: Option<&str>,
        kind: &str,
        data: serde_json::Value,
    ) -> ChangeRecord {
        let rec = ChangeRecord {
            id: (self.id_gen)(),
            t: self.now.clone(),
            device: self.device.clone(),
            paper: paper.map(String::from),
            kind: kind.to_string(),
            data,
            extra: Default::default(),
        };
        self.records.push(rec.clone());
        rec
    }

    /// 把另一個（一篇文獻的）結果併進來。
    pub fn absorb(&mut self, other: MergeCtx) {
        self.conflicts.extend(other.conflicts);
        for n in other.notices {
            self.push_notice(n);
        }
        self.records.extend(other.records);
        self.summary.absorb(other.summary);
    }

    /// 記一項會新增、更新或移除的東西。
    pub fn note(
        &mut self,
        change: Change,
        kind: ItemKind,
        paper: Option<&str>,
        title: Option<&str>,
        n: usize,
    ) {
        self.summary.note(change, kind, paper, title, n);
    }

    /// 一個新的空白結果，選擇與時間相同（一篇一篇算，整篇略過時丟掉）。
    pub fn fork(&self, id_gen: Box<dyn FnMut() -> String>) -> MergeCtx {
        let mut c = MergeCtx::new(
            self.now.clone(),
            self.now_compact.clone(),
            self.device.clone(),
            self.remote_device.clone(),
            self.choices.clone(),
            id_gen,
        );
        c.local_thoughts = self.local_thoughts.clone();
        c.remote_thoughts = self.remote_thoughts.clone();
        c
    }

    /// 撤回一筆彙總（那一項後來沒有寫）。
    pub fn unnote(&mut self, change: Change, kind: ItemKind, paper: Option<&str>) {
        let map = match change {
            Change::Added => &mut self.summary.added,
            Change::Updated => &mut self.summary.updated,
            Change::Removed => &mut self.summary.removed,
        };
        let key = (kind, paper.map(String::from));
        if let Some(e) = map.get_mut(&key) {
            e.1 = e.1.saturating_sub(1);
            if e.1 == 0 {
                map.remove(&key);
            }
        }
    }
}

/// 衝突的內部鍵：種類、文獻、項目。預覽的代號（`c1` …）在最後依順序編，寫入時換回這個鍵。
pub fn conflict_key(kind: ConflictKind, paper: Option<&str>, item: Option<&str>) -> String {
    format!("{kind:?}|{}|{}", paper.unwrap_or(""), item.unwrap_or(""))
}

/// 衝突的一邊（沒有時間與文字的種類）。
pub fn side(
    exists: bool,
    modified: Option<&str>,
    device: Option<&str>,
    text: Option<&str>,
) -> ConflictSide {
    ConflictSide {
        exists,
        modified: modified.map(String::from),
        device: device.map(String::from),
        text: text.map(String::from),
        fields: Vec::new(),
    }
}
