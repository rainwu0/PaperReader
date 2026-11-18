//! 由閱讀事件算出的閱讀狀況。
//!
//! 只有純計算：輸入是這篇文獻有效的事件與材料清單，輸出是狀況。事件從哪裡來、
//! 文獻在不在目錄裡，都不在這裡管。

use crate::reading::{ReadingKind, ValidEvent};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ReadStatus {
    /// 沒有任何有效的事件。
    Unread,
    /// 有打開或翻頁的紀錄，而且沒有被標成已讀。
    Reading,
    /// 最近一筆有效的標記是「已讀」。
    Read,
}

/// 某台裝置讀到的位置。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReadPosition {
    pub material: String,
    pub page: u32,
    pub offset: f64,
    pub zoom: Option<f64>,
    /// 畫面左緣在這一頁寬度的幾分之幾；沒有記是 `None`。
    pub left: Option<f64>,
    pub t: String,
    pub device: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReadingProgress {
    pub status: ReadStatus,
    /// 狀態是已讀時，生效的那一筆標記的時間。
    pub read_at: Option<String>,
    /// 閱讀位置：各裝置的閱讀位置裡時間最晚的那一筆；時間相同取 `device` 較大的。
    pub position: Option<ReadPosition>,
    /// 每台裝置各一筆：那台裝置最近一筆可用的位置。依裝置代號排序。
    pub device_positions: Vec<ReadPosition>,
    /// 每項材料（以代號為鍵）可用位置裡最大的頁碼。
    pub furthest_pages: BTreeMap<String, u32>,
    /// 最近一筆有效的打開或位置事件的時間（不可用的位置事件也算）。
    pub last_read_at: Option<String>,
}

impl ReadingProgress {
    /// 沒有任何事件時的狀況。
    pub fn unread() -> ReadingProgress {
        ReadingProgress {
            status: ReadStatus::Unread,
            read_at: None,
            position: None,
            device_positions: Vec::new(),
            furthest_pages: BTreeMap::new(),
            last_read_at: None,
        }
    }
}

/// 一項材料的代號與頁數（沒記頁數是 `None`），算「可用」時要對照。
pub struct MaterialInfo<'a> {
    pub id: &'a str,
    pub pages: Option<u32>,
}

/// 照閱讀狀況的規則表計算。`events` 是這篇文獻有效的事件，必須已依
/// （`t`、`device`、`id`）排好序。
pub fn compute(events: &[ValidEvent], materials: &[MaterialInfo<'_>]) -> ReadingProgress {
    let mut out = ReadingProgress::unread();

    let mut last_status: Option<(&str, bool)> = None;
    let mut by_device: BTreeMap<&str, ReadPosition> = BTreeMap::new();

    for e in events {
        match &e.kind {
            ReadingKind::Status { read } => last_status = Some((e.t.as_str(), *read)),
            ReadingKind::Open { .. } => {
                bump_last_read(&mut out, &e.t);
            }
            ReadingKind::Position {
                material,
                page,
                offset,
                zoom,
                left,
            } => {
                bump_last_read(&mut out, &e.t);
                // 可用：材料現在仍在清單裡，而且（記有頁數時）頁碼不超過頁數
                let usable = materials
                    .iter()
                    .find(|m| m.id == material)
                    .is_some_and(|m| m.pages.is_none_or(|n| *page <= n));
                if !usable {
                    continue;
                }
                let far = out.furthest_pages.entry(material.clone()).or_insert(0);
                *far = (*far).max(*page);
                // 事件已排序，同一台裝置後來的覆蓋先前的
                by_device.insert(
                    e.device.as_str(),
                    ReadPosition {
                        material: material.clone(),
                        page: *page,
                        offset: *offset,
                        zoom: *zoom,
                        left: *left,
                        t: e.t.clone(),
                        device: e.device.clone(),
                    },
                );
            }
        }
    }

    out.device_positions = by_device.into_values().collect();
    // 時間最晚的；時間相同取 device 較大的
    out.position = out
        .device_positions
        .iter()
        .max_by(|a, b| (&a.t, &a.device).cmp(&(&b.t, &b.device)))
        .cloned();

    match last_status {
        Some((t, true)) => {
            out.status = ReadStatus::Read;
            out.read_at = Some(t.to_string());
        }
        _ if out.last_read_at.is_some() => out.status = ReadStatus::Reading,
        _ => {}
    }
    out
}

fn bump_last_read(p: &mut ReadingProgress, t: &str) {
    // 事件已排序，後來的一定不早於先前的
    p.last_read_at = Some(t.to_string());
}
