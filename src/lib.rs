//! JustHTML - A pure Rust HTML5 parser following the WHATWG specification.
//!
//! This is a straightforward implementation without specialized optimizations,
//! designed to match the architecture of swift-justhtml.

mod constants;
mod entities;
mod node;
pub mod serialize;
mod tokens;
mod tokenizer;
mod tree_builder;

pub use node::{Doctype, Namespace, Node, NodeData};
pub use tokens::{ParseError, Token};

use tree_builder::TreeBuilder;
use tokenizer::{Tokenizer, State as TokenizerState};

/// Fragment parsing context
#[derive(Debug, Clone)]
pub struct FragmentContext {
    pub tag_name: String,
    pub namespace: Option<Namespace>,
}

impl FragmentContext {
    pub fn new(tag_name: &str) -> Self {
        Self {
            tag_name: tag_name.to_lowercase(),
            namespace: None,
        }
    }

    pub fn with_namespace(tag_name: &str, namespace: Namespace) -> Self {
        Self {
            tag_name: tag_name.to_lowercase(),
            namespace: Some(namespace),
        }
    }
}

/// Parse options
#[derive(Debug, Clone, Default)]
pub struct ParseOptions {
    /// Parse as fragment in specific context
    pub fragment_context: Option<FragmentContext>,
    /// Enable JavaScript scripting mode
    pub scripting: bool,
    /// Parse iframe srcdoc content
    pub iframe_srcdoc: bool,
}

/// Main entry point for HTML parsing
pub struct JustHTML {
    /// The parsed document root
    pub root: Node,
    /// Collection of parse errors
    pub errors: Vec<ParseError>,
}

impl JustHTML {
    /// Parse HTML from a string
    pub fn parse(html: &str) -> Self {
        Self::parse_with_options(html, ParseOptions::default())
    }

    /// Parse HTML with options
    pub fn parse_with_options(html: &str, options: ParseOptions) -> Self {
        let mut tree_builder = TreeBuilder::new(
            options.fragment_context.as_ref(),
            options.scripting,
            options.iframe_srcdoc,
        );

        // Determine initial tokenizer state based on fragment context (per WHATWG spec)
        let initial_state = if let Some(ref ctx) = options.fragment_context {
            match ctx.tag_name.as_str() {
                "title" | "textarea" => TokenizerState::Rcdata,
                "style" | "xmp" | "iframe" | "noembed" | "noframes" => TokenizerState::Rawtext,
                "script" => TokenizerState::ScriptData,
                "noscript" if options.scripting => TokenizerState::Rawtext,
                "plaintext" => TokenizerState::Plaintext,
                _ => TokenizerState::Data,
            }
        } else {
            TokenizerState::Data
        };

        let mut tokenizer = Tokenizer::with_options(&mut tree_builder, initial_state, options.scripting);
        tokenizer.run(html);

        let (root, errors) = tree_builder.finish();

        Self { root, errors }
    }

    /// Parse a fragment
    pub fn parse_fragment(html: &str, context: FragmentContext) -> Self {
        Self::parse_with_options(html, ParseOptions {
            fragment_context: Some(context),
            ..Default::default()
        })
    }

    /// Serialize to HTML
    pub fn to_html(&self) -> String {
        serialize::to_html(&self.root, false, 2)
    }

    /// Serialize to pretty HTML
    pub fn to_html_pretty(&self) -> String {
        serialize::to_html(&self.root, true, 2)
    }

    /// Serialize to html5lib test format
    pub fn to_test_format(&self) -> String {
        serialize::to_test_format(&self.root)
    }

    /// Extract text content
    pub fn to_text(&self) -> String {
        serialize::to_text(&self.root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_parse() {
        let doc = JustHTML::parse("<p>Hello</p>");
        assert!(!doc.root.children.is_empty());
    }

    #[test]
    fn fragment_parse() {
        let doc = JustHTML::parse_fragment("<p>Hello</p>", FragmentContext::new("body"));
        assert_eq!(doc.root.name, "#document-fragment");
    }
}
