//! Layout of the math AST into terminal cells.
//!
//! [`layout::layout`] produces a multi-line [`canvas::Canvas`]; [`inline::render_inline`]
//! produces the compact single-line form used inside prose.

pub mod canvas;
pub mod inline;
pub mod layout;

pub use canvas::{Canvas, Cell, CellKind, Role};
pub use inline::{render_inline, render_inline_compact};
pub use layout::{layout, LayoutOptions};
