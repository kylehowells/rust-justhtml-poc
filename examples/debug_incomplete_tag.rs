use justhtml::{JustHTML, FragmentContext};
use justhtml::serialize::to_test_format;

fn main() {
    // Test 19 from webkit02.dat
    let html = "<option><XH<optgroup></optgroup>";
    println!("Input: {:?}", html);
    let doc = JustHTML::parse_fragment(html, FragmentContext::new("select"));
    println!("\nActual output:");
    println!("{}", to_test_format(&doc.root));
    println!("\nExpected:");
    println!("| <option>");
    println!("|   <xh<optgroup>");

    // Let's also test what the tokenizer outputs for <XH<
    println!("\n\n=== Debug tokenizer for '<XH<optgroup>' ===");
    let test = "<XH<optgroup>";
    println!("Input: {:?}", test);
    // This should become text "<xh<optgroup>"
}
