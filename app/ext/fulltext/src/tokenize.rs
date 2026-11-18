//! 切詞。索引與找位置共用同一個函式，記號序就和 FTS5 裡的位置一致。

use paperreader_core::references::Folded;

/// 一個記號：正規化之後的字，與它在 `Folded.text` 裡的字元範圍 `[start, end)`（字元序，不是位元組）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

/// 兩列記號：`main` 進 FTS5 的 `t` 欄；`joined`（行尾連字號接起來的記號）進 `j` 欄。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tokens {
    pub main: Vec<Token>,
    pub joined: Vec<Token>,
}

impl Tokens {
    /// 交給 FTS5 的兩欄：記號以空白接起來（`ascii` 分詞器把非 ASCII 的字當記號的一部分）。
    pub fn columns(&self) -> (String, String) {
        let join = |v: &[Token]| {
            v.iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        };
        (join(&self.main), join(&self.joined))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    /// 中日韓字：每兩字一組。
    Cjk,
    /// 字母數字：連續的是一個記號。
    Word,
    /// 空白、標點、符號、連字號。
    Sep,
}

/// 中日韓字的區段。
fn in_cjk_block(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF
        | 0x3400..=0x4DBF
        | 0x20000..=0x3FFFF
        | 0xF900..=0xFAFF
        | 0x3005..=0x3007
        | 0x303B
        | 0x3040..=0x309F
        | 0x30A0..=0x30FF
        | 0x31F0..=0x31FF
        | 0xAC00..=0xD7AF
        | 0x1100..=0x11FF
        | 0x3130..=0x318F
        | 0xA960..=0xA97F
        | 0xD7B0..=0xD7FF)
}

/// 字元分類：先看中日韓字（Alphabetic 而且在區段裡），再看字母數字，其餘是分隔。
pub(crate) fn class(c: char) -> Class {
    if c.is_alphabetic() && in_cjk_block(c) {
        Class::Cjk
    } else if c.is_alphanumeric() {
        Class::Word
    } else {
        Class::Sep
    }
}

/// 切詞：交出主列與接回列，每個記號帶它在 `f.text` 裡的字元範圍。
pub fn tokens(f: &Folded) -> Tokens {
    let chars: Vec<char> = f.text.chars().collect();
    tokenize(&chars, &f.joins)
}

/// `tokens` 的本體：`joins` 是接回點之後第一個字的字元序（由小到大）。
pub(crate) fn tokenize(chars: &[char], joins: &[usize]) -> Tokens {
    let mut out = Tokens::default();
    let push = |row: &mut Vec<Token>, s: usize, e: usize| {
        row.push(Token {
            text: chars[s..e].iter().collect(),
            start: s,
            end: e,
        })
    };
    let mut i = 0;
    while i < chars.len() {
        match class(chars[i]) {
            Class::Sep => i += 1,
            Class::Cjk => {
                let s = i;
                while i < chars.len() && class(chars[i]) == Class::Cjk {
                    i += 1;
                }
                // 相鄰兩字一組，再加這串的最後一個字（一串裡每個字都是某個記號的開頭）
                for k in s..i - 1 {
                    push(&mut out.main, k, k + 2);
                }
                push(&mut out.main, i - 1, i);
            }
            Class::Word => {
                let s = i;
                let mut cut = s;
                let mut joined = false;
                i += 1;
                while i < chars.len() && class(chars[i]) == Class::Word {
                    // 接回點在一串字母數字的中間：主列從這裡切開，接起來的整串另放接回列
                    if joins.binary_search(&i).is_ok() {
                        push(&mut out.main, cut, i);
                        cut = i;
                        joined = true;
                    }
                    i += 1;
                }
                push(&mut out.main, cut, i);
                if joined {
                    push(&mut out.joined, s, i);
                }
            }
        }
    }
    out
}
