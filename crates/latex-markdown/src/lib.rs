//! Markdown math extraction and replacement.
//!
//! The scanner deliberately avoids fenced code and inline code, and rejects
//! currency-like `$` pairs. The transform layer replaces only math spans.

pub mod scan;
pub mod transform;

pub use scan::{extract, MathDelim, Segment};
pub use transform::{transform, transform_with_stats, BlockPolicy, InlinePolicy, Options, Stats};
