use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // namespace-sensitivity.dat test 1
    let html = "<body><table><tr><td><svg><td><foreignObject><span></td>Foo";
    println!("Input: {:?}", html);

    let doc = JustHTML::parse(html);
    println!("\nResult:\n{}", to_test_format(&doc.root));

    // Expected:
    // | <html>
    // |   <head>
    // |   <body>
    // |     "Foo"
    // |     <table>
    // |       <tbody>
    // |         <tr>
    // |           <td>
    // |             <svg svg>
    // |               <svg td>
    // |                 <svg foreignObject>
    // |                   <span>
}
