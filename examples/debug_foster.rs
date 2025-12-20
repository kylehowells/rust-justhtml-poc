use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    let html = "<table><div>x<div></div>x</span>x";
    println!("Input: {:?}", html);
    let doc = JustHTML::parse(html);
    println!("\nActual output:");
    println!("{}", to_test_format(&doc.root));
    println!("\nExpected:");
    println!("| <html>");
    println!("|   <head>");
    println!("|   <body>");
    println!("|     <div>");
    println!("|       \"x\"");
    println!("|       <div>");
    println!("|       \"xx\"");
    println!("|     <table>");
}
