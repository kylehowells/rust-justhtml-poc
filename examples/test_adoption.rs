use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // Test 6 from adoption01.dat
    let html = "<table><a>1<p>2</a>3</p>";
    println!("Input: {:?}", html);
    let doc = JustHTML::parse(html);
    println!("\nActual output:");
    println!("{}", to_test_format(&doc.root));
    println!("\nExpected:");
    println!("| <html>");
    println!("|   <head>");
    println!("|   <body>");
    println!("|     <a>");
    println!("|       \"1\"");
    println!("|     <p>");
    println!("|       <a>");
    println!("|         \"2\"");
    println!("|       \"3\"");
    println!("|     <table>");
}
