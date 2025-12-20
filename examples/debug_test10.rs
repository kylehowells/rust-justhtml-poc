use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // tests7.dat test 13
    let html = "<!doctype html><table><TBODY><script> <tr>x </script> </table>";
    println!("Input: {:?}", html);

    let doc = JustHTML::parse(html);
    println!("\nResult:\n{}", to_test_format(&doc.root));

    // Expected:
    // | <!DOCTYPE html>
    // | <html>
    // |   <head>
    // |   <body>
    // |     <table>
    // |       <tbody>
    // |         <script>
    // |           " <tr>x "
    // |         " "
}
