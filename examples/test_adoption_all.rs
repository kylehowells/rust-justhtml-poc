use justhtml::JustHTML;
use justhtml::serialize::to_test_format;

fn test(name: &str, html: &str, expected: &str) {
    let doc = JustHTML::parse(html);
    let actual = to_test_format(&doc.root);
    let actual_trimmed = actual.trim();
    let expected_trimmed = expected.trim();
    if actual_trimmed != expected_trimmed {
        println!("FAIL: {}", name);
        println!("Input: {:?}", html);
        println!("Expected:\n{}", expected);
        println!("Actual:\n{}", actual);
        println!();
    } else {
        println!("PASS: {}", name);
    }
}

fn main() {
    test("test4 a>b", "<a>1<b>2</a>3</b>", 
r#"| <html>
|   <head>
|   <body>
|     <a>
|       "1"
|       <b>
|         "2"
|     <b>
|       "3""#);

    test("test5 a>div>div", "<a>1<div>2<div>3</a>4</div>5</div>",
r#"| <html>
|   <head>
|   <body>
|     <a>
|       "1"
|     <div>
|       <a>
|         "2"
|       <div>
|         <a>
|           "3"
|         "4"
|       "5""#);
}
