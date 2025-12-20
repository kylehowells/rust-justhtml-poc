use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // Let's test in parts to see what's happening

    // Part 1: Just up to the span
    let html1 = "<body><table><tr><td><svg><td><foreignObject><span>";
    println!("Input 1: {:?}", html1);
    let doc1 = JustHTML::parse(html1);
    println!("Result 1:\n{}", to_test_format(&doc1.root));
    println!("\n---\n");

    // Part 2: With </td>
    let html2 = "<body><table><tr><td><svg><td><foreignObject><span></td>";
    println!("Input 2: {:?}", html2);
    let doc2 = JustHTML::parse(html2);
    println!("Result 2:\n{}", to_test_format(&doc2.root));
    println!("\n---\n");

    // Part 3: Full test
    let html3 = "<body><table><tr><td><svg><td><foreignObject><span></td>Foo";
    println!("Input 3: {:?}", html3);
    let doc3 = JustHTML::parse(html3);
    println!("Result 3:\n{}", to_test_format(&doc3.root));

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
