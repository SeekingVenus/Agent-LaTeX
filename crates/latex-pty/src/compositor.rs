//! Virtual screen + cell-level compositor.
//!
//! Display math is rendered with a small amount of display reflow: the 2D art
//! reserves its full height, later conversation rows move down, and older rows
//! scroll out of the viewport above the fixed input/status area.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::num::NonZeroU16;

use latex_term::{render, Fallback, Mode, Options as RenderOptions, Theme};
use unicode_width::UnicodeWidthStr;
use vt100::Color;

use crate::math::{
    content_end, detect_input_start, find_math_with_policy, find_reflow_blocks, row_is_blank,
    scroll_banner_row, MathPolicy, MathReplacements, ReflowBlock, Replacement,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Style {
    fg: Color,
    bg: Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

enum RowKind {
    Blank,
    Source(u16),
    Art {
        prefix_row: u16,
        start: u16,
        text: String,
    },
}

pub struct Compositor {
    parser: vt100::Parser,
    rows: u16,
    cols: u16,
    replace_inline: bool,
    math_policy: MathPolicy,
}

impl Compositor {
    pub fn new(rows: u16, cols: u16, replace_inline: bool) -> Self {
        let rows = rows.max(1);
        let cols = cols.max(1);
        Self {
            parser: vt100::Parser::new(
                NonZeroU16::new(rows).unwrap(),
                NonZeroU16::new(cols).unwrap(),
                0,
            ),
            rows,
            cols,
            replace_inline,
            math_policy: MathPolicy::Agent,
        }
    }

    /// Use the aggressive policy for agent prose or the conservative policy
    /// for generic command output.
    pub fn set_math_policy(&mut self, policy: MathPolicy) {
        self.math_policy = policy;
    }

    /// Toggle between rendered math and the original terminal output.
    pub fn toggle_rendering(&mut self) {
        self.replace_inline = !self.replace_inline;
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        self.rows = rows;
        self.cols = cols;
        self.parser.set_size(
            NonZeroU16::new(rows).unwrap(),
            NonZeroU16::new(cols).unwrap(),
        );
    }

    pub fn cursor(&self) -> (u16, u16) {
        self.parser.screen().cursor_position()
    }

    pub fn render(&mut self) -> Vec<u8> {
        self.render_frame().to_bytes()
    }

    /// Build the logical frame. The PTY loop diffs it against the previous
    /// frame and only writes changed rows.
    pub fn render_frame(&mut self) -> Frame {
        let screen = self.parser.screen();
        let (cursor_row, cursor_col) = screen.cursor_position();

        // Keep the whole detected input/status area fixed. Rows above it are
        // the conversation area that may be reflowed.
        let banner_row = scroll_banner_row(screen);
        let conversation_end =
            detect_input_start(screen, cursor_row).min(banner_row.unwrap_or(self.rows));
        let scrolled = banner_row.is_some();
        let math = if self.replace_inline {
            find_math_with_policy(screen, self.math_policy)
        } else {
            MathReplacements::default()
        };
        let reflow = if self.replace_inline {
            find_reflow_blocks(screen, conversation_end, cursor_row, self.math_policy)
        } else {
            Vec::new()
        };

        // Free rows at the bottom of the conversation absorb reflow growth.
        // Without this, a short conversation grows by deleting its own first
        // message from the top (the fragment that used to appear above a
        // just-sent prompt).
        let trailing_blanks = trailing_blank_rows(screen, conversation_end, self.cols) as isize;

        let mut applied = Vec::new();
        let mut block_rows = HashSet::new();
        for block in reflow {
            let Some(lines) = render_block_art(screen, &block, self.cols) else {
                continue;
            };
            // Shrinking complete formulas is safe above the fixed scroll overlay.
            if lines.len() > usize::from(conversation_end)
                || (scrolled && lines.len() > usize::from(block.source_rows))
            {
                continue;
            }
            let start = usize::from(block.row);
            let end = start + usize::from(block.source_rows);
            if end > usize::from(conversation_end) {
                continue;
            }
            // Once the free rows below are exhausted, reflow drops rows from
            // the top. If that would clip the art itself, leave the formula
            // compact instead of drawing a fragment.
            let extra = lines.len() as isize - block.source_rows as isize;
            if extra > block.row as isize + trailing_blanks {
                continue;
            }
            for row in block.row..block.row + block.source_rows {
                block_rows.insert(row);
            }
            applied.push((start, end, block.start, lines));
        }

        let mut in_place = Vec::new();
        for block in &math.blocks {
            let top = block.row;
            let bottom = top.saturating_add(block.height);
            if bottom > conversation_end
                || (cursor_row >= top && cursor_row < bottom)
                || (top..bottom).any(|row| block_rows.contains(&row))
            {
                continue;
            }
            for row in top..bottom {
                block_rows.insert(row);
            }
            in_place.push(block);
        }

        let inline: Vec<&Replacement> = math
            .inline
            .iter()
            .filter(|replacement| !block_rows.contains(&replacement.row))
            .collect();

        let mut kinds: Vec<RowKind> = (0..conversation_end).map(RowKind::Source).collect();

        applied.sort_by_key(|(start, _, _, _)| *start);

        // Track where the terminal cursor lands after reflow. Rows below a
        // block keep their visual position because the inserted rows are
        // dropped from the top; a shorter block moves them up instead.
        let total_growth: isize = applied
            .iter()
            .map(|(start, end, _, lines)| lines.len() as isize - (*end as isize - *start as isize))
            .sum();
        let dropped = (total_growth - trailing_blanks).max(0);
        let mut cursor_visual = cursor_row as isize;
        if usize::from(cursor_row) < usize::from(conversation_end) {
            let mut delta = 0isize;
            let mut inside = None;
            for (start, end, _, lines) in &applied {
                let art = lines.len() as isize;
                let span = *end as isize - *start as isize;
                if usize::from(cursor_row) >= *end {
                    delta += art - span;
                } else if usize::from(cursor_row) >= *start {
                    inside = Some(*start as isize + delta - dropped);
                    break;
                } else {
                    break;
                }
            }
            cursor_visual = inside.unwrap_or(cursor_row as isize + delta - dropped);
        }
        let cursor_visual = cursor_visual.clamp(0, self.rows.saturating_sub(1) as isize) as u16;

        for block in in_place {
            for (offset, text) in block.lines.iter().enumerate() {
                kinds[usize::from(block.row) + offset] = RowKind::Art {
                    prefix_row: block.prefix_row,
                    start: block.start,
                    text: text.clone(),
                };
            }
        }

        // Apply from bottom to top so earlier indices remain valid.
        for (start, end, art_start, lines) in applied.into_iter().rev() {
            if end > kinds.len() {
                continue;
            }
            let prefix_row = start as u16;
            let replacement = lines
                .into_iter()
                .map(|text| RowKind::Art {
                    prefix_row,
                    start: art_start,
                    text,
                })
                .collect::<Vec<_>>();
            kinds.splice(start..end, replacement);
        }

        // Reflow grows the conversation. Consume blank rows at the bottom
        // first; only then scroll older rows out of the top.
        while kinds.len() > usize::from(conversation_end) {
            let blank_tail = match kinds.last() {
                Some(RowKind::Blank) => true,
                Some(RowKind::Art { text, .. }) => text.trim().is_empty(),
                Some(RowKind::Source(row)) => row_is_blank(screen, *row, self.cols),
                None => false,
            };
            if blank_tail {
                kinds.pop();
            } else {
                kinds.remove(0);
            }
        }
        while kinds.len() < usize::from(conversation_end) {
            kinds.push(RowKind::Blank);
        }

        let conversation_rows = usize::from(conversation_end);
        let mut rows = Vec::with_capacity(usize::from(self.rows));
        for (visual_row, kind) in kinds.iter().enumerate() {
            let mut row = String::new();
            match kind {
                RowKind::Blank => {
                    render_blank_row(&mut row, screen, visual_row as u16, self.cols);
                }
                RowKind::Source(source) => {
                    render_source_row(&mut row, screen, *source, self.cols, &inline);
                }
                RowKind::Art {
                    prefix_row,
                    start,
                    text,
                } => {
                    render_art_row(
                        &mut row,
                        screen,
                        *prefix_row,
                        visual_row as u16,
                        *start,
                        text,
                        self.cols,
                    );
                }
            }
            row.push_str("\x1b[0m");
            rows.push(row);
        }
        for row in (conversation_rows as u16)..self.rows {
            let mut line = String::new();
            render_source_row(&mut line, screen, row, self.cols, &inline);
            line.push_str("\x1b[0m");
            rows.push(line);
        }

        let mut cursor_col = cursor_col.min(self.cols.saturating_sub(1));
        for replacement in inline
            .iter()
            .filter(|item| item.row == cursor_row && item.shift)
        {
            if cursor_col >= replacement.end {
                cursor_col = cursor_col.saturating_sub(
                    (replacement.end - replacement.start).saturating_sub(replacement.width),
                );
            } else if cursor_col > replacement.start && cursor_col < replacement.end {
                cursor_col = replacement.start;
            }
        }
        let cursor_col = cursor_col.min(self.cols.saturating_sub(1));

        Frame {
            rows,
            cursor: (cursor_visual, cursor_col),
            hide_cursor: screen.hide_cursor(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub rows: Vec<String>,
    pub cursor: (u16, u16),
    pub hide_cursor: bool,
}

impl Frame {
    /// Write the complete screen. Used on the first frame and by tests.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = String::new();
        out.push_str("\x1b[?2026h\x1b[?25l\x1b[H");
        for (index, row) in self.rows.iter().enumerate() {
            out.push_str(row);
            if index + 1 < self.rows.len() {
                out.push_str("\r\n");
            }
        }
        let _ = write!(out, "\x1b[{};{}H", self.cursor.0 + 1, self.cursor.1 + 1);
        out.push_str(if self.hide_cursor {
            "\x1b[?25l"
        } else {
            "\x1b[?25h"
        });
        out.push_str("\x1b[?2026l");
        out.into_bytes()
    }

    /// Write only rows that differ from `previous`, plus the cursor. Returns
    /// an empty vector when nothing changed.
    pub fn diff(&self, previous: Option<&Frame>) -> Vec<u8> {
        let full = match previous {
            Some(previous) => previous.rows.len() != self.rows.len(),
            None => true,
        };
        let changed: Vec<usize> = if full {
            (0..self.rows.len()).collect()
        } else {
            self.rows
                .iter()
                .zip(previous.unwrap().rows.iter())
                .enumerate()
                .filter(|(_, (current, old))| current != old)
                .map(|(index, _)| index)
                .collect()
        };
        let cursor_changed = match previous {
            Some(previous) => {
                previous.cursor != self.cursor || previous.hide_cursor != self.hide_cursor
            }
            None => true,
        };
        if changed.is_empty() && !cursor_changed {
            return Vec::new();
        }

        let mut out = String::new();
        out.push_str("\x1b[?2026h\x1b[?25l");
        for index in changed {
            let _ = write!(out, "\x1b[{};1H", index + 1);
            out.push_str(&self.rows[index]);
        }
        let _ = write!(out, "\x1b[{};{}H", self.cursor.0 + 1, self.cursor.1 + 1);
        out.push_str(if self.hide_cursor {
            "\x1b[?25l"
        } else {
            "\x1b[?25h"
        });
        out.push_str("\x1b[?2026l");
        out.into_bytes()
    }
}

fn trailing_blank_rows(screen: &vt100::Screen, conversation_end: u16, cols: u16) -> u16 {
    let mut count = 0;
    let mut row = conversation_end;
    while row > 0 {
        row -= 1;
        if !row_is_blank(screen, row, cols) {
            break;
        }
        count += 1;
    }
    count
}

fn render_block_art(screen: &vt100::Screen, block: &ReflowBlock, cols: u16) -> Option<Vec<String>> {
    let available =
        usize::from(content_end(screen, block.row, block.start, cols).saturating_sub(block.start));
    if available == 0 {
        return None;
    }
    let options = RenderOptions {
        mode: Mode::Block,
        theme: Theme::mono(),
        max_width: Some(available),
        align: latex_core::ast::Align::Left,
        indent: 0,
        fallback: Fallback::Source,
    };
    let output = render(&block.source, &options);
    if output.fallback || output.height == 0 || output.width > available {
        return None;
    }
    Some(output.text.lines().map(str::to_string).collect())
}

fn render_source_row(
    out: &mut String,
    screen: &vt100::Screen,
    row: u16,
    cols: u16,
    inline: &[&Replacement],
) {
    let mut replacements = inline.iter().filter(|item| item.row == row).peekable();
    let mut current = Style::default();
    write_style(out, current);
    let mut removed = 0usize;
    let mut col = 0u16;

    while col < cols {
        if let Some(replacement) = replacements.peek() {
            if replacement.start == col {
                let replacement = *replacement;
                replacements.next();
                if replacement.shift {
                    removed += usize::from(
                        (replacement.end - replacement.start).saturating_sub(replacement.width),
                    );
                }
                current = style_of(screen.cell(row, col));
                write_style(out, current);
                write_replacement(out, replacement);
                col = replacement.end;
                continue;
            }
        }

        let Some(cell) = screen.cell(row, col) else {
            col += 1;
            continue;
        };
        if cell.is_wide_continuation() {
            col += 1;
            continue;
        }
        let style = style_of(Some(cell));
        if style != current {
            current = style;
            write_style(out, current);
        }
        let contents = cell.contents();
        if contents.is_empty() {
            out.push(' ');
        } else {
            out.push_str(contents);
        }
        col += 1;
    }

    if removed > 0 {
        let style = style_of(screen.cell(row, cols.saturating_sub(1)));
        if style != current {
            write_style(out, style);
        }
        out.push_str(&" ".repeat(removed));
    }
}

fn render_blank_row(out: &mut String, screen: &vt100::Screen, row: u16, cols: u16) {
    let mut current = Style::default();
    write_style(out, current);
    for col in 0..cols {
        let style = style_of(screen.cell(row, col));
        if style != current {
            current = style;
            write_style(out, current);
        }
        out.push(' ');
    }
}

fn render_art_row(
    out: &mut String,
    screen: &vt100::Screen,
    prefix_row: u16,
    visual_row: u16,
    start: u16,
    text: &str,
    cols: u16,
) {
    let mut current = Style::default();
    write_style(out, current);

    // Preserve the source row's left border/indent for every art line.
    let mut col = 0u16;
    while col < start {
        let Some(cell) = screen.cell(prefix_row, col) else {
            out.push(' ');
            col += 1;
            continue;
        };
        if cell.is_wide_continuation() {
            col += 1;
            continue;
        }
        let style = style_of(Some(cell));
        if style != current {
            current = style;
            write_style(out, current);
        }
        let contents = cell.contents();
        if contents.is_empty() {
            out.push(' ');
        } else {
            out.push_str(contents);
        }
        col += 1;
    }

    let style = style_of(screen.cell(prefix_row, start));
    if style != current {
        current = style;
        write_style(out, current);
    }
    out.push_str(text);
    let used = UnicodeWidthStr::width(text);
    let end = content_end(screen, prefix_row, start, cols);
    let available = usize::from(end.saturating_sub(start));
    if used < available {
        out.push_str(&" ".repeat(available - used));
    }
    // Keep the neighbouring pane at its original screen coordinates and style.
    for col in end..cols {
        let Some(cell) = screen.cell(visual_row, col) else {
            continue;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        let style = style_of(Some(cell));
        if style != current {
            current = style;
            write_style(out, current);
        }
        let contents = cell.contents();
        if contents.is_empty() {
            out.push(' ');
        } else {
            out.push_str(contents);
        }
    }
}

fn write_replacement(out: &mut String, replacement: &Replacement) {
    out.push_str(&replacement.text);
    if !replacement.shift {
        let used = usize::from(replacement.width);
        let span = usize::from(replacement.end - replacement.start);
        if used < span {
            out.push_str(&" ".repeat(span - used));
        }
    }
}

fn style_of(cell: Option<&vt100::Cell>) -> Style {
    let Some(cell) = cell else {
        return Style::default();
    };
    Style {
        fg: cell.fgcolor(),
        bg: cell.bgcolor(),
        bold: cell.bold(),
        dim: cell.dim(),
        italic: cell.italic(),
        underline: cell.underline(),
        inverse: cell.inverse(),
    }
}

fn write_style(out: &mut String, style: Style) {
    let mut codes = vec!["0".to_string()];
    if style.bold {
        codes.push("1".into());
    }
    if style.dim {
        codes.push("2".into());
    }
    if style.italic {
        codes.push("3".into());
    }
    if style.underline {
        codes.push("4".into());
    }
    if style.inverse {
        codes.push("7".into());
    }
    push_color(&mut codes, style.fg, true);
    push_color(&mut codes, style.bg, false);
    let _ = write!(out, "\x1b[{}m", codes.join(";"));
}

fn push_color(codes: &mut Vec<String>, color: Color, foreground: bool) {
    match color {
        Color::Default => {}
        Color::Idx(index) => {
            codes.push(format!("{};5;{}", if foreground { 38 } else { 48 }, index))
        }
        Color::Rgb(red, green, blue) => codes.push(format!(
            "{};2;{};{};{}",
            if foreground { 38 } else { 48 },
            red,
            green,
            blue
        )),
    }
}
