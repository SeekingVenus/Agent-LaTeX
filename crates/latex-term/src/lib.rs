//! Terminal presentation for the math layout.
//!
//! The public entry point is [`render::render`]. It parses source, lays it out
//! and paints the resulting cell canvas with ANSI styles.

pub mod paint;
pub mod render;
pub mod theme;

pub use paint::paint;
pub use render::{render, Fallback, Mode, Options, RenderOutput};
pub use theme::{Color, RoleStyle, Theme};
