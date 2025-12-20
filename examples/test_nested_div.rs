use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    std::env::set_var("DEBUG_AA", "1");
    
    let html = "<a>1<div>2<div>3</a>4</div>5</div>";
    println!("Input: {:?}", html);
    let doc = JustHTML::parse(html);
    println!("\nActual output:");
    println!("{}", to_test_format(&doc.root));
}
