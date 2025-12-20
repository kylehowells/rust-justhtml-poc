use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // Test 1
    let html1 = "<frameset></frameset></frameset>";
    println!("Test 1: {:?}", html1);
    let doc1 = JustHTML::parse(html1);
    println!("Actual:");
    println!("{}", to_test_format(&doc1.root));
    println!("\nExpected:");
    println!("| <html>");
    println!("|   <head>");
    println!("|   <frameset>");
    println!();

    // Test 2
    let html2 = "<figure><frameset><h1><aside><figcaption><html><frameset>";
    println!("Test 2: {:?}", html2);
    let doc2 = JustHTML::parse(html2);
    println!("Actual:");
    println!("{}", to_test_format(&doc2.root));
    println!("\nExpected:");
    println!("| <html>");
    println!("|   <head>");
    println!("|   <frameset>");
}
