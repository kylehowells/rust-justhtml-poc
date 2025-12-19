//! HTML5 tree construction algorithm

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use crate::node::{Doctype, Namespace, Node, NodeData};
use crate::tokens::{ParseError, Token};
use crate::tokenizer::TokenSink;
use crate::FragmentContext;
use crate::constants::*;

/// Insertion modes for the tree builder
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InsertionMode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    InHeadNoscript,
    AfterHead,
    InBody,
    Text,
    InTable,
    InTableText,
    InCaption,
    InColumnGroup,
    InTableBody,
    InRow,
    InCell,
    InSelect,
    InSelectInTable,
    InTemplate,
    AfterBody,
    InFrameset,
    AfterFrameset,
    AfterAfterBody,
    AfterAfterFrameset,
}

// Tag name sets for fast lookups
static HEADING_TAGS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    ["h1", "h2", "h3", "h4", "h5", "h6"].into_iter().collect()
});

static TABLE_SECTION_TAGS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    ["tbody", "tfoot", "thead"].into_iter().collect()
});

static TABLE_CELL_TAGS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    ["td", "th"].into_iter().collect()
});

static TABLE_CONTEXT_TAGS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    ["table", "template", "html"].into_iter().collect()
});

/// Tree builder that constructs DOM from tokens
pub struct TreeBuilder {
    /// Document root
    document: Node,

    /// Stack of open elements
    open_elements: Vec<Node>,

    /// Active formatting elements (None = marker)
    active_formatting_elements: Vec<Option<Node>>,

    /// Current insertion mode
    insertion_mode: InsertionMode,
    original_insertion_mode: InsertionMode,

    /// Template insertion mode stack
    template_insertion_modes: Vec<InsertionMode>,

    /// Head and body element indices
    head_element_index: Option<usize>,
    body_element_index: Option<usize>,

    /// Form element index
    form_element_index: Option<usize>,

    /// Fragment context
    fragment_context: Option<FragmentContext>,

    /// Flags
    frameset_ok: bool,
    skip_next_newline: bool,
    scripting: bool,
    iframe_srcdoc: bool,
    foster_parenting: bool,

    /// Pending table character tokens
    pending_table_chars: String,

    /// Index where html element should be inserted in document
    html_insert_index: usize,

    /// Errors
    pub errors: Vec<ParseError>,
}

impl TreeBuilder {
    pub fn new(
        fragment_context: Option<&FragmentContext>,
        scripting: bool,
        iframe_srcdoc: bool,
    ) -> Self {
        let is_fragment = fragment_context.is_some();
        let document = if is_fragment {
            Node::document_fragment()
        } else {
            Node::document()
        };

        let mut builder = Self {
            document,
            open_elements: Vec::new(),
            active_formatting_elements: Vec::new(),
            insertion_mode: InsertionMode::Initial,
            original_insertion_mode: InsertionMode::Initial,
            template_insertion_modes: Vec::new(),
            head_element_index: None,
            body_element_index: None,
            form_element_index: None,
            fragment_context: fragment_context.cloned(),
            frameset_ok: true,
            skip_next_newline: false,
            scripting,
            iframe_srcdoc,
            foster_parenting: false,
            pending_table_chars: String::new(),
            html_insert_index: 0,
            errors: Vec::new(),
        };

        if let Some(ctx) = fragment_context {
            builder.setup_fragment_context(ctx);
        }

        builder
    }

    fn setup_fragment_context(&mut self, ctx: &FragmentContext) {
        // Create a dummy HTML element as the root
        let html = Node::element("html", HashMap::new());
        self.open_elements.push(html);

        // Set initial insertion mode based on context
        let mode = match ctx.tag_name.as_str() {
            "title" | "textarea" => InsertionMode::Text,
            "style" | "xmp" | "iframe" | "noembed" | "noframes" => InsertionMode::Text,
            "script" => InsertionMode::Text,
            "noscript" if self.scripting => InsertionMode::Text,
            "plaintext" => InsertionMode::Text,
            "template" => {
                self.template_insertion_modes.push(InsertionMode::InTemplate);
                InsertionMode::InTemplate
            }
            "head" => InsertionMode::InBody,
            "select" => InsertionMode::InSelect,
            "td" | "th" => InsertionMode::InCell,
            "tr" => InsertionMode::InRow,
            "tbody" | "thead" | "tfoot" => InsertionMode::InTableBody,
            "caption" => InsertionMode::InCaption,
            "colgroup" => InsertionMode::InColumnGroup,
            "table" => InsertionMode::InTable,
            "frameset" => InsertionMode::InFrameset,
            "html" => InsertionMode::BeforeHead,
            _ => InsertionMode::InBody,
        };

        self.insertion_mode = mode;
        // For Text mode fragments, set original_insertion_mode to InBody
        // so EOF doesn't trigger Initial mode processing
        if mode == InsertionMode::Text {
            self.original_insertion_mode = InsertionMode::InBody;
        }
    }

    pub fn finish(mut self) -> (Node, Vec<ParseError>) {
        // Pop all elements from the stack, nesting them properly
        while self.open_elements.len() > 1 {
            self.pop_and_add_to_parent();
        }

        // Move the root element to the document
        if let Some(root) = self.open_elements.pop() {
            if self.fragment_context.is_some() {
                // For fragments, the children of html become document-fragment children
                for child in root.children {
                    self.document.children.push(child);
                }
            } else {
                // Insert html at the position recorded when we entered BeforeHtml
                // This preserves the order: doctype, pre-html comments, html, post-html comments
                self.document.children.insert(self.html_insert_index, root);
            }
        }

        (self.document, self.errors)
    }

    fn error(&mut self, code: &str) {
        self.errors.push(ParseError::new(code));
    }

    fn current_node(&self) -> Option<&Node> {
        self.open_elements.last()
    }

    fn current_node_mut(&mut self) -> Option<&mut Node> {
        self.open_elements.last_mut()
    }

    fn adjusted_current_node(&self) -> Option<&Node> {
        if self.fragment_context.is_some() && self.open_elements.len() == 1 {
            // Return context element in fragment case
            self.open_elements.first()
        } else {
            self.current_node()
        }
    }

    fn insert_element(&mut self, name: &str, attrs: HashMap<String, String>) {
        let namespace = self.current_node()
            .and_then(|n| n.namespace)
            .unwrap_or(Namespace::Html);

        // Apply SVG element name adjustments
        let adjusted_name = if namespace == Namespace::Svg {
            SVG_ELEMENT_ADJUSTMENTS.get(name).copied().unwrap_or(name)
        } else {
            name
        };

        let element = Node::element_ns(adjusted_name, namespace, attrs);
        self.open_elements.push(element);
    }

    fn insert_element_for_token(&mut self, name: &str, attrs: HashMap<String, String>, self_closing: bool) {
        self.insert_element(name, attrs);

        // Handle self-closing and void elements
        if self_closing || VOID_ELEMENTS.contains(name) {
            self.pop_and_add_to_parent();
        }
    }

    fn insert_html_element(&mut self, name: &str, attrs: HashMap<String, String>) {
        let mut element = Node::element(name, attrs);

        // Mark for foster parenting if currently in foster parent mode
        if self.foster_parenting {
            element.foster_parented = true;
        }

        self.open_elements.push(element);
    }


    fn insert_character(&mut self, c: char) {
        // Skip leading newline after pre/listing/textarea
        if self.skip_next_newline {
            self.skip_next_newline = false;
            if c == '\n' {
                return;
            }
        }

        let text = c.to_string();

        if self.foster_parenting {
            // Find the foster parent location (before the table)
            if let Some((parent_idx, table_child_idx)) = self.find_foster_parent_location() {
                let parent = &mut self.open_elements[parent_idx];
                // Try to append to existing text node before the table
                if table_child_idx > 0 {
                    if let Some(prev_child) = parent.children.get_mut(table_child_idx - 1) {
                        if let Some(NodeData::Text(ref mut existing)) = prev_child.data {
                            existing.push(c);
                            return;
                        }
                    }
                }
                // Insert new text node before table
                parent.children.insert(table_child_idx, Node::text(&text));
            }
            return;
        }

        // Try to append to existing text node
        if let Some(current) = self.open_elements.last_mut() {
            if let Some(last_child) = current.children.last_mut() {
                if let Some(NodeData::Text(ref mut existing)) = last_child.data {
                    existing.push(c);
                    return;
                }
            }
            // Create new text node
            current.children.push(Node::text(&text));
        }
    }

    /// Find the foster parent location: returns (parent_index, child_index_of_table)
    /// where we should insert before the table.
    fn find_foster_parent_location(&self) -> Option<(usize, usize)> {
        // Find the last table element in the stack
        for i in (0..self.open_elements.len()).rev() {
            if self.open_elements[i].name == "table" {
                // The foster parent is the element before the table in the stack
                if i > 0 {
                    let parent_idx = i - 1;
                    let parent = &self.open_elements[parent_idx];
                    // Find where the table is in the parent's children
                    // Since table might not be added yet, use children.len()
                    return Some((parent_idx, parent.children.len()));
                }
            }
        }
        None
    }

    fn insert_comment(&mut self, data: &str) {
        let comment = Node::comment(data);
        if let Some(current) = self.open_elements.last_mut() {
            current.children.push(comment);
        } else {
            self.document.children.push(comment);
        }
    }

    fn pop_current_element(&mut self) -> Option<Node> {
        self.open_elements.pop()
    }

    fn pop_elements_until(&mut self, tag_name: &str) {
        while let Some(node) = self.open_elements.last() {
            let name = node.name.clone();
            self.pop_and_add_to_parent();
            if name == tag_name {
                break;
            }
        }
    }

    fn pop_elements_until_one_of(&mut self, tags: &[&str]) {
        while let Some(node) = self.open_elements.last() {
            let name = node.name.clone();
            self.pop_and_add_to_parent();
            if tags.contains(&name.as_str()) {
                break;
            }
        }
    }

    fn pop_and_add_to_parent(&mut self) {
        if let Some(mut node) = self.open_elements.pop() {
            // Skip adding to parent if already parented (e.g., elements inserted during head reinsertion)
            if node.is_parented {
                return;
            }

            // Handle foster parenting: insert before the table
            if node.foster_parented {
                node.foster_parented = false; // Clear the flag
                if let Some((parent_idx, insert_idx)) = self.find_foster_parent_location() {
                    self.open_elements[parent_idx].children.insert(insert_idx, node);
                    return;
                }
            }

            if let Some(parent) = self.open_elements.last_mut() {
                parent.children.push(node);
            } else {
                self.document.children.push(node);
            }
        }
    }

    fn has_element_in_scope(&self, tag_name: &str) -> bool {
        self.has_element_in_scope_with(&SCOPE_ELEMENTS, tag_name)
    }

    fn has_element_in_scope_with(&self, scope_elements: &HashSet<&str>, tag_name: &str) -> bool {
        for node in self.open_elements.iter().rev() {
            if node.name == tag_name {
                return true;
            }
            if scope_elements.contains(node.name.as_str()) {
                return false;
            }
        }
        false
    }

    fn has_element_in_button_scope(&self, tag_name: &str) -> bool {
        self.has_element_in_scope_with(&BUTTON_SCOPE_ELEMENTS, tag_name)
    }

    fn has_element_in_list_item_scope(&self, tag_name: &str) -> bool {
        self.has_element_in_scope_with(&LIST_ITEM_SCOPE_ELEMENTS, tag_name)
    }

    fn has_element_in_table_scope(&self, tag_name: &str) -> bool {
        self.has_element_in_scope_with(&TABLE_SCOPE_ELEMENTS, tag_name)
    }

    fn has_element_in_select_scope(&self, tag_name: &str) -> bool {
        for node in self.open_elements.iter().rev() {
            if node.name == tag_name {
                return true;
            }
            if node.name != "optgroup" && node.name != "option" {
                return false;
            }
        }
        false
    }

    fn generate_implied_end_tags(&mut self) {
        self.generate_implied_end_tags_except(None);
    }

    fn generate_implied_end_tags_except(&mut self, except: Option<&str>) {
        loop {
            if let Some(current) = self.open_elements.last() {
                if IMPLIED_END_TAGS.contains(current.name.as_str()) {
                    if except.map_or(true, |e| current.name != e) {
                        self.pop_and_add_to_parent();
                        continue;
                    }
                }
            }
            break;
        }
    }

    fn close_p_element(&mut self) {
        self.generate_implied_end_tags_except(Some("p"));
        if self.open_elements.last().map_or(false, |n| n.name != "p") {
            self.error("expected-closing-tag-but-got-another");
        }
        self.pop_elements_until("p");
    }

    fn reconstruct_active_formatting_elements(&mut self) {
        // Simplified implementation - full adoption agency algorithm is complex
        if self.active_formatting_elements.is_empty() {
            return;
        }

        // Find last marker or beginning
        let mut entry_index = self.active_formatting_elements.len();
        for (i, entry) in self.active_formatting_elements.iter().enumerate().rev() {
            if entry.is_none() {
                entry_index = i + 1;
                break;
            }
            // Check if element is in open elements
            if let Some(ref node) = entry {
                if self.open_elements.iter().any(|n| n.name == node.name) {
                    entry_index = i + 1;
                    break;
                }
            }
        }

        // Reconstruct from entry_index
        while entry_index < self.active_formatting_elements.len() {
            if let Some(Some(ref entry)) = self.active_formatting_elements.get(entry_index) {
                let element = Node::element(&entry.name, entry.attrs.clone());
                self.open_elements.push(element);
            }
            entry_index += 1;
        }
    }

    fn clear_active_formatting_to_marker(&mut self) {
        while let Some(entry) = self.active_formatting_elements.pop() {
            if entry.is_none() {
                break;
            }
        }
    }

    fn push_active_formatting_element(&mut self, name: &str, attrs: HashMap<String, String>) {
        let element = Node::element(name, attrs);
        self.active_formatting_elements.push(Some(element));
    }

    fn push_formatting_marker(&mut self) {
        self.active_formatting_elements.push(None);
    }

    fn has_active_formatting_entry(&self, name: &str) -> bool {
        for entry in self.active_formatting_elements.iter().rev() {
            match entry {
                None => return false, // Hit marker
                Some(node) if node.name == name => return true,
                _ => continue,
            }
        }
        false
    }

    fn adoption_agency(&mut self, name: &str) {
        // Step 1: If current node is the subject and not in active formatting, just pop it
        if let Some(current) = self.current_node() {
            if current.name == name && !self.has_active_formatting_entry(name) {
                self.pop_elements_until(name);
                return;
            }
        }

        // Step 2: Outer loop (max 8 iterations)
        for _ in 0..8 {
            // Step 3: Find formatting element in active formatting list
            let mut fe_active_idx: Option<usize> = None;
            for i in (0..self.active_formatting_elements.len()).rev() {
                match &self.active_formatting_elements[i] {
                    None => break, // Hit marker
                    Some(node) if node.name == name => {
                        fe_active_idx = Some(i);
                        break;
                    }
                    _ => continue,
                }
            }

            let Some(fe_active_idx) = fe_active_idx else {
                // No formatting element found - use any other end tag handling
                self.any_other_end_tag(name);
                return;
            };

            // Step 4: Find formatting element in open elements
            let fe_stack_idx = self.open_elements.iter().rposition(|n| n.name == name);

            let Some(fe_stack_idx) = fe_stack_idx else {
                // Formatting element not in open elements - remove from active formatting
                self.error("adoption-agency-1.3");
                self.active_formatting_elements.remove(fe_active_idx);
                return;
            };

            // Step 5: Check if formatting element is in scope
            if !self.has_element_in_scope(name) {
                self.error("adoption-agency-1.3");
                return;
            }

            // Step 6: If formatting element is not current node, emit error
            if fe_stack_idx != self.open_elements.len() - 1 {
                self.error("adoption-agency-1.3");
            }

            // Step 7: Find furthest block (first special element after formatting element)
            let mut furthest_block_idx: Option<usize> = None;
            for i in (fe_stack_idx + 1)..self.open_elements.len() {
                if SPECIAL_ELEMENTS.contains(self.open_elements[i].name.as_str()) {
                    furthest_block_idx = Some(i);
                    break;
                }
            }

            // Step 8: If no furthest block, pop to formatting element and remove from active formatting
            let Some(_fb_idx) = furthest_block_idx else {
                while self.open_elements.len() > fe_stack_idx {
                    self.pop_and_add_to_parent();
                }
                self.active_formatting_elements.remove(fe_active_idx);
                return;
            };

            // For the complex case with furthest block, we use a simplified approach:
            // Just pop elements and remove from active formatting.
            // This doesn't produce perfect output for complex adoption agency cases,
            // but handles the common cases.
            self.generate_implied_end_tags();
            while self.open_elements.len() > fe_stack_idx {
                self.pop_and_add_to_parent();
            }
            self.active_formatting_elements.remove(fe_active_idx);
            return;
        }
    }

    fn any_other_end_tag(&mut self, name: &str) {
        for i in (0..self.open_elements.len()).rev() {
            if self.open_elements[i].name == name {
                self.generate_implied_end_tags_except(Some(name));
                while self.open_elements.len() > i {
                    self.pop_and_add_to_parent();
                }
                break;
            }
            if SPECIAL_ELEMENTS.contains(self.open_elements[i].name.as_str()) {
                self.error("unexpected-end-tag");
                break;
            }
        }
    }
}

impl TokenSink for TreeBuilder {
    fn process_token(&mut self, token: Token) {
        match token {
            Token::Doctype(doctype) => self.process_doctype(doctype),
            Token::StartTag { name, attrs, self_closing } => {
                self.process_start_tag(&name, attrs, self_closing);
            }
            Token::EndTag { name } => {
                self.process_end_tag(&name);
            }
            Token::Character(c) => {
                self.process_character(c);
            }
            Token::Comment(data) => {
                self.process_comment(&data);
            }
            Token::Eof => {
                self.process_eof();
            }
        }
    }

    fn current_namespace(&self) -> Option<Namespace> {
        self.current_node().and_then(|n| n.namespace)
    }
}

impl TreeBuilder {
    fn process_doctype(&mut self, doctype: Doctype) {
        match self.insertion_mode {
            InsertionMode::Initial => {
                let node = Node::doctype(doctype);
                self.document.children.push(node);
                self.insertion_mode = InsertionMode::BeforeHtml;
            }
            _ => {
                // Ignore spurious doctypes
            }
        }
    }

    fn process_start_tag(&mut self, name: &str, attrs: HashMap<String, String>, self_closing: bool) {
        match self.insertion_mode {
            InsertionMode::Initial => {
                self.insertion_mode = InsertionMode::BeforeHtml;
                self.process_start_tag(name, attrs, self_closing);
            }
            InsertionMode::BeforeHtml => {
                // Record where html should be inserted (after any pre-html content)
                self.html_insert_index = self.document.children.len();
                if name == "html" {
                    self.insert_html_element(name, attrs);
                    self.insertion_mode = InsertionMode::BeforeHead;
                } else {
                    self.insert_html_element("html", HashMap::new());
                    self.insertion_mode = InsertionMode::BeforeHead;
                    self.process_start_tag(name, attrs, self_closing);
                }
            }
            InsertionMode::BeforeHead => {
                match name {
                    "html" => {
                        // Add attributes to existing html element
                        if let Some(html) = self.open_elements.first_mut() {
                            for (k, v) in attrs {
                                if !html.attrs.contains_key(&k) {
                                    html.attrs.insert(k, v);
                                }
                            }
                        }
                    }
                    "head" => {
                        self.insert_html_element(name, attrs);
                        self.head_element_index = Some(self.open_elements.len() - 1);
                        self.insertion_mode = InsertionMode::InHead;
                    }
                    _ => {
                        self.insert_html_element("head", HashMap::new());
                        self.head_element_index = Some(self.open_elements.len() - 1);
                        self.insertion_mode = InsertionMode::InHead;
                        self.process_start_tag(name, attrs, self_closing);
                    }
                }
            }
            InsertionMode::InHead => {
                self.process_in_head_start_tag(name, attrs, self_closing);
            }
            InsertionMode::InHeadNoscript => {
                match name {
                    "html" => {
                        self.process_in_body_start_tag(name, attrs, self_closing);
                    }
                    "basefont" | "bgsound" | "link" | "meta" | "noframes" | "style" => {
                        self.process_in_head_start_tag(name, attrs, self_closing);
                    }
                    "head" | "noscript" => {
                        self.error("unexpected-start-tag-in-head-noscript");
                    }
                    _ => {
                        self.error("unexpected-start-tag-in-head-noscript");
                        self.pop_and_add_to_parent(); // Pop noscript
                        self.insertion_mode = InsertionMode::InHead;
                        self.process_start_tag(name, attrs, self_closing);
                    }
                }
            }
            InsertionMode::AfterHead => {
                match name {
                    "html" => {
                        self.process_in_body_start_tag(name, attrs, self_closing);
                    }
                    "body" => {
                        self.insert_html_element(name, attrs);
                        self.frameset_ok = false;
                        self.body_element_index = Some(self.open_elements.len() - 1);
                        self.insertion_mode = InsertionMode::InBody;
                    }
                    "frameset" => {
                        self.insert_html_element(name, attrs);
                        self.insertion_mode = InsertionMode::InFrameset;
                    }
                    "base" | "basefont" | "bgsound" | "link" | "meta" |
                    "noframes" | "script" | "style" | "template" | "title" => {
                        self.error("unexpected-start-tag-after-head");
                        // Push head back onto stack temporarily (per WHATWG spec)
                        if let Some(html) = self.open_elements.first_mut() {
                            if let Some(head_idx) = html.children.iter().position(|c| c.name == "head") {
                                let head = html.children.remove(head_idx);
                                self.open_elements.push(head);

                                // Save original_insertion_mode so Text mode returns to AfterHead
                                let saved_original_mode = self.original_insertion_mode;
                                self.original_insertion_mode = InsertionMode::AfterHead;

                                self.insertion_mode = InsertionMode::InHead;
                                self.process_start_tag(name, attrs, self_closing);

                                // Remove head from stack (it might not be at the top)
                                // Elements inserted after head need to be recorded as children of head
                                if let Some(head_pos) = self.open_elements.iter().position(|n| n.name == "head") {
                                    // Remove head and elements after it
                                    let mut removed: Vec<Node> = self.open_elements.drain(head_pos..).collect();

                                    // First element is head, rest are elements inserted after head
                                    let mut head = removed.remove(0);
                                    let mut elements_after: Vec<Node> = removed;

                                    // Add elements as children of head and mark them as parented
                                    for elem in &elements_after {
                                        let mut child = elem.clone();
                                        child.is_parented = true;
                                        head.children.push(child);
                                    }

                                    // Put head back in html.children
                                    if let Some(html) = self.open_elements.first_mut() {
                                        html.children.insert(head_idx, head);
                                    }

                                    // Put elements back on the stack (they're still open)
                                    // Mark them as already parented so pop_and_add_to_parent skips them
                                    for elem in &mut elements_after {
                                        elem.is_parented = true;
                                    }
                                    for elem in elements_after {
                                        self.open_elements.push(elem);
                                    }
                                }
                                // Don't restore original_insertion_mode - we want Text to return to AfterHead
                            }
                        }
                    }
                    "head" => {
                        self.error("unexpected-start-tag-after-head");
                    }
                    _ => {
                        self.insert_html_element("body", HashMap::new());
                        self.body_element_index = Some(self.open_elements.len() - 1);
                        self.insertion_mode = InsertionMode::InBody;
                        self.process_start_tag(name, attrs, self_closing);
                    }
                }
            }
            InsertionMode::InBody => {
                self.process_in_body_start_tag(name, attrs, self_closing);
            }
            InsertionMode::Text => {
                // Should not receive start tags in text mode
            }
            InsertionMode::InTable => {
                self.process_in_table_start_tag(name, attrs, self_closing);
            }
            InsertionMode::InTableText => {
                self.flush_table_text();
                self.insertion_mode = InsertionMode::InTable;
                self.process_start_tag(name, attrs, self_closing);
            }
            InsertionMode::InCaption => {
                match name {
                    "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr" => {
                        if self.has_element_in_table_scope("caption") {
                            self.generate_implied_end_tags();
                            self.pop_elements_until("caption");
                            self.clear_active_formatting_to_marker();
                            self.insertion_mode = InsertionMode::InTable;
                            self.process_start_tag(name, attrs, self_closing);
                        }
                    }
                    _ => {
                        self.process_in_body_start_tag(name, attrs, self_closing);
                    }
                }
            }
            InsertionMode::InColumnGroup => {
                match name {
                    "html" => {
                        self.process_in_body_start_tag(name, attrs, self_closing);
                    }
                    "col" => {
                        self.insert_element_for_token(name, attrs, true);
                    }
                    "template" => {
                        self.process_in_head_start_tag(name, attrs, self_closing);
                    }
                    _ => {
                        if self.open_elements.last().map_or(false, |n| n.name == "colgroup") {
                            self.pop_and_add_to_parent();
                            self.insertion_mode = InsertionMode::InTable;
                            self.process_start_tag(name, attrs, self_closing);
                        }
                    }
                }
            }
            InsertionMode::InTableBody => {
                match name {
                    "tr" => {
                        self.clear_stack_to_table_body_context();
                        self.insert_html_element(name, attrs);
                        self.insertion_mode = InsertionMode::InRow;
                    }
                    "th" | "td" => {
                        self.error("unexpected-cell-in-table-body");
                        self.clear_stack_to_table_body_context();
                        self.insert_html_element("tr", HashMap::new());
                        self.insertion_mode = InsertionMode::InRow;
                        self.process_start_tag(name, attrs, self_closing);
                    }
                    "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead" => {
                        if self.has_element_in_table_scope("tbody") ||
                           self.has_element_in_table_scope("thead") ||
                           self.has_element_in_table_scope("tfoot") {
                            self.clear_stack_to_table_body_context();
                            self.pop_and_add_to_parent();
                            self.insertion_mode = InsertionMode::InTable;
                            self.process_start_tag(name, attrs, self_closing);
                        }
                    }
                    _ => {
                        self.process_in_table_start_tag(name, attrs, self_closing);
                    }
                }
            }
            InsertionMode::InRow => {
                match name {
                    "th" | "td" => {
                        self.clear_stack_to_table_row_context();
                        self.insert_html_element(name, attrs);
                        self.insertion_mode = InsertionMode::InCell;
                        self.push_formatting_marker();
                    }
                    "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead" | "tr" => {
                        if self.has_element_in_table_scope("tr") {
                            self.clear_stack_to_table_row_context();
                            self.pop_and_add_to_parent();
                            self.insertion_mode = InsertionMode::InTableBody;
                            self.process_start_tag(name, attrs, self_closing);
                        }
                    }
                    _ => {
                        self.process_in_table_start_tag(name, attrs, self_closing);
                    }
                }
            }
            InsertionMode::InCell => {
                match name {
                    "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr" => {
                        if self.has_element_in_table_scope("td") || self.has_element_in_table_scope("th") {
                            self.close_cell();
                            self.process_start_tag(name, attrs, self_closing);
                        }
                    }
                    _ => {
                        self.process_in_body_start_tag(name, attrs, self_closing);
                    }
                }
            }
            InsertionMode::InSelect => {
                self.process_in_select_start_tag(name, attrs, self_closing);
            }
            InsertionMode::InSelectInTable => {
                match name {
                    "caption" | "table" | "tbody" | "tfoot" | "thead" | "tr" | "td" | "th" => {
                        self.error("unexpected-table-element-in-select");
                        self.pop_elements_until("select");
                        self.reset_insertion_mode();
                        self.process_start_tag(name, attrs, self_closing);
                    }
                    _ => {
                        self.process_in_select_start_tag(name, attrs, self_closing);
                    }
                }
            }
            InsertionMode::InTemplate => {
                match name {
                    "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" |
                    "script" | "style" | "template" | "title" => {
                        self.process_in_head_start_tag(name, attrs, self_closing);
                    }
                    "caption" | "colgroup" | "tbody" | "tfoot" | "thead" => {
                        self.template_insertion_modes.pop();
                        self.template_insertion_modes.push(InsertionMode::InTable);
                        self.insertion_mode = InsertionMode::InTable;
                        self.process_start_tag(name, attrs, self_closing);
                    }
                    "col" => {
                        self.template_insertion_modes.pop();
                        self.template_insertion_modes.push(InsertionMode::InColumnGroup);
                        self.insertion_mode = InsertionMode::InColumnGroup;
                        self.process_start_tag(name, attrs, self_closing);
                    }
                    "tr" => {
                        self.template_insertion_modes.pop();
                        self.template_insertion_modes.push(InsertionMode::InTableBody);
                        self.insertion_mode = InsertionMode::InTableBody;
                        self.process_start_tag(name, attrs, self_closing);
                    }
                    "td" | "th" => {
                        self.template_insertion_modes.pop();
                        self.template_insertion_modes.push(InsertionMode::InRow);
                        self.insertion_mode = InsertionMode::InRow;
                        self.process_start_tag(name, attrs, self_closing);
                    }
                    _ => {
                        self.template_insertion_modes.pop();
                        self.template_insertion_modes.push(InsertionMode::InBody);
                        self.insertion_mode = InsertionMode::InBody;
                        self.process_start_tag(name, attrs, self_closing);
                    }
                }
            }
            InsertionMode::AfterBody => {
                match name {
                    "html" => {
                        self.process_in_body_start_tag(name, attrs, self_closing);
                    }
                    _ => {
                        self.error("unexpected-start-tag-after-body");
                        self.insertion_mode = InsertionMode::InBody;
                        self.process_start_tag(name, attrs, self_closing);
                    }
                }
            }
            InsertionMode::InFrameset => {
                match name {
                    "html" => {
                        self.process_in_body_start_tag(name, attrs, self_closing);
                    }
                    "frameset" => {
                        self.insert_html_element(name, attrs);
                    }
                    "frame" => {
                        self.insert_element_for_token(name, attrs, true);
                    }
                    "noframes" => {
                        self.process_in_head_start_tag(name, attrs, self_closing);
                    }
                    _ => {
                        self.error("unexpected-start-tag-in-frameset");
                    }
                }
            }
            InsertionMode::AfterFrameset => {
                match name {
                    "html" => {
                        self.process_in_body_start_tag(name, attrs, self_closing);
                    }
                    "noframes" => {
                        self.process_in_head_start_tag(name, attrs, self_closing);
                    }
                    _ => {
                        self.error("unexpected-start-tag-after-frameset");
                    }
                }
            }
            InsertionMode::AfterAfterBody => {
                match name {
                    "html" => {
                        self.process_in_body_start_tag(name, attrs, self_closing);
                    }
                    _ => {
                        self.error("unexpected-start-tag-after-body");
                        self.insertion_mode = InsertionMode::InBody;
                        self.process_start_tag(name, attrs, self_closing);
                    }
                }
            }
            InsertionMode::AfterAfterFrameset => {
                match name {
                    "html" => {
                        self.process_in_body_start_tag(name, attrs, self_closing);
                    }
                    "noframes" => {
                        self.process_in_head_start_tag(name, attrs, self_closing);
                    }
                    _ => {
                        self.error("unexpected-start-tag-after-frameset");
                    }
                }
            }
        }
    }

    fn process_in_head_start_tag(&mut self, name: &str, attrs: HashMap<String, String>, self_closing: bool) {
        match name {
            "html" => {
                self.process_in_body_start_tag(name, attrs, self_closing);
            }
            "base" | "basefont" | "bgsound" | "link" => {
                self.insert_element_for_token(name, attrs, true);
            }
            "meta" => {
                self.insert_element_for_token(name, attrs, true);
            }
            "title" => {
                self.insert_html_element(name, attrs);
                self.original_insertion_mode = self.insertion_mode;
                self.insertion_mode = InsertionMode::Text;
            }
            "noscript" if self.scripting => {
                self.insert_html_element(name, attrs);
                self.original_insertion_mode = self.insertion_mode;
                self.insertion_mode = InsertionMode::Text;
            }
            "noframes" | "style" => {
                self.insert_html_element(name, attrs);
                self.original_insertion_mode = self.insertion_mode;
                self.insertion_mode = InsertionMode::Text;
            }
            "noscript" => {
                self.insert_html_element(name, attrs);
                self.insertion_mode = InsertionMode::InHeadNoscript;
            }
            "script" => {
                self.insert_html_element(name, attrs);
                self.original_insertion_mode = self.insertion_mode;
                self.insertion_mode = InsertionMode::Text;
            }
            "template" => {
                self.insert_html_element(name, attrs);
                self.push_formatting_marker();
                self.frameset_ok = false;
                self.insertion_mode = InsertionMode::InTemplate;
                self.template_insertion_modes.push(InsertionMode::InTemplate);
            }
            "head" => {
                self.error("unexpected-start-tag-in-head");
            }
            _ => {
                self.pop_and_add_to_parent(); // Pop head
                self.insertion_mode = InsertionMode::AfterHead;
                self.process_start_tag(name, attrs, self_closing);
            }
        }
    }

    fn process_in_body_start_tag(&mut self, name: &str, attrs: HashMap<String, String>, self_closing: bool) {
        match name {
            "html" => {
                self.error("unexpected-html-element-in-body");
                if let Some(html) = self.open_elements.first_mut() {
                    for (k, v) in attrs {
                        if !html.attrs.contains_key(&k) {
                            html.attrs.insert(k, v);
                        }
                    }
                }
            }
            "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" |
            "script" | "style" | "template" | "title" => {
                self.process_in_head_start_tag(name, attrs, self_closing);
            }
            "body" => {
                self.error("unexpected-body-element");
                if let Some(body) = self.open_elements.get_mut(1) {
                    if body.name == "body" {
                        for (k, v) in attrs {
                            if !body.attrs.contains_key(&k) {
                                body.attrs.insert(k, v);
                            }
                        }
                    }
                }
            }
            "frameset" => {
                self.error("unexpected-frameset-in-body");
            }
            "address" | "article" | "aside" | "blockquote" | "center" | "details" |
            "dialog" | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" |
            "footer" | "header" | "hgroup" | "main" | "menu" | "nav" | "ol" |
            "p" | "search" | "section" | "summary" | "ul" => {
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                self.insert_html_element(name, attrs);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                if let Some(current) = self.current_node() {
                    if HEADING_TAGS.contains(current.name.as_str()) {
                        self.error("unexpected-heading-in-heading");
                        self.pop_and_add_to_parent();
                    }
                }
                self.insert_html_element(name, attrs);
            }
            "pre" | "listing" => {
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                self.insert_html_element(name, attrs);
                self.skip_next_newline = true;
                self.frameset_ok = false;
            }
            "form" => {
                if self.form_element_index.is_some() && !self.has_element_in_scope("template") {
                    self.error("unexpected-form-element");
                } else {
                    if self.has_element_in_button_scope("p") {
                        self.close_p_element();
                    }
                    self.insert_html_element(name, attrs);
                    if !self.has_element_in_scope("template") {
                        self.form_element_index = Some(self.open_elements.len() - 1);
                    }
                }
            }
            "li" => {
                self.frameset_ok = false;
                // Close any open li elements
                for i in (0..self.open_elements.len()).rev() {
                    if self.open_elements[i].name == "li" {
                        self.generate_implied_end_tags_except(Some("li"));
                        self.pop_elements_until("li");
                        break;
                    }
                    if SPECIAL_ELEMENTS.contains(self.open_elements[i].name.as_str()) &&
                       !["address", "div", "p"].contains(&self.open_elements[i].name.as_str()) {
                        break;
                    }
                }
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                self.insert_html_element(name, attrs);
            }
            "dd" | "dt" => {
                self.frameset_ok = false;
                let mut found_tag: Option<String> = None;
                for i in (0..self.open_elements.len()).rev() {
                    let node_name = &self.open_elements[i].name;
                    if node_name == "dd" || node_name == "dt" {
                        found_tag = Some(node_name.clone());
                        break;
                    }
                    if SPECIAL_ELEMENTS.contains(node_name.as_str()) &&
                       !["address", "div", "p"].contains(&node_name.as_str()) {
                        break;
                    }
                }
                if let Some(tag) = found_tag {
                    self.generate_implied_end_tags_except(Some(&tag));
                    self.pop_elements_until(&tag);
                }
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                self.insert_html_element(name, attrs);
            }
            "plaintext" => {
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                self.insert_html_element(name, attrs);
                // Tokenizer should switch to plaintext state
            }
            "button" => {
                if self.has_element_in_scope("button") {
                    self.error("unexpected-button-in-button");
                    self.generate_implied_end_tags();
                    self.pop_elements_until("button");
                }
                self.reconstruct_active_formatting_elements();
                self.insert_html_element(name, attrs);
                self.frameset_ok = false;
            }
            "a" => {
                // Check for existing a element
                let has_a = self.active_formatting_elements.iter().rev()
                    .take_while(|e| e.is_some())
                    .any(|e| e.as_ref().map_or(false, |n| n.name == "a"));

                if has_a {
                    self.error("unexpected-anchor-in-anchor");
                    // Run adoption agency (simplified)
                    self.pop_elements_until("a");
                }

                self.reconstruct_active_formatting_elements();
                self.insert_html_element(name, attrs.clone());
                self.push_active_formatting_element(name, attrs);
            }
            "b" | "big" | "code" | "em" | "font" | "i" | "s" | "small" |
            "strike" | "strong" | "tt" | "u" => {
                self.reconstruct_active_formatting_elements();
                self.insert_html_element(name, attrs.clone());
                self.push_active_formatting_element(name, attrs);
            }
            "nobr" => {
                self.reconstruct_active_formatting_elements();
                if self.has_element_in_scope("nobr") {
                    self.error("unexpected-nobr-in-nobr");
                    self.pop_elements_until("nobr");
                    self.reconstruct_active_formatting_elements();
                }
                self.insert_html_element(name, attrs.clone());
                self.push_active_formatting_element(name, attrs);
            }
            "applet" | "marquee" | "object" => {
                self.reconstruct_active_formatting_elements();
                self.insert_html_element(name, attrs);
                self.push_formatting_marker();
                self.frameset_ok = false;
            }
            "table" => {
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                self.insert_html_element(name, attrs);
                self.frameset_ok = false;
                self.insertion_mode = InsertionMode::InTable;
            }
            "area" | "br" | "embed" | "img" | "keygen" | "wbr" => {
                self.reconstruct_active_formatting_elements();
                self.insert_element_for_token(name, attrs, true);
                self.frameset_ok = false;
            }
            "input" => {
                self.reconstruct_active_formatting_elements();
                self.insert_element_for_token(name, attrs.clone(), true);
                let is_hidden = attrs.get("type")
                    .map_or(false, |t| t.eq_ignore_ascii_case("hidden"));
                if !is_hidden {
                    self.frameset_ok = false;
                }
            }
            "param" | "source" | "track" => {
                self.insert_element_for_token(name, attrs, true);
            }
            "hr" => {
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                self.insert_element_for_token(name, attrs, true);
                self.frameset_ok = false;
            }
            "image" => {
                self.error("unexpected-image-tag");
                self.process_start_tag("img", attrs, self_closing);
            }
            "textarea" => {
                self.insert_html_element(name, attrs);
                self.skip_next_newline = true;
                self.original_insertion_mode = self.insertion_mode;
                self.frameset_ok = false;
                self.insertion_mode = InsertionMode::Text;
            }
            "xmp" => {
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                self.reconstruct_active_formatting_elements();
                self.frameset_ok = false;
                self.insert_html_element(name, attrs);
                self.original_insertion_mode = self.insertion_mode;
                self.insertion_mode = InsertionMode::Text;
            }
            "iframe" => {
                self.frameset_ok = false;
                self.insert_html_element(name, attrs);
                self.original_insertion_mode = self.insertion_mode;
                self.insertion_mode = InsertionMode::Text;
            }
            "noembed" => {
                self.insert_html_element(name, attrs);
                self.original_insertion_mode = self.insertion_mode;
                self.insertion_mode = InsertionMode::Text;
            }
            "noscript" if self.scripting => {
                self.insert_html_element(name, attrs);
                self.original_insertion_mode = self.insertion_mode;
                self.insertion_mode = InsertionMode::Text;
            }
            "select" => {
                self.reconstruct_active_formatting_elements();
                self.insert_html_element(name, attrs);
                self.frameset_ok = false;
                match self.insertion_mode {
                    InsertionMode::InTable | InsertionMode::InCaption |
                    InsertionMode::InTableBody | InsertionMode::InRow |
                    InsertionMode::InCell => {
                        self.insertion_mode = InsertionMode::InSelectInTable;
                    }
                    _ => {
                        self.insertion_mode = InsertionMode::InSelect;
                    }
                }
            }
            "optgroup" | "option" => {
                if self.current_node().map_or(false, |n| n.name == "option") {
                    self.pop_and_add_to_parent();
                }
                self.reconstruct_active_formatting_elements();
                self.insert_html_element(name, attrs);
            }
            "rb" | "rtc" => {
                if self.has_element_in_scope("ruby") {
                    self.generate_implied_end_tags();
                }
                self.insert_html_element(name, attrs);
            }
            "rp" | "rt" => {
                if self.has_element_in_scope("ruby") {
                    self.generate_implied_end_tags_except(Some("rtc"));
                }
                self.insert_html_element(name, attrs);
            }
            "math" => {
                self.reconstruct_active_formatting_elements();
                // Create MathML element
                let element = Node::element_ns(name, Namespace::MathML, attrs);
                self.open_elements.push(element);
                if self_closing {
                    self.pop_and_add_to_parent();
                }
            }
            "svg" => {
                self.reconstruct_active_formatting_elements();
                // Create SVG element
                let element = Node::element_ns(name, Namespace::Svg, attrs);
                self.open_elements.push(element);
                if self_closing {
                    self.pop_and_add_to_parent();
                }
            }
            "caption" | "col" | "colgroup" | "frame" | "head" |
            "tbody" | "td" | "tfoot" | "th" | "thead" | "tr" => {
                self.error("unexpected-table-element-in-body");
            }
            _ => {
                self.reconstruct_active_formatting_elements();
                // Use insert_element to inherit namespace from parent (for foreign content)
                if self.is_in_foreign_content() {
                    self.insert_element(name, attrs);
                    if self_closing {
                        self.pop_and_add_to_parent();
                    }
                } else {
                    self.insert_html_element(name, attrs);
                }
            }
        }
    }

    fn is_in_foreign_content(&self) -> bool {
        self.current_node()
            .and_then(|n| n.namespace)
            .map_or(false, |ns| ns != Namespace::Html)
    }

    /// Process an end tag while in foreign content per WHATWG spec.
    /// Returns true if fully handled (don't continue to normal processing),
    /// false if we should fall through to HTML processing.
    fn process_end_tag_in_foreign_content(&mut self, name: &str) -> bool {
        let name_lower = name.to_ascii_lowercase();

        // Step 1: If the current node's tag name (case-insensitively) matches, pop it
        if let Some(current) = self.current_node() {
            if current.name.to_ascii_lowercase() == name_lower {
                self.pop_and_add_to_parent();
                return true;
            }
        }

        // Step 2: Loop through the stack backwards
        // Find either a matching element to pop, or an HTML element to break out to
        for i in (0..self.open_elements.len()).rev() {
            let node = &self.open_elements[i];

            // If we hit an HTML namespace element, break out to normal processing
            if node.namespace == Some(Namespace::Html) {
                // Pop all elements after this one (the foreign content elements)
                while self.open_elements.len() > i + 1 {
                    self.pop_and_add_to_parent();
                }
                return false; // Process using normal HTML rules
            }

            // If tag name matches, pop elements up to and including this one
            if node.name.to_ascii_lowercase() == name_lower {
                while self.open_elements.len() > i {
                    self.pop_and_add_to_parent();
                }
                return true;
            }
        }

        // No match found, but also no HTML element - just continue
        false
    }

    fn process_in_table_start_tag(&mut self, name: &str, attrs: HashMap<String, String>, self_closing: bool) {
        match name {
            "caption" => {
                self.clear_stack_to_table_context();
                self.push_formatting_marker();
                self.insert_html_element(name, attrs);
                self.insertion_mode = InsertionMode::InCaption;
            }
            "colgroup" => {
                self.clear_stack_to_table_context();
                self.insert_html_element(name, attrs);
                self.insertion_mode = InsertionMode::InColumnGroup;
            }
            "col" => {
                self.clear_stack_to_table_context();
                self.insert_html_element("colgroup", HashMap::new());
                self.insertion_mode = InsertionMode::InColumnGroup;
                self.process_start_tag(name, attrs, self_closing);
            }
            "tbody" | "tfoot" | "thead" => {
                self.clear_stack_to_table_context();
                self.insert_html_element(name, attrs);
                self.insertion_mode = InsertionMode::InTableBody;
            }
            "td" | "th" | "tr" => {
                self.clear_stack_to_table_context();
                self.insert_html_element("tbody", HashMap::new());
                self.insertion_mode = InsertionMode::InTableBody;
                self.process_start_tag(name, attrs, self_closing);
            }
            "table" => {
                self.error("unexpected-table-in-table");
                if self.has_element_in_table_scope("table") {
                    self.pop_elements_until("table");
                    self.reset_insertion_mode();
                    self.process_start_tag(name, attrs, self_closing);
                }
            }
            "style" | "script" | "template" => {
                self.process_in_head_start_tag(name, attrs, self_closing);
            }
            "input" => {
                let is_hidden = attrs.get("type")
                    .map_or(false, |t| t.eq_ignore_ascii_case("hidden"));
                if is_hidden {
                    self.error("unexpected-hidden-input-in-table");
                    self.insert_element_for_token(name, attrs, true);
                } else {
                    self.error("unexpected-input-in-table");
                    self.foster_parent_token(name, attrs, self_closing);
                }
            }
            "form" => {
                self.error("unexpected-form-in-table");
                if self.form_element_index.is_none() && !self.has_element_in_scope("template") {
                    self.insert_html_element(name, attrs);
                    self.form_element_index = Some(self.open_elements.len() - 1);
                    self.pop_and_add_to_parent();
                }
            }
            _ => {
                self.error("unexpected-element-in-table");
                self.foster_parent_token(name, attrs, self_closing);
            }
        }
    }

    fn process_in_select_start_tag(&mut self, name: &str, attrs: HashMap<String, String>, self_closing: bool) {
        match name {
            "html" => {
                self.process_in_body_start_tag(name, attrs, self_closing);
            }
            "option" => {
                if self.current_node().map_or(false, |n| n.name == "option") {
                    self.pop_and_add_to_parent();
                }
                self.insert_html_element(name, attrs);
            }
            "optgroup" => {
                if self.current_node().map_or(false, |n| n.name == "option") {
                    self.pop_and_add_to_parent();
                }
                if self.current_node().map_or(false, |n| n.name == "optgroup") {
                    self.pop_and_add_to_parent();
                }
                self.insert_html_element(name, attrs);
            }
            "hr" => {
                if self.current_node().map_or(false, |n| n.name == "option") {
                    self.pop_and_add_to_parent();
                }
                if self.current_node().map_or(false, |n| n.name == "optgroup") {
                    self.pop_and_add_to_parent();
                }
                self.insert_element_for_token(name, attrs, true);
            }
            "select" => {
                self.error("unexpected-select-in-select");
                self.pop_elements_until("select");
                self.reset_insertion_mode();
            }
            "input" | "textarea" => {
                self.error("unexpected-input-in-select");
                if self.has_element_in_select_scope("select") {
                    self.pop_elements_until("select");
                    self.reset_insertion_mode();
                    self.process_start_tag(name, attrs, self_closing);
                }
            }
            "script" | "template" => {
                self.process_in_head_start_tag(name, attrs, self_closing);
            }
            _ => {
                self.error("unexpected-element-in-select");
            }
        }
    }

    fn process_end_tag(&mut self, name: &str) {
        // Check for foreign content handling first
        if self.is_in_foreign_content() {
            if self.process_end_tag_in_foreign_content(name) {
                return;
            }
        }

        match self.insertion_mode {
            InsertionMode::Initial => {
                self.insertion_mode = InsertionMode::BeforeHtml;
                self.process_end_tag(name);
            }
            InsertionMode::BeforeHtml => {
                match name {
                    "head" | "body" | "html" | "br" => {
                        // Record where html should be inserted (after doctype/pre-html content)
                        self.html_insert_index = self.document.children.len();
                        self.insert_html_element("html", HashMap::new());
                        self.insertion_mode = InsertionMode::BeforeHead;
                        self.process_end_tag(name);
                    }
                    _ => {
                        self.error("unexpected-end-tag-before-html");
                    }
                }
            }
            InsertionMode::BeforeHead => {
                match name {
                    "head" | "body" | "html" | "br" => {
                        self.insert_html_element("head", HashMap::new());
                        self.head_element_index = Some(self.open_elements.len() - 1);
                        self.insertion_mode = InsertionMode::InHead;
                        self.process_end_tag(name);
                    }
                    _ => {
                        self.error("unexpected-end-tag-before-head");
                    }
                }
            }
            InsertionMode::InHead => {
                match name {
                    "head" => {
                        self.pop_and_add_to_parent();
                        self.insertion_mode = InsertionMode::AfterHead;
                    }
                    "body" | "html" | "br" => {
                        self.pop_and_add_to_parent();
                        self.insertion_mode = InsertionMode::AfterHead;
                        self.process_end_tag(name);
                    }
                    "template" => {
                        if self.has_element_in_scope("template") {
                            self.generate_implied_end_tags();
                            self.pop_elements_until("template");
                            self.clear_active_formatting_to_marker();
                            self.template_insertion_modes.pop();
                            self.reset_insertion_mode();
                        } else {
                            self.error("unexpected-template-end-tag");
                        }
                    }
                    _ => {
                        self.error("unexpected-end-tag-in-head");
                    }
                }
            }
            InsertionMode::InHeadNoscript => {
                match name {
                    "noscript" => {
                        self.pop_and_add_to_parent();
                        self.insertion_mode = InsertionMode::InHead;
                    }
                    "br" => {
                        self.error("unexpected-br-in-noscript");
                        self.pop_and_add_to_parent();
                        self.insertion_mode = InsertionMode::InHead;
                        self.process_end_tag(name);
                    }
                    _ => {
                        self.error("unexpected-end-tag-in-noscript");
                    }
                }
            }
            InsertionMode::AfterHead => {
                match name {
                    "template" => {
                        self.process_end_tag_in_head(name);
                    }
                    "body" | "html" | "br" => {
                        self.insert_html_element("body", HashMap::new());
                        self.body_element_index = Some(self.open_elements.len() - 1);
                        self.insertion_mode = InsertionMode::InBody;
                        self.process_end_tag(name);
                    }
                    _ => {
                        self.error("unexpected-end-tag-after-head");
                    }
                }
            }
            InsertionMode::InBody => {
                self.process_in_body_end_tag(name);
            }
            InsertionMode::Text => {
                if name == "script" {
                    self.pop_and_add_to_parent();
                } else {
                    self.pop_and_add_to_parent();
                }
                self.insertion_mode = self.original_insertion_mode;
            }
            InsertionMode::InTable => {
                self.process_in_table_end_tag(name);
            }
            InsertionMode::InTableText => {
                self.flush_table_text();
                self.insertion_mode = InsertionMode::InTable;
                self.process_end_tag(name);
            }
            InsertionMode::InCaption => {
                match name {
                    "caption" => {
                        if self.has_element_in_table_scope("caption") {
                            self.generate_implied_end_tags();
                            self.pop_elements_until("caption");
                            self.clear_active_formatting_to_marker();
                            self.insertion_mode = InsertionMode::InTable;
                        } else {
                            self.error("unexpected-end-tag-in-caption");
                        }
                    }
                    "table" => {
                        if self.has_element_in_table_scope("caption") {
                            self.generate_implied_end_tags();
                            self.pop_elements_until("caption");
                            self.clear_active_formatting_to_marker();
                            self.insertion_mode = InsertionMode::InTable;
                            self.process_end_tag(name);
                        }
                    }
                    "body" | "col" | "colgroup" | "html" | "tbody" | "td" |
                    "tfoot" | "th" | "thead" | "tr" => {
                        self.error("unexpected-end-tag-in-caption");
                    }
                    _ => {
                        self.process_in_body_end_tag(name);
                    }
                }
            }
            InsertionMode::InColumnGroup => {
                match name {
                    "colgroup" => {
                        if self.current_node().map_or(false, |n| n.name == "colgroup") {
                            self.pop_and_add_to_parent();
                            self.insertion_mode = InsertionMode::InTable;
                        } else {
                            self.error("unexpected-colgroup-end-tag");
                        }
                    }
                    "col" => {
                        self.error("unexpected-col-end-tag");
                    }
                    "template" => {
                        self.process_end_tag_in_head(name);
                    }
                    _ => {
                        if self.current_node().map_or(false, |n| n.name == "colgroup") {
                            self.pop_and_add_to_parent();
                            self.insertion_mode = InsertionMode::InTable;
                            self.process_end_tag(name);
                        }
                    }
                }
            }
            InsertionMode::InTableBody => {
                match name {
                    "tbody" | "tfoot" | "thead" => {
                        if self.has_element_in_table_scope(name) {
                            self.clear_stack_to_table_body_context();
                            self.pop_and_add_to_parent();
                            self.insertion_mode = InsertionMode::InTable;
                        } else {
                            self.error("unexpected-table-section-end-tag");
                        }
                    }
                    "table" => {
                        if self.has_element_in_table_scope("tbody") ||
                           self.has_element_in_table_scope("thead") ||
                           self.has_element_in_table_scope("tfoot") {
                            self.clear_stack_to_table_body_context();
                            self.pop_and_add_to_parent();
                            self.insertion_mode = InsertionMode::InTable;
                            self.process_end_tag(name);
                        }
                    }
                    "body" | "caption" | "col" | "colgroup" | "html" | "td" | "th" | "tr" => {
                        self.error("unexpected-end-tag-in-table-body");
                    }
                    _ => {
                        self.process_in_table_end_tag(name);
                    }
                }
            }
            InsertionMode::InRow => {
                match name {
                    "tr" => {
                        if self.has_element_in_table_scope("tr") {
                            self.clear_stack_to_table_row_context();
                            self.pop_and_add_to_parent();
                            self.insertion_mode = InsertionMode::InTableBody;
                        } else {
                            self.error("unexpected-tr-end-tag");
                        }
                    }
                    "table" => {
                        if self.has_element_in_table_scope("tr") {
                            self.clear_stack_to_table_row_context();
                            self.pop_and_add_to_parent();
                            self.insertion_mode = InsertionMode::InTableBody;
                            self.process_end_tag(name);
                        }
                    }
                    "tbody" | "tfoot" | "thead" => {
                        if self.has_element_in_table_scope(name) {
                            if self.has_element_in_table_scope("tr") {
                                self.clear_stack_to_table_row_context();
                                self.pop_and_add_to_parent();
                                self.insertion_mode = InsertionMode::InTableBody;
                                self.process_end_tag(name);
                            }
                        } else {
                            self.error("unexpected-table-section-end-tag");
                        }
                    }
                    "body" | "caption" | "col" | "colgroup" | "html" | "td" | "th" => {
                        self.error("unexpected-end-tag-in-row");
                    }
                    _ => {
                        self.process_in_table_end_tag(name);
                    }
                }
            }
            InsertionMode::InCell => {
                match name {
                    "td" | "th" => {
                        if self.has_element_in_table_scope(name) {
                            self.generate_implied_end_tags();
                            self.pop_elements_until(name);
                            self.clear_active_formatting_to_marker();
                            self.insertion_mode = InsertionMode::InRow;
                        } else {
                            self.error("unexpected-cell-end-tag");
                        }
                    }
                    "body" | "caption" | "col" | "colgroup" | "html" => {
                        self.error("unexpected-end-tag-in-cell");
                    }
                    "table" | "tbody" | "tfoot" | "thead" | "tr" => {
                        if self.has_element_in_table_scope(name) {
                            self.close_cell();
                            self.process_end_tag(name);
                        }
                    }
                    _ => {
                        self.process_in_body_end_tag(name);
                    }
                }
            }
            InsertionMode::InSelect => {
                match name {
                    "optgroup" => {
                        if self.current_node().map_or(false, |n| n.name == "option") {
                            if self.open_elements.len() >= 2 {
                                if self.open_elements[self.open_elements.len() - 2].name == "optgroup" {
                                    self.pop_and_add_to_parent();
                                }
                            }
                        }
                        if self.current_node().map_or(false, |n| n.name == "optgroup") {
                            self.pop_and_add_to_parent();
                        } else {
                            self.error("unexpected-optgroup-end-tag");
                        }
                    }
                    "option" => {
                        if self.current_node().map_or(false, |n| n.name == "option") {
                            self.pop_and_add_to_parent();
                        } else {
                            self.error("unexpected-option-end-tag");
                        }
                    }
                    "select" => {
                        if self.has_element_in_select_scope("select") {
                            self.pop_elements_until("select");
                            self.reset_insertion_mode();
                        } else {
                            self.error("unexpected-select-end-tag");
                        }
                    }
                    "template" => {
                        self.process_end_tag_in_head(name);
                    }
                    _ => {
                        self.error("unexpected-end-tag-in-select");
                    }
                }
            }
            InsertionMode::InSelectInTable => {
                match name {
                    "caption" | "table" | "tbody" | "tfoot" | "thead" | "tr" | "td" | "th" => {
                        self.error("unexpected-table-end-tag-in-select");
                        if self.has_element_in_table_scope(name) {
                            self.pop_elements_until("select");
                            self.reset_insertion_mode();
                            self.process_end_tag(name);
                        }
                    }
                    _ => {
                        self.process_end_tag_in_select(name);
                    }
                }
            }
            InsertionMode::InTemplate => {
                match name {
                    "template" => {
                        self.process_end_tag_in_head(name);
                    }
                    _ => {
                        self.error("unexpected-end-tag-in-template");
                    }
                }
            }
            InsertionMode::AfterBody => {
                match name {
                    "html" => {
                        if self.fragment_context.is_some() {
                            self.error("unexpected-html-end-tag-in-fragment");
                        } else {
                            self.insertion_mode = InsertionMode::AfterAfterBody;
                        }
                    }
                    _ => {
                        self.error("unexpected-end-tag-after-body");
                        self.insertion_mode = InsertionMode::InBody;
                        self.process_end_tag(name);
                    }
                }
            }
            InsertionMode::InFrameset => {
                match name {
                    "frameset" => {
                        if self.current_node().map_or(false, |n| n.name == "html") {
                            self.error("unexpected-frameset-end-tag");
                        } else {
                            self.pop_and_add_to_parent();
                            if self.fragment_context.is_none() &&
                               self.current_node().map_or(false, |n| n.name != "frameset") {
                                self.insertion_mode = InsertionMode::AfterFrameset;
                            }
                        }
                    }
                    _ => {
                        self.error("unexpected-end-tag-in-frameset");
                    }
                }
            }
            InsertionMode::AfterFrameset => {
                match name {
                    "html" => {
                        self.insertion_mode = InsertionMode::AfterAfterFrameset;
                    }
                    _ => {
                        self.error("unexpected-end-tag-after-frameset");
                    }
                }
            }
            InsertionMode::AfterAfterBody => {
                self.error("unexpected-end-tag-after-body");
                self.insertion_mode = InsertionMode::InBody;
                self.process_end_tag(name);
            }
            InsertionMode::AfterAfterFrameset => {
                self.error("unexpected-end-tag-after-frameset");
            }
        }
    }

    fn process_end_tag_in_head(&mut self, name: &str) {
        match name {
            "template" => {
                if self.has_element_in_scope("template") {
                    self.generate_implied_end_tags();
                    self.pop_elements_until("template");
                    self.clear_active_formatting_to_marker();
                    self.template_insertion_modes.pop();
                    self.reset_insertion_mode();
                } else {
                    self.error("unexpected-template-end-tag");
                }
            }
            _ => {}
        }
    }

    fn process_end_tag_in_select(&mut self, name: &str) {
        match name {
            "optgroup" | "option" | "select" | "template" => {
                // Already handled in InSelect
            }
            _ => {
                self.error("unexpected-end-tag-in-select");
            }
        }
    }

    fn process_in_body_end_tag(&mut self, name: &str) {
        match name {
            "template" => {
                self.process_end_tag_in_head(name);
            }
            "body" => {
                if !self.has_element_in_scope("body") {
                    self.error("unexpected-body-end-tag");
                } else {
                    self.insertion_mode = InsertionMode::AfterBody;
                }
            }
            "html" => {
                if !self.has_element_in_scope("body") {
                    self.error("unexpected-html-end-tag");
                } else {
                    self.insertion_mode = InsertionMode::AfterBody;
                    self.process_end_tag(name);
                }
            }
            "address" | "article" | "aside" | "blockquote" | "button" | "center" |
            "details" | "dialog" | "dir" | "div" | "dl" | "fieldset" | "figcaption" |
            "figure" | "footer" | "header" | "hgroup" | "listing" | "main" | "menu" |
            "nav" | "ol" | "pre" | "search" | "section" | "summary" | "ul" => {
                if self.has_element_in_scope(name) {
                    self.generate_implied_end_tags();
                    self.pop_elements_until(name);
                } else {
                    self.error("unexpected-end-tag");
                }
            }
            "form" => {
                if !self.has_element_in_scope("template") {
                    self.form_element_index = None;
                    if self.has_element_in_scope("form") {
                        self.generate_implied_end_tags();
                        self.pop_elements_until("form");
                    } else {
                        self.error("unexpected-form-end-tag");
                    }
                } else if self.has_element_in_scope("form") {
                    self.generate_implied_end_tags();
                    self.pop_elements_until("form");
                } else {
                    self.error("unexpected-form-end-tag");
                }
            }
            "p" => {
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                } else {
                    self.error("unexpected-p-end-tag");
                    self.insert_html_element("p", HashMap::new());
                    self.close_p_element();
                }
            }
            "li" => {
                if self.has_element_in_list_item_scope("li") {
                    self.generate_implied_end_tags_except(Some("li"));
                    self.pop_elements_until("li");
                } else {
                    self.error("unexpected-li-end-tag");
                }
            }
            "dd" | "dt" => {
                if self.has_element_in_scope(name) {
                    self.generate_implied_end_tags_except(Some(name));
                    self.pop_elements_until(name);
                } else {
                    self.error("unexpected-dd-dt-end-tag");
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if self.has_element_in_scope("h1") || self.has_element_in_scope("h2") ||
                   self.has_element_in_scope("h3") || self.has_element_in_scope("h4") ||
                   self.has_element_in_scope("h5") || self.has_element_in_scope("h6") {
                    self.generate_implied_end_tags();
                    self.pop_elements_until_one_of(&["h1", "h2", "h3", "h4", "h5", "h6"]);
                } else {
                    self.error("unexpected-heading-end-tag");
                }
            }
            "a" | "b" | "big" | "code" | "em" | "font" | "i" | "nobr" | "s" |
            "small" | "strike" | "strong" | "tt" | "u" => {
                self.adoption_agency(name);
            }
            "applet" | "marquee" | "object" => {
                if self.has_element_in_scope(name) {
                    self.generate_implied_end_tags();
                    self.pop_elements_until(name);
                    self.clear_active_formatting_to_marker();
                } else {
                    self.error("unexpected-end-tag");
                }
            }
            "br" => {
                self.error("unexpected-br-end-tag");
                self.reconstruct_active_formatting_elements();
                self.insert_element_for_token("br", HashMap::new(), true);
            }
            _ => {
                self.any_other_end_tag(name);
            }
        }
    }

    fn process_in_table_end_tag(&mut self, name: &str) {
        match name {
            "table" => {
                if self.has_element_in_table_scope("table") {
                    self.pop_elements_until("table");
                    self.reset_insertion_mode();
                } else {
                    self.error("unexpected-table-end-tag");
                }
            }
            "body" | "caption" | "col" | "colgroup" | "html" | "tbody" |
            "td" | "tfoot" | "th" | "thead" | "tr" => {
                self.error("unexpected-end-tag-in-table");
            }
            "template" => {
                self.process_end_tag_in_head(name);
            }
            _ => {
                self.error("unexpected-end-tag-in-table");
                self.foster_parent_end_tag(name);
            }
        }
    }

    fn process_character(&mut self, c: char) {
        match self.insertion_mode {
            InsertionMode::Initial => {
                if c.is_ascii_whitespace() {
                    // Ignore
                } else {
                    self.insertion_mode = InsertionMode::BeforeHtml;
                    self.process_character(c);
                }
            }
            InsertionMode::BeforeHtml => {
                if c.is_ascii_whitespace() {
                    // Ignore
                } else {
                    self.html_insert_index = self.document.children.len();
                    self.insert_html_element("html", HashMap::new());
                    self.insertion_mode = InsertionMode::BeforeHead;
                    self.process_character(c);
                }
            }
            InsertionMode::BeforeHead => {
                if c.is_ascii_whitespace() {
                    // Ignore
                } else {
                    self.insert_html_element("head", HashMap::new());
                    self.head_element_index = Some(self.open_elements.len() - 1);
                    self.insertion_mode = InsertionMode::InHead;
                    self.process_character(c);
                }
            }
            InsertionMode::InHead | InsertionMode::InHeadNoscript => {
                if c == '\t' || c == '\n' || c == '\x0C' || c == '\r' || c == ' ' {
                    self.insert_character(c);
                } else {
                    self.pop_and_add_to_parent(); // Pop head
                    self.insertion_mode = InsertionMode::AfterHead;
                    self.process_character(c);
                }
            }
            InsertionMode::AfterHead => {
                if c == '\t' || c == '\n' || c == '\x0C' || c == '\r' || c == ' ' {
                    self.insert_character(c);
                } else {
                    self.insert_html_element("body", HashMap::new());
                    self.body_element_index = Some(self.open_elements.len() - 1);
                    self.insertion_mode = InsertionMode::InBody;
                    self.process_character(c);
                }
            }
            InsertionMode::InBody => {
                if c == '\0' {
                    self.error("unexpected-null-character");
                } else {
                    self.reconstruct_active_formatting_elements();
                    self.insert_character(c);
                    if !c.is_ascii_whitespace() {
                        self.frameset_ok = false;
                    }
                }
            }
            InsertionMode::Text => {
                if c == '\0' {
                    self.error("unexpected-null-character");
                } else {
                    self.insert_character(c);
                }
            }
            InsertionMode::InTable => {
                if TABLE_CONTEXT_TAGS.contains(self.current_node().map_or("", |n| n.name.as_str())) {
                    self.pending_table_chars.clear();
                    self.original_insertion_mode = self.insertion_mode;
                    self.insertion_mode = InsertionMode::InTableText;
                    self.process_character(c);
                } else {
                    self.error("unexpected-character-in-table");
                    self.foster_parent_character(c);
                }
            }
            InsertionMode::InTableText => {
                if c == '\0' {
                    self.error("unexpected-null-character");
                } else {
                    self.pending_table_chars.push(c);
                }
            }
            InsertionMode::InCaption | InsertionMode::InCell => {
                self.process_in_body_character(c);
            }
            InsertionMode::InColumnGroup => {
                if c.is_ascii_whitespace() {
                    self.insert_character(c);
                } else {
                    if self.current_node().map_or(false, |n| n.name == "colgroup") {
                        self.pop_and_add_to_parent();
                        self.insertion_mode = InsertionMode::InTable;
                        self.process_character(c);
                    }
                }
            }
            InsertionMode::InTableBody | InsertionMode::InRow => {
                self.process_in_table_character(c);
            }
            InsertionMode::InSelect | InsertionMode::InSelectInTable => {
                if c == '\0' {
                    self.error("unexpected-null-character");
                } else {
                    self.insert_character(c);
                }
            }
            InsertionMode::InTemplate => {
                self.process_in_body_character(c);
            }
            InsertionMode::AfterBody => {
                if c.is_ascii_whitespace() {
                    self.process_in_body_character(c);
                } else {
                    self.error("unexpected-character-after-body");
                    self.insertion_mode = InsertionMode::InBody;
                    self.process_character(c);
                }
            }
            InsertionMode::InFrameset | InsertionMode::AfterFrameset => {
                if c.is_ascii_whitespace() {
                    self.insert_character(c);
                } else {
                    self.error("unexpected-character-in-frameset");
                }
            }
            InsertionMode::AfterAfterBody => {
                if c.is_ascii_whitespace() {
                    self.process_in_body_character(c);
                } else {
                    self.error("unexpected-character-after-body");
                    self.insertion_mode = InsertionMode::InBody;
                    self.process_character(c);
                }
            }
            InsertionMode::AfterAfterFrameset => {
                if c.is_ascii_whitespace() {
                    self.insert_character(c);
                } else {
                    self.error("unexpected-character-after-frameset");
                }
            }
        }
    }

    fn process_in_body_character(&mut self, c: char) {
        if c == '\0' {
            self.error("unexpected-null-character");
        } else {
            self.reconstruct_active_formatting_elements();
            self.insert_character(c);
            if !c.is_ascii_whitespace() {
                self.frameset_ok = false;
            }
        }
    }

    fn process_in_table_character(&mut self, c: char) {
        let current = self.current_node();
        let current_name = current.map_or("", |n| n.name.as_str());

        if TABLE_CONTEXT_TAGS.contains(current_name) {
            self.pending_table_chars.clear();
            self.original_insertion_mode = self.insertion_mode;
            self.insertion_mode = InsertionMode::InTableText;
            self.process_character(c);
        } else if current.map_or(false, |n| n.foster_parented) {
            // We're inside a foster-parented element, process text normally
            self.insert_character(c);
        } else {
            self.error("unexpected-character-in-table");
            self.foster_parent_character(c);
        }
    }

    fn process_comment(&mut self, data: &str) {
        match self.insertion_mode {
            InsertionMode::Initial | InsertionMode::BeforeHtml |
            InsertionMode::AfterAfterBody | InsertionMode::AfterAfterFrameset => {
                let comment = Node::comment(data);
                self.document.children.push(comment);
            }
            InsertionMode::AfterBody => {
                // Append to html element
                if let Some(html) = self.open_elements.first_mut() {
                    html.children.push(Node::comment(data));
                }
            }
            _ => {
                self.insert_comment(data);
            }
        }
    }

    fn process_eof(&mut self) {
        match self.insertion_mode {
            InsertionMode::Initial => {
                self.insertion_mode = InsertionMode::BeforeHtml;
                self.process_eof();
            }
            InsertionMode::BeforeHtml => {
                self.html_insert_index = self.document.children.len();
                self.insert_html_element("html", HashMap::new());
                self.insertion_mode = InsertionMode::BeforeHead;
                self.process_eof();
            }
            InsertionMode::BeforeHead => {
                self.insert_html_element("head", HashMap::new());
                self.head_element_index = Some(self.open_elements.len() - 1);
                self.insertion_mode = InsertionMode::InHead;
                self.process_eof();
            }
            InsertionMode::InHead | InsertionMode::InHeadNoscript => {
                // Only pop if head is actually on the stack
                // (It might have been removed during AfterHead->InHead head reinsertion)
                if self.current_node().map_or(false, |n| n.name == "head") {
                    self.pop_and_add_to_parent();
                }
                self.insertion_mode = InsertionMode::AfterHead;
                self.process_eof();
            }
            InsertionMode::AfterHead => {
                self.insert_html_element("body", HashMap::new());
                self.body_element_index = Some(self.open_elements.len() - 1);
                self.insertion_mode = InsertionMode::InBody;
                self.process_eof();
            }
            InsertionMode::InBody | InsertionMode::InCell | InsertionMode::InCaption |
            InsertionMode::InRow => {
                // Stop parsing
            }
            InsertionMode::Text => {
                self.error("eof-in-text");
                // Don't pop the root element in fragment parsing
                if !(self.fragment_context.is_some() && self.open_elements.len() == 1) {
                    self.pop_and_add_to_parent();
                }
                self.insertion_mode = self.original_insertion_mode;
                self.process_eof();
            }
            InsertionMode::InTable | InsertionMode::InTableBody |
            InsertionMode::InColumnGroup => {
                // Stop parsing
            }
            InsertionMode::InTableText => {
                // Flush pending table text before stopping
                self.flush_table_text();
                // Stop parsing
            }
            InsertionMode::InSelect | InsertionMode::InSelectInTable => {
                // Stop parsing
            }
            InsertionMode::InTemplate => {
                if self.template_insertion_modes.is_empty() {
                    // Stop parsing
                } else {
                    self.error("eof-in-template");
                    self.pop_elements_until("template");
                    self.clear_active_formatting_to_marker();
                    self.template_insertion_modes.pop();
                    self.reset_insertion_mode();
                    self.process_eof();
                }
            }
            InsertionMode::AfterBody | InsertionMode::AfterFrameset |
            InsertionMode::AfterAfterBody | InsertionMode::AfterAfterFrameset |
            InsertionMode::InFrameset => {
                // Stop parsing
            }
        }
    }

    // Helper methods
    fn clear_stack_to_table_context(&mut self) {
        while let Some(node) = self.open_elements.last() {
            if TABLE_CONTEXT_TAGS.contains(node.name.as_str()) {
                break;
            }
            self.pop_and_add_to_parent();
        }
    }

    fn clear_stack_to_table_body_context(&mut self) {
        while let Some(node) = self.open_elements.last() {
            if ["tbody", "tfoot", "thead", "template", "html"].contains(&node.name.as_str()) {
                break;
            }
            self.pop_and_add_to_parent();
        }
    }

    fn clear_stack_to_table_row_context(&mut self) {
        while let Some(node) = self.open_elements.last() {
            if ["tr", "template", "html"].contains(&node.name.as_str()) {
                break;
            }
            self.pop_and_add_to_parent();
        }
    }

    fn close_cell(&mut self) {
        self.generate_implied_end_tags();
        if let Some(node) = self.open_elements.last() {
            if node.name == "td" || node.name == "th" {
                self.pop_elements_until_one_of(&["td", "th"]);
            }
        }
        self.clear_active_formatting_to_marker();
        self.insertion_mode = InsertionMode::InRow;
    }

    fn reset_insertion_mode(&mut self) {
        for i in (0..self.open_elements.len()).rev() {
            let node = &self.open_elements[i];
            let last = i == 0;

            let name = if last {
                if let Some(ref ctx) = self.fragment_context {
                    &ctx.tag_name
                } else {
                    &node.name
                }
            } else {
                &node.name
            };

            self.insertion_mode = match name.as_str() {
                "select" => {
                    if !last {
                        for j in (0..i).rev() {
                            let ancestor = &self.open_elements[j];
                            if ancestor.name == "template" {
                                break;
                            }
                            if ancestor.name == "table" {
                                return self.insertion_mode = InsertionMode::InSelectInTable;
                            }
                        }
                    }
                    InsertionMode::InSelect
                }
                "td" | "th" if !last => InsertionMode::InCell,
                "tr" => InsertionMode::InRow,
                "tbody" | "thead" | "tfoot" => InsertionMode::InTableBody,
                "caption" => InsertionMode::InCaption,
                "colgroup" => InsertionMode::InColumnGroup,
                "table" => InsertionMode::InTable,
                "template" => {
                    self.template_insertion_modes.last().copied()
                        .unwrap_or(InsertionMode::InTemplate)
                }
                "head" if !last => InsertionMode::InHead,
                "body" => InsertionMode::InBody,
                "frameset" => InsertionMode::InFrameset,
                "html" => {
                    if self.head_element_index.is_none() {
                        InsertionMode::BeforeHead
                    } else {
                        InsertionMode::AfterHead
                    }
                }
                _ if last => InsertionMode::InBody,
                _ => continue,
            };
            return;
        }
        self.insertion_mode = InsertionMode::InBody;
    }

    fn flush_table_text(&mut self) {
        let text = std::mem::take(&mut self.pending_table_chars);
        if text.chars().any(|c| !c.is_ascii_whitespace()) {
            // Contains non-whitespace - foster parent
            for c in text.chars() {
                self.foster_parent_character(c);
            }
        } else {
            // All whitespace - insert normally
            for c in text.chars() {
                self.insert_character(c);
            }
        }
    }

    fn foster_parent_token(&mut self, name: &str, attrs: HashMap<String, String>, self_closing: bool) {
        // Simplified foster parenting - just insert in body
        self.foster_parenting = true;
        self.process_in_body_start_tag(name, attrs, self_closing);
        self.foster_parenting = false;
    }

    fn foster_parent_end_tag(&mut self, name: &str) {
        self.foster_parenting = true;
        self.process_in_body_end_tag(name);
        self.foster_parenting = false;
    }

    fn foster_parent_character(&mut self, c: char) {
        self.foster_parenting = true;
        self.process_in_body_character(c);
        self.foster_parenting = false;
    }
}
