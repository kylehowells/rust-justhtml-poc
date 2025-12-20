use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // Simple test
    let html = "<select><td>text";
    println!("Input: {:?}", html);
    let doc = JustHTML::parse(html);
    println!("Document children count: {}", doc.root.children.len());
    for (i, child) in doc.root.children.iter().enumerate() {
        println!("  Child {}: {} (ns: {:?})", i, child.name, child.namespace);
        for (j, grandchild) in child.children.iter().enumerate() {
            println!("    Grandchild {}: {} (ns: {:?})", j, grandchild.name, grandchild.namespace);
        }
    }
    println!("\nActual output:");
    println!("{}", to_test_format(&doc.root));
}
