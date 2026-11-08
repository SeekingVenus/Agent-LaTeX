//! Two-dimensional terminal layout.
//!
//! The engine is a conventional box model: every node becomes a [`Canvas`] with
//! a baseline, and composition aligns baselines. There is no terminal I/O here.

use latex_core::ast::{
    AccentKind, Align, AtomClass, Delimiter, Face, FractionKind, LimitMode, Node, PhantomKind,
    Space,
};
use latex_core::symbols;

use crate::canvas::{strip_controls, Canvas, Cell, CellKind, Role};
use crate::inline::{
    combining_accent, render_inline, simple_root_char, to_subscript, to_superscript,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutOptions {
    /// Display style (block math) rather than inline/text style.
    pub display: bool,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self { display: true }
    }
}

pub fn layout(node: &Node, options: &LayoutOptions) -> Canvas {
    layout_node(node, options, None)
}

fn layout_node(node: &Node, options: &LayoutOptions, face: Option<Face>) -> Canvas {
    match node {
        Node::Row(nodes) => layout_row(nodes, options, face),
        Node::Symbol { ch, class } => Canvas::text(
            &ch.to_string(),
            role_for_class(*class),
            face.unwrap_or(Face::Roman),
        ),
        Node::Ident(text) => layout_ident(text, face),
        Node::Number(text) => {
            let f = face.unwrap_or(Face::Roman);
            Canvas::text(&map_face_text(text, f), Role::Number, f)
        }
        Node::Text { text, face: own } => {
            let f = face.unwrap_or(*own);
            Canvas::text(&map_face_text(text, f), Role::Text, f)
        }
        Node::Space(space) => Canvas::space(space_width(*space)),
        Node::Fraction { num, den, kind } => layout_fraction(num, den, *kind, options, face),
        Node::Radical { index, body } => layout_radical(index.as_deref(), body, options, face),
        Node::Script { base, sup, sub } => {
            layout_script(base, sup.as_deref(), sub.as_deref(), options, face)
        }
        Node::Fence { open, close, body } => layout_fence(*open, *close, body, options, face),
        Node::Matrix {
            open,
            close,
            rows,
            align,
        } => layout_matrix(*open, *close, rows, align, options, face),
        Node::Accent { kind, body } => layout_accent(*kind, body, options, face),
        Node::OverUnder { body, over, under } => {
            layout_overunder(body, over.as_deref(), under.as_deref(), options, face)
        }
        Node::BigOperator { ch, name, .. } => {
            if options.display && is_integral(name) {
                integral_canvas(name, face)
            } else {
                Canvas::text(&ch.to_string(), Role::Operator, face.unwrap_or(Face::Roman))
            }
        }
        Node::OperatorName(name) => Canvas::text(name, Role::Operator, face.unwrap_or(Face::Roman)),
        Node::Styled { face: own, body } => layout_node(body, options, Some(*own)),
        Node::Phantom { body, kind } => {
            let inner = layout_node(body, options, face);
            match kind {
                PhantomKind::Full => inner.phantom(),
                PhantomKind::Horizontal => {
                    let mut out = Canvas::blank(inner.width, 0);
                    out.baseline = 0;
                    out
                }
                PhantomKind::Vertical => {
                    let mut out = Canvas::blank(0, inner.height);
                    out.baseline = inner.baseline;
                    out
                }
            }
        }
        Node::Error(text) => Canvas::text(text, Role::Error, Face::Roman),
    }
}

fn layout_row(nodes: &[Node], options: &LayoutOptions, face: Option<Face>) -> Canvas {
    let mut parts = Vec::new();
    let mut previous: Option<AtomClass> = None;
    let mut unary_sign = false;

    for node in nodes {
        if let Node::Space(space) = node {
            parts.push(Canvas::space(space_width(*space)));
            previous = None;
            unary_sign = false;
            continue;
        }
        if matches!(node, Node::Row(inner) if inner.is_empty()) {
            continue;
        }

        let class = effective_class(node, previous);
        if let Some(previous) = previous {
            // Separate a unary sign from the fraction rule on the same baseline.
            let gap = spacing(previous, class).max(usize::from(
                unary_sign && matches!(node, Node::Fraction { .. }),
            ));
            if gap > 0 {
                parts.push(Canvas::space(gap));
            }
        }
        parts.push(layout_node(node, options, face));
        unary_sign = class == AtomClass::Ord
            && matches!(
                node,
                Node::Symbol {
                    ch: '+' | '-' | '−',
                    class: AtomClass::Bin
                }
            );
        previous = Some(class);
    }

    Canvas::hcat(&parts, 0)
}

pub(crate) fn effective_class(node: &Node, previous: Option<AtomClass>) -> AtomClass {
    let class = node.class();
    if class == AtomClass::Bin
        && matches!(
            previous,
            None | Some(AtomClass::Bin)
                | Some(AtomClass::Rel)
                | Some(AtomClass::Open)
                | Some(AtomClass::Punct)
        )
    {
        AtomClass::Ord
    } else {
        class
    }
}

pub(crate) fn spacing(previous: AtomClass, current: AtomClass) -> usize {
    if previous == AtomClass::Open || current == AtomClass::Close {
        return 0;
    }
    if previous == AtomClass::Punct {
        return 1;
    }
    if matches!(previous, AtomClass::Bin | AtomClass::Rel)
        || matches!(current, AtomClass::Bin | AtomClass::Rel)
    {
        return 1;
    }
    if previous == AtomClass::Op && current == AtomClass::Ord {
        return 1;
    }
    0
}

pub(crate) fn space_width(space: Space) -> usize {
    match space {
        Space::Thin | Space::Medium | Space::Thick | Space::Normal => 1,
        Space::Quad => 2,
        Space::DoubleQuad => 4,
        Space::NegativeThin => 0,
    }
}

fn role_for_class(class: AtomClass) -> Role {
    match class {
        AtomClass::Open | AtomClass::Close => Role::Delimiter,
        AtomClass::Rel => Role::Relation,
        AtomClass::Bin | AtomClass::Op => Role::Operator,
        AtomClass::Punct | AtomClass::Ord | AtomClass::Inner => Role::Text,
    }
}

fn layout_ident(text: &str, face: Option<Face>) -> Canvas {
    let face = face.unwrap_or(Face::Italic);
    let role = if face == Face::Italic {
        Role::Variable
    } else {
        Role::Text
    };
    Canvas::text(&map_face_text(text, face), role, face)
}

fn layout_fraction(
    num: &Node,
    den: &Node,
    _kind: FractionKind,
    options: &LayoutOptions,
    face: Option<Face>,
) -> Canvas {
    let num_c = layout_node(num, options, face);
    let den_c = layout_node(den, options, face);

    let inner = num_c.width.max(den_c.width);
    // Match libtexprintf's compact fraction shape: no extra side padding, but
    // keep at least three cells so a short fraction still reads as a fraction.
    let width = inner.max(3);
    let num_height = num_c.height.max(1);
    let den_height = den_c.height.max(1);
    let bar_y = num_height;
    let height = num_height + 1 + den_height;

    let mut out = Canvas::blank(width, height);
    out.baseline = bar_y;

    let num_x = (width.saturating_sub(num_c.width)) / 2;
    out.blit(&num_c, num_x, num_height.saturating_sub(num_c.height));

    for x in 0..width {
        out.set(x, bar_y, Cell::glyph("─", 1, Role::Rule, Face::Roman));
    }

    let den_x = (width.saturating_sub(den_c.width)) / 2;
    out.blit(&den_c, den_x, bar_y + 1);

    out
}

fn layout_radical(
    index: Option<&Node>,
    body: &Node,
    options: &LayoutOptions,
    face: Option<Face>,
) -> Canvas {
    let body_c = layout_node(body, options, face);
    let body_height = body_c.height.max(1);
    // A single symbol such as `\sqrt{x}` reads fine as `√x`. Anything wider
    // or taller gets an explicit overline so the scope of the root is clear.
    let extra_top = usize::from(body_c.width > 1 || body_c.height > 1);
    let width = body_c.width + 1;
    let height = body_height + extra_top;
    // Keep the surrounding text aligned to the body's baseline, but draw the
    // radical sign at the bottom of a tall body so it encloses the whole
    // radicand instead of appearing to apply only to the numerator.
    let sign_y = if body_height > 1 {
        height - 1
    } else {
        body_c.baseline + extra_top
    };
    let baseline = body_c.baseline + extra_top;

    let mut radical = Canvas::blank(width, height);
    radical.baseline = baseline;
    radical.set(0, sign_y, Cell::glyph("√", 1, Role::Operator, Face::Roman));
    radical.blit(&body_c, 1, extra_top);

    if extra_top > 0 {
        radical.set(0, 0, Cell::glyph("┌", 1, Role::Rule, Face::Roman));
        for x in 1..width {
            radical.set(x, 0, Cell::glyph("─", 1, Role::Rule, Face::Roman));
        }
        // Connect the overline to the bottom sign with a continuous left edge.
        for y in 1..sign_y {
            radical.set(0, y, Cell::glyph("│", 1, Role::Rule, Face::Roman));
        }
    }

    let Some(index) = index else {
        return radical;
    };

    if let Some(root) = simple_root_char(index) {
        let mut out = radical;
        out.set(
            0,
            sign_y,
            Cell::glyph(root.to_string(), 1, Role::Operator, Face::Roman),
        );
        return out;
    }

    let index_c = layout_node(index, options, face);
    let top = index_c.height.max(1);
    let mut out = Canvas::blank(index_c.width + radical.width, top + radical.height);
    out.baseline = top + radical.baseline;
    out.blit(&index_c, 0, 0);
    out.blit(&radical, index_c.width, top);
    out
}

fn layout_script(
    base: &Node,
    sup: Option<&Node>,
    sub: Option<&Node>,
    options: &LayoutOptions,
    face: Option<Face>,
) -> Canvas {
    let base_c = layout_node(base, options, face);

    let use_limits = options.display
        && match base {
            Node::BigOperator {
                limits,
                display_limits,
                name,
                ..
            } => {
                is_integral(name)
                    || matches!(limits, LimitMode::Limits)
                    || (matches!(limits, LimitMode::Auto) && *display_limits)
            }
            Node::OperatorName(name) => is_limit_operator(name),
            _ => false,
        };

    if use_limits {
        let sup_c = sup.map(|node| layout_node(node, options, face));
        let sub_c = sub.map(|node| layout_node(node, options, face));
        return layout_limits(&base_c, sup_c.as_ref(), sub_c.as_ref());
    }

    // Prefer a compact Unicode form (`b²`, `xᵢ`, `xⁿ⁺¹`) when the base is a
    // single row and every script character has a Unicode super/subscript.
    // This is both shorter and avoids the blank column that side scripts leave
    // between a base and the following atom.
    if let Some(compact) = compact_script(base, sup, sub, options, face) {
        return compact;
    }

    // Terminal cells cannot shrink nested scripts; linearize their contents
    // so a fraction or another script does not build a staircase above the base.
    let script = |node: &Node| {
        Canvas::text(
            &render_inline(node),
            Role::Text,
            face.unwrap_or(Face::Roman),
        )
    };
    let sup_c = sup.map(script);
    let sub_c = sub.map(script);
    layout_side_scripts(&base_c, sup_c.as_ref(), sub_c.as_ref())
}

fn compact_script(
    base: &Node,
    sup: Option<&Node>,
    sub: Option<&Node>,
    options: &LayoutOptions,
    face: Option<Face>,
) -> Option<Canvas> {
    let base_c = layout_node(base, options, face);
    if base_c.height != 1 {
        return None;
    }

    let mut script = String::new();
    if let Some(sub) = sub {
        let text = render_inline(sub).replace(' ', "");
        script.push_str(&to_subscript(&text)?);
    }
    if let Some(sup) = sup {
        let text = render_inline(sup).replace(' ', "");
        script.push_str(&to_superscript(&text)?);
    }
    if script.is_empty() {
        return None;
    }

    let script_c = Canvas::text(&script, Role::Text, face.unwrap_or(Face::Roman));
    Some(Canvas::hcat(&[base_c, script_c], 0))
}

fn layout_limits(base: &Canvas, sup: Option<&Canvas>, sub: Option<&Canvas>) -> Canvas {
    let width = base
        .width
        .max(sup.map_or(0, |c| c.width))
        .max(sub.map_or(0, |c| c.width));
    let sup_height = sup.map_or(0, |c| c.height);
    let sub_height = sub.map_or(0, |c| c.height);
    let height = sup_height + base.height + sub_height;

    let mut out = Canvas::blank(width, height);
    out.baseline = sup_height + base.baseline;
    if let Some(sup) = sup {
        out.blit(sup, (width.saturating_sub(sup.width)) / 2, 0);
    }
    out.blit(base, (width.saturating_sub(base.width)) / 2, sup_height);
    if let Some(sub) = sub {
        out.blit(
            sub,
            (width.saturating_sub(sub.width)) / 2,
            sup_height + base.height,
        );
    }
    out
}

fn layout_side_scripts(base: &Canvas, sup: Option<&Canvas>, sub: Option<&Canvas>) -> Canvas {
    let sup_width = sup.map_or(0, |c| c.width);
    let sub_width = sub.map_or(0, |c| c.width);
    let width = base.width + sup_width.max(sub_width);

    let sup_top = sup.map(|canvas| base.baseline as isize - canvas.height as isize);
    let sub_top = sub.map(|_| base.baseline as isize + 1);

    let top = sup_top.map_or(0, |value| value.min(0));
    let mut bottom = base.height as isize;
    if let Some(sub) = sub {
        bottom = bottom.max(base.baseline as isize + 1 + sub.height as isize);
    }
    let height = (bottom - top).max(0) as usize;

    let mut out = Canvas::blank(width, height);
    out.baseline = (base.baseline as isize - top) as usize;

    let base_y = (0 - top) as usize;
    out.blit(base, 0, base_y);

    if let (Some(sup), Some(sup_top)) = (sup, sup_top) {
        out.blit(sup, base.width, (sup_top - top) as usize);
    }
    if let (Some(sub), Some(sub_top)) = (sub, sub_top) {
        out.blit(sub, base.width, (sub_top - top) as usize);
    }

    out
}

fn layout_fence(
    open: Delimiter,
    close: Delimiter,
    body: &Node,
    options: &LayoutOptions,
    face: Option<Face>,
) -> Canvas {
    let body_c = layout_node(body, options, face);
    let height = body_c.height.max(1);
    let left = delimiter_canvas(open, height, body_c.baseline, true);
    let right = delimiter_canvas(close, height, body_c.baseline, false);
    Canvas::hcat(&[left, body_c, right], 1)
}

fn is_integral(name: &str) -> bool {
    matches!(
        name,
        "int" | "iint" | "iiint" | "iiiint" | "oint" | "oiint" | "oiiint"
    )
}

fn integral_count(name: &str) -> usize {
    match name {
        "iint" | "oiint" => 2,
        "iiint" | "oiiint" => 3,
        "iiiint" => 4,
        _ => 1,
    }
}

fn integral_canvas(name: &str, face: Option<Face>) -> Canvas {
    let count = integral_count(name);
    let face = face.unwrap_or(Face::Roman);
    let mut out = Canvas::blank(count, 3);
    out.baseline = 1;
    for x in 0..count {
        out.set(x, 0, Cell::glyph("⌠", 1, Role::Operator, face));
        out.set(x, 1, Cell::glyph("⎮", 1, Role::Operator, face));
        out.set(x, 2, Cell::glyph("⌡", 1, Role::Operator, face));
    }
    out
}

fn is_limit_operator(name: &str) -> bool {
    matches!(
        name,
        "lim" | "lim sup" | "lim inf" | "max" | "min" | "sup" | "inf" | "det" | "gcd" | "Pr"
    )
}

fn delimiter_canvas(delimiter: Delimiter, height: usize, baseline: usize, open: bool) -> Canvas {
    if delimiter == Delimiter::None {
        return Canvas::empty();
    }

    let height = height.max(1);
    if height == 1 {
        return Canvas::text(
            &symbols::delimiter_char(delimiter, open).to_string(),
            Role::Delimiter,
            Face::Roman,
        );
    }

    let mut out = Canvas::blank(1, height);
    out.baseline = baseline.min(height - 1);
    let middle = height / 2;

    let (top, middle_char, bottom, extension) = match delimiter {
        Delimiter::None => (' ', ' ', ' ', ' '),
        Delimiter::Paren => {
            if open {
                ('⎛', '⎜', '⎝', '⎜')
            } else {
                ('⎞', '⎟', '⎠', '⎟')
            }
        }
        Delimiter::Bracket => {
            if open {
                ('⎡', '⎢', '⎣', '⎢')
            } else {
                ('⎤', '⎥', '⎦', '⎥')
            }
        }
        Delimiter::Brace => {
            if open {
                ('⎧', '⎨', '⎩', '⎪')
            } else {
                ('⎫', '⎬', '⎭', '⎪')
            }
        }
        Delimiter::Angle => {
            if open {
                ('╱', '│', '╲', '│')
            } else {
                ('╲', '│', '╱', '│')
            }
        }
        Delimiter::Bar => ('│', '│', '│', '│'),
        Delimiter::DoubleBar => ('║', '║', '║', '║'),
        Delimiter::Floor => {
            if open {
                ('⎢', '⎢', '⎣', '⎢')
            } else {
                ('⎥', '⎥', '⎦', '⎥')
            }
        }
        Delimiter::Ceil => {
            if open {
                ('⎡', '⎢', '⎢', '⎢')
            } else {
                ('⎤', '⎥', '⎥', '⎥')
            }
        }
        Delimiter::Slash => ('╱', '╱', '╱', '╱'),
        Delimiter::Backslash => ('╲', '╲', '╲', '╲'),
        Delimiter::Custom(ch) => (ch, ch, ch, ch),
    };

    for y in 0..height {
        let ch = if y == 0 {
            top
        } else if y == height - 1 {
            bottom
        } else if (y == middle && matches!(delimiter, Delimiter::Brace))
            || matches!(
                delimiter,
                Delimiter::Angle | Delimiter::Bar | Delimiter::DoubleBar
            )
        {
            middle_char
        } else {
            extension
        };
        out.set(
            0,
            y,
            Cell::glyph(ch.to_string(), 1, Role::Delimiter, Face::Roman),
        );
    }

    out
}

fn layout_matrix(
    open: Delimiter,
    close: Delimiter,
    rows: &[Vec<Node>],
    align: &[Align],
    options: &LayoutOptions,
    face: Option<Face>,
) -> Canvas {
    if rows.is_empty() {
        return Canvas::empty();
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return Canvas::empty();
    }

    let mut cells: Vec<Vec<Canvas>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|node| layout_node(node, options, face))
                .collect::<Vec<_>>()
        })
        .collect();
    for row in &mut cells {
        while row.len() < columns {
            row.push(Canvas::empty());
        }
    }

    let mut column_widths = vec![0usize; columns];
    for row in &cells {
        for (column, cell) in row.iter().enumerate() {
            column_widths[column] = column_widths[column].max(cell.width);
        }
    }

    let mut row_baselines = vec![0usize; rows.len()];
    let mut row_heights = vec![0usize; rows.len()];
    for (index, row) in cells.iter().enumerate() {
        let baseline = row.iter().map(|cell| cell.baseline).max().unwrap_or(0);
        let below = row
            .iter()
            .map(|cell| cell.height.saturating_sub(cell.baseline))
            .max()
            .unwrap_or(0);
        row_baselines[index] = baseline;
        row_heights[index] = (baseline + below).max(1);
    }

    let gap = 2usize;
    let grid_width =
        column_widths.iter().sum::<usize>() + gap.saturating_mul(columns.saturating_sub(1));
    let grid_height = row_heights.iter().sum::<usize>();

    let mut grid = Canvas::blank(grid_width, grid_height);
    grid.baseline = grid_height / 2;

    let mut y = 0usize;
    for row in 0..rows.len() {
        let mut x = 0usize;
        for column in 0..columns {
            let cell = &cells[row][column];
            let alignment = align.get(column).copied().unwrap_or(Align::Center);
            let offset = match alignment {
                Align::Left => 0,
                Align::Center => column_widths[column].saturating_sub(cell.width) / 2,
                Align::Right => column_widths[column].saturating_sub(cell.width),
            };
            let dy = y + row_baselines[row].saturating_sub(cell.baseline);
            grid.blit(cell, x + offset, dy);
            x += column_widths[column] + gap;
        }
        y += row_heights[row];
    }

    let height = grid.height;
    let left = delimiter_canvas(open, height, grid.baseline, true);
    let right = delimiter_canvas(close, height, grid.baseline, false);
    Canvas::hcat(&[left, grid, right], 0)
}

fn layout_accent(
    kind: AccentKind,
    body: &Node,
    options: &LayoutOptions,
    face: Option<Face>,
) -> Canvas {
    let body_c = layout_node(body, options, face);

    if body_c.width == 1 && body_c.height == 1 {
        if let Some(combining) = combining_accent(kind) {
            let mut out = body_c;
            if let Some(cell) = out
                .cells
                .iter_mut()
                .find(|cell| cell.kind == CellKind::Glyph)
            {
                cell.glyph.push(combining);
            }
            return out;
        }
    }

    if kind == AccentKind::Underline {
        let width = body_c.width.max(1);
        let mut out = Canvas::blank(width, body_c.height + 1);
        out.baseline = body_c.baseline;
        out.blit(&body_c, 0, 0);
        let line = Canvas::text(&"─".repeat(width), Role::Rule, Face::Roman);
        out.blit(&line, 0, body_c.height);
        return out;
    }

    let width = body_c.width.max(1);
    let accent = accent_row(kind, width);
    let mut out = Canvas::blank(width, body_c.height + 1);
    out.baseline = body_c.baseline + 1;
    out.blit(&body_c, 0, 1);
    out.blit(&Canvas::text(&accent, Role::Accent, Face::Roman), 0, 0);
    out
}

fn accent_row(kind: AccentKind, width: usize) -> String {
    let width = width.max(1);
    match kind {
        AccentKind::Bar | AccentKind::Overline => "─".repeat(width),
        AccentKind::Hat => centered_char(width, '^'),
        AccentKind::Vec => centered_char(width, '→'),
        AccentKind::Dot => centered_char(width, '˙'),
        AccentKind::Ddot => centered_char(width, '¨'),
        AccentKind::Tilde => centered_char(width, '~'),
        AccentKind::Check => centered_char(width, 'ˇ'),
        AccentKind::Breve => centered_char(width, '˘'),
        AccentKind::Acute => centered_char(width, '´'),
        AccentKind::Grave => centered_char(width, '`'),
        AccentKind::Underline => "─".repeat(width),
    }
}

fn centered_char(width: usize, ch: char) -> String {
    if width <= 1 {
        return ch.to_string();
    }
    let left = (width - 1) / 2;
    let right = width - 1 - left;
    format!("{}{}{}", " ".repeat(left), ch, " ".repeat(right))
}

fn layout_overunder(
    body: &Node,
    over: Option<&Node>,
    under: Option<&Node>,
    options: &LayoutOptions,
    face: Option<Face>,
) -> Canvas {
    let body_c = layout_node(body, options, face);
    let over_c = over.map(|node| layout_node(node, options, face));
    let under_c = under.map(|node| layout_node(node, options, face));

    let width = body_c
        .width
        .max(over_c.as_ref().map_or(0, |canvas| canvas.width))
        .max(under_c.as_ref().map_or(0, |canvas| canvas.width));
    let over_height = over_c.as_ref().map_or(0, |canvas| canvas.height);
    let under_height = under_c.as_ref().map_or(0, |canvas| canvas.height);
    let height = over_height + body_c.height + under_height;

    let mut out = Canvas::blank(width, height);
    out.baseline = over_height + body_c.baseline;

    if let Some(over) = &over_c {
        out.blit(over, (width.saturating_sub(over.width)) / 2, 0);
    }
    out.blit(
        &body_c,
        (width.saturating_sub(body_c.width)) / 2,
        over_height,
    );
    if let Some(under) = &under_c {
        out.blit(
            under,
            (width.saturating_sub(under.width)) / 2,
            over_height + body_c.height,
        );
    }

    out
}

pub(crate) fn map_face_text(text: &str, face: Face) -> String {
    let text = strip_controls(text);
    match face {
        Face::Blackboard => text
            .chars()
            .map(|ch| match ch {
                'A' => '𝔸',
                'B' => '𝔹',
                'C' => 'ℂ',
                'D' => '𝔻',
                'E' => '𝔼',
                'F' => '𝔽',
                'G' => '𝔾',
                'H' => 'ℍ',
                'I' => '𝕀',
                'J' => '𝕁',
                'K' => '𝕂',
                'L' => '𝕃',
                'M' => '𝕄',
                'N' => 'ℕ',
                'O' => '𝕆',
                'P' => 'ℙ',
                'Q' => 'ℚ',
                'R' => 'ℝ',
                'S' => '𝕊',
                'T' => '𝕋',
                'U' => '𝕌',
                'V' => '𝕍',
                'W' => '𝕎',
                'X' => '𝕏',
                'Y' => '𝕐',
                'Z' => 'ℤ',
                '0' => '𝟘',
                '1' => '𝟙',
                '2' => '𝟚',
                '3' => '𝟛',
                '4' => '𝟜',
                '5' => '𝟝',
                '6' => '𝟞',
                '7' => '𝟟',
                '8' => '𝟠',
                '9' => '𝟡',
                other => other,
            })
            .collect(),
        Face::Calligraphic => text
            .chars()
            .map(|ch| match ch {
                'A' => '𝒜',
                'B' => 'ℬ',
                'C' => '𝒞',
                'D' => '𝒟',
                'E' => 'ℰ',
                'F' => 'ℱ',
                'G' => '𝒢',
                'H' => 'ℋ',
                'I' => 'ℐ',
                'J' => '𝒥',
                'K' => '𝒦',
                'L' => 'ℒ',
                'M' => 'ℳ',
                'N' => '𝒩',
                'O' => '𝒪',
                'P' => '𝒫',
                'Q' => '𝒬',
                'R' => 'ℛ',
                'S' => '𝒮',
                'T' => '𝒯',
                'U' => '𝒰',
                'V' => '𝒱',
                'W' => '𝒲',
                'X' => '𝒳',
                'Y' => '𝒴',
                'Z' => '𝒵',
                other => other,
            })
            .collect(),
        Face::Fraktur => text
            .chars()
            .map(|ch| match ch {
                'A' => '𝔄',
                'B' => '𝔅',
                'C' => 'ℭ',
                'D' => '𝔇',
                'E' => '𝔈',
                'F' => '𝔉',
                'G' => '𝔊',
                'H' => 'ℌ',
                'I' => 'ℑ',
                'J' => '𝔍',
                'K' => '𝔎',
                'L' => '𝔏',
                'M' => '𝔐',
                'N' => '𝔑',
                'O' => '𝔒',
                'P' => '𝔓',
                'Q' => '𝔔',
                'R' => 'ℜ',
                'S' => '𝔖',
                'T' => '𝔗',
                'U' => '𝔘',
                'V' => '𝔙',
                'W' => '𝔚',
                'X' => '𝔛',
                'Y' => '𝔜',
                'Z' => 'ℨ',
                'a' => '𝔞',
                'b' => '𝔟',
                'c' => '𝔠',
                'd' => '𝔡',
                'e' => '𝔢',
                'f' => '𝔣',
                'g' => '𝔤',
                'h' => '𝔥',
                'i' => '𝔦',
                'j' => '𝔧',
                'k' => '𝔨',
                'l' => '𝔩',
                'm' => '𝔪',
                'n' => '𝔫',
                'o' => '𝔬',
                'p' => '𝔭',
                'q' => '𝔮',
                'r' => '𝔯',
                's' => '𝔰',
                't' => '𝔱',
                'u' => '𝔲',
                'v' => '𝔳',
                'w' => '𝔴',
                'x' => '𝔵',
                'y' => '𝔶',
                'z' => '𝔷',
                other => other,
            })
            .collect(),
        _ => text,
    }
}
