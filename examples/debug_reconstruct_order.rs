use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // Without table (test 92 - passes)
    let html1 = "<!doctype html><i>a<b>b<div>c";
    println!("Without table: {:?}", html1);
    let doc1 = JustHTML::parse(html1);
    println!("{}", to_test_format(&doc1.root));
    println!();
    
    // With table (test 91 - fails)
    let html2 = "<!doctype html><table><i>a<b>b<div>c";
    println!("With table: {:?}", html2);
    let doc2 = JustHTML::parse(html2);
    println!("{}", to_test_format(&doc2.root));
}
