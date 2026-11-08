//! The math AST produced by the parser.
//!
//! The tree is deliberately presentation-neutral: it describes TeX-level
//! structure (fractions, scripts, delimiters, matrices, ...) but says nothing
//! about terminal cells. Layout lives in `latex-layout`.

/// TeX atom classes, used for inter-atom spacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtomClass {
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    Inner,
}

/// A font face requested by commands such as `\mathbf` or `\mathbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    Roman,
    Italic,
    Bold,
    SansSerif,
    Monospace,
    Blackboard,
    Calligraphic,
    Fraktur,
}

/// Explicit spacing commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Space {
    Thin,
    Medium,
    Thick,
    Quad,
    DoubleQuad,
    NegativeThin,
    Normal,
}

/// Fraction variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FractionKind {
    Normal,
    Display,
    Text,
}

/// A delimiter, independent of how it is scaled during layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Delimiter {
    None,
    Paren,
    Bracket,
    Brace,
    Angle,
    Bar,
    DoubleBar,
    Floor,
    Ceil,
    Slash,
    Backslash,
    Custom(char),
}

/// Column alignment inside matrix-like environments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Accents and lines drawn over/under a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccentKind {
    Hat,
    Bar,
    Vec,
    Dot,
    Ddot,
    Tilde,
    Check,
    Breve,
    Acute,
    Grave,
    Overline,
    Underline,
}

/// Which dimensions a phantom occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhantomKind {
    /// Width and height (`\phantom`).
    Full,
    /// Width only (`\hphantom`).
    Horizontal,
    /// Height only (`\vphantom`).
    Vertical,
}

/// Where limits on a big operator go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitMode {
    /// Use the default for the operator and the current style.
    Auto,
    /// Force limits above/below.
    Limits,
    /// Force limits to the side.
    NoLimits,
}

/// The core math expression tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A horizontal sequence. An empty row renders as nothing.
    Row(Vec<Node>),

    /// A resolved symbol and its atom class.
    Symbol {
        ch: char,
        class: AtomClass,
    },

    /// A run of letters, rendered as a variable.
    Ident(String),

    /// A run of digits (and a decimal point), rendered as a number.
    Number(String),

    /// Text mode content, e.g. `\text{if } x > 0`.
    Text {
        text: String,
        face: Face,
    },

    /// Explicit horizontal space.
    Space(Space),

    Fraction {
        num: Box<Node>,
        den: Box<Node>,
        kind: FractionKind,
    },

    Radical {
        index: Option<Box<Node>>,
        body: Box<Node>,
    },

    Script {
        base: Box<Node>,
        sup: Option<Box<Node>>,
        sub: Option<Box<Node>>,
    },

    Fence {
        open: Delimiter,
        close: Delimiter,
        body: Box<Node>,
    },

    Matrix {
        open: Delimiter,
        close: Delimiter,
        rows: Vec<Vec<Node>>,
        align: Vec<Align>,
    },

    Accent {
        kind: AccentKind,
        body: Box<Node>,
    },

    OverUnder {
        body: Box<Node>,
        over: Option<Box<Node>>,
        under: Option<Box<Node>>,
    },

    BigOperator {
        ch: char,
        name: String,
        class: AtomClass,
        limits: LimitMode,
        /// Whether `Auto` puts limits above/below in display style.
        display_limits: bool,
    },

    /// A function name such as `\sin` or `\operatorname{rank}`.
    OperatorName(String),

    /// Apply a face to a subtree.
    Styled {
        face: Face,
        body: Box<Node>,
    },

    /// Occupies space but renders nothing.
    Phantom {
        body: Box<Node>,
        kind: PhantomKind,
    },

    /// Unparsed or unsupported input, kept so the renderer can degrade
    /// gracefully instead of dropping content.
    Error(String),
}

impl Node {
    /// The TeX atom class used for spacing decisions.
    pub fn class(&self) -> AtomClass {
        match self {
            Node::Symbol { class, .. } => *class,
            Node::Ident(_) | Node::Number(_) | Node::Text { .. } => AtomClass::Ord,
            Node::BigOperator { class, .. } => *class,
            Node::OperatorName(_) => AtomClass::Op,
            Node::Row(nodes) => nodes
                .iter()
                .rev()
                .find(|node| !matches!(node, Node::Space(_)))
                .map(Node::class)
                .unwrap_or(AtomClass::Ord),
            Node::Fraction { .. }
            | Node::Radical { .. }
            | Node::Matrix { .. }
            | Node::Fence { .. } => AtomClass::Inner,
            Node::Script { base, .. } => base.class(),
            Node::Accent { body, .. }
            | Node::Styled { body, .. }
            | Node::Phantom { body, .. }
            | Node::OverUnder { body, .. } => body.class(),
            Node::Space(_) | Node::Error(_) => AtomClass::Ord,
        }
    }

    /// True when the node produces no visible content.
    pub fn is_empty(&self) -> bool {
        matches!(self, Node::Row(nodes) if nodes.is_empty())
    }
}

/// Map a character to its default atom class, for characters typed directly
/// rather than through a command.
pub fn class_of_char(ch: char) -> AtomClass {
    match ch {
        '(' | '[' | '{' | '⟨' => AtomClass::Open,
        ')' | ']' | '}' | '⟩' => AtomClass::Close,
        '+' | '-' | '*' | '/' | '±' | '∓' | '×' | '÷' | '·' => AtomClass::Bin,
        '=' | '<' | '>' | '≤' | '≥' | '≠' | '≈' | '≡' | '∼' | '∝' => AtomClass::Rel,
        ',' | ';' => AtomClass::Punct,
        _ => AtomClass::Ord,
    }
}
