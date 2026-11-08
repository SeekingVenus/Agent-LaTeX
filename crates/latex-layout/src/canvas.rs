//! A tiny cell canvas used by the 2D layout engine.
//!
//! A canvas owns a rectangular grid of terminal cells plus a baseline row.
//! Wide characters occupy two cells: the glyph cell followed by a continuation
//! cell. Combining marks are appended to the preceding glyph.

use latex_core::ast::Face;
use unicode_width::UnicodeWidthChar;

/// Semantic role of a cell, used by the terminal theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Text,
    Variable,
    Number,
    Operator,
    Relation,
    Delimiter,
    Rule,
    Accent,
    Error,
    Muted,
}

impl Role {
    pub const COUNT: usize = 10;

    pub const fn index(self) -> usize {
        match self {
            Role::Text => 0,
            Role::Variable => 1,
            Role::Number => 2,
            Role::Operator => 3,
            Role::Relation => 4,
            Role::Delimiter => 5,
            Role::Rule => 6,
            Role::Accent => 7,
            Role::Error => 8,
            Role::Muted => 9,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Empty,
    Glyph,
    Continuation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub kind: CellKind,
    pub glyph: String,
    /// Display width in terminal cells: 0 for empty/continuation, 1 or 2 for glyphs.
    pub width: u8,
    pub role: Role,
    pub face: Face,
}

impl Cell {
    pub fn empty() -> Self {
        Self {
            kind: CellKind::Empty,
            glyph: String::new(),
            width: 0,
            role: Role::Text,
            face: Face::Roman,
        }
    }

    pub fn glyph(glyph: impl Into<String>, width: u8, role: Role, face: Face) -> Self {
        Self {
            kind: CellKind::Glyph,
            glyph: glyph.into(),
            width,
            role,
            face,
        }
    }

    pub fn continuation(role: Role, face: Face) -> Self {
        Self {
            kind: CellKind::Continuation,
            glyph: String::new(),
            width: 0,
            role,
            face,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.kind == CellKind::Empty
    }
}

/// Drop terminal control characters from formula text. Line breaks are kept so
/// callers can turn them into spaces or preserve them deliberately.
pub(crate) fn strip_controls(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control() || *ch == '\n')
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    /// Row index of the math baseline. Must be `<= height` (equal is valid for
    /// a canvas that only extends above the baseline).
    pub baseline: usize,
    pub cells: Vec<Cell>,
}

impl Canvas {
    pub fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            baseline: 0,
            cells: Vec::new(),
        }
    }

    pub fn blank(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            baseline: 0,
            cells: vec![Cell::empty(); width.saturating_mul(height)],
        }
    }

    pub fn text(text: &str, role: Role, face: Face) -> Self {
        let text = strip_controls(text).replace('\n', " ");
        let mut cells = Vec::new();
        let mut width = 0usize;

        for ch in text.chars() {
            let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if char_width == 0 {
                if let Some(pos) = cells
                    .iter()
                    .rposition(|cell: &Cell| cell.kind == CellKind::Glyph)
                {
                    cells[pos].glyph.push(ch);
                }
                continue;
            }

            cells.push(Cell::glyph(ch.to_string(), char_width as u8, role, face));
            width += char_width;
            if char_width == 2 {
                cells.push(Cell::continuation(role, face));
            }
        }

        let height = if text.is_empty() { 0 } else { 1 };
        Self {
            width,
            height,
            baseline: 0,
            cells,
        }
    }

    pub fn space(width: usize) -> Self {
        if width == 0 {
            Self::empty()
        } else {
            Self::text(&" ".repeat(width), Role::Text, Face::Roman)
        }
    }

    pub fn set(&mut self, x: usize, y: usize, cell: Cell) {
        if x < self.width && y < self.height {
            self.cells[y * self.width + x] = cell;
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Option<&Cell> {
        if x < self.width && y < self.height {
            self.cells.get(y * self.width + x)
        } else {
            None
        }
    }

    pub fn row(&self, y: usize) -> &[Cell] {
        if y >= self.height || self.width == 0 {
            &[]
        } else {
            &self.cells[y * self.width..(y + 1) * self.width]
        }
    }

    pub fn blit(&mut self, source: &Canvas, x0: usize, y0: usize) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        for sy in 0..source.height {
            let dy = y0 + sy;
            if dy >= self.height {
                break;
            }
            for sx in 0..source.width {
                let dx = x0 + sx;
                if dx >= self.width {
                    break;
                }
                let cell = &source.cells[sy * source.width + sx];
                if cell.kind == CellKind::Empty {
                    continue;
                }
                self.cells[dy * self.width + dx] = cell.clone();
            }
        }
    }

    /// Place canvases side by side, aligning their baselines.
    pub fn hcat(parts: &[Canvas], gap: usize) -> Self {
        let parts: Vec<&Canvas> = parts
            .iter()
            .filter(|canvas| canvas.width > 0 || canvas.height > 0)
            .collect();
        if parts.is_empty() {
            return Self::empty();
        }

        let baseline = parts
            .iter()
            .map(|canvas| canvas.baseline)
            .max()
            .unwrap_or(0);
        let below = parts
            .iter()
            .map(|canvas| canvas.height.saturating_sub(canvas.baseline))
            .max()
            .unwrap_or(0);
        let height = baseline + below;
        let width = parts.iter().map(|canvas| canvas.width).sum::<usize>()
            + gap.saturating_mul(parts.len().saturating_sub(1));

        let mut out = Self::blank(width, height);
        out.baseline = baseline;

        let mut x = 0usize;
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                x += gap;
            }
            out.blit(part, x, baseline - part.baseline);
            x += part.width;
        }
        out
    }

    pub fn pad_left(&self, amount: usize) -> Self {
        if amount == 0 {
            return self.clone();
        }
        let mut out = Self::blank(self.width + amount, self.height);
        out.baseline = self.baseline;
        out.blit(self, amount, 0);
        out
    }

    pub fn pad_right(&self, amount: usize) -> Self {
        if amount == 0 {
            return self.clone();
        }
        let mut out = Self::blank(self.width + amount, self.height);
        out.baseline = self.baseline;
        out.blit(self, 0, 0);
        out
    }

    pub fn pad_top(&self, amount: usize) -> Self {
        if amount == 0 {
            return self.clone();
        }
        let mut out = Self::blank(self.width, self.height + amount);
        out.baseline = self.baseline + amount;
        out.blit(self, 0, amount);
        out
    }

    pub fn pad_bottom(&self, amount: usize) -> Self {
        if amount == 0 {
            return self.clone();
        }
        let mut out = Self::blank(self.width, self.height + amount);
        out.baseline = self.baseline;
        out.blit(self, 0, 0);
        out
    }

    pub fn center_in(&self, width: usize) -> Self {
        if width <= self.width {
            return self.clone();
        }
        let left = (width - self.width) / 2;
        self.pad_left(left).pad_right(width - self.width - left)
    }

    /// Replace visible glyphs with spaces while keeping the same footprint.
    pub fn phantom(&self) -> Self {
        let mut out = Self::blank(self.width, self.height);
        out.baseline = self.baseline;
        for y in 0..self.height {
            if self.width > 0 {
                let spaces = Canvas::text(&" ".repeat(self.width), Role::Text, Face::Roman);
                out.blit(&spaces, 0, y);
            }
        }
        out
    }

    /// Debug/test representation with trailing spaces removed per line.
    pub fn to_string_trimmed(&self) -> String {
        let mut lines = Vec::with_capacity(self.height);
        for y in 0..self.height {
            let mut line = String::new();
            for cell in self.row(y) {
                match cell.kind {
                    CellKind::Empty => line.push(' '),
                    CellKind::Continuation => {}
                    CellKind::Glyph => line.push_str(&cell.glyph),
                }
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }
}
