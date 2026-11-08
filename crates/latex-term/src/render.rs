//! Public rendering facade: source text in, terminal text out.

use latex_core::ast::Align;
use latex_core::{parse, Diagnostic, Node};
use latex_layout::{layout, render_inline, render_inline_compact, LayoutOptions};
use unicode_width::UnicodeWidthStr;

use crate::paint::paint;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Full 2D layout, potentially several rows tall.
    Block,
    /// Compact single-line form for inline math.
    Inline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    /// Show the original source when rendering is not possible.
    Source,
    /// Emit nothing.
    Empty,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub mode: Mode,
    pub theme: Theme,
    pub max_width: Option<usize>,
    pub align: Align,
    pub indent: usize,
    pub fallback: Fallback,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Block,
            theme: Theme::auto(),
            max_width: None,
            align: Align::Left,
            indent: 0,
            fallback: Fallback::Source,
        }
    }
}

impl Options {
    pub fn inline() -> Self {
        Self {
            mode: Mode::Inline,
            ..Self::default()
        }
    }

    pub fn block() -> Self {
        Self {
            mode: Mode::Block,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderOutput {
    pub text: String,
    pub width: usize,
    pub height: usize,
    /// Baseline row inside the rendered canvas. Zero for inline/fallback.
    pub baseline: usize,
    pub diagnostics: Vec<Diagnostic>,
    /// True when the source was shown instead of a rendered form.
    pub fallback: bool,
}

impl RenderOutput {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

pub fn render(source: &str, options: &Options) -> RenderOutput {
    let parsed = parse(source);
    let diagnostics = parsed.diagnostics;

    if source.trim().is_empty() {
        return RenderOutput {
            text: String::new(),
            width: 0,
            height: 0,
            baseline: 0,
            diagnostics,
            fallback: false,
        };
    }

    match options.mode {
        Mode::Inline => render_inline_mode(source, &parsed.ast, diagnostics, options),
        Mode::Block => render_block_mode(source, &parsed.ast, diagnostics, options),
    }
}

fn render_inline_mode(
    source: &str,
    ast: &Node,
    diagnostics: Vec<Diagnostic>,
    options: &Options,
) -> RenderOutput {
    let inline = render_inline(ast);
    let width = UnicodeWidthStr::width(inline.as_str());
    let too_wide = options.max_width.map(|max| width > max).unwrap_or(false);

    if inline.trim().is_empty() || too_wide {
        // Inline math often has to fit the original `$...$` span. Try the
        // compact form (no automatic inter-atom spaces) before giving up.
        let compact = render_inline_compact(ast);
        let compact_width = UnicodeWidthStr::width(compact.as_str());
        let compact_too_wide = options
            .max_width
            .map(|max| compact_width > max)
            .unwrap_or(false);
        if !compact.trim().is_empty() && !compact_too_wide {
            return RenderOutput {
                text: compact,
                width: compact_width,
                height: 1,
                baseline: 0,
                diagnostics,
                fallback: false,
            };
        }
        return fallback(source, diagnostics, options);
    }

    RenderOutput {
        text: inline,
        width,
        height: 1,
        baseline: 0,
        diagnostics,
        fallback: false,
    }
}

fn render_block_mode(
    source: &str,
    ast: &Node,
    diagnostics: Vec<Diagnostic>,
    options: &Options,
) -> RenderOutput {
    let canvas = layout(ast, &LayoutOptions { display: true });
    if canvas.width == 0 && canvas.height == 0 {
        return fallback(source, diagnostics, options);
    }

    let too_wide = options
        .max_width
        .map(|max| canvas.width > max)
        .unwrap_or(false);

    if too_wide {
        // A too-wide 2D block is still useful as a compact inline expression.
        let inline = render_inline(ast);
        let inline_width = UnicodeWidthStr::width(inline.as_str());
        let inline_fits = options
            .max_width
            .map(|max| inline_width <= max)
            .unwrap_or(true);
        if !inline.trim().is_empty() && inline_fits {
            return RenderOutput {
                text: inline,
                width: inline_width,
                height: 1,
                baseline: 0,
                diagnostics,
                // This is a rendered form, not the original source. Callers
                // rely on `fallback` to decide whether to echo TeX.
                fallback: false,
            };
        }
        if let Some(lines) = options
            .max_width
            .and_then(|width| wrap_inline(&inline, width))
        {
            return RenderOutput {
                width: lines
                    .iter()
                    .map(|line| UnicodeWidthStr::width(line.as_str()))
                    .max()
                    .unwrap_or(0),
                height: lines.len(),
                text: lines.join("\n"),
                baseline: 0,
                diagnostics,
                fallback: false,
            };
        }
        return fallback(source, diagnostics, options);
    }

    let text = paint(
        &canvas,
        &options.theme,
        options.align,
        options.max_width,
        options.indent,
    );

    RenderOutput {
        text,
        width: canvas.width,
        height: canvas.height,
        baseline: canvas.baseline,
        diagnostics,
        fallback: false,
    }
}

// Wrap the linear form rather than slicing through fractions or script rows.
fn wrap_inline(text: &str, width: usize) -> Option<Vec<String>> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    for word in text.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if word_width > width {
            return None;
        }
        if !line.is_empty() && used + 1 + word_width > width {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        if !line.is_empty() {
            line.push(' ');
            used += 1;
        }
        line.push_str(word);
        used += word_width;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines)
    }
}

fn fallback(source: &str, diagnostics: Vec<Diagnostic>, options: &Options) -> RenderOutput {
    match options.fallback {
        Fallback::Empty => RenderOutput {
            text: String::new(),
            width: 0,
            height: 0,
            baseline: 0,
            diagnostics,
            fallback: true,
        },
        Fallback::Source => {
            let text = source.trim().to_string();
            let width = text.lines().map(UnicodeWidthStr::width).max().unwrap_or(0);
            let height = text.lines().count();
            RenderOutput {
                text,
                width,
                height,
                baseline: 0,
                diagnostics,
                fallback: true,
            }
        }
    }
}
