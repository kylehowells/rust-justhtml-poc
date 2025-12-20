use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    let html = "<div><svg><path><foreignObject><math></div>a";
    println!("Input: {}", html);

    let doc = JustHTML::parse(html);
    println!("\nResult:\n{}", to_test_format(&doc.root));
    println!("\nErrors: {:?}", doc.errors);
}
