//! 寫閱讀事件的三個操作：記下打開、記下位置、設定已讀。
//!
//! 共通的檢查：文獻不在目錄裡（包括已在回收區）→ 找不到；材料不屬於這篇文獻 → 輸入不合法；
//! 欄位不在範圍內 → 輸入不合法；文獻庫唯讀 → 唯讀。檢查沒過就一個位元組也不寫。

use super::LocalLibrary;
use crate::error::{LibError, Result};
use crate::format::{Event, Map, PaperFile};
use crate::ids::{format_utc_millis, new_id, now_utc_millis};
use crate::reading::{
    LEFT_FIELD, ReadingKind, TYPE_OPEN, TYPE_POSITION, TYPE_STATUS, parse_event, same_left,
    same_position, validate_left, validate_position,
};
use serde_json::{Value, json};

impl LocalLibrary {
    /// 使用者打開一篇文獻的閱讀畫面。
    pub fn record_open(&mut self, paper_id: &str, material_id: &str) -> Result<Event> {
        let paper = self.prepare_write(paper_id)?;
        material_pages(&paper, material_id)?;
        self.write_event(paper_id, TYPE_OPEN, json!({ "material": material_id }))
    }

    /// 使用者讀到了某個位置。和這台裝置對同一篇、同一項材料的上一筆位置事件比，
    /// 頁碼相同、頁內位置相差小於 0.02、縮放相同，就不寫，交回 `None`。不帶水平位置
    /// （同 `record_position_with_left` 給 `None`）。
    pub fn record_position(
        &mut self,
        paper_id: &str,
        material_id: &str,
        page: i64,
        offset: f64,
        zoom: Option<f64>,
    ) -> Result<Option<Event>> {
        self.record_position_with_left(paper_id, material_id, page, offset, zoom, None)
    }

    /// 同 `record_position`，多一個選填的水平位置：畫面左緣在這一頁寬度的幾分之幾，
    /// 0 到 1。不重複寫的比對多一項：水平位置相同（兩邊都不知道，或相差小於 0.02）。
    pub fn record_position_with_left(
        &mut self,
        paper_id: &str,
        material_id: &str,
        page: i64,
        offset: f64,
        zoom: Option<f64>,
        left: Option<f64>,
    ) -> Result<Option<Event>> {
        let paper = self.prepare_write(paper_id)?;
        let pages = material_pages(&paper, material_id)?;
        let (page, offset, zoom) = validate_position(page, offset, zoom)
            .map_err(|reason| LibError::InvalidInput { reason })?;
        let left = validate_left(left).map_err(|reason| LibError::InvalidInput { reason })?;
        if let Some(n) = pages
            && page > n
        {
            return Err(LibError::InvalidInput {
                reason: format!("page {page} is beyond the material's {n} pages"),
            });
        }

        if let Some((prev, prev_left)) = self.last_own_position(paper_id, material_id)
            && same_position(prev, (page, offset, zoom))
            && same_left(prev_left, left)
        {
            return Ok(None);
        }

        let mut data = json!({
            "material": material_id,
            "page": page,
            "offset": offset,
            "zoom": zoom,
        });
        // 沒有水平位置時不寫這一欄（和這一版之前寫的事件一樣）
        if let (Some(l), Some(obj)) = (left, data.as_object_mut()) {
            obj.insert(LEFT_FIELD.to_string(), json!(l));
        }
        self.write_event(paper_id, TYPE_POSITION, data).map(Some)
    }

    /// 使用者手動標成已讀（`true`），或取消這個標記（`false`）。
    pub fn set_read(&mut self, paper_id: &str, read: bool) -> Result<Event> {
        self.prepare_write(paper_id)?;
        self.write_event(paper_id, TYPE_STATUS, json!({ "read": read }))
    }

    /// 寫入前的共通準備：唯讀就拒絕；紀錄檔若在開啟之後被別的程序改過就重讀（才知道這台裝置
    /// 上一筆的時間與上一筆位置）；確認文獻在目錄裡（以磁碟上的 `paper.json` 為準）。
    /// 交回這篇文獻目前的描述檔，供檢查材料。
    fn prepare_write(&mut self, paper_id: &str) -> Result<PaperFile> {
        self.require_writable()?;
        self.refresh_events_if_changed()?;
        Ok(self.locate_mut(paper_id)?.paper)
    }

    /// 這台裝置對這篇文獻、這項材料的上一筆有效位置事件：（頁碼, 頁內位置, 縮放）與水平位置。
    #[allow(clippy::type_complexity)]
    fn last_own_position(
        &self,
        paper_id: &str,
        material_id: &str,
    ) -> Option<((u32, f64, Option<f64>), Option<f64>)> {
        self.events
            .events_of(paper_id)
            .iter()
            .rev()
            .filter(|e| e.device == self.device_id)
            .find_map(|e| match &e.kind {
                ReadingKind::Position {
                    material,
                    page,
                    offset,
                    zoom,
                    left,
                } if material == material_id => Some(((*page, *offset, *zoom), *left)),
                _ => None,
            })
    }

    /// 組出事件並附加到紀錄檔。時間只往前走：取「現在」與「這台裝置上一筆事件的
    /// 時間加 1 毫秒」兩者中較晚的。
    fn write_event(&mut self, paper_id: &str, kind: &str, data: Value) -> Result<Event> {
        let now = now_utc_millis();
        let ms = match self.events.own_last_ms() {
            Some(last) => now.max(last.saturating_add(1)),
            None => now,
        };
        let t = format_utc_millis(ms).ok_or_else(|| LibError::InvalidInput {
            reason: format!("event time {ms} ms is out of range"),
        })?;
        let event = Event {
            id: new_id(),
            t,
            device: self.device_id.clone(),
            paper: paper_id.to_string(),
            kind: kind.to_string(),
            data,
            extra: Map::new(),
        };
        // 寫得進去的事件讀回來一定有效：用讀取端同一套判斷先檢查
        if parse_event(&event).is_none() {
            return Err(LibError::InvalidInput {
                reason: format!("the {kind} event would not be valid when read back"),
            });
        }
        self.append_raw(&event)?;
        Ok(event)
    }
}

/// 材料屬於這篇文獻就交回它記的頁數（沒記是 `None`）；不屬於是「輸入不合法」。
fn material_pages(paper: &PaperFile, material_id: &str) -> Result<Option<u32>> {
    paper
        .materials
        .iter()
        .find(|m| m.id == material_id)
        .map(|m| m.pages)
        .ok_or_else(|| LibError::InvalidInput {
            reason: format!(
                "material {material_id:?} does not belong to paper {}",
                paper.id
            ),
        })
}
