use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // Test without table
    let html1 = "<!doctype html><i>a<b>b<div>c";
    println!("Input (no table): {:?}", html1);
    let doc1 = JustHTML::parse(html1);
    println!("Output:");
    println!("{}", to_test_format(&doc1.root));
    println!();
    
    // Test with table
    let html2 = "<!doctype html><table><i>a<b>b<div>c";
    println!("Input (with table): {:?}", html2);
    let doc2 = JustHTML::parse(html2);
    println!("Output:");
    println!("{}", to_test_format(&doc2.root));
}
