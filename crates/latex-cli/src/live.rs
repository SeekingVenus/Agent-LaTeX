//! `agent-latex live`: consume a streaming text/markdown source.
//!
//! On a TTY, an open math span is shown as a replaceable preview. On a pipe,
//! previews are skipped and only committed spans are rendered.

use std::io::{IsTerminal, Read, Write};

use anyhow::{Context, Result};
use latex_core::ast::Align;
use latex_stream::{MathDelim, StreamEvent, StreamingScanner};
use latex_term::{render, Fallback, Mode, Options as RenderOptions, Theme};
use unicode_width::UnicodeWidthChar;

pub struct LiveOptions {
    pub width: Option<usize>,
    pub theme: Theme,
}

pub fn run(options: LiveOptions) -> Result<()> {
    if std::io::stdout().is_terminal() {
        let width = options.width.unwrap_or(80);
        let mut surface = LiveSurface::new(width, options.theme);
        stream_stdin(|event| surface.handle(event))?;
        surface.finish()
    } else {
        let mut writer = PlainWriter::new(options.width, options.theme);
        stream_stdin(|event| writer.handle(event))?;
        writer.finish()
    }
}

fn stream_stdin(mut on_event: impl FnMut(StreamEvent) -> Result<()>) -> Result<()> {
    let mut scanner = StreamingScanner::new();
    let mut stdin = std::io::stdin().lock();
    let mut buf = [0u8; 8192];
    let mut carry: Vec<u8> = Vec::new();

    loop {
        let read = stdin.read(&mut buf).context("failed to read stdin")?;
        if read == 0 {
            break;
        }
        carry.extend_from_slice(&buf[..read]);

        let mut consumed = 0usize;
        while consumed < carry.len() {
            match std::str::from_utf8(&carry[consumed..]) {
                Ok(text) => {
                    feed(&mut scanner, text, &mut on_event)?;
                    consumed = carry.len();
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    if valid > 0 {
                        let text = std::str::from_utf8(&carry[consumed..consumed + valid])
                            .expect("valid UTF-8 prefix");
                        feed(&mut scanner, text, &mut on_event)?;
                        consumed += valid;
                    }
                    match error.error_len() {
                        Some(len) => {
                            let replacement =
                                String::from_utf8_lossy(&carry[consumed..consumed + len])
                                    .to_string();
                            feed(&mut scanner, &replacement, &mut on_event)?;
                            consumed += len;
                        }
                        None => break,
                    }
                }
            }
        }

        if consumed > 0 {
            carry.drain(..consumed);
        }
    }

    if !carry.is_empty() {
        let tail = String::from_utf8_lossy(&carry).to_string();
        feed(&mut scanner, &tail, &mut on_event)?;
    }
    finish(&mut scanner, &mut on_event)
}

fn feed(
    scanner: &mut StreamingScanner,
    text: &str,
    on_event: &mut impl FnMut(StreamEvent) -> Result<()>,
) -> Result<()> {
    let mut error = None;
    scanner.feed(text, &mut |event| {
        if error.is_none() {
            if let Err(cause) = on_event(event) {
                error = Some(cause);
            }
        }
    });
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn finish(
    scanner: &mut StreamingScanner,
    on_event: &mut impl FnMut(StreamEvent) -> Result<()>,
) -> Result<()> {
    let mut error = None;
    scanner.finish(&mut |event| {
        if error.is_none() {
            if let Err(cause) = on_event(event) {
                error = Some(cause);
            }
        }
    });
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

struct PlainWriter {
    width: Option<usize>,
    theme: Theme,
    column: usize,
}

impl PlainWriter {
    fn new(width: Option<usize>, theme: Theme) -> Self {
        Self {
            width,
            theme,
            column: 0,
        }
    }

    fn handle(&mut self, event: StreamEvent) -> Result<()> {
        match event {
            StreamEvent::Text(text) => self.write_text(&text),
            StreamEvent::MathPreview { .. } => Ok(()),
            StreamEvent::MathCommit {
                source,
                display,
                delim,
                ..
            } => self.write_math(&source, display, delim, false),
            StreamEvent::MathAbort {
                source,
                display,
                delim,
                ..
            } => self.write_math(&source, display, delim, true),
        }
    }

    fn finish(&mut self) -> Result<()> {
        std::io::stdout().flush()?;
        Ok(())
    }

    fn write_text(&mut self, text: &str) -> Result<()> {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(text.as_bytes())?;
        self.column = column_after(self.column, text);
        stdout.flush()?;
        Ok(())
    }

    fn write_math(
        &mut self,
        source: &str,
        display: bool,
        delim: MathDelim,
        aborted: bool,
    ) -> Result<()> {
        let mut stdout = std::io::stdout().lock();

        if display {
            let options = RenderOptions {
                mode: Mode::Block,
                theme: self.theme.clone(),
                max_width: self.width,
                align: Align::Left,
                indent: 0,
                fallback: Fallback::Source,
            };
            let output = render(source, &options);
            if aborted || output.fallback {
                if self.column > 0 {
                    stdout.write_all(b"\n")?;
                }
                let (open, close) = display_delimiters(delim);
                writeln!(stdout, "{open}")?;
                stdout.write_all(source.trim().as_bytes())?;
                stdout.write_all(b"\n")?;
                writeln!(stdout, "{close}")?;
                self.column = 0;
            } else {
                if self.column > 0 {
                    stdout.write_all(b"\n")?;
                }
                stdout.write_all(output.text.as_bytes())?;
                stdout.write_all(b"\n")?;
                self.column = 0;
            }
        } else {
            let max_width = self.width.map(|width| width.saturating_sub(self.column));
            let options = RenderOptions {
                mode: Mode::Inline,
                theme: self.theme.clone(),
                max_width,
                align: Align::Left,
                indent: 0,
                fallback: Fallback::Source,
            };
            let output = render(source, &options);
            let text = if aborted || output.fallback {
                inline_delimited(delim, source)
            } else {
                output.text
            };
            stdout.write_all(text.as_bytes())?;
            self.column += display_width(&text);
        }

        stdout.flush()?;
        Ok(())
    }
}

struct Preview {
    art: String,
    display: bool,
}

struct LiveSurface {
    width: usize,
    theme: Theme,
    out: std::io::Stdout,
    current_line: String,
    current_line_width: usize,
    preview: Option<Preview>,
    preview_height: usize,
}

impl LiveSurface {
    fn new(width: usize, theme: Theme) -> Self {
        Self {
            width,
            theme,
            out: std::io::stdout(),
            current_line: String::new(),
            current_line_width: 0,
            preview: None,
            preview_height: 0,
        }
    }

    fn handle(&mut self, event: StreamEvent) -> Result<()> {
        match event {
            StreamEvent::Text(text) => {
                self.clear_preview()?;
                self.write_text(&text)?;
                self.draw_preview()?;
            }
            StreamEvent::MathPreview {
                source,
                display,
                delim,
                ..
            } => {
                let options = self.render_options(display);
                let output = render(&source, &options);
                let art = if output.fallback {
                    sanitize_math_source(&source)
                } else {
                    output.text
                };
                self.clear_preview()?;
                self.preview = Some(Preview { art, display });
                self.draw_preview()?;
                let _ = delim;
            }
            StreamEvent::MathCommit {
                source,
                display,
                delim,
                ..
            } => {
                self.clear_preview()?;
                self.write_final(&source, display, delim, false)?;
            }
            StreamEvent::MathAbort {
                source,
                display,
                delim,
                ..
            } => {
                self.clear_preview()?;
                self.write_final(&source, display, delim, true)?;
            }
        }
        self.out.flush()?;
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        self.clear_preview()?;
        self.out.flush()?;
        Ok(())
    }

    fn render_options(&self, display: bool) -> RenderOptions {
        RenderOptions {
            mode: if display { Mode::Block } else { Mode::Inline },
            theme: self.theme.clone(),
            max_width: Some(if display {
                self.width
            } else {
                self.width.saturating_sub(self.current_line_width)
            }),
            align: Align::Left,
            indent: 0,
            fallback: Fallback::Source,
        }
    }

    fn write_text(&mut self, text: &str) -> Result<()> {
        self.out.write_all(text.as_bytes())?;
        if let Some(position) = text.rfind(['\n', '\r']) {
            self.current_line.clear();
            self.current_line.push_str(&text[position + 1..]);
            self.current_line_width = display_width(&text[position + 1..]);
        } else {
            self.current_line.push_str(text);
            self.current_line_width = column_after(self.current_line_width, text);
        }
        Ok(())
    }

    fn write_final(
        &mut self,
        source: &str,
        display: bool,
        delim: MathDelim,
        aborted: bool,
    ) -> Result<()> {
        if display {
            let options = self.render_options(true);
            let output = render(source, &options);
            if aborted || output.fallback {
                self.write_display_source(delim, source)?;
            } else {
                if !self.current_line.is_empty() {
                    self.out.write_all(b"\n")?;
                    self.current_line.clear();
                    self.current_line_width = 0;
                }
                self.out.write_all(output.text.as_bytes())?;
                self.out.write_all(b"\n")?;
            }
            return Ok(());
        }

        let options = self.render_options(false);
        let output = render(source, &options);
        let text = if aborted || output.fallback {
            inline_delimited(delim, &sanitize_math_source(source))
        } else {
            output.text
        };
        self.out.write_all(text.as_bytes())?;
        self.current_line.push_str(&text);
        self.current_line_width += display_width(&text);
        Ok(())
    }

    fn write_display_source(&mut self, delim: MathDelim, source: &str) -> Result<()> {
        if !self.current_line.is_empty() {
            self.out.write_all(b"\n")?;
            self.current_line.clear();
            self.current_line_width = 0;
        }
        let (open, close) = display_delimiters(delim);
        writeln!(self.out, "{open}")?;
        self.out
            .write_all(sanitize_math_source(source.trim()).as_bytes())?;
        self.out.write_all(b"\n")?;
        writeln!(self.out, "{close}")?;
        Ok(())
    }

    fn clear_preview(&mut self) -> Result<()> {
        let Some(preview) = self.preview.take() else {
            return Ok(());
        };
        if preview.display {
            if self.preview_height > 0 {
                write!(self.out, "\x1b[{}A\x1b[J", self.preview_height)?;
                self.preview_height = 0;
            }
        } else {
            self.out.write_all(b"\r\x1b[2K")?;
            self.out.write_all(self.current_line.as_bytes())?;
        }
        Ok(())
    }

    fn draw_preview(&mut self) -> Result<()> {
        let Some(preview) = &self.preview else {
            return Ok(());
        };

        if preview.display {
            if !self.current_line.is_empty() {
                self.out.write_all(b"\n")?;
                self.current_line.clear();
                self.current_line_width = 0;
            }
            let mut height = 0usize;
            for line in preview.art.lines() {
                let line = clip_string(line, Some(self.width));
                self.out.write_all(line.as_bytes())?;
                self.out.write_all(b"\n")?;
                height += 1;
            }
            self.preview_height = height;
        } else {
            let max_width = self.width.saturating_sub(self.current_line_width);
            let art = clip_string(&preview.art, Some(max_width));
            self.out.write_all(b"\r\x1b[2K")?;
            self.out.write_all(self.current_line.as_bytes())?;
            self.out.write_all(art.as_bytes())?;
        }
        Ok(())
    }
}

fn display_delimiters(delim: MathDelim) -> (&'static str, &'static str) {
    match delim {
        MathDelim::Bracket => ("\\[", "\\]"),
        _ => ("$$", "$$"),
    }
}

fn inline_delimited(delim: MathDelim, source: &str) -> String {
    match delim {
        MathDelim::Paren => format!("\\({source}\\)"),
        _ => format!("${source}$"),
    }
}

fn column_after(column: usize, text: &str) -> usize {
    if let Some(position) = text.rfind(['\n', '\r']) {
        display_width(&text[position + 1..])
    } else {
        column + display_width(text)
    }
}

/// TeX source is shown on a TTY when it cannot be rendered. Keep line breaks
/// but never emit terminal control sequences from it.
fn sanitize_math_source(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control() || *ch == '\n')
        .collect()
}

fn display_width(text: &str) -> usize {
    let mut width = 0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\x07' {
                            break;
                        }
                        if next == '\x1b' && chars.peek().copied() == Some('\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        width += UnicodeWidthChar::width(ch).unwrap_or(0);
    }
    width
}

fn clip_string(text: &str, max_width: Option<usize>) -> String {
    let Some(max_width) = max_width else {
        return text.to_string();
    };
    let mut out = String::new();
    let mut width = 0usize;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            out.push(ch);
            if let Some('[') = chars.peek() {
                out.push(chars.next().unwrap());
                for next in chars.by_ref() {
                    out.push(next);
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + char_width > max_width {
            break;
        }
        out.push(ch);
        width += char_width;
    }
    out
}
