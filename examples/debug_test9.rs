use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // tests7.dat test 5
    let html = "<!doctype html></head><base>X";
    println!("Input: {:?}", html);

    let doc = JustHTML::parse(html);
    println!("\nResult:\n{}", to_test_format(&doc.root));

    // Print document children to see order
    println!("\nDocument children count: {}", doc.root.children.len());
    for (i, child) in doc.root.children.iter().enumerate() {
        println!("Child {}: {} (children: {})", i, child.name, child.children.len());
    }

    // Expected:
    // | <!DOCTYPE html>
    // | <html>
    // |   <head>
    // |     <base>
    // |   <body>
    // |     "X"
}
