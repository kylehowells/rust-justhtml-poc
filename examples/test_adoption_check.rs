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
    // Test 6 - table case
    test("test6_table", "<table><a>1<p>2</a>3</p>",
r#"| <html>
|   <head>
|   <body>
|     <a>
|       "1"
|     <p>
|       <a>
|         "2"
|       "3"
|     <table>"#);

    // Test 12 - table with td
    test("test12_table_td", "<table><a>1<td>2</td>3</table>",
r#"| <html>
|   <head>
|   <body>
|     <a>
|       "1"
|     <a>
|       "3"
|     <table>
|       <tbody>
|         <tr>
|           <td>
|             "2""#);
}
