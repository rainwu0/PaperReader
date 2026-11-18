//! 群組的操作。定義放在 `library.json` 的 `groups`，所屬寫在每篇的 `paper.json`。

use super::scan::scan_folders;
use super::{LIBRARY_FILE, LocalLibrary, PAPER_FILE};
use crate::error::{LibError, Result};
use crate::format::{LibraryFile, PaperFile};
use crate::groups::{GroupDef, check_deleted_groups, clean_name, parse_groups};
use crate::ids::{new_id, now_utc};
use crate::jsonio::{VersionToken, read_json, write_json_atomic};
use crate::store::DeleteOutcome;
use serde_json::{Value, json};

/// 寫入群組前讀到的 `library.json`：檔案、定義與刪除記錄的原始值（寫回時沒改到的原樣放回）。
pub(crate) struct GroupsFile {
    pub file: LibraryFile,
    pub defs: Vec<GroupDef>,
    pub raw_groups: Vec<Value>,
    pub raw_deleted: Vec<Value>,
}

impl LocalLibrary {
    fn library_path(&self) -> std::path::PathBuf {
        self.root.join(LIBRARY_FILE)
    }

    /// 交回群組（代號重複的只留第一個）與 `library.json` 的版本記號。每次都從磁碟讀
    /// （使用者可能在應用開著時手改）；唯讀的文獻庫也能列。只需要 `groups` 讀得懂。
    pub fn list_groups(&self) -> Result<(Vec<GroupDef>, VersionToken)> {
        let (lf, token) = read_json::<LibraryFile>(&self.library_path())?;
        let defs = parse_groups(&lf.groups)?;
        let mut seen = std::collections::HashSet::new();
        let defs = defs
            .into_iter()
            .filter(|d| seen.insert(d.id.clone()))
            .collect();
        Ok((defs, token))
    }

    /// 寫入前的共通讀取：重讀 `library.json`、記號不符 → `Conflict`；`groups` 或
    /// `deleted_groups` 讀不懂 → `Unreadable`（不覆寫）。呼叫的一方先檢查唯讀。
    pub(crate) fn read_groups_file(&self, token: &VersionToken) -> Result<GroupsFile> {
        let (file, current) = read_json::<LibraryFile>(&self.library_path())?;
        if &current != token {
            return Err(LibError::Conflict);
        }
        self.parse_groups_file(file)
    }

    /// 同 `read_groups_file`，但不比記號（背景寫入自己讀、自己比）。
    pub(crate) fn parse_groups_file(&self, file: LibraryFile) -> Result<GroupsFile> {
        let defs = parse_groups(&file.groups)?;
        check_deleted_groups(&file.deleted_groups)?;
        let raw_groups = array_of(&file.groups);
        let raw_deleted = array_of(&file.deleted_groups);
        Ok(GroupsFile {
            file,
            defs,
            raw_groups,
            raw_deleted,
        })
    }

    /// 把改過的 `groups`、`deleted_groups` 寫回 `library.json`，並更新記憶體裡的 `library_file()`。
    pub(crate) fn write_groups_file(
        &mut self,
        mut file: LibraryFile,
        groups: Vec<Value>,
        deleted: Vec<Value>,
    ) -> Result<VersionToken> {
        file.groups = Value::Array(groups);
        file.deleted_groups = Value::Array(deleted);
        let token = write_json_atomic(&self.library_path(), &file)?;
        self.library = file;
        Ok(token)
    }

    pub fn create_group(
        &mut self,
        name: &str,
        token: &VersionToken,
    ) -> Result<(GroupDef, VersionToken)> {
        self.require_writable()?;
        let name = clean_name(name)?;
        let mut g = self.read_groups_file(token)?;
        ensure_name_free(&g.defs, &name, None)?;

        let now = now_utc();
        let def = GroupDef {
            id: new_id(),
            name,
            created: Some(now.clone()),
            modified: Some(now.clone()),
            modified_by: Some(self.device_id.clone()),
            extra: Default::default(),
        };
        g.raw_groups.push(json!({
            "id": def.id,
            "name": def.name,
            "created": now,
            "modified": now,
            "modified_by": self.device_id,
        }));
        let new_token = self.write_groups_file(g.file, g.raw_groups, g.raw_deleted)?;
        Ok((def, new_token))
    }

    /// 只更新第一個代號相符的定義的 `name`、`modified`、`modified_by`；不動任何 `paper.json`。
    pub fn rename_group(
        &mut self,
        group_id: &str,
        name: &str,
        token: &VersionToken,
    ) -> Result<VersionToken> {
        self.require_writable()?;
        let name = clean_name(name)?;
        let mut g = self.read_groups_file(token)?;
        let idx = g
            .defs
            .iter()
            .position(|d| d.id == group_id)
            .ok_or_else(|| LibError::NotFound {
                what: format!("group {group_id}"),
            })?;
        ensure_name_free(&g.defs, &name, Some(group_id))?;
        let Value::Object(obj) = &mut g.raw_groups[idx] else {
            unreachable!("parse_groups accepted only objects");
        };
        obj.insert("name".into(), Value::String(name));
        obj.insert("modified".into(), Value::String(now_utc()));
        obj.insert("modified_by".into(), Value::String(self.device_id.clone()));
        self.write_groups_file(g.file, g.raw_groups, g.raw_deleted)
    }

    /// 把一篇加入群組。`token` 是那篇 `paper.json` 的版本記號。已在群組裡時不寫，
    /// 交回與 `token` 相同的記號。只改 `groups`（加在最後、順便去掉重複）與
    /// `groups_modified`、`groups_modified_by`，不動 `bib.modified`。
    pub fn add_to_group(
        &mut self,
        paper_id: &str,
        group_id: &str,
        token: &VersionToken,
    ) -> Result<VersionToken> {
        let located = self.locate_for_write(paper_id, token)?;
        let mut paper = located.paper;
        let (defs, _) = self.list_groups()?;
        if !defs.iter().any(|d| d.id == group_id) {
            return Err(LibError::NotFound {
                what: format!("group {group_id}"),
            });
        }
        if paper.groups.iter().any(|g| g == group_id) {
            return Ok(token.clone());
        }
        paper.groups.push(group_id.to_string());
        let mut seen = std::collections::HashSet::new();
        paper.groups.retain(|g| seen.insert(g.clone()));
        self.write_membership(&located.folder, paper)
    }

    /// 把一篇移出群組。不在群組裡時不寫；不檢查群組是否存在（懸空的代號也拿得掉）。
    pub fn remove_from_group(
        &mut self,
        paper_id: &str,
        group_id: &str,
        token: &VersionToken,
    ) -> Result<VersionToken> {
        let located = self.locate_for_write(paper_id, token)?;
        let mut paper = located.paper;
        if !paper.groups.iter().any(|g| g == group_id) {
            return Ok(token.clone());
        }
        paper.groups.retain(|g| g != group_id);
        dedupe(&mut paper.groups);
        self.write_membership(&located.folder, paper)
    }

    /// 蓋上時間與裝置、寫入 `paper.json`、更新記憶體裡的目錄。
    fn write_membership(&mut self, folder: &str, mut paper: PaperFile) -> Result<VersionToken> {
        paper.groups_modified = Some(now_utc());
        paper.groups_modified_by = Some(self.device_id.clone());
        let path = self.papers_dir().join(folder).join(PAPER_FILE);
        let token = write_json_atomic(&path, &paper)?;
        self.remember_paper(folder, paper)?;
        Ok(token)
    }

    /// 刪除群組。先改所有成員（拿掉這個代號），最後才刪定義、留一筆刪除記錄。`token` 是
    /// `library.json` 的版本記號。某一篇寫不進去（`format` 較新、沒有權限、`Io`、重做仍
    /// `Conflict`）就記進 `skipped`、繼續下一篇；只有空間不足時停下來回報 `NoSpace`
    /// （定義還在、已改的成員就是已經移出；再呼叫一次即可接著做）。
    pub fn delete_group(&mut self, group_id: &str, token: &VersionToken) -> Result<DeleteOutcome> {
        self.require_writable()?;
        let g = self.read_groups_file(token)?;
        if !g.defs.iter().any(|d| d.id == group_id) {
            return Err(LibError::NotFound {
                what: format!("group {group_id}"),
            });
        }

        // 不靠記憶體裡的目錄：重新掃描 papers/，找出帶著這個代號的每一篇
        self.folders = scan_folders(&self.papers_dir(), None)?;
        let members: Vec<String> = self
            .catalog()
            .into_iter()
            .filter(|(_, p)| p.groups.iter().any(|x| x == group_id))
            .map(|(_, p)| p.id.clone())
            .collect();
        let mut skipped = Vec::new();
        let device = self.device_id.clone();
        for id in members {
            let r = self.update_paper_background(&id, |p| {
                if !p.groups.iter().any(|x| x == group_id) {
                    return Ok(false);
                }
                p.groups.retain(|x| x != group_id);
                dedupe(&mut p.groups);
                p.groups_modified = Some(now_utc());
                p.groups_modified_by = Some(device.clone());
                Ok(true)
            });
            match r {
                Ok(_) => {}
                Err(LibError::NoSpace) => return Err(LibError::NoSpace),
                // 這一篇在掃描之後不見了：沒有什麼要改的
                Err(LibError::NotFound { .. }) => {}
                Err(_) => skipped.push(id),
            }
        }

        // 全部成員處理完：再讀一次 library.json 才刪定義（背景寫入：不同就重做一次）
        for _ in 0..2 {
            let (file, current) = read_json::<LibraryFile>(&self.library_path())?;
            let mut g = self.parse_groups_file(file)?;
            g.raw_groups
                .retain(|v| v.get("id").and_then(Value::as_str) != Some(group_id));
            let already = check_deleted_groups(&Value::Array(g.raw_deleted.clone()))?
                .iter()
                .any(|d| d == group_id);
            if !already {
                g.raw_deleted.push(json!({
                    "id": group_id,
                    "deleted": now_utc(),
                    "deleted_by": self.device_id,
                }));
            }
            match std::fs::read(self.library_path()) {
                Ok(bytes) if crate::jsonio::sha256_hex(&bytes) == current.0 => {}
                _ => continue,
            }
            let new_token = self.write_groups_file(g.file, g.raw_groups, g.raw_deleted)?;
            return Ok(DeleteOutcome {
                token: new_token,
                skipped,
            });
        }
        Err(LibError::Conflict)
    }
}

/// 去掉重複的代號（留第一個）。這篇的群組被改時順便做。
fn dedupe(groups: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    groups.retain(|g| seen.insert(g.clone()));
}

fn array_of(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

/// 群組之間不分大小寫不可同名（比對用 `to_lowercase`）。`except`：改名時不和自己（同代號）比。
fn ensure_name_free(defs: &[GroupDef], name: &str, except: Option<&str>) -> Result<()> {
    let lower = name.to_lowercase();
    if defs
        .iter()
        .any(|d| Some(d.id.as_str()) != except && d.name.to_lowercase() == lower)
    {
        return Err(LibError::InvalidInput {
            reason: format!("a group named {name:?} already exists"),
        });
    }
    Ok(())
}
