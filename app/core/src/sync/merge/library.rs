//! `library.json` 的合併。

use super::{Change, MergeCtx, cmp_stamp};
use crate::format::LibraryFile;
use crate::groups::{check_deleted_groups, parse_groups};
use crate::store::{ItemKind, NoticeKind};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashSet;

/// `library.json` 合併的結果。
pub struct LibraryMerge {
    pub file: LibraryFile,
    /// 內容真的變了才要寫。
    pub changed: bool,
    /// 合併後已刪除的群組代號；群組讀不懂而沒有合併時是 `None`（這時各篇的群組所屬也不合併）。
    pub deleted: Option<HashSet<String>>,
}

fn arr(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a.clone(),
        _ => Vec::new(),
    }
}

fn id_of(v: &Value) -> Option<&str> {
    v.get("id").and_then(|x| x.as_str())
}

fn str_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
}

pub fn merge_library(
    local: &LibraryFile,
    remote: &LibraryFile,
    ctx: &mut MergeCtx,
) -> LibraryMerge {
    let mut out = local.clone();
    // 其他不認得的欄位：本機的保留；匯入那邊多出的鍵加進來
    for (k, v) in &remote.extra {
        if !out.extra.contains_key(k) {
            out.extra.insert(k.clone(), v.clone());
        }
    }

    let local_ok =
        parse_groups(&local.groups).is_ok() && check_deleted_groups(&local.deleted_groups).is_ok();
    let remote_ok = parse_groups(&remote.groups).is_ok()
        && check_deleted_groups(&remote.deleted_groups).is_ok();
    if !local_ok {
        ctx.notice(
            NoticeKind::LocalUnreadable,
            None,
            Some(crate::local::LIBRARY_FILE),
            None,
        );
    }
    if !remote_ok {
        ctx.notice(
            NoticeKind::IncomingUnreadable,
            None,
            Some(crate::local::LIBRARY_FILE),
            None,
        );
    }
    if !local_ok || !remote_ok {
        let changed = out != *local;
        return LibraryMerge {
            file: out,
            changed,
            deleted: None,
        };
    }

    // deleted_groups：依代號取聯集，同一個代號兩邊都有時留本機那一筆
    let mut deleted_raw = arr(&local.deleted_groups);
    let mut deleted: HashSet<String> = deleted_raw
        .iter()
        .filter_map(id_of)
        .map(String::from)
        .collect();
    for d in arr(&remote.deleted_groups) {
        if let Some(id) = id_of(&d)
            && deleted.insert(id.to_string())
        {
            deleted_raw.push(d.clone());
        }
    }

    // groups：依代號；兩邊都有的依（modified, modified_by）取較新；順序是本機的，再接上匯入那邊多出的
    let mut groups = arr(&local.groups);
    let local_ids: HashSet<String> = groups.iter().filter_map(id_of).map(String::from).collect();
    let mut added_names: Vec<(String, String)> = Vec::new();
    for g in arr(&remote.groups) {
        let Some(id) = id_of(&g) else { continue };
        if !local_ids.contains(id) {
            if !deleted.contains(id) {
                added_names.push((id.to_string(), str_of(&g, "name").unwrap_or("").to_string()));
                ctx.note(Change::Added, ItemKind::Group, None, str_of(&g, "name"), 1);
            }
            groups.push(g.clone());
            continue;
        }
        let i = groups
            .iter()
            .position(|x| id_of(x) == Some(id))
            .expect("id is local");
        let ord = cmp_stamp(
            str_of(&g, "modified"),
            str_of(&g, "modified_by"),
            str_of(&groups[i], "modified"),
            str_of(&groups[i], "modified_by"),
        );
        if ord == Ordering::Greater && groups[i] != g {
            groups[i] = g.clone();
            if !deleted.contains(id) {
                ctx.note(
                    Change::Updated,
                    ItemKind::Group,
                    None,
                    str_of(&g, "name"),
                    1,
                );
            }
        }
    }
    // 刪除勝過改名
    let before = groups.len();
    let removed_names: Vec<String> = groups
        .iter()
        .filter(|g| {
            id_of(g).is_some_and(|id| deleted.contains(id))
                && local_ids.contains(id_of(g).unwrap_or(""))
        })
        .filter_map(|g| str_of(g, "name").map(String::from))
        .collect();
    groups.retain(|g| !id_of(g).is_some_and(|id| deleted.contains(id)));
    let _ = before;
    for n in &removed_names {
        ctx.note(Change::Removed, ItemKind::Group, None, Some(n), 1);
    }
    // 同名不同代號：兩個都留，提醒
    for (id, name) in &added_names {
        let lower = name.to_lowercase();
        if let Some(other) = groups.iter().find(|g| {
            id_of(g) != Some(id) && str_of(g, "name").is_some_and(|n| n.to_lowercase() == lower)
        }) {
            ctx.notice(NoticeKind::SameGroupName, None, None, id_of(other));
        }
    }

    out.groups = Value::Array(groups);
    out.deleted_groups = Value::Array(deleted_raw);
    let changed = out != *local;
    LibraryMerge {
        file: out,
        changed,
        deleted: Some(deleted),
    }
}
