use justhtml::{JustHTML, FragmentContext};
use justhtml::serialize::to_test_format;

fn main() {
    // Test from svg.dat - td fragment context
    let html = "<svg><tr><td><title><tr>";
    println!("Input: {:?}", html);
    println!("Fragment context: td");

    let ctx = FragmentContext::new("td");
    let doc = JustHTML::parse_fragment(html, ctx);
    println!("\nDocument children count: {}", doc.root.children.len());
    for (i, child) in doc.root.children.iter().enumerate() {
        println!("  Child {}: {} (ns: {:?})", i, child.name, child.namespace);
    }
    println!("\nActual output:");
    println!("{}", to_test_format(&doc.root));

    println!("\nExpected:");
    println!("| <svg svg>");
    println!("|   <svg tr>");
    println!("|     <svg td>");
    println!("|       <svg title>");
}
