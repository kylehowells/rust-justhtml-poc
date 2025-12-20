use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    let html = "<!doctype html><i>a<b>b<div>c<a>d</i>e</b>f";
    println!("Input: {:?}", html);
    let doc = JustHTML::parse(html);
    println!("\nActual:");
    println!("{}", to_test_format(&doc.root));
    
    println!("\nExpected:");
    println!("| <!DOCTYPE html>");
    println!("| <html>");
    println!("|   <head>");
    println!("|   <body>");
    println!("|     <i>");
    println!("|       \"a\"");
    println!("|       <b>");
    println!("|         \"b\"");
    println!("|     <b>");
    println!("|     <div>");
    println!("|       <b>");
    println!("|         <i>");
    println!("|           \"c\"");
    println!("|           <a>");
    println!("|             \"d\"");
    println!("|         <a>");
    println!("|           \"e\"");
    println!("|       <a>");
    println!("|         \"f\"");
}
