//! 一篇（一個代號）的合併：`paper.json` 與材料、筆跡、譯文疊加檔、重點筆記、位置（目錄或回收區）、
//! 筆記檔。

use super::*;

impl LocalLibrary {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn merge_unit(
        &self,
        id: &str,
        title: Option<&str>,
        lp: Option<&(Place, String, PaperFile)>,
        rp: Option<&crate::sync::tree::RemotePaper>,
        tree: &RemoteTree,
        records: &[ChangeRecord],
        deleted_groups: Option<&HashSet<String>>,
        finder: &mut CandidateFinder,
        ctx: &mut MergeCtx,
        unit: &mut Unit,
        names: &mut UnitNames,
        notes: &NotesCtx,
        local_sources: &HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        let deleted_strokes = deleted_ids(records, id, STROKES_DELETED, "strokes");
        let deleted_judgments = deleted_ids(records, id, LINK_JUDGMENT_DELETED, "judgments");

        // 永久刪除：最晚的一筆位置記錄是永久刪除時，回收區裡的那篇從兩邊拿掉、不帶回；
        // 任一邊在目錄裡的一律不刪（之後放回、或一直在用），提醒並記一筆放回，下次同步不再提醒
        let catalog = matches!(lp, Some((Place::Papers, _, _)))
            || rp.is_some_and(|r| r.place == Place::Papers);
        let purged = purged_last(records, id);
        if purged && !catalog {
            if matches!(lp, Some((Place::Trash, _, _))) {
                unit.purges.push(id.to_string());
                ctx.note(Change::Removed, ItemKind::Paper, Some(id), title, 1);
            }
            if notes.ok {
                self.merge_notes_unit(id, title, records, ctx, unit, names, notes)?;
            }
            return Ok(());
        }
        // 到這裡 purged 表示那篇在某一邊的目錄裡：結果放在目錄
        let keep_in_catalog = purged;
        if keep_in_catalog {
            ctx.notice(NoticeKind::PurgedButRestored, Some(id), None, None);
            ctx.record(Some(id), PAPER_RESTORED, serde_json::json!({}));
        }

        match (lp, rp) {
            (Some((lplace, lfolder, lpaper)), r) => {
                let ldir = self.root.join(base_of(*lplace)).join(lfolder);
                let dir_rel = format!("{}/{lfolder}", base_of(*lplace));
                unit.dir_rel = Some(dir_rel.clone());
                unit.read.push((
                    format!("{dir_rel}/{PAPER_FILE}"),
                    file_sha(&ldir.join(PAPER_FILE)),
                ));
                let rplace = r.map_or(Place::Absent, |r| r.place);
                // paper.json 與材料
                if let Some(r) = r {
                    let folder_names = names_in_or_empty(&ldir);
                    let state = |m: &crate::format::Material| material_status(&ldir, m);
                    let mut pm = merge_paper(
                        lpaper,
                        &LocalFolder {
                            names: &folder_names,
                            file_state: &state,
                        },
                        &r.paper,
                        deleted_groups,
                        records,
                        title,
                        ctx,
                    );
                    for f in &pm.files {
                        let rel = paper_rel(r.place, &r.folder, &f.remote_path);
                        let Some((src, after)) = finder.resolve(tree, &rel, f.sha256.as_deref())
                        else {
                            // 只帶變更的同步包省略了它、本機也找不到：只略過這一項材料
                            ctx.notice(
                                NoticeKind::MissingFile,
                                Some(id),
                                Some(&f.local_path),
                                Some(&f.material),
                            );
                            let at = pm.paper.materials.iter().position(|m| m.id == f.material);
                            let had = lpaper.materials.iter().find(|m| m.id == f.material);
                            match (at, had) {
                                (Some(i), Some(lm)) => {
                                    pm.paper.materials[i] = lm.clone();
                                    ctx.unnote(Change::Updated, ItemKind::Material, Some(id));
                                }
                                (Some(i), None) => {
                                    pm.paper.materials.remove(i);
                                    ctx.unnote(Change::Added, ItemKind::Material, Some(id));
                                }
                                _ => {}
                            }
                            continue;
                        };
                        unit.paper_writes.push(WriteItem {
                            paper: Some(id.to_string()),
                            target: f.local_path.clone(),
                            before: file_sha(&ldir.join(&f.local_path)),
                            after,
                            src,
                            notes: false,
                        });
                    }
                    // 筆跡
                    self.merge_ink_files(
                        id,
                        title,
                        Some(&ldir),
                        Some(&paper_rel(r.place, &r.folder, INK_FILE)),
                        tree,
                        &deleted_strokes,
                        ctx,
                        unit,
                    )?;
                    // 參考文獻清單：抽出的部分比的是合併後那篇原文的指紋
                    self.merge_references_files(
                        id,
                        title,
                        &ldir,
                        Some(&paper_rel(r.place, &r.folder, REFERENCES_FILE)),
                        tree,
                        &deleted_judgments,
                        source_sha(&pm.paper).as_deref(),
                        ctx,
                        unit,
                    )?;
                    // 抽出的文字：同上，比的是合併後那篇原文的指紋
                    self.merge_text_files(
                        id,
                        title,
                        &ldir,
                        &paper_rel(r.place, &r.folder, TEXT_FILE),
                        tree,
                        source_sha(&pm.paper).as_deref(),
                        ctx,
                        unit,
                    )?;
                    // 譯文疊加檔
                    for o in &pm.overlays {
                        let lpath = ldir.join(&o.local_path);
                        unit.read
                            .push((format!("{dir_rel}/{}", o.local_path), file_sha(&lpath)));
                        let rrel = paper_rel(r.place, &r.folder, &o.remote_path);
                        // 匯入端的路徑要是同步包裡放進來的檔
                        if !tree.has(&rrel) {
                            continue;
                        }
                        let lo = read_opt::<OverlayFile>(&lpath);
                        let ro = read_opt::<OverlayFile>(&tree.path(&rrel));
                        let (Ok(Some(lo)), Ok(Some(ro))) = (&lo, &ro) else {
                            if lo.is_err() {
                                ctx.notice(
                                    NoticeKind::LocalUnreadable,
                                    Some(id),
                                    Some(&format!("{dir_rel}/{}", o.local_path)),
                                    None,
                                );
                            }
                            if ro.is_err() {
                                ctx.notice(
                                    NoticeKind::IncomingUnreadable,
                                    Some(id),
                                    Some(&rrel),
                                    None,
                                );
                            }
                            continue;
                        };
                        if lo.format > FORMAT_VERSION || ro.format > FORMAT_VERSION {
                            ctx.notice(NoticeKind::NewerFormat, Some(id), Some(&rrel), None);
                            continue;
                        }
                        if let Some(doc) = crate::sync::merge::ink::merge_overlay(
                            lo,
                            ro,
                            id,
                            &o.material,
                            &rrel,
                            title,
                            ctx,
                        ) {
                            let bytes = canonical(&doc).map_err(|e| e.to_string())?;
                            unit.paper_writes.push(WriteItem {
                                paper: Some(id.to_string()),
                                target: o.local_path.clone(),
                                before: file_sha(&lpath),
                                after: sha256_hex(&bytes),
                                src: Src::Bytes(bytes),
                                notes: false,
                            });
                        }
                    }
                    // 重點筆記：逐點合併
                    for o in &pm.digests {
                        self.merge_digest_unit(
                            id,
                            title,
                            records,
                            &ldir,
                            &dir_rel,
                            &paper_rel(r.place, &r.folder, &o.remote_path),
                            o,
                            tree,
                            ctx,
                            unit,
                        );
                    }
                    // 合併後的刪除記錄拿掉的本機材料（合併層只會因 material-removed 拿掉本機已有的項目）：
                    // paper.json 寫好之後把檔搬進回收區
                    for lm in &lpaper.materials {
                        if !pm.paper.materials.iter().any(|m| m.id == lm.id) {
                            unit.trash_materials.push((id.to_string(), lm.clone()));
                            ctx.note(Change::Removed, ItemKind::Material, Some(id), title, 1);
                        }
                    }
                    if pm.paper != *lpaper {
                        let bytes = canonical(&pm.paper).map_err(|e| e.to_string())?;
                        unit.paper_writes.push(WriteItem {
                            paper: Some(id.to_string()),
                            target: PAPER_FILE.to_string(),
                            before: file_sha(&ldir.join(PAPER_FILE)),
                            after: sha256_hex(&bytes),
                            src: Src::Bytes(bytes),
                            notes: false,
                        });
                    }
                } else {
                    self.merge_ink_files(
                        id,
                        title,
                        Some(&ldir),
                        None,
                        tree,
                        &deleted_strokes,
                        ctx,
                        unit,
                    )?;
                    self.merge_references_files(
                        id,
                        title,
                        &ldir,
                        None,
                        tree,
                        &deleted_judgments,
                        source_sha(lpaper).as_deref(),
                        ctx,
                        unit,
                    )?;
                }
                // 位置
                let trash_folder = if *lplace == Place::Trash {
                    lfolder.clone()
                } else {
                    format!("{}-{lfolder}", ctx.now_compact)
                };
                let place = if keep_in_catalog {
                    Place::Papers
                } else {
                    merge_place(id, title, *lplace, rplace, records, &trash_folder, ctx)
                };
                if place != *lplace && place != Place::Absent {
                    let name = if place == Place::Trash {
                        unique_lower(names.trash, &trash_folder)
                    } else {
                        let base = strip_trash_prefix(lfolder);
                        unique_lower(names.papers, base)
                    };
                    if place == Place::Trash {
                        ctx.note(Change::Removed, ItemKind::TrashState, Some(id), title, 1);
                    } else {
                        ctx.note(Change::Updated, ItemKind::TrashState, Some(id), title, 1);
                    }
                    unit.moves.push((id.to_string(), place, name));
                }
            }
            (None, Some(r)) => {
                // 只在匯入那邊：整篇加進來
                let mut newp = prepare_new_paper(&r.paper, deleted_groups, records);
                let place = if keep_in_catalog {
                    Place::Papers
                } else {
                    merge_place(id, title, Place::Absent, r.place, records, &r.folder, ctx)
                };
                let place = if place == Place::Absent {
                    r.place
                } else {
                    place
                };
                let mut files: Vec<(String, Src, String)> = Vec::new();
                let mut unresolved: Vec<String> = Vec::new();
                for m in &newp.materials {
                    if !is_valid_material_path(&m.path) || m.path.contains('\\') {
                        continue;
                    }
                    let rel = paper_rel(r.place, &r.folder, &m.path);
                    if tree.entry(&rel).is_none() {
                        continue; // 匯出端本來就沒有這個檔
                    }
                    let Some((src, after)) = finder.resolve(tree, &rel, m.sha256.as_deref()) else {
                        // 只略過這一項材料
                        ctx.notice(
                            NoticeKind::MissingFile,
                            Some(id),
                            Some(&m.path),
                            Some(&m.id),
                        );
                        unresolved.push(m.id.clone());
                        continue;
                    };
                    files.push((m.path.clone(), src, after));
                    if m.role == crate::format::ROLE_SOURCE
                        && let Some(s) = &m.sha256
                        && let Some(other) = local_sources.get(s)
                    {
                        ctx.notice(NoticeKind::SameSource, Some(id), None, Some(other));
                    }
                }
                let rink = paper_rel(r.place, &r.folder, INK_FILE);
                if tree.has(&rink) {
                    match read_opt::<InkFile>(&tree.path(&rink)) {
                        Ok(Some(ink)) if ink.format <= FORMAT_VERSION => {
                            if let Some(doc) = merge_ink_mod::merge_ink(
                                None,
                                Some(&ink),
                                &deleted_strokes,
                                id,
                                title,
                                ctx,
                            ) {
                                let bytes = canonical(&doc).map_err(|e| e.to_string())?;
                                let sha = sha256_hex(&bytes);
                                files.push((INK_FILE.to_string(), Src::Bytes(bytes), sha));
                            }
                        }
                        Ok(Some(_)) => {
                            ctx.notice(NoticeKind::NewerFormat, Some(id), Some(&rink), None)
                        }
                        Ok(None) => {}
                        Err(()) => {
                            ctx.notice(NoticeKind::IncomingUnreadable, Some(id), Some(&rink), None)
                        }
                    }
                }
                // 參考文獻清單：照原樣複製（拿掉已刪除的判斷後）
                let rrefs = paper_rel(r.place, &r.folder, REFERENCES_FILE);
                if tree.has(&rrefs) {
                    match read_references_file(&tree.path(&rrefs)) {
                        Ok(Some((doc, _))) if doc.format <= FORMAT_VERSION => {
                            if let Some(doc) = crate::sync::merge::references::merge_references(
                                None,
                                Some(&doc),
                                &deleted_judgments,
                                source_sha(&newp).as_deref(),
                                id,
                                title,
                                ctx,
                            ) {
                                let bytes = canonical(&doc).map_err(|e| e.to_string())?;
                                let sha = sha256_hex(&bytes);
                                files.push((REFERENCES_FILE.to_string(), Src::Bytes(bytes), sha));
                            }
                        }
                        Ok(Some(_)) => {
                            ctx.notice(NoticeKind::NewerFormat, Some(id), Some(&rrefs), None)
                        }
                        Ok(None) => {}
                        Err(_) => {
                            ctx.notice(NoticeKind::IncomingUnreadable, Some(id), Some(&rrefs), None)
                        }
                    }
                }
                // 抽出的文字：照複製（讀得懂、format 不較新、在上限內）
                let rtext = paper_rel(r.place, &r.folder, TEXT_FILE);
                if tree.has(&rtext) {
                    match read_text_file(&tree.path(&rtext)) {
                        Ok(Some((doc, _))) if doc.format <= FORMAT_VERSION => {
                            if let Some(doc) = crate::sync::merge::text::merge_text(
                                None,
                                Some(&doc),
                                source_sha(&newp).as_deref(),
                                id,
                                &rtext,
                                title,
                                ctx,
                            ) {
                                let bytes = canonical(&doc).map_err(|e| e.to_string())?;
                                let sha = sha256_hex(&bytes);
                                files.push((TEXT_FILE.to_string(), Src::Bytes(bytes), sha));
                            }
                        }
                        Ok(Some(_)) => {
                            ctx.notice(NoticeKind::NewerFormat, Some(id), Some(&rtext), None)
                        }
                        Ok(None) => {}
                        Err(_) => {
                            ctx.notice(NoticeKind::IncomingUnreadable, Some(id), Some(&rtext), None)
                        }
                    }
                }
                newp.materials.retain(|m| !unresolved.contains(&m.id));
                let bytes = canonical(&newp).map_err(|e| e.to_string())?;
                let sha = sha256_hex(&bytes);
                files.push((PAPER_FILE.to_string(), Src::Bytes(bytes), sha));
                let name = if place == Place::Trash {
                    // 匯入那邊在目錄、因記錄要放進回收區：名稱加上時間
                    let base = if r.place == Place::Trash {
                        r.folder.clone()
                    } else {
                        format!("{}-{}", ctx.now_compact, r.folder)
                    };
                    unique_lower(names.trash, &base)
                } else {
                    unique_lower(names.papers, strip_trash_prefix(&r.folder))
                };
                ctx.note(Change::Added, ItemKind::Paper, Some(id), title, 1);
                unit.new_papers.push(NewPaperItem {
                    paper: id.to_string(),
                    to: place,
                    name,
                    files,
                });
            }
            (None, None) => {}
        }

        // 筆記檔
        if notes.ok {
            self.merge_notes_unit(id, title, records, ctx, unit, names, notes)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn merge_ink_files(
        &self,
        id: &str,
        title: Option<&str>,
        ldir: Option<&Path>,
        rrel: Option<&str>,
        tree: &RemoteTree,
        deleted: &HashSet<String>,
        ctx: &mut MergeCtx,
        unit: &mut Unit,
    ) -> std::result::Result<(), String> {
        let Some(ldir) = ldir else { return Ok(()) };
        let lpath = ldir.join(INK_FILE);
        let lrel = self.rel_of(&lpath);
        unit.read.push((lrel.clone(), file_sha(&lpath)));
        let li = match read_opt::<InkFile>(&lpath) {
            Ok(v) => v,
            Err(()) => {
                ctx.notice(NoticeKind::LocalUnreadable, Some(id), Some(&lrel), None);
                return Ok(());
            }
        };
        let ri = match rrel {
            Some(rel) if tree.has(rel) => match read_opt::<InkFile>(&tree.path(rel)) {
                Ok(v) => v,
                Err(()) => {
                    ctx.notice(NoticeKind::IncomingUnreadable, Some(id), Some(rel), None);
                    return Ok(());
                }
            },
            _ => None,
        };
        if li.as_ref().is_some_and(|i| i.format > FORMAT_VERSION)
            || ri.as_ref().is_some_and(|i| i.format > FORMAT_VERSION)
        {
            ctx.notice(NoticeKind::NewerFormat, Some(id), Some(&lrel), None);
            return Ok(());
        }
        if let Some(doc) =
            merge_ink_mod::merge_ink(li.as_ref(), ri.as_ref(), deleted, id, title, ctx)
        {
            let bytes = canonical(&doc).map_err(|e| e.to_string())?;
            unit.paper_writes.push(WriteItem {
                paper: Some(id.to_string()),
                target: INK_FILE.to_string(),
                before: file_sha(&lpath),
                after: sha256_hex(&bytes),
                src: Src::Bytes(bytes),
                notes: false,
            });
        }
        Ok(())
    }

    /// 一篇的參考文獻清單：依檔合併，同 `ink.json`。任一邊讀不懂或 `format` 較新：
    /// 不合併，本機的不動，提醒。`source` 是合併後那篇原文的指紋。
    #[allow(clippy::too_many_arguments)]
    fn merge_references_files(
        &self,
        id: &str,
        title: Option<&str>,
        ldir: &Path,
        rrel: Option<&str>,
        tree: &RemoteTree,
        deleted: &HashSet<String>,
        source: Option<&str>,
        ctx: &mut MergeCtx,
        unit: &mut Unit,
    ) -> std::result::Result<(), String> {
        let lpath = ldir.join(REFERENCES_FILE);
        let lrel = self.rel_of(&lpath);
        unit.read.push((lrel.clone(), file_sha(&lpath)));
        let local = match read_references_file(&lpath) {
            Ok(v) => v.map(|(doc, _)| doc),
            Err(_) => {
                ctx.notice(NoticeKind::LocalUnreadable, Some(id), Some(&lrel), None);
                return Ok(());
            }
        };
        let remote = match rrel {
            Some(rel) if tree.has(rel) => match read_references_file(&tree.path(rel)) {
                Ok(v) => v.map(|(doc, _)| doc),
                Err(_) => {
                    ctx.notice(NoticeKind::IncomingUnreadable, Some(id), Some(rel), None);
                    return Ok(());
                }
            },
            _ => None,
        };
        if local.as_ref().is_some_and(|d| d.format > FORMAT_VERSION)
            || remote.as_ref().is_some_and(|d| d.format > FORMAT_VERSION)
        {
            ctx.notice(NoticeKind::NewerFormat, Some(id), Some(&lrel), None);
            return Ok(());
        }
        if let Some(doc) = crate::sync::merge::references::merge_references(
            local.as_ref(),
            remote.as_ref(),
            deleted,
            source,
            id,
            title,
            ctx,
        ) {
            let bytes = canonical(&doc).map_err(|e| e.to_string())?;
            unit.paper_writes.push(WriteItem {
                paper: Some(id.to_string()),
                target: REFERENCES_FILE.to_string(),
                before: file_sha(&lpath),
                after: sha256_hex(&bytes),
                src: Src::Bytes(bytes),
                notes: false,
            });
        }
        Ok(())
    }

    /// 一篇的抽出的文字：依檔合併，整份取一邊。同步包裡沒帶（「只帶變更」省略的）時不讀本機那份、
    /// 不算它的指紋，本機的留著：先看 `tree.has`，才把本機那份記進 `unit.read`。任一邊讀不懂或 `format` 較新：
    /// 不合併，本機的不動，提醒。`source` 是合併後那篇原文的指紋。只在本機有的不動（呼叫的一方只在匯入那邊也有
    /// 這篇時呼叫）。
    #[allow(clippy::too_many_arguments)]
    fn merge_text_files(
        &self,
        id: &str,
        title: Option<&str>,
        ldir: &Path,
        rrel: &str,
        tree: &RemoteTree,
        source: Option<&str>,
        ctx: &mut MergeCtx,
        unit: &mut Unit,
    ) -> std::result::Result<(), String> {
        if !tree.has(rrel) {
            return Ok(());
        }
        let lpath = ldir.join(TEXT_FILE);
        let lrel = self.rel_of(&lpath);
        unit.read.push((lrel.clone(), file_sha(&lpath)));
        let local = match read_text_file(&lpath) {
            Ok(v) => v.map(|(doc, _)| doc),
            Err(_) => {
                ctx.notice(NoticeKind::LocalUnreadable, Some(id), Some(&lrel), None);
                return Ok(());
            }
        };
        let remote = match read_text_file(&tree.path(rrel)) {
            Ok(v) => v.map(|(doc, _)| doc),
            Err(_) => {
                ctx.notice(NoticeKind::IncomingUnreadable, Some(id), Some(rrel), None);
                return Ok(());
            }
        };
        if local.as_ref().is_some_and(|d| d.format > FORMAT_VERSION)
            || remote.as_ref().is_some_and(|d| d.format > FORMAT_VERSION)
        {
            ctx.notice(NoticeKind::NewerFormat, Some(id), Some(&lrel), None);
            return Ok(());
        }
        if let Some(doc) = crate::sync::merge::text::merge_text(
            local.as_ref(),
            remote.as_ref(),
            source,
            id,
            &lrel,
            title,
            ctx,
        ) {
            let bytes = canonical(&doc).map_err(|e| e.to_string())?;
            unit.paper_writes.push(WriteItem {
                paper: Some(id.to_string()),
                target: TEXT_FILE.to_string(),
                before: file_sha(&lpath),
                after: sha256_hex(&bytes),
                src: Src::Bytes(bytes),
                notes: false,
            });
        }
        Ok(())
    }

    /// 一份兩邊都有的重點筆記：依點的代號合併、看 `digest-point-deleted`；同一點兩邊文字不同、
    /// 或點以外的文字不同時留本機的，匯入那份另存衝突副本（放在那篇的資料夾，檔名照 4.6 的規則）。
    #[allow(clippy::too_many_arguments)]
    fn merge_digest_unit(
        &self,
        id: &str,
        title: Option<&str>,
        records: &[ChangeRecord],
        ldir: &Path,
        dir_rel: &str,
        rrel: &str,
        o: &crate::sync::merge::paper::OverlayPair,
        tree: &RemoteTree,
        ctx: &mut MergeCtx,
        unit: &mut Unit,
    ) {
        let lpath = ldir.join(&o.local_path);
        let lrel = format!("{dir_rel}/{}", o.local_path);
        unit.read.push((lrel.clone(), file_sha(&lpath)));
        // 匯入端的路徑要是同步包裡放進來的檔
        if !tree.has(rrel) {
            return;
        }
        let read_text = |p: &Path| fs::read(p).ok().and_then(|b| String::from_utf8(b).ok());
        let lt = read_text(&lpath).filter(|t| crate::digest::parse(t).is_ok());
        let rt = read_text(&tree.path(rrel)).filter(|t| crate::digest::parse(t).is_ok());
        let (Some(lt), Some(rt)) = (lt, rt) else {
            if read_text(&lpath)
                .filter(|t| crate::digest::parse(t).is_ok())
                .is_none()
            {
                ctx.notice(NoticeKind::LocalUnreadable, Some(id), Some(&lrel), None);
            } else {
                ctx.notice(NoticeKind::IncomingUnreadable, Some(id), Some(rrel), None);
            }
            return;
        };
        let deleted: HashSet<String> = records
            .iter()
            .filter(|r| r.kind == DIGEST_POINT_DELETED && r.paper.as_deref() == Some(id))
            .filter(|r| r.data_str("material") == Some(o.material.as_str()))
            .filter_map(|r| r.data_str("point").map(String::from))
            .collect();
        let stamp = crate::digest::DigestStamp {
            time: ctx.now.clone(),
            device: ctx.device.clone(),
        };
        let Ok(m) = crate::digest::merge(&lt, &rt, &deleted, &stamp) else {
            return;
        };
        if m.point_conflicts > 0 {
            ctx.notice(
                NoticeKind::DigestPointConflict,
                Some(id),
                Some(&lrel),
                Some(&o.material),
            );
        }
        if let Some(t) = m.text {
            let bytes = t.into_bytes();
            unit.paper_writes.push(WriteItem {
                paper: Some(id.to_string()),
                target: o.local_path.clone(),
                before: file_sha(&lpath),
                after: sha256_hex(&bytes),
                src: Src::Bytes(bytes),
                notes: false,
            });
            ctx.note(Change::Updated, ItemKind::Material, Some(id), title, 1);
        }
        let Some(copy) = m.copy else { return };
        // 同一份差異已經有副本：不再寫，提醒指向那一份（同 4.6「不重複寫副本」）
        let file = last_segment(&o.local_path);
        let stem = file.strip_suffix(".md").unwrap_or(file);
        let prefix = format!("{stem} (conflict ");
        let names = names_in_or_empty(ldir);
        let same = names.iter().find(|n| {
            n.starts_with(&prefix)
                && n.ends_with(".md")
                && read_text(&ldir.join(n.as_str()))
                    .is_some_and(|t| unify_newlines(&t) == unify_newlines(&copy))
        });
        if let Some(n) = same {
            ctx.notice(
                NoticeKind::ConflictCopy,
                Some(id),
                Some(&format!("{dir_rel}/{n}")),
                None,
            );
            return;
        }
        let dev6: String = ctx.remote_device.chars().take(6).collect();
        let base = unique_stem(
            &names,
            ".md",
            &format!("{stem} (conflict {dev6} {})", ctx.now_compact),
        );
        let name = format!("{base}.md");
        let rel = format!("{dir_rel}/{name}");
        let bytes = copy.into_bytes();
        ctx.notice(NoticeKind::ConflictCopy, Some(id), Some(&rel), None);
        unit.copies.push(rel);
        unit.paper_writes.push(WriteItem {
            paper: Some(id.to_string()),
            target: name,
            before: None,
            after: sha256_hex(&bytes),
            src: Src::Bytes(bytes),
            notes: false,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn merge_notes_unit(
        &self,
        id: &str,
        title: Option<&str>,
        records: &[ChangeRecord],
        ctx: &mut MergeCtx,
        unit: &mut Unit,
        names: &mut UnitNames,
        notes: &NotesCtx,
    ) -> std::result::Result<(), String> {
        let dir = self.root.join(notes.dir);
        let local_name = notes.local.get(id);
        let local_text = match local_name {
            Some(n) => match read_notes_text(&dir.join(n)) {
                Ok((t, _)) => Some(t),
                Err(_) => {
                    ctx.notice(
                        NoticeKind::LocalUnreadable,
                        Some(id),
                        Some(&format!("{}/{n}", notes.dir)),
                        None,
                    );
                    return Ok(());
                }
            },
            None => None,
        };
        if let Some(n) = local_name {
            let rel = format!("{}/{n}", notes.dir);
            unit.read.push((rel.clone(), file_sha(&dir.join(n))));
        }
        let remote = notes.remote.get(id);
        let remote_text = match remote {
            Some((n, None)) => {
                ctx.notice(NoticeKind::IncomingUnreadable, Some(id), Some(n), None);
                None
            }
            Some((_, Some(t))) => Some(t.as_str()),
            None => None,
        };
        if local_text.is_none() && remote_text.is_none() {
            return Ok(());
        }
        let labels = self.notes_labels.clone();
        let path = local_name
            .map(|n| format!("{}/{n}", notes.dir))
            .or_else(|| remote.map(|(n, _)| n.clone()))
            .unwrap_or_default();
        let nm = merge_notes(
            id,
            title,
            local_text.as_deref(),
            remote_text,
            records,
            &labels,
            &path,
            ctx,
        );
        let file_name = match local_name {
            Some(n) => n.clone(),
            None => {
                let rn = remote
                    .map(|(n, _)| n.clone())
                    .unwrap_or_else(|| format!("{id}.md"));
                let stem = rn.strip_suffix(".md").unwrap_or(&rn).to_string();
                let base = unique_lower_md(names.notes, &stem);
                format!("{base}.md")
            }
        };
        if let Some(t) = nm.text {
            let bytes = t.into_bytes();
            unit.root_writes.push(WriteItem {
                paper: None,
                target: format!("{}/{file_name}", notes.dir),
                before: local_name.and_then(|n| file_sha(&dir.join(n))),
                after: sha256_hex(&bytes),
                src: Src::Bytes(bytes),
                notes: true,
            });
        }
        // 衝突副本：本機合併前那份、匯入那份，需要幾份寫幾份；檔名帶各自的裝置代號前 6 碼
        for copy in nm.copies {
            let same = notes.existing_copies.get(id).and_then(|v| {
                v.iter()
                    .find(|(_, t)| unify_newlines(t) == unify_newlines(&copy.text))
                    .map(|(n, _)| n.clone())
            });
            match same {
                Some(n) => {
                    ctx.notice_for(
                        NoticeKind::ConflictCopy,
                        Some(id),
                        Some(&format!("{}/{n}", notes.dir)),
                        None,
                        copy.conflicts.clone(),
                    );
                }
                None => {
                    let stem = file_name.strip_suffix(".md").unwrap_or(&file_name);
                    let dev6: String = copy.device.chars().take(6).collect();
                    let base = unique_lower_md(
                        names.notes,
                        &format!("{stem} (conflict {dev6} {})", ctx.now_compact),
                    );
                    let target = format!("{}/{base}.md", notes.dir);
                    let bytes = copy.text.into_bytes();
                    ctx.notice_for(
                        NoticeKind::ConflictCopy,
                        Some(id),
                        Some(&target),
                        None,
                        copy.conflicts,
                    );
                    unit.copies.push(target.clone());
                    unit.root_writes.push(WriteItem {
                        paper: None,
                        target,
                        before: None,
                        after: sha256_hex(&bytes),
                        src: Src::Bytes(bytes),
                        notes: false,
                    });
                }
            }
        }
        Ok(())
    }
}

/// 合併後的紀錄裡，這篇某一種刪除記錄（`strokes-deleted`、`link-judgment-deleted`）列的每一個代號。
fn deleted_ids(records: &[ChangeRecord], paper: &str, kind: &str, key: &str) -> HashSet<String> {
    records
        .iter()
        .filter(|r| r.kind == kind && r.paper.as_deref() == Some(paper))
        .flat_map(|r| {
            r.data
                .get(key)
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect()
}

/// 那篇原文的指紋（材料清單裡 `role` 是 `source` 的那一項）。
fn source_sha(p: &PaperFile) -> Option<String> {
    p.materials
        .iter()
        .find(|m| m.role == crate::format::ROLE_SOURCE)
        .and_then(|m| m.sha256.clone())
}

/// `.trash/` 的資料夾名稱去掉前面的時間（同 `restore_paper`）。
fn strip_trash_prefix(folder: &str) -> &str {
    let b = folder.as_bytes();
    let stamped = b.len() > 17
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'T'
        && b[9..15].iter().all(u8::is_ascii_digit)
        && b[15] == b'Z'
        && b[16] == b'-';
    if stamped { &folder[17..] } else { folder }
}

/// 在一組（小寫的）名稱裡挑一個不撞名的（` (2)` …），並把它記進去。
fn unique_lower(taken: &mut HashSet<String>, base: &str) -> String {
    let existing: Vec<String> = taken.iter().cloned().collect();
    let name = unique_name(&existing, base);
    taken.insert(name.to_lowercase());
    name
}

/// 同 `unique_lower`，名稱是筆記檔（`taken` 裡的是含 `.md` 的檔名）。交回不含 `.md` 的名稱。
fn unique_lower_md(taken: &mut HashSet<String>, stem: &str) -> String {
    let existing: Vec<String> = taken.iter().cloned().collect();
    let base = unique_stem(&existing, ".md", stem);
    taken.insert(format!("{base}.md").to_lowercase());
    base
}
