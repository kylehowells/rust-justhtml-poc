use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    let html = "<a><b><big><em><strong><div>X</a>";
    let doc = JustHTML::parse(html);
    println!("Input: {}", html);
    println!("\nExpected:");
    println!("| <html>");
    println!("|   <head>");
    println!("|   <body>");
    println!("|     <a>");
    println!("|       <b>");
    println!("|         <big>");
    println!("|           <em>");
    println!("|             <strong>");
    println!("|     <big>");
    println!("|       <em>");
    println!("|         <strong>");
    println!("|           <div>");
    println!("|             <a>");
    println!("|               \"X\"");
    println!("\nActual:");
    println!("{}", to_test_format(&doc.root));
}
