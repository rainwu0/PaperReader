//! 把原文 PDF 裡「被譯文蓋住的範圍」內的文字指令刪掉。
//!
//! 譯本 PDF 是在原文的頁面上蓋一個白色矩形再寫譯文；只蓋不刪的話，原文的文字還在檔案裡，
//! 選字、劃線的引句會撈到英文。這裡在畫面用 pdf-lib 寫入譯文之前先處理：讀每一頁的內容串流，
//! 追蹤圖形狀態與文字矩陣，文字顯示指令（`Tj`、`TJ`、`'`、`"`）的起點落在任何一個蓋住的矩形裡就刪掉它。
//! 用 `lopdf`（MIT）讀寫；其餘的內容（圖、公式、向量圖、頁首頁尾）原樣保留。
//!
//! 限制（寫進結果文件）：
//! - 起點是文字矩陣與目前變換矩陣算出來的；`Tj` 之後的前進量要字寬才知道，這裡不算，
//!   所以同一個 `BT` 裡連續幾個沒有 `Td`／`Tm` 的顯示指令，位置以第一個的起點為準
//!   （一段被蓋的範圍通常整行都在裡面，所以一起刪掉）。
//! - 只處理頁面本身的內容串流，不進表單物件（Form XObject）裡的文字；那一部分由閱讀畫面的文字層過濾擋掉。
//! - 加密的 PDF 讀不了時交回錯誤，由呼叫的一方保留原檔、只靠過濾。

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, Stream, dictionary};
use std::collections::BTreeMap;

/// `[x0, y0, x1, y1]`，PDF 頁面座標。
pub type Rect = [f64; 4];

type Mat = [f64; 6];
const IDENT: Mat = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
/// 起點離矩形邊多遠還算在裡面（點）。
const TOL: f64 = 1.0;

/// `a` 先、`b` 後（PDF 的 `cm`、`Tm` 都是新矩陣乘在前面：`new × old`）。
fn mul(a: &Mat, b: &Mat) -> Mat {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}

fn nums(ops: &[Object]) -> Vec<f64> {
    ops.iter().filter_map(num).collect()
}

struct State {
    ctm: Mat,
    stack: Vec<Mat>,
    tm: Mat,
    tlm: Mat,
    leading: f64,
    size: f64,
}

impl State {
    fn origin(&self) -> (f64, f64) {
        let m = mul(&self.tm, &self.ctm);
        // 字的中段（基線上方約三成字高）：落在行框的正中
        let dy = self.size * 0.3;
        (m[4] + m[2] * dy, m[5] + m[3] * dy)
    }

    fn newline(&mut self, tx: f64, ty: f64) {
        self.tlm = mul(&[1.0, 0.0, 0.0, 1.0, tx, ty], &self.tlm);
        self.tm = self.tlm;
    }
}

fn inside(p: (f64, f64), rects: &[Rect]) -> bool {
    rects
        .iter()
        .any(|r| p.0 >= r[0] - TOL && p.0 <= r[2] + TOL && p.1 >= r[1] - TOL && p.1 <= r[3] + TOL)
}

/// 處理一頁的內容；交回新的運算列與刪掉的文字指令數。
fn strip_content(content: Content, rects: &[Rect]) -> (Vec<Operation>, usize) {
    let mut st = State {
        ctm: IDENT,
        stack: vec![],
        tm: IDENT,
        tlm: IDENT,
        leading: 0.0,
        size: 10.0,
    };
    let mut out = Vec::with_capacity(content.operations.len());
    let mut removed = 0;
    for op in content.operations {
        let a = nums(&op.operands);
        match op.operator.as_str() {
            "q" => st.stack.push(st.ctm),
            "Q" => {
                if let Some(m) = st.stack.pop() {
                    st.ctm = m;
                }
            }
            "cm" if a.len() == 6 => {
                st.ctm = mul(&[a[0], a[1], a[2], a[3], a[4], a[5]], &st.ctm);
            }
            "BT" => {
                st.tm = IDENT;
                st.tlm = IDENT;
            }
            "Tf" => {
                if let Some(s) = op.operands.get(1).and_then(num) {
                    st.size = s.abs().max(1.0);
                }
            }
            "TL" if a.len() == 1 => st.leading = a[0],
            "Td" if a.len() == 2 => st.newline(a[0], a[1]),
            "TD" if a.len() == 2 => {
                st.leading = -a[1];
                st.newline(a[0], a[1]);
            }
            "Tm" if a.len() == 6 => {
                st.tm = [a[0], a[1], a[2], a[3], a[4], a[5]];
                st.tlm = st.tm;
            }
            "T*" => st.newline(0.0, -st.leading),
            "Tj" | "TJ" => {
                if inside(st.origin(), rects) {
                    removed += 1;
                    continue;
                }
            }
            "'" | "\"" => {
                st.newline(0.0, -st.leading);
                if inside(st.origin(), rects) {
                    removed += 1;
                    // 換行的效果留著，字刪掉
                    out.push(Operation::new("T*", vec![]));
                    continue;
                }
            }
            _ => {}
        }
        out.push(op);
    }
    (out, removed)
}

/// 刪掉每一頁裡落在 `covers`（頁序從 0 起 → 矩形們）內的文字指令，交回新檔的位元組與刪掉的指令數。
pub fn strip_covered_text(
    pdf: &[u8],
    covers: &BTreeMap<u32, Vec<Rect>>,
) -> Result<(Vec<u8>, usize), String> {
    let mut doc = Document::load_mem(pdf).map_err(|e| format!("cannot read PDF: {e}"))?;
    let pages = doc.get_pages();
    let mut total = 0;
    for (index, rects) in covers {
        let Some(&page_id) = pages.get(&(index + 1)) else {
            continue;
        };
        // 嚴格解析：讀到一半停（lopdf 的寬鬆解析會丟掉剩下的）或讀不懂內嵌圖（交回沒有運算元的 `BI`）時，
        // 這一頁不改，保留原內容；殘留的字由閱讀畫面的文字層過濾擋
        let data = doc.get_page_content(page_id);
        let Ok(content) = Content::decode_strict(&data) else {
            continue;
        };
        if content
            .operations
            .iter()
            .any(|o| o.operator == "BI" && o.operands.is_empty())
        {
            continue;
        }
        let (ops, removed) = strip_content(content, rects);
        if removed == 0 {
            continue;
        }
        let Ok(bytes) = (Content { operations: ops }).encode() else {
            continue;
        };
        // 一律新建串流物件、把這一頁的 `Contents` 指過去：幾頁共用同一個內容串流時，別的頁不會被一起改到
        let stream = doc.add_object(Stream::new(dictionary! {}, bytes));
        if let Ok(Object::Dictionary(dict)) = doc.get_object_mut(page_id) {
            dict.set("Contents", Object::Reference(stream));
            total += removed;
        }
    }
    // 一個字都沒刪：原檔交回，不經過 lopdf 轉寫
    if total == 0 {
        return Ok((pdf.to_vec(), 0));
    }
    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| format!("cannot write PDF: {e}"))?;
    Ok((out, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::Stream;
    use lopdf::dictionary;

    /// 一頁：三行字（Td 定位）、一個圖（矩形路徑）、一行在 `cm` 平移過的區塊裡的字。
    fn sample() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content =
            b"BT /F1 10 Tf 50 700 Td (Covered line one) Tj 0 -14 Td (Covered line two) Tj ET\n\
            BT /F1 10 Tf 50 500 Td (Outside line) Tj ET\n\
            100 100 200 50 re S\n\
            q 1 0 0 1 300 0 cm BT /F1 10 Tf 0 700 Td [(Shifted ) -20 (covered)] TJ ET Q\n\
            BT /F1 10 Tf 12 TL 50 400 Td (Quote A) Tj (Quote B) ' ET";
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => resources_id,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    fn text_of(pdf: &[u8]) -> String {
        let doc = Document::load_mem(pdf).unwrap();
        let (&n, _) = doc.get_pages().iter().next().unwrap();
        doc.extract_text(&[n]).unwrap_or_default()
    }

    fn ops_of(pdf: &[u8]) -> Vec<String> {
        let doc = Document::load_mem(pdf).unwrap();
        let (_, &id) = doc.get_pages().iter().next().unwrap();
        doc.get_and_decode_page_content(id)
            .unwrap()
            .operations
            .into_iter()
            .map(|o| o.operator)
            .collect()
    }

    #[test]
    fn removes_text_inside_the_covers_and_keeps_everything_else() {
        let pdf = sample();
        let before = text_of(&pdf);
        assert!(before.contains("Covered line one") && before.contains("Outside line"));
        let mut covers = BTreeMap::new();
        // 蓋住前兩行（y 700 與 686）、平移後的那一行（x 300 起、y 700）與 `'` 顯示的那行（y 400-12=388）
        covers.insert(
            0,
            vec![
                [40.0, 680.0, 250.0, 715.0],
                [290.0, 690.0, 420.0, 715.0],
                [40.0, 380.0, 250.0, 395.0],
            ],
        );
        let (out, removed) = strip_covered_text(&pdf, &covers).unwrap();
        assert_eq!(removed, 4, "兩行、平移的一行、一個 '");
        let after = text_of(&out);
        assert!(!after.contains("Covered line"), "{after}");
        assert!(!after.contains("Shifted"), "{after}");
        assert!(!after.contains("Quote B"), "{after}");
        assert!(after.contains("Outside line"), "沒蓋到的字留著：{after}");
        assert!(
            after.contains("Quote A"),
            "第一個 Quote A 在範圍外：{after}"
        );
        // 圖（re、S）原樣；`'` 換成 T*，換行還在
        let ops = ops_of(&out);
        assert!(ops.contains(&"re".to_string()) && ops.contains(&"S".to_string()));
        assert!(ops.contains(&"T*".to_string()));
        assert!(!ops.contains(&"'".to_string()));
    }

    #[test]
    fn nothing_to_remove_keeps_the_text() {
        let pdf = sample();
        let mut covers = BTreeMap::new();
        covers.insert(0, vec![[0.0, 0.0, 10.0, 10.0]]);
        covers.insert(7, vec![[0.0, 0.0, 600.0, 800.0]]); // 沒有這一頁
        let (out, removed) = strip_covered_text(&pdf, &covers).unwrap();
        assert_eq!(removed, 0);
        assert!(text_of(&out).contains("Covered line one"));
    }

    #[test]
    fn unreadable_input_is_an_error() {
        let r = strip_covered_text(b"not a pdf", &BTreeMap::new());
        assert!(r.is_err());
    }
    /// 一頁，內容串流由 `content` 直接給（可以放讀不懂的東西）。
    fn page_with(content: &[u8]) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources_id =
            doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => resources_id,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    fn raw_content(pdf: &[u8]) -> Vec<u8> {
        let doc = Document::load_mem(pdf).unwrap();
        let (_, &id) = doc.get_pages().iter().next().unwrap();
        doc.get_page_content(id)
    }

    #[test]
    fn a_page_with_unreadable_content_is_left_exactly_as_it_was() {
        // 前面有一行該刪的字，中間有 lopdf 讀不懂的東西，後面還有圖與文字：這一頁整個不動
        let content = [
            &b"BT /F1 10 Tf 50 700 Td (Covered line) Tj ET\n"[..],
            &b"<< >> ] ] [[ \\ ) (\n"[..],
            &b"100 100 200 50 re S\n"[..],
            &b"BT /F1 10 Tf 50 500 Td (After the damage) Tj ET"[..],
        ]
        .concat();
        let pdf = page_with(&content);
        let before = raw_content(&pdf);
        assert!(String::from_utf8_lossy(&before).contains("After the damage"));
        let mut covers = BTreeMap::new();
        covers.insert(0, vec![[40.0, 680.0, 250.0, 715.0]]);
        let (out, removed) = strip_covered_text(&pdf, &covers).unwrap();
        assert_eq!(removed, 0, "讀不懂的頁不改");
        assert_eq!(raw_content(&out), before, "內容串流一個位元組都沒變");
    }

    #[test]
    fn an_operandless_inline_image_marker_means_the_page_is_left_alone() {
        let content = [
            &b"BT /F1 10 Tf 50 700 Td (Covered line) Tj ET\n"[..],
            &b"BI /W 1 /H 1 ID \xff EI\n"[..],
            &b"BT /F1 10 Tf 50 500 Td (Outside) Tj ET"[..],
        ]
        .concat();
        let pdf = page_with(&content);
        let before = raw_content(&pdf);
        let mut covers = BTreeMap::new();
        covers.insert(0, vec![[40.0, 680.0, 250.0, 715.0]]);
        let (out, removed) = strip_covered_text(&pdf, &covers).unwrap();
        // 讀得懂的內嵌圖可以處理；讀不懂（沒有運算元的 BI）就不改。兩種結果都不能弄壞後半頁
        let after = String::from_utf8_lossy(&raw_content(&out)).into_owned();
        assert!(after.contains("Outside"), "{after}");
        if removed == 0 {
            assert_eq!(raw_content(&out), before);
        } else {
            assert!(!after.contains("Covered line"));
            assert!(
                !after.contains(
                    "BI
"
                ) || after.contains("ID"),
                "{after}"
            );
        }
    }

    #[test]
    fn nothing_removed_returns_the_original_bytes() {
        let pdf = sample();
        let mut covers = BTreeMap::new();
        covers.insert(0, vec![[0.0, 0.0, 10.0, 10.0]]);
        let (out, removed) = strip_covered_text(&pdf, &covers).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(out, pdf, "沒刪任何東西就原檔交回，不轉寫");
    }
}
