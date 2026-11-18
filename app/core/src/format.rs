//! 檔案格式：`library.json`、`paper.json`、閱讀紀錄的一行。
//!
//! 欄位名稱、順序、可否為 `null` 都照規格。每個結構都以 `extra` 保留不認得的欄位，
//! 讀進來再寫回去不會丟掉。

use serde::{Deserialize, Serialize};

pub type Map = serde_json::Map<String, serde_json::Value>;

pub const FORMAT_VERSION: u32 = 1;
pub const ROLE_SOURCE: &str = "source";
pub const ROLE_TRANSLATION: &str = "translation";
pub const FORM_PDF: &str = "pdf";
pub const METHOD_ADDED_BY_USER: &str = "added-by-user";
/// 整批匯入的材料。
pub const METHOD_IMPORTED: &str = "imported";

pub(crate) fn one() -> u32 {
    FORMAT_VERSION
}
fn default_notes_dir() -> String {
    "notes".to_string()
}
fn empty_array() -> serde_json::Value {
    serde_json::Value::Array(Vec::new())
}

/// 欄位寫成 `null` 時當作缺少，用型別的預設值（`id` 以外的欄位 `null` 不算讀不懂）。
pub(crate) fn null_as_default<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// `format` 缺少或 `null`：當作目前的格式版本。
pub(crate) fn format_or_current<'de, D>(d: D) -> std::result::Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u32>::deserialize(d)?.unwrap_or(FORMAT_VERSION))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryFile {
    #[serde(default = "one")]
    pub format: u32,
    pub id: String,
    #[serde(default)]
    pub created: String,
    #[serde(default = "default_notes_dir")]
    pub notes_dir: String,
    /// 群組的定義，存原始的 JSON 值、開啟時不解讀：寫壞了不可以讓文獻庫打不開。
    /// 由 `groups` 模組在用到時解讀。
    #[serde(default = "empty_array")]
    pub groups: serde_json::Value,
    /// 已刪除群組的記錄，同上。
    #[serde(default = "empty_array")]
    pub deleted_groups: serde_json::Value,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperFile {
    #[serde(default = "one", deserialize_with = "format_or_current")]
    pub format: u32,
    pub id: String,
    #[serde(default)]
    pub short_name: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub added: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub bib: Bib,
    #[serde(default, deserialize_with = "null_as_default")]
    pub materials: Vec<Material>,
    /// 這篇屬於哪些群組（群組的代號，依加入的先後；可以有已不存在的懸空代號）。
    #[serde(default, deserialize_with = "null_as_default")]
    pub groups: Vec<String>,
    /// 群組所屬最後一次改變的時間與裝置；從沒改過是 `null`。
    #[serde(default)]
    pub groups_modified: Option<String>,
    #[serde(default)]
    pub groups_modified_by: Option<String>,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Bib {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub authors: Vec<Author>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub venue: Option<String>,
    #[serde(default)]
    pub doi: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// 使用者用改書目清空過的欄位（`year`、`doi`、`url` 之中）：合併與自動補書目都不補這幾欄；
    /// 之後填了值就拿掉。沒有時不寫進檔案（舊的 `paper.json` 照原樣）。
    #[serde(
        default,
        deserialize_with = "null_as_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub cleared: Vec<String>,
    #[serde(default)]
    pub modified: Option<String>,
    #[serde(default)]
    pub modified_by: Option<String>,
    #[serde(flatten)]
    pub extra: Map,
}

/// 自動補書目會補的三欄。合併時「較新那份的空欄用另一份補上」也只限這三欄。
pub const FILLABLE_BIB_FIELDS: [&str; 3] = ["year", "doi", "url"];

impl Bib {
    /// 這一欄（`FILLABLE_BIB_FIELDS` 之一）空著：`null` 或空字串。
    pub fn is_blank(&self, field: &str) -> bool {
        match field {
            "year" => self.year.is_none(),
            "doi" => self.doi.as_deref().is_none_or(|s| s.trim().is_empty()),
            "url" => self.url.as_deref().is_none_or(|s| s.trim().is_empty()),
            _ => false,
        }
    }

    /// 書目合併時，這份是較新的那份（時間相同時兩邊各算一次）：較舊那份 `cleared` 記下、這份仍空著的欄併進 `cleared`，
    /// 那一欄也不補；然後用較舊那份補空欄（`fill_blanks_from`）。不動 `modified`。兩台各自匯入對方時算出同一份
    /// 結果。
    pub fn merged_with_older(&self, older: &Bib) -> Bib {
        let mut b = self.clone();
        let carried: Vec<&str> = FILLABLE_BIB_FIELDS
            .into_iter()
            .filter(|f| {
                older.cleared.iter().any(|c| c == f)
                    && b.is_blank(f)
                    && !b.cleared.iter().any(|c| c == f)
            })
            .collect();
        if !carried.is_empty() {
            let mut all = b.cleared.clone();
            all.extend(carried.into_iter().map(String::from));
            b.cleared = canonical_cleared(&all);
        }
        b.fill_blanks_from(older);
        b
    }

    /// 用 `other` 的值補上這份空著、而且沒有記在 `cleared` 的三欄；不動 `modified`。有補到交回 `true`。
    pub fn fill_blanks_from(&mut self, other: &Bib) -> bool {
        let mut changed = false;
        for f in FILLABLE_BIB_FIELDS {
            if !self.is_blank(f) || other.is_blank(f) || self.cleared.iter().any(|c| c == f) {
                continue;
            }
            match f {
                "year" => self.year = other.year,
                "doi" => self.doi = other.doi.clone(),
                _ => self.url = other.url.clone(),
            }
            changed = true;
        }
        changed
    }
}

/// `cleared` 的固定順序：認得的三欄照 `FILLABLE_BIB_FIELDS` 的順序、不重複，不認得的（新版應用記的）接在後面照原樣。
/// 兩台算出的 `cleared` 因此逐字相同。
pub fn canonical_cleared(cleared: &[String]) -> Vec<String> {
    let mut out: Vec<String> = FILLABLE_BIB_FIELDS
        .into_iter()
        .filter(|f| cleared.iter().any(|c| c == f))
        .map(String::from)
        .collect();
    for c in cleared {
        if !FILLABLE_BIB_FIELDS.contains(&c.as_str()) && !out.contains(c) {
            out.push(c.clone());
        }
    }
    out
}

/// 自動補書目（`fill_bib_blanks`）交來的值；`None` 表示這一欄沒抽到。
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct BibFill {
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub doi: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Author {
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub given: Option<String>,
    #[serde(default)]
    pub literal: Option<String>,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub id: String,
    pub role: String,
    pub form: String,
    pub path: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub bytes: Option<u64>,
    #[serde(default)]
    pub pages: Option<u32>,
    /// 頁數最後一次寫入的時間與裝置；從沒寫過頁數是 `null`。
    #[serde(default)]
    pub pages_modified: Option<String>,
    #[serde(default)]
    pub pages_modified_by: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub provenance: Provenance,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub origin_name: Option<String>,
    #[serde(default)]
    pub proofread: Option<bool>,
    #[serde(default)]
    pub page_map: Option<String>,
    #[serde(default)]
    pub source_sha256: Option<String>,
    #[serde(flatten)]
    pub extra: Map,
}

/// 回收區裡一項移除的材料的說明檔名（`.trash/<資料夾>/removed-material.json`）。
pub const REMOVED_MATERIAL_FILE: &str = "removed-material.json";

/// 移除一項材料（`remove_material`）時，和材料檔一起放進 `.trash/<資料夾>/` 的說明。
/// 資料夾裡沒有 `paper.json`，所以不會被當成一篇文獻。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemovedMaterialFile {
    #[serde(default = "one", deserialize_with = "format_or_current")]
    pub format: u32,
    /// 那篇文獻的代號。
    pub paper: String,
    /// 移除時那篇的資料夾名稱（只給人看）。
    #[serde(default)]
    pub paper_folder: String,
    pub removed: String,
    pub removed_by: String,
    /// 材料檔在這個資料夾裡的檔名；材料檔當時不在時是 `null`。
    #[serde(default)]
    pub file: Option<String>,
    /// 移除前 `paper.json` 材料清單裡的那一項，原樣。
    pub material: Material,
    #[serde(flatten)]
    pub extra: Map,
}

/// 閱讀紀錄檔裡的一行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub t: String,
    pub device: String,
    pub paper: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub data: serde_json::Value,
    #[serde(flatten)]
    pub extra: Map,
}

/// 加入或修改時給的書目草稿。沒有 `modified` 這類由文獻庫蓋章的欄位。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BibDraft {
    pub title: Option<String>,
    pub authors: Vec<Author>,
    pub year: Option<i32>,
    pub venue: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
}

/// DOI 一律存成小寫、不含網址前綴。
pub fn normalize_doi(s: &str) -> String {
    let t = s.trim();
    let lower = t.to_lowercase();
    for prefix in [
        "https://doi.org/",
        "http://doi.org/",
        "https://www.doi.org/",
        "http://www.doi.org/",
        "https://dx.doi.org/",
        "http://dx.doi.org/",
        "doi.org/",
        "dx.doi.org/",
        "doi:",
    ] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return rest.trim().to_string();
        }
    }
    lower
}
