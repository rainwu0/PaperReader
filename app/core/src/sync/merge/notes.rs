//! 筆記檔的合併。以本機的檔為底，只換掉要換的那幾段與屬性區的兩行時間。

use super::{Change, MergeCtx, cmp_time, side};
use crate::changes::{
    ChangeRecord, HIGHLIGHT_DELETED, HIGHLIGHT_RESTORED, THOUGHTS_WRITTEN, thoughts_hash,
    thoughts_node,
};
use crate::notes::{NotesLabels, ParsedNotes, Stamp, as_conflict_copy, parse, stamp};
use crate::store::{ConflictChoice, ConflictKind, ItemKind, NoticeKind};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

pub struct NotesMerge {
    /// 本機筆記檔的新內容（只在匯入那邊時是要建立的內容）；不必寫是 `None`。
    pub text: Option<String>,
    /// 要寫的衝突副本（已改了屬性區的鍵）：本機合併前那份、匯入那份，需要幾份就幾份。
    pub copies: Vec<NoteCopy>,
}

/// 一份衝突副本：內容與它來自哪一台裝置（檔名帶這台的代號前 6 碼）。
#[derive(Debug, Clone, PartialEq)]
pub struct NoteCopy {
    pub text: String,
    pub device: String,
    /// 只因為這些衝突（內部鍵）選了「兩個都留」才寫；空的表示和選擇無關。
    pub conflicts: Vec<String>,
}

/// 這條劃線的刪除狀態：刪除與放回記錄裡最晚的一筆是刪除時，交回它的時間。
fn deleted_at(records: &[ChangeRecord], pid: &str, hid: &str) -> Option<String> {
    records
        .iter()
        .filter(|r| {
            (r.kind == HIGHLIGHT_DELETED || r.kind == HIGHLIGHT_RESTORED)
                && r.paper.as_deref() == Some(pid)
                && r.data_str("highlight") == Some(hid)
        })
        .max_by(|a, b| a.order_key().cmp(&b.order_key()))
        .filter(|r| r.kind == HIGHLIGHT_DELETED)
        .map(|r| r.t.clone())
}

/// 一條劃線的內容：資料行的 JSON 值（含不認得的鍵）加上短評。
fn content_of(p: &ParsedNotes, hid: &str) -> Option<(Value, String, String)> {
    let data = p.highlight_data(hid)?;
    let h = p.highlights().into_iter().find(|h| h.id == hid)?;
    Some((data, h.comment, h.modified))
}

/// 把一條劃線原文的資料行 `modified` 改成 `now`。
fn restamp_raw(raw: &str, now: &str) -> String {
    let (first, rest) = match raw.find('\n') {
        Some(i) => (&raw[..i], &raw[i..]),
        None => (raw, ""),
    };
    let first_trim = first.trim_end_matches('\r');
    let cr = &first[first_trim.len()..];
    let prefix = "<!-- paperreader:highlight ";
    let Some(json) = first_trim
        .trim_start()
        .strip_prefix(prefix)
        .and_then(|s| s.trim_end().strip_suffix("-->"))
    else {
        return raw.to_string();
    };
    let Ok(mut v) = serde_json::from_str::<Value>(json.trim()) else {
        return raw.to_string();
    };
    v["modified"] = Value::String(now.to_string());
    let j = serde_json::to_string(&v)
        .expect("json serializes")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");
    format!("{prefix}{j} -->{cr}{rest}")
}

/// 想法的節點：記錄、或空的內容當成「起點」、或沒有節點。
#[derive(Debug, Clone, PartialEq)]
enum Node {
    Record(String),
    Origin,
    None,
}

/// `had_records`：合併之前，那一邊的紀錄裡有沒有這篇的想法記錄。空的而沒有節點，只有在那一邊從沒記過
/// 想法時才算起點；記過卻沒有節點，照一般的規則（沒有節點）。
fn node_of(
    records: &[ChangeRecord],
    device: &str,
    pid: &str,
    text: &str,
    had_records: bool,
) -> Node {
    match thoughts_node(records, device, pid, &thoughts_hash(text)) {
        Some(r) => Node::Record(r.id.clone()),
        None if text.is_empty() && !had_records => Node::Origin,
        None => Node::None,
    }
}

/// `a` 是不是 `b` 的上游（從 `b` 沿著 `prev` 往上走得到 `a`）。
fn is_ancestor(prevs: &HashMap<String, Vec<String>>, a: &str, b: &str) -> bool {
    let mut stack = vec![b.to_string()];
    let mut seen = HashSet::new();
    while let Some(x) = stack.pop() {
        if !seen.insert(x.clone()) {
            continue;
        }
        for p in prevs.get(&x).into_iter().flatten() {
            if p == a {
                return true;
            }
            stack.push(p.clone());
        }
    }
    false
}

enum ThoughtsDecision {
    Keep,
    TakeRemote,
    Conflict,
}

fn decide_thoughts(records: &[ChangeRecord], pid: &str, nl: &Node, nr: &Node) -> ThoughtsDecision {
    let prevs: HashMap<String, Vec<String>> = records
        .iter()
        .filter(|r| r.kind == THOUGHTS_WRITTEN && r.paper.as_deref() == Some(pid))
        .map(|r| (r.id.clone(), r.prev()))
        .collect();
    match (nl, nr) {
        (Node::Origin, Node::Record(_)) => ThoughtsDecision::TakeRemote,
        (Node::Record(_), Node::Origin) => ThoughtsDecision::Keep,
        (Node::Record(a), Node::Record(b)) => {
            let down = is_ancestor(&prevs, a, b);
            let up = is_ancestor(&prevs, b, a);
            match (down, up) {
                (true, false) => ThoughtsDecision::TakeRemote,
                (false, true) => ThoughtsDecision::Keep,
                _ => ThoughtsDecision::Conflict,
            }
        }
        _ => ThoughtsDecision::Conflict,
    }
}

/// 為什麼要寫衝突副本：被捨棄的是匯入那份，或是本機合併前的那份。
#[derive(Default)]
struct CopyNeed {
    remote: Need,
    local: Need,
}

/// 一份副本需不需要、為什麼：和選擇無關的原因（`always`），或只因為這些衝突選了「兩個都留」（內部鍵）。
#[derive(Default)]
struct Need {
    always: bool,
    keys: Vec<String>,
}

impl Need {
    fn any(&self) -> bool {
        self.always || !self.keys.is_empty()
    }
    fn because(&mut self, kind: ConflictKind, pid: &str, item: Option<&str>) {
        let k = super::conflict_key(kind, Some(pid), item);
        if !self.keys.contains(&k) {
            self.keys.push(k);
        }
    }
    /// 提醒要記的衝突：有和選擇無關的原因時不記（一定會出現）。
    fn conflicts(&self) -> Vec<String> {
        if self.always {
            Vec::new()
        } else {
            self.keys.clone()
        }
    }
}

/// 合併一篇的筆記檔。`local` 是 `None`：本機沒有筆記檔；`remote` 是 `None`：匯入那邊沒有。
/// 本機的檔讀不了時由呼叫的一方處理（不合併）。
#[allow(clippy::too_many_arguments)]
pub fn merge_notes(
    pid: &str,
    title: Option<&str>,
    local: Option<&str>,
    remote: Option<&str>,
    records: &[ChangeRecord],
    labels: &NotesLabels,
    path: &str,
    ctx: &mut MergeCtx,
) -> NotesMerge {
    let Some(base) = local.or(remote) else {
        return NotesMerge {
            text: None,
            copies: Vec::new(),
        };
    };
    let lp = local.map(parse);
    let rp = remote.map(parse);
    let mut text = base.to_string();
    let mut need = CopyNeed::default();
    let mut edits = 0usize;

    // 匯入那邊讀不懂或不完整的劃線：不合併，另存副本
    if let (Some(_), Some(r)) = (local, &rp)
        && (!r.unreadable().is_empty() || r.highlights().iter().any(|h| !h.complete))
    {
        ctx.notice(NoticeKind::IncomingUnreadable, Some(pid), Some(path), None);
        need.remote.always = true;
    }

    // ---- 劃線 ----
    let complete = |p: &Option<ParsedNotes>| -> Vec<String> {
        p.as_ref()
            .map(|p| {
                p.highlights()
                    .into_iter()
                    .filter(|h| h.complete)
                    .map(|h| h.id)
                    .collect()
            })
            .unwrap_or_default()
    };
    let local_ids = if local.is_some() {
        complete(&lp)
    } else {
        Vec::new()
    };
    let remote_ids = complete(&rp);
    let local_incomplete: HashSet<String> = lp
        .as_ref()
        .map(|p| {
            p.highlights()
                .into_iter()
                .filter(|h| !h.complete)
                .map(|h| h.id)
                .collect()
        })
        .unwrap_or_default();
    // 只在匯入那邊時，基底就是匯入的檔：它的劃線算「只在匯入那邊」、已經在檔裡
    let only_remote_file = local.is_none();
    let mut order: Vec<String> = local_ids.clone();
    for id in &remote_ids {
        if !order.contains(id) {
            order.push(id.clone());
        }
    }
    for hid in order {
        if local_incomplete.contains(&hid) {
            continue;
        }
        let in_local = local_ids.contains(&hid);
        let in_remote = remote_ids.contains(&hid);
        let lc = lp
            .as_ref()
            .and_then(|p| content_of(p, &hid))
            .filter(|_| in_local);
        let rc = rp
            .as_ref()
            .and_then(|p| content_of(p, &hid))
            .filter(|_| in_remote);
        let rraw = rp.as_ref().and_then(|p| p.highlight_raw(&hid));
        // 目前文字裡有沒有這一條（只在匯入那邊的檔時，匯入的劃線已經在裡面）
        let present = in_local || (only_remote_file && in_remote);
        let mut keep_content_rules = true;
        if let Some(t) = deleted_at(records, pid, &hid) {
            let after = |c: &Option<(Value, String, String)>| {
                c.as_ref()
                    .is_some_and(|(_, _, m)| cmp_time(Some(m), Some(&t)) == Ordering::Greater)
            };
            let (l_after, r_after) = (after(&lc), after(&rc));
            let both = in_local && in_remote;
            let edited_local = if both {
                match (l_after, r_after) {
                    (false, false) => None,
                    (true, true) => Some(None), // 兩邊都在刪除之後改過：直接留下
                    (true, false) => Some(Some(true)),
                    (false, true) => Some(Some(false)),
                }
            } else if in_local {
                if l_after { Some(Some(true)) } else { None }
            } else if r_after {
                Some(Some(false))
            } else {
                None
            };
            match edited_local {
                None => {
                    // 拿掉（在本機就拿掉；在匯入那邊就不加）
                    if present && remove(&mut text, &hid) {
                        edits += 1;
                        ctx.note(Change::Removed, ItemKind::Highlight, Some(pid), title, 1);
                    }
                    keep_content_rules = false;
                }
                Some(None) => {}
                Some(Some(local_edited)) => {
                    let (lt, rt) = (
                        lc.as_ref().map(|c| c.1.as_str()),
                        rc.as_ref().map(|c| c.1.as_str()),
                    );
                    let choice = ctx.conflict(
                        ConflictKind::HighlightDeletedButEdited,
                        Some(pid),
                        title,
                        Some(&hid),
                        side(in_local, lc.as_ref().map(|c| c.2.as_str()), None, lt),
                        side(in_remote, rc.as_ref().map(|c| c.2.as_str()), None, rt),
                        vec![
                            ConflictChoice::Local,
                            ConflictChoice::Incoming,
                            ConflictChoice::Both,
                        ],
                        ConflictChoice::Both,
                        1,
                    );
                    keep_content_rules = false;
                    match choice {
                        ConflictChoice::Incoming => {
                            ctx.record(
                                Some(pid),
                                HIGHLIGHT_RESTORED,
                                serde_json::json!({ "highlight": hid }),
                            );
                            // 留下改過的那一份，資料行的 modified 改成現在
                            let edited_raw = if local_edited {
                                lp.as_ref().and_then(|p| p.highlight_raw(&hid))
                            } else {
                                rraw.clone()
                            };
                            if let Some(raw) = edited_raw {
                                let raw = restamp_raw(&raw, &ctx.now);
                                if present {
                                    replace(&mut text, &hid, &raw);
                                } else {
                                    text = parse(&text).insert_highlight_raw(&raw, labels);
                                    ctx.note(
                                        Change::Added,
                                        ItemKind::Highlight,
                                        Some(pid),
                                        title,
                                        1,
                                    );
                                }
                                edits += 1;
                            }
                        }
                        _ => {
                            ctx.record(
                                Some(pid),
                                HIGHLIGHT_DELETED,
                                serde_json::json!({ "highlight": hid }),
                            );
                            if present && remove(&mut text, &hid) {
                                edits += 1;
                                ctx.note(Change::Removed, ItemKind::Highlight, Some(pid), title, 1);
                            }
                            if choice == ConflictChoice::Both {
                                let n = if local_edited && local.is_some() {
                                    &mut need.local
                                } else {
                                    &mut need.remote
                                };
                                n.because(ConflictKind::HighlightDeletedButEdited, pid, Some(&hid));
                            }
                        }
                    }
                }
            }
        }
        if !keep_content_rules || only_remote_file {
            continue;
        }
        // 兩邊的內容
        match (&lc, &rc) {
            (None, Some(_)) => {
                if let Some(raw) = &rraw {
                    text = parse(&text).insert_highlight_raw(raw, labels);
                    edits += 1;
                    ctx.note(Change::Added, ItemKind::Highlight, Some(pid), title, 1);
                }
            }
            (Some((ld, lcm, lm)), Some((rd, rcm, rm))) => {
                if ld == rd && lcm == rcm {
                    continue;
                }
                match cmp_time(Some(rm), Some(lm)) {
                    Ordering::Greater => {
                        if let Some(raw) = &rraw {
                            replace(&mut text, &hid, raw);
                            edits += 1;
                            ctx.note(Change::Updated, ItemKind::Highlight, Some(pid), title, 1);
                        }
                    }
                    Ordering::Less => {}
                    Ordering::Equal => {
                        let choice = ctx.conflict(
                            ConflictKind::HighlightEdited,
                            Some(pid),
                            title,
                            Some(&hid),
                            side(true, Some(lm), None, Some(lcm)),
                            side(true, Some(rm), None, Some(rcm)),
                            vec![
                                ConflictChoice::Local,
                                ConflictChoice::Incoming,
                                ConflictChoice::Both,
                            ],
                            ConflictChoice::Both,
                            1,
                        );
                        let chosen = if choice == ConflictChoice::Incoming {
                            rraw.clone()
                        } else {
                            parse(&text).highlight_raw(&hid)
                        };
                        if let Some(raw) = chosen {
                            replace(&mut text, &hid, &restamp_raw(&raw, &ctx.now));
                            edits += 1;
                            ctx.note(Change::Updated, ItemKind::Highlight, Some(pid), title, 1);
                        }
                        if choice == ConflictChoice::Both {
                            need.remote
                                .because(ConflictKind::HighlightEdited, pid, Some(&hid));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // ---- 整篇的想法 ----
    if let (Some(l), Some(r)) = (&lp, &rp) {
        let lt = l.thoughts();
        let rt = r.thoughts();
        if lt != rt {
            let nl = node_of(
                records,
                &ctx.device,
                pid,
                &lt,
                ctx.local_thoughts.contains(pid),
            );
            let nr = node_of(
                records,
                &ctx.remote_device,
                pid,
                &rt,
                ctx.remote_thoughts.contains(pid),
            );
            let complete = l.thoughts_complete();
            let decision = if complete {
                decide_thoughts(records, pid, &nl, &nr)
            } else {
                ThoughtsDecision::Conflict
            };
            match decision {
                ThoughtsDecision::Keep => {}
                ThoughtsDecision::TakeRemote => {
                    if let Ok(t) = parse(&text).replace_thoughts(&rt, labels) {
                        text = t;
                        edits += 1;
                        ctx.note(Change::Updated, ItemKind::Thoughts, Some(pid), title, 1);
                    }
                }
                ThoughtsDecision::Conflict => {
                    let options = if complete {
                        vec![
                            ConflictChoice::Local,
                            ConflictChoice::Incoming,
                            ConflictChoice::Both,
                        ]
                    } else {
                        vec![ConflictChoice::Local, ConflictChoice::Both]
                    };
                    // 兩邊的時間：那一邊內容的節點那一筆 thoughts-written 的時間（沒有節點是 null）
                    let time_of = |n: &Node| match n {
                        Node::Record(id) => {
                            records.iter().find(|r| &r.id == id).map(|r| r.t.clone())
                        }
                        _ => None,
                    };
                    let (lt_time, rt_time) = (time_of(&nl), time_of(&nr));
                    let choice = ctx.conflict(
                        ConflictKind::Thoughts,
                        Some(pid),
                        title,
                        None,
                        side(
                            true,
                            lt_time.as_deref(),
                            Some(&ctx.device.clone()),
                            Some(&lt),
                        ),
                        side(
                            true,
                            rt_time.as_deref(),
                            Some(&ctx.remote_device.clone()),
                            Some(&rt),
                        ),
                        options,
                        ConflictChoice::Both,
                        1,
                    );
                    let chosen = if choice == ConflictChoice::Incoming {
                        &rt
                    } else {
                        &lt
                    };
                    let mut prev = Vec::new();
                    for n in [&nl, &nr] {
                        if let Node::Record(id) = n {
                            prev.push(id.clone());
                        }
                    }
                    ctx.record(
                        Some(pid),
                        THOUGHTS_WRITTEN,
                        serde_json::json!({ "sha256": thoughts_hash(chosen), "prev": prev }),
                    );
                    if choice == ConflictChoice::Incoming
                        && let Ok(t) = parse(&text).replace_thoughts(&rt, labels)
                    {
                        text = t;
                        edits += 1;
                        ctx.note(Change::Updated, ItemKind::Thoughts, Some(pid), title, 1);
                    }
                    if choice == ConflictChoice::Both {
                        need.remote.because(ConflictKind::Thoughts, pid, None);
                    }
                }
            }
        }
        // 標記以外的文字
        if l.outside_text() != r.outside_text() {
            ctx.notice(NoticeKind::NotesOutsideText, Some(pid), Some(path), None);
            need.remote.always = true;
        }
    }

    let new_text = if edits > 0 {
        Some(stamp(
            &text,
            &Stamp {
                time: ctx.now.clone(),
                device: ctx.device.clone(),
            },
        ))
    } else if local.is_none() {
        Some(text)
    } else {
        None
    };
    if local.is_none() {
        ctx.note(Change::Added, ItemKind::NotesFile, Some(pid), title, 1);
    }
    let mut copies = Vec::new();
    if need.local.any()
        && let Some(l) = local
    {
        copies.push(NoteCopy {
            text: as_conflict_copy(l),
            device: ctx.device.clone(),
            conflicts: need.local.conflicts(),
        });
    }
    if need.remote.any()
        && let Some(r) = remote
    {
        copies.push(NoteCopy {
            text: as_conflict_copy(r),
            device: ctx.remote_device.clone(),
            conflicts: need.remote.conflicts(),
        });
    }
    NotesMerge {
        text: new_text,
        copies,
    }
}

fn remove(text: &mut String, hid: &str) -> bool {
    match parse(text).remove_highlight(hid) {
        Ok(t) => {
            *text = t;
            true
        }
        Err(_) => false,
    }
}

fn replace(text: &mut String, hid: &str, raw: &str) {
    if let Ok(t) = parse(text).replace_highlight_raw(hid, raw) {
        *text = t;
    }
}
