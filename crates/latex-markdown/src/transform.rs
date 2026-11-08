//! Replace math spans with rendered terminal output while preserving the rest
//! of the Markdown text verbatim.

use latex_core::ast::Align;
use latex_term::{render, Fallback, Mode, Options as RenderOptions, Theme};
use unicode_width::UnicodeWidthStr;

use crate::scan::{extract, MathDelim, Segment};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlinePolicy {
    /// Render compact math when possible.
    Render,
    /// Leave inline math untouched.
    Preserve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockPolicy {
    /// Render 2D art when possible.
    Render,
    /// Leave display math untouched.
    Preserve,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub width: Option<usize>,
    pub theme: Theme,
    pub inline_policy: InlinePolicy,
    pub block_policy: BlockPolicy,
    /// Append the original TeX source under block art.
    pub keep_source: bool,
    /// Wrap block art in a `text` code fence so Markdown renderers preserve
    /// its whitespace. Disable for plain terminal output.
    pub fence: bool,
    pub align: Align,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            width: None,
            theme: Theme::auto(),
            inline_policy: InlinePolicy::Render,
            block_policy: BlockPolicy::Render,
            keep_source: false,
            fence: true,
            align: Align::Left,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub total: usize,
    pub rendered: usize,
    pub fallback: usize,
}

pub fn transform(input: &str, options: &Options) -> String {
    transform_with_stats(input, options).0
}

pub fn transform_with_stats(input: &str, options: &Options) -> (String, Stats) {
    let segments = extract(input);
    let mut out = String::new();
    let mut stats = Stats::default();

    for (index, segment) in segments.iter().enumerate() {
        match segment {
            Segment::Text(text) => out.push_str(text),
            Segment::Math {
                source,
                display,
                delim,
            } => {
                stats.total += 1;
                if *display {
                    render_block(
                        &mut out, source, *delim, options, &segments, index, &mut stats,
                    );
                } else {
                    render_inline(&mut out, source, *delim, options, &mut stats);
                }
            }
        }
    }

    (out, stats)
}

fn render_inline(
    out: &mut String,
    source: &str,
    delim: MathDelim,
    options: &Options,
    stats: &mut Stats,
) {
    let column = current_column(out);
    let max_width = options.width.map(|width| width.saturating_sub(column));
    let render_options = RenderOptions {
        mode: Mode::Inline,
        theme: options.theme.clone(),
        max_width,
        align: Align::Left,
        indent: 0,
        fallback: Fallback::Source,
    };
    let rendered = render(source, &render_options);

    if options.inline_policy == InlinePolicy::Preserve || rendered.fallback {
        out.push_str(&delimited(delim, source));
        stats.fallback += 1;
    } else {
        out.push_str(&rendered.text);
        stats.rendered += 1;
    }
}

fn render_block(
    out: &mut String,
    source: &str,
    delim: MathDelim,
    options: &Options,
    segments: &[Segment],
    index: usize,
    stats: &mut Stats,
) {
    let indent = prepare_block_position(out);
    let max_width = options.width.map(|width| width.saturating_sub(indent));
    let render_options = RenderOptions {
        mode: Mode::Block,
        theme: options.theme.clone(),
        max_width,
        align: options.align,
        indent: 0,
        fallback: Fallback::Source,
    };
    let rendered = render(source, &render_options);

    if options.block_policy == BlockPolicy::Preserve || rendered.fallback {
        out.push_str(&delimited(delim, source));
        stats.fallback += 1;
    } else {
        if options.fence {
            out.push_str(&" ".repeat(indent));
            out.push_str("```text\n");
        }
        append_indented(out, &rendered.text, indent);
        if options.keep_source {
            if options.fence {
                append_source_plain(out, source);
            } else {
                append_source(out, source, indent, options.theme.color_enabled());
            }
        }
        if options.fence {
            out.push('\n');
            out.push_str(&" ".repeat(indent));
            out.push_str("```");
        }
        stats.rendered += 1;
    }

    if !out.ends_with('\n') {
        let next_starts_with_newline = matches!(
            segments.get(index + 1),
            Some(Segment::Text(text)) if text.starts_with('\n')
        );
        if !next_starts_with_newline {
            out.push('\n');
        }
    }
}

/// Move to a clean line for block art and return the indentation to apply.
fn prepare_block_position(out: &mut String) -> usize {
    let line_start = out.rfind('\n').map(|position| position + 1).unwrap_or(0);
    let prefix = &out[line_start..];
    let indent = prefix.chars().take_while(|ch| *ch == ' ').count();

    if prefix.trim().is_empty() {
        out.truncate(line_start);
    } else {
        out.push('\n');
    }
    indent
}

fn append_indented(out: &mut String, text: &str, indent: usize) {
    let prefix = " ".repeat(indent);
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if !line.is_empty() {
            out.push_str(&prefix);
        }
        out.push_str(line);
    }
}

fn append_source_plain(out: &mut String, source: &str) {
    for line in source.lines() {
        out.push_str("\n% ");
        out.push_str(line);
    }
}

fn append_source(out: &mut String, source: &str, indent: usize, color: bool) {
    out.push('\n');
    let dim = if color { "\x1b[2m" } else { "" };
    let reset = if color { "\x1b[0m" } else { "" };
    let prefix = " ".repeat(indent);
    for line in source.lines() {
        out.push_str(&format!("{prefix}{dim}% {line}{reset}\n"));
    }
}

fn delimited(delim: MathDelim, source: &str) -> String {
    match delim {
        MathDelim::Dollar => format!("${source}$"),
        MathDelim::DoubleDollar => format!("$$\n{}\n$$", source.trim()),
        MathDelim::Paren => format!("\\({source}\\)"),
        MathDelim::Bracket => format!("\\[\n{}\n\\]", source.trim()),
        MathDelim::Fence => format!("```latex\n{}\n```", source.trim()),
    }
}

fn current_column(out: &str) -> usize {
    let line_start = out.rfind('\n').map(|position| position + 1).unwrap_or(0);
    UnicodeWidthStr::width(&out[line_start..])
}
