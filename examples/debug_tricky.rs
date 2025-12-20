use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // Test 6 from tricky01.dat
    let html = "<table><center> <font>a</center> <img> <tr><td> </td> </tr> </table>";
    println!("Input: {:?}", html);
    let doc = JustHTML::parse(html);
    println!("\nActual output:");
    println!("{}", to_test_format(&doc.root));
    println!("\nExpected key portion:");
    println!("| <font>");
    println!("|   <img>");
    println!("|   \" \"");
    println!("\nActual shows:");
    println!("| <font>");
    println!("|   \" \"");
    println!("| <img>");
}
