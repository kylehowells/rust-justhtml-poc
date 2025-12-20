use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    let html = "<a><p>1<a><p>2";
    let doc = JustHTML::parse(html);
    println!("Input: {}", html);
    println!("Output:");
    println!("{}", to_test_format(&doc.root));
}
