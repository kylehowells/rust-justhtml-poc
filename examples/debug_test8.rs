use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // Test 54
    let html = "<b id=a><p><b id=b></p></b>TEST";
    println!("Input: {}", html);

    let doc = JustHTML::parse(html);
    println!("\nResult:\n{}", to_test_format(&doc.root));

    // Expected:
    // | <html>
    // |   <head>
    // |   <body>
    // |     <b>
    // |       id="a"
    // |       <p>
    // |         <b>
    // |           id="b"
    // |       "TEST"
    //
    // Problem: TEST is inside a new <b id=a> instead of the original
}
