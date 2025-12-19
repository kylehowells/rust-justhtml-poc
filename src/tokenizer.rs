//! HTML5 tokenizer state machine

use std::collections::HashMap;
use crate::tokens::{ParseError, Token};
use crate::node::{Doctype, Namespace};
use crate::entities::{decode_numeric_entity, NAMED_ENTITIES, LEGACY_ENTITIES};

/// Protocol for receiving tokens from the tokenizer
pub trait TokenSink {
    fn process_token(&mut self, token: Token);
    fn current_namespace(&self) -> Option<Namespace>;
}

/// RCDATA elements that switch tokenizer to RCDATA state
const RCDATA_ELEMENTS: &[&str] = &["title", "textarea"];

/// RAWTEXT elements that switch tokenizer to RAWTEXT state
const RAWTEXT_ELEMENTS: &[&str] = &["style", "xmp", "iframe", "noembed", "noframes"];

/// Preprocess line endings per HTML5 spec
fn preprocess_line_endings(html: &str) -> String {
    if !html.contains('\r') {
        return html.to_string();
    }

    let mut result = String::with_capacity(html.len());
    let mut prev_was_cr = false;

    for c in html.chars() {
        if c == '\r' {
            result.push('\n');
            prev_was_cr = true;
        } else if c == '\n' && prev_was_cr {
            prev_was_cr = false;
        } else {
            result.push(c);
            prev_was_cr = false;
        }
    }

    result
}

/// Tokenizer states
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    Data,
    Rcdata,
    Rawtext,
    ScriptData,
    Plaintext,
    TagOpen,
    EndTagOpen,
    TagName,
    RcdataLessThan,
    RcdataEndTagOpen,
    RcdataEndTagName,
    RawtextLessThan,
    RawtextEndTagOpen,
    RawtextEndTagName,
    ScriptDataLessThan,
    ScriptDataEndTagOpen,
    ScriptDataEndTagName,
    ScriptDataEscapeStart,
    ScriptDataEscapeStartDash,
    ScriptDataEscaped,
    ScriptDataEscapedDash,
    ScriptDataEscapedDashDash,
    ScriptDataEscapedLessThan,
    ScriptDataEscapedEndTagOpen,
    ScriptDataEscapedEndTagName,
    ScriptDataDoubleEscapeStart,
    ScriptDataDoubleEscaped,
    ScriptDataDoubleEscapedDash,
    ScriptDataDoubleEscapedDashDash,
    ScriptDataDoubleEscapedLessThan,
    ScriptDataDoubleEscapeEnd,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    AfterDoctypePublicKeyword,
    BeforeDoctypePublicIdentifier,
    DoctypePublicIdentifierDoubleQuoted,
    DoctypePublicIdentifierSingleQuoted,
    AfterDoctypePublicIdentifier,
    BetweenDoctypePublicAndSystemIdentifiers,
    AfterDoctypeSystemKeyword,
    BeforeDoctypeSystemIdentifier,
    DoctypeSystemIdentifierDoubleQuoted,
    DoctypeSystemIdentifierSingleQuoted,
    AfterDoctypeSystemIdentifier,
    BogusDoctype,
    CdataSection,
    CdataSectionBracket,
    CdataSectionEnd,
    CharacterReference,
    NamedCharacterReference,
    AmbiguousAmpersand,
    NumericCharacterReference,
    HexadecimalCharacterReferenceStart,
    DecimalCharacterReferenceStart,
    HexadecimalCharacterReference,
    DecimalCharacterReference,
    NumericCharacterReferenceEnd,
}

/// HTML5 tokenizer
pub struct Tokenizer<'a> {
    sink: &'a mut dyn TokenSink,
    scripting: bool,

    state: State,
    return_state: State,

    // Input handling
    input: Vec<char>,
    pos: usize,

    // Current token being built
    current_tag_name: String,
    current_tag_is_end: bool,
    current_tag_self_closing: bool,
    current_attrs: HashMap<String, String>,
    current_attr_name: String,
    current_attr_value: String,

    // Comment/doctype building
    current_comment: String,
    current_doctype_name: String,
    current_doctype_public_id: Option<String>,
    current_doctype_system_id: Option<String>,
    current_doctype_force_quirks: bool,

    // Character buffer
    char_buffer: String,

    // Temporary buffer for rawtext/rcdata end tag matching
    temp_buffer: String,
    last_start_tag_name: String,

    // Character reference state
    char_ref_code: u32,

    // Track if last consume was EOF (for proper reconsume behavior)
    last_was_eof: bool,

    // Error collection
    pub errors: Vec<ParseError>,
}

impl<'a> Tokenizer<'a> {
    pub fn new(sink: &'a mut dyn TokenSink) -> Self {
        Self::with_options(sink, State::Data, false)
    }

    pub fn with_options(sink: &'a mut dyn TokenSink, initial_state: State, scripting: bool) -> Self {
        Self {
            sink,
            scripting,
            state: initial_state,
            return_state: State::Data,
            input: Vec::new(),
            pos: 0,
            current_tag_name: String::new(),
            current_tag_is_end: false,
            current_tag_self_closing: false,
            current_attrs: HashMap::new(),
            current_attr_name: String::new(),
            current_attr_value: String::new(),
            current_comment: String::new(),
            current_doctype_name: String::new(),
            current_doctype_public_id: None,
            current_doctype_system_id: None,
            current_doctype_force_quirks: false,
            char_buffer: String::new(),
            temp_buffer: String::new(),
            last_start_tag_name: String::new(),
            char_ref_code: 0,
            last_was_eof: false,
            errors: Vec::new(),
        }
    }

    pub fn run(&mut self, html: &str) {
        let preprocessed = preprocess_line_endings(html);
        self.input = preprocessed.chars().collect();
        self.pos = 0;

        // Process all input
        while self.pos < self.input.len() {
            self.process_state();
        }

        // Handle EOF
        let mut eof_iterations = 0;
        while self.state != State::Data && eof_iterations < 100 {
            self.process_state();
            eof_iterations += 1;
        }

        // Flush and emit EOF
        self.flush_char_buffer();
        self.emit(Token::Eof);
    }

    fn consume(&mut self) -> Option<char> {
        if self.pos < self.input.len() {
            let c = self.input[self.pos];
            self.pos += 1;
            self.last_was_eof = false;
            Some(c)
        } else {
            self.last_was_eof = true;
            None
        }
    }

    fn peek(&self) -> Option<char> {
        if self.pos < self.input.len() {
            Some(self.input[self.pos])
        } else {
            None
        }
    }

    fn reconsume(&mut self) {
        // Only decrement if the last consume wasn't EOF
        // (EOF doesn't advance pos, so reconsuming EOF shouldn't decrement)
        if !self.last_was_eof && self.pos > 0 {
            self.pos -= 1;
        }
    }

    fn consume_if(&mut self, expected: &str, case_insensitive: bool) -> bool {
        let expected_chars: Vec<char> = expected.chars().collect();
        let mut temp_pos = self.pos;

        for &expected_char in &expected_chars {
            if temp_pos >= self.input.len() {
                return false;
            }

            let input_char = self.input[temp_pos];
            let matches = if case_insensitive {
                input_char.to_ascii_lowercase() == expected_char.to_ascii_lowercase()
            } else {
                input_char == expected_char
            };

            if !matches {
                return false;
            }
            temp_pos += 1;
        }

        self.pos = temp_pos;
        true
    }

    // Token emission
    fn emit(&mut self, token: Token) {
        self.flush_char_buffer();
        self.sink.process_token(token);
    }

    fn emit_char(&mut self, c: char) {
        self.char_buffer.push(c);
    }

    fn emit_string(&mut self, s: &str) {
        self.char_buffer.push_str(s);
    }

    fn flush_char_buffer(&mut self) {
        if !self.char_buffer.is_empty() {
            let text = std::mem::take(&mut self.char_buffer);
            for c in text.chars() {
                self.sink.process_token(Token::Character(c));
            }
        }
    }

    fn emit_current_tag(&mut self) {
        self.flush_char_buffer();
        if self.current_tag_is_end {
            self.sink.process_token(Token::EndTag {
                name: self.current_tag_name.clone(),
            });
        } else {
            self.sink.process_token(Token::StartTag {
                name: self.current_tag_name.clone(),
                attrs: self.current_attrs.clone(),
                self_closing: self.current_tag_self_closing,
            });
            self.last_start_tag_name = self.current_tag_name.clone();

            // Switch to appropriate state for special elements
            let ns = self.sink.current_namespace();
            if ns.is_none() || ns == Some(Namespace::Html) {
                if RCDATA_ELEMENTS.contains(&self.current_tag_name.as_str()) {
                    self.state = State::Rcdata;
                } else if RAWTEXT_ELEMENTS.contains(&self.current_tag_name.as_str()) {
                    self.state = State::Rawtext;
                } else if self.current_tag_name == "noscript" && self.scripting {
                    self.state = State::Rawtext;
                } else if self.current_tag_name == "script" {
                    self.state = State::ScriptData;
                } else if self.current_tag_name == "plaintext" {
                    self.state = State::Plaintext;
                }
            }
        }
        self.reset_tag();
    }

    fn emit_current_comment(&mut self) {
        self.emit(Token::Comment(self.current_comment.clone()));
        self.current_comment.clear();
    }

    fn emit_current_doctype(&mut self) {
        let doctype = Doctype {
            name: if self.current_doctype_name.is_empty() {
                None
            } else {
                Some(self.current_doctype_name.clone())
            },
            public_id: self.current_doctype_public_id.clone(),
            system_id: self.current_doctype_system_id.clone(),
            force_quirks: self.current_doctype_force_quirks,
        };
        self.emit(Token::Doctype(doctype));
        self.reset_doctype();
    }

    fn reset_tag(&mut self) {
        self.current_tag_name.clear();
        self.current_tag_is_end = false;
        self.current_tag_self_closing = false;
        self.current_attrs.clear();
        self.current_attr_name.clear();
        self.current_attr_value.clear();
    }

    fn reset_doctype(&mut self) {
        self.current_doctype_name.clear();
        self.current_doctype_public_id = None;
        self.current_doctype_system_id = None;
        self.current_doctype_force_quirks = false;
    }

    fn finish_attribute(&mut self) {
        if !self.current_attr_name.is_empty() {
            let name = std::mem::take(&mut self.current_attr_name);
            let value = std::mem::take(&mut self.current_attr_value);
            // Only add if not already present (first wins)
            if !self.current_attrs.contains_key(&name) {
                self.current_attrs.insert(name, value);
            }
        }
    }

    fn is_appropriate_end_tag(&self) -> bool {
        !self.last_start_tag_name.is_empty()
            && self.temp_buffer.to_ascii_lowercase() == self.last_start_tag_name
    }

    fn error(&mut self, code: &str) {
        self.errors.push(ParseError::new(code));
    }

    fn process_state(&mut self) {
        match self.state {
            State::Data => self.data_state(),
            State::Rcdata => self.rcdata_state(),
            State::Rawtext => self.rawtext_state(),
            State::ScriptData => self.script_data_state(),
            State::Plaintext => self.plaintext_state(),
            State::TagOpen => self.tag_open_state(),
            State::EndTagOpen => self.end_tag_open_state(),
            State::TagName => self.tag_name_state(),
            State::RcdataLessThan => self.rcdata_less_than_state(),
            State::RcdataEndTagOpen => self.rcdata_end_tag_open_state(),
            State::RcdataEndTagName => self.rcdata_end_tag_name_state(),
            State::RawtextLessThan => self.rawtext_less_than_state(),
            State::RawtextEndTagOpen => self.rawtext_end_tag_open_state(),
            State::RawtextEndTagName => self.rawtext_end_tag_name_state(),
            State::ScriptDataLessThan => self.script_data_less_than_state(),
            State::ScriptDataEndTagOpen => self.script_data_end_tag_open_state(),
            State::ScriptDataEndTagName => self.script_data_end_tag_name_state(),
            State::ScriptDataEscapeStart => self.script_data_escape_start_state(),
            State::ScriptDataEscapeStartDash => self.script_data_escape_start_dash_state(),
            State::ScriptDataEscaped => self.script_data_escaped_state(),
            State::ScriptDataEscapedDash => self.script_data_escaped_dash_state(),
            State::ScriptDataEscapedDashDash => self.script_data_escaped_dash_dash_state(),
            State::ScriptDataEscapedLessThan => self.script_data_escaped_less_than_state(),
            State::ScriptDataEscapedEndTagOpen => self.script_data_escaped_end_tag_open_state(),
            State::ScriptDataEscapedEndTagName => self.script_data_escaped_end_tag_name_state(),
            State::ScriptDataDoubleEscapeStart => self.script_data_double_escape_start_state(),
            State::ScriptDataDoubleEscaped => self.script_data_double_escaped_state(),
            State::ScriptDataDoubleEscapedDash => self.script_data_double_escaped_dash_state(),
            State::ScriptDataDoubleEscapedDashDash => self.script_data_double_escaped_dash_dash_state(),
            State::ScriptDataDoubleEscapedLessThan => self.script_data_double_escaped_less_than_state(),
            State::ScriptDataDoubleEscapeEnd => self.script_data_double_escape_end_state(),
            State::BeforeAttributeName => self.before_attribute_name_state(),
            State::AttributeName => self.attribute_name_state(),
            State::AfterAttributeName => self.after_attribute_name_state(),
            State::BeforeAttributeValue => self.before_attribute_value_state(),
            State::AttributeValueDoubleQuoted => self.attribute_value_double_quoted_state(),
            State::AttributeValueSingleQuoted => self.attribute_value_single_quoted_state(),
            State::AttributeValueUnquoted => self.attribute_value_unquoted_state(),
            State::AfterAttributeValueQuoted => self.after_attribute_value_quoted_state(),
            State::SelfClosingStartTag => self.self_closing_start_tag_state(),
            State::BogusComment => self.bogus_comment_state(),
            State::MarkupDeclarationOpen => self.markup_declaration_open_state(),
            State::CommentStart => self.comment_start_state(),
            State::CommentStartDash => self.comment_start_dash_state(),
            State::Comment => self.comment_state(),
            State::CommentEndDash => self.comment_end_dash_state(),
            State::CommentEnd => self.comment_end_state(),
            State::CommentEndBang => self.comment_end_bang_state(),
            State::Doctype => self.doctype_state(),
            State::BeforeDoctypeName => self.before_doctype_name_state(),
            State::DoctypeName => self.doctype_name_state(),
            State::AfterDoctypeName => self.after_doctype_name_state(),
            State::AfterDoctypePublicKeyword => self.after_doctype_public_keyword_state(),
            State::BeforeDoctypePublicIdentifier => self.before_doctype_public_identifier_state(),
            State::DoctypePublicIdentifierDoubleQuoted => self.doctype_public_identifier_double_quoted_state(),
            State::DoctypePublicIdentifierSingleQuoted => self.doctype_public_identifier_single_quoted_state(),
            State::AfterDoctypePublicIdentifier => self.after_doctype_public_identifier_state(),
            State::BetweenDoctypePublicAndSystemIdentifiers => self.between_doctype_public_and_system_identifiers_state(),
            State::AfterDoctypeSystemKeyword => self.after_doctype_system_keyword_state(),
            State::BeforeDoctypeSystemIdentifier => self.before_doctype_system_identifier_state(),
            State::DoctypeSystemIdentifierDoubleQuoted => self.doctype_system_identifier_double_quoted_state(),
            State::DoctypeSystemIdentifierSingleQuoted => self.doctype_system_identifier_single_quoted_state(),
            State::AfterDoctypeSystemIdentifier => self.after_doctype_system_identifier_state(),
            State::BogusDoctype => self.bogus_doctype_state(),
            State::CdataSection => self.cdata_section_state(),
            State::CdataSectionBracket => self.cdata_section_bracket_state(),
            State::CdataSectionEnd => self.cdata_section_end_state(),
            State::CharacterReference => self.character_reference_state(),
            State::NamedCharacterReference => self.named_character_reference_state(),
            State::AmbiguousAmpersand => self.ambiguous_ampersand_state(),
            State::NumericCharacterReference => self.numeric_character_reference_state(),
            State::HexadecimalCharacterReferenceStart => self.hexadecimal_character_reference_start_state(),
            State::DecimalCharacterReferenceStart => self.decimal_character_reference_start_state(),
            State::HexadecimalCharacterReference => self.hexadecimal_character_reference_state(),
            State::DecimalCharacterReference => self.decimal_character_reference_state(),
            State::NumericCharacterReferenceEnd => self.numeric_character_reference_end_state(),
        }
    }

    // State implementations
    fn data_state(&mut self) {
        match self.consume() {
            Some('&') => {
                self.return_state = State::Data;
                self.state = State::CharacterReference;
            }
            Some('<') => {
                self.state = State::TagOpen;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.emit_char('\u{FFFD}');
            }
            Some(c) => {
                self.emit_char(c);
            }
            None => {
                self.state = State::Data;
            }
        }
    }

    fn rcdata_state(&mut self) {
        match self.consume() {
            Some('&') => {
                self.return_state = State::Rcdata;
                self.state = State::CharacterReference;
            }
            Some('<') => {
                self.state = State::RcdataLessThan;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.emit_char('\u{FFFD}');
            }
            Some(c) => {
                self.emit_char(c);
            }
            None => {
                self.state = State::Data;
            }
        }
    }

    fn rawtext_state(&mut self) {
        match self.consume() {
            Some('<') => {
                self.state = State::RawtextLessThan;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.emit_char('\u{FFFD}');
            }
            Some(c) => {
                self.emit_char(c);
            }
            None => {
                self.state = State::Data;
            }
        }
    }

    fn script_data_state(&mut self) {
        match self.consume() {
            Some('<') => {
                self.state = State::ScriptDataLessThan;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.emit_char('\u{FFFD}');
            }
            Some(c) => {
                self.emit_char(c);
            }
            None => {
                self.state = State::Data;
            }
        }
    }

    fn plaintext_state(&mut self) {
        match self.consume() {
            Some('\0') => {
                self.error("unexpected-null-character");
                self.emit_char('\u{FFFD}');
            }
            Some(c) => {
                self.emit_char(c);
            }
            None => {
                self.state = State::Data;
            }
        }
    }

    fn tag_open_state(&mut self) {
        match self.consume() {
            Some('!') => {
                self.state = State::MarkupDeclarationOpen;
            }
            Some('/') => {
                self.state = State::EndTagOpen;
            }
            Some(c) if c.is_ascii_alphabetic() => {
                self.current_tag_name.clear();
                self.current_tag_is_end = false;
                self.reconsume();
                self.state = State::TagName;
            }
            Some('?') => {
                self.error("unexpected-question-mark-instead-of-tag-name");
                self.current_comment.clear();
                self.reconsume();
                self.state = State::BogusComment;
            }
            Some(_) => {
                self.error("invalid-first-character-of-tag-name");
                self.emit_char('<');
                self.reconsume();
                self.state = State::Data;
            }
            None => {
                self.error("eof-before-tag-name");
                self.emit_char('<');
                self.state = State::Data;
            }
        }
    }

    fn end_tag_open_state(&mut self) {
        match self.consume() {
            Some(c) if c.is_ascii_alphabetic() => {
                self.current_tag_name.clear();
                self.current_tag_is_end = true;
                self.reconsume();
                self.state = State::TagName;
            }
            Some('>') => {
                self.error("missing-end-tag-name");
                self.state = State::Data;
            }
            Some(_) => {
                self.error("invalid-first-character-of-tag-name");
                self.current_comment.clear();
                self.reconsume();
                self.state = State::BogusComment;
            }
            None => {
                self.error("eof-before-tag-name");
                self.emit_char('<');
                self.emit_char('/');
                self.state = State::Data;
            }
        }
    }

    fn tag_name_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                self.state = State::BeforeAttributeName;
            }
            Some('/') => {
                self.state = State::SelfClosingStartTag;
            }
            Some('>') => {
                self.state = State::Data;
                self.emit_current_tag();
            }
            Some(c) if c.is_ascii_uppercase() => {
                self.current_tag_name.push(c.to_ascii_lowercase());
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.current_tag_name.push('\u{FFFD}');
            }
            Some(c) => {
                self.current_tag_name.push(c);
            }
            None => {
                self.error("eof-in-tag");
                self.state = State::Data;
            }
        }
    }

    fn rcdata_less_than_state(&mut self) {
        match self.consume() {
            Some('/') => {
                self.temp_buffer.clear();
                self.state = State::RcdataEndTagOpen;
            }
            _ => {
                self.emit_char('<');
                self.reconsume();
                self.state = State::Rcdata;
            }
        }
    }

    fn rcdata_end_tag_open_state(&mut self) {
        match self.consume() {
            Some(c) if c.is_ascii_alphabetic() => {
                self.current_tag_name.clear();
                self.current_tag_is_end = true;
                self.reconsume();
                self.state = State::RcdataEndTagName;
            }
            _ => {
                self.emit_char('<');
                self.emit_char('/');
                self.reconsume();
                self.state = State::Rcdata;
            }
        }
    }

    fn rcdata_end_tag_name_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') if self.is_appropriate_end_tag() => {
                self.state = State::BeforeAttributeName;
            }
            Some('/') if self.is_appropriate_end_tag() => {
                self.state = State::SelfClosingStartTag;
            }
            Some('>') if self.is_appropriate_end_tag() => {
                self.state = State::Data;
                self.emit_current_tag();
            }
            Some(c) if c.is_ascii_uppercase() => {
                self.current_tag_name.push(c.to_ascii_lowercase());
                self.temp_buffer.push(c);
            }
            Some(c) if c.is_ascii_lowercase() => {
                self.current_tag_name.push(c);
                self.temp_buffer.push(c);
            }
            _ => {
                self.emit_char('<');
                self.emit_char('/');
                self.emit_string(&self.temp_buffer.clone());
                self.reconsume();
                self.state = State::Rcdata;
            }
        }
    }

    fn rawtext_less_than_state(&mut self) {
        match self.consume() {
            Some('/') => {
                self.temp_buffer.clear();
                self.state = State::RawtextEndTagOpen;
            }
            _ => {
                self.emit_char('<');
                self.reconsume();
                self.state = State::Rawtext;
            }
        }
    }

    fn rawtext_end_tag_open_state(&mut self) {
        match self.consume() {
            Some(c) if c.is_ascii_alphabetic() => {
                self.current_tag_name.clear();
                self.current_tag_is_end = true;
                self.reconsume();
                self.state = State::RawtextEndTagName;
            }
            _ => {
                self.emit_char('<');
                self.emit_char('/');
                self.reconsume();
                self.state = State::Rawtext;
            }
        }
    }

    fn rawtext_end_tag_name_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') if self.is_appropriate_end_tag() => {
                self.state = State::BeforeAttributeName;
            }
            Some('/') if self.is_appropriate_end_tag() => {
                self.state = State::SelfClosingStartTag;
            }
            Some('>') if self.is_appropriate_end_tag() => {
                self.state = State::Data;
                self.emit_current_tag();
            }
            Some(c) if c.is_ascii_uppercase() => {
                self.current_tag_name.push(c.to_ascii_lowercase());
                self.temp_buffer.push(c);
            }
            Some(c) if c.is_ascii_lowercase() => {
                self.current_tag_name.push(c);
                self.temp_buffer.push(c);
            }
            _ => {
                self.emit_char('<');
                self.emit_char('/');
                self.emit_string(&self.temp_buffer.clone());
                self.reconsume();
                self.state = State::Rawtext;
            }
        }
    }

    fn script_data_less_than_state(&mut self) {
        match self.consume() {
            Some('/') => {
                self.temp_buffer.clear();
                self.state = State::ScriptDataEndTagOpen;
            }
            Some('!') => {
                self.state = State::ScriptDataEscapeStart;
                self.emit_char('<');
                self.emit_char('!');
            }
            _ => {
                self.emit_char('<');
                self.reconsume();
                self.state = State::ScriptData;
            }
        }
    }

    fn script_data_end_tag_open_state(&mut self) {
        match self.consume() {
            Some(c) if c.is_ascii_alphabetic() => {
                self.current_tag_name.clear();
                self.current_tag_is_end = true;
                self.reconsume();
                self.state = State::ScriptDataEndTagName;
            }
            _ => {
                self.emit_char('<');
                self.emit_char('/');
                self.reconsume();
                self.state = State::ScriptData;
            }
        }
    }

    fn script_data_end_tag_name_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') if self.is_appropriate_end_tag() => {
                self.state = State::BeforeAttributeName;
            }
            Some('/') if self.is_appropriate_end_tag() => {
                self.state = State::SelfClosingStartTag;
            }
            Some('>') if self.is_appropriate_end_tag() => {
                self.state = State::Data;
                self.emit_current_tag();
            }
            Some(c) if c.is_ascii_uppercase() => {
                self.current_tag_name.push(c.to_ascii_lowercase());
                self.temp_buffer.push(c);
            }
            Some(c) if c.is_ascii_lowercase() => {
                self.current_tag_name.push(c);
                self.temp_buffer.push(c);
            }
            _ => {
                self.emit_char('<');
                self.emit_char('/');
                self.emit_string(&self.temp_buffer.clone());
                self.reconsume();
                self.state = State::ScriptData;
            }
        }
    }

    fn script_data_escape_start_state(&mut self) {
        match self.consume() {
            Some('-') => {
                self.state = State::ScriptDataEscapeStartDash;
                self.emit_char('-');
            }
            _ => {
                self.reconsume();
                self.state = State::ScriptData;
            }
        }
    }

    fn script_data_escape_start_dash_state(&mut self) {
        match self.consume() {
            Some('-') => {
                self.state = State::ScriptDataEscapedDashDash;
                self.emit_char('-');
            }
            _ => {
                self.reconsume();
                self.state = State::ScriptData;
            }
        }
    }

    fn script_data_escaped_state(&mut self) {
        match self.consume() {
            Some('-') => {
                self.state = State::ScriptDataEscapedDash;
                self.emit_char('-');
            }
            Some('<') => {
                self.state = State::ScriptDataEscapedLessThan;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.emit_char('\u{FFFD}');
            }
            Some(c) => {
                self.emit_char(c);
            }
            None => {
                self.error("eof-in-script-html-comment-like-text");
                self.state = State::Data;
            }
        }
    }

    fn script_data_escaped_dash_state(&mut self) {
        match self.consume() {
            Some('-') => {
                self.state = State::ScriptDataEscapedDashDash;
                self.emit_char('-');
            }
            Some('<') => {
                self.state = State::ScriptDataEscapedLessThan;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.state = State::ScriptDataEscaped;
                self.emit_char('\u{FFFD}');
            }
            Some(c) => {
                self.state = State::ScriptDataEscaped;
                self.emit_char(c);
            }
            None => {
                self.error("eof-in-script-html-comment-like-text");
                self.state = State::Data;
            }
        }
    }

    fn script_data_escaped_dash_dash_state(&mut self) {
        match self.consume() {
            Some('-') => {
                self.emit_char('-');
            }
            Some('<') => {
                self.state = State::ScriptDataEscapedLessThan;
            }
            Some('>') => {
                self.state = State::ScriptData;
                self.emit_char('>');
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.state = State::ScriptDataEscaped;
                self.emit_char('\u{FFFD}');
            }
            Some(c) => {
                self.state = State::ScriptDataEscaped;
                self.emit_char(c);
            }
            None => {
                self.error("eof-in-script-html-comment-like-text");
                self.state = State::Data;
            }
        }
    }

    fn script_data_escaped_less_than_state(&mut self) {
        match self.consume() {
            Some('/') => {
                self.temp_buffer.clear();
                self.state = State::ScriptDataEscapedEndTagOpen;
            }
            Some(c) if c.is_ascii_alphabetic() => {
                self.temp_buffer.clear();
                self.emit_char('<');
                self.reconsume();
                self.state = State::ScriptDataDoubleEscapeStart;
            }
            _ => {
                self.emit_char('<');
                self.reconsume();
                self.state = State::ScriptDataEscaped;
            }
        }
    }

    fn script_data_escaped_end_tag_open_state(&mut self) {
        match self.consume() {
            Some(c) if c.is_ascii_alphabetic() => {
                self.current_tag_name.clear();
                self.current_tag_is_end = true;
                self.reconsume();
                self.state = State::ScriptDataEscapedEndTagName;
            }
            _ => {
                self.emit_char('<');
                self.emit_char('/');
                self.reconsume();
                self.state = State::ScriptDataEscaped;
            }
        }
    }

    fn script_data_escaped_end_tag_name_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') if self.is_appropriate_end_tag() => {
                self.state = State::BeforeAttributeName;
            }
            Some('/') if self.is_appropriate_end_tag() => {
                self.state = State::SelfClosingStartTag;
            }
            Some('>') if self.is_appropriate_end_tag() => {
                self.state = State::Data;
                self.emit_current_tag();
            }
            Some(c) if c.is_ascii_uppercase() => {
                self.current_tag_name.push(c.to_ascii_lowercase());
                self.temp_buffer.push(c);
            }
            Some(c) if c.is_ascii_lowercase() => {
                self.current_tag_name.push(c);
                self.temp_buffer.push(c);
            }
            _ => {
                self.emit_char('<');
                self.emit_char('/');
                self.emit_string(&self.temp_buffer.clone());
                self.reconsume();
                self.state = State::ScriptDataEscaped;
            }
        }
    }

    fn script_data_double_escape_start_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') | Some('/') | Some('>') => {
                if self.temp_buffer.to_ascii_lowercase() == "script" {
                    self.state = State::ScriptDataDoubleEscaped;
                } else {
                    self.state = State::ScriptDataEscaped;
                }
                self.emit_char(self.input[self.pos - 1]);
            }
            Some(c) if c.is_ascii_uppercase() => {
                self.temp_buffer.push(c.to_ascii_lowercase());
                self.emit_char(c);
            }
            Some(c) if c.is_ascii_lowercase() => {
                self.temp_buffer.push(c);
                self.emit_char(c);
            }
            _ => {
                self.reconsume();
                self.state = State::ScriptDataEscaped;
            }
        }
    }

    fn script_data_double_escaped_state(&mut self) {
        match self.consume() {
            Some('-') => {
                self.state = State::ScriptDataDoubleEscapedDash;
                self.emit_char('-');
            }
            Some('<') => {
                self.state = State::ScriptDataDoubleEscapedLessThan;
                self.emit_char('<');
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.emit_char('\u{FFFD}');
            }
            Some(c) => {
                self.emit_char(c);
            }
            None => {
                self.error("eof-in-script-html-comment-like-text");
                self.state = State::Data;
            }
        }
    }

    fn script_data_double_escaped_dash_state(&mut self) {
        match self.consume() {
            Some('-') => {
                self.state = State::ScriptDataDoubleEscapedDashDash;
                self.emit_char('-');
            }
            Some('<') => {
                self.state = State::ScriptDataDoubleEscapedLessThan;
                self.emit_char('<');
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.state = State::ScriptDataDoubleEscaped;
                self.emit_char('\u{FFFD}');
            }
            Some(c) => {
                self.state = State::ScriptDataDoubleEscaped;
                self.emit_char(c);
            }
            None => {
                self.error("eof-in-script-html-comment-like-text");
                self.state = State::Data;
            }
        }
    }

    fn script_data_double_escaped_dash_dash_state(&mut self) {
        match self.consume() {
            Some('-') => {
                self.emit_char('-');
            }
            Some('<') => {
                self.state = State::ScriptDataDoubleEscapedLessThan;
                self.emit_char('<');
            }
            Some('>') => {
                self.state = State::ScriptData;
                self.emit_char('>');
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.state = State::ScriptDataDoubleEscaped;
                self.emit_char('\u{FFFD}');
            }
            Some(c) => {
                self.state = State::ScriptDataDoubleEscaped;
                self.emit_char(c);
            }
            None => {
                self.error("eof-in-script-html-comment-like-text");
                self.state = State::Data;
            }
        }
    }

    fn script_data_double_escaped_less_than_state(&mut self) {
        match self.consume() {
            Some('/') => {
                self.temp_buffer.clear();
                self.state = State::ScriptDataDoubleEscapeEnd;
                self.emit_char('/');
            }
            _ => {
                self.reconsume();
                self.state = State::ScriptDataDoubleEscaped;
            }
        }
    }

    fn script_data_double_escape_end_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') | Some('/') | Some('>') => {
                if self.temp_buffer.to_ascii_lowercase() == "script" {
                    self.state = State::ScriptDataEscaped;
                } else {
                    self.state = State::ScriptDataDoubleEscaped;
                }
                self.emit_char(self.input[self.pos - 1]);
            }
            Some(c) if c.is_ascii_uppercase() => {
                self.temp_buffer.push(c.to_ascii_lowercase());
                self.emit_char(c);
            }
            Some(c) if c.is_ascii_lowercase() => {
                self.temp_buffer.push(c);
                self.emit_char(c);
            }
            _ => {
                self.reconsume();
                self.state = State::ScriptDataDoubleEscaped;
            }
        }
    }

    fn before_attribute_name_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                // Ignore
            }
            Some('/') | Some('>') | None => {
                self.reconsume();
                self.state = State::AfterAttributeName;
            }
            Some('=') => {
                self.error("unexpected-equals-sign-before-attribute-name");
                self.current_attr_name = "=".to_string();
                self.current_attr_value.clear();
                self.state = State::AttributeName;
            }
            _ => {
                self.current_attr_name.clear();
                self.current_attr_value.clear();
                self.reconsume();
                self.state = State::AttributeName;
            }
        }
    }

    fn attribute_name_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') | Some('/') | Some('>') | None => {
                self.reconsume();
                self.state = State::AfterAttributeName;
            }
            Some('=') => {
                self.state = State::BeforeAttributeValue;
            }
            Some(c) if c.is_ascii_uppercase() => {
                self.current_attr_name.push(c.to_ascii_lowercase());
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.current_attr_name.push('\u{FFFD}');
            }
            Some('"') | Some('\'') | Some('<') => {
                self.error("unexpected-character-in-attribute-name");
                self.current_attr_name.push(self.input[self.pos - 1]);
            }
            Some(c) => {
                self.current_attr_name.push(c);
            }
        }
    }

    fn after_attribute_name_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                // Ignore
            }
            Some('/') => {
                self.finish_attribute();
                self.state = State::SelfClosingStartTag;
            }
            Some('=') => {
                self.state = State::BeforeAttributeValue;
            }
            Some('>') => {
                self.finish_attribute();
                self.state = State::Data;
                self.emit_current_tag();
            }
            None => {
                self.error("eof-in-tag");
                self.state = State::Data;
            }
            _ => {
                self.finish_attribute();
                self.current_attr_name.clear();
                self.current_attr_value.clear();
                self.reconsume();
                self.state = State::AttributeName;
            }
        }
    }

    fn before_attribute_value_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                // Ignore
            }
            Some('"') => {
                self.state = State::AttributeValueDoubleQuoted;
            }
            Some('\'') => {
                self.state = State::AttributeValueSingleQuoted;
            }
            Some('>') => {
                self.error("missing-attribute-value");
                self.finish_attribute();
                self.state = State::Data;
                self.emit_current_tag();
            }
            _ => {
                self.reconsume();
                self.state = State::AttributeValueUnquoted;
            }
        }
    }

    fn attribute_value_double_quoted_state(&mut self) {
        match self.consume() {
            Some('"') => {
                self.finish_attribute();
                self.state = State::AfterAttributeValueQuoted;
            }
            Some('&') => {
                self.return_state = State::AttributeValueDoubleQuoted;
                self.state = State::CharacterReference;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.current_attr_value.push('\u{FFFD}');
            }
            Some(c) => {
                self.current_attr_value.push(c);
            }
            None => {
                self.error("eof-in-tag");
                self.state = State::Data;
            }
        }
    }

    fn attribute_value_single_quoted_state(&mut self) {
        match self.consume() {
            Some('\'') => {
                self.finish_attribute();
                self.state = State::AfterAttributeValueQuoted;
            }
            Some('&') => {
                self.return_state = State::AttributeValueSingleQuoted;
                self.state = State::CharacterReference;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.current_attr_value.push('\u{FFFD}');
            }
            Some(c) => {
                self.current_attr_value.push(c);
            }
            None => {
                self.error("eof-in-tag");
                self.state = State::Data;
            }
        }
    }

    fn attribute_value_unquoted_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                self.finish_attribute();
                self.state = State::BeforeAttributeName;
            }
            Some('&') => {
                self.return_state = State::AttributeValueUnquoted;
                self.state = State::CharacterReference;
            }
            Some('>') => {
                self.finish_attribute();
                self.state = State::Data;
                self.emit_current_tag();
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.current_attr_value.push('\u{FFFD}');
            }
            Some('"') | Some('\'') | Some('<') | Some('=') | Some('`') => {
                self.error("unexpected-character-in-unquoted-attribute-value");
                self.current_attr_value.push(self.input[self.pos - 1]);
            }
            Some(c) => {
                self.current_attr_value.push(c);
            }
            None => {
                self.error("eof-in-tag");
                self.state = State::Data;
            }
        }
    }

    fn after_attribute_value_quoted_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                self.state = State::BeforeAttributeName;
            }
            Some('/') => {
                self.state = State::SelfClosingStartTag;
            }
            Some('>') => {
                self.state = State::Data;
                self.emit_current_tag();
            }
            None => {
                self.error("eof-in-tag");
                self.state = State::Data;
            }
            _ => {
                self.error("missing-whitespace-between-attributes");
                self.reconsume();
                self.state = State::BeforeAttributeName;
            }
        }
    }

    fn self_closing_start_tag_state(&mut self) {
        match self.consume() {
            Some('>') => {
                self.current_tag_self_closing = true;
                self.state = State::Data;
                self.emit_current_tag();
            }
            None => {
                self.error("eof-in-tag");
                self.state = State::Data;
            }
            _ => {
                self.error("unexpected-solidus-in-tag");
                self.reconsume();
                self.state = State::BeforeAttributeName;
            }
        }
    }

    fn bogus_comment_state(&mut self) {
        match self.consume() {
            Some('>') => {
                self.state = State::Data;
                self.emit_current_comment();
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.current_comment.push('\u{FFFD}');
            }
            Some(c) => {
                self.current_comment.push(c);
            }
            None => {
                self.emit_current_comment();
                self.state = State::Data;
            }
        }
    }

    fn markup_declaration_open_state(&mut self) {
        if self.consume_if("--", false) {
            self.current_comment.clear();
            self.state = State::CommentStart;
        } else if self.consume_if("DOCTYPE", true) {
            self.state = State::Doctype;
        } else if self.consume_if("[CDATA[", false) {
            // Only allowed in foreign content
            let ns = self.sink.current_namespace();
            if ns == Some(Namespace::Svg) || ns == Some(Namespace::MathML) {
                self.state = State::CdataSection;
            } else {
                self.error("cdata-in-html-content");
                self.current_comment = "[CDATA[".to_string();
                self.state = State::BogusComment;
            }
        } else {
            self.error("incorrectly-opened-comment");
            self.current_comment.clear();
            self.state = State::BogusComment;
        }
    }

    fn comment_start_state(&mut self) {
        match self.consume() {
            Some('-') => {
                self.state = State::CommentStartDash;
            }
            Some('>') => {
                self.error("abrupt-closing-of-empty-comment");
                self.state = State::Data;
                self.emit_current_comment();
            }
            _ => {
                self.reconsume();
                self.state = State::Comment;
            }
        }
    }

    fn comment_start_dash_state(&mut self) {
        match self.consume() {
            Some('-') => {
                self.state = State::CommentEnd;
            }
            Some('>') => {
                self.error("abrupt-closing-of-empty-comment");
                self.state = State::Data;
                self.emit_current_comment();
            }
            None => {
                self.error("eof-in-comment");
                self.emit_current_comment();
                self.state = State::Data;
            }
            _ => {
                self.current_comment.push('-');
                self.reconsume();
                self.state = State::Comment;
            }
        }
    }

    fn comment_state(&mut self) {
        match self.consume() {
            Some('<') => {
                self.current_comment.push('<');
                // Skip the comment less-than sign state for simplicity
            }
            Some('-') => {
                self.state = State::CommentEndDash;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.current_comment.push('\u{FFFD}');
            }
            Some(c) => {
                self.current_comment.push(c);
            }
            None => {
                self.error("eof-in-comment");
                self.emit_current_comment();
                self.state = State::Data;
            }
        }
    }

    fn comment_end_dash_state(&mut self) {
        match self.consume() {
            Some('-') => {
                self.state = State::CommentEnd;
            }
            None => {
                self.error("eof-in-comment");
                self.emit_current_comment();
                self.state = State::Data;
            }
            _ => {
                self.current_comment.push('-');
                self.reconsume();
                self.state = State::Comment;
            }
        }
    }

    fn comment_end_state(&mut self) {
        match self.consume() {
            Some('>') => {
                self.state = State::Data;
                self.emit_current_comment();
            }
            Some('!') => {
                self.state = State::CommentEndBang;
            }
            Some('-') => {
                self.current_comment.push('-');
            }
            None => {
                self.error("eof-in-comment");
                self.emit_current_comment();
                self.state = State::Data;
            }
            _ => {
                self.current_comment.push_str("--");
                self.reconsume();
                self.state = State::Comment;
            }
        }
    }

    fn comment_end_bang_state(&mut self) {
        match self.consume() {
            Some('-') => {
                self.current_comment.push_str("--!");
                self.state = State::CommentEndDash;
            }
            Some('>') => {
                self.error("incorrectly-closed-comment");
                self.state = State::Data;
                self.emit_current_comment();
            }
            None => {
                self.error("eof-in-comment");
                self.emit_current_comment();
                self.state = State::Data;
            }
            _ => {
                self.current_comment.push_str("--!");
                self.reconsume();
                self.state = State::Comment;
            }
        }
    }

    fn doctype_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                self.state = State::BeforeDoctypeName;
            }
            Some('>') => {
                self.reconsume();
                self.state = State::BeforeDoctypeName;
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
            _ => {
                self.error("missing-whitespace-before-doctype-name");
                self.reconsume();
                self.state = State::BeforeDoctypeName;
            }
        }
    }

    fn before_doctype_name_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                // Ignore
            }
            Some(c) if c.is_ascii_uppercase() => {
                self.current_doctype_name = c.to_ascii_lowercase().to_string();
                self.state = State::DoctypeName;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.current_doctype_name = "\u{FFFD}".to_string();
                self.state = State::DoctypeName;
            }
            Some('>') => {
                self.error("missing-doctype-name");
                self.current_doctype_force_quirks = true;
                self.state = State::Data;
                self.emit_current_doctype();
            }
            Some(c) => {
                self.current_doctype_name = c.to_string();
                self.state = State::DoctypeName;
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
        }
    }

    fn doctype_name_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                self.state = State::AfterDoctypeName;
            }
            Some('>') => {
                self.state = State::Data;
                self.emit_current_doctype();
            }
            Some(c) if c.is_ascii_uppercase() => {
                self.current_doctype_name.push(c.to_ascii_lowercase());
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                self.current_doctype_name.push('\u{FFFD}');
            }
            Some(c) => {
                self.current_doctype_name.push(c);
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
        }
    }

    fn after_doctype_name_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                // Ignore
            }
            Some('>') => {
                self.state = State::Data;
                self.emit_current_doctype();
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
            _ => {
                self.reconsume();
                if self.consume_if("PUBLIC", true) {
                    self.state = State::AfterDoctypePublicKeyword;
                } else if self.consume_if("SYSTEM", true) {
                    self.state = State::AfterDoctypeSystemKeyword;
                } else {
                    self.error("invalid-character-sequence-after-doctype-name");
                    self.current_doctype_force_quirks = true;
                    self.reconsume();
                    self.state = State::BogusDoctype;
                }
            }
        }
    }

    fn after_doctype_public_keyword_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                self.state = State::BeforeDoctypePublicIdentifier;
            }
            Some('"') => {
                self.error("missing-whitespace-after-doctype-public-keyword");
                self.current_doctype_public_id = Some(String::new());
                self.state = State::DoctypePublicIdentifierDoubleQuoted;
            }
            Some('\'') => {
                self.error("missing-whitespace-after-doctype-public-keyword");
                self.current_doctype_public_id = Some(String::new());
                self.state = State::DoctypePublicIdentifierSingleQuoted;
            }
            Some('>') => {
                self.error("missing-doctype-public-identifier");
                self.current_doctype_force_quirks = true;
                self.state = State::Data;
                self.emit_current_doctype();
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
            _ => {
                self.error("missing-quote-before-doctype-public-identifier");
                self.current_doctype_force_quirks = true;
                self.reconsume();
                self.state = State::BogusDoctype;
            }
        }
    }

    fn before_doctype_public_identifier_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                // Ignore
            }
            Some('"') => {
                self.current_doctype_public_id = Some(String::new());
                self.state = State::DoctypePublicIdentifierDoubleQuoted;
            }
            Some('\'') => {
                self.current_doctype_public_id = Some(String::new());
                self.state = State::DoctypePublicIdentifierSingleQuoted;
            }
            Some('>') => {
                self.error("missing-doctype-public-identifier");
                self.current_doctype_force_quirks = true;
                self.state = State::Data;
                self.emit_current_doctype();
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
            _ => {
                self.error("missing-quote-before-doctype-public-identifier");
                self.current_doctype_force_quirks = true;
                self.reconsume();
                self.state = State::BogusDoctype;
            }
        }
    }

    fn doctype_public_identifier_double_quoted_state(&mut self) {
        match self.consume() {
            Some('"') => {
                self.state = State::AfterDoctypePublicIdentifier;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                if let Some(ref mut id) = self.current_doctype_public_id {
                    id.push('\u{FFFD}');
                }
            }
            Some('>') => {
                self.error("abrupt-doctype-public-identifier");
                self.current_doctype_force_quirks = true;
                self.state = State::Data;
                self.emit_current_doctype();
            }
            Some(c) => {
                if let Some(ref mut id) = self.current_doctype_public_id {
                    id.push(c);
                }
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
        }
    }

    fn doctype_public_identifier_single_quoted_state(&mut self) {
        match self.consume() {
            Some('\'') => {
                self.state = State::AfterDoctypePublicIdentifier;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                if let Some(ref mut id) = self.current_doctype_public_id {
                    id.push('\u{FFFD}');
                }
            }
            Some('>') => {
                self.error("abrupt-doctype-public-identifier");
                self.current_doctype_force_quirks = true;
                self.state = State::Data;
                self.emit_current_doctype();
            }
            Some(c) => {
                if let Some(ref mut id) = self.current_doctype_public_id {
                    id.push(c);
                }
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
        }
    }

    fn after_doctype_public_identifier_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                self.state = State::BetweenDoctypePublicAndSystemIdentifiers;
            }
            Some('>') => {
                self.state = State::Data;
                self.emit_current_doctype();
            }
            Some('"') => {
                self.error("missing-whitespace-between-doctype-public-and-system-identifiers");
                self.current_doctype_system_id = Some(String::new());
                self.state = State::DoctypeSystemIdentifierDoubleQuoted;
            }
            Some('\'') => {
                self.error("missing-whitespace-between-doctype-public-and-system-identifiers");
                self.current_doctype_system_id = Some(String::new());
                self.state = State::DoctypeSystemIdentifierSingleQuoted;
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
            _ => {
                self.error("missing-quote-before-doctype-system-identifier");
                self.current_doctype_force_quirks = true;
                self.reconsume();
                self.state = State::BogusDoctype;
            }
        }
    }

    fn between_doctype_public_and_system_identifiers_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                // Ignore
            }
            Some('>') => {
                self.state = State::Data;
                self.emit_current_doctype();
            }
            Some('"') => {
                self.current_doctype_system_id = Some(String::new());
                self.state = State::DoctypeSystemIdentifierDoubleQuoted;
            }
            Some('\'') => {
                self.current_doctype_system_id = Some(String::new());
                self.state = State::DoctypeSystemIdentifierSingleQuoted;
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
            _ => {
                self.error("missing-quote-before-doctype-system-identifier");
                self.current_doctype_force_quirks = true;
                self.reconsume();
                self.state = State::BogusDoctype;
            }
        }
    }

    fn after_doctype_system_keyword_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                self.state = State::BeforeDoctypeSystemIdentifier;
            }
            Some('"') => {
                self.error("missing-whitespace-after-doctype-system-keyword");
                self.current_doctype_system_id = Some(String::new());
                self.state = State::DoctypeSystemIdentifierDoubleQuoted;
            }
            Some('\'') => {
                self.error("missing-whitespace-after-doctype-system-keyword");
                self.current_doctype_system_id = Some(String::new());
                self.state = State::DoctypeSystemIdentifierSingleQuoted;
            }
            Some('>') => {
                self.error("missing-doctype-system-identifier");
                self.current_doctype_force_quirks = true;
                self.state = State::Data;
                self.emit_current_doctype();
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
            _ => {
                self.error("missing-quote-before-doctype-system-identifier");
                self.current_doctype_force_quirks = true;
                self.reconsume();
                self.state = State::BogusDoctype;
            }
        }
    }

    fn before_doctype_system_identifier_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                // Ignore
            }
            Some('"') => {
                self.current_doctype_system_id = Some(String::new());
                self.state = State::DoctypeSystemIdentifierDoubleQuoted;
            }
            Some('\'') => {
                self.current_doctype_system_id = Some(String::new());
                self.state = State::DoctypeSystemIdentifierSingleQuoted;
            }
            Some('>') => {
                self.error("missing-doctype-system-identifier");
                self.current_doctype_force_quirks = true;
                self.state = State::Data;
                self.emit_current_doctype();
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
            _ => {
                self.error("missing-quote-before-doctype-system-identifier");
                self.current_doctype_force_quirks = true;
                self.reconsume();
                self.state = State::BogusDoctype;
            }
        }
    }

    fn doctype_system_identifier_double_quoted_state(&mut self) {
        match self.consume() {
            Some('"') => {
                self.state = State::AfterDoctypeSystemIdentifier;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                if let Some(ref mut id) = self.current_doctype_system_id {
                    id.push('\u{FFFD}');
                }
            }
            Some('>') => {
                self.error("abrupt-doctype-system-identifier");
                self.current_doctype_force_quirks = true;
                self.state = State::Data;
                self.emit_current_doctype();
            }
            Some(c) => {
                if let Some(ref mut id) = self.current_doctype_system_id {
                    id.push(c);
                }
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
        }
    }

    fn doctype_system_identifier_single_quoted_state(&mut self) {
        match self.consume() {
            Some('\'') => {
                self.state = State::AfterDoctypeSystemIdentifier;
            }
            Some('\0') => {
                self.error("unexpected-null-character");
                if let Some(ref mut id) = self.current_doctype_system_id {
                    id.push('\u{FFFD}');
                }
            }
            Some('>') => {
                self.error("abrupt-doctype-system-identifier");
                self.current_doctype_force_quirks = true;
                self.state = State::Data;
                self.emit_current_doctype();
            }
            Some(c) => {
                if let Some(ref mut id) = self.current_doctype_system_id {
                    id.push(c);
                }
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
        }
    }

    fn after_doctype_system_identifier_state(&mut self) {
        match self.consume() {
            Some('\t') | Some('\n') | Some('\x0C') | Some(' ') => {
                // Ignore
            }
            Some('>') => {
                self.state = State::Data;
                self.emit_current_doctype();
            }
            None => {
                self.error("eof-in-doctype");
                self.current_doctype_force_quirks = true;
                self.emit_current_doctype();
                self.state = State::Data;
            }
            _ => {
                self.error("unexpected-character-after-doctype-system-identifier");
                self.reconsume();
                self.state = State::BogusDoctype;
            }
        }
    }

    fn bogus_doctype_state(&mut self) {
        match self.consume() {
            Some('>') => {
                self.state = State::Data;
                self.emit_current_doctype();
            }
            Some('\0') => {
                self.error("unexpected-null-character");
            }
            Some(_) => {
                // Ignore
            }
            None => {
                self.emit_current_doctype();
                self.state = State::Data;
            }
        }
    }

    fn cdata_section_state(&mut self) {
        match self.consume() {
            Some(']') => {
                self.state = State::CdataSectionBracket;
            }
            Some(c) => {
                self.emit_char(c);
            }
            None => {
                self.error("eof-in-cdata");
                self.state = State::Data;
            }
        }
    }

    fn cdata_section_bracket_state(&mut self) {
        match self.consume() {
            Some(']') => {
                self.state = State::CdataSectionEnd;
            }
            _ => {
                self.emit_char(']');
                self.reconsume();
                self.state = State::CdataSection;
            }
        }
    }

    fn cdata_section_end_state(&mut self) {
        match self.consume() {
            Some(']') => {
                self.emit_char(']');
            }
            Some('>') => {
                self.state = State::Data;
            }
            _ => {
                self.emit_char(']');
                self.emit_char(']');
                self.reconsume();
                self.state = State::CdataSection;
            }
        }
    }

    fn character_reference_state(&mut self) {
        self.temp_buffer.clear();
        self.temp_buffer.push('&');

        match self.consume() {
            Some('#') => {
                self.temp_buffer.push('#');
                self.state = State::NumericCharacterReference;
            }
            Some(c) if c.is_ascii_alphanumeric() => {
                self.reconsume();
                self.state = State::NamedCharacterReference;
            }
            _ => {
                self.flush_temp_buffer_to_return_state();
                self.reconsume();
                self.state = self.return_state;
            }
        }
    }

    fn named_character_reference_state(&mut self) {
        // Collect all alphanumeric characters
        let mut entity_name = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() {
                entity_name.push(c);
                self.consume();
            } else {
                break;
            }
        }

        let has_semicolon = self.peek() == Some(';');

        // Try exact match with semicolon
        if has_semicolon {
            if let Some(decoded) = NAMED_ENTITIES.get(entity_name.as_str()) {
                self.consume(); // consume semicolon
                self.flush_char_ref_to_return_state(decoded);
                self.state = self.return_state;
                return;
            }
        }

        // Try legacy entities
        if LEGACY_ENTITIES.contains(entity_name.as_str()) {
            if let Some(decoded) = NAMED_ENTITIES.get(entity_name.as_str()) {
                // Check if in attribute and followed by alphanumeric or =
                let in_attribute = matches!(
                    self.return_state,
                    State::AttributeValueDoubleQuoted
                        | State::AttributeValueSingleQuoted
                        | State::AttributeValueUnquoted
                );

                if in_attribute {
                    if let Some(next) = self.peek() {
                        if next.is_ascii_alphanumeric() || next == '=' {
                            // Don't decode - emit as-is
                            self.temp_buffer.push_str(&entity_name);
                            self.flush_temp_buffer_to_return_state();
                            self.state = self.return_state;
                            return;
                        }
                    }
                }

                if has_semicolon {
                    self.consume();
                }
                self.flush_char_ref_to_return_state(decoded);
                self.state = self.return_state;
                return;
            }
        }

        // Try prefix match for legacy entities
        let in_attribute = matches!(
            self.return_state,
            State::AttributeValueDoubleQuoted
                | State::AttributeValueSingleQuoted
                | State::AttributeValueUnquoted
        );

        for k in (1..=entity_name.len()).rev() {
            let prefix = &entity_name[..k];
            if LEGACY_ENTITIES.contains(prefix) {
                if let Some(decoded) = NAMED_ENTITIES.get(prefix) {
                    // In attributes, check if character after prefix is alphanumeric or =
                    if in_attribute && k < entity_name.len() {
                        let next_char = entity_name.chars().nth(k).unwrap();
                        if next_char.is_ascii_alphanumeric() || next_char == '=' {
                            // Don't decode - emit as-is
                            continue;
                        }
                    }

                    // Move position back for unmatched chars
                    for _ in 0..(entity_name.len() - k) {
                        self.reconsume();
                    }
                    self.flush_char_ref_to_return_state(decoded);
                    self.state = self.return_state;
                    return;
                }
            }
        }

        // No match - emit as-is
        self.temp_buffer.push_str(&entity_name);
        if has_semicolon {
            self.temp_buffer.push(';');
            self.consume();
        }
        self.flush_temp_buffer_to_return_state();
        self.state = self.return_state;
    }

    fn ambiguous_ampersand_state(&mut self) {
        match self.consume() {
            Some(c) if c.is_ascii_alphanumeric() => {
                self.flush_to_return_state(c);
            }
            Some(';') => {
                self.error("unknown-named-character-reference");
                self.reconsume();
                self.state = self.return_state;
            }
            _ => {
                self.reconsume();
                self.state = self.return_state;
            }
        }
    }

    fn numeric_character_reference_state(&mut self) {
        self.char_ref_code = 0;

        match self.consume() {
            Some('x') | Some('X') => {
                self.temp_buffer.push(self.input[self.pos - 1]);
                self.state = State::HexadecimalCharacterReferenceStart;
            }
            _ => {
                self.reconsume();
                self.state = State::DecimalCharacterReferenceStart;
            }
        }
    }

    fn hexadecimal_character_reference_start_state(&mut self) {
        match self.consume() {
            Some(c) if c.is_ascii_hexdigit() => {
                self.reconsume();
                self.state = State::HexadecimalCharacterReference;
            }
            _ => {
                self.error("absence-of-digits-in-numeric-character-reference");
                self.flush_temp_buffer_to_return_state();
                self.reconsume();
                self.state = self.return_state;
            }
        }
    }

    fn decimal_character_reference_start_state(&mut self) {
        match self.consume() {
            Some(c) if c.is_ascii_digit() => {
                self.reconsume();
                self.state = State::DecimalCharacterReference;
            }
            _ => {
                self.error("absence-of-digits-in-numeric-character-reference");
                self.flush_temp_buffer_to_return_state();
                self.reconsume();
                self.state = self.return_state;
            }
        }
    }

    fn hexadecimal_character_reference_state(&mut self) {
        match self.consume() {
            Some(c) if c.is_ascii_digit() => {
                self.char_ref_code = self.char_ref_code.saturating_mul(16)
                    .saturating_add((c as u32) - 0x30);
            }
            Some(c) if c.is_ascii_uppercase() && c.is_ascii_hexdigit() => {
                self.char_ref_code = self.char_ref_code.saturating_mul(16)
                    .saturating_add((c as u32) - 0x37);
            }
            Some(c) if c.is_ascii_lowercase() && c.is_ascii_hexdigit() => {
                self.char_ref_code = self.char_ref_code.saturating_mul(16)
                    .saturating_add((c as u32) - 0x57);
            }
            Some(';') => {
                self.state = State::NumericCharacterReferenceEnd;
            }
            _ => {
                self.error("missing-semicolon-after-character-reference");
                self.reconsume();
                self.state = State::NumericCharacterReferenceEnd;
            }
        }
    }

    fn decimal_character_reference_state(&mut self) {
        match self.consume() {
            Some(c) if c.is_ascii_digit() => {
                self.char_ref_code = self.char_ref_code.saturating_mul(10)
                    .saturating_add((c as u32) - 0x30);
            }
            Some(';') => {
                self.state = State::NumericCharacterReferenceEnd;
            }
            _ => {
                self.error("missing-semicolon-after-character-reference");
                self.reconsume();
                self.state = State::NumericCharacterReferenceEnd;
            }
        }
    }

    fn numeric_character_reference_end_state(&mut self) {
        let decoded = decode_numeric_entity(&self.char_ref_code.to_string(), false);
        self.flush_char_ref_to_return_state(&decoded);
        self.state = self.return_state;
    }

    // Helper methods for character references
    fn flush_temp_buffer_to_return_state(&mut self) {
        let temp = std::mem::take(&mut self.temp_buffer);
        match self.return_state {
            State::AttributeValueDoubleQuoted
            | State::AttributeValueSingleQuoted
            | State::AttributeValueUnquoted => {
                self.current_attr_value.push_str(&temp);
            }
            _ => {
                self.emit_string(&temp);
            }
        }
    }

    fn flush_char_ref_to_return_state(&mut self, decoded: &str) {
        match self.return_state {
            State::AttributeValueDoubleQuoted
            | State::AttributeValueSingleQuoted
            | State::AttributeValueUnquoted => {
                self.current_attr_value.push_str(decoded);
            }
            _ => {
                self.emit_string(decoded);
            }
        }
    }

    fn flush_to_return_state(&mut self, c: char) {
        match self.return_state {
            State::AttributeValueDoubleQuoted
            | State::AttributeValueSingleQuoted
            | State::AttributeValueUnquoted => {
                self.current_attr_value.push(c);
            }
            _ => {
                self.emit_char(c);
            }
        }
    }
}
