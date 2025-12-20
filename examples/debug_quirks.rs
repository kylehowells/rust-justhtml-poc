use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn main() {
    // Test 1: Frameset doctype - should NOT be quirks mode
    // <table> should close <p> (siblings)
    let html1 = "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Frameset//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-frameset.dtd\"><p><table>";
    println!("Test 1: Frameset");
    let doc1 = JustHTML::parse(html1);
    println!("{}", to_test_format(&doc1.root));
    // Expected: p and table are siblings

    println!("\n---\n");

    // Test 2: IBM system ID - should be quirks mode
    // <table> should stay inside <p>
    let html2 = "<!DOCTYPE html SYSTEM \"http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd\"><p><table>";
    println!("Test 2: IBM doctype");
    let doc2 = JustHTML::parse(html2);
    println!("{}", to_test_format(&doc2.root));
    // Expected: table inside p

    println!("\n---\n");

    // Test 3: Public ID "html" - should be quirks mode
    let html3 = "<!DOCTYPE html PUBLIC \"html\"><p><table>";
    println!("Test 3: Public 'html'");
    let doc3 = JustHTML::parse(html3);
    println!("{}", to_test_format(&doc3.root));
    // Expected: table inside p

    println!("\n---\n");

    // Test 4: HTML 3.2 - should be quirks mode
    let html4 = "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 3.2//EN\" \"http://www.w3.org/TR/html4/strict.dtd\"><p><table>";
    println!("Test 4: HTML 3.2");
    let doc4 = JustHTML::parse(html4);
    println!("{}", to_test_format(&doc4.root));
    // Expected: table inside p
}
