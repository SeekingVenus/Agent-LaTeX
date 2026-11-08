//! Safe extraction of math spans from Markdown-ish conversation text.
//!
//! The scanner skips fenced code and inline code, rejects currency-like `$`
//! pairs, and keeps the original text untouched outside math spans.

pub use latex_stream::MathDelim;
use latex_stream::{
    find_unescaped, is_closing_fence_line, is_escaped, parse_fence_line, valid_inline,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Math {
        source: String,
        display: bool,
        delim: MathDelim,
    },
}

pub fn extract(input: &str) -> Vec<Segment> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut text_start = 0usize;
    let mut i = 0usize;

    while i < input.len() {
        // Fenced and indented code blocks are only recognized at the start of
        // a line.
        if i == 0 || bytes.get(i.wrapping_sub(1)) == Some(&b'\n') {
            if let Some(end) = indented_code_end(input, i) {
                i = end;
                continue;
            }
            if let Some(fence) = fence_at(input, i) {
                if fence.is_math {
                    push_text(&mut out, &input[text_start..i]);
                    out.push(Segment::Math {
                        source: fence.content.trim().to_string(),
                        display: true,
                        delim: MathDelim::Fence,
                    });
                    i = fence.end;
                    text_start = i;
                } else {
                    i = fence.end;
                }
                continue;
            }
        }

        let Some(ch) = input[i..].chars().next() else {
            break;
        };

        match ch {
            '`' => {
                // Inline code: skip the matching run of backticks.
                let run = ch.len_utf8() * input[i..].chars().take_while(|c| *c == '`').count();
                if let Some(rel) = find_pattern(input, i + run, &"`".repeat(run / ch.len_utf8())) {
                    i = rel + run;
                } else {
                    i += run;
                }
            }
            '\\' => {
                // Escaped delimiter or display delimiter.
                let next = input[i + 1..].chars().next();
                match next {
                    Some('$') => i += 2,
                    Some('[') => {
                        if let Some(close) = find_unescaped(input, i + 2, "\\]") {
                            push_text(&mut out, &input[text_start..i]);
                            out.push(Segment::Math {
                                source: input[i + 2..close].to_string(),
                                display: true,
                                delim: MathDelim::Bracket,
                            });
                            i = close + 2;
                            text_start = i;
                        } else {
                            i += 2;
                        }
                    }
                    Some('(') => {
                        if let Some(close) = find_unescaped(input, i + 2, "\\)") {
                            push_text(&mut out, &input[text_start..i]);
                            out.push(Segment::Math {
                                source: input[i + 2..close].to_string(),
                                display: false,
                                delim: MathDelim::Paren,
                            });
                            i = close + 2;
                            text_start = i;
                        } else {
                            i += 2;
                        }
                    }
                    _ => {
                        i += 1 + next.map_or(0, char::len_utf8);
                    }
                }
            }
            '$' if input[i..].starts_with("$$") => {
                if let Some(close) = find_unescaped(input, i + 2, "$$") {
                    push_text(&mut out, &input[text_start..i]);
                    out.push(Segment::Math {
                        source: input[i + 2..close].to_string(),
                        display: true,
                        delim: MathDelim::DoubleDollar,
                    });
                    i = close + 2;
                    text_start = i;
                } else {
                    i += 2;
                }
            }
            '$' => {
                if let Some(close) = find_inline_close(input, i + 1) {
                    let content = &input[i + 1..close];
                    if valid_inline(content) {
                        push_text(&mut out, &input[text_start..i]);
                        out.push(Segment::Math {
                            source: content.to_string(),
                            display: false,
                            delim: MathDelim::Dollar,
                        });
                        i = close + 1;
                        text_start = i;
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            _ => i += ch.len_utf8(),
        }
    }

    push_text(&mut out, &input[text_start..]);
    out
}

fn push_text(out: &mut Vec<Segment>, text: &str) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(Segment::Text(previous)) => previous.push_str(text),
        _ => out.push(Segment::Text(text.to_string())),
    }
}

/// End of a Markdown indented code block, when `start` begins one. Recognized
/// only in the common form: a 4-space or tab line after a blank line (or at
/// the document start). Content stays text; only scanning is skipped.
///
/// shortcut: list content indented four spaces is treated as code too. Add real
/// list tracking if math inside deep list items needs to render.
fn indented_code_end(input: &str, start: usize) -> Option<usize> {
    if start > 0 {
        let before = &input[..start - 1];
        let previous_start = before.rfind('\n').map(|offset| offset + 1).unwrap_or(0);
        if !before[previous_start..].trim().is_empty() {
            return None;
        }
    }
    let first_end = input[start..]
        .find('\n')
        .map(|offset| start + offset)
        .unwrap_or(input.len());
    let first = &input[start..first_end];
    if !(first.starts_with("    ") || first.starts_with('\t')) {
        return None;
    }

    let mut position = start;
    let mut end = first_end;
    loop {
        let line_end = input[position..]
            .find('\n')
            .map(|offset| position + offset)
            .unwrap_or(input.len());
        let line = &input[position..line_end];
        if !(line.trim().is_empty() || line.starts_with("    ") || line.starts_with('\t')) {
            break;
        }
        end = if line_end < input.len() {
            line_end + 1
        } else {
            line_end
        };
        if line_end >= input.len() {
            break;
        }
        position = line_end + 1;
    }
    Some(end)
}

struct Fence {
    is_math: bool,
    content: String,
    end: usize,
}

fn fence_at(input: &str, start: usize) -> Option<Fence> {
    let line_end = input[start..]
        .find('\n')
        .map(|offset| start + offset)
        .unwrap_or(input.len());
    let line = &input[start..line_end];
    let (marker, marker_len, info) = parse_fence_line(line)?;

    let language = info
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let is_math = matches!(
        language.as_str(),
        "latex" | "math" | "tex" | "tex-math" | "katex"
    );

    let content_start = if line_end < input.len() {
        line_end + 1
    } else {
        line_end
    };
    let closing = find_closing_fence(input, content_start, marker, marker_len);
    let (content_end, end) = closing.unwrap_or((input.len(), input.len()));

    Some(Fence {
        is_math,
        content: input[content_start..content_end].to_string(),
        end,
    })
}

fn find_closing_fence(
    input: &str,
    start: usize,
    marker: char,
    marker_len: usize,
) -> Option<(usize, usize)> {
    let mut line_start = start;
    while line_start <= input.len() {
        let line_end = input[line_start..]
            .find('\n')
            .map(|offset| line_start + offset)
            .unwrap_or(input.len());
        let line = &input[line_start..line_end];
        if is_closing_fence_line(line, marker, marker_len) {
            let end = if line_end < input.len() {
                line_end + 1
            } else {
                line_end
            };
            return Some((line_start, end));
        }
        if line_end >= input.len() {
            break;
        }
        line_start = line_end + 1;
    }
    None
}

fn find_pattern(input: &str, start: usize, pattern: &str) -> Option<usize> {
    input[start..].find(pattern).map(|offset| start + offset)
}

fn find_inline_close(input: &str, start: usize) -> Option<usize> {
    let mut cursor = start;
    while cursor < input.len() {
        let ch = input[cursor..].chars().next()?;
        match ch {
            '\n' => return None,
            '\\' => {
                cursor += 1 + input[cursor + 1..].chars().next().map_or(0, char::len_utf8);
            }
            '$' => {
                if !is_escaped(input, cursor) {
                    return Some(cursor);
                }
                cursor += 1;
            }
            _ => cursor += ch.len_utf8(),
        }
    }
    None
}
