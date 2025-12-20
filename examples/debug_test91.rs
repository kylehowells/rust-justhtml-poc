use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    let html = "<!doctype html><table><i>a<b>b<div>c<a>d</i>e</b>f";
    println!("Input: {:?}", html);
    let doc = JustHTML::parse(html);
    println!("\nActual:");
    println!("{}", to_test_format(&doc.root));
}
