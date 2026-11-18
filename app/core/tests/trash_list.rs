//! 列出回收區、還原取移除時間較晚的那一個。

mod common;

use common::*;
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::TrashEntry;
use std::fs;
use std::path::Path;

fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<_> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => vec![],
    };
    v.sort();
    v
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            fs::copy(e.path(), dest).unwrap();
        }
    }
}

/// `20261113T083011Z` → `2026-11-13T08:30:11.000Z`
fn iso_from_compact(c: &str) -> String {
    format!(
        "{}-{}-{}T{}:{}:{}.000Z",
        &c[0..4],
        &c[4..6],
        &c[6..8],
        &c[9..11],
        &c[11..13],
        &c[13..15]
    )
}

#[test]
fn two_removed_papers_are_listed_newest_first_with_matching_times() {
    let (dir, mut lib) = temp_library();
    let (a, _) = add_paper(&mut lib, 1, "Alpha", Some(3));
    let (b, _) = add_paper(&mut lib, 2, "Beta", Some(3));
    lib.remove_paper(&a).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    lib.remove_paper(&b).unwrap();

    let list = lib.list_trash().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(
        (list[0].paper_id.as_str(), list[0].name.as_str()),
        (b.as_str(), "Beta")
    );
    assert_eq!(
        (list[1].paper_id.as_str(), list[1].name.as_str()),
        (a.as_str(), "Alpha")
    );
    assert_eq!(list[0].bib.title.as_deref(), Some("Beta"));
    assert_eq!(list[1].bib.title.as_deref(), Some("Alpha"));
    for e in &list {
        let compact = &e.folder[..16];
        assert_eq!(
            e.removed_at.as_deref(),
            Some(iso_from_compact(compact).as_str())
        );
        assert_eq!(e.folder, format!("{compact}-{}", e.name));
    }
    assert!(list[0].removed_at > list[1].removed_at);
    // 只讀不寫
    assert_eq!(names_in(&dir.path().join(".trash")).len(), 2);
}

#[test]
fn folders_without_a_readable_paper_json_are_not_listed_and_not_touched() {
    let (dir, mut lib) = temp_library();
    let (a, _) = add_paper(&mut lib, 1, "Alpha", Some(3));
    lib.remove_paper(&a).unwrap();
    let trash = dir.path().join(".trash");
    fs::create_dir_all(trash.join("my-own-folder")).unwrap();
    fs::write(trash.join("my-own-folder/note.txt"), "mine").unwrap();
    fs::create_dir_all(trash.join("20261113T083011Z-broken")).unwrap();
    fs::write(trash.join("20261113T083011Z-broken/paper.json"), "{ nope").unwrap();
    fs::write(trash.join("loose-file.txt"), "x").unwrap();
    let before = snapshot(&trash);

    let list = lib.list_trash().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].paper_id, a);
    assert_eq!(snapshot(&trash), before);
}

#[test]
fn a_folder_without_a_time_prefix_but_with_paper_json_is_listed_last_with_unknown_time() {
    let (dir, mut lib) = temp_library();
    let (a, _) = add_paper(&mut lib, 1, "Alpha", Some(3));
    let (b, _) = add_paper(&mut lib, 2, "Beta", Some(3));
    lib.remove_paper(&a).unwrap();
    lib.remove_paper(&b).unwrap();
    // 把 Beta 的資料夾改成沒有時間前綴的名稱
    let trash = dir.path().join(".trash");
    let beta = names_in(&trash)
        .into_iter()
        .find(|n| n.ends_with("-Beta"))
        .unwrap();
    fs::rename(trash.join(&beta), trash.join("Beta by hand")).unwrap();

    let list = lib.list_trash().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].paper_id, a, "有時間的在前");
    assert_eq!(list[1].paper_id, b);
    assert_eq!(list[1].removed_at, None);
    assert_eq!(list[1].name, "Beta by hand");
    assert_eq!(list[1].folder, "Beta by hand");
}

#[test]
fn a_time_prefix_that_is_not_a_real_time_counts_as_no_prefix() {
    // 前綴看起來像時間、但不是真實存在的時間
    let (dir, mut lib) = temp_library();
    let (a, _) = add_paper(&mut lib, 1, "Alpha", Some(3));
    let (b, _) = add_paper(&mut lib, 2, "Beta", Some(3));
    let (c, _) = add_paper(&mut lib, 3, "Gamma", Some(3));
    for id in [&a, &b, &c] {
        lib.remove_paper(id).unwrap();
    }
    let trash = dir.path().join(".trash");
    let find = |suffix: &str| {
        names_in(&trash)
            .into_iter()
            .find(|n| n.ends_with(suffix))
            .unwrap()
    };
    fs::rename(
        trash.join(find("-Beta")),
        trash.join("99999999T999999Z-Beta"),
    )
    .unwrap();
    fs::rename(
        trash.join(find("-Gamma")),
        trash.join("20261332T256199Z-Gamma"),
    )
    .unwrap();

    let list = lib.list_trash().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].paper_id, a, "只有真實的時間排在前面");
    assert!(list[0].removed_at.is_some());
    let beta = list.iter().find(|e| e.paper_id == b).unwrap();
    assert_eq!(beta.removed_at, None);
    assert_eq!(beta.name, "99999999T999999Z-Beta");
    assert_eq!(beta.folder, "99999999T999999Z-Beta");
    let gamma = list.iter().find(|e| e.paper_id == c).unwrap();
    assert_eq!(gamma.removed_at, None);
    assert_eq!(gamma.name, "20261332T256199Z-Gamma");
    // 還原之後資料夾名稱是整個名稱
    lib.restore_paper(&b).unwrap();
    let back = lib.list_papers().into_iter().find(|s| s.id == b).unwrap();
    assert_eq!(back.folder, "99999999T999999Z-Beta");
}

#[test]
fn same_second_is_ordered_by_name_and_unknown_times_by_name() {
    let (dir, mut lib) = temp_library();
    let (a, _) = add_paper(&mut lib, 1, "Zeta", Some(3));
    let (b, _) = add_paper(&mut lib, 2, "Alpha", Some(3));
    let (c, _) = add_paper(&mut lib, 3, "Yy", Some(3));
    let (d, _) = add_paper(&mut lib, 4, "Aa", Some(3));
    for id in [&a, &b, &c, &d] {
        lib.remove_paper(id).unwrap();
    }
    // 全部改成同一秒；其中 Yy 與 Aa 去掉時間前綴
    let trash = dir.path().join(".trash");
    for n in names_in(&trash) {
        let name = &n[17..];
        let new = match name {
            "Yy" | "Aa" => format!("{name}-hand"),
            _ => format!("20261113T083011Z-{name}"),
        };
        fs::rename(trash.join(&n), trash.join(new)).unwrap();
    }
    let order: Vec<String> = lib
        .list_trash()
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(order, ["Alpha", "Zeta", "Aa-hand", "Yy-hand"]);
}

#[test]
fn two_folders_with_the_same_id_are_both_listed_and_restore_takes_the_later_one() {
    let (dir, mut lib) = temp_library();
    let (a, _) = add_paper(&mut lib, 1, "Alpha", Some(3));
    lib.remove_paper(&a).unwrap();
    let trash = dir.path().join(".trash");
    let newer = names_in(&trash)[0].clone();
    // 複製一份當作「較早移除」的，並把它的篇名改掉以便分辨
    let older = format!("20200101T000000Z-{}", &newer[17..]);
    copy_dir(&trash.join(&newer), &trash.join(&older));
    let pj = trash.join(&older).join("paper.json");
    let text = fs::read_to_string(&pj)
        .unwrap()
        .replace("\"Alpha\"", "\"Alpha OLD\"");
    fs::write(&pj, text).unwrap();

    let list = lib.list_trash().unwrap();
    assert_eq!(list.len(), 2, "代號相同的兩個都列出");
    assert!(list.iter().all(|e| e.paper_id == a));

    lib.restore_paper(&a).unwrap();
    let back = lib.list_papers();
    assert_eq!(back.len(), 1);
    assert_eq!(
        back[0].bib.title.as_deref(),
        Some("Alpha"),
        "回到目錄的是移除時間較晚的那一個"
    );
    let left = names_in(&trash);
    assert_eq!(left, vec![older], "另一個還在回收區");
}

#[test]
fn restore_prefers_a_folder_with_a_known_time_over_an_unknown_one() {
    let (dir, mut lib) = temp_library();
    let (a, _) = add_paper(&mut lib, 1, "Alpha", Some(3));
    lib.remove_paper(&a).unwrap();
    let trash = dir.path().join(".trash");
    let stamped = names_in(&trash)[0].clone();
    copy_dir(&trash.join(&stamped), &trash.join("Alpha by hand"));
    let pj = trash.join("Alpha by hand").join("paper.json");
    fs::write(
        &pj,
        fs::read_to_string(&pj)
            .unwrap()
            .replace("\"Alpha\"", "\"Hand\""),
    )
    .unwrap();
    lib.restore_paper(&a).unwrap();
    assert_eq!(lib.list_papers()[0].bib.title.as_deref(), Some("Alpha"));
    assert_eq!(names_in(&trash), vec!["Alpha by hand".to_string()]);
}

#[test]
fn same_id_in_catalog_and_trash_is_still_listed() {
    let (dir, mut lib) = temp_library();
    let (a, _) = add_paper(&mut lib, 1, "Alpha", Some(3));
    lib.remove_paper(&a).unwrap();
    let trash = dir.path().join(".trash");
    let folder = names_in(&trash)[0].clone();
    // 使用者把回收區裡的資料夾複製回 papers/
    copy_dir(
        &trash.join(&folder),
        &dir.path().join("papers").join("Alpha copy"),
    );
    lib.rescan().unwrap();
    assert_eq!(lib.list_papers().len(), 1);
    assert_eq!(lib.list_trash().unwrap().len(), 1);
}

#[test]
fn missing_trash_folder_gives_an_empty_list_and_read_only_can_list() {
    let (dir, lib) = temp_library();
    assert!(lib.list_trash().unwrap().is_empty());
    assert!(!dir.path().join(".trash").exists(), "列出不建立資料夾");
    drop(lib);

    // 唯讀的文獻庫也能列
    let (dir, mut lib) = temp_library();
    let (a, _) = add_paper(&mut lib, 1, "Alpha", Some(3));
    lib.remove_paper(&a).unwrap();
    drop(lib);
    let lj = dir.path().join("library.json");
    fs::write(
        &lj,
        fs::read_to_string(&lj)
            .unwrap()
            .replace("\"format\": 1", "\"format\": 2"),
    )
    .unwrap();
    let lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    assert!(lib.is_read_only());
    let list: Vec<TrashEntry> = lib.list_trash().unwrap();
    assert_eq!(list.len(), 1);
}

#[test]
fn list_trash_works_through_the_trait_object() {
    let (_dir, mut lib) = temp_library();
    let (a, _) = add_paper(&mut lib, 1, "Alpha", Some(3));
    lib.remove_paper(&a).unwrap();
    let store = &lib;
    assert_eq!(store.list_trash().unwrap().len(), 1);
}
