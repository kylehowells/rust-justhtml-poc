//! HTML5 tree construction algorithm

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use crate::node::{Doctype, Namespace, Node, NodeData, generate_node_id};
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
    // For clear_stack_to_table_context
    ["table", "template", "html"].into_iter().collect()
});

// Per WHATWG spec: elements that trigger InTableText mode for characters in InTable
static TABLE_TEXT_CONTEXT_TAGS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    ["table", "tbody", "template", "tfoot", "thead", "tr"].into_iter().collect()
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
    quirks_mode: bool,

    /// Pending table character tokens
    pending_table_chars: String,

    /// Index where html element should be inserted in document
    html_insert_index: usize,

    /// Comments that should appear after body in the html element
    /// (inserted when parsing ends)
    after_body_comments: Vec<Node>,

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
            quirks_mode: false,
            pending_table_chars: String::new(),
            html_insert_index: 0,
            after_body_comments: Vec::new(),
            errors: Vec::new(),
        };

        if let Some(ctx) = fragment_context {
            builder.setup_fragment_context(ctx);
        }

        builder
    }

    fn setup_fragment_context(&mut self, ctx: &FragmentContext) {
        // Handle SVG/MathML namespace fragments specially
        if let Some(ref ns) = ctx.namespace {
            // Create context element with namespace
            let element = Node::element_ns(&ctx.tag_name, *ns, HashMap::new());
            self.open_elements.push(element);
            // Use InBody mode - foreign content is handled by adjusted_current_node
            self.insertion_mode = InsertionMode::InBody;
            return;
        }

        // Set initial insertion mode based on context
        let mode = match ctx.tag_name.as_str() {
            "title" | "textarea" => InsertionMode::Text,
            "style" | "xmp" | "iframe" | "noembed" | "noframes" => InsertionMode::Text,
            "script" => InsertionMode::Text,
            "noscript" if self.scripting => InsertionMode::Text,
            "plaintext" => InsertionMode::Text,
            "template" => {
                // For template context, push a template element directly
                let template = Node::element("template", HashMap::new());
                self.open_elements.push(template);
                self.template_insertion_modes.push(InsertionMode::InTemplate);
                self.insertion_mode = InsertionMode::InTemplate;
                return; // Early return - don't create html wrapper
            }
            "select" => {
                // For select context, push html and select elements
                // Per html5lib behavior: select fragments use inBody mode, not inSelect
                // This allows unknown elements to be inserted inside select context
                let html = Node::element("html", HashMap::new());
                self.open_elements.push(html);
                let select = Node::element("select", HashMap::new());
                self.open_elements.push(select);
                self.insertion_mode = InsertionMode::InBody;
                return;
            }
            "head" => InsertionMode::InBody,
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

        // Create a dummy HTML element as the root (for non-template contexts)
        let html = Node::element("html", HashMap::new());
        self.open_elements.push(html);

        self.insertion_mode = mode;
        // For Text mode fragments, set original_insertion_mode to InBody
        // so EOF doesn't trigger Initial mode processing
        if mode == InsertionMode::Text {
            self.original_insertion_mode = InsertionMode::InBody;
        }
    }

    pub fn finish(mut self) -> (Node, Vec<ParseError>) {
        // Pop all elements from the stack, nesting them properly
        // Keep popping until we reach the html element (which should be inserted into document)
        while self.open_elements.len() > 1 {
            self.pop_and_add_to_parent();
        }

        // Check if the remaining element is a placeholder (is_parented=true)
        // This can happen when close_cell pops everything and we insert with empty stack
        while let Some(node) = self.open_elements.last() {
            if node.is_parented && node.real_node_id.is_some() {
                // This is a placeholder - pop it without adding to document
                self.open_elements.pop();
            } else {
                break;
            }
        }

        // Insert any pending after-body comments into the html element
        // These comments were seen in AfterBody mode and should appear after body
        if let Some(html) = self.open_elements.first_mut() {
            for comment in self.after_body_comments.drain(..) {
                html.children.push(comment);
            }
        }

        // Move the root element to the document
        if let Some(root) = self.open_elements.pop() {
            if let Some(ref ctx) = self.fragment_context {
                if ctx.tag_name == "template" {
                    // For template fragment, add the template element directly
                    self.document.children.push(root);
                } else if ctx.tag_name == "select" {
                    // For select fragment, get the children of the select element
                    // root is html, its first child should be select
                    for child in root.children {
                        if child.name == "select" {
                            for select_child in child.children {
                                self.document.children.push(select_child);
                            }
                        } else {
                            self.document.children.push(child);
                        }
                    }
                } else {
                    // For other fragments, the children of html become document-fragment children
                    for child in root.children {
                        self.document.children.push(child);
                    }
                }
            } else {
                // Insert html at the position recorded when we entered BeforeHtml
                // This preserves the order: doctype, pre-html comments, html, post-html comments
                self.document.children.insert(self.html_insert_index, root);
            }
        }

        // Post-process selectedcontent elements to clone selected option content
        Self::process_selectedcontent(&mut self.document);

        (self.document, self.errors)
    }

    /// Post-process selectedcontent elements: clone the selected option's content into them
    fn process_selectedcontent(node: &mut Node) {
        // Process children recursively first
        for child in &mut node.children {
            Self::process_selectedcontent(child);
        }

        // If this is a select element, handle selectedcontent
        if node.name == "select" {
            Self::process_select_selectedcontent(node);
        }
    }

    /// Handle selectedcontent within a select element
    fn process_select_selectedcontent(select: &mut Node) {
        // Find the selected option (or first option) among select's children
        let mut selected_option_content: Option<Vec<Node>> = None;
        let mut first_option_content: Option<Vec<Node>> = None;

        for child in &select.children {
            Self::find_option_content(child, &mut selected_option_content, &mut first_option_content);
        }

        // Use selected option content, or first option content if none selected
        let content_to_clone = selected_option_content.or(first_option_content);

        if let Some(content) = content_to_clone {
            // Find and populate selectedcontent elements
            for child in &mut select.children {
                Self::populate_selectedcontent(child, &content);
            }
        }
    }

    /// Find option content recursively (handles options nested in optgroups, buttons, etc.)
    fn find_option_content(
        node: &Node,
        selected: &mut Option<Vec<Node>>,
        first: &mut Option<Vec<Node>>,
    ) {
        if node.name == "option" {
            let is_selected = node.attrs.contains_key("selected");
            let children_clone = node.children.clone();

            if first.is_none() {
                *first = Some(children_clone.clone());
            }
            if is_selected {
                *selected = Some(children_clone);
            }
        } else if node.name != "selectedcontent" {
            // Don't look inside selectedcontent, but do look inside other elements
            for child in &node.children {
                Self::find_option_content(child, selected, first);
            }
        }
    }

    /// Populate selectedcontent elements with cloned content
    fn populate_selectedcontent(node: &mut Node, content: &[Node]) {
        if node.name == "selectedcontent" {
            // Clear existing children and add cloned content
            node.children.clear();
            for item in content {
                node.children.push(item.clone());
            }
        } else {
            // Recurse into children
            for child in &mut node.children {
                Self::populate_selectedcontent(child, content);
            }
        }
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

    /// Find a node by ID in the entire open elements tree and return mutable reference
    fn find_real_node_mut(&mut self, target_id: u64) -> Option<&mut Node> {
        // Search through all open elements and their subtrees
        for element in self.open_elements.iter_mut() {
            if let Some(found) = element.find_by_id_mut(target_id) {
                return Some(found);
            }
        }
        None
    }

    /// Extract (remove and return) a node by ID from a list of children, searching recursively.
    fn extract_node_by_id(children: &mut Vec<Node>, target_id: u64) -> Option<Node> {
        for i in 0..children.len() {
            if children[i].id == target_id {
                return Some(children.remove(i));
            }
            // Recursively search in nested children
            if let Some(extracted) = Self::extract_node_by_id(&mut children[i].children, target_id) {
                return Some(extracted);
            }
        }
        None
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

        let mut element = Node::element_ns(adjusted_name, namespace, attrs);

        // Check if current node is a foster parented element that redirects to a real DOM node
        let current_is_foster_parented = self.open_elements.last()
            .map_or(false, |n| n.is_parented && n.real_node_id.is_some());
        let current_real_node_id = self.open_elements.last().and_then(|n| n.real_node_id);

        // Handle foster parenting (insert before table in DOM, but still push to stack)
        if self.foster_parenting {
            if current_is_foster_parented {
                // Insert into the current element's real DOM location
                if let Some(target_id) = current_real_node_id {
                    let dom_element = element.clone_deep();
                    element.is_parented = true;
                    element.real_node_id = Some(dom_element.id);
                    if let Some(real_node) = self.find_real_node_mut(target_id) {
                        real_node.children.push(dom_element);
                    }
                }
            } else if let Some((parent_idx, insert_idx)) = self.find_foster_parent_location() {
                let dom_element = element.clone_deep(); // Use clone_deep to get a new ID
                element.is_parented = true;
                element.real_node_id = Some(dom_element.id);
                self.open_elements[parent_idx].children.insert(insert_idx, dom_element);
            }
        } else if self.open_elements.is_empty() && adjusted_name != "html" {
            // Stack is empty and we're NOT creating the initial html element
            // Find html element in document children and insert there
            // This matches Swift's adjustedInsertionTarget behavior
            let dom_element = element.clone_deep();
            element.is_parented = true;
            element.real_node_id = Some(dom_element.id);
            for child in &mut self.document.children {
                if child.name == "html" {
                    child.children.push(dom_element);
                    break;
                }
            }
            // If no html found, element just gets pushed to stack and will be
            // added to document when popped (normal behavior)
        }

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

        // Check if current node is a placeholder that redirects to a real DOM node
        let current_is_placeholder = self.open_elements.last()
            .map_or(false, |n| n.is_parented && n.real_node_id.is_some());
        // Check if current is a formatting element (blocks should not nest inside formatting elements)
        let current_is_formatting = self.open_elements.last()
            .map_or(false, |n| FORMATTING_ELEMENTS.contains(n.name.as_str()));
        // Check if NEW element is a block (special) element
        let new_is_block = SPECIAL_ELEMENTS.contains(name);
        let current_real_node_id = self.open_elements.last().and_then(|n| n.real_node_id);

        // Count consecutive formatting element placeholders at the end of the stack
        let formatting_placeholder_count = self.open_elements.iter().rev()
            .take_while(|elem| elem.is_parented && FORMATTING_ELEMENTS.contains(elem.name.as_str()))
            .count();

        if self.foster_parenting {
            // In foster parenting mode:
            // - If multiple formatting placeholders and new is block, wrap block in innermost formatting at foster parent
            // - If block element, foster parent at foster parent location (blocks don't go inside formatting)
            // - Otherwise, if current is a placeholder, insert into the real DOM node
            // - Otherwise, foster parent normally
            // Check if current node is a table-related element (only foster parent if so)
            let current_is_table_related = self.open_elements.last()
                .map_or(false, |n| matches!(n.name.as_str(), "table" | "tbody" | "tfoot" | "thead" | "tr"));

            if current_is_placeholder && new_is_block && formatting_placeholder_count > 1 {
                // Multiple nested formatting elements: wrap block in innermost formatting at foster parent location
                if let Some((parent_idx, insert_idx)) = self.find_foster_parent_location() {
                    // Get the innermost formatting element info (current element)
                    let current_elem = self.open_elements.last().unwrap();
                    let wrapper_name = current_elem.name.clone();
                    let wrapper_attrs = current_elem.attrs.clone();
                    let current_elem_id = current_elem.id;

                    // Create a new formatting element wrapper at foster parent location
                    let mut wrapper = Node::element(&wrapper_name, wrapper_attrs);

                    // Create the block element inside the wrapper
                    let dom_element = element.clone_deep();
                    element.is_parented = true;
                    element.real_node_id = Some(dom_element.id);
                    // Track that the wrapper formatting element is a DOM ancestor
                    element.formatting_ancestor_ids.push(current_elem_id);
                    wrapper.children.push(dom_element);

                    // Mark the corresponding AFE entry as parented so reconstruction doesn't duplicate it
                    for afe_entry in self.active_formatting_elements.iter_mut() {
                        if let Some(ref mut entry) = afe_entry {
                            if entry.id == current_elem_id {
                                entry.is_parented = true;
                                break;
                            }
                        }
                    }

                    // Insert wrapper at foster parent location
                    self.open_elements[parent_idx].children.insert(insert_idx, wrapper);
                }
            } else if current_is_placeholder {
                // Insert into the placeholder's real DOM location
                // This handles: single formatting element with block, or any non-block element
                if let Some(target_id) = current_real_node_id {
                    let dom_element = element.clone_deep();
                    element.is_parented = true;
                    element.real_node_id = Some(dom_element.id);

                    // If current is a formatting element and we're inserting a block,
                    // track that this formatting element is a DOM ancestor of the block
                    if new_is_block {
                        let current_elem = self.open_elements.last().unwrap();
                        if FORMATTING_ELEMENTS.contains(current_elem.name.as_str()) {
                            element.formatting_ancestor_ids.push(current_elem.id);
                        }
                    }

                    if let Some(real_node) = self.find_real_node_mut(target_id) {
                        real_node.children.push(dom_element);
                    }
                }
            } else if current_is_table_related {
                // Only actually foster parent when target is a table-related element
                if let Some((parent_idx, insert_idx)) = self.find_foster_parent_location() {
                    let dom_element = element.clone_deep();
                    element.is_parented = true;
                    element.real_node_id = Some(dom_element.id);
                    self.open_elements[parent_idx].children.insert(insert_idx, dom_element);
                }
            } else if let Some((parent_idx, insert_idx)) = self.find_foster_parent_location() {
                // Insert at foster parent location (before the table)
                let dom_element = element.clone_deep();
                element.is_parented = true;
                element.real_node_id = Some(dom_element.id);
                self.open_elements[parent_idx].children.insert(insert_idx, dom_element);
            }
        } else if current_is_placeholder {
            // Current node is a placeholder from adoption agency - redirect to real DOM node
            if let Some(target_id) = current_real_node_id {
                let dom_element = element.clone_deep();
                element.is_parented = true;
                element.real_node_id = Some(dom_element.id);
                if let Some(real_node) = self.find_real_node_mut(target_id) {
                    real_node.children.push(dom_element);
                }
            }
        } else if self.open_elements.is_empty() && name != "html" {
            // Stack is empty and we're NOT creating the initial html element
            // Find html element in document children and insert there
            // This matches Swift's adjustedInsertionTarget behavior
            let dom_element = element.clone_deep();
            element.is_parented = true;
            element.real_node_id = Some(dom_element.id);
            for child in &mut self.document.children {
                if child.name == "html" {
                    child.children.push(dom_element);
                    break;
                }
            }
            // If no html found, element just gets pushed to stack and will be
            // added to document when popped (normal behavior)
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

        // Get current node info before any mutable borrows
        let (current_name, current_real_node_id) = {
            if let Some(current) = self.open_elements.last() {
                (current.name.clone(), current.real_node_id)
            } else {
                (String::new(), None)
            }
        };

        // If current is a template with real_node_id, insert into the real template's content
        if current_name == "template" {
            if let Some(target_id) = current_real_node_id {
                // Find the real template node and insert into its content
                if let Some(real_template) = self.find_real_node_mut(target_id) {
                    if let Some(ref mut content) = real_template.template_content {
                        // Try to append to existing text node
                        if let Some(last_child) = content.children.last_mut() {
                            if let Some(NodeData::Text(ref mut existing)) = last_child.data {
                                existing.push(c);
                                return;
                            }
                        }
                        content.children.push(Node::text(&text));
                        return;
                    }
                }
            }
        }

        // Check if current node redirects to a real DOM node (foster parented or adoption agency)
        // This handles non-template elements with real_node_id
        if let Some(target_id) = current_real_node_id {
            if current_name != "template" {
                // Find the real node and add content there
                if let Some(real_node) = self.find_real_node_mut(target_id) {
                    // Try to append to existing text node
                    if let Some(last_child) = real_node.children.last_mut() {
                        if let Some(NodeData::Text(ref mut existing)) = last_child.data {
                            existing.push(c);
                            return;
                        }
                    }
                    real_node.children.push(Node::text(&text));
                    return;
                }
            }
        }

        // Foster parenting: insert at foster parent location (before the table)
        if self.foster_parenting {
            if let Some((parent_idx, table_child_idx)) = self.find_foster_parent_location() {
                let parent = &mut self.open_elements[parent_idx];
                // Try to append to existing text node before the insertion point
                if table_child_idx > 0 {
                    if let Some(prev_child) = parent.children.get_mut(table_child_idx - 1) {
                        if let Some(NodeData::Text(ref mut existing)) = prev_child.data {
                            existing.push(c);
                            return;
                        }
                    }
                }
                // Insert new text node at foster parent location
                parent.children.insert(table_child_idx, Node::text(&text));
            }
            return;
        }

        if let Some(current) = self.open_elements.last_mut() {
            // If current is a template, insert into its content
            if current.name == "template" {
                if let Some(ref mut content) = current.template_content {
                    // Try to append to existing text node
                    if let Some(last_child) = content.children.last_mut() {
                        if let Some(NodeData::Text(ref mut existing)) = last_child.data {
                            existing.push(c);
                            return;
                        }
                    }
                    content.children.push(Node::text(&text));
                    return;
                }
            }
            // Normal case: insert into current element
            if let Some(last_child) = current.children.last_mut() {
                if let Some(NodeData::Text(ref mut existing)) = last_child.data {
                    existing.push(c);
                    return;
                }
            }
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
                    let table_id = self.open_elements[i].id;
                    // Find the actual position of the table in the parent's children
                    // If table is at children[2], we want to insert at index 2 (before it)
                    let insert_idx = parent.children.iter()
                        .position(|c| c.id == table_id)
                        .unwrap_or(parent.children.len());
                    return Some((parent_idx, insert_idx));
                }
            }
        }
        None
    }

    fn insert_comment(&mut self, data: &str) {
        let comment = Node::comment(data);
        if let Some(current) = self.open_elements.last_mut() {
            // If current is a template, insert into its content
            if current.name == "template" {
                if let Some(ref mut content) = current.template_content {
                    content.children.push(comment);
                    return;
                }
            }
            current.children.push(comment);
        } else {
            self.document.children.push(comment);
        }
    }

    fn pop_current_element(&mut self) -> Option<Node> {
        self.open_elements.pop()
    }

    fn pop_elements_until(&mut self, tag_name: &str) {
        // Pop until we find an HTML element with the specified tag name
        // (Foreign elements with the same name don't count)
        while let Some(node) = self.open_elements.last() {
            let name = node.name.clone();
            let is_html = node.namespace == Some(Namespace::Html) || node.namespace.is_none();
            self.pop_and_add_to_parent();
            if is_html && name == tag_name {
                break;
            }
        }
    }

    fn pop_elements_until_one_of(&mut self, tags: &[&str]) {
        // Pop until we find an HTML element matching one of the specified tag names
        while let Some(node) = self.open_elements.last() {
            let name = node.name.clone();
            let is_html = node.namespace == Some(Namespace::Html) || node.namespace.is_none();
            self.pop_and_add_to_parent();
            if is_html && tags.contains(&name.as_str()) {
                break;
            }
        }
    }

    fn pop_elements_until_html_template(&mut self) {
        // Pop elements until we find an HTML template element (not SVG/MathML template)
        while let Some(node) = self.open_elements.last() {
            let name = node.name.clone();
            let is_html = node.namespace == Some(Namespace::Html) || node.namespace.is_none();
            self.pop_and_add_to_parent();
            if name == "template" && is_html {
                break;
            }
        }
    }

    fn pop_and_add_to_parent(&mut self) {
        if let Some(node) = self.open_elements.pop() {
            // Handle foster-parented elements: transfer children to the real DOM node
            // is_parented=true means this is a PLACEHOLDER (like for foster parenting)
            // and we should only transfer its children, not the node itself
            if node.is_parented && node.real_node_id.is_some() {
                if let Some(real_id) = node.real_node_id {
                    // Find the real DOM node and transfer children from the placeholder
                    if let Some(real_node) = self.find_real_node_mut(real_id) {
                        for child in node.children {
                            real_node.children.push(child);
                        }
                    }
                }
                return;
            }

            // If node has real_node_id but is NOT is_parented, it means we should add
            // this node as a child of real_node_id (e.g., form removal case)
            if let Some(target_id) = node.real_node_id {
                if !node.is_parented {
                    if let Some(real_node) = self.find_real_node_mut(target_id) {
                        real_node.children.push(node);
                        return;
                    }
                }
            }

            // Check if parent has real_node_id AND is_parented (meaning parent is a placeholder)
            // Only in that case should we redirect to the real DOM node
            let (should_redirect, parent_is_template) = self.open_elements.last()
                .map_or((false, false), |p| (p.is_parented && p.real_node_id.is_some(), p.name == "template"));

            if should_redirect {
                let target_id = self.open_elements.last().and_then(|p| p.real_node_id);
                if let Some(target_id) = target_id {
                    // Find the real node in the document tree and add there
                    if let Some(real_node) = self.find_real_node_mut(target_id) {
                        // If real node is a template, add to its template_content
                        if parent_is_template {
                            if let Some(ref mut content) = real_node.template_content {
                                content.children.push(node);
                                return;
                            }
                        }
                        real_node.children.push(node);
                        return;
                    }
                }
            }

            if let Some(parent) = self.open_elements.last_mut() {
                // If parent is a template, add to its content instead
                if parent.name == "template" {
                    if let Some(ref mut content) = parent.template_content {
                        content.children.push(node);
                        return;
                    }
                }
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
        // Per WHATWG spec, scope checking involves specific elements in specific namespaces
        static MATHML_SCOPE_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
            ["mi", "mo", "mn", "ms", "mtext", "annotation-xml"].into_iter().collect()
        });
        static SVG_SCOPE_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
            ["foreignObject", "desc", "title"].into_iter().collect()
        });

        for node in self.open_elements.iter().rev() {
            let is_html = node.namespace == Some(Namespace::Html) || node.namespace.is_none();
            let is_mathml = node.namespace == Some(Namespace::MathML);
            let is_svg = node.namespace == Some(Namespace::Svg);

            // Only match HTML elements for the target
            if is_html && node.name == tag_name {
                return true;
            }

            // Check scope boundaries based on namespace
            if is_html && scope_elements.contains(node.name.as_str()) {
                return false;
            }
            if is_mathml && MATHML_SCOPE_ELEMENTS.contains(node.name.as_str()) {
                return false;
            }
            if is_svg && SVG_SCOPE_ELEMENTS.contains(node.name.as_str()) {
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
        // Table scope only includes html, table, template as barriers
        // It does NOT include SVG foreignObject/desc/title or MathML elements
        for node in self.open_elements.iter().rev() {
            let is_html = node.namespace == Some(Namespace::Html) || node.namespace.is_none();

            // Only match HTML elements for the target
            // SVG/MathML elements with table-related names should NOT match
            if is_html && node.name == tag_name {
                return true;
            }

            // Only HTML table scope elements are barriers
            if is_html && TABLE_SCOPE_ELEMENTS.contains(node.name.as_str()) {
                return false;
            }
        }
        false
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
        if self.active_formatting_elements.is_empty() {
            return;
        }

        // Check if current node is a foster parented block element (special element)
        // In this case, formatting elements need to be reconstructed inside the block
        let current_is_foster_parented_block = self.open_elements.last()
            .map_or(false, |n| n.is_parented && SPECIAL_ELEMENTS.contains(n.name.as_str()));

        // Get the formatting ancestor IDs from the current block element
        // These are formatting elements whose DOM already contains this block, so they shouldn't be reconstructed
        let formatting_ancestors: Vec<u64> = self.open_elements.last()
            .map_or(Vec::new(), |n| n.formatting_ancestor_ids.clone());

        // Helper to check if a node is "active" (on stack, should not be reconstructed)
        // Per HTML5 spec: if the element is in open_elements, it's active
        // But if it's a foster parented placeholder AND the current block is also foster parented,
        // we need to check if the formatting element is a DOM ancestor of the block
        let is_active = |node: &Node, open_elements: &[Node], foster_block: bool, ancestors: &[u64]| -> bool {
            if node.is_parented {
                return true;
            }
            // Find matching element on stack by ID
            for stack_elem in open_elements.iter() {
                if stack_elem.id == node.id {
                    // If the formatting element is a DOM ancestor of the current block,
                    // it's active (don't reconstruct inside the block)
                    if ancestors.contains(&stack_elem.id) {
                        return true;
                    }
                    // If the stack element is a foster parented placeholder AND we're in
                    // a foster parented block that is NOT a child of this element,
                    // then reconstruct the formatting inside the block
                    if stack_elem.is_parented && foster_block && !ancestors.contains(&stack_elem.id) {
                        return false;
                    }
                    return true;
                }
            }
            false
        };

        // Check if last entry is a marker or already in open elements
        // But don't return early if an entry is just is_parented - we may still need to reconstruct earlier entries
        if let Some(last) = self.active_formatting_elements.last() {
            if last.is_none() {
                return;
            }
            if let Some(ref node) = last {
                // Only return early if the entry is truly active (not just is_parented from wrapper creation)
                // An is_parented entry from wrapper creation should be skipped, but earlier entries may need reconstruction
                if !node.is_parented && is_active(node, &self.open_elements, current_is_foster_parented_block, &formatting_ancestors) {
                    return;
                }
            }
        }

        // Step 4: Rewind - find where to start
        let mut entry_index = self.active_formatting_elements.len() - 1;
        loop {
            if entry_index == 0 {
                break;
            }
            entry_index -= 1;
            let entry = &self.active_formatting_elements[entry_index];
            if entry.is_none() {
                entry_index += 1;
                break;
            }
            if let Some(ref node) = entry {
                if is_active(node, &self.open_elements, current_is_foster_parented_block, &formatting_ancestors) {
                    entry_index += 1;
                    break;
                }
            }
        }

        // Step 7: Advance and create elements
        // When inside a foster-parented block, reverse the order (innermost first)
        // This matches how adoption agency restructures elements
        let indices: Vec<usize> = if current_is_foster_parented_block {
            // Collect indices that need reconstruction, then reverse
            let mut indices = Vec::new();
            let mut idx = entry_index;
            while idx < self.active_formatting_elements.len() {
                if let Some(ref entry) = self.active_formatting_elements[idx] {
                    if !entry.is_parented {
                        indices.push(idx);
                    }
                }
                idx += 1;
            }
            indices.into_iter().rev().collect()
        } else {
            (entry_index..self.active_formatting_elements.len()).collect()
        };

        for idx in indices {
            let entry_clone = self.active_formatting_elements[idx].clone();
            if let Some(entry) = entry_clone {
                // Skip entries that are already parented (from adoption agency)
                if entry.is_parented {
                    continue;
                }
                // Use insert_html_element to properly handle foster parenting
                self.insert_html_element(&entry.name, entry.attrs.clone());
                // Update the entry in active_formatting to have matching ID
                if let Some(new_elem) = self.open_elements.last() {
                    let new_id = new_elem.id;
                    let new_real_id = new_elem.real_node_id;
                    if let Some(ref mut active_entry) = self.active_formatting_elements[idx] {
                        active_entry.id = new_id;
                        // Also track the real DOM node ID for foster parented elements
                        if new_real_id.is_some() {
                            active_entry.real_node_id = new_real_id;
                        }
                    }
                }
            }
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
        // Noah's Ark clause: if there are already 3 matching elements after the last marker,
        // remove the earliest one
        let mut matching_count = 0;
        let mut earliest_match_idx: Option<usize> = None;
        for (i, entry) in self.active_formatting_elements.iter().enumerate().rev() {
            match entry {
                None => break, // Hit marker
                Some(node) if node.name == name && node.attrs == attrs => {
                    matching_count += 1;
                    earliest_match_idx = Some(i);
                }
                _ => {}
            }
        }
        if matching_count >= 3 {
            if let Some(idx) = earliest_match_idx {
                self.active_formatting_elements.remove(idx);
            }
        }

        // Get the ID from the element we just pushed to open_elements
        let id = self.open_elements.last().map(|n| n.id).unwrap_or(0);
        let mut element = Node::element(name, attrs);
        element.id = id;
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

    /// Close formatting elements that were interrupted by a table.
    /// These are elements on the stack but no longer in AFE (removed when adoption agency failed).
    fn close_interrupted_formatting_elements(&mut self) {
        // Pop formatting elements from the stack that are not in AFE
        // Stop at body, html, or when we hit a non-formatting element
        while let Some(node) = self.open_elements.last() {
            let name = node.name.clone();
            // Stop if we hit body, html, or template
            if name == "body" || name == "html" || name == "template" {
                break;
            }
            // Only close formatting elements
            if !FORMATTING_ELEMENTS.contains(name.as_str()) {
                break;
            }
            // Check if this element is in AFE (by comparing IDs)
            let node_id = node.id;
            let in_afe = self.active_formatting_elements.iter().any(|e| {
                e.as_ref().map_or(false, |n| n.id == node_id)
            });
            // If it's in AFE, don't close it
            if in_afe {
                break;
            }
            // Pop this interrupted formatting element
            self.pop_and_add_to_parent();
        }
    }

    /// Returns true if the element was processed (and thus removed from stack/AFE)
    fn adoption_agency(&mut self, name: &str) -> bool {
        // Step 1: If current node is the subject and not in active formatting, just pop it
        if let Some(current) = self.current_node() {
            if current.name == name && !self.has_active_formatting_entry(name) {
                self.pop_elements_until(name);
                return true;
            }
        }

        // Step 2: Outer loop (max 8 iterations)
        for _outer in 0..8 {
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
                return false;
            };

            // Get the formatting element's info from active formatting
            let fe_entry = match &self.active_formatting_elements[fe_active_idx] {
                Some(n) => n.clone(),
                None => return false,
            };
            let fe_name = fe_entry.name.clone();
            let fe_attrs = fe_entry.attrs.clone();
            let fe_namespace = fe_entry.namespace;
            let fe_id = fe_entry.id;

            // Step 4: Find formatting element in open elements by ID
            // Per WHATWG spec: we're looking for the specific element from AFE.
            // If that exact element is not on the stack, remove from AFE and return.
            // Don't fall back to finding by name - that would process a different element.
            let fe_stack_idx = self.open_elements.iter()
                .position(|n| n.id == fe_id);

            let Some(fe_stack_idx) = fe_stack_idx else {
                // Formatting element not in open elements - remove from active formatting
                self.error("adoption-agency-1.3");
                self.active_formatting_elements.remove(fe_active_idx);
                return true;
            };

            // Step 5: Check if formatting element is in scope
            if !self.has_element_in_scope(&fe_name) {
                self.error("adoption-agency-1.3");
                return false;
            }

            // Step 6: If formatting element is not current node, emit error
            if fe_stack_idx != self.open_elements.len() - 1 {
                self.error("adoption-agency-1.3");
            }

            // Step 7: Find furthest block (first special element after formatting element)
            // Only HTML namespace elements can be furthest blocks - foreign content elements don't count
            let mut furthest_block_idx: Option<usize> = None;
            for i in (fe_stack_idx + 1)..self.open_elements.len() {
                let elem = &self.open_elements[i];
                if elem.namespace == Some(Namespace::Html) && SPECIAL_ELEMENTS.contains(elem.name.as_str()) {
                    furthest_block_idx = Some(i);
                    break;
                }
            }

            // If the formatting element is a placeholder (from a previous adoption agency iteration),
            // the stack contains placeholders but the real DOM manipulation was already done.
            // We need to handle this by directly manipulating the DOM using the real nodes.
            let fe_is_placeholder = self.open_elements[fe_stack_idx].is_parented;
            let fe_real_node_id = self.open_elements[fe_stack_idx].real_node_id;

            if fe_is_placeholder && fe_real_node_id.is_some() && furthest_block_idx.is_some() {
                // The formatting element is a placeholder - do DOM manipulation on real nodes
                let fb_idx = furthest_block_idx.unwrap();
                let fb_name = self.open_elements[fb_idx].name.clone();

                let fe_real_id = fe_real_node_id.unwrap();
                let fb_real_id = self.open_elements[fb_idx].real_node_id;
                let common_ancestor_idx = fe_stack_idx - 1;
                let common_ancestor_name = self.open_elements[common_ancestor_idx].name.clone();
                let common_ancestor_is_table_related = matches!(common_ancestor_name.as_str(), "table" | "tbody" | "tfoot" | "thead" | "tr");

                // fb is either a placeholder (with real_node_id) or a regular node in the DOM
                let actual_fb_id = fb_real_id.unwrap_or_else(|| self.open_elements[fb_idx].id);

                // Step 11: Remove fb from fe and add to common ancestor (or foster parent)
                let mut fb_node_to_move: Option<Node> = None;

                // Find and extract fb from fe's children in the DOM
                if let Some(fe_real) = self.find_real_node_mut(fe_real_id) {
                    if let Some(idx) = fe_real.children.iter().position(|c| c.id == actual_fb_id) {
                        fb_node_to_move = Some(fe_real.children.remove(idx));
                    }
                }

                // If fb wasn't found in fe's children and fb_real_id is None,
                // this is the first iteration (foster parenting case) - fall through to normal algorithm
                // The placeholder branch is only for subsequent iterations where fb was already restructured
                if fb_node_to_move.is_none() && fb_real_id.is_none() {
                    // Fall through to normal algorithm below
                }

                // If we found and removed fb, add it to common ancestor or foster parent
                if let Some(mut fb_node) = fb_node_to_move {
                    // Create new formatting element
                    let mut new_fe = Node::element_ns(&fe_name, fe_namespace.unwrap_or(Namespace::Html), fe_attrs.clone());

                    // Move fb's children to new_fe
                    new_fe.children = std::mem::take(&mut fb_node.children);

                    // Add new_fe to fb
                    let new_fe_id = new_fe.id;
                    fb_node.children.push(new_fe.clone());

                    // Determine where to insert fb
                    if self.foster_parenting && common_ancestor_is_table_related {
                        // Foster parent the restructured fb
                        if let Some((parent_idx, insert_idx)) = self.find_foster_parent_location() {
                            self.open_elements[parent_idx].children.insert(insert_idx, fb_node);
                        }
                    } else {
                        // Add fb to common ancestor (or its real DOM location if it's a placeholder)
                        let ca_real_id = self.open_elements[common_ancestor_idx].real_node_id;
                        if let Some(ca_real_id) = ca_real_id {
                            if let Some(real_node) = self.find_real_node_mut(ca_real_id) {
                                real_node.children.push(fb_node);
                            }
                        } else {
                            self.open_elements[common_ancestor_idx].children.push(fb_node);
                        }
                    }

                    // Update AFE
                    self.active_formatting_elements.remove(fe_active_idx);
                    let new_bookmark = fe_active_idx.min(self.active_formatting_elements.len());
                    self.active_formatting_elements.insert(new_bookmark, Some(new_fe));

                    // Save elements after fb before popping (they need to stay on stack for next iteration)
                    let mut elements_after_fb_placeholder: Vec<Node> = Vec::new();
                    while self.open_elements.len() > fb_idx + 1 {
                        elements_after_fb_placeholder.push(self.open_elements.pop().unwrap());
                    }
                    elements_after_fb_placeholder.reverse();

                    // Update stack: pop everything from fb onwards (fe was already marked, fb was already cloned)
                    while self.open_elements.len() > fe_stack_idx {
                        self.pop_and_add_to_parent();
                    }

                    // Push new placeholders
                    {
                        let mut fb_placeholder = Node::new(&fb_name);
                        fb_placeholder.is_parented = true;
                        fb_placeholder.real_node_id = Some(actual_fb_id);
                        self.open_elements.push(fb_placeholder);
                    }
                    {
                        let mut new_fe_placeholder = Node::element(&fe_name, fe_attrs.clone());
                        new_fe_placeholder.is_parented = true;
                        new_fe_placeholder.real_node_id = Some(new_fe_id);
                        if let Some(ref mut af_entry) = self.active_formatting_elements.get_mut(new_bookmark) {
                            if let Some(ref mut node) = af_entry {
                                node.id = new_fe_placeholder.id;
                                node.is_parented = true;  // Mark AFE entry so reconstruct skips it
                                node.real_node_id = Some(new_fe_id);  // Track real node
                            }
                        }
                        self.open_elements.push(new_fe_placeholder);
                    }

                    // Push back elements that were after fb
                    for elem in elements_after_fb_placeholder {
                        self.open_elements.push(elem);
                    }

                    continue; // Continue outer loop only if we did DOM manipulation
                }
                // If fb wasn't found, fall through to normal algorithm
            }

            // Step 8: If no furthest block, pop to formatting element and remove from active formatting
            let Some(fb_idx) = furthest_block_idx else {
                while self.open_elements.len() > fe_stack_idx {
                    self.pop_and_add_to_parent();
                }
                self.active_formatting_elements.remove(fe_active_idx);
                return true;
            };

            // Step 9: Common ancestor (element before formatting element)
            if fe_stack_idx == 0 {
                self.active_formatting_elements.remove(fe_active_idx);
                return true;
            }
            let common_ancestor_idx = fe_stack_idx - 1;

            // Track if the formatting element was foster parented (before we modify the stack)
            let fe_was_foster_parented = self.open_elements[fe_stack_idx].is_parented;

            // Step 10: bookmark - where to insert new formatting element entry
            let mut bookmark = fe_active_idx + 1;

            // Pop elements from stack end to fb+1 (elements after furthest block)
            let mut elements_after_fb: Vec<Node> = Vec::new();
            while self.open_elements.len() > fb_idx + 1 {
                elements_after_fb.push(self.open_elements.pop().unwrap());
            }
            elements_after_fb.reverse();

            // Pop furthest block
            let furthest_block_placeholder = self.open_elements.pop().unwrap();

            // Track whether the original FB was a placeholder (from a previous iteration)
            let fb_was_placeholder = furthest_block_placeholder.is_parented;

            // If the furthest block is a placeholder (is_parented=true with real_node_id),
            // we need to find the real node in the DOM and remove it from its current parent.
            // Otherwise, the real node would stay in the DOM and we'd have duplicates.
            let furthest_block = if furthest_block_placeholder.is_parented {
                if let Some(real_id) = furthest_block_placeholder.real_node_id {
                    // Find and extract the real node from its current parent
                    let mut extracted: Option<Node> = None;

                    // Search through all open elements' children
                    for elem in self.open_elements.iter_mut() {
                        if let Some(idx) = elem.children.iter().position(|c| c.id == real_id) {
                            extracted = Some(elem.children.remove(idx));
                            break;
                        }
                        // Also search in nested children
                        if extracted.is_none() {
                            extracted = Self::extract_node_by_id(&mut elem.children, real_id);
                        }
                        if extracted.is_some() {
                            break;
                        }
                    }

                    extracted.unwrap_or(furthest_block_placeholder)
                } else {
                    furthest_block_placeholder
                }
            } else {
                furthest_block_placeholder
            };

            // Step 11-12: Inner loop simulation
            // Walk backwards from furthest block toward formatting element
            // Collect info about elements that will form the new chain
            let mut elements_between: Vec<Node> = Vec::new();
            // Store the actual new elements (same ones go in AFE and DOM chain)
            let mut new_chain_elements: Vec<Node> = Vec::new();
            let mut inner_loop_counter = 0;

            while self.open_elements.len() > fe_stack_idx + 1 {
                inner_loop_counter += 1;
                let node = self.open_elements.pop().unwrap();
                let node_name = node.name.clone();
                let node_attrs = node.attrs.clone();
                let node_ns = node.namespace;
                let node_id = node.id;

                // Find this node in active formatting by ID (not just name)
                // This is critical when there are multiple elements with the same name
                let node_formatting_idx = self.active_formatting_elements.iter()
                    .position(|e| e.as_ref().map_or(false, |n| n.id == node_id));

                // If node is not in active formatting, just remove from stack (already done via pop)
                let Some(nf_idx) = node_formatting_idx else {
                    elements_between.push(node);
                    continue;
                };

                // If inner loop counter > 3, remove from active formatting
                if inner_loop_counter > 3 {
                    self.active_formatting_elements.remove(nf_idx);
                    if nf_idx < bookmark {
                        bookmark -= 1;
                    }
                    elements_between.push(node);
                    continue;
                }

                // Create a new element (clone) and replace in active formatting
                // This SAME element will be used in both AFE and the DOM chain
                let new_elem = Node::element_ns(&node_name, node_ns.unwrap_or(Namespace::Html), node_attrs.clone());
                self.active_formatting_elements[nf_idx] = Some(new_elem.clone());

                // Track the new element for building the chain and stack placeholders
                new_chain_elements.push(new_elem);

                // The original node becomes part of the original chain (with its children)
                elements_between.push(node);
            }

            // Pop formatting element
            let mut formatting_element = self.open_elements.pop().unwrap();

            // Build the original chain: fe > first_between > ... > last_between
            // These elements KEEP their children (e.g., "2" in <i>)
            // elements_between is in reverse order (closest to fb first), so reverse it
            {
                let mut current = &mut formatting_element;
                for elem in elements_between.into_iter().rev() {
                    // Keep the element's children - they contain content added during parsing
                    current.children.push(elem);
                    current = current.children.last_mut().unwrap();
                }
            }

            // Add formatting element to common ancestor
            // But skip this if fe was a placeholder - it's already in the DOM
            if !fe_is_placeholder {
                if self.open_elements[common_ancestor_idx].name == "template" {
                    if let Some(ref mut content) = self.open_elements[common_ancestor_idx].template_content {
                        content.children.push(formatting_element);
                    }
                } else {
                    self.open_elements[common_ancestor_idx].children.push(formatting_element);
                }
            }

            // Build the new chain using the elements from the inner loop
            // new_chain_elements is in order: closest to fb first (innermost)
            // So first element wraps the furthest block, last element is outermost
            // Chain: outermost > ... > innermost > furthest_block

            // Save furthest block's name for later stack placeholder
            let fb_name = furthest_block.name.clone();

            let mut current_chain = furthest_block;

            // Collect info for placeholders BEFORE consuming new_chain_elements
            // We need: (real_node_id, name, attrs) for each element
            let chain_placeholder_info: Vec<(u64, String, HashMap<String, String>)> =
                new_chain_elements.iter()
                    .map(|e| (e.id, e.name.clone(), e.attrs.clone()))
                    .collect();

            // Wrap furthest block with the new elements (innermost first)
            // Move elements instead of cloning to preserve IDs
            for mut new_elem in new_chain_elements.into_iter() {
                new_elem.children.push(current_chain);
                current_chain = new_elem;
            }

            // Create new formatting element, take furthest_block's children
            let mut new_fe = Node::element_ns(&fe_name, fe_namespace.unwrap_or(Namespace::Html), fe_attrs.clone());

            // Find the actual furthest block inside the chain (the special element)
            // and move its children to new_fe. Also capture IDs for stack placeholders.
            let mut fb_real_id: Option<u64> = None;
            let new_fe_id = new_fe.id;  // Capture ID before moving


            {
                let mut current = &mut current_chain;
                loop {
                    if SPECIAL_ELEMENTS.contains(current.name.as_str()) {
                        // Found the furthest block - capture its ID for the stack placeholder
                        fb_real_id = Some(current.id);
                        // Move its children to new_fe (these keep their original IDs)
                        new_fe.children = std::mem::take(&mut current.children);
                        // Move new_fe into furthest block (DOM now has original IDs)
                        current.children.push(new_fe);
                        break;
                    }
                    if current.children.is_empty() {
                        break;
                    }
                    // Move to the last child
                    current = current.children.last_mut().unwrap();
                }
            }

            // Create a fresh new_fe for AFE with the same ID
            let mut new_fe = Node::element_ns(&fe_name, fe_namespace.unwrap_or(Namespace::Html), fe_attrs.clone());
            new_fe.id = new_fe_id;

            // Add the new chain (with furthest_block inside) to common ancestor
            // If the formatting element was foster parented, add to foster parent location instead
            if fe_was_foster_parented && self.foster_parenting {
                // Add to foster parent location (before the table)
                if let Some((parent_idx, insert_idx)) = self.find_foster_parent_location() {
                    self.open_elements[parent_idx].children.insert(insert_idx, current_chain);
                }
            } else if self.open_elements[common_ancestor_idx].name == "template" {
                if let Some(ref mut content) = self.open_elements[common_ancestor_idx].template_content {
                    content.children.push(current_chain);
                }
            } else {
                // Check if the common ancestor is a placeholder that redirects to a real DOM node
                let ca_real_node_id = self.open_elements[common_ancestor_idx].real_node_id;
                if let Some(target_id) = ca_real_node_id {
                    // Find the real node in the document tree and add there
                    if let Some(real_node) = self.find_real_node_mut(target_id) {
                        real_node.children.push(current_chain);
                    }
                } else {
                    self.open_elements[common_ancestor_idx].children.push(current_chain);
                }
            }

            // Update active formatting list
            self.active_formatting_elements.remove(fe_active_idx);
            let bookmark = bookmark.saturating_sub(1).min(self.active_formatting_elements.len());
            self.active_formatting_elements.insert(bookmark, Some(new_fe.clone()));

            // Push placeholders for the chain elements onto the stack
            // chain_placeholder_info is innermost first, but for the stack we need outermost first
            // The outermost element (last in chain_placeholder_info) should be lowest on stack
            for (real_id, name, attrs) in chain_placeholder_info.iter().rev() {
                let mut placeholder = Node::element(name, attrs.clone());
                placeholder.is_parented = true;
                placeholder.real_node_id = Some(*real_id);
                // Update the AFE entry's ID and is_parented flag to match the placeholder
                for afe_entry in self.active_formatting_elements.iter_mut() {
                    if let Some(ref mut node) = afe_entry {
                        if node.id == *real_id {
                            node.id = placeholder.id;
                            node.is_parented = true;  // Mark AFE entry so reconstruct skips it
                            node.real_node_id = Some(*real_id);  // Track real node
                            break;
                        }
                    }
                }
                self.open_elements.push(placeholder);
            }

            // Create a placeholder for the furthest block on the stack
            // This placeholder redirects content to the real fb in the DOM
            {
                let mut fb_node = Node::new(&fb_name);
                fb_node.is_parented = true;  // Already in the DOM
                fb_node.real_node_id = fb_real_id;  // Redirect content to real fb
                self.open_elements.push(fb_node);
            }

            // Step 17: Push new_fe onto the stack (as a placeholder since the real one is in the DOM)
            {
                let mut new_fe_placeholder = Node::element(&fe_name, fe_attrs.clone());
                new_fe_placeholder.is_parented = true;  // Already in the DOM via current_chain
                new_fe_placeholder.real_node_id = Some(new_fe_id);  // Redirect content to real new_fe in DOM
                // Update the ID and is_parented in active_formatting to match this placeholder
                if let Some(ref mut af_entry) = self.active_formatting_elements.get_mut(bookmark) {
                    if let Some(ref mut node) = af_entry {
                        node.id = new_fe_placeholder.id;
                        node.is_parented = true;  // Mark AFE entry so reconstruct skips it
                        node.real_node_id = Some(new_fe_id);  // Track real node
                    }
                }
                self.open_elements.push(new_fe_placeholder);
            }

            // Push back elements that were after furthest block
            // For non-placeholder cases, they stay on the stack normally
            // For placeholder cases (like foster parenting), they need to be added to the DOM
            for mut elem in elements_after_fb.into_iter() {
                // Only convert to placeholders if we're working with placeholder-based restructuring
                // This happens when fb was also a placeholder (from a previous iteration)
                if fb_was_placeholder {
                    // If element is already a placeholder (from reconstruction), it's already in the DOM
                    // We just need to update its real_node_id to point to the right place
                    if elem.is_parented && elem.real_node_id.is_some() {
                        // Element is already in the DOM, no need to clone/add again
                        // Just push the placeholder back to the stack
                    } else {
                        // The element should be added to new_fe in the DOM
                        if let Some(new_fe_in_dom) = self.find_real_node_mut(new_fe_id) {
                            let elem_id = elem.id;
                            new_fe_in_dom.children.push(elem.clone());
                            // Convert the stack element to a placeholder
                            elem.is_parented = true;
                            elem.real_node_id = Some(elem_id);
                            elem.children.clear();
                        }
                    }
                }
                self.open_elements.push(elem);
            }
        }
        // If we completed all iterations of the outer loop, we processed it
        true
    }

    fn any_other_end_tag(&mut self, name: &str) {
        // In fragment mode, don't pop below the context element
        let min_stack_size = if self.fragment_context.is_some() { 1 } else { 0 };

        for i in (0..self.open_elements.len()).rev() {
            if self.open_elements[i].name == name {
                self.generate_implied_end_tags_except(Some(name));
                while self.open_elements.len() > i && self.open_elements.len() > min_stack_size {
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
                // Determine quirks mode based on doctype per WHATWG spec
                self.quirks_mode = self.should_be_quirks_mode(&doctype);
                let node = Node::doctype(doctype);
                self.document.children.push(node);
                self.insertion_mode = InsertionMode::BeforeHtml;
            }
            _ => {
                // Ignore spurious doctypes
            }
        }
    }

    fn should_be_quirks_mode(&self, doctype: &Doctype) -> bool {
        // Per WHATWG spec section 13.2.6.4.1

        // If force-quirks is set, quirks mode
        if doctype.force_quirks {
            return true;
        }

        // If name is not "html" (case-insensitive), quirks mode
        let name = doctype.name.as_ref().map(|s| s.to_ascii_lowercase());
        if name.as_deref() != Some("html") {
            return true;
        }

        // Check public identifier
        if let Some(ref public_id) = doctype.public_id {
            let public_lower = public_id.to_ascii_lowercase();

            // Non-standard public IDs that don't look like proper FPIs trigger quirks
            // Standard FPIs start with +// or -// (formal public identifier format)
            if !public_lower.starts_with("+//") && !public_lower.starts_with("-//") {
                return true;
            }

            // Quirks mode public IDs (starts with)
            static QUIRKS_PUBLIC_ID_PREFIXES: &[&str] = &[
                "+//silmaril//dtd html pro v0r11 19970101//",
                "-//as//dtd html 3.0 aswedit + extensions//",
                "-//advasoft ltd//dtd html 3.0 aswedit + extensions//",
                "-//ietf//dtd html 2.0 level 1//",
                "-//ietf//dtd html 2.0 level 2//",
                "-//ietf//dtd html 2.0 strict level 1//",
                "-//ietf//dtd html 2.0 strict level 2//",
                "-//ietf//dtd html 2.0 strict//",
                "-//ietf//dtd html 2.0//",
                "-//ietf//dtd html 2.1e//",
                "-//ietf//dtd html 3.0//",
                "-//ietf//dtd html 3.2 final//",
                "-//ietf//dtd html 3.2//",
                "-//ietf//dtd html 3//",
                "-//ietf//dtd html level 0//",
                "-//ietf//dtd html level 1//",
                "-//ietf//dtd html level 2//",
                "-//ietf//dtd html level 3//",
                "-//ietf//dtd html strict level 0//",
                "-//ietf//dtd html strict level 1//",
                "-//ietf//dtd html strict level 2//",
                "-//ietf//dtd html strict level 3//",
                "-//ietf//dtd html strict//",
                "-//ietf//dtd html//",
                "-//metrius//dtd metrius presentational//",
                "-//microsoft//dtd internet explorer 2.0 html strict//",
                "-//microsoft//dtd internet explorer 2.0 html//",
                "-//microsoft//dtd internet explorer 2.0 tables//",
                "-//microsoft//dtd internet explorer 3.0 html strict//",
                "-//microsoft//dtd internet explorer 3.0 html//",
                "-//microsoft//dtd internet explorer 3.0 tables//",
                "-//netscape comm. corp.//dtd html//",
                "-//netscape comm. corp.//dtd strict html//",
                "-//o'reilly and associates//dtd html 2.0//",
                "-//o'reilly and associates//dtd html extended 1.0//",
                "-//o'reilly and associates//dtd html extended relaxed 1.0//",
                "-//sq//dtd html 2.0 hotmetal + extensions//",
                "-//softquad software//dtd hotmetal pro 6.0::19990601::extensions to html 4.0//",
                "-//softquad//dtd hotmetal pro 4.0::19971010::extensions to html 4.0//",
                "-//spyglass//dtd html 2.0 extended//",
                "-//sun microsystems corp.//dtd hotjava html//",
                "-//sun microsystems corp.//dtd hotjava strict html//",
                "-//w3c//dtd html 3 1995-03-24//",
                "-//w3c//dtd html 3.2 draft//",
                "-//w3c//dtd html 3.2 final//",
                "-//w3c//dtd html 3.2//",
                "-//w3c//dtd html 3.2s draft//",
                "-//w3c//dtd html 4.0 frameset//",
                "-//w3c//dtd html 4.0 transitional//",
                "-//w3c//dtd html experimental 19960712//",
                "-//w3c//dtd html experimental 970421//",
                "-//w3c//dtd w3 html//",
                "-//w3o//dtd w3 html 3.0//",
                "-//webtechs//dtd mozilla html 2.0//",
                "-//webtechs//dtd mozilla html//",
            ];

            for prefix in QUIRKS_PUBLIC_ID_PREFIXES {
                if public_lower.starts_with(prefix) {
                    return true;
                }
            }

            // Quirks if no system ID and public ID starts with these
            if doctype.system_id.is_none() {
                static QUIRKS_NO_SYSTEM_PREFIXES: &[&str] = &[
                    "-//w3c//dtd html 4.01 frameset//",
                    "-//w3c//dtd html 4.01 transitional//",
                ];
                for prefix in QUIRKS_NO_SYSTEM_PREFIXES {
                    if public_lower.starts_with(prefix) {
                        return true;
                    }
                }
            }
        }

        // Check system identifier for quirks mode
        if let Some(ref system_id) = doctype.system_id {
            let system_lower = system_id.to_ascii_lowercase();
            if system_lower == "http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd" {
                return true;
            }
        }

        // Not quirks mode
        false
    }

    fn process_start_tag(&mut self, name: &str, attrs: HashMap<String, String>, self_closing: bool) {
        // Check for foreign content handling first
        // Use the token-aware check per WHATWG spec
        if self.should_process_start_tag_in_foreign_content(name) {
            if self.process_start_tag_in_foreign_content(name, attrs.clone(), self_closing) {
                return;
            }
        }

        match self.insertion_mode {
            InsertionMode::Initial => {
                // Missing doctype triggers quirks mode
                self.quirks_mode = true;
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

                                self.insertion_mode = InsertionMode::InHead;
                                self.process_start_tag(name, attrs, self_closing);

                                // Set original_insertion_mode to AfterHead AFTER process_start_tag
                                // because InHead style/script handler overwrites it to InHead.
                                // We want Text mode to return to AfterHead, not InHead.
                                if self.insertion_mode == InsertionMode::Text {
                                    self.original_insertion_mode = InsertionMode::AfterHead;
                                }

                                // Remove head from stack (it might not be at the top)
                                // Elements inserted after head need to be recorded as children of head
                                if let Some(head_pos) = self.open_elements.iter().position(|n| n.name == "head") {
                                    // Remove head and elements after it
                                    let mut removed: Vec<Node> = self.open_elements.drain(head_pos..).collect();

                                    // First element is head, rest are elements inserted after head
                                    let mut head = removed.remove(0);
                                    let mut elements_after: Vec<Node> = removed;

                                    // Add elements as children of head and set up real_node_id
                                    // so content inserted into stack copy goes to DOM copy
                                    for elem in &mut elements_after {
                                        let mut dom_node = elem.clone_deep();
                                        // Give DOM copy a new unique id
                                        dom_node.id = generate_node_id();
                                        let dom_id = dom_node.id;
                                        head.children.push(dom_node);

                                        // Mark stack copy to redirect content to DOM copy
                                        elem.is_parented = true;
                                        elem.real_node_id = Some(dom_id);
                                    }

                                    // Put head back in html.children
                                    if let Some(html) = self.open_elements.first_mut() {
                                        html.children.insert(head_idx, head);
                                    }

                                    // Put elements back on the stack (they're still open)
                                    for elem in elements_after {
                                        self.open_elements.push(elem);
                                    }
                                }

                                // Restore insertion mode to AfterHead unless we're in Text mode
                                // or InTemplate mode (template needs to stay in InTemplate)
                                // (Text mode will return to AfterHead via original_insertion_mode)
                                if self.insertion_mode != InsertionMode::Text &&
                                   self.insertion_mode != InsertionMode::InTemplate {
                                    self.insertion_mode = InsertionMode::AfterHead;
                                }
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
                self.insertion_mode = self.original_insertion_mode;
                self.process_start_tag(name, attrs, self_closing);
            }
            InsertionMode::InCaption => {
                match name {
                    "caption" | "col" | "colgroup" | "table" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr" => {
                        // Per spec: close caption, switch to InTable, reprocess
                        // But in fragment mode with caption context, don't close the context element
                        let is_caption_fragment = self.fragment_context.as_ref()
                            .map_or(false, |ctx| ctx.tag_name == "caption");
                        if is_caption_fragment && self.open_elements.len() == 1 {
                            // In caption fragment mode, just process in body (insert as child)
                            self.process_in_body_start_tag(name, attrs, self_closing);
                        } else if self.has_element_in_table_scope("caption") {
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
                        // Per html5lib behavior: switch to InBody mode before processing
                        // This allows subsequent framesets to be ignored if frameset_ok is false
                        self.insertion_mode = InsertionMode::InBody;
                        self.process_start_tag(name, attrs, self_closing);
                        return;
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
                // In template fragment context, if we only have the context template on stack,
                // skip creating another template element (the input's <template> is the context element)
                let is_template_fragment = self.fragment_context.as_ref()
                    .map_or(false, |ctx| ctx.tag_name == "template");
                let only_has_context_template = self.open_elements.len() == 1 &&
                    self.open_elements.first().map_or(false, |n| n.name == "template");

                if is_template_fragment && only_has_context_template {
                    // Skip - this <template> represents the context element
                    return;
                }

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
                // Per spec: if there's a template on the stack, ignore the token
                if self.has_element_in_scope("template") {
                    return;
                }
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
                // Per spec: if there's a template on the stack, ignore the token
                if self.has_element_in_scope("template") {
                    return;
                }
                // Also check if second element is body
                if self.open_elements.get(1).map_or(true, |n| n.name != "body") {
                    return;
                }
                // Set frameset-ok to "not ok" per WHATWG spec
                self.frameset_ok = false;
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
                // Per WHATWG spec:
                // 1. If stack has only html, or second element is not body, ignore
                // 2. If frameset-ok is false, ignore
                // 3. Otherwise, remove body from parent, pop all except html, insert frameset
                if self.open_elements.len() <= 1 {
                    return; // Only html on stack
                }
                if self.open_elements.get(1).map_or(true, |n| n.name != "body") {
                    return; // Second element is not body
                }
                if !self.frameset_ok {
                    return; // frameset-ok flag is false
                }
                // Remove body element from its parent (html)
                if self.open_elements.len() > 1 {
                    let body = self.open_elements.remove(1);
                    // Don't add body back to html - it's being removed
                    // The body is just discarded
                }
                // Pop all elements from stack except html
                while self.open_elements.len() > 1 {
                    self.open_elements.pop();
                }
                // Insert frameset element
                self.insert_html_element(name, attrs);
                self.insertion_mode = InsertionMode::InFrameset;
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
                // Check for existing a element in active formatting (up to the last marker)
                let has_a = self.active_formatting_elements.iter().rev()
                    .take_while(|e| e.is_some())
                    .any(|e| e.as_ref().map_or(false, |n| n.name == "a"));

                if has_a {
                    self.error("unexpected-anchor-in-anchor");
                    // Run adoption agency for the existing anchor
                    let processed = self.adoption_agency("a");
                    // Only manually remove from AFE if adoption_agency didn't already handle it
                    // Don't remove from stack - the element should stay open for proper nesting
                    // (especially in foster parenting contexts where table breaks scope)
                    if !processed {
                        // Remove the anchor from active formatting if still present
                        if let Some(idx) = self.active_formatting_elements.iter().rposition(
                            |e| e.as_ref().map_or(false, |n| n.name == "a")
                        ) {
                            self.active_formatting_elements.remove(idx);
                        }
                        // NOTE: We intentionally don't remove from the stack here.
                        // If adoption agency failed (e.g., due to scope issues in table context),
                        // the element should stay on the stack so content is properly nested.
                    }
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
                    self.adoption_agency("nobr");
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
                // In quirks mode, don't close <p> before table
                if !self.quirks_mode && self.has_element_in_button_scope("p") {
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
                // Apply MathML attribute adjustments
                let adjusted_attrs: HashMap<String, String> = attrs.into_iter()
                    .map(|(k, v)| {
                        let key_lower = k.to_ascii_lowercase();
                        let adjusted_key = MATHML_ATTRIBUTE_ADJUSTMENTS.get(key_lower.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or(key_lower);
                        (adjusted_key, v)
                    })
                    .collect();
                // Create MathML element with foster parenting support
                let mut element = Node::element_ns(name, Namespace::MathML, adjusted_attrs);
                if self.foster_parenting {
                    if let Some((parent_idx, insert_idx)) = self.find_foster_parent_location() {
                        let dom_element = element.clone_deep();
                        element.is_parented = true;
                        element.real_node_id = Some(dom_element.id);
                        self.open_elements[parent_idx].children.insert(insert_idx, dom_element);
                    }
                }
                self.open_elements.push(element);
                if self_closing {
                    self.pop_and_add_to_parent();
                }
            }
            "svg" => {
                self.reconstruct_active_formatting_elements();
                // Apply SVG attribute adjustments
                let adjusted_attrs: HashMap<String, String> = attrs.into_iter()
                    .map(|(k, v)| {
                        let adjusted_key = SVG_ATTRIBUTE_ADJUSTMENTS.get(k.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or(k);
                        (adjusted_key, v)
                    })
                    .collect();
                // Create SVG element with foster parenting support
                let mut element = Node::element_ns(name, Namespace::Svg, adjusted_attrs);
                if self.foster_parenting {
                    if let Some((parent_idx, insert_idx)) = self.find_foster_parent_location() {
                        let dom_element = element.clone_deep();
                        element.is_parented = true;
                        element.real_node_id = Some(dom_element.id);
                        self.open_elements[parent_idx].children.insert(insert_idx, dom_element);
                    }
                }
                self.open_elements.push(element);
                if self_closing {
                    self.pop_and_add_to_parent();
                }
            }
            "col" | "colgroup" | "frame" | "head" => {
                // These are always ignored in InBody mode per WHATWG spec
                self.error("unexpected-element-in-body");
            }
            "caption" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr" => {
                self.error("unexpected-table-element-in-body");

                // Edge case: if the stack contains foreign elements (SVG/MathML), these table
                // elements should be inserted at the html level rather than ignored.
                // This handles cases like: <!><svg><th><title><n><select><td>
                // where <td> exits the select but has no table context.
                let has_foreign = self.open_elements.iter().any(|el| {
                    matches!(el.namespace, Some(Namespace::Svg) | Some(Namespace::MathML))
                });

                if has_foreign {
                    // Pop everything except html, adding to parents as we go
                    while self.open_elements.len() > 1 {
                        self.pop_and_add_to_parent();
                    }

                    // Create the new element
                    let mut element = Node::new(name);
                    for (attr_name, attr_value) in attrs {
                        element.attrs.insert(attr_name.clone(), attr_value.clone());
                    }

                    // Push onto stack - it will be added to html.children when popped
                    // (Don't add to html.children here to avoid duplicate)
                    self.open_elements.push(element);
                }
                // Otherwise, just ignore (normal case per WHATWG spec)
            }
            _ => {
                self.reconstruct_active_formatting_elements();
                // By this point, we've already checked should_process_start_tag_in_foreign_content.
                // If we're here, we should process as HTML (either not in foreign content,
                // or at an HTML integration point).
                self.insert_html_element(name, attrs);
            }
        }
    }

    fn is_in_foreign_content(&self) -> bool {
        self.current_node()
            .and_then(|n| n.namespace)
            .map_or(false, |ns| ns != Namespace::Html)
    }

    /// Check if the current node is an HTML integration point
    fn is_html_integration_point(&self, node: &Node) -> bool {
        if node.namespace == Some(Namespace::MathML) && node.name == "annotation-xml" {
            if let Some(encoding) = node.attrs.get("encoding") {
                let lower = encoding.to_ascii_lowercase();
                return lower == "text/html" || lower == "application/xhtml+xml";
            }
            return false;
        }
        // SVG integration points (case-insensitive since names may be lowercased)
        if node.namespace == Some(Namespace::Svg) {
            let name_lower = node.name.to_ascii_lowercase();
            return name_lower == "foreignobject" || name_lower == "desc" || name_lower == "title";
        }
        false
    }

    /// Check if the current node is a MathML text integration point
    fn is_mathml_text_integration_point(&self, node: &Node) -> bool {
        node.namespace == Some(Namespace::MathML) &&
            matches!(node.name.as_str(), "mi" | "mo" | "mn" | "ms" | "mtext")
    }

    /// Check if we should process a start tag in foreign content
    /// Per WHATWG spec, certain combinations fall through to HTML processing
    fn should_process_start_tag_in_foreign_content(&self, name: &str) -> bool {
        let current = match self.current_node() {
            Some(n) => n,
            None => return false,
        };

        // If current node is in HTML namespace, not in foreign content
        if current.namespace == Some(Namespace::Html) || current.namespace.is_none() {
            return false;
        }

        // If current is a MathML text integration point and tag is not mglyph/malignmark,
        // fall through to HTML processing
        if self.is_mathml_text_integration_point(current) {
            if name != "mglyph" && name != "malignmark" {
                return false;
            }
        }

        // If current is MathML annotation-xml and tag is "svg", fall through to HTML processing
        if current.namespace == Some(Namespace::MathML) && current.name == "annotation-xml" {
            if name == "svg" {
                return false;
            }
        }

        // If current is an HTML integration point, fall through to HTML processing
        if self.is_html_integration_point(current) {
            return false;
        }

        true
    }

    /// Process a start tag while in foreign content per WHATWG spec.
    /// Returns true if fully handled, false if we should fall through to HTML processing.
    fn process_start_tag_in_foreign_content(&mut self, name: &str, attrs: HashMap<String, String>, self_closing: bool) -> bool {
        // Elements that break out of foreign content
        static FOREIGN_BREAKOUT_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
            [
                "b", "big", "blockquote", "body", "br", "center", "code", "dd", "div", "dl", "dt",
                "em", "embed", "h1", "h2", "h3", "h4", "h5", "h6", "head", "hr", "i", "img", "li",
                "listing", "menu", "meta", "nobr", "ol", "p", "pre", "ruby", "s", "small",
                "span", "strong", "strike", "sub", "sup", "table", "tt", "u", "ul", "var"
            ].into_iter().collect()
        });

        // Check for font with specific attributes (also breaks out)
        let is_breakout_font = name == "font" &&
            (attrs.contains_key("color") || attrs.contains_key("face") || attrs.contains_key("size"));

        if FOREIGN_BREAKOUT_ELEMENTS.contains(name) || is_breakout_font {
            // Pop foreign elements until we hit an HTML element or integration point
            // But in fragment mode, don't pop the context element
            let min_stack_size = if self.fragment_context.is_some() { 1 } else { 0 };
            while self.open_elements.len() > min_stack_size {
                let current = self.current_node();
                if current.map_or(true, |n| n.namespace == Some(Namespace::Html)) {
                    break;
                }
                // Check for MathML text integration point or HTML integration point
                if let Some(node) = current {
                    let is_mathml_text_integration = node.namespace == Some(Namespace::MathML) &&
                        matches!(node.name.as_str(), "mi" | "mo" | "mn" | "ms" | "mtext");
                    // SVG integration points (case-insensitive)
                    let name_lower = node.name.to_ascii_lowercase();
                    let is_html_integration =
                        (node.namespace == Some(Namespace::MathML) && node.name == "annotation-xml" &&
                            node.attrs.get("encoding").map_or(false, |e|
                                e.eq_ignore_ascii_case("text/html") || e.eq_ignore_ascii_case("application/xhtml+xml"))) ||
                        (node.namespace == Some(Namespace::Svg) &&
                            (name_lower == "foreignobject" || name_lower == "desc" || name_lower == "title"));

                    if is_mathml_text_integration || is_html_integration {
                        break;
                    }
                }
                self.pop_and_add_to_parent();
            }
            // Fall through to normal HTML processing
            return false;
        }

        // Create foreign element with current namespace
        let namespace = self.current_node()
            .and_then(|n| n.namespace)
            .unwrap_or(Namespace::Html);

        // Apply SVG element name adjustments
        let adjusted_name = if namespace == Namespace::Svg {
            SVG_ELEMENT_ADJUSTMENTS.get(name).copied().unwrap_or(name)
        } else {
            name
        };

        // Apply attribute name adjustments (SVG or MathML)
        let adjusted_attrs = if namespace == Namespace::Svg {
            attrs.into_iter()
                .map(|(k, v)| {
                    let adjusted_key = SVG_ATTRIBUTE_ADJUSTMENTS.get(k.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or(k);
                    (adjusted_key, v)
                })
                .collect()
        } else if namespace == Namespace::MathML {
            attrs.into_iter()
                .map(|(k, v)| {
                    let key_lower = k.to_ascii_lowercase();
                    let adjusted_key = MATHML_ATTRIBUTE_ADJUSTMENTS.get(key_lower.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or(key_lower);
                    (adjusted_key, v)
                })
                .collect()
        } else {
            attrs
        };

        let element = Node::element_ns(adjusted_name, namespace, adjusted_attrs);
        self.open_elements.push(element);

        if self_closing {
            self.pop_and_add_to_parent();
        }

        true
    }

    /// Process an end tag while in foreign content per WHATWG spec.
    /// Returns true if fully handled (don't continue to normal processing),
    /// false if we should fall through to HTML processing.
    fn process_end_tag_in_foreign_content(&mut self, name: &str) -> bool {
        // Per WHATWG spec "Any other end tag" in foreign content:
        // 1. Initialize node to current node
        // 2. If node's tag name doesn't match, this is a parse error
        // 3. Loop:
        //    a. If node is topmost in stack, return (fragment case - don't pop context)
        //    b. If node's tag name matches, pop until node is popped, return
        //    c. Set node to previous entry
        //    d. If node is in HTML namespace, process using current insertion mode

        let name_lower = name.to_ascii_lowercase();
        let stack_len = self.open_elements.len();

        if stack_len == 0 {
            return false;
        }

        // Per WHATWG: End tags "br" and "p" in foreign content are special
        // Pop foreign elements and reprocess as HTML
        if name_lower == "br" || name_lower == "p" {
            self.error("unexpected-html-end-tag-in-foreign-content");
            // Pop elements until current is in HTML namespace
            let min_stack_size = if self.fragment_context.is_some() { 1 } else { 0 };
            while self.open_elements.len() > min_stack_size {
                if let Some(current) = self.current_node() {
                    if current.namespace == Some(Namespace::Html) {
                        break;
                    }
                    // Also check for integration points
                    if self.is_html_integration_point(current) || self.is_mathml_text_integration_point(current) {
                        break;
                    }
                }
                self.pop_and_add_to_parent();
            }
            // Fall through to HTML processing
            return false;
        }

        // Check for parse error (current node doesn't match)
        if let Some(current) = self.current_node() {
            if current.name.to_ascii_lowercase() != name_lower {
                self.error("unexpected-end-tag-in-foreign-content");
            }
        }

        // Loop through stack per WHATWG spec:
        // The key is we check name match for current node, then check namespace
        // of the PREVIOUS node before continuing. If previous is HTML, we fall
        // through to HTML processing WITHOUT checking if previous's name matches.
        let mut i = stack_len - 1; // Start at current node
        loop {
            let node = &self.open_elements[i];

            // Step i: If node is topmost, return without popping (fragment case)
            if i == 0 {
                return true;
            }

            // Step ii: If node's tag name matches, pop until node is popped
            if node.name.to_ascii_lowercase() == name_lower {
                while self.open_elements.len() > i {
                    self.pop_and_add_to_parent();
                }
                return true;
            }

            // Step iii: Set node to previous entry
            // Step iv-v: Check previous node's namespace
            let prev_node = &self.open_elements[i - 1];
            if prev_node.namespace == Some(Namespace::Html) || prev_node.namespace.is_none() {
                // Previous is HTML namespace, fall through to insertion mode processing
                return false;
            }

            // Previous is not HTML namespace, continue loop with previous as current
            i -= 1;
        }
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
                self.reconstruct_active_formatting_elements();
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
                // In fragment mode with select context, there may not be an actual select on stack
                // after we've already popped it once. Check if we're in that situation.
                let is_select_fragment = self.fragment_context.as_ref()
                    .map_or(false, |ctx| ctx.tag_name == "select");
                let select_on_stack = self.open_elements.iter().any(|n| n.name == "select");

                if select_on_stack {
                    self.pop_elements_until("select");
                    self.reset_insertion_mode();
                    self.process_start_tag(name, attrs, self_closing);
                } else if is_select_fragment {
                    // In select fragment with no select on stack, process directly
                    self.process_in_body_start_tag(name, attrs, self_closing);
                }
            }
            "keygen" => {
                // Per html5lib tests, keygen should be inserted in select mode
                self.insert_element_for_token(name, attrs, true);
            }
            "script" | "template" => {
                self.process_in_head_start_tag(name, attrs, self_closing);
            }
            "caption" | "table" | "tbody" | "tfoot" | "thead" | "tr" | "td" | "th" => {
                // These should close the select and reprocess
                self.error("unexpected-table-element-in-select");
                self.pop_elements_until("select");
                self.reset_insertion_mode();
                self.process_start_tag(name, attrs, self_closing);
            }
            "svg" => {
                // Per html5lib tests, SVG elements should be inserted in select mode
                let element = Node::element_ns(name, Namespace::Svg, attrs);
                if self_closing {
                    // Self-closing: just insert as a child of current node
                    if let Some(current) = self.open_elements.last_mut() {
                        current.children.push(element);
                    }
                } else {
                    self.open_elements.push(element);
                }
            }
            "math" => {
                // Per html5lib tests, MathML elements should be inserted in select mode
                let element = Node::element_ns(name, Namespace::MathML, attrs);
                if self_closing {
                    if let Some(current) = self.open_elements.last_mut() {
                        current.children.push(element);
                    }
                } else {
                    self.open_elements.push(element);
                }
            }
            "b" | "big" | "code" | "em" | "font" | "i" | "nobr" | "s" | "small" |
            "strike" | "strong" | "tt" | "u" | "a" => {
                // Formatting elements in select mode: insert AND push to AFE
                // so they persist when select closes
                self.insert_html_element(name, attrs.clone());
                self.push_active_formatting_element(name, attrs);
            }
            "button" | "datalist" | "menuitem" | "selectedcontent" => {
                // These elements should be inserted in select mode per html5lib tests
                self.insert_html_element(name, attrs);
            }
            "br" | "embed" | "img" | "meta" => {
                // Void elements in select mode: insert as self-closing
                self.insert_element_for_token(name, attrs, true);
            }
            "plaintext" => {
                // Plaintext in select mode: insert element (tokenizer will switch to plaintext state)
                self.insert_html_element(name, attrs);
            }
            "blockquote" | "body" | "center" | "dd" | "div" |
            "dl" | "dt" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "head" |
            "li" | "listing" | "menu" | "ol" | "p" |
            "pre" | "ruby" | "span" | "sub" | "sup" | "ul" | "var" => {
                // Non-formatting block elements in select mode: just insert
                self.insert_html_element(name, attrs);
            }
            _ => {
                // Per WHATWG spec: "Any other start tag" in select is a parse error, ignore the token
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
                // Missing doctype triggers quirks mode
                self.quirks_mode = true;
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
                        self.process_end_tag_in_head(name);
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
                self.insertion_mode = self.original_insertion_mode;
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
                    "b" | "i" | "u" | "em" | "strong" | "font" | "a" | "nobr" | "s" |
                    "strike" | "tt" | "big" | "small" | "code" => {
                        // Formatting element end tags use adoption agency algorithm
                        // But only if the element is inside the select (after select on stack)
                        let mut element_idx = None;
                        let mut select_idx = None;
                        for (i, node) in self.open_elements.iter().enumerate().rev() {
                            if node.name == "select" {
                                select_idx = Some(i);
                                break;
                            }
                            if node.name == name && element_idx.is_none() {
                                element_idx = Some(i);
                            }
                        }
                        // Only run adoption agency if element is after select (inside select)
                        if let Some(elem_idx) = element_idx {
                            if select_idx.map_or(true, |sel_idx| elem_idx > sel_idx) {
                                self.adoption_agency(name);
                            }
                        }
                    }
                    "button" | "div" | "selectedcontent" | "span" | "datalist" |
                    "center" | "blockquote" | "dd" | "dl" | "dt" | "h1" | "h2" | "h3" |
                    "h4" | "h5" | "h6" | "li" | "listing" | "menu" | "ol" | "p" | "pre" |
                    "ruby" | "sub" | "sup" | "ul" | "var" | "menuitem" => {
                        // Non-formatting elements that can be opened in select mode should be closed properly
                        // Pop up to and including the named element, but DON'T pop past select
                        // Find position of the element, ensuring it's after any select
                        let mut element_idx = None;
                        let mut select_idx = None;
                        for (i, node) in self.open_elements.iter().enumerate().rev() {
                            if node.name == "select" {
                                select_idx = Some(i);
                                break;
                            }
                            if node.name == name && element_idx.is_none() {
                                element_idx = Some(i);
                            }
                        }
                        // Only pop if the element is after select (inside select)
                        if let Some(elem_idx) = element_idx {
                            if select_idx.map_or(true, |sel_idx| elem_idx > sel_idx) {
                                // Pop up to and including the element
                                while self.open_elements.len() > elem_idx {
                                    self.pop_and_add_to_parent();
                                }
                            }
                        }
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
                // In template fragment context with only context template, skip
                let is_template_fragment = self.fragment_context.as_ref()
                    .map_or(false, |ctx| ctx.tag_name == "template");
                let only_has_context_template = self.open_elements.len() == 1 &&
                    self.open_elements.first().map_or(false, |n| n.name == "template");

                if is_template_fragment && only_has_context_template {
                    // Skip - this </template> is for the context element
                    return;
                }

                // Per spec: check if an HTML template is on the stack (not SVG/MathML)
                let has_html_template = self.open_elements.iter().any(|n| {
                    n.name == "template" &&
                    (n.namespace == Some(Namespace::Html) || n.namespace.is_none())
                });
                if has_html_template {
                    self.generate_implied_end_tags();
                    self.pop_elements_until_html_template();
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
                    // Per WHATWG: use form element pointer, remove from stack (not pop until)
                    let node_idx = self.form_element_index;
                    self.form_element_index = None;
                    if let Some(idx) = node_idx {
                        if idx < self.open_elements.len() && self.open_elements[idx].name == "form" {
                            self.generate_implied_end_tags();
                            // Per WHATWG: "Remove node from the stack of open elements"
                            // Elements above form in stack should become form's children eventually
                            // We use real_node_id to redirect child additions to form

                            // First, mark elements above form to redirect to form when popped
                            // Use real_node_id WITHOUT is_parented to indicate "add me to this parent"
                            let form_id = self.open_elements[idx].id;
                            for elem in self.open_elements.iter_mut().skip(idx + 1) {
                                if elem.real_node_id.is_none() {
                                    elem.real_node_id = Some(form_id);
                                }
                            }

                            // Remove form from stack and add to parent
                            let form_node = self.open_elements.remove(idx);
                            if idx > 0 {
                                self.open_elements[idx - 1].children.push(form_node);
                            }
                        } else {
                            self.error("unexpected-form-end-tag");
                        }
                    } else {
                        self.error("unexpected-form-end-tag");
                    }
                } else if self.has_element_in_scope("form") {
                    // Template case: pop elements until form
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
                // Must use insert_html_element to ensure HTML namespace even if current is foreign
                self.insert_html_element("br", HashMap::new());
                self.pop_and_add_to_parent(); // br is a void element
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
                    // Close any formatting elements that were interrupted by the table
                    // (elements on stack but no longer in AFE due to failed adoption agency)
                    self.close_interrupted_formatting_elements();
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
                    // Missing doctype triggers quirks mode
                    self.quirks_mode = true;
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
            InsertionMode::InHead => {
                if c == '\t' || c == '\n' || c == '\x0C' || c == '\r' || c == ' ' {
                    self.insert_character(c);
                } else {
                    self.pop_and_add_to_parent(); // Pop head
                    self.insertion_mode = InsertionMode::AfterHead;
                    self.process_character(c);
                }
            }
            InsertionMode::InHeadNoscript => {
                if c == '\t' || c == '\n' || c == '\x0C' || c == '\r' || c == ' ' {
                    self.insert_character(c);
                } else {
                    // Non-whitespace in head noscript: pop noscript and reprocess
                    self.error("unexpected-char-in-head-noscript");
                    self.pop_and_add_to_parent(); // Pop noscript
                    self.insertion_mode = InsertionMode::InHead;
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
                } else if c == '\x0C' && self.is_in_foreign_content() {
                    // Form feed (U+000C) is not valid XML whitespace. In MathML/SVG contexts,
                    // it should be silently dropped per XML spec (only space, tab, LF, CR are valid).
                    // This is a parse error but we don't need to emit one - just ignore it.
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
                // Check if we're in foreign content - if so, insert normally (except form feed)
                if self.is_in_foreign_content() {
                    if c == '\0' {
                        self.error("unexpected-null-character");
                        self.insert_character('\u{FFFD}');
                    } else if c == '\x0C' {
                        // Form feed in foreign content is dropped
                    } else {
                        self.insert_character(c);
                    }
                } else if c == '\x0C' {
                    // Form feed in table is dropped
                } else if TABLE_TEXT_CONTEXT_TAGS.contains(self.current_node().map_or("", |n| n.name.as_str())) {
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
                } else if c == '\x0C' {
                    // Form feed in table text is dropped per tests
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
                } else if c == '\x0C' {
                    // Form feed in select is dropped per tests
                } else {
                    // Reconstruct formatting elements to handle nested formatting after adoption agency
                    self.reconstruct_active_formatting_elements();
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
        // Check if we're in foreign content first - if so, insert normally
        if self.is_in_foreign_content() {
            self.insert_character(c);
            return;
        }

        let current = self.current_node();
        let current_name = current.map_or("", |n| n.name.as_str());

        if TABLE_TEXT_CONTEXT_TAGS.contains(current_name) {
            self.pending_table_chars.clear();
            self.original_insertion_mode = self.insertion_mode;
            self.insertion_mode = InsertionMode::InTableText;
            self.process_character(c);
        } else if current.map_or(false, |n| n.foster_parented) {
            // We're inside a foster-parented element, process text normally
            self.insert_character(c);
        } else {
            // Per spec: if template is on stack, no foster parenting - insert normally
            let has_template = self.open_elements.iter().any(|n| n.name == "template");
            if has_template {
                self.insert_character(c);
            } else {
                self.error("unexpected-character-in-table");
                self.foster_parent_character(c);
            }
        }
    }

    fn process_comment(&mut self, data: &str) {
        // Handle InTableText: flush pending text before processing comment
        if self.insertion_mode == InsertionMode::InTableText {
            self.flush_table_text();
            self.insertion_mode = self.original_insertion_mode;
        }

        match self.insertion_mode {
            InsertionMode::Initial | InsertionMode::BeforeHtml |
            InsertionMode::AfterAfterBody | InsertionMode::AfterAfterFrameset => {
                let comment = Node::comment(data);
                self.document.children.push(comment);
            }
            InsertionMode::AfterBody => {
                // Store comment to be inserted after body when parsing finishes
                // (body is still on the stack and will be added to html when popped)
                self.after_body_comments.push(Node::comment(data));
            }
            _ => {
                self.insert_comment(data);
            }
        }
    }

    fn process_eof(&mut self) {
        match self.insertion_mode {
            InsertionMode::Initial => {
                // Missing doctype triggers quirks mode
                self.quirks_mode = true;
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
            InsertionMode::InHead => {
                // Only pop if head is actually on the stack
                // (It might have been removed during AfterHead->InHead head reinsertion)
                if self.current_node().map_or(false, |n| n.name == "head") {
                    self.pop_and_add_to_parent();
                }
                self.insertion_mode = InsertionMode::AfterHead;
                self.process_eof();
            }
            InsertionMode::InHeadNoscript => {
                // Pop noscript from the stack
                self.error("eof-in-head-noscript");
                self.pop_and_add_to_parent();
                self.insertion_mode = InsertionMode::InHead;
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
                // If we're inside a template, handle EOF in template mode first
                if !self.template_insertion_modes.is_empty() {
                    self.insertion_mode = InsertionMode::InTemplate;
                    self.process_eof();
                    return;
                }
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
                // If we're inside a template, handle EOF in template mode first
                if !self.template_insertion_modes.is_empty() {
                    self.insertion_mode = InsertionMode::InTemplate;
                    self.process_eof();
                    return;
                }
                // Stop parsing
            }
            InsertionMode::InTableText => {
                // Flush pending table text before stopping
                self.flush_table_text();
                // If we're inside a template, handle EOF in template mode first
                if !self.template_insertion_modes.is_empty() {
                    self.insertion_mode = InsertionMode::InTemplate;
                    self.process_eof();
                    return;
                }
                // Stop parsing
            }
            InsertionMode::InSelect | InsertionMode::InSelectInTable => {
                // If we're inside a template, handle EOF in template mode first
                if !self.template_insertion_modes.is_empty() {
                    self.insertion_mode = InsertionMode::InTemplate;
                    self.process_eof();
                    return;
                }
                // Stop parsing
            }
            InsertionMode::InTemplate => {
                if self.template_insertion_modes.is_empty() {
                    // Stop parsing
                } else {
                    self.error("eof-in-template");
                    // Pop until we find an HTML template element (not SVG/MathML)
                    self.pop_elements_until_html_template();
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
            if node.name != "td" && node.name != "th" {
                self.error("end-tag-too-early");
            }
        }
        // Pop until HTML td or th (may pop to empty stack if no HTML td/th exists)
        // This matches Swift's behavior for SVG elements with td/th names
        self.pop_elements_until_one_of(&["td", "th"]);
        self.clear_active_formatting_to_marker();
        self.insertion_mode = InsertionMode::InRow;
    }

    fn reset_insertion_mode(&mut self) {
        for i in (0..self.open_elements.len()).rev() {
            let node = &self.open_elements[i];
            let last = i == 0;

            // Check if this is an HTML element (not SVG or MathML)
            let is_html = node.namespace == Some(Namespace::Html) || node.namespace.is_none();

            let name = if last {
                if let Some(ref ctx) = self.fragment_context {
                    &ctx.tag_name
                } else {
                    &node.name
                }
            } else {
                &node.name
            };

            // Skip non-HTML elements for table-related modes
            // SVG/MathML elements with names like "tr", "td", "th" should not trigger table modes
            if !is_html && !last {
                match name.as_str() {
                    "select" | "td" | "th" | "tr" | "tbody" | "thead" | "tfoot" |
                    "caption" | "colgroup" | "table" => continue,
                    _ => {}
                }
            }

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
                    // For select fragment context, use InBody mode per html5lib behavior
                    // This allows unknown elements to be inserted inside select context
                    if last && self.fragment_context.is_some() {
                        InsertionMode::InBody
                    } else {
                        InsertionMode::InSelect
                    }
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
            // Contains non-whitespace - need to foster parent
            // Find the template that contains the table, if any
            let has_template = self.open_elements.iter().any(|n| n.name == "template");

            if has_template {
                // Foster parent into template_content before the table
                // Find the table and its parent (which should be template)
                if let Some((parent_idx, _)) = self.find_foster_parent_location() {
                    let parent = &mut self.open_elements[parent_idx];
                    // If parent is a template, insert into its content before the table
                    if parent.name == "template" {
                        if let Some(ref mut content) = parent.template_content {
                            // Find where the table will be inserted and insert text before it
                            content.children.push(Node::text(&text));
                            return;
                        }
                    }
                }
                // Fallback: insert normally
                for c in text.chars() {
                    self.insert_character(c);
                }
            } else {
                // No template - foster parent to body
                for c in text.chars() {
                    self.foster_parent_character(c);
                }
            }
        } else {
            // All whitespace - insert normally
            for c in text.chars() {
                self.insert_character(c);
            }
        }
    }

    fn foster_parent_token(&mut self, name: &str, attrs: HashMap<String, String>, self_closing: bool) {
        // If current node is an HTML integration point (like foreignObject), don't foster-parent
        // Instead, insert into the current node as normal HTML content
        if let Some(current) = self.current_node() {
            if self.is_html_integration_point(current) {
                self.insert_html_element(name, attrs);
                if self_closing || VOID_ELEMENTS.contains(name) {
                    self.pop_and_add_to_parent();
                }
                return;
            }
        }

        // Per spec: if template is on stack, don't foster parent to body
        let has_template = self.open_elements.iter().any(|n| n.name == "template");
        if has_template {
            // Find the foster parent location (before the table)
            if let Some((parent_idx, insert_idx)) = self.find_foster_parent_location() {
                // Close table elements up to (but not including) the parent
                while self.open_elements.len() > parent_idx + 1 {
                    if let Some(node) = self.open_elements.last() {
                        if ["table", "tbody", "thead", "tfoot", "tr", "td", "th", "caption", "colgroup"].contains(&node.name.as_str()) {
                            self.pop_and_add_to_parent();
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                // Create the element and insert it at the foster parent location
                let mut element = Node::element(name, attrs);
                let dom_element = element.clone_deep();
                let dom_id = dom_element.id;

                // Mark stack element as already parented
                element.is_parented = true;
                element.real_node_id = Some(dom_id);

                let parent = &mut self.open_elements[parent_idx];
                // If parent is a template, insert into its content
                if parent.name == "template" {
                    if let Some(ref mut content) = parent.template_content {
                        if insert_idx <= content.children.len() {
                            content.children.insert(insert_idx, dom_element);
                        } else {
                            content.children.push(dom_element);
                        }
                    }
                } else {
                    if insert_idx <= parent.children.len() {
                        parent.children.insert(insert_idx, dom_element);
                    } else {
                        parent.children.push(dom_element);
                    }
                }
                // Push the element to the stack if it's not void
                if !VOID_ELEMENTS.contains(name) && !self_closing {
                    self.open_elements.push(element);
                }
            } else {
                // No table found, but still in table context inside template
                // Close table context elements and insert as sibling
                while let Some(node) = self.open_elements.last() {
                    if node.name == "template" || node.name == "html" {
                        break;
                    }
                    if ["table", "tbody", "thead", "tfoot", "tr", "td", "th", "caption", "colgroup"].contains(&node.name.as_str()) {
                        self.pop_and_add_to_parent();
                    } else {
                        break;
                    }
                }
                // Now insert the element
                self.insert_html_element(name, attrs);
                if self_closing || VOID_ELEMENTS.contains(name) {
                    self.pop_and_add_to_parent();
                }
            }
        } else {
            // Simplified foster parenting - just insert in body
            self.foster_parenting = true;
            self.process_in_body_start_tag(name, attrs, self_closing);
            self.foster_parenting = false;
        }
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
