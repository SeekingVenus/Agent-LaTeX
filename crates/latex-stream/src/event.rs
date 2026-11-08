//! Agent-neutral streaming events.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathDelim {
    Dollar,
    DoubleDollar,
    Paren,
    Bracket,
    Fence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// Stable prose/code text. Emit immediately.
    Text(String),
    /// A replaceable preview of an open math span.
    MathPreview {
        id: u64,
        source: String,
        display: bool,
        delim: MathDelim,
    },
    /// A math span closed normally.
    MathCommit {
        id: u64,
        source: String,
        display: bool,
        delim: MathDelim,
    },
    /// A closed math span was not a valid formula (for example currency-like
    /// `$5 and $10`). The host should emit the original delimited source instead
    /// of rendered art. Unterminated spans are replayed as [`StreamEvent::Text`]
    /// by the scanner, so the host never has to invent a closing delimiter.
    MathAbort {
        id: u64,
        source: String,
        display: bool,
        delim: MathDelim,
    },
}
