//! Convert a [`Canvas`] into an ANSI/Unicode string.

use latex_core::ast::{Align, Face};
use latex_layout::{Canvas, CellKind};

use crate::theme::{Color, RoleStyle, Theme};

pub fn paint(
    canvas: &Canvas,
    theme: &Theme,
    align: Align,
    width: Option<usize>,
    indent: usize,
) -> String {
    let mut lines = Vec::with_capacity(canvas.height);

    for y in 0..canvas.height {
        let row = canvas.row(y);
        let visible_end = row
            .iter()
            .rposition(|cell| cell.kind == CellKind::Glyph && !cell.glyph.trim().is_empty())
            .map(|index| index + 1)
            .unwrap_or(0);

        let mut line = String::new();
        let mut current: Option<RoleStyle> = None;

        for cell in row.iter().take(visible_end) {
            match cell.kind {
                CellKind::Empty => line.push(' '),
                CellKind::Continuation => {}
                CellKind::Glyph => {
                    if cell.glyph.trim().is_empty() {
                        line.push(' ');
                        continue;
                    }
                    if theme.color_enabled() {
                        let style = style_for_cell(theme, cell.role, cell.face);
                        if current != Some(style) {
                            if !(current.is_none() && style == RoleStyle::plain()) {
                                line.push_str(&sgr(style));
                            }
                            current = Some(style);
                        }
                    }
                    line.push_str(&cell.glyph);
                }
            }
        }

        if theme.color_enabled()
            && current
                .map(|style| style != RoleStyle::plain())
                .unwrap_or(false)
        {
            line.push_str("\x1b[0m");
        }

        let target = width.unwrap_or(canvas.width);
        let extra = target.saturating_sub(canvas.width);
        let prefix = match align {
            Align::Left => 0,
            Align::Center => extra / 2,
            Align::Right => extra,
        };
        if !line.is_empty() {
            line.insert_str(0, &" ".repeat(indent + prefix));
        }
        lines.push(line);
    }

    lines.join("\n")
}

fn style_for_cell(theme: &Theme, role: latex_layout::Role, face: Face) -> RoleStyle {
    let mut style = theme.role(role);
    match face {
        Face::Bold => style.bold = true,
        Face::Italic => style.italic = true,
        _ => {}
    }
    style
}

fn sgr(style: RoleStyle) -> String {
    let mut codes = Vec::new();
    if style.bold {
        codes.push("1".to_string());
    }
    if style.dim {
        codes.push("2".to_string());
    }
    if style.italic {
        codes.push("3".to_string());
    }
    match style.color {
        Color::Default => {}
        Color::Ansi256(index) => codes.push(format!("38;5;{index}")),
        Color::Rgb(r, g, b) => codes.push(format!("38;2;{r};{g};{b}")),
    }

    if codes.is_empty() {
        "\x1b[0m".to_string()
    } else {
        format!("\x1b[{}m", codes.join(";"))
    }
}
