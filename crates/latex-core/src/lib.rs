//! Parsing and AST for a practical subset of TeX math.
//!
//! This crate has no knowledge of terminals, colours or terminal cell widths.
//! It turns source text into [`ast::Node`] and reports recoverable problems as
//! [`diagnostic::Diagnostic`] values.

pub mod ast;
pub mod diagnostic;
pub mod lexer;
pub mod parser;
pub mod symbols;

pub use ast::Node;
pub use diagnostic::{Diagnostic, Severity};
pub use parser::{parse, ParseResult};
