//! 版面模型（PP-DocLayoutV3）的推論：收一張已縮成模型輸入大小的頁面圖，交回版面區塊的方框。
//! 只用在翻譯的分段；怎麼用這些方框分段在畫面一側（`src/translate/regions.ts`）。
//!
//! 前處理與後處理照 `app/scripts/layout-model.md` 在 Node 上驗過的做法：
//! 輸入是 800×800 的 RGB（不保持長寬比，由畫面縮好）、除以 255、不減平均值；另給 `im_shape`（輸入的高、寬）
//! 與 `scale_factor`（輸入對原圖的縮放：高、寬）。輸出每一列是類別、分數、方框（原圖座標）。
//! 原圖的單位由呼叫的人決定：給 PDF 點，方框就是 PDF 點（左上為原點、y 向下）。
//!
//! ONNX Runtime 用 `ort` 預設下載的 pyke.io 建置；遙測明確關掉。模型檔的下載與核對在外殼
//! （和離線翻譯的模型同一套，`download.rs`）。

use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use serde::Serialize;
use std::path::Path;

/// 模型的輸入邊長（像素）。PP-DocLayoutV3 的 `inference.yml` 是 800×800。
pub const INPUT_SIZE: usize = 800;

/// 一張輸入圖的位元組數（RGB）。
pub const RGB_BYTES: usize = INPUT_SIZE * INPUT_SIZE * 3;

/// 信心門檻：官方 `inference.yml` 的 `draw_threshold`。
pub const DEFAULT_THRESHOLD: f32 = 0.5;

/// PP-DocLayoutV3 的類別，依模型輸出的編號排列（`app/scripts/layout-model.md`）。
pub const LABELS: [&str; 25] = [
    "abstract",
    "algorithm",
    "aside_text",
    "chart",
    "content",
    "display_formula",
    "doc_title",
    "figure_title",
    "footer",
    "footer_image",
    "footnote",
    "formula_number",
    "header",
    "header_image",
    "image",
    "inline_formula",
    "number",
    "paragraph_title",
    "reference",
    "reference_content",
    "seal",
    "table",
    "text",
    "vertical_text",
    "vision_footnote",
];

/// 一個版面區塊。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LayoutBox {
    /// 類別名稱（`LABELS`）；編號超出範圍時是 `other`。
    pub label: &'static str,
    pub score: f32,
    /// 原圖座標的 x0、y0、x1、y1（左上為原點、y 向下）。
    pub bbox: [f32; 4],
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    /// 交進來的圖或大小不合（外來的內容）。
    #[error("bad input: {0}")]
    BadInput(&'static str),
    /// 模型檔載入或推論失敗（ONNX Runtime 回報的錯誤）。
    #[error("runtime: {0}")]
    Runtime(String),
}

fn runtime(e: impl std::fmt::Display) -> LayoutError {
    LayoutError::Runtime(e.to_string())
}

/// ONNX Runtime 一個程序只能建一次環境（`ort` 的 `EnvironmentBuilder::build`；建過之後即使放掉也不能再建，
/// 所以整個程序共用這一個。之後其他用到 ONNX Runtime 的地方（例如嵌入）也要共用它。
static ENVIRONMENT: std::sync::Mutex<Option<ort::environment::Environment>> =
    std::sync::Mutex::new(None);

fn environment() -> Result<ort::environment::Environment, LayoutError> {
    let mut env = ENVIRONMENT.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(e) = env.as_ref() {
        return Ok(e.clone());
    }
    let e = ort::init()
        .with_name("paperreader")
        .with_telemetry(false)
        .build()
        .map_err(runtime)?;
    *env = Some(e.clone());
    Ok(e)
}

/// 載入好的模型。推論要獨占（`Session::run` 要 `&mut`），由呼叫的人加鎖。
pub struct LayoutModel {
    session: Session,
    /// 模型宣告的輸入名稱（照名稱餵，不假設順序）。
    inputs: Vec<String>,
}

impl LayoutModel {
    /// 載入模型檔。`threads` 是單次推論用的執行緒數（0 表示由 ONNX Runtime 決定）。
    pub fn load(path: &Path, threads: usize) -> Result<Self, LayoutError> {
        let env = environment()?;
        let mut builder = Session::builder(&env)
            .map_err(runtime)?
            .with_optimization_level(GraphOptimizationLevel::All)
            .map_err(runtime)?;
        if threads > 0 {
            builder = builder
                .with_intra_threads(threads)
                .map_err(runtime)?
                .with_inter_threads(1)
                .map_err(runtime)?;
        }
        let session = builder.commit_from_file(path).map_err(runtime)?;
        let inputs = session
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();
        Ok(Self { session, inputs })
    }

    /// 跑一張圖。`rgb` 是 `INPUT_SIZE`×`INPUT_SIZE` 的 RGB（列優先，每點 3 位元組），
    /// `page_w`、`page_h` 是原圖的寬高（交回的方框用這個單位）。
    pub fn detect(
        &mut self,
        rgb: &[u8],
        page_w: f32,
        page_h: f32,
        threshold: f32,
    ) -> Result<Vec<LayoutBox>, LayoutError> {
        if rgb.len() != RGB_BYTES {
            return Err(LayoutError::BadInput("image size"));
        }
        if !(page_w.is_finite() && page_h.is_finite() && page_w > 0.0 && page_h > 0.0) {
            return Err(LayoutError::BadInput("page size"));
        }
        let mut image = Some(to_chw(rgb));
        let side = INPUT_SIZE as f32;
        let mut feeds: Vec<(String, ort::session::SessionInputValue<'_>)> = Vec::new();
        for name in &self.inputs {
            let value: ort::value::DynValue = match name.as_str() {
                "image" | "images" | "x" => {
                    let data = image
                        .take()
                        .ok_or_else(|| LayoutError::Runtime("two image inputs".into()))?;
                    Tensor::from_array(([1usize, 3, INPUT_SIZE, INPUT_SIZE], data))
                        .map_err(runtime)?
                        .into_dyn()
                }
                "im_shape" => Tensor::from_array(([1usize, 2], vec![side, side]))
                    .map_err(runtime)?
                    .into_dyn(),
                "scale_factor" => {
                    Tensor::from_array(([1usize, 2], vec![side / page_h, side / page_w]))
                        .map_err(runtime)?
                        .into_dyn()
                }
                _ => return Err(LayoutError::Runtime(format!("unknown model input {name}"))),
            };
            feeds.push((name.clone(), value.into()));
        }
        let outputs = self.session.run(feeds).map_err(runtime)?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>().map_err(runtime)?;
        let cols = shape.last().copied().unwrap_or(0).max(0) as usize;
        if cols < 6 {
            return Err(LayoutError::Runtime(format!(
                "unexpected output shape {shape:?}"
            )));
        }
        Ok(decode(data, cols, threshold))
    }
}

/// RGB（列優先、每點 3 位元組）轉成 CHW 的浮點數，除以 255。
fn to_chw(rgb: &[u8]) -> Vec<f32> {
    let n = rgb.len() / 3;
    let mut f = vec![0f32; 3 * n];
    let k = 1.0 / 255.0;
    for (i, px) in rgb.chunks_exact(3).enumerate() {
        f[i] = f32::from(px[0]) * k;
        f[n + i] = f32::from(px[1]) * k;
        f[2 * n + i] = f32::from(px[2]) * k;
    }
    f
}

/// 輸出每列：類別、分數、x0、y0、x1、y1、（之後的欄不用）。類別小於 0 或分數低於門檻的不要。
fn decode(data: &[f32], cols: usize, threshold: f32) -> Vec<LayoutBox> {
    data.chunks_exact(cols)
        .filter(|r| r[0] >= 0.0 && r[1] >= threshold && r.iter().all(|v| v.is_finite()))
        .map(|r| LayoutBox {
            label: LABELS
                .get(r[0].round() as usize)
                .copied()
                .unwrap_or("other"),
            score: r[1],
            bbox: [r[2], r[3], r[4], r[5]],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chw_splits_channels_and_scales() {
        let f = to_chw(&[255u8, 0, 51, 0, 255, 102]);
        let want = [1.0, 0.0, 0.0, 1.0, 0.2, 0.4];
        assert!(
            f.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6),
            "{f:?}"
        );
    }

    #[test]
    fn decode_drops_low_scores_padding_and_unknown_numbers() {
        let data = [
            22.0,
            0.9,
            1.0,
            2.0,
            3.0,
            4.0,
            5.0, //
            6.0,
            0.3,
            1.0,
            2.0,
            3.0,
            4.0,
            1.0, //
            -1.0,
            0.99,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0, //
            99.0,
            0.8,
            1.0,
            1.0,
            2.0,
            2.0,
            0.0, //
            22.0,
            0.8,
            f32::NAN,
            1.0,
            2.0,
            2.0,
            0.0,
        ];
        let b = decode(&data, 7, DEFAULT_THRESHOLD);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].label, "text");
        assert_eq!(b[0].bbox, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(b[1].label, "other");
    }

    /// 真的跑模型：要設 `PR_LAYOUT_MODEL`（模型檔）與 `PR_LAYOUT_RGB`（800×800 的 RGB 原始位元組）。
    /// 沒設就略過，`cargo test --workspace` 不需要模型檔。
    #[test]
    fn runs_real_model_when_given() {
        let (Ok(model), Ok(img)) = (
            std::env::var("PR_LAYOUT_MODEL"),
            std::env::var("PR_LAYOUT_RGB"),
        ) else {
            return;
        };
        let mut m = LayoutModel::load(Path::new(&model), 0).expect("load");
        let rgb = std::fs::read(img).expect("read image");
        let boxes = m
            .detect(&rgb, 612.0, 792.0, DEFAULT_THRESHOLD)
            .expect("detect");
        assert!(!boxes.is_empty());
        assert!(matches!(
            m.detect(&rgb[1..], 612.0, 792.0, DEFAULT_THRESHOLD),
            Err(LayoutError::BadInput(_))
        ));
        // 同一個程序裡再載入一次：環境共用，不能失敗
        let mut again = LayoutModel::load(Path::new(&model), 1).expect("load again");
        let n = again
            .detect(&rgb, 612.0, 792.0, DEFAULT_THRESHOLD)
            .expect("detect again")
            .len();
        assert_eq!(n, boxes.len());
    }
}
