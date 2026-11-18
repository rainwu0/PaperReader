//! `paper.json` 與材料的合併。

use super::{Change, MergeCtx, cmp_stamp, cmp_time, side};
use crate::changes::{ChangeRecord, MATERIAL_REMOVED, MATERIAL_RESTORED};
use crate::digest::is_digest;
use crate::format::{Bib, Material, PaperFile};
use crate::naming::{free_file_name, last_segment, split_extension};
use crate::overlay::OVERLAY_FORM;
use crate::store::{
    ConflictChoice, ConflictField, ConflictKind, ItemKind, MaterialStatus, NoticeKind,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// 要從匯入那邊拿過來的一個材料檔。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileFromRemote {
    pub material: String,
    /// 在匯入那篇資料夾裡的路徑（材料的 `path`）。
    pub remote_path: String,
    /// 放到本機那篇資料夾裡的路徑。
    pub local_path: String,
    pub sha256: Option<String>,
    pub bytes: Option<u64>,
}

/// 兩邊都有的譯文疊加檔：內容另外依 4.7 合併。
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayPair {
    pub material: String,
    pub local_path: String,
    pub remote_path: String,
}

pub struct PaperMerge {
    pub paper: PaperFile,
    pub changed: bool,
    pub files: Vec<FileFromRemote>,
    pub overlays: Vec<OverlayPair>,
    /// 兩邊都有的重點筆記：內容另外逐點合併。
    pub digests: Vec<OverlayPair>,
}

/// 本機那篇資料夾的狀況。
pub struct LocalFolder<'a> {
    /// 資料夾裡現有的項目名稱（比對時不分大小寫）。
    pub names: &'a [String],
    /// 這項材料的檔的狀況：在而且大小對（`Ok`）、不見了（`Missing`）、在但大小與 `paper.json` 記的不符
    /// （`SizeMismatch`，在應用以外被改過）、路徑不合法（`InvalidPath`）。
    pub file_state: &'a dyn Fn(&Material) -> MaterialStatus,
}

/// 照加入譯本的做法：名稱被占用（不分大小寫，`taken` 是小寫的名稱）時在副檔名之前加 `-2`、`-3` …（`naming.rs`）。
pub fn free_name(taken: &HashSet<String>, name: &str) -> String {
    let (stem, ext) = split_extension(name);
    free_file_name(taken, stem, ext)
}

/// 拿掉已刪除群組的代號，時間與裝置保留原樣。
fn strip_groups(p: &mut PaperFile, deleted: &HashSet<String>) {
    p.groups.retain(|g| !deleted.contains(g));
}

/// 只在匯入那邊的文獻：拿掉合併後已刪除群組的代號、合併後已刪除的材料。
pub fn prepare_new_paper(
    remote: &PaperFile,
    deleted_groups: Option<&HashSet<String>>,
    records: &[ChangeRecord],
) -> PaperFile {
    let mut p = remote.clone();
    if let Some(d) = deleted_groups {
        strip_groups(&mut p, d);
    }
    let removed = removed_materials(records, &p.id);
    p.materials.retain(|m| !removed.contains(&m.id));
    p
}

/// 合併後的紀錄裡，這篇最晚的一筆是 `material-removed` 的材料（同一項材料的移除與放回取較新的一筆，
/// 為準）。
fn removed_materials(records: &[ChangeRecord], paper: &str) -> HashSet<String> {
    let mut latest: HashMap<&str, &ChangeRecord> = HashMap::new();
    for r in records.iter().filter(|r| {
        (r.kind == MATERIAL_REMOVED || r.kind == MATERIAL_RESTORED)
            && r.paper.as_deref() == Some(paper)
    }) {
        let Some(m) = r.data_str("material") else {
            continue;
        };
        let newer = latest.get(m).is_none_or(|o| r.order_key() > o.order_key());
        if newer {
            latest.insert(m, r);
        }
    }
    latest
        .into_iter()
        .filter(|(_, r)| r.kind == MATERIAL_REMOVED)
        .map(|(m, _)| m.to_string())
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn merge_paper(
    local: &PaperFile,
    folder: &LocalFolder,
    remote: &PaperFile,
    deleted_groups: Option<&HashSet<String>>,
    records: &[ChangeRecord],
    title: Option<&str>,
    ctx: &mut MergeCtx,
) -> PaperMerge {
    let pid = local.id.as_str();
    let mut out = local.clone();

    // bib：整塊依（modified, modified_by）取較新，然後較新那份的 year、doi、url 空著時用另一份的值補上
    // （`cleared` 記下的不補；自動補書目不改書目的時間）。時間與裝置都相同時先兩邊互相補上空欄，
    // 補完仍不同才是衝突
    // 較舊那份 `cleared` 記下、較新那份仍空著的欄也併進來、不補（`merged_with_older`）
    let lb = local.bib.merged_with_older(&remote.bib);
    let rb = remote.bib.merged_with_older(&local.bib);
    match cmp_stamp(
        remote.bib.modified.as_deref(),
        remote.bib.modified_by.as_deref(),
        local.bib.modified.as_deref(),
        local.bib.modified_by.as_deref(),
    ) {
        Ordering::Greater => {
            if rb != local.bib {
                out.bib = rb;
                ctx.note(Change::Updated, ItemKind::Bib, Some(pid), title, 1);
            }
        }
        Ordering::Less => {
            if lb != local.bib {
                out.bib = lb;
                ctx.note(Change::Updated, ItemKind::Bib, Some(pid), title, 1);
            }
        }
        Ordering::Equal if lb == rb => {
            if lb != local.bib {
                out.bib = lb;
                ctx.note(Change::Updated, ItemKind::Bib, Some(pid), title, 1);
            }
        }
        Ordering::Equal => {
            // 兩邊帶上互相補過空欄之後仍不同的那幾欄，預覽看得出差在哪
            let (lf, rf) = differing_fields(&lb, &rb);
            let mut ls = side(
                true,
                local.bib.modified.as_deref(),
                local.bib.modified_by.as_deref(),
                local.bib.title.as_deref(),
            );
            ls.fields = lf;
            let mut rs = side(
                true,
                remote.bib.modified.as_deref(),
                remote.bib.modified_by.as_deref(),
                remote.bib.title.as_deref(),
            );
            rs.fields = rf;
            let choice = ctx.conflict(
                ConflictKind::Bib,
                Some(pid),
                title,
                None,
                ls,
                rs,
                vec![ConflictChoice::Local, ConflictChoice::Incoming],
                ConflictChoice::Local,
                1,
            );
            if choice == ConflictChoice::Incoming {
                out.bib = rb;
                ctx.note(Change::Updated, ItemKind::Bib, Some(pid), title, 1);
            } else {
                out.bib = lb;
            }
            out.bib.modified = Some(ctx.now.clone());
            out.bib.modified_by = Some(ctx.device.clone());
        }
    }

    // short_name
    if out.short_name.is_none() {
        out.short_name = remote.short_name.clone();
    }
    // aliases：聯集
    for a in &remote.aliases {
        if !out.aliases.contains(a) {
            out.aliases.push(a.clone());
        }
    }
    // added：取較早的
    out.added = match (&local.added, &remote.added) {
        (Some(a), Some(b)) => {
            if cmp_time(Some(b), Some(a)) == Ordering::Less {
                Some(b.clone())
            } else {
                Some(a.clone())
            }
        }
        (a, b) => a.clone().or_else(|| b.clone()),
    };

    // 群組所屬：整塊取較新；群組讀不懂時留本機的
    if let Some(deleted) = deleted_groups {
        if cmp_stamp(
            remote.groups_modified.as_deref(),
            remote.groups_modified_by.as_deref(),
            local.groups_modified.as_deref(),
            local.groups_modified_by.as_deref(),
        ) == Ordering::Greater
            && (remote.groups != local.groups
                || remote.groups_modified != local.groups_modified
                || remote.groups_modified_by != local.groups_modified_by)
        {
            out.groups = remote.groups.clone();
            out.groups_modified = remote.groups_modified.clone();
            out.groups_modified_by = remote.groups_modified_by.clone();
            if remote.groups != local.groups {
                ctx.note(Change::Updated, ItemKind::Groups, Some(pid), title, 1);
            }
        }
        strip_groups(&mut out, deleted);
    }

    // 其他不認得的欄位：本機的保留；匯入那邊多出的鍵加進來
    for (k, v) in &remote.extra {
        if !out.extra.contains_key(k) {
            out.extra.insert(k.clone(), v.clone());
        }
    }

    // 材料
    let mut taken: HashSet<String> = folder.names.iter().map(|n| n.to_lowercase()).collect();
    for m in &out.materials {
        taken.insert(last_segment(&m.path).to_lowercase());
    }
    let mut files = Vec::new();
    let mut overlays = Vec::new();
    let mut digests = Vec::new();
    for rm in &remote.materials {
        let Some(i) = out.materials.iter().position(|m| m.id == rm.id) else {
            // 只在匯入那邊：加進來，接在本機清單後面
            let name = free_name(&taken, last_segment(&rm.path));
            taken.insert(name.to_lowercase());
            let mut m = rm.clone();
            m.path = name.clone();
            files.push(FileFromRemote {
                material: rm.id.clone(),
                remote_path: rm.path.clone(),
                local_path: name,
                sha256: rm.sha256.clone(),
                bytes: rm.bytes,
            });
            out.materials.push(m);
            ctx.note(Change::Added, ItemKind::Material, Some(pid), title, 1);
            continue;
        };
        let lm = out.materials[i].clone();
        let state = (folder.file_state)(&lm);
        let local_ok = state == MaterialStatus::Ok;
        if is_digest(&lm) && is_digest(rm) {
            // 重點筆記：可以在筆記工具裡改，不比指紋與大小；內容逐點合併
            take_pages(&mut out.materials[i], rm, pid, title, ctx);
            // 「已校對」兩邊有一邊是就算是
            if rm.provenance.proofread == Some(true) && lm.provenance.proofread != Some(true) {
                out.materials[i].provenance.proofread = Some(true);
            }
            match state {
                MaterialStatus::Missing => {
                    files.push(FileFromRemote {
                        material: rm.id.clone(),
                        remote_path: rm.path.clone(),
                        local_path: lm.path.clone(),
                        sha256: None,
                        bytes: None,
                    });
                    ctx.note(Change::Updated, ItemKind::Material, Some(pid), title, 1);
                }
                MaterialStatus::InvalidPath => {}
                _ => digests.push(OverlayPair {
                    material: lm.id.clone(),
                    local_path: lm.path.clone(),
                    remote_path: rm.path.clone(),
                }),
            }
            continue;
        }
        // 本機的檔還在、但在應用以外被改過：不補回、不覆寫，提醒
        if state == MaterialStatus::SizeMismatch {
            ctx.notice(
                NoticeKind::MaterialChangedOutside,
                Some(pid),
                Some(&lm.path),
                Some(&lm.id),
            );
            if lm.sha256 == rm.sha256 {
                take_pages(&mut out.materials[i], rm, pid, title, ctx);
            }
            continue;
        }
        if lm.form == OVERLAY_FORM && rm.form == OVERLAY_FORM {
            // 譯文疊加檔：項目照規則（指紋都是 null），內容另外合併
            take_pages(&mut out.materials[i], rm, pid, title, ctx);
            if local_ok {
                overlays.push(OverlayPair {
                    material: lm.id.clone(),
                    local_path: lm.path.clone(),
                    remote_path: rm.path.clone(),
                });
            } else {
                files.push(FileFromRemote {
                    material: rm.id.clone(),
                    remote_path: rm.path.clone(),
                    local_path: lm.path.clone(),
                    sha256: None,
                    bytes: None,
                });
                ctx.note(Change::Updated, ItemKind::Material, Some(pid), title, 1);
            }
            continue;
        }
        if lm.sha256 == rm.sha256 {
            take_pages(&mut out.materials[i], rm, pid, title, ctx);
            if state == MaterialStatus::Missing {
                files.push(FileFromRemote {
                    material: rm.id.clone(),
                    remote_path: rm.path.clone(),
                    local_path: lm.path.clone(),
                    sha256: rm.sha256.clone(),
                    bytes: rm.bytes,
                });
                ctx.note(Change::Updated, ItemKind::Material, Some(pid), title, 1);
            }
            continue;
        }
        // 指紋不同：整項依 provenance.time 取較新，檔案內容跟著它；時間相同：衝突
        let ord = cmp_time(rm.provenance.time.as_deref(), lm.provenance.time.as_deref());
        let take_remote = match ord {
            Ordering::Greater => true,
            Ordering::Less => state == MaterialStatus::Missing,
            Ordering::Equal => {
                let choice = ctx.conflict(
                    ConflictKind::MaterialFile,
                    Some(pid),
                    title,
                    Some(&lm.id),
                    side(true, lm.provenance.time.as_deref(), None, None),
                    side(true, rm.provenance.time.as_deref(), None, None),
                    vec![ConflictChoice::Local, ConflictChoice::Incoming],
                    ConflictChoice::Local,
                    1,
                );
                if choice == ConflictChoice::Local {
                    out.materials[i].provenance.time = Some(ctx.now.clone());
                    false
                } else {
                    true
                }
            }
        };
        if take_remote {
            let mut m = rm.clone();
            m.path = lm.path.clone();
            if ord == Ordering::Equal {
                m.provenance.time = Some(ctx.now.clone());
            }
            out.materials[i] = m;
            files.push(FileFromRemote {
                material: rm.id.clone(),
                remote_path: rm.path.clone(),
                local_path: lm.path.clone(),
                sha256: rm.sha256.clone(),
                bytes: rm.bytes,
            });
            ctx.note(Change::Updated, ItemKind::Material, Some(pid), title, 1);
        }
    }
    // 合併後的刪除記錄有 material-removed 的：拿掉這一項（本機的檔由寫入的計畫搬進回收區）
    let removed = removed_materials(records, pid);
    if !removed.is_empty() {
        out.materials.retain(|m| !removed.contains(&m.id));
        files.retain(|f| !removed.contains(&f.material));
        overlays.retain(|o| !removed.contains(&o.material));
        digests.retain(|o| !removed.contains(&o.material));
    }

    let changed = out != *local;
    PaperMerge {
        paper: out,
        changed,
        files,
        overlays,
        digests,
    }
}

/// 書目給人看的幾欄（依這個順序）：篇名、作者（「姓, 名」或整串名稱，以「; 」分開）、年份、出處、DOI、網址。空的是 `None`。
fn bib_fields(b: &Bib) -> [(&'static str, Option<String>); 6] {
    let text = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    let authors: Vec<String> = b
        .authors
        .iter()
        .filter_map(
            |a| match (text(&a.family), text(&a.given), text(&a.literal)) {
                (Some(f), Some(g), _) => Some(format!("{f}, {g}")),
                (Some(f), None, _) => Some(f),
                (None, _, Some(l)) => Some(l),
                (None, g, None) => g,
            },
        )
        .collect();
    [
        ("title", text(&b.title)),
        ("authors", (!authors.is_empty()).then(|| authors.join("; "))),
        ("year", b.year.map(|y| y.to_string())),
        ("venue", text(&b.venue)),
        ("doi", text(&b.doi)),
        ("url", text(&b.url)),
    ]
}

/// 兩份書目不同的那幾欄：（本機那邊的, 匯入那邊的）。
fn differing_fields(l: &Bib, r: &Bib) -> (Vec<ConflictField>, Vec<ConflictField>) {
    let mut lo = Vec::new();
    let mut ro = Vec::new();
    for ((name, lv), (_, rv)) in bib_fields(l).into_iter().zip(bib_fields(r)) {
        if lv != rv {
            lo.push(ConflictField {
                name: name.to_string(),
                value: lv,
            });
            ro.push(ConflictField {
                name: name.to_string(),
                value: rv,
            });
        }
    }
    (lo, ro)
}

/// `pages`、`pages_modified`、`pages_modified_by` 整塊取較新。
fn take_pages(
    lm: &mut Material,
    rm: &Material,
    pid: &str,
    title: Option<&str>,
    ctx: &mut MergeCtx,
) {
    if cmp_stamp(
        rm.pages_modified.as_deref(),
        rm.pages_modified_by.as_deref(),
        lm.pages_modified.as_deref(),
        lm.pages_modified_by.as_deref(),
    ) == Ordering::Greater
        && (lm.pages, &lm.pages_modified, &lm.pages_modified_by)
            != (rm.pages, &rm.pages_modified, &rm.pages_modified_by)
    {
        if lm.pages != rm.pages {
            ctx.note(Change::Updated, ItemKind::Pages, Some(pid), title, 1);
        }
        lm.pages = rm.pages;
        lm.pages_modified = rm.pages_modified.clone();
        lm.pages_modified_by = rm.pages_modified_by.clone();
    }
}
