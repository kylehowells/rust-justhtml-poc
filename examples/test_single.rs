use justhtml::JustHTML;
use justhtml::serialize::to_test_format;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: test_single <html>");
        return;
    }
    let html = &args[1];
    let doc = JustHTML::parse(html);
    print!("{}", to_test_format(&doc.root).trim());
}
