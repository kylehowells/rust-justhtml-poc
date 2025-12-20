use std::fs;

use justhtml::{JustHTML, FragmentContext, Namespace};
use justhtml::serialize::to_test_format;

struct TestCase {
    data: String,
    expected: String,
    fragment_context: Option<String>,
}

fn parse_test_file(content: &str) -> Vec<TestCase> {
    let mut tests = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        while i < lines.len() && lines[i].is_empty() { i += 1; }
        if i >= lines.len() { break; }
        if lines[i] != "#data" { i += 1; continue; }
        i += 1;

        let mut data_lines = Vec::new();
        while i < lines.len() && !lines[i].starts_with('#') {
            data_lines.push(lines[i]);
            i += 1;
        }
        let data = data_lines.join("\n");

        if i < lines.len() && lines[i] == "#errors" {
            i += 1;
            while i < lines.len() && !lines[i].starts_with('#') { i += 1; }
        }
        if i < lines.len() && lines[i] == "#new-errors" {
            i += 1;
            while i < lines.len() && !lines[i].starts_with('#') { i += 1; }
        }
        if i < lines.len() && (lines[i] == "#script-on" || lines[i] == "#script-off") { i += 1; }

        let mut fragment_context = None;
        if i < lines.len() && lines[i] == "#document-fragment" {
            i += 1;
            if i < lines.len() && !lines[i].starts_with('#') {
                fragment_context = Some(lines[i].to_string());
                i += 1;
            }
        }

        if i >= lines.len() || lines[i] != "#document" { continue; }
        i += 1;

        let mut expected_lines = Vec::new();
        while i < lines.len() && !lines[i].is_empty() && !lines[i].starts_with('#') {
            expected_lines.push(lines[i]);
            i += 1;
        }
        let expected = expected_lines.join("\n");

        tests.push(TestCase { data, expected, fragment_context });
    }
    tests
}

fn main() {
    let content = fs::read_to_string("/home/kyle/Development/justhtml/html5lib-tests/tree-construction/template.dat")
        .expect("Failed to read test file");

    let tests = parse_test_file(&content);

    for (i, test) in tests.iter().enumerate() {
        let doc = if let Some(ref ctx) = test.fragment_context {
            let ns = if ctx.contains("svg ") {
                Namespace::Svg
            } else if ctx.contains("math ") {
                Namespace::MathML
            } else {
                Namespace::Html
            };
            let tag = ctx.split_whitespace().last().unwrap_or(ctx);
            let context = FragmentContext::with_namespace(tag, ns);
            JustHTML::parse_fragment(&test.data, context)
        } else {
            JustHTML::parse(&test.data)
        };

        let result = to_test_format(&doc.root);
        let result = result.trim();
        let expected = test.expected.trim();

        if result != expected {
            println!("=== Test {} FAILED ===", i + 1);
            println!("Input: {:?}", test.data);
            if test.fragment_context.is_some() {
                println!("Fragment context: {:?}", test.fragment_context);
            }
            println!("\nExpected:\n{}", expected);
            println!("\nGot:\n{}", result);
            println!("\n");
        }
    }
}
