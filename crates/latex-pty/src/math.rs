//! Find math spans in the visible screen.
//!
//! Inline spans in prose use the compact one-line renderer. Spans that occupy a
//! whole row (or display math) can use the full 2D renderer when the rows below
//! are blank and the cursor is not in the input area.

use latex_core::ast::Align;
use latex_stream::{find_unescaped, is_escaped, valid_inline};
use latex_term::{render, Fallback, Mode, Options, Theme};
use unicode_width::UnicodeWidthStr;
use vt100::Screen;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub row: u16,
    pub start: u16,
    pub end: u16,
    pub width: u16,
    /// Whether the rest of the row may be shifted left. False on rows that
    /// look like they contain a second pane/overlay to the right.
    pub shift: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockReplacement {
    pub row: u16,
    pub prefix_row: u16,
    pub start: u16,
    pub width: u16,
    pub height: u16,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MathReplacements {
    pub inline: Vec<Replacement>,
    pub blocks: Vec<BlockReplacement>,
}

/// How aggressively `$...$` is treated as math.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathPolicy {
    /// Claude's Markdown display can strip math delimiter and row-break escapes.
    Claude,
    /// Agent prose: `$x$` and `$n$` are math.
    Agent,
    /// Generic command output: only unambiguous math, never shell variables.
    Generic,
}

impl MathPolicy {
    /// Conservative detection also looks at what surrounds the span. Shell
    /// assignments such as `PATH=$HOME/bin` put a delimiter right after `=`,
    /// while prose math is surrounded by whitespace or punctuation.
    pub fn accepts_inline_at(
        self,
        source: &str,
        before: Option<char>,
        after: Option<char>,
    ) -> bool {
        match self {
            // Agent prose stays aggressive: math next to operators, CJK text
            // or borders must still render. Only shell-like attachment is
            // rejected (`PATH=$HOME:$PATH`, `$A` next to a word).
            Self::Agent | Self::Claude => agent_boundary_ok(before, after),
            Self::Generic => context_ok(before, after) && looks_like_math(source),
        }
    }
}

fn agent_boundary_ok(before: Option<char>, after: Option<char>) -> bool {
    let left_bad = before
        .map(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '=')
        .unwrap_or(false);
    let right_bad = after
        .map(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        .unwrap_or(false);
    !left_bad && !right_bad
}

fn context_ok(before: Option<char>, after: Option<char>) -> bool {
    let before_ok = before
        .map(|ch| {
            ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | ',' | ';' | ':' | '"' | '\'')
        })
        .unwrap_or(true);
    let after_ok = after
        .map(|ch| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ')' | ']' | '}' | ',' | '.' | ';' | ':' | '!' | '?' | '"' | '\''
                )
        })
        .unwrap_or(true);
    before_ok && after_ok
}

fn looks_like_math(source: &str) -> bool {
    let source = source.trim();
    if source.is_empty() || is_shell_variable(source) || is_plain_number(source) {
        return false;
    }
    source.chars().any(|ch| {
        matches!(
            ch,
            '\\' | '^'
                | '_'
                | '{'
                | '}'
                | '='
                | '+'
                | '-'
                | '*'
                | '/'
                | '<'
                | '>'
                | '('
                | ')'
                | '['
                | ']'
                | '|'
                | '±'
                | '×'
                | '÷'
                | '≤'
                | '≥'
                | '≠'
                | '∞'
                | '√'
                | '∑'
                | '∫'
                | '∂'
                | 'π'
                | 'Δ'
                | 'Θ'
                | 'Λ'
                | 'Σ'
                | 'Ω'
                | 'α'
                | 'β'
                | 'γ'
                | 'δ'
                | 'θ'
                | 'λ'
                | 'μ'
                | 'σ'
                | 'φ'
                | 'ψ'
                | 'ω'
        )
    })
}

fn is_shell_variable(source: &str) -> bool {
    let mut chars = source.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if source.chars().count() == 1 {
        return !first.is_ascii_alphabetic() || first == '_';
    }
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_plain_number(source: &str) -> bool {
    !source.is_empty()
        && source
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ',' | ' '))
}

pub fn find_math(screen: &Screen) -> MathReplacements {
    find_math_with_policy(screen, MathPolicy::Agent)
}

pub fn find_math_with_policy(screen: &Screen, policy: MathPolicy) -> MathReplacements {
    find_math_internal(screen, policy)
}

fn find_math_internal(screen: &Screen, policy: MathPolicy) -> MathReplacements {
    let (rows, cols) = screen.size();
    let rows = rows.get();
    let cols = cols.get();
    let cursor_row = screen.cursor_position().0;
    let rows = detect_input_start(screen, cursor_row).min(rows);
    let rows_data: Vec<(String, Vec<u16>)> =
        (0..rows).map(|row| row_text(screen, row, cols)).collect();
    let mut result = MathReplacements::default();
    let mut occupied = vec![false; usize::from(rows)];

    result.blocks.extend(find_multiline_display(
        screen,
        &rows_data,
        rows,
        cols,
        cursor_row,
        &mut occupied,
        policy,
    ));

    result.inline.extend(find_wrapped_inline(
        screen,
        &rows_data,
        rows,
        cols,
        cursor_row,
        &mut occupied,
        policy,
    ));

    for row in 0..rows {
        if occupied[usize::from(row)] {
            continue;
        }
        let (text, col_at_byte) = &rows_data[usize::from(row)];
        let (inline, blocks) = find_in_row(
            screen,
            row,
            rows,
            cols,
            cursor_row,
            text,
            col_at_byte,
            &mut occupied,
            policy,
        );
        result.inline.extend(inline);
        result.blocks.extend(blocks);
    }

    result
}

pub fn find_inline_math(screen: &Screen) -> Vec<Replacement> {
    find_math(screen).inline
}

fn unclosed_inline_open(text: &str) -> Option<(usize, usize, &'static str)> {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if text[index..].starts_with("\\(") && !is_escaped(text, index) {
            match find_unescaped(text, index + 2, "\\)") {
                Some(close) => {
                    index = close + 2;
                    continue;
                }
                None => return Some((index, 2, "\\)")),
            }
        }
        if bytes[index] != b'$' || is_escaped(text, index) {
            let ch = text[index..].chars().next().unwrap();
            index += ch.len_utf8();
            continue;
        }
        if text[index..].starts_with("$$") {
            let close = find_unescaped(text, index + 2, "$$")?;
            index = close + 2;
            continue;
        }
        if let Some(close) = find_unescaped(text, index + 1, "$") {
            index = close + 1;
            continue;
        }
        let rest = &text[index + 1..];
        if rest.is_empty() || rest.chars().next().map(char::is_whitespace).unwrap_or(true) {
            return None;
        }
        return Some((index, 1, "$"));
    }
    None
}

/// Handle an inline `$...$` formula whose closing delimiter was pushed onto a
/// following wrapped row. The replacement rewraps only the affected rows and
/// never changes their count, so it also works while the transcript is
/// scrolled and reflow is disabled.
#[allow(clippy::too_many_arguments)]
fn find_wrapped_inline(
    screen: &Screen,
    rows_data: &[(String, Vec<u16>)],
    rows: u16,
    cols: u16,
    cursor_row: u16,
    occupied: &mut [bool],
    policy: MathPolicy,
) -> Vec<Replacement> {
    let mut out = Vec::new();
    let mut row = 0usize;
    while row < usize::from(rows) {
        if occupied[row] {
            row += 1;
            continue;
        }
        let (text, _) = &rows_data[row];
        let trimmed = text.trim_start();
        let prefix_len = text.len() - trimmed.len();
        if !text[..prefix_len].chars().all(|ch| ch == ' ') {
            row += 1;
            continue;
        }
        let Some((open_byte, source_offset, close_pattern)) = unclosed_inline_open(text) else {
            row += 1;
            continue;
        };
        let source_start = open_byte + source_offset;
        let first_raw = &text[source_start..];
        let mut trailing = vec![first_raw.ends_with(char::is_whitespace)];
        let mut parts = vec![first_raw.trim_end().to_string()];
        let mut end_row = row;
        let mut suffix = None;
        let mut next = row + 1;
        // shortcut: only join up to 32 wrapped rows; a single inline formula
        // taller than that is usually not worth rendering. Raise when reports
        // show narrow terminals needing more.
        while next < usize::from(rows) && next <= row + 32 {
            if occupied[next] {
                break;
            }

            let (line, _) = &rows_data[next];
            if transcript_boundary(screen, next as u16, cols, line) {
                break;
            }
            if line.trim().is_empty() || !line.starts_with(&text[..prefix_len]) {
                break;
            }
            let body_raw = &line[prefix_len..];
            let body = body_raw.trim_end();
            if body.is_empty() {
                break;
            }
            trailing.push(body_raw.ends_with(char::is_whitespace));
            if let Some(close) = find_unescaped(body, 0, close_pattern) {
                parts.push(body[..close].trim().to_string());
                suffix = Some(body[close + close_pattern.len()..].to_string());
                end_row = next;
                break;
            }
            parts.push(body.to_string());
            end_row = next;
            next += 1;
        }
        let Some(suffix) = suffix else {
            row += 1;
            continue;
        };
        if usize::from(cursor_row) >= row && usize::from(cursor_row) <= end_row {
            // The cursor is inside the wrapped formula: keep it raw while the
            // user is editing it.
            row = end_row + 1;
            continue;
        }
        let formula = join_wrapped_parts(&parts, &trailing);
        let formula = formula.trim();
        let before = text[..open_byte].chars().next_back();
        let after = suffix.chars().next();
        let is_paren = close_pattern == "\\)";
        if !valid_inline(formula)
            || (!is_paren && !policy.accepts_inline_at(formula, before, after))
        {
            row += 1;
            continue;
        }
        let end_col = (row..=end_row)
            .map(|row| content_end(screen, row as u16, prefix_len as u16, cols))
            .min()
            .unwrap_or(cols);
        let available = usize::from(end_col.saturating_sub(prefix_len as u16));
        if available < 4 {
            row += 1;
            continue;
        }
        let rendered = render(
            formula,
            &Options {
                mode: Mode::Inline,
                theme: Theme::mono(),
                max_width: Some(available),
                align: Align::Left,
                indent: 0,
                fallback: Fallback::Source,
            },
        );
        if rendered.fallback || rendered.text.contains('\n') || rendered.width == 0 {
            row += 1;
            continue;
        }

        let mut paragraph = String::new();
        paragraph.push_str(&text[prefix_len..open_byte]);
        paragraph.push_str(&rendered.text);
        paragraph.push_str(&suffix);
        let paragraph = render_paragraph_inline(&paragraph, available, policy);
        let Some(lines) = wrap_text(paragraph.trim_end(), available) else {
            row += 1;
            continue;
        };
        let span = end_row - row + 1;
        if lines.len() > span {
            row += 1;
            continue;
        }
        for (offset, line) in lines.iter().enumerate() {
            out.push(Replacement {
                row: (row + offset) as u16,
                start: prefix_len as u16,
                end: end_col,
                width: UnicodeWidthStr::width(line.as_str()) as u16,
                shift: false,
                text: line.clone(),
            });
            occupied[row + offset] = true;
        }
        for extra in row + lines.len()..=end_row {
            out.push(Replacement {
                row: extra as u16,
                start: prefix_len as u16,
                end: end_col,
                width: 0,
                shift: false,
                text: String::new(),
            });
        }
        for slot in &mut occupied[row + lines.len()..=end_row] {
            *slot = true;
        }
        row = end_row + 1;
    }
    out
}

/// Render every inline formula that remains in a paragraph before rewrapping
/// it. The paragraph rows are rewritten as a whole, so formulas that were
/// closed on a single row would otherwise stay raw.
fn join_wrapped_parts(parts: &[String], trailing_space: &[bool]) -> String {
    let mut out = String::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0
            && trailing_space.get(index - 1).copied().unwrap_or(false)
            && !part.starts_with(char::is_whitespace)
            && !out.ends_with(char::is_whitespace)
        {
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

fn append_math_rows(source: &mut String, part: &str, policy: MathPolicy) {
    if part.is_empty() {
        return;
    }
    if policy == MathPolicy::Claude
        && source.ends_with(" \\")
        && source.rfind("\\begin{") > source.rfind("\\end{")
    {
        source.push('\\');
    }
    if !source.is_empty() && !looks_like_split_command(source, part) {
        source.push(' ');
    }
    source.push_str(part);
}

/// Join split commands, including a complete prefix such as `\mid` + `dle`.
fn looks_like_split_command(text: &str, part: &str) -> bool {
    // Only the end can be a split command; cap the backward scan so a tall
    // block does not rescan its whole source for every row.
    let tail_start = text
        .char_indices()
        .rev()
        .nth(63)
        .map(|(index, _)| index)
        .unwrap_or(0);
    let tail = &text[tail_start..];
    let Some((index, _)) = tail.char_indices().rev().find(|(_, ch)| *ch == '\\') else {
        return false;
    };
    let name = &tail[index + 1..];
    if is_escaped(tail, index) {
        return false;
    }
    // A soft wrap may separate the backslash itself from the command name.
    if name.is_empty() {
        return true;
    }
    if !name.chars().all(|ch| ch.is_ascii_alphabetic() || ch == '*') {
        return false;
    }
    // Ask the parser so structural commands use the same vocabulary as symbols.
    let unsupported = |name: &str| {
        let warning = format!("unsupported command \\{name}");
        latex_core::parse(&format!("\\{name}"))
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == warning)
    };
    let continuation: String = part.chars().take_while(char::is_ascii_alphabetic).collect();
    (!continuation.is_empty() && !unsupported(&format!("{name}{continuation}")))
        || unsupported(name)
}

fn render_paragraph_inline(text: &str, width: usize, policy: MathPolicy) -> String {
    let mut out = String::new();
    let mut index = 0usize;
    while index < text.len() {
        let bytes = text.as_bytes();
        if bytes[index] == b'$' && !is_escaped(text, index) {
            if text[index..].starts_with("$$") {
                if let Some(close) = find_unescaped(text, index + 2, "$$") {
                    let source = &text[index + 2..close];
                    if append_compact(&mut out, source, width) {
                        index = close + 2;
                        continue;
                    }
                }
            } else if let Some(close) = find_unescaped(text, index + 1, "$") {
                let source = &text[index + 1..close];
                if valid_inline(source)
                    && policy.accepts_inline_at(
                        source,
                        text[..index].chars().next_back(),
                        text[close + 1..].chars().next(),
                    )
                    && append_compact(&mut out, source, width)
                {
                    index = close + 1;
                    continue;
                }
            }
        } else if bytes[index] == b'\\' && text[index..].starts_with("\\(") {
            if let Some(close) = find_unescaped(text, index + 2, "\\)") {
                let source = &text[index + 2..close];
                if valid_inline(source) && append_compact(&mut out, source, width) {
                    index = close + 2;
                    continue;
                }
            }
        }
        let ch = text[index..].chars().next().unwrap();
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn append_compact(out: &mut String, source: &str, width: usize) -> bool {
    let rendered = render(
        source,
        &Options {
            mode: Mode::Inline,
            theme: Theme::mono(),
            max_width: Some(width),
            align: Align::Left,
            indent: 0,
            fallback: Fallback::Source,
        },
    );
    if rendered.fallback || rendered.text.contains('\n') || rendered.width == 0 {
        return false;
    }
    out.push_str(&rendered.text);
    true
}

fn wrap_text(text: &str, width: usize) -> Option<Vec<String>> {
    if width == 0 {
        return None;
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for word in text.split(' ').filter(|word| !word.is_empty()) {
        let word_width = UnicodeWidthStr::width(word);
        if word_width > width {
            return None;
        }
        if current.is_empty() {
            current.push_str(word);
            current_width = word_width;
        } else if current_width + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
            current_width = word_width;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    Some(lines)
}

fn row_text(screen: &Screen, row: u16, cols: u16) -> (String, Vec<u16>) {
    let mut text = String::new();
    let mut col_at_byte = Vec::new();

    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            continue;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        let contents = cell.contents();
        if contents.is_empty() {
            text.push(' ');
            col_at_byte.push(col);
            continue;
        }
        for ch in contents.chars() {
            let start = text.len();
            text.push(ch);
            for _ in start..text.len() {
                col_at_byte.push(col);
            }
        }
    }

    (text, col_at_byte)
}

#[allow(clippy::too_many_arguments)]
fn find_in_row(
    screen: &Screen,
    row: u16,
    rows: u16,
    cols: u16,
    cursor_row: u16,
    text: &str,
    col_at_byte: &[u16],
    occupied: &mut [bool],
    policy: MathPolicy,
) -> (Vec<Replacement>, Vec<BlockReplacement>) {
    let mut inline = Vec::new();
    let mut blocks = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0usize;

    while index < bytes.len() {
        if policy == MathPolicy::Claude && bytes[index] == b'(' {
            let mut depth = 1;
            let close = text[index + 1..].char_indices().find_map(|(offset, ch)| {
                if ch == '(' {
                    depth += 1;
                }
                if ch == ')' {
                    depth -= 1;
                }
                (depth == 0).then_some(index + 1 + offset)
            });
            if let Some(close) = close {
                let source = &text[index + 1..close];
                if source.contains('\\') && latex_core::parse(source).diagnostics.is_empty() {
                    try_inline(
                        screen,
                        row,
                        rows,
                        cols,
                        cursor_row,
                        text,
                        col_at_byte,
                        index,
                        index + 1,
                        close,
                        &mut inline,
                        &mut blocks,
                        occupied,
                        policy,
                    );
                    index = close + 1;
                    continue;
                }
            }
        }
        if bytes[index] == b'\\' {
            // \[ ... \] and \( ... \)
            if text[index..].starts_with("\\[") {
                if let Some(close) = find_unescaped(text, index + 2, "\\]") {
                    try_display(
                        screen,
                        row,
                        rows,
                        cols,
                        cursor_row,
                        text,
                        col_at_byte,
                        index,
                        index + 2,
                        close,
                        &mut inline,
                        &mut blocks,
                        occupied,
                    );
                    index = close + 2;
                    continue;
                }
            }
            if text[index..].starts_with("\\(") {
                if let Some(close) = find_unescaped(text, index + 2, "\\)") {
                    try_inline(
                        screen,
                        row,
                        rows,
                        cols,
                        cursor_row,
                        text,
                        col_at_byte,
                        index,
                        index + 2,
                        close,
                        &mut inline,
                        &mut blocks,
                        occupied,
                        policy,
                    );
                    index = close + 2;
                    continue;
                }
            }
            index += 1;
            continue;
        }

        if bytes[index] != b'$' || is_escaped(text, index) {
            index += 1;
            continue;
        }

        if text[index..].starts_with("$$") {
            if let Some(close) = find_unescaped(text, index + 2, "$$") {
                try_display(
                    screen,
                    row,
                    rows,
                    cols,
                    cursor_row,
                    text,
                    col_at_byte,
                    index,
                    index + 2,
                    close,
                    &mut inline,
                    &mut blocks,
                    occupied,
                );
                index = close + 2;
            } else {
                // No closing delimiter on this row: a multi-row display is
                // handled by the reflow/block scanners, and an inline formula
                // wrapped onto the next row is handled by
                // `find_wrapped_inline`.
                break;
            }
            continue;
        }

        let Some(close) = find_unescaped(text, index + 1, "$") else {
            break;
        };
        try_inline(
            screen,
            row,
            rows,
            cols,
            cursor_row,
            text,
            col_at_byte,
            index,
            index + 1,
            close,
            &mut inline,
            &mut blocks,
            occupied,
            policy,
        );
        index = close + 1;
    }

    (inline, blocks)
}

#[allow(clippy::too_many_arguments)]
fn try_inline(
    screen: &Screen,
    row: u16,
    rows: u16,
    cols: u16,
    cursor_row: u16,
    text: &str,
    col_at_byte: &[u16],
    start: usize,
    source_start: usize,
    close: usize,
    inline: &mut Vec<Replacement>,
    blocks: &mut Vec<BlockReplacement>,
    occupied: &mut [bool],
    policy: MathPolicy,
) {
    let source = &text[source_start..close];
    if !valid_inline(source) || matches!(source.trim(), "..." | "…") {
        return;
    }
    let Some(&start_col) = col_at_byte.get(start) else {
        return;
    };
    let Some(&close_col) = col_at_byte.get(close) else {
        return;
    };
    let closing_width = if text[start..].starts_with("\\(") {
        2
    } else {
        1
    };
    let end_col = close_col + closing_width;
    if end_col <= start_col {
        return;
    }
    let end = close + usize::from(closing_width);
    let is_paren = text[start..].starts_with("\\(");
    if !is_paren
        && !policy.accepts_inline_at(
            source,
            text[..start].chars().next_back(),
            text[end..].chars().next(),
        )
    {
        return;
    }

    if is_standalone(text, start, end) && !(row == cursor_row && row >= rows.saturating_sub(3)) {
        if let Some(block) = try_block(screen, row, rows, cols, start_col, source, occupied) {
            blocks.push(block);
            return;
        }
    }

    let span = usize::from(end_col - start_col);
    let options = Options {
        mode: Mode::Inline,
        theme: Theme::mono(),
        max_width: Some(span),
        align: Align::Left,
        indent: 0,
        fallback: Fallback::Source,
    };
    let rendered = render(source, &options);
    let width = UnicodeWidthStr::width(rendered.text.as_str());
    if !rendered.fallback && !rendered.text.contains('\n') && width > 0 && width <= span {
        inline.push(Replacement {
            row,
            start: start_col,
            end: end_col,
            width: width as u16,
            shift: shift_safe(&text[end..]) && content_end(screen, row, start_col, cols) == cols,
            text: rendered.text,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn try_display(
    screen: &Screen,
    row: u16,
    rows: u16,
    cols: u16,
    cursor_row: u16,
    text: &str,
    col_at_byte: &[u16],
    start: usize,
    source_start: usize,
    close: usize,
    inline: &mut Vec<Replacement>,
    blocks: &mut Vec<BlockReplacement>,
    occupied: &mut [bool],
) {
    let source = &text[source_start..close];
    if source.trim().is_empty() || matches!(source.trim(), "..." | "…") {
        return;
    }
    let end = close + 2;
    let Some(&start_col) = col_at_byte.get(start) else {
        return;
    };

    // A display formula on the cursor row is part of the editable input. Leave
    // it raw so the cursor and line breaks stay consistent.
    if row == cursor_row {
        return;
    }

    // A 2D block is only safe for a standalone display formula.
    let can_block = is_standalone(text, start, end);
    if can_block {
        if let Some(block) = try_block(screen, row, rows, cols, start_col, source, occupied) {
            blocks.push(block);
            return;
        }
    }

    // No room for the 2D form. Fall back to a compact one-line render rather
    // than leaving raw `$$...$$` in the conversation.
    let Some(&close_col) = col_at_byte.get(close) else {
        return;
    };
    let end_col = close_col + 2;
    if end_col <= start_col {
        return;
    }
    let span = usize::from(end_col - start_col);
    let options = Options {
        mode: Mode::Inline,
        theme: Theme::mono(),
        max_width: Some(span),
        align: Align::Left,
        indent: 0,
        fallback: Fallback::Source,
    };
    let rendered = render(source, &options);
    let width = UnicodeWidthStr::width(rendered.text.as_str());
    if !rendered.fallback && !rendered.text.contains('\n') && width > 0 && width <= span {
        inline.push(Replacement {
            row,
            start: start_col,
            end: end_col,
            width: width as u16,
            shift: shift_safe(&text[end..]) && content_end(screen, row, start_col, cols) == cols,
            text: rendered.text,
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflowBlock {
    pub row: u16,
    pub source_rows: u16,
    pub start: u16,
    pub source: String,
}

/// Find display-math spans in the conversation area without applying the
/// normal "does it fit in blank rows" constraint. The compositor uses these to
/// reserve the full 2D height and reflow the conversation below.
#[allow(clippy::too_many_arguments)]
pub fn find_reflow_blocks(
    screen: &Screen,
    conversation_end: u16,
    cursor_row: u16,
    policy: MathPolicy,
) -> Vec<ReflowBlock> {
    let (rows, cols) = screen.size();
    let rows = rows.get();
    let cols = cols.get();
    let rows_data: Vec<(String, Vec<u16>)> =
        (0..rows).map(|row| row_text(screen, row, cols)).collect();
    let mut out = Vec::new();
    let mut row = 0usize;

    while row < usize::from(conversation_end) {
        let (text, col_at_byte) = &rows_data[row];
        let Some((open, close_pattern, source_offset)) =
            display_open(text, policy).or_else(|| dollar_open(text))
        else {
            row += 1;
            continue;
        };
        let prefix = &text[..open];
        if !is_decoration_only(prefix) {
            row += 1;
            continue;
        }
        let source_start = open + source_offset;
        let Some(&start_col) = col_at_byte.get(open) else {
            row += 1;
            continue;
        };

        if let Some(close) = find_unescaped(text, source_start, close_pattern) {
            if !is_decoration_only(&text[close + close_pattern.len()..]) {
                row += 1;
                continue;
            }
            let source = text[source_start..close].trim().to_string();
            if !source.is_empty() && row as u16 != cursor_row {
                out.push(ReflowBlock {
                    row: row as u16,
                    source_rows: 1,
                    start: start_col,
                    source: normalize_claude_rows(source, policy),
                });
            }
            row += 1;
            continue;
        }

        let mut source = text[source_start..].trim().to_string();
        let mut end_row = row + 1;
        let mut found = false;
        while end_row < usize::from(conversation_end) {
            let (raw_line, _) = &rows_data[end_row];
            let line = strip_row_prefix(raw_line, prefix);
            if transcript_boundary(screen, end_row as u16, cols, line) {
                break;
            }
            if let Some(close) = display_close(line, close_pattern) {
                if !is_decoration_only(&line[close + close_pattern.len()..]) {
                    break;
                }
                append_math_rows(&mut source, line[..close].trim(), policy);
                found = true;
                break;
            }
            append_math_rows(&mut source, line.trim(), policy);
            end_row += 1;
        }

        if found && !source.trim().is_empty() && (close_pattern != "]" || source.contains('\\')) {
            if usize::from(cursor_row) >= row && usize::from(cursor_row) <= end_row {
                row = end_row + 1;
                continue;
            }
            out.push(ReflowBlock {
                row: row as u16,
                source_rows: (end_row - row + 1) as u16,
                start: start_col,
                source: normalize_claude_rows(source, policy),
            });
            row = end_row + 1;
            continue;
        }

        row += 1;
    }

    out
}

fn display_close(text: &str, pattern: &str) -> Option<usize> {
    if pattern == "]" {
        (text.trim() == "]").then(|| text.find(']').unwrap())
    } else {
        find_unescaped(text, 0, pattern)
    }
}

fn transcript_boundary(screen: &Screen, row: u16, cols: u16, text: &str) -> bool {
    if is_rule_row(screen, row, cols)
        || text.contains("Jump to bottom (")
        || text.contains("Jump to latest")
    {
        return true;
    }
    text.trim()
        .split_once(". ")
        .is_some_and(|(number, heading)| {
            !number.is_empty()
                && number.chars().all(|ch| ch.is_ascii_digit())
                && heading.chars().next().is_some_and(char::is_alphabetic)
        })
}

fn normalize_claude_rows(mut source: String, policy: MathPolicy) -> String {
    if policy != MathPolicy::Claude {
        return source;
    }
    use latex_core::lexer::{lex, TokenKind};
    let mut insertions = Vec::new();
    let mut position = 0;
    while let Some(offset) = source[position..].find("\\begin{") {
        let start = position + offset + 7;
        let Some(end_name) = source[start..].find('}') else {
            break;
        };
        let name = &source[start..start + end_name];
        let body_start = start + end_name + 1;
        position = body_start;
        if !matches!(
            name,
            "matrix"
                | "pmatrix"
                | "bmatrix"
                | "Bmatrix"
                | "vmatrix"
                | "Vmatrix"
                | "cases"
                | "aligned"
                | "align"
                | "array"
                | "split"
        ) {
            continue;
        }
        let close = format!("\\end{{{name}}}");
        let Some(length) = source[body_start..].find(&close) else {
            continue;
        };
        let body = &source[body_start..body_start + length];
        // Lost escapes cannot be inferred safely across nested environments.
        if body.contains("\\begin{") {
            position = body_start + length + close.len();
            continue;
        }
        let mut depth = 0usize;
        let mut ampersand = false;
        for token in lex(body) {
            match token.kind {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => depth = depth.saturating_sub(1),
                TokenKind::Amp if depth == 0 => ampersand = true,
                TokenKind::Newline if depth == 0 => ampersand = false,
                TokenKind::Command(ref command)
                    if depth == 0
                        && command == " "
                        && ampersand
                        && body[token.span.end..].contains('&') =>
                {
                    // Claude's Markdown can collapse a matrix row separator to a space escape.
                    insertions.push(body_start + token.span.start);
                    ampersand = false;
                }
                _ => {}
            }
        }
        position = body_start + length + close.len();
    }
    for index in insertions.into_iter().rev() {
        source.insert(index, '\\');
    }
    source
}

fn dollar_open(text: &str) -> Option<(usize, &'static str, usize)> {
    let mut cursor = 0usize;
    while let Some(index) = find_unescaped(text, cursor, "$") {
        if text[index..].starts_with("$$") {
            cursor = index + 2;
            continue;
        }
        return Some((index, "$", 1));
    }
    None
}

fn display_open(text: &str, policy: MathPolicy) -> Option<(usize, &'static str, usize)> {
    if policy == MathPolicy::Claude && text.trim() == "[" {
        return text.find('[').map(|index| (index, "]", 1));
    }
    let dollar = find_unescaped(text, 0, "$$").map(|index| (index, "$$", 2));
    let bracket = find_unescaped(text, 0, "\\[").map(|index| (index, "\\]", 2));
    match (dollar, bracket) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn find_multiline_display(
    screen: &Screen,
    rows_data: &[(String, Vec<u16>)],
    rows: u16,
    cols: u16,
    cursor_row: u16,
    occupied: &mut [bool],
    policy: MathPolicy,
) -> Vec<BlockReplacement> {
    let mut out = Vec::new();
    let mut row = 0usize;

    while row < rows_data.len() {
        if occupied[row] {
            row += 1;
            continue;
        }
        let (text, col_at_byte) = &rows_data[row];
        let Some((open, close_pattern, source_offset)) = display_open(text, policy) else {
            row += 1;
            continue;
        };
        // Same-row display math is handled by the per-row scanner.
        if find_unescaped(text, open + source_offset, close_pattern).is_some() {
            row += 1;
            continue;
        }
        if !is_decoration_only(&text[..open]) {
            row += 1;
            continue;
        }

        let opening_prefix = &text[..open];
        let mut source = text[open + source_offset..].trim().to_string();
        let mut end_row = row + 1;
        let mut found = false;

        while end_row < rows_data.len() {
            let (raw_line, _) = &rows_data[end_row];
            let line = strip_row_prefix(raw_line, opening_prefix);
            if transcript_boundary(screen, end_row as u16, cols, line) {
                break;
            }
            if let Some(close) = display_close(line, close_pattern) {
                if !is_decoration_only(&line[close + close_pattern.len()..]) {
                    break;
                }
                append_math_rows(&mut source, line[..close].trim(), policy);
                found = true;
                break;
            }
            append_math_rows(&mut source, line.trim(), policy);
            end_row += 1;
        }

        if !found || source.trim().is_empty() || (close_pattern == "]" && !source.contains('\\')) {
            row += 1;
            continue;
        }
        let source = normalize_claude_rows(source, policy);
        if usize::from(cursor_row) >= row && usize::from(cursor_row) <= end_row {
            // Editing inside a multi-line display formula: leave the whole span
            // raw. Rendering only the first row would split the input.
            for index in row..=end_row {
                if let Some(slot) = occupied.get_mut(index) {
                    *slot = true;
                }
            }
            row = end_row + 1;
            continue;
        }
        let start_col = col_at_byte.get(open).copied().unwrap_or(0);
        let source_rows = end_row - row + 1;

        let available =
            usize::from(content_end(screen, row as u16, start_col, cols).saturating_sub(start_col));
        let options = Options {
            mode: Mode::Block,
            theme: Theme::mono(),
            max_width: Some(available),
            align: Align::Left,
            indent: 0,
            fallback: Fallback::Source,
        };
        let rendered = render(source.trim(), &options);
        if rendered.fallback || rendered.width == 0 || rendered.width > available {
            row = end_row + 1;
            continue;
        }

        let mut lines: Vec<String> = rendered.text.lines().map(str::to_string).collect();
        while lines.len() < source_rows {
            lines.push(String::new());
        }
        let height = lines.len();
        let baseline = rendered.baseline;

        // Try baseline alignment first, then shift the art upward through all
        // possible offsets. A display near the bottom can use free rows above.
        let max_above = height.saturating_sub(1).min(row);
        let mut candidates = Vec::new();
        if baseline > 0 {
            candidates.push(row.saturating_sub(baseline));
        }
        for offset in (0..=max_above).rev() {
            candidates.push(row.saturating_sub(offset));
        }
        candidates.push(row);
        candidates.dedup();

        let mut accepted = None;
        for top in candidates {
            if top + height > usize::from(rows) || top + height <= end_row {
                continue;
            }
            let above = row.saturating_sub(top);
            let below = (top + height).saturating_sub(end_row + 1);
            let above_ok = (0..above).all(|dy| row_is_blank(screen, (top + dy) as u16, cols));
            let below_ok =
                (0..below).all(|dy| row_is_blank(screen, (end_row + 1 + dy) as u16, cols));
            let free =
                (0..height).all(|dy| occupied.get(top + dy).map(|taken| !taken).unwrap_or(false));
            if above_ok && below_ok && free {
                accepted = Some(top);
                break;
            }
        }

        let Some(top) = accepted else {
            // No room for 2D art. Keep the conversation readable with a compact
            // one-line rendering that uses the original block's rows.
            let compact_options = Options {
                mode: Mode::Inline,
                theme: Theme::mono(),
                max_width: Some(available),
                align: Align::Left,
                indent: 0,
                fallback: Fallback::Source,
            };
            let compact = render(source.trim(), &compact_options);
            let compact_width = UnicodeWidthStr::width(compact.text.as_str());
            if !compact.fallback
                && !compact.text.contains('\n')
                && compact_width > 0
                && compact_width <= available
            {
                let mut lines = vec![compact.text];
                while lines.len() < source_rows {
                    lines.push(String::new());
                }
                for index in row..row + lines.len() {
                    if let Some(slot) = occupied.get_mut(index) {
                        *slot = true;
                    }
                }
                out.push(BlockReplacement {
                    row: row as u16,
                    prefix_row: row as u16,
                    start: start_col,
                    width: compact_width as u16,
                    height: lines.len() as u16,
                    lines,
                });
            }
            row = end_row + 1;
            continue;
        };
        for index in top..top + height {
            if let Some(slot) = occupied.get_mut(index) {
                *slot = true;
            }
        }
        out.push(BlockReplacement {
            row: top as u16,
            prefix_row: row as u16,
            start: start_col,
            width: rendered.width as u16,
            height: height as u16,
            lines,
        });
        row = end_row + 1;
    }

    out
}

fn try_block(
    screen: &Screen,
    row: u16,
    rows: u16,
    cols: u16,
    start: u16,
    source: &str,
    occupied: &mut [bool],
) -> Option<BlockReplacement> {
    let available = usize::from(content_end(screen, row, start, cols).saturating_sub(start));
    let options = Options {
        mode: Mode::Block,
        theme: Theme::mono(),
        max_width: Some(available),
        align: Align::Left,
        indent: 0,
        fallback: Fallback::Source,
    };
    let rendered = render(source, &options);
    if rendered.fallback || rendered.width == 0 || rendered.width > available {
        return None;
    }
    let lines: Vec<String> = rendered.text.lines().map(str::to_string).collect();
    if lines.is_empty() {
        return None;
    }
    let height = lines.len() as u16;
    let baseline = rendered.baseline as u16;

    // Try every vertical placement that keeps the source row inside the art:
    // baseline-aligned first, then bottom-aligned, then progressively higher.
    // This lets a formula near the bottom use free rows above it.
    let max_above = height.saturating_sub(1).min(row);
    let mut candidates = Vec::new();
    if baseline > 0 {
        candidates.push(row.saturating_sub(baseline));
    }
    for offset in (0..=max_above).rev() {
        candidates.push(row.saturating_sub(offset));
    }
    candidates.push(row);
    candidates.dedup();

    for top in candidates {
        if top.saturating_add(height) > rows {
            continue;
        }
        let above = row.saturating_sub(top);
        let below = top.saturating_add(height).saturating_sub(row + 1);
        let above_ok = (0..above).all(|dy| row_is_blank(screen, top + dy, cols));
        let below_ok = (0..below).all(|dy| row_is_blank(screen, row + 1 + dy, cols));
        let free = (0..height).all(|dy| {
            occupied
                .get(usize::from(top + dy))
                .map(|taken| !taken)
                .unwrap_or(false)
        });
        if above_ok && below_ok && free {
            for dy in 0..height {
                if let Some(slot) = occupied.get_mut(usize::from(top + dy)) {
                    *slot = true;
                }
            }
            return Some(BlockReplacement {
                row: top,
                prefix_row: row,
                start,
                width: rendered.width as u16,
                height,
                lines,
            });
        }
    }

    None
}

pub(crate) fn row_is_blank(screen: &Screen, row: u16, cols: u16) -> bool {
    (0..cols).all(|col| {
        screen
            .cell(row, col)
            .map(|cell| cell.contents().trim().is_empty())
            .unwrap_or(true)
    })
}

/// A different background marks the next pane, even when it contains no text.
pub(crate) fn content_end(screen: &Screen, row: u16, start: u16, cols: u16) -> u16 {
    let background = screen.cell(row, start).map(|cell| cell.bgcolor());
    (start..cols)
        .find(|col| screen.cell(row, *col).map(|cell| cell.bgcolor()) != background)
        .unwrap_or(cols)
}

fn is_standalone(text: &str, start: usize, end: usize) -> bool {
    let mut outside = String::with_capacity(text.len());
    outside.push_str(&text[..start]);
    outside.push_str(&text[end.min(text.len())..]);
    is_decoration_only(&outside)
}

fn strip_row_prefix<'a>(line: &'a str, opening_prefix: &str) -> &'a str {
    if !opening_prefix.is_empty() {
        if let Some(rest) = line.strip_prefix(opening_prefix) {
            return rest;
        }
    }
    let start = line
        .char_indices()
        .find(|(_, ch)| !is_border_or_space(*ch))
        .map(|(index, _)| index)
        .unwrap_or(line.len());
    &line[start..]
}

fn is_border_or_space(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '┃' | '│' | '║' | '|' | '>')
}

pub fn detect_input_start(screen: &Screen, cursor_row: u16) -> u16 {
    let (rows, cols) = screen.size();
    let rows = rows.get();
    let cols = cols.get();

    // OpenCode ends the composer with `╹` followed by a `▀` rule. That marker
    // is bottom-anchored and independent of the cursor, so it still works while
    // a prompt is queued or the cursor is parked outside the composer.
    if let Some((bottom, border_col)) = composer_bottom(screen, rows, cols) {
        let top = if bottom > 0 {
            walk_border_up(screen, bottom - 1, border_col).unwrap_or(bottom)
        } else {
            bottom
        };
        return top;
    }

    // Claude Code uses a single-line `❯` prompt without vertical borders.
    if let Some(row) = prompt_row(screen, rows, cols) {
        return row;
    }

    // Fallback for TUIs without that marker: walk up from the cursor along the
    // left border of the row it is on.
    if rows >= 16 && cursor_row >= rows.saturating_sub(15) {
        if let Some(border_col) = (0..cols).find(|col| is_border_cell(screen, cursor_row, *col)) {
            if let Some(top) = walk_border_up(screen, cursor_row, border_col) {
                return top;
            }
        }
    }

    // Last resort: the lowest border run anchored near the bottom. This keeps
    // a composer with a different bottom decoration (for example while the
    // transcript is scrolled) out of the reflow area instead of rendering its
    // formula and hiding the input.
    lowest_border_run(screen, rows, cols).unwrap_or(rows)
}

fn lowest_border_run(screen: &Screen, rows: u16, cols: u16) -> Option<u16> {
    let floor = rows.saturating_sub(12);
    let mut row = rows;
    while row > floor {
        row -= 1;
        for col in 0..cols {
            if !is_border_cell(screen, row, col) {
                continue;
            }
            if let Some(top) = walk_border_up(screen, row, col) {
                if row > top {
                    return Some(top);
                }
            }
        }
    }
    None
}

fn prompt_row(screen: &Screen, rows: u16, cols: u16) -> Option<u16> {
    let mut row = rows;
    while row > 0 {
        row -= 1;
        let first = (0..cols).find(|col| {
            cell_char(screen, row, *col)
                .map(|ch| !ch.is_whitespace())
                .unwrap_or(false)
        });
        let Some(col) = first else { continue };
        if cell_char(screen, row, col) != Some('❯') {
            continue;
        }
        // Claude Code puts a horizontal rule directly under the prompt. This
        // distinguishes the composer from transcript lines that may also start
        // with the same glyph.
        if row + 1 < rows && is_rule_row(screen, row + 1, cols) {
            return Some(row);
        }
    }
    None
}

fn is_rule_row(screen: &Screen, row: u16, cols: u16) -> bool {
    let mut dashes = 0;
    for col in 0..cols {
        match cell_char(screen, row, col) {
            Some('─') => dashes += 1,
            Some(ch) if !ch.is_whitespace() => return false,
            _ => {}
        }
    }
    dashes >= 4
}

fn composer_bottom(screen: &Screen, rows: u16, cols: u16) -> Option<(u16, u16)> {
    let mut row = rows;
    while row > 0 {
        row -= 1;
        for col in 0..cols.saturating_sub(1) {
            if cell_char(screen, row, col) == Some('╹')
                && cell_char(screen, row, col + 1) == Some('▀')
            {
                return Some((row, col));
            }
        }
    }
    None
}

fn walk_border_up(screen: &Screen, from: u16, border_col: u16) -> Option<u16> {
    let mut start = None;
    let mut row = from;
    let mut steps = 0u16;
    while steps < 32 {
        if !is_border_cell(screen, row, border_col) {
            break;
        }
        start = Some(row);
        if row == 0 {
            break;
        }
        row -= 1;
        steps += 1;
    }
    start
}

fn cell_char(screen: &Screen, row: u16, col: u16) -> Option<char> {
    screen.cell(row, col)?.contents().chars().next()
}

fn is_border_cell(screen: &Screen, row: u16, col: u16) -> bool {
    screen
        .cell(row, col)
        .map(|cell| {
            matches!(
                cell.contents().chars().next().unwrap_or(' '),
                '┃' | '│' | '║' | '╭' | '┏' | '┌'
            )
        })
        .unwrap_or(false)
}

/// Keep the transcript's row positions while either agent shows its scroll banner.
pub fn is_scrolled_up(screen: &Screen) -> bool {
    scroll_banner_row(screen).is_some()
}

pub(crate) fn scroll_banner_row(screen: &Screen) -> Option<u16> {
    let (rows, cols) = screen.size();
    (0..rows.get()).find(|row| {
        let (text, _) = row_text(screen, *row, cols.get());
        text.contains("Jump to latest") || text.contains("Jump to bottom (")
    })
}

fn is_decoration_only(text: &str) -> bool {
    text.chars().all(is_decoration)
}

fn is_decoration(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '┃' | '│' | '║' | '|' | '>' | '•' | '·' | '*' | '-' | '–' | '—'
        )
}

fn shift_safe(suffix: &str) -> bool {
    if suffix.trim_end().is_empty() {
        return true;
    }
    if suffix
        .chars()
        .any(|ch| matches!(ch, '│' | '┃' | '║' | '|' | '┆' | '┇'))
    {
        return false;
    }

    // A run of three or more spaces between text is a pane/column boundary,
    // even when normal prose appears before it. Shifting such a row would move
    // the right-hand pane (for example the /resume overlay) out of alignment.
    let mut spaces = 0usize;
    for ch in suffix.chars() {
        if ch == ' ' {
            spaces += 1;
        } else {
            if spaces >= 3 {
                return false;
            }
            spaces = 0;
        }
    }
    true
}
