//! Single-line rendering, for inline math inside prose.
//!
//! Block layout can grow vertically. Inline math cannot: a terminal line has a
//! fixed height, and inserting a fraction would reflow the surrounding text.
//! This module therefore uses compact textual forms (`x²`, `a/b`, `√(x)`), and
//! falls back to source when a construct has no honest one-line form.

use latex_core::ast::{AccentKind, AtomClass, Delimiter, Face, Node, PhantomKind};
use latex_core::symbols;
use unicode_width::UnicodeWidthStr;

use crate::layout::{effective_class, map_face_text, space_width, spacing};

pub fn render_inline(node: &Node) -> String {
    render_node(node, None, false).replace('\n', " ")
}

/// Compact form used when the spaced form would not fit the source span.
/// Automatic inter-atom spacing is suppressed; explicit `\,`/`\quad` still
/// produce space.
pub fn render_inline_compact(node: &Node) -> String {
    render_node(node, None, true).replace('\n', " ")
}

fn render_node(node: &Node, face: Option<Face>, compact: bool) -> String {
    match node {
        Node::Row(nodes) => render_row(nodes, face, compact),
        Node::Symbol { ch, .. } => {
            if ch.is_control() {
                String::new()
            } else {
                ch.to_string()
            }
        }
        Node::Ident(text) => {
            let face = face.unwrap_or(Face::Italic);
            map_face_text(text, face)
        }
        Node::Number(text) => map_face_text(text, face.unwrap_or(Face::Roman)),
        Node::Text { text, face: own } => map_face_text(text, face.unwrap_or(*own)),
        Node::Space(space) => " ".repeat(space_width(*space)),
        Node::Fraction { num, den, .. } => {
            let num = render_operand(num, face, compact);
            let den = render_operand(den, face, compact);
            if num.contains('/') || den.contains('/') {
                format!("({num})/({den})")
            } else {
                format!("{num}/{den}")
            }
        }
        Node::Radical { index, body } => {
            let body_text = render_node(body, face, compact);
            match index {
                Some(index) => {
                    if let Some(root) = simple_root_char(index) {
                        format!("{root}({body_text})")
                    } else {
                        format!("√[{}]({body_text})", render_node(index, face, compact))
                    }
                }
                None => format!("√({body_text})"),
            }
        }
        Node::Script { base, sup, sub } => {
            render_script(base, sup.as_deref(), sub.as_deref(), face, compact)
        }
        Node::Fence { open, close, body } => {
            let open = delimiter_text(*open, true);
            let close = delimiter_text(*close, false);
            format!("{open}{}{close}", render_node(body, face, compact))
        }
        Node::Matrix {
            open, close, rows, ..
        } => {
            let open = delimiter_text(*open, true);
            let close = delimiter_text(*close, false);
            let rows = rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|node| render_node(node, face, compact))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .collect::<Vec<_>>()
                .join("; ");
            format!("{open}{rows}{close}")
        }
        Node::Accent { kind, body } => render_accent(*kind, body, face, compact),
        Node::OverUnder { body, over, under } => {
            let mut out = render_operand(body, face, compact);
            if let Some(over) = over {
                out.push('^');
                out.push_str(&wrap_script(&render_node(over, face, compact)));
            }
            if let Some(under) = under {
                out.push('_');
                out.push_str(&wrap_script(&render_node(under, face, compact)));
            }
            out
        }
        Node::BigOperator { ch, .. } => ch.to_string(),
        Node::OperatorName(name) => map_face_text(name, Face::Roman),
        Node::Styled { face: own, body } => render_node(body, Some(*own), compact),
        Node::Phantom { body, kind } => match kind {
            PhantomKind::Vertical => String::new(),
            PhantomKind::Full | PhantomKind::Horizontal => {
                let text = render_node(body, face, compact);
                " ".repeat(UnicodeWidthStr::width(text.as_str()))
            }
        },
        Node::Error(text) => map_face_text(text, Face::Roman),
    }
}

fn render_row(nodes: &[Node], face: Option<Face>, compact: bool) -> String {
    let mut out = String::new();
    let mut previous: Option<AtomClass> = None;

    for node in nodes {
        if let Node::Space(space) = node {
            out.push_str(&" ".repeat(space_width(*space)));
            previous = None;
            continue;
        }
        if matches!(node, Node::Row(inner) if inner.is_empty()) {
            continue;
        }

        let class = effective_class(node, previous);
        if let Some(previous) = previous {
            if !compact {
                let gap = spacing(previous, class);
                if gap > 0 {
                    out.push_str(&" ".repeat(gap));
                }
            }
        }
        out.push_str(&render_node(node, face, compact));
        previous = Some(class);
    }

    out
}

fn render_operand(node: &Node, face: Option<Face>, compact: bool) -> String {
    let text = render_node(node, face, compact);
    if needs_parens(node) {
        format!("({text})")
    } else {
        text
    }
}

fn needs_parens(node: &Node) -> bool {
    match node {
        Node::Row(nodes) => {
            let atoms: Vec<&Node> = nodes
                .iter()
                .filter(|node| !matches!(node, Node::Space(_)))
                .collect();
            // Products also need grouping: a/(2σ²) must not become a/2σ².
            atoms.len() > 1
        }
        Node::Matrix { .. } => true,
        _ => false,
    }
}

fn render_script(
    base: &Node,
    sup: Option<&Node>,
    sub: Option<&Node>,
    face: Option<Face>,
    compact: bool,
) -> String {
    let base = render_operand(base, face, compact);
    let sup = sup.map(|node| render_node(node, face, compact));
    let sub = sub.map(|node| render_node(node, face, compact));

    match (sup, sub) {
        (Some(sup), Some(sub)) => {
            let compact_sup = sup.replace(' ', "");
            let compact_sub = sub.replace(' ', "");
            match (to_superscript(&compact_sup), to_subscript(&compact_sub)) {
                (Some(sup), Some(sub)) => format!("{base}{sup}{sub}"),
                _ => format!("{base}_{}^{}", wrap_script(&sub), wrap_script(&sup)),
            }
        }
        (Some(sup), None) => {
            let compact = sup.replace(' ', "");
            match to_superscript(&compact) {
                Some(sup) => format!("{base}{sup}"),
                None => format!("{base}^{}", wrap_script(&sup)),
            }
        }
        (None, Some(sub)) => {
            let compact = sub.replace(' ', "");
            match to_subscript(&compact) {
                Some(sub) => format!("{base}{sub}"),
                None => format!("{base}_{}", wrap_script(&sub)),
            }
        }
        (None, None) => base,
    }
}

fn wrap_script(text: &str) -> String {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) if ch.is_alphanumeric() || matches!(ch, '′' | '″' | '+' | '-') => {
            text.to_string()
        }
        _ => format!("({text})"),
    }
}

fn render_accent(kind: AccentKind, body: &Node, face: Option<Face>, compact: bool) -> String {
    let body = render_node(body, face, compact);
    if body.chars().count() == 1 {
        if let Some(combining) = combining_accent(kind) {
            return format!("{body}{combining}");
        }
    }
    match kind {
        AccentKind::Underline => format!("underline({body})"),
        AccentKind::Overline => format!("overline({body})"),
        AccentKind::Bar => format!("bar({body})"),
        AccentKind::Hat => format!("hat({body})"),
        AccentKind::Vec => format!("vec({body})"),
        AccentKind::Dot => format!("dot({body})"),
        AccentKind::Ddot => format!("ddot({body})"),
        AccentKind::Tilde => format!("tilde({body})"),
        AccentKind::Check => format!("check({body})"),
        AccentKind::Breve => format!("breve({body})"),
        AccentKind::Acute => format!("acute({body})"),
        AccentKind::Grave => format!("grave({body})"),
    }
}

pub(crate) fn combining_accent(kind: AccentKind) -> Option<char> {
    Some(match kind {
        AccentKind::Hat => '\u{0302}',
        AccentKind::Bar => '\u{0304}',
        AccentKind::Vec => '\u{20D7}',
        AccentKind::Dot => '\u{0307}',
        AccentKind::Ddot => '\u{0308}',
        AccentKind::Tilde => '\u{0303}',
        AccentKind::Check => '\u{030C}',
        AccentKind::Breve => '\u{0306}',
        AccentKind::Acute => '\u{0301}',
        AccentKind::Grave => '\u{0300}',
        AccentKind::Overline => return None,
        AccentKind::Underline => return None,
    })
}

fn delimiter_text(delimiter: Delimiter, open: bool) -> String {
    if delimiter == Delimiter::None {
        String::new()
    } else {
        symbols::delimiter_char(delimiter, open).to_string()
    }
}

pub(crate) fn simple_root_char(node: &Node) -> Option<char> {
    match node {
        Node::Number(text) if text == "3" => Some('∛'),
        Node::Number(text) if text == "4" => Some('∜'),
        Node::Row(nodes) if nodes.len() == 1 => simple_root_char(&nodes[0]),
        _ => None,
    }
}

pub(crate) fn to_superscript(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let mut out = String::new();
    for ch in text.chars() {
        out.push(superscript(ch)?);
    }
    Some(out)
}

pub(crate) fn to_subscript(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let mut out = String::new();
    for ch in text.chars() {
        out.push(subscript(ch)?);
    }
    Some(out)
}

fn superscript(ch: char) -> Option<char> {
    Some(match ch {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'a' => 'ᵃ',
        'b' => 'ᵇ',
        'c' => 'ᶜ',
        'd' => 'ᵈ',
        'e' => 'ᵉ',
        'f' => 'ᶠ',
        'g' => 'ᵍ',
        'h' => 'ʰ',
        'i' => 'ⁱ',
        'j' => 'ʲ',
        'k' => 'ᵏ',
        'l' => 'ˡ',
        'm' => 'ᵐ',
        'n' => 'ⁿ',
        'o' => 'ᵒ',
        'p' => 'ᵖ',
        'r' => 'ʳ',
        's' => 'ˢ',
        't' => 'ᵗ',
        'u' => 'ᵘ',
        'v' => 'ᵛ',
        'w' => 'ʷ',
        'x' => 'ˣ',
        'y' => 'ʸ',
        'z' => 'ᶻ',
        'A' => 'ᴬ',
        'B' => 'ᴮ',
        'D' => 'ᴰ',
        'E' => 'ᴱ',
        'G' => 'ᴳ',
        'H' => 'ᴴ',
        'I' => 'ᴵ',
        'J' => 'ᴶ',
        'K' => 'ᴷ',
        'L' => 'ᴸ',
        'M' => 'ᴹ',
        'N' => 'ᴺ',
        'O' => 'ᴼ',
        'P' => 'ᴾ',
        'R' => 'ᴿ',
        'T' => 'ᵀ',
        'U' => 'ᵁ',
        'V' => 'ⱽ',
        'W' => 'ᵂ',
        'α' => 'ᵅ',
        'β' => 'ᵝ',
        'γ' => 'ᵞ',
        'δ' => 'ᵟ',
        'ε' => 'ᵋ',
        'θ' => 'ᶿ',
        'ι' => 'ᶥ',
        'φ' => 'ᵠ',
        'χ' => 'ᵡ',
        _ => return None,
    })
}

fn subscript(ch: char) -> Option<char> {
    Some(match ch {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'h' => 'ₕ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'o' => 'ₒ',
        'p' => 'ₚ',
        'r' => 'ᵣ',
        's' => 'ₛ',
        't' => 'ₜ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        'x' => 'ₓ',
        'β' => 'ᵦ',
        'γ' => 'ᵧ',
        'ρ' => 'ᵨ',
        'φ' => 'ᵩ',
        'χ' => 'ᵪ',
        _ => return None,
    })
}
