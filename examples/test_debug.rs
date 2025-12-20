use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    std::env::set_var("DEBUG_ADOPTION", "1");
    
    // Test 6 from adoption01.dat
    let html = "<table><a>1<p>2</a>3</p>";
    println!("Input: {:?}", html);
    let doc = JustHTML::parse(html);
    println!("\nActual output:");
    println!("{}", to_test_format(&doc.root));
}
