//! Incremental math scanning for streaming conversation output.
//!
//! The scanner emits stable text immediately and buffers only the currently
//! open math span. Hosts that can repaint use [`StreamEvent::MathPreview`];
//! append-only hosts ignore previews and act on [`StreamEvent::MathCommit`].

pub mod event;
pub mod scan;
pub mod scanner;

pub use event::{MathDelim, StreamEvent};
pub use scan::{find_unescaped, is_closing_fence_line, is_escaped, parse_fence_line, valid_inline};
pub use scanner::StreamingScanner;
