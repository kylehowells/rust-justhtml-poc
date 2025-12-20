use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // The expected output shows:
    // | <html>
    // |   <head>
    // |   <body>
    // |     "Foo"           <- foster parented to body
    // |     <table>
    // |       <tbody>
    // |         <tr>
    // |           <td>
    // |             <svg svg>
    // |               <svg td>
    // |                 <svg foreignObject>
    // |                   <span>

    let html = "<body><table><tr><td><svg><td><foreignObject><span></td>Foo";
    let doc = JustHTML::parse(html);
    println!("Input: {}", html);
    println!("\nExpected output:");
    println!("| <html>");
    println!("|   <head>");
    println!("|   <body>");
    println!("|     \"Foo\"");
    println!("|     <table>");
    println!("|       <tbody>");
    println!("|         <tr>");
    println!("|           <td>");
    println!("|             <svg svg>");
    println!("|               <svg td>");
    println!("|                 <svg foreignObject>");
    println!("|                   <span>");
    println!("\nActual output:");
    println!("{}", to_test_format(&doc.root));
}
