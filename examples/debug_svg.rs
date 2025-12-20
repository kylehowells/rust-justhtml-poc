use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    let html = "<!><svg><th><title><n><select><td>";
    println!("Input: {:?}", html);
    let doc = JustHTML::parse(html);
    println!("\nActual output:");
    println!("{}", to_test_format(&doc.root));
    
    // Let's also check a simpler case
    println!("\n\n=== Simpler test ===");
    let html2 = "<select><td>text";
    println!("Input: {:?}", html2);
    let doc2 = JustHTML::parse(html2);
    println!("{}", to_test_format(&doc2.root));
}
