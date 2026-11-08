//! A small TeX lexer.
//!
//! TeX is not a conventional token language, but separating commands, braces
//! and script markers keeps the parser readable. Whitespace is emitted as a
//! token so `\text{...}` can preserve it.

use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Char(char),
    Command(String),
    LBrace,
    RBrace,
    Sup,
    Sub,
    Amp,
    Newline,
    Space,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Range<usize>,
}

pub fn lex(input: &str) -> Vec<Token> {
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let (start, ch) = chars[i];
        match ch {
            '%' => {
                // TeX comment: consume through the end of the line.
                while i < chars.len() && chars[i].1 != '\n' {
                    i += 1;
                }
                if i < chars.len() {
                    i += 1;
                }
                tokens.push(Token {
                    kind: TokenKind::Space,
                    span: start
                        ..chars
                            .get(i.saturating_sub(1))
                            .map_or(start, |(p, _)| *p + 1),
                });
            }
            '\\' => {
                i += 1;
                if i >= chars.len() {
                    tokens.push(Token {
                        kind: TokenKind::Command("\\".into()),
                        span: start..start + 1,
                    });
                    break;
                }
                let (next_start, next) = chars[i];
                if next == '\\' {
                    i += 1;
                    tokens.push(Token {
                        kind: TokenKind::Newline,
                        span: start..next_start + 1,
                    });
                } else if next.is_ascii_alphabetic() {
                    let mut name = String::new();
                    while i < chars.len() && chars[i].1.is_ascii_alphabetic() {
                        name.push(chars[i].1);
                        i += 1;
                    }
                    if i < chars.len() && chars[i].1 == '*' {
                        name.push('*');
                        i += 1;
                    }
                    let end = chars
                        .get(i.saturating_sub(1))
                        .map_or(next_start + 1, |(p, _)| *p + 1);
                    tokens.push(Token {
                        kind: TokenKind::Command(name),
                        span: start..end,
                    });
                } else {
                    i += 1;
                    tokens.push(Token {
                        kind: TokenKind::Command(next.to_string()),
                        span: start..next_start + 1,
                    });
                }
            }
            '{' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::LBrace,
                    span: start..start + 1,
                });
            }
            '}' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::RBrace,
                    span: start..start + 1,
                });
            }
            '^' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Sup,
                    span: start..start + 1,
                });
            }
            '_' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Sub,
                    span: start..start + 1,
                });
            }
            '&' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Amp,
                    span: start..start + 1,
                });
            }
            '~' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Space,
                    span: start..start + 1,
                });
            }
            c if c.is_whitespace() => {
                while i < chars.len() && chars[i].1.is_whitespace() {
                    i += 1;
                }
                let end = chars
                    .get(i.saturating_sub(1))
                    .map_or(start + 1, |(p, _)| *p + 1);
                tokens.push(Token {
                    kind: TokenKind::Space,
                    span: start..end,
                });
            }
            _ => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Char(ch),
                    span: start..start + ch.len_utf8(),
                });
            }
        }
    }

    let end = input.len();
    tokens.push(Token {
        kind: TokenKind::Eof,
        span: end..end,
    });
    tokens
}
