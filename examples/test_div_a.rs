use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    let html = "<div><a><b><div><div><div><div><div><div><div><div><div><div></a>";
    println!("Input: {:?}", html);
    let doc = JustHTML::parse(html);
    println!("\nActual:");
    println!("{}", to_test_format(&doc.root));
}
