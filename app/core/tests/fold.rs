//! 帶位置的正規化 `fold_text_mapped`：既定的向量；字串逐字等於 `fold_text`
//! （全部的輸入，加上以固定種子隨機組合特例字元的一萬條字串）；每個位置都在原字串裡、落在字元邊界、起點不倒退；
//! `joins` 指到接回點。

use paperreader_core::references::{FOLD_VERSION, Folded, fold_text, fold_text_mapped};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// 檢查一條輸入的每一項性質，失敗時印出輸入。
fn check(s: &str) -> Folded {
    let f = fold_text_mapped(s);
    assert_eq!(
        f.text,
        fold_text(s),
        "text differs from fold_text for {s:?}"
    );
    let n = f.text.chars().count();
    assert_eq!(f.spans.len(), n, "one span per character of {s:?}");
    let mut last_start = 0;
    for (i, &(a, b)) in f.spans.iter().enumerate() {
        assert!(a < b && b <= s.len(), "span {i} {:?} of {s:?}", (a, b));
        assert!(
            s.is_char_boundary(a) && s.is_char_boundary(b),
            "span {i} {:?} is not on character boundaries of {s:?}",
            (a, b)
        );
        assert!(
            a >= last_start,
            "span {i} starts before the previous one in {s:?}"
        );
        last_start = a;
    }
    // 英數字：那一段原字自己正規化之後含有它（位置對到產生它的原字）
    for (c, &(a, b)) in f.text.chars().zip(&f.spans) {
        if c.is_ascii_alphanumeric() {
            assert!(
                fold_text(&s[a..b]).contains(c),
                "{c:?} maps to {:?} in {s:?}",
                &s[a..b]
            );
        }
    }
    // 被去重音拿掉的附加符號：前後都有交出的字時，它的位元組落在某一段裡（框到詞尾不切開字形）
    for (i, c) in s.char_indices() {
        let inside_text = f.spans.iter().any(|&(a, _)| a < i)
            && f.spans.iter().any(|&(_, b)| b > i + c.len_utf8());
        if is_combining_mark(c) && inside_text {
            assert!(
                f.spans
                    .iter()
                    .any(|&(a, b)| a <= i && i + c.len_utf8() <= b),
                "mark {c:?} at {i} is not covered in {s:?}: {:?}",
                f.spans
            );
        }
    }
    // 接回點：由小到大、在字串裡；原字串在它前面（去掉空白、軟連字號與被去重音拿掉的附加符號）是一個連字號
    assert!(
        f.joins.windows(2).all(|w| w[0] < w[1]),
        "joins sorted in {s:?}"
    );
    for &j in &f.joins {
        assert!(j < n, "join {j} beyond the text of {s:?}");
        let before = s[..f.spans[j].0]
            .trim_end_matches(|c: char| c.is_whitespace() || c == '\u{AD}' || is_combining_mark(c));
        let last = before.chars().last().expect("something before a join");
        let k: String = last.to_string().nfkc().collect();
        assert!(
            k.ends_with('-') || k.ends_with('\u{2010}'),
            "join {j} in {s:?} is not after a hyphen (found {last:?})"
        );
    }
    f
}

fn spans_of(f: &Folded) -> Vec<(usize, usize)> {
    f.spans.clone()
}

#[test]
fn fold_version_is_one() {
    assert_eq!(FOLD_VERSION, 1);
}

// ---------- 既定的向量 ----------

#[test]
fn ligature_maps_both_letters_to_the_ligature() {
    let f = check("\u{FB01}ne");
    assert_eq!(f.text, "fine");
    assert_eq!(spans_of(&f), [(0, 3), (0, 3), (3, 4), (4, 5)]);
    assert!(f.joins.is_empty());
}

#[test]
fn hyphen_before_space_and_lowercase_is_joined() {
    let f = check("detec- tion");
    assert_eq!(f.text, "detection");
    assert_eq!(f.joins, [5]);
    assert_eq!(f.spans[5], (7, 8));
    let f = check("real-\ntime");
    assert_eq!(f.text, "realtime");
    assert_eq!(f.joins, [4]);
}

#[test]
fn collapsed_space_covers_the_whole_run_with_the_soft_hyphen() {
    let f = check("a \u{00AD}\n b");
    assert_eq!(f.text, "a b");
    assert_eq!(f.spans[1], (1, 6));
    assert_eq!(f.spans[0], (0, 1));
    assert_eq!(f.spans[2], (6, 7));
}

#[test]
fn accents_map_to_the_letters_they_came_with() {
    let f = check("na\u{00EF}ve");
    assert_eq!(f.text, "naive");
    assert_eq!(f.spans[2], (2, 4));
    let f = check("nai\u{0308}ve");
    assert_eq!(f.text, "naive");
    assert_eq!(f.spans[2], (2, 5));
    assert_eq!(f.spans[3], (5, 6));
}

/// 沒有合成進前一個字、在去重音時拿掉的附加符號，併進前一個交出的字（只加長結尾）。
#[test]
fn removed_marks_extend_the_previous_letter() {
    let f = check("q\u{0301}");
    assert_eq!(f.text, "q");
    assert_eq!(spans_of(&f), [(0, 3)]);
    let f = check("x\u{0F77}y");
    assert_eq!(f.text, "xy");
    assert_eq!(spans_of(&f), [(0, 4), (4, 5)]);
    // 夾在空白後面的：併進那串空白
    let f = check("re-\n\u{0301}enter");
    assert_eq!(f.text, "re- enter");
    assert_eq!(f.spans[3], (3, 6));
}

#[test]
fn full_width_letters_map_one_by_one() {
    let f = check("\u{FF21}\u{FF50}\u{FF50}\u{FF4C}\u{FF45}");
    assert_eq!(f.text, "apple");
    assert_eq!(spans_of(&f), [(0, 3), (3, 6), (6, 9), (9, 12), (12, 15)]);
}

#[test]
fn letters_outside_the_basic_plane_count_as_one() {
    let f = check("\u{1D465} = 1");
    assert_eq!(f.text, "x = 1");
    assert_eq!(f.spans[0], (0, 4));
    assert_eq!(f.spans[1], (4, 5));
}

#[test]
fn final_sigma_follows_the_whole_string() {
    let f = check("ΟΔΟΣ ΣΑ");
    assert_eq!(f.text, "οδος σα");
    assert_eq!(f.text.chars().nth(3), Some('ς'));
    assert_eq!(f.spans[3], (6, 8));
}

#[test]
fn hyphen_before_uppercase_is_not_joined() {
    let f = check("RT-\nDETR");
    assert_eq!(f.text, "rt- detr");
    assert!(f.joins.is_empty());
}

// ---------- 全部的輸入 ----------

const INPUTS: &[&str] = &[
    // 單字、符號與連字號
    "\u{FB01}ne",
    "detec- tion",
    "real-\ntime",
    "a \u{00AD}\n b",
    "na\u{00EF}ve",
    "nai\u{0308}ve",
    "\u{FF21}\u{FF50}\u{FF50}\u{FF4C}\u{FF45}",
    "\u{1D465} = 1",
    "ΟΔΟΣ ΣΑ",
    "RT-\nDETR",
    // 句子、全形與混排
    "Real-time ob-\njects",
    "\u{FB01}ne-tuning",
    "\u{FF21}\u{FF50}\u{FF50}\u{FF4C}\u{FF45}\u{3000}\u{FF21}\u{FF30}\u{FF15}\u{FF10}",
    "RT-DETR 在 COCO 上達到 53.1 AP",
    "非極大值抑制（NMS）",
    "我們在COCO上達到",
    "Transformer模型的表現",
    "スーパー・モデル",
    "self-\nsupervised learning",
    "end-to-\nend",
    "state-of-the-\nart",
    "object detec-\ntion",
    "Let \u{1D465} denote",
    // 中文的詞
    "極大值",
    "detect",
    "抑制 NMS",
    "\"near field\"",
    "在COCO上",
    "Transformer模型",
    "self-supervised",
    "selfsupervised",
    "end-to-end",
    "end-\nto-end",
    "state-of-the-art",
    "\"real-time detection\"",
    "real-\ntime detection",
    "\"for real-time\"",
    "for real-\ntime",
    "detection",
    "\"object detection\"",
    "圖 3",
    "\u{1D465} denote",
    "示意圖",
    "\u{FF02}near\u{FF02}",
    "\u{201C}near field\u{201D}",
];

#[test]
fn every_appendix_c_input_folds_like_fold_text() {
    for s in INPUTS {
        check(s);
    }
    // 接回點：上面句子裡行尾斷開的幾個
    assert_eq!(check("Real-time ob-\njects").joins, [12]);
    assert_eq!(check("self-\nsupervised learning").joins, [4]);
    assert_eq!(check("end-to-\nend").joins, [6]);
    assert_eq!(check("end-\nto-end").joins, [3]);
    assert_eq!(check("state-of-the-\nart").joins, [12]);
    assert_eq!(check("object detec-\ntion").joins, [12]);
}

#[test]
fn cjk_and_mixed_text_map_each_character() {
    let f = check("我們在COCO上");
    assert_eq!(f.text, "我們在coco上");
    // 每個中文字 3 個位元組
    assert_eq!(f.spans[0], (0, 3));
    assert_eq!(f.spans[2], (6, 9));
    assert_eq!(f.spans[3], (9, 10));
    assert_eq!(f.spans[7], (13, 16));
}

#[test]
fn hangul_jamo_compose_and_keep_their_range() {
    // L、V、T 三個字母合成一個音節：涵蓋三個
    let f = check("\u{1100}\u{1161}\u{11A8}x");
    assert_eq!(f.text.chars().count(), 2);
    assert_eq!(f.spans[0], (0, 9));
    assert_eq!(f.spans[1], (9, 10));
    // L、附加符號、V：去重音之後才合成
    check("\u{1100}\u{0301}\u{1161}");
}

#[test]
fn reordered_marks_keep_starts_in_order() {
    // 兩個附加符號順序顛倒（標準排序會對調）
    check("a\u{0301}\u{0323}b");
    check("q\u{0301}\u{0323}");
    check("\u{0301}\u{0323}x");
}

// ---------- 隨機：一萬條 ----------

/// 固定種子的小亂數（xorshift64*），不加相依。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// 特例字元與會影響正規化的字（加上諺文字母、各種連字號與空白、Σ、İ）。
const ALPHABET: &[char] = &[
    'a',
    'b',
    'e',
    'i',
    'n',
    't',
    'x',
    'A',
    'R',
    'T',
    '1',
    '3',
    ' ',
    ' ',
    '\n',
    '\t',
    '-',
    '-',
    '\u{2010}',
    '\u{2011}',
    '\u{FE63}',
    '\u{FF0D}',
    '\u{00AD}',
    '\u{00A0}',
    '\u{3000}',
    '\u{FB01}',
    '\u{FB03}',
    '\u{00EF}',
    '\u{0308}',
    '\u{0301}',
    '\u{0323}',
    '\u{0345}',
    '\u{0130}',
    'Σ',
    'Ο',
    'Δ',
    'σ',
    '\u{FF21}',
    '\u{FF50}',
    '\u{1D465}',
    '\u{1100}',
    '\u{1161}',
    '\u{11A8}',
    '가',
    '中',
    '文',
    'ー',
    '\u{FF76}',
    '\u{FF9E}',
    '\u{01C5}',
    'ß',
    '\u{1E9E}',
    '①',
    '\u{338F}',
    '"',
    '\u{FF02}',
    '\u{201C}',
    '.',
    '(',
];

#[test]
fn ten_thousand_random_strings_fold_like_fold_text() {
    let mut rng = Rng(0x5EED_F01D_2026_1118);
    for _ in 0..10_000 {
        let len = rng.below(25);
        let s: String = (0..len)
            .map(|_| ALPHABET[rng.below(ALPHABET.len())])
            .collect();
        check(&s);
    }
}
