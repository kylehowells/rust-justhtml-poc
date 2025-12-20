use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    let html = "<head></head> <style></style>ddd";
    println!("Input: {}", html);

    let doc = JustHTML::parse(html);
    println!("\nResult:\n{}", to_test_format(&doc.root));
}
