use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // First, verify form feed string conversion
    let ff = '\x0c';
    let ff_str = ff.to_string();
    println!("Form feed char: {:?}", ff);
    println!("Form feed string: {:?}", ff_str);
    println!("Form feed string len: {}", ff_str.len());
    println!("Form feed string bytes: {:?}", ff_str.as_bytes());
    println!();

    // Test case from branch_coverage.dat test 2
    let html = "<math><mi>\x00\x0c</mi></math>";
    println!("Input bytes: {:?}", html.as_bytes());
    println!("Input: {:?}", html);

    let doc = JustHTML::parse(html);
    let output = to_test_format(&doc.root);
    println!("\nActual output:");
    println!("{}", output);

    println!("\nExpected output:");
    println!("| <html>");
    println!("|   <head>");
    println!("|   <body>");
    println!("|     <math math>");
    println!("|       <math mi>");

    // Also test just form feed
    println!("\n\n=== Just form feed ===");
    let html2 = "<math><mi>\x0c</mi></math>";
    println!("Input bytes: {:?}", html2.as_bytes());
    let doc2 = JustHTML::parse(html2);
    let output2 = to_test_format(&doc2.root);
    println!("\nActual output:");
    println!("{}", output2);

    // Debug: look at the mi element's children directly
    println!("\nDebug: mi children:");
    fn find_mi(node: &justhtml::Node) -> Option<&justhtml::Node> {
        if node.name == "mi" {
            return Some(node);
        }
        for child in &node.children {
            if let Some(found) = find_mi(child) {
                return Some(found);
            }
        }
        None
    }
    if let Some(mi) = find_mi(&doc2.root) {
        println!("mi has {} children", mi.children.len());
        for (i, child) in mi.children.iter().enumerate() {
            println!("  Child {}: name={:?}, data={:?}", i, child.name, child.data);
            if let Some(justhtml::NodeData::Text(ref text)) = child.data {
                println!("    Text bytes: {:?}", text.as_bytes());
            }
        }
    }

    // Test spaces (should be preserved)
    println!("\n\n=== Three spaces ===");
    let html3 = "<math><mi>   </mi></math>";
    let doc3 = JustHTML::parse(html3);
    let output3 = to_test_format(&doc3.root);
    println!("\nActual output:");
    println!("{}", output3);
    println!("\nExpected: should have \"   \" text node");

    // Test table with form feed
    println!("\n\n=== Table with form feed ===");
    let html4 = "<table>\x0c</table>";
    println!("Input bytes: {:?}", html4.as_bytes());
    let doc4 = JustHTML::parse(html4);
    let output4 = to_test_format(&doc4.root);
    println!("\nActual output:");
    println!("{}", output4);
    println!("\nExpected: <table> with no children");
}
