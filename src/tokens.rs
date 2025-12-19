//! Token types for the HTML tokenizer

use std::collections::HashMap;
use crate::node::Doctype;

/// Token types emitted by the tokenizer
#[derive(Debug, Clone)]
pub enum Token {
    StartTag {
        name: String,
        attrs: HashMap<String, String>,
        self_closing: bool,
    },
    EndTag {
        name: String,
    },
    Character(char),
    Comment(String),
    Doctype(Doctype),
    Eof,
}

/// Parse error with location information
#[derive(Debug, Clone)]
pub struct ParseError {
    /// Error code (kebab-case, matches html5lib-tests)
    pub code: String,
    /// Human-readable message
    pub message: String,
    /// Line number (1-based)
    pub line: Option<usize>,
    /// Column number (1-based)
    pub column: Option<usize>,
}

impl ParseError {
    pub fn new(code: &str) -> Self {
        Self {
            code: code.to_string(),
            message: code.to_string(),
            line: None,
            column: None,
        }
    }

    pub fn with_location(code: &str, line: usize, column: usize) -> Self {
        Self {
            code: code.to_string(),
            message: code.to_string(),
            line: Some(line),
            column: Some(column),
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let (Some(line), Some(column)) = (self.line, self.column) {
            write!(f, "({},{}): {}", line, column, self.code)
        } else {
            write!(f, "{}", self.code)
        }
    }
}

impl std::error::Error for ParseError {}
