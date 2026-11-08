//! Transparent PTY layer for existing terminal TUIs.
//!
//! The child runs in a PTY. Its byte stream is parsed into a virtual screen,
//! math spans are replaced at the cell level, and the resulting frame is drawn
//! to the real terminal. The child's input, session and internal buffer are
//! never modified.

pub mod compositor;
pub mod hotkey;
pub mod math;
pub mod wrap;

pub use wrap::{run, WrapOptions};
