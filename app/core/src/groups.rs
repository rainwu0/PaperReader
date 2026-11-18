//! 群組的定義：解讀 `library.json` 的 `groups`、名稱的規則。純計算，不碰檔案。

use crate::error::{LibError, Result};
use crate::format::Map;
use serde::Serialize;
use serde_json::Value;

/// 群組名稱最長的字元數（以 Unicode 字元計）。
pub const MAX_NAME_CHARS: usize = 100;

/// 給畫面看的群組定義。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupDef {
    pub id: String,
    pub name: String,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub modified_by: Option<String>,
    /// 不認得的欄位，原樣保留。
    #[serde(flatten)]
    pub extra: Map,
}

fn unreadable(reason: impl Into<String>) -> LibError {
    LibError::Unreadable {
        path: crate::local::LIBRARY_FILE.to_string(),
        reason: reason.into(),
    }
}

/// 值是字串、`null` 或缺少：交回字串或 `None`；其他型別是讀不懂。
fn opt_string(obj: &Map, key: &str, what: &str) -> Result<Option<String>> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(unreadable(format!("{what}.{key} is not a string"))),
    }
}

fn require_id(obj: &Map, what: &str) -> Result<String> {
    match obj.get("id") {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(unreadable(format!("{what}.id is not a non-empty string"))),
    }
}

/// 缺少或 `null` 當作空陣列；其他不是陣列的值讀不懂。
fn as_items<'a>(value: &'a Value, what: &str) -> Result<&'a [Value]> {
    match value {
        Value::Null => Ok(&[]),
        Value::Array(a) => Ok(a),
        _ => Err(unreadable(format!("{what} is not an array"))),
    }
}

/// 把 `library.json` 的 `groups` 解讀成定義，依檔案裡的順序、**包含**代號重複的（呼叫的一方決定怎麼處理重複）。
/// 讀不懂交回 `Unreadable`。
pub fn parse_groups(value: &Value) -> Result<Vec<GroupDef>> {
    let mut out = Vec::new();
    for (i, item) in as_items(value, "groups")?.iter().enumerate() {
        let what = format!("groups[{i}]");
        let Value::Object(obj) = item else {
            return Err(unreadable(format!("{what} is not an object")));
        };
        let id = require_id(obj, &what)?;
        let name = opt_string(obj, "name", &what)?.unwrap_or_default();
        let created = opt_string(obj, "created", &what)?;
        let modified = opt_string(obj, "modified", &what)?;
        let modified_by = opt_string(obj, "modified_by", &what)?;
        let mut extra = obj.clone();
        for k in ["id", "name", "created", "modified", "modified_by"] {
            extra.remove(k);
        }
        out.push(GroupDef {
            id,
            name,
            created,
            modified,
            modified_by,
            extra,
        });
    }
    Ok(out)
}

/// `deleted_groups` 的解讀，只用來確認它讀得懂；交回被刪掉的代號。
pub fn check_deleted_groups(value: &Value) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for (i, item) in as_items(value, "deleted_groups")?.iter().enumerate() {
        let what = format!("deleted_groups[{i}]");
        let Value::Object(obj) = item else {
            return Err(unreadable(format!("{what} is not an object")));
        };
        ids.push(require_id(obj, &what)?);
        opt_string(obj, "deleted", &what)?;
        opt_string(obj, "deleted_by", &what)?;
    }
    Ok(ids)
}

/// 名稱規則：交回去掉前後空白之後的名稱，或 `InvalidInput`。
/// 1 到 100 個字元（以 Unicode 字元計），不含控制字元。
pub fn clean_name(name: &str) -> Result<String> {
    let t = name.trim();
    let n = t.chars().count();
    if n == 0 || n > MAX_NAME_CHARS {
        return Err(LibError::InvalidInput {
            reason: format!("group name must be 1 to {MAX_NAME_CHARS} characters"),
        });
    }
    if t.chars().any(char::is_control) {
        return Err(LibError::InvalidInput {
            reason: "group name must not contain control characters".to_string(),
        });
    }
    Ok(t.to_string())
}
