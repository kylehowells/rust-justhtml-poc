use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // Test with table
    let html = "<!doctype html><table><i>a<b>b<div>c";
    println!("Input: {:?}", html);
    
    // Enable debug
    std::env::set_var("DEBUG_RECONSTRUCT", "1");
    
    let doc = JustHTML::parse(html);
    println!("\nOutput:");
    println!("{}", to_test_format(&doc.root));
}
