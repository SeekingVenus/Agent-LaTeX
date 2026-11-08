//! Incremental state machine.
//!
//! The scanner never assumes that a delimiter, command or UTF-8 code point
//! arrives in one chunk. Ambiguous tails are held in the internal buffer until
//! the next feed (or `finish`).

use crate::event::{MathDelim, StreamEvent};
use crate::scan::{is_closing_fence_line, parse_fence_line, valid_inline};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StateKind {
    Normal,
    InlineMath,
    DisplayMath,
    MathFence,
    CodeFence,
    InlineCode,
}

pub struct StreamingScanner {
    buffer: String,
    pos: usize,
    state: StateKind,
    at_line_start: bool,
    next_id: u64,
    pending_text: String,

    math_id: u64,
    /// The original opening delimiter or fence line, for replaying an
    /// unterminated span as plain text.
    math_open: String,
    math_source: String,
    math_display: bool,
    math_delim: MathDelim,
    math_dirty: bool,

    fence_marker: char,
    fence_marker_len: usize,
    inline_code_marker_len: usize,
}

impl Default for StreamingScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingScanner {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            pos: 0,
            state: StateKind::Normal,
            at_line_start: true,
            next_id: 1,
            pending_text: String::new(),
            math_id: 0,
            math_open: String::new(),
            math_source: String::new(),
            math_display: false,
            math_delim: MathDelim::Dollar,
            math_dirty: false,
            fence_marker: '`',
            fence_marker_len: 3,
            inline_code_marker_len: 1,
        }
    }

    /// Feed a chunk of text. The callback receives events in order.
    pub fn feed(&mut self, chunk: &str, sink: &mut impl FnMut(StreamEvent)) {
        self.buffer.push_str(chunk);
        self.process(false, sink);
        self.flush_text(sink);
        self.emit_preview(sink);
        self.compact();
    }

    /// Signal end of input and flush any remaining state.
    pub fn finish(&mut self, sink: &mut impl FnMut(StreamEvent)) {
        self.process(true, sink);
        self.flush_text(sink);
        if matches!(
            self.state,
            StateKind::InlineMath | StateKind::DisplayMath | StateKind::MathFence
        ) {
            // No closing delimiter arrived: replay the original bytes instead
            // of inventing one.
            self.restore_math_as_text();
        }
        self.flush_text(sink);
        self.compact();
    }

    fn process(&mut self, final_chunk: bool, sink: &mut impl FnMut(StreamEvent)) {
        loop {
            if self.pos >= self.buffer.len() {
                break;
            }
            let before_pos = self.pos;
            let before_line_start = self.at_line_start;

            match self.state {
                StateKind::Normal => {
                    if self.at_line_start {
                        self.process_line_start(final_chunk, sink);
                    } else {
                        self.process_normal_char(final_chunk, sink);
                    }
                }
                StateKind::InlineMath => self.process_inline_math(final_chunk, sink),
                StateKind::DisplayMath => self.process_display_math(final_chunk, sink),
                StateKind::MathFence => self.process_math_fence(final_chunk, sink),
                StateKind::CodeFence => self.process_code_fence(final_chunk),
                StateKind::InlineCode => self.process_inline_code(final_chunk),
            }

            if self.pos == before_pos && self.at_line_start == before_line_start {
                break;
            }
        }
    }

    fn process_line_start(&mut self, final_chunk: bool, sink: &mut impl FnMut(StreamEvent)) {
        let rest = &self.buffer[self.pos..];

        let mut first_offset = None;
        for (offset, ch) in rest.char_indices() {
            if ch == ' ' || ch == '\t' {
                continue;
            }
            first_offset = Some(offset);
            break;
        }

        let Some(offset) = first_offset else {
            if final_chunk {
                let text = rest.to_string();
                self.pending_text.push_str(&text);
                self.pos = self.buffer.len();
            }
            return;
        };

        let first = rest[offset..].chars().next().unwrap_or(' ');
        if first != '`' && first != '~' {
            self.at_line_start = false;
            return;
        }

        // A fence needs the complete opening line so we can read its info
        // string. Hold a partial line until the next feed.
        let Some(line_end) = rest.find('\n') else {
            if final_chunk {
                self.at_line_start = false;
            }
            return;
        };

        let line = &rest[..line_end];
        let advance = line_end + 1;

        if let Some((marker, marker_len, info)) = parse_fence_line(line) {
            let language = info
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            let is_math = matches!(
                language.as_str(),
                "latex" | "math" | "tex" | "tex-math" | "katex"
            );

            if is_math {
                let opening = rest[..advance].to_string();
                self.flush_text(sink);
                self.start_math(MathDelim::Fence, true, sink);
                self.math_open = opening;
                self.state = StateKind::MathFence;
                self.fence_marker = marker;
                self.fence_marker_len = marker_len;
                self.at_line_start = true;
            } else {
                self.pending_text.push_str(&rest[..advance]);
                self.state = StateKind::CodeFence;
                self.fence_marker = marker;
                self.fence_marker_len = marker_len;
                self.at_line_start = true;
            }
            self.pos += advance;
            return;
        }

        self.at_line_start = false;
    }

    fn process_normal_char(&mut self, final_chunk: bool, sink: &mut impl FnMut(StreamEvent)) {
        let rest = &self.buffer[self.pos..];
        let Some(ch) = rest.chars().next() else {
            return;
        };

        match ch {
            '\\' => {
                if rest.len() == 1 {
                    if final_chunk {
                        self.pending_text.push('\\');
                        self.pos += 1;
                    }
                    return;
                }
                let next = rest[1..].chars().next().unwrap_or('\\');
                match next {
                    '$' => {
                        self.pending_text.push_str("\\$");
                        self.pos += 2;
                    }
                    '[' => {
                        self.flush_text(sink);
                        self.start_math_without_flush(MathDelim::Bracket, true);
                        self.pos += 2;
                    }
                    '(' => {
                        self.flush_text(sink);
                        self.start_math_without_flush(MathDelim::Paren, false);
                        self.pos += 2;
                    }
                    '\\' => {
                        self.pending_text.push_str("\\\\");
                        self.pos += 2;
                    }
                    _ => {
                        self.pending_text.push('\\');
                        self.pos += 1;
                    }
                }
            }
            '$' => {
                if rest == "$" && !final_chunk {
                    return;
                }
                if rest.starts_with("$$") {
                    self.flush_text(sink);
                    self.start_math_without_flush(MathDelim::DoubleDollar, true);
                    self.pos += 2;
                } else {
                    self.flush_text(sink);
                    self.start_math_without_flush(MathDelim::Dollar, false);
                    self.pos += 1;
                }
            }
            '`' => {
                let run = rest.chars().take_while(|ch| *ch == '`').count();
                if self.pos + run >= self.buffer.len() && !final_chunk {
                    return;
                }
                self.pending_text.push_str(&"`".repeat(run));
                self.state = StateKind::InlineCode;
                self.inline_code_marker_len = run;
                self.pos += run;
            }
            '\n' => {
                self.pending_text.push('\n');
                self.pos += 1;
                self.at_line_start = true;
            }
            _ => {
                self.pending_text.push(ch);
                self.pos += ch.len_utf8();
                self.at_line_start = false;
            }
        }
    }

    fn process_inline_math(&mut self, final_chunk: bool, sink: &mut impl FnMut(StreamEvent)) {
        let rest = &self.buffer[self.pos..];
        if rest.is_empty() {
            return;
        }

        match self.math_delim {
            MathDelim::Dollar if rest.starts_with('$') => {
                if valid_inline(&self.math_source) {
                    self.commit_math(sink);
                } else {
                    self.abort_math(sink);
                }
                self.pos += 1;
                return;
            }
            MathDelim::Paren => {
                if rest.starts_with("\\)") {
                    self.commit_math(sink);
                    self.pos += 2;
                    return;
                }
                if !final_chunk && "\\)".starts_with(rest) {
                    return;
                }
            }
            _ => {}
        }

        let ch = rest.chars().next().unwrap();

        if ch == '\n' {
            // Inline math cannot cross a line. Replay the opening delimiter and
            // source exactly; the newline itself is handled below.
            self.restore_math_as_text();
            self.pending_text.push('\n');
            self.pos += 1;
            self.at_line_start = true;
            return;
        }

        if ch == '\\' {
            if rest.len() == 1 {
                if final_chunk {
                    self.math_source.push('\\');
                    self.pos += 1;
                    self.math_dirty = true;
                }
                return;
            }
            let next = rest[1..].chars().next().unwrap_or('\\');
            if self.math_delim == MathDelim::Dollar && next == '$' {
                self.math_source.push_str("\\$");
                self.pos += 2;
                self.math_dirty = true;
                return;
            }
            self.math_source.push('\\');
            self.pos += 1;
            self.math_dirty = true;
            return;
        }

        self.math_source.push(ch);
        self.pos += ch.len_utf8();
        self.math_dirty = true;
    }

    fn process_display_math(&mut self, final_chunk: bool, sink: &mut impl FnMut(StreamEvent)) {
        let rest = &self.buffer[self.pos..];
        if rest.is_empty() {
            return;
        }

        let close = match self.math_delim {
            MathDelim::DoubleDollar => "$$",
            MathDelim::Bracket => "\\]",
            _ => "",
        };

        if !close.is_empty() {
            if rest.starts_with(close) {
                self.commit_math(sink);
                self.pos += close.len();
                return;
            }
            if !final_chunk && close.starts_with(rest) {
                return;
            }
        }

        let ch = rest.chars().next().unwrap();
        self.math_source.push(ch);
        self.pos += ch.len_utf8();
        self.math_dirty = true;
    }

    fn process_math_fence(&mut self, final_chunk: bool, sink: &mut impl FnMut(StreamEvent)) {
        let rest = &self.buffer[self.pos..];

        let Some(line_end) = rest.find('\n') else {
            if final_chunk && !rest.is_empty() {
                if self.at_line_start
                    && is_closing_fence_line(rest, self.fence_marker, self.fence_marker_len)
                {
                    self.commit_math(sink);
                    self.pos = self.buffer.len();
                    self.at_line_start = true;
                } else {
                    self.math_source.push_str(rest);
                    self.pos = self.buffer.len();
                    self.math_dirty = true;
                }
            }
            return;
        };

        let line = &rest[..line_end];
        let advance = line_end + 1;

        if self.at_line_start
            && is_closing_fence_line(line, self.fence_marker, self.fence_marker_len)
        {
            self.commit_math(sink);
            self.pos += advance;
            self.at_line_start = true;
            return;
        }

        self.math_source.push_str(line);
        self.math_source.push('\n');
        self.pos += advance;
        self.at_line_start = true;
        self.math_dirty = true;
    }

    fn process_code_fence(&mut self, final_chunk: bool) {
        let rest = &self.buffer[self.pos..];

        let Some(line_end) = rest.find('\n') else {
            if final_chunk && !rest.is_empty() {
                self.pending_text.push_str(rest);
                if self.at_line_start
                    && is_closing_fence_line(rest, self.fence_marker, self.fence_marker_len)
                {
                    self.state = StateKind::Normal;
                }
                self.pos = self.buffer.len();
            }
            return;
        };

        let line = &rest[..line_end];
        let advance = line_end + 1;
        self.pending_text.push_str(&rest[..advance]);
        if self.at_line_start
            && is_closing_fence_line(line, self.fence_marker, self.fence_marker_len)
        {
            self.state = StateKind::Normal;
        }
        self.pos += advance;
        self.at_line_start = true;
    }

    fn process_inline_code(&mut self, final_chunk: bool) {
        let rest = &self.buffer[self.pos..];
        let Some(ch) = rest.chars().next() else {
            return;
        };

        if ch == '`' {
            let run = rest.chars().take_while(|ch| *ch == '`').count();
            if self.pos + run >= self.buffer.len() && !final_chunk {
                return;
            }
            self.pending_text.push_str(&"`".repeat(run));
            self.pos += run;
            if run == self.inline_code_marker_len {
                self.state = StateKind::Normal;
                self.at_line_start = false;
            }
            return;
        }

        self.pending_text.push(ch);
        self.pos += ch.len_utf8();
    }

    fn start_math(
        &mut self,
        delimiter: MathDelim,
        display: bool,
        sink: &mut impl FnMut(StreamEvent),
    ) {
        self.flush_text(sink);
        self.start_math_without_flush(delimiter, display);
    }

    fn start_math_without_flush(&mut self, delimiter: MathDelim, display: bool) {
        self.math_id = self.next_id;
        self.next_id += 1;
        self.math_source.clear();
        self.math_open = match delimiter {
            MathDelim::Dollar => "$",
            MathDelim::DoubleDollar => "$$",
            MathDelim::Bracket => "\\[",
            MathDelim::Paren => "\\(",
            MathDelim::Fence => "",
        }
        .to_string();
        self.math_display = display;
        self.math_delim = delimiter;
        self.math_dirty = false;
        self.state = if display {
            StateKind::DisplayMath
        } else {
            StateKind::InlineMath
        };
        self.at_line_start = false;
    }

    fn commit_math(&mut self, sink: &mut impl FnMut(StreamEvent)) {
        let source = self.math_source.trim().to_string();
        sink(StreamEvent::MathCommit {
            id: self.math_id,
            source,
            display: self.math_display,
            delim: self.math_delim,
        });
        self.state = StateKind::Normal;
        self.math_open.clear();
        self.math_source.clear();
        self.math_dirty = false;
        self.at_line_start = false;
    }

    fn abort_math(&mut self, sink: &mut impl FnMut(StreamEvent)) {
        sink(StreamEvent::MathAbort {
            id: self.math_id,
            source: std::mem::take(&mut self.math_source),
            display: self.math_display,
            delim: self.math_delim,
        });
        self.state = StateKind::Normal;
        self.math_open.clear();
        self.math_dirty = false;
        self.at_line_start = false;
    }

    /// Replay an unterminated span byte for byte: opening delimiter plus the
    /// buffered source, with no invented closing delimiter.
    fn restore_math_as_text(&mut self) {
        self.pending_text.push_str(&self.math_open);
        self.pending_text.push_str(&self.math_source);
        self.state = StateKind::Normal;
        self.math_open.clear();
        self.math_source.clear();
        self.math_dirty = false;
        self.at_line_start = false;
    }

    fn emit_preview(&mut self, sink: &mut impl FnMut(StreamEvent)) {
        if !self.math_dirty {
            return;
        }
        if matches!(
            self.state,
            StateKind::InlineMath | StateKind::DisplayMath | StateKind::MathFence
        ) && !self.math_source.trim().is_empty()
        {
            sink(StreamEvent::MathPreview {
                id: self.math_id,
                source: self.math_source.clone(),
                display: self.math_display,
                delim: self.math_delim,
            });
        }
        self.math_dirty = false;
    }

    fn flush_text(&mut self, sink: &mut impl FnMut(StreamEvent)) {
        if !self.pending_text.is_empty() {
            sink(StreamEvent::Text(std::mem::take(&mut self.pending_text)));
        }
    }

    fn compact(&mut self) {
        if self.pos > 0 {
            self.buffer.drain(..self.pos);
            self.pos = 0;
        }
    }
}
