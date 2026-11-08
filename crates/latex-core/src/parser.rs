//! Recursive-descent parser for the supported TeX math subset.
//!
//! The parser is intentionally forgiving: malformed input becomes `Node::Error`
//! with a diagnostic instead of aborting the whole formula. That matters for
//! streaming, where a formula is routinely incomplete for a few frames.

use crate::ast::{
    class_of_char, AccentKind, Align, AtomClass, Delimiter, Face, FractionKind, LimitMode, Node,
    PhantomKind, Space,
};
use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::{lex, Token, TokenKind};
use crate::symbols;

/// Bound recursive descent so hostile nesting cannot overflow the stack.
const MAX_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub struct ParseResult {
    pub ast: Node,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    RBrace,
    Amp,
    Newline,
    Right,
    End,
    Eof,
    RBracket,
}

pub fn parse(input: &str) -> ParseResult {
    let mut parser = Parser::new(lex(input));
    let nodes = parser.parse_list(&[Stop::Eof]);
    ParseResult {
        ast: Node::Row(nodes),
        diagnostics: parser.diagnostics,
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
    depth: usize,
    depth_error_reported: bool,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            diagnostics: Vec::new(),
            depth: 0,
            depth_error_reported: false,
        }
    }

    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.pos)
            .map(|token| &token.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn peek_span(&self) -> Option<(usize, usize)> {
        self.tokens
            .get(self.pos)
            .map(|token| (token.span.start, token.span.end))
    }

    fn bump(&mut self) -> TokenKind {
        let kind = self.peek().clone();
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        kind
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), TokenKind::Space) {
            self.pos += 1;
        }
    }

    fn at_stop(&self, stops: &[Stop]) -> bool {
        for stop in stops {
            let matches = match stop {
                Stop::RBrace => matches!(self.peek(), TokenKind::RBrace),
                Stop::Amp => matches!(self.peek(), TokenKind::Amp),
                Stop::Newline => matches!(self.peek(), TokenKind::Newline),
                Stop::Right => matches!(self.peek(), TokenKind::Command(name) if name == "right"),
                Stop::End => matches!(self.peek(), TokenKind::Command(name) if name == "end"),
                Stop::Eof => matches!(self.peek(), TokenKind::Eof),
                Stop::RBracket => matches!(self.peek(), TokenKind::Char(']')),
            };
            if matches {
                return true;
            }
        }
        false
    }

    fn diag(&mut self, severity: Severity, message: impl Into<String>) {
        let span = self.peek_span();
        self.diagnostics.push(Diagnostic {
            severity,
            message: message.into(),
            span,
        });
    }

    fn parse_list(&mut self, stops: &[Stop]) -> Vec<Node> {
        let mut nodes = Vec::new();
        loop {
            self.skip_spaces();
            if self.at_stop(stops) {
                break;
            }
            if matches!(self.peek(), TokenKind::Eof) {
                break;
            }
            let before = self.pos;
            if let Some(node) = self.parse_scripted() {
                nodes.push(node);
            }
            if self.pos == before {
                // Guarantee progress on malformed input.
                self.bump();
            }
        }
        nodes
    }

    fn parse_scripted(&mut self) -> Option<Node> {
        let mut base = self.parse_atom()?;
        let mut sup: Option<Node> = None;
        let mut sub: Option<Node> = None;
        let mut limit_mode: Option<LimitMode> = None;

        loop {
            self.skip_spaces();
            match self.peek() {
                TokenKind::Sup => {
                    self.bump();
                    let arg = self.parse_script_arg("^");
                    sup = Some(merge_script(sup, arg));
                }
                TokenKind::Sub => {
                    self.bump();
                    let arg = self.parse_script_arg("_");
                    sub = Some(merge_script(sub, arg));
                }
                TokenKind::Char('\'') => {
                    self.bump();
                    let prime = Node::Symbol {
                        ch: '′',
                        class: AtomClass::Ord,
                    };
                    sup = Some(merge_script(sup, prime));
                }
                TokenKind::Command(name) if name == "limits" => {
                    self.bump();
                    limit_mode = Some(LimitMode::Limits);
                }
                TokenKind::Command(name) if name == "nolimits" => {
                    self.bump();
                    limit_mode = Some(LimitMode::NoLimits);
                }
                _ => break,
            }
        }

        if let Node::BigOperator { limits, .. } = &mut base {
            if let Some(mode) = limit_mode {
                *limits = mode;
            }
        }

        if sup.is_some() || sub.is_some() {
            Some(Node::Script {
                base: Box::new(base),
                sup: sup.map(Box::new),
                sub: sub.map(Box::new),
            })
        } else {
            Some(base)
        }
    }

    fn parse_script_arg(&mut self, marker: &str) -> Node {
        self.skip_spaces();
        match self.peek() {
            TokenKind::LBrace => {
                self.bump();
                let nodes = self.parse_list(&[Stop::RBrace]);
                if matches!(self.peek(), TokenKind::RBrace) {
                    self.bump();
                } else {
                    self.diag(
                        Severity::Error,
                        format!("missing closing brace for '{marker}'"),
                    );
                }
                Node::Row(nodes)
            }
            TokenKind::Eof => {
                self.diag(Severity::Error, format!("missing argument for '{marker}'"));
                Node::Error("□".into())
            }
            _ => self.parse_atom().unwrap_or_else(|| Node::Error("□".into())),
        }
    }

    fn parse_atom(&mut self) -> Option<Node> {
        if self.depth >= MAX_DEPTH {
            if !self.depth_error_reported {
                self.diag(Severity::Error, "nesting is too deep");
                self.depth_error_reported = true;
            }
            // Consume one token so every caller still makes progress instead of
            // recursing or looping on the same input.
            self.bump();
            return Some(Node::Error("…".into()));
        }
        self.depth += 1;
        let result = self.parse_atom_inner();
        self.depth -= 1;
        result
    }

    fn parse_atom_inner(&mut self) -> Option<Node> {
        self.skip_spaces();
        match self.peek().clone() {
            TokenKind::Char(ch) => {
                self.bump();
                Some(self.parse_char_atom(ch))
            }
            TokenKind::LBrace => {
                self.bump();
                let nodes = self.parse_list(&[Stop::RBrace]);
                if matches!(self.peek(), TokenKind::RBrace) {
                    self.bump();
                } else {
                    self.diag(Severity::Error, "missing closing brace");
                }
                Some(Node::Row(nodes))
            }
            TokenKind::Command(name) => {
                self.bump();
                self.parse_command(&name)
            }
            TokenKind::Newline => {
                // Outside an environment, treat `\\` as spacing.
                self.bump();
                Some(Node::Space(Space::Normal))
            }
            TokenKind::Sup => {
                self.bump();
                self.diag(Severity::Warning, "'^' without a base");
                Some(Node::Error("^".into()))
            }
            TokenKind::Sub => {
                self.bump();
                self.diag(Severity::Warning, "'_' without a base");
                Some(Node::Error("_".into()))
            }
            TokenKind::RBrace | TokenKind::Amp | TokenKind::Eof | TokenKind::Space => None,
        }
    }

    fn parse_char_atom(&mut self, ch: char) -> Node {
        if ch.is_alphabetic() {
            let mut text = String::new();
            text.push(ch);
            while let TokenKind::Char(next) = self.peek().clone() {
                if next.is_alphabetic() {
                    text.push(next);
                    self.bump();
                } else {
                    break;
                }
            }
            return Node::Ident(text);
        }

        if ch.is_ascii_digit() || ch == '.' {
            let mut text = String::new();
            text.push(ch);
            let mut seen_dot = ch == '.';
            loop {
                match self.peek().clone() {
                    TokenKind::Char(next) if next.is_ascii_digit() => {
                        text.push(next);
                        self.bump();
                    }
                    TokenKind::Char('.') if !seen_dot => {
                        // Only absorb a decimal point when a digit follows.
                        let followed_by_digit = self
                            .tokens
                            .get(self.pos + 1)
                            .map(|token| matches!(token.kind, TokenKind::Char(c) if c.is_ascii_digit()))
                            .unwrap_or(false);
                        if followed_by_digit {
                            text.push('.');
                            seen_dot = true;
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            if text.len() == 1 && text == "." {
                return Node::Symbol {
                    ch: '.',
                    class: AtomClass::Ord,
                };
            }
            return Node::Number(text);
        }

        let class = class_of_char(ch);
        Node::Symbol { ch, class }
    }

    fn parse_command(&mut self, raw_name: &str) -> Option<Node> {
        let name = raw_name.trim_end_matches('*');

        match name {
            "frac" | "dfrac" | "tfrac" => {
                let num = self.parse_arg();
                let den = self.parse_arg();
                let kind = match name {
                    "dfrac" => FractionKind::Display,
                    "tfrac" => FractionKind::Text,
                    _ => FractionKind::Normal,
                };
                return Some(Node::Fraction {
                    num: Box::new(num),
                    den: Box::new(den),
                    kind,
                });
            }
            "binom" | "dbinom" | "tbinom" => {
                let num = self.parse_arg();
                let den = self.parse_arg();
                return Some(Node::Fence {
                    open: Delimiter::Paren,
                    close: Delimiter::Paren,
                    body: Box::new(Node::Matrix {
                        open: Delimiter::None,
                        close: Delimiter::None,
                        rows: vec![vec![num], vec![den]],
                        align: vec![Align::Center],
                    }),
                });
            }
            "sqrt" => {
                let index = self.parse_optional_index();
                let body = self.parse_arg();
                return Some(Node::Radical {
                    index: index.map(Box::new),
                    body: Box::new(body),
                });
            }
            "left" => return Some(self.parse_left()),
            "right" => {
                self.diag(Severity::Error, "unexpected \\right");
                return Some(Node::Error("\\right".into()));
            }
            "begin" => return Some(self.parse_environment()),
            "end" => {
                self.diag(Severity::Error, "unexpected \\end");
                return Some(Node::Error("\\end".into()));
            }
            "not" => return Some(self.parse_not()),
            "phantom" | "hphantom" | "vphantom" => {
                let kind = match name {
                    "hphantom" => PhantomKind::Horizontal,
                    "vphantom" => PhantomKind::Vertical,
                    _ => PhantomKind::Full,
                };
                return Some(Node::Phantom {
                    body: Box::new(self.parse_arg()),
                    kind,
                });
            }
            "operatorname" => {
                let node = self.parse_text_group(Face::Roman);
                let text = match node {
                    Node::Text { text, .. } => text,
                    _ => String::new(),
                };
                return Some(Node::OperatorName(text.trim().to_string()));
            }
            "pmod" => {
                let arg = self.parse_arg();
                return Some(Node::Row(vec![
                    Node::Space(Space::Medium),
                    Node::Text {
                        text: "(mod ".into(),
                        face: Face::Roman,
                    },
                    arg,
                    Node::Text {
                        text: ")".into(),
                        face: Face::Roman,
                    },
                ]));
            }
            "overset" | "stackrel" => {
                let over = self.parse_arg();
                let body = self.parse_arg();
                return Some(Node::OverUnder {
                    body: Box::new(body),
                    over: Some(Box::new(over)),
                    under: None,
                });
            }
            "underset" => {
                let under = self.parse_arg();
                let body = self.parse_arg();
                return Some(Node::OverUnder {
                    body: Box::new(body),
                    over: None,
                    under: Some(Box::new(under)),
                });
            }
            "overbrace" => {
                let body = self.parse_arg();
                return Some(Node::OverUnder {
                    body: Box::new(body),
                    over: Some(Box::new(Node::Text {
                        text: "⏞".into(),
                        face: Face::Roman,
                    })),
                    under: None,
                });
            }
            "underbrace" => {
                let body = self.parse_arg();
                return Some(Node::OverUnder {
                    body: Box::new(body),
                    over: None,
                    under: Some(Box::new(Node::Text {
                        text: "⏟".into(),
                        face: Face::Roman,
                    })),
                });
            }
            "color" => {
                let _ = self.parse_raw_group();
                return Some(Node::Row(Vec::new()));
            }
            "textcolor" => {
                let _ = self.parse_raw_group();
                return Some(self.parse_arg());
            }
            "boxed" | "cancel" | "bcancel" | "xcancel" => return Some(self.parse_arg()),
            "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle" => {
                return Some(Node::Row(Vec::new()));
            }
            "limits" | "nolimits" => return Some(Node::Row(Vec::new())),
            "hspace" | "hskip" | "mkern" | "mskip" => {
                let raw = self.parse_raw_group();
                let width = if raw.contains("2em") {
                    Space::DoubleQuad
                } else if raw.contains("1em") {
                    Space::Quad
                } else {
                    Space::Normal
                };
                return Some(Node::Space(width));
            }
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl"
            | "biggr" | "Biggl" | "Biggr" => {
                self.skip_spaces();
                let closing = match self.peek() {
                    TokenKind::Char(')' | ']' | '>') | TokenKind::RBrace => true,
                    TokenKind::Command(command) => matches!(
                        command.as_str(),
                        "}" | "rbrace" | "rangle" | "rceil" | "rfloor" | "rvert" | "rVert"
                    ),
                    _ => false,
                };
                let open = name.ends_with('l') || (!name.ends_with('r') && !closing);
                let delimiter = self.parse_delimiter(open);
                let ch = symbols::delimiter_char(delimiter, open);
                let class = if name.ends_with('l') {
                    AtomClass::Open
                } else if name.ends_with('r') {
                    AtomClass::Close
                } else {
                    AtomClass::Ord
                };
                return Some(Node::Symbol { ch, class });
            }
            "middle" => {
                let delimiter = self.parse_delimiter(true);
                return Some(Node::Symbol {
                    ch: symbols::delimiter_char(delimiter, true),
                    class: AtomClass::Ord,
                });
            }
            _ => {}
        }

        if let Some(arrow) = relation_arrow(name) {
            let under = self.parse_optional_index();
            let over = self.parse_arg();
            return Some(Node::OverUnder {
                body: Box::new(Node::Symbol {
                    ch: arrow,
                    class: AtomClass::Rel,
                }),
                over: Some(Box::new(over)),
                under: under.map(Box::new),
            });
        }

        // Text-mode commands.
        if let Some(face) = symbols::text_face(name) {
            let is_text = matches!(
                name,
                "text"
                    | "textrm"
                    | "textnormal"
                    | "mbox"
                    | "textbf"
                    | "textit"
                    | "textsf"
                    | "texttt"
            );
            if is_text {
                return Some(self.parse_text_group(face));
            }
            let body = self.parse_arg();
            return Some(Node::Styled {
                face,
                body: Box::new(body),
            });
        }

        if let Some(kind) = symbols::accent(name) {
            let body = self.parse_arg();
            return Some(Node::Accent {
                kind,
                body: Box::new(body),
            });
        }

        if let Some(spec) = symbols::symbol(name) {
            return Some(Node::Symbol {
                ch: spec.ch,
                class: spec.class,
            });
        }

        if let Some(spec) = symbols::big_operator(name) {
            return Some(Node::BigOperator {
                ch: spec.ch,
                name: name.to_string(),
                class: spec.class,
                limits: LimitMode::Auto,
                display_limits: spec.display_limits,
            });
        }

        if let Some(function) = symbols::function(name) {
            return Some(Node::OperatorName(function.to_string()));
        }

        if let Some(space) = symbols::space(name) {
            return Some(Node::Space(space));
        }

        self.diag(Severity::Warning, format!("unsupported command \\{name}"));
        Some(Node::Error(format!("\\{name}")))
    }

    fn parse_not(&mut self) -> Node {
        self.skip_spaces();
        if let TokenKind::Command(name) = self.peek().clone() {
            if let Some(ch) = symbols::negated(&name) {
                self.bump();
                return Node::Symbol {
                    ch,
                    class: AtomClass::Rel,
                };
            }
        }
        if matches!(self.peek(), TokenKind::Char('=')) {
            self.bump();
            return Node::Symbol {
                ch: '≠',
                class: AtomClass::Rel,
            };
        }
        match self.parse_atom() {
            Some(node) => Node::Row(vec![
                Node::Symbol {
                    ch: '¬',
                    class: AtomClass::Ord,
                },
                node,
            ]),
            None => Node::Symbol {
                ch: '¬',
                class: AtomClass::Ord,
            },
        }
    }

    fn parse_arg(&mut self) -> Node {
        self.skip_spaces();
        match self.peek() {
            TokenKind::LBrace => {
                self.bump();
                let nodes = self.parse_list(&[Stop::RBrace]);
                if matches!(self.peek(), TokenKind::RBrace) {
                    self.bump();
                } else {
                    self.diag(Severity::Error, "missing closing brace");
                }
                Node::Row(nodes)
            }
            TokenKind::Eof => {
                self.diag(Severity::Error, "missing argument");
                Node::Error("□".into())
            }
            _ => self.parse_atom().unwrap_or(Node::Error("□".into())),
        }
    }

    fn parse_optional_index(&mut self) -> Option<Node> {
        self.skip_spaces();
        if matches!(self.peek(), TokenKind::Char('[')) {
            self.bump();
            let nodes = self.parse_list(&[Stop::RBracket]);
            if matches!(self.peek(), TokenKind::Char(']')) {
                self.bump();
            } else {
                self.diag(Severity::Error, "missing closing bracket in root index");
            }
            Some(Node::Row(nodes))
        } else {
            None
        }
    }

    fn parse_delimiter(&mut self, open: bool) -> Delimiter {
        self.skip_spaces();
        match self.bump() {
            TokenKind::Char(ch) => match ch {
                '(' | ')' => Delimiter::Paren,
                '[' | ']' => Delimiter::Bracket,
                '{' | '}' => Delimiter::Brace,
                '|' => Delimiter::Bar,
                '.' => Delimiter::None,
                '/' => Delimiter::Slash,
                '<' | '>' => Delimiter::Angle,
                other => Delimiter::Custom(other),
            },
            TokenKind::Command(name) => symbols::delimiter(&name).unwrap_or_else(|| {
                let ch = symbols::symbol(&name)
                    .map(|spec| spec.ch)
                    .unwrap_or(if open { '(' } else { ')' });
                Delimiter::Custom(ch)
            }),
            TokenKind::LBrace | TokenKind::RBrace => Delimiter::Brace,
            _ => Delimiter::None,
        }
    }

    fn parse_left(&mut self) -> Node {
        let open = self.parse_delimiter(true);
        let nodes = self.parse_list(&[Stop::Right]);
        if matches!(self.peek(), TokenKind::Command(name) if name == "right") {
            self.bump();
        } else {
            self.diag(Severity::Error, "missing \\right");
        }
        let close = self.parse_delimiter(false);
        Node::Fence {
            open,
            close,
            body: Box::new(Node::Row(nodes)),
        }
    }

    fn parse_text_group(&mut self, face: Face) -> Node {
        self.skip_spaces();
        if !matches!(self.peek(), TokenKind::LBrace) {
            self.diag(Severity::Error, "text command requires braces");
            self.bump();
            return Node::Text {
                text: String::new(),
                face,
            };
        }
        self.bump();
        let mut depth = 1usize;
        let mut text = String::new();
        while depth > 0 {
            match self.bump() {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                TokenKind::Char(ch) => text.push(ch),
                TokenKind::Space => {
                    if !text.ends_with(' ') {
                        text.push(' ');
                    }
                }
                TokenKind::Newline => {
                    if !text.ends_with(' ') {
                        text.push(' ');
                    }
                }
                TokenKind::Sup => text.push('^'),
                TokenKind::Sub => text.push('_'),
                TokenKind::Amp => text.push('&'),
                TokenKind::Command(name) => {
                    if name == "\\" {
                        if !text.ends_with(' ') {
                            text.push(' ');
                        }
                    } else if let Some(symbol) = symbols::text_symbol(&name) {
                        text.push_str(symbol);
                    } else if let Some(spec) = symbols::symbol(&name) {
                        text.push(spec.ch);
                    } else {
                        text.push_str(&format!("\\{name}"));
                    }
                }
                TokenKind::Eof => break,
            }
        }
        Node::Text { text, face }
    }

    fn parse_raw_group(&mut self) -> String {
        self.skip_spaces();
        if !matches!(self.peek(), TokenKind::LBrace) {
            self.diag(Severity::Error, "missing group");
            return String::new();
        }
        self.bump();
        let mut depth = 1usize;
        let mut out = String::new();
        while depth > 0 {
            match self.bump() {
                TokenKind::LBrace => {
                    depth += 1;
                    if depth > 1 {
                        out.push('{');
                    }
                }
                TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push('}');
                }
                TokenKind::Char(ch) => out.push(ch),
                TokenKind::Command(name) => out.push_str(&name),
                TokenKind::Space => out.push(' '),
                TokenKind::Newline => out.push(' '),
                TokenKind::Sup => out.push('^'),
                TokenKind::Sub => out.push('_'),
                TokenKind::Amp => out.push('&'),
                TokenKind::Eof => break,
            }
        }
        out.trim().to_string()
    }

    fn parse_environment(&mut self) -> Node {
        let env = self.parse_raw_group();
        let normalized = env.trim_end_matches('*');

        // `array` takes a column specification.
        let mut align = Vec::new();
        if matches!(normalized, "array" | "alignedat" | "alignat")
            && matches!(self.peek(), TokenKind::LBrace)
        {
            let spec = self.parse_raw_group();
            align = spec
                .chars()
                .filter_map(|ch| match ch {
                    'l' => Some(Align::Left),
                    'c' => Some(Align::Center),
                    'r' => Some(Align::Right),
                    _ => None,
                })
                .collect();
        }

        let (open, close, mut default_align) = match normalized {
            "matrix" | "smallmatrix" => (Delimiter::None, Delimiter::None, vec![Align::Center]),
            "pmatrix" => (Delimiter::Paren, Delimiter::Paren, vec![Align::Center]),
            "bmatrix" => (Delimiter::Bracket, Delimiter::Bracket, vec![Align::Center]),
            "Bmatrix" => (Delimiter::Brace, Delimiter::Brace, vec![Align::Center]),
            "vmatrix" => (Delimiter::Bar, Delimiter::Bar, vec![Align::Center]),
            "Vmatrix" => (
                Delimiter::DoubleBar,
                Delimiter::DoubleBar,
                vec![Align::Center],
            ),
            "cases" => (
                Delimiter::Brace,
                Delimiter::None,
                vec![Align::Left, Align::Left],
            ),
            "aligned" | "align" | "split" | "flalign" | "eqnarray" => (
                Delimiter::None,
                Delimiter::None,
                vec![Align::Right, Align::Left],
            ),
            "gathered" | "gather" | "multline" => {
                (Delimiter::None, Delimiter::None, vec![Align::Center])
            }
            "equation" | "displaymath" | "math" => {
                // Single-row environments: parse the body and return it directly.
                let nodes = self.parse_list(&[Stop::End]);
                self.consume_end(&env);
                return Node::Row(nodes);
            }
            _ => {
                self.diag(Severity::Warning, format!("unknown environment {env}"));
                (Delimiter::None, Delimiter::None, vec![Align::Center])
            }
        };

        if !align.is_empty() {
            default_align = align;
        }

        let mut rows: Vec<Vec<Node>> = Vec::new();
        let mut current: Vec<Node> = Vec::new();
        loop {
            let nodes = self.parse_list(&[Stop::Amp, Stop::Newline, Stop::End]);
            current.push(Node::Row(nodes));

            match self.peek().clone() {
                TokenKind::Amp => {
                    self.bump();
                }
                TokenKind::Newline => {
                    self.bump();
                    rows.push(std::mem::take(&mut current));
                }
                TokenKind::Command(name) if name == "end" => {
                    self.bump();
                    let end_name = self.parse_raw_group();
                    if end_name != env {
                        self.diag(
                            Severity::Error,
                            format!("expected \\end{{{env}}}, found \\end{{{end_name}}}"),
                        );
                    }
                    rows.push(std::mem::take(&mut current));
                    break;
                }
                TokenKind::Eof => {
                    self.diag(Severity::Error, format!("missing \\end{{{env}}}"));
                    rows.push(std::mem::take(&mut current));
                    break;
                }
                _ => {
                    rows.push(std::mem::take(&mut current));
                    break;
                }
            }
        }

        if rows.len() > 1
            && rows
                .last()
                .map(|row| row.iter().all(Node::is_empty))
                .unwrap_or(false)
        {
            rows.pop();
        }

        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns > 0 {
            while default_align.len() < columns {
                default_align.push(Align::Center);
            }
            default_align.truncate(columns);
        }

        Node::Matrix {
            open,
            close,
            rows,
            align: default_align,
        }
    }

    fn consume_end(&mut self, env: &str) {
        if matches!(self.peek(), TokenKind::Command(name) if name == "end") {
            self.bump();
            let end_name = self.parse_raw_group();
            if end_name != env {
                self.diag(
                    Severity::Error,
                    format!("expected \\end{{{env}}}, found \\end{{{end_name}}}"),
                );
            }
        } else {
            self.diag(Severity::Error, format!("missing \\end{{{env}}}"));
        }
    }
}

fn relation_arrow(name: &str) -> Option<char> {
    Some(match name {
        "xrightarrow" => '→',
        "xleftarrow" => '←',
        "xleftrightarrow" => '↔',
        "xRightarrow" => '⇒',
        "xLeftarrow" => '⇐',
        "xLeftrightarrow" => '⇔',
        "xmapsto" => '↦',
        "xhookrightarrow" => '↪',
        "xhookleftarrow" => '↩',
        _ => return None,
    })
}

fn merge_script(existing: Option<Node>, next: Node) -> Node {
    match existing {
        Some(previous) => Node::Row(vec![previous, next]),
        None => next,
    }
}

#[allow(dead_code)]
fn _accent_name(kind: AccentKind) -> &'static str {
    match kind {
        AccentKind::Hat => "hat",
        AccentKind::Bar | AccentKind::Overline => "bar",
        AccentKind::Vec => "vec",
        AccentKind::Dot => "dot",
        AccentKind::Ddot => "ddot",
        AccentKind::Tilde => "tilde",
        AccentKind::Check => "check",
        AccentKind::Breve => "breve",
        AccentKind::Acute => "acute",
        AccentKind::Grave => "grave",
        AccentKind::Underline => "underline",
    }
}
