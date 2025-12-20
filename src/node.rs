//! DOM node representation

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

/// Namespace for HTML elements
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Html,
    Svg,
    MathML,
}

impl Namespace {
    pub fn as_str(&self) -> &'static str {
        match self {
            Namespace::Html => "html",
            Namespace::Svg => "svg",
            Namespace::MathML => "math",
        }
    }
}

/// DOCTYPE information
#[derive(Debug, Clone, Default)]
pub struct Doctype {
    pub name: Option<String>,
    pub public_id: Option<String>,
    pub system_id: Option<String>,
    pub force_quirks: bool,
}

impl Doctype {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Data payload for special node types
#[derive(Debug, Clone)]
pub enum NodeData {
    Text(String),
    Comment(String),
    Doctype(Doctype),
}

/// A handle to a DOM node (reference-counted)
pub type Handle = Rc<RefCell<NodeInner>>;
pub type WeakHandle = Weak<RefCell<NodeInner>>;

/// Inner node data (wrapped in RefCell for mutability)
#[derive(Debug)]
pub struct NodeInner {
    /// Node type/name: "#document", "#document-fragment", "#text", "#comment", or tag name
    pub name: String,
    /// Namespace: Html, Svg, MathML, or None for non-elements
    pub namespace: Option<Namespace>,
    /// Parent node (weak to avoid cycles)
    pub parent: Option<WeakHandle>,
    /// Child nodes
    pub children: Vec<Handle>,
    /// Attributes (empty for non-elements)
    pub attrs: HashMap<String, String>,
    /// Data for text/comment/doctype nodes
    pub data: Option<NodeData>,
    /// Template content (for <template> elements)
    pub template_content: Option<Handle>,
}

impl NodeInner {
    pub fn new(name: &str, namespace: Option<Namespace>) -> Self {
        let actual_namespace = if name.starts_with('#') || name == "!doctype" {
            None
        } else {
            namespace.or(Some(Namespace::Html))
        };

        Self {
            name: name.to_string(),
            namespace: actual_namespace,
            parent: None,
            children: Vec::new(),
            attrs: HashMap::new(),
            data: None,
            template_content: None,
        }
    }
}

/// A DOM node with a simpler interface
#[derive(Debug, Clone)]
pub struct Node {
    /// Unique node ID for identity tracking
    pub id: u64,
    /// Node type/name
    pub name: String,
    /// Namespace
    pub namespace: Option<Namespace>,
    /// Child nodes
    pub children: Vec<Node>,
    /// Attributes
    pub attrs: HashMap<String, String>,
    /// Data for text/comment/doctype
    pub data: Option<NodeData>,
    /// Template content
    pub template_content: Option<Box<Node>>,
    /// Flag to track if this node is already a child of another node
    /// Used for cases where elements need to be on the stack but are already parented
    #[doc(hidden)]
    pub is_parented: bool,
    /// Flag to track if this element was inserted via foster parenting
    #[doc(hidden)]
    pub foster_parented: bool,
    /// For adoption agency: the ID of the "real" node in the DOM tree
    /// When content is added to this placeholder, it should go to the real node
    #[doc(hidden)]
    pub real_node_id: Option<u64>,
    /// For block elements inserted inside a formatting element's DOM during foster parenting:
    /// tracks which formatting element IDs are DOM ancestors of this block
    #[doc(hidden)]
    pub formatting_ancestor_ids: Vec<u64>,
}

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_NODE_ID: AtomicU64 = AtomicU64::new(1);

pub fn generate_node_id() -> u64 {
    NEXT_NODE_ID.fetch_add(1, Ordering::Relaxed)
}

impl Node {
    pub fn new(name: &str) -> Self {
        let namespace = if name.starts_with('#') || name == "!doctype" {
            None
        } else {
            Some(Namespace::Html)
        };

        Self {
            id: generate_node_id(),
            name: name.to_string(),
            namespace,
            children: Vec::new(),
            attrs: HashMap::new(),
            data: None,
            template_content: None,
            is_parented: false,
            foster_parented: false,
            real_node_id: None,
            formatting_ancestor_ids: Vec::new(),
        }
    }

    pub fn with_namespace(name: &str, namespace: Option<Namespace>) -> Self {
        let actual_namespace = if name.starts_with('#') || name == "!doctype" {
            None
        } else {
            namespace.or(Some(Namespace::Html))
        };

        Self {
            id: generate_node_id(),
            name: name.to_string(),
            namespace: actual_namespace,
            children: Vec::new(),
            attrs: HashMap::new(),
            data: None,
            template_content: None,
            is_parented: false,
            foster_parented: false,
            real_node_id: None,
            formatting_ancestor_ids: Vec::new(),
        }
    }

    pub fn document() -> Self {
        Self::new("#document")
    }

    pub fn document_fragment() -> Self {
        Self::new("#document-fragment")
    }

    pub fn text(content: &str) -> Self {
        let mut node = Self::new("#text");
        node.data = Some(NodeData::Text(content.to_string()));
        node
    }

    pub fn comment(content: &str) -> Self {
        let mut node = Self::new("#comment");
        node.data = Some(NodeData::Comment(content.to_string()));
        node
    }

    pub fn doctype(doctype: Doctype) -> Self {
        let mut node = Self::new("!doctype");
        node.data = Some(NodeData::Doctype(doctype));
        node
    }

    pub fn element(name: &str, attrs: HashMap<String, String>) -> Self {
        Self {
            id: generate_node_id(),
            name: name.to_string(),
            namespace: Some(Namespace::Html),
            children: Vec::new(),
            attrs,
            data: None,
            template_content: if name == "template" {
                Some(Box::new(Self::document_fragment()))
            } else {
                None
            },
            is_parented: false,
            foster_parented: false,
            real_node_id: None,
            formatting_ancestor_ids: Vec::new(),
        }
    }

    pub fn element_ns(name: &str, namespace: Namespace, attrs: HashMap<String, String>) -> Self {
        Self {
            id: generate_node_id(),
            name: name.to_string(),
            namespace: Some(namespace),
            children: Vec::new(),
            attrs,
            data: None,
            template_content: if name == "template" && namespace == Namespace::Html {
                Some(Box::new(Self::document_fragment()))
            } else {
                None
            },
            is_parented: false,
            foster_parented: false,
            real_node_id: None,
            formatting_ancestor_ids: Vec::new(),
        }
    }

    /// Check if this is an element node
    pub fn is_element(&self) -> bool {
        !self.name.starts_with('#') && self.name != "!doctype"
    }

    /// Check if this is a text node
    pub fn is_text(&self) -> bool {
        self.name == "#text"
    }

    /// Check if this is a comment node
    pub fn is_comment(&self) -> bool {
        self.name == "#comment"
    }

    /// Check if this is a document node
    pub fn is_document(&self) -> bool {
        self.name == "#document"
    }

    /// Check if this is a document fragment node
    pub fn is_document_fragment(&self) -> bool {
        self.name == "#document-fragment"
    }

    /// Get text content (for text nodes)
    pub fn text_content(&self) -> Option<&str> {
        if let Some(NodeData::Text(ref s)) = self.data {
            Some(s)
        } else {
            None
        }
    }

    /// Get comment content (for comment nodes)
    pub fn comment_content(&self) -> Option<&str> {
        if let Some(NodeData::Comment(ref s)) = self.data {
            Some(s)
        } else {
            None
        }
    }

    /// Get doctype data
    pub fn doctype_data(&self) -> Option<&Doctype> {
        if let Some(NodeData::Doctype(ref d)) = self.data {
            Some(d)
        } else {
            None
        }
    }

    /// Append a child node
    pub fn append_child(&mut self, child: Node) {
        self.children.push(child);
    }

    /// Insert a child before a reference node
    pub fn insert_before(&mut self, child: Node, index: usize) {
        if index <= self.children.len() {
            self.children.insert(index, child);
        } else {
            self.children.push(child);
        }
    }

    /// Remove a child at index
    pub fn remove_child(&mut self, index: usize) -> Option<Node> {
        if index < self.children.len() {
            Some(self.children.remove(index))
        } else {
            None
        }
    }

    /// Clone the node (deep clone)
    pub fn clone_deep(&self) -> Self {
        let mut clone = Self {
            id: generate_node_id(),
            name: self.name.clone(),
            namespace: self.namespace,
            children: Vec::new(),
            attrs: self.attrs.clone(),
            data: self.data.clone(),
            template_content: self.template_content.as_ref().map(|t| Box::new(t.clone_deep())),
            is_parented: self.is_parented,
            foster_parented: self.foster_parented,
            real_node_id: self.real_node_id,
            formatting_ancestor_ids: self.formatting_ancestor_ids.clone(),
        };

        for child in &self.children {
            clone.children.push(child.clone_deep());
        }

        clone
    }

    /// Find a node by ID in this subtree (returns mutable reference)
    pub fn find_by_id_mut(&mut self, target_id: u64) -> Option<&mut Node> {
        if self.id == target_id {
            return Some(self);
        }
        for child in &mut self.children {
            if let Some(found) = child.find_by_id_mut(target_id) {
                return Some(found);
            }
        }
        if let Some(ref mut content) = self.template_content {
            if let Some(found) = content.find_by_id_mut(target_id) {
                return Some(found);
            }
        }
        None
    }
}

/// Convert from Handle to Node (for final output)
pub fn handle_to_node(handle: &Handle) -> Node {
    let inner = handle.borrow();

    let mut node = Node {
        id: generate_node_id(),
        name: inner.name.clone(),
        namespace: inner.namespace,
        children: Vec::new(),
        attrs: inner.attrs.clone(),
        data: inner.data.clone(),
        template_content: None,
        is_parented: false,
        foster_parented: false,
        real_node_id: None,
        formatting_ancestor_ids: Vec::new(),
    };

    // Convert children
    for child in &inner.children {
        node.children.push(handle_to_node(child));
    }

    // Convert template content
    if let Some(ref template) = inner.template_content {
        node.template_content = Some(Box::new(handle_to_node(template)));
    }

    node
}
