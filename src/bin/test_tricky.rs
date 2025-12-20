use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    let html = "<b><p>Bold </b> Not bold</p>\nAlso not bold.";
    let doc = JustHTML::parse(html);
    println!("Input: {}", html);
    println!("\nExpected:");
    println!("| <html>");
    println!("|   <head>");
    println!("|   <body>");
    println!("|     <b>");
    println!("|     <p>");
    println!("|       <b>");
    println!("|         \"Bold \"");
    println!("|       \" Not bold\"");
    println!("|     \"\\nAlso not bold.\"");
    println!("\nActual:");
    println!("{}", to_test_format(&doc.root));
}
