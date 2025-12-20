//! HTML serialization

use crate::node::{Namespace, Node, NodeData};
use crate::constants::VOID_ELEMENTS;

/// Escape HTML special characters
fn escape_html(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            _ => result.push(c),
        }
    }
    result
}

/// Serialize a node to HTML
pub fn to_html(node: &Node, pretty: bool, indent_size: usize) -> String {
    let mut output = String::new();
    serialize_node(node, &mut output, pretty, indent_size, 0);
    output
}

fn serialize_node(node: &Node, output: &mut String, pretty: bool, indent_size: usize, depth: usize) {
    let indent = if pretty {
        " ".repeat(depth * indent_size)
    } else {
        String::new()
    };
    let newline = if pretty { "\n" } else { "" };

    match node.name.as_str() {
        "#document" | "#document-fragment" => {
            for child in &node.children {
                serialize_node(child, output, pretty, indent_size, depth);
            }
        }
        "#text" => {
            if let Some(NodeData::Text(ref text)) = node.data {
                output.push_str(&escape_html(text));
            }
        }
        "#comment" => {
            if let Some(NodeData::Comment(ref text)) = node.data {
                output.push_str(&indent);
                output.push_str("<!--");
                output.push_str(text);
                output.push_str("-->");
                output.push_str(newline);
            }
        }
        "!doctype" => {
            if let Some(NodeData::Doctype(ref doctype)) = node.data {
                output.push_str("<!DOCTYPE ");
                if let Some(ref name) = doctype.name {
                    output.push_str(name);
                }
                if let Some(ref public_id) = doctype.public_id {
                    output.push_str(" PUBLIC \"");
                    output.push_str(public_id);
                    output.push('"');
                    if let Some(ref system_id) = doctype.system_id {
                        output.push_str(" \"");
                        output.push_str(system_id);
                        output.push('"');
                    }
                } else if let Some(ref system_id) = doctype.system_id {
                    output.push_str(" SYSTEM \"");
                    output.push_str(system_id);
                    output.push('"');
                }
                output.push('>');
                output.push_str(newline);
            }
        }
        tag_name => {
            // Element node
            output.push_str(&indent);
            output.push('<');
            output.push_str(tag_name);

            // Attributes
            let mut attrs: Vec<_> = node.attrs.iter().collect();
            attrs.sort_by_key(|(k, _)| *k);
            for (key, value) in attrs {
                output.push(' ');
                output.push_str(key);
                output.push_str("=\"");
                output.push_str(&escape_html(value));
                output.push('"');
            }

            // Self-closing or void element
            if VOID_ELEMENTS.contains(tag_name) {
                output.push_str(" />");
                output.push_str(newline);
            } else {
                output.push('>');

                // Children
                let has_element_children = node.children.iter().any(|c| c.is_element());
                if pretty && has_element_children {
                    output.push_str(newline);
                }

                for child in &node.children {
                    serialize_node(child, output, pretty, indent_size, depth + 1);
                }

                // Template content
                if let Some(ref template_content) = node.template_content {
                    for child in &template_content.children {
                        serialize_node(child, output, pretty, indent_size, depth + 1);
                    }
                }

                if pretty && has_element_children {
                    output.push_str(&indent);
                }
                output.push_str("</");
                output.push_str(tag_name);
                output.push('>');
                output.push_str(newline);
            }
        }
    }
}

/// Serialize a node to html5lib test format
pub fn to_test_format(node: &Node) -> String {
    let mut output = String::new();
    serialize_test_format(node, &mut output, 0);
    output.trim_end().to_string()
}

fn serialize_test_format(node: &Node, output: &mut String, depth: usize) {
    let indent = "| ".to_string() + &"  ".repeat(depth);

    match node.name.as_str() {
        "#document" | "#document-fragment" => {
            for child in &node.children {
                serialize_test_format(child, output, depth);
            }
        }
        "#text" => {
            if let Some(NodeData::Text(ref text)) = node.data {
                output.push_str(&indent);
                output.push('"');
                output.push_str(text);
                output.push_str("\"\n");
            }
        }
        "#comment" => {
            if let Some(NodeData::Comment(ref text)) = node.data {
                output.push_str(&indent);
                output.push_str("<!-- ");
                output.push_str(text);
                output.push_str(" -->\n");
            }
        }
        "!doctype" => {
            if let Some(NodeData::Doctype(ref doctype)) = node.data {
                output.push_str(&indent);
                output.push_str("<!DOCTYPE ");
                output.push_str(doctype.name.as_deref().unwrap_or(""));
                if doctype.public_id.is_some() || doctype.system_id.is_some() {
                    output.push_str(" \"");
                    output.push_str(doctype.public_id.as_deref().unwrap_or(""));
                    output.push_str("\" \"");
                    output.push_str(doctype.system_id.as_deref().unwrap_or(""));
                    output.push('"');
                }
                output.push_str(">\n");
            }
        }
        tag_name => {
            // Element node
            output.push_str(&indent);
            output.push('<');

            // Namespace prefix
            if let Some(ns) = node.namespace {
                match ns {
                    Namespace::Svg => output.push_str("svg "),
                    Namespace::MathML => output.push_str("math "),
                    Namespace::Html => {}
                }
            }

            output.push_str(tag_name);
            output.push_str(">\n");

            // Attributes (sorted case-sensitively as per html5lib test format)
            // For foreign content, we need to sort by the OUTPUT key, not the input key
            let is_foreign = node.namespace == Some(Namespace::Svg) ||
                             node.namespace == Some(Namespace::MathML);

            // Helper to get output key for foreign attributes
            static XLINK_ATTRS: [&str; 7] = ["actuate", "arcrole", "href", "role", "show", "title", "type"];
            static XML_ATTRS: [&str; 2] = ["lang", "space"];

            let get_output_key = |key: &str| -> String {
                if !is_foreign {
                    return key.to_string();
                }
                if let Some(local) = key.strip_prefix("xlink:") {
                    if XLINK_ATTRS.contains(&local) {
                        return format!("xlink {}", local);
                    }
                } else if let Some(local) = key.strip_prefix("xml:") {
                    if XML_ATTRS.contains(&local) {
                        return format!("xml {}", local);
                    }
                } else if key == "xmlns:xlink" {
                    return "xmlns xlink".to_string();
                }
                key.to_string()
            };

            // Collect and sort by output key
            let mut attrs: Vec<_> = node.attrs.iter()
                .map(|(k, v)| (get_output_key(k), v.as_str()))
                .collect();
            attrs.sort_by(|(k1, _), (k2, _)| k1.cmp(k2));

            for (key, value) in attrs {
                output.push_str(&indent);
                output.push_str("  ");
                output.push_str(&key);
                output.push_str("=\"");
                output.push_str(value);
                output.push_str("\"\n");
            }

            // Children
            for child in &node.children {
                serialize_test_format(child, output, depth + 1);
            }

            // Template content
            if tag_name == "template" {
                if let Some(ref template_content) = node.template_content {
                    output.push_str(&indent);
                    output.push_str("  content\n");
                    for child in &template_content.children {
                        serialize_test_format(child, output, depth + 2);
                    }
                }
            }
        }
    }
}

/// Extract text content from a node
pub fn to_text(node: &Node) -> String {
    let mut parts = Vec::new();
    collect_text(node, &mut parts);
    let text = parts.join("");

    // Collapse whitespace
    let mut result = String::new();
    let mut prev_ws = true;
    for c in text.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                result.push(' ');
                prev_ws = true;
            }
        } else {
            result.push(c);
            prev_ws = false;
        }
    }
    result.trim().to_string()
}

fn collect_text(node: &Node, parts: &mut Vec<String>) {
    if let Some(NodeData::Text(ref text)) = node.data {
        parts.push(text.clone());
        return;
    }

    for child in &node.children {
        collect_text(child, parts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("&<>\""), "&amp;&lt;&gt;&quot;");
        assert_eq!(escape_html("hello"), "hello");
    }

    #[test]
    fn test_to_text() {
        let mut doc = Node::new("#document");
        let mut p = Node::new("p");
        p.children.push(Node::text("Hello "));
        p.children.push(Node::text("World"));
        doc.children.push(p);

        assert_eq!(to_text(&doc), "Hello World");
    }
}
