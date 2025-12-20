//! Run html5lib tree construction tests

use std::env;
use std::fs;
use std::path::Path;

use justhtml::{JustHTML, FragmentContext, Namespace, ParseOptions};
use justhtml::serialize::to_test_format;

struct TestCase {
    data: String,
    expected: String,
    fragment_context: Option<String>,
    scripting: bool,
    iframe_srcdoc: bool,
    xml_coercion: bool,
}

/// Unescape \xNN, \n, and \u{NNNN} sequences in test data
fn unescape(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('x') => {
                    chars.next(); // consume 'x'
                    let mut hex = String::new();
                    if let Some(h1) = chars.next() {
                        hex.push(h1);
                    }
                    if let Some(h2) = chars.next() {
                        hex.push(h2);
                    }
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        result.push(byte as char);
                    } else {
                        result.push('\\');
                        result.push('x');
                        result.push_str(&hex);
                    }
                }
                Some('u') => {
                    chars.next(); // consume 'u'
                    if chars.peek() == Some(&'{') {
                        chars.next(); // consume '{'
                        let mut hex = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch == '}' {
                                chars.next(); // consume '}'
                                break;
                            }
                            hex.push(ch);
                            chars.next();
                        }
                        if let Ok(code) = u32::from_str_radix(&hex, 16) {
                            if let Some(ch) = char::from_u32(code) {
                                result.push(ch);
                            }
                        }
                    } else {
                        result.push('\\');
                        result.push('u');
                    }
                }
                Some('n') => {
                    chars.next();
                    result.push('\n');
                }
                Some('\\') => {
                    chars.next();
                    result.push('\\');
                }
                _ => {
                    result.push(c);
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn parse_test_file(content: &str) -> Vec<TestCase> {
    let mut tests = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        // Skip empty lines
        while i < lines.len() && lines[i].is_empty() {
            i += 1;
        }

        if i >= lines.len() {
            break;
        }

        // Expect #data
        if lines[i] != "#data" {
            i += 1;
            continue;
        }
        i += 1;

        // Initialize flags for this test
        let mut scripting = false;
        let mut iframe_srcdoc = false;
        let mut xml_coercion = false;

        // Check for directives that can appear right after #data
        while i < lines.len() {
            let line = lines[i];
            if line == "#script-on" {
                scripting = true;
                i += 1;
            } else if line == "#script-off" {
                scripting = false;
                i += 1;
            } else if line == "#iframe-srcdoc" {
                iframe_srcdoc = true;
                i += 1;
            } else if line == "#xml-coercion" {
                xml_coercion = true;
                i += 1;
            } else {
                break;
            }
        }

        // Collect data until next section
        let mut data_lines = Vec::new();
        while i < lines.len() && !lines[i].starts_with('#') {
            data_lines.push(lines[i]);
            i += 1;
        }
        // Apply unescape to handle \xNN and \n sequences
        let data = unescape(&data_lines.join("\n"));

        // Process remaining directives until #document
        while i < lines.len() {
            let line = lines[i];

            if line == "#errors" || line == "#new-errors" {
                i += 1;
                while i < lines.len() && !lines[i].starts_with('#') {
                    i += 1;
                }
            } else if line == "#script-on" {
                scripting = true;
                i += 1;
            } else if line == "#script-off" {
                scripting = false;
                i += 1;
            } else if line == "#iframe-srcdoc" {
                iframe_srcdoc = true;
                i += 1;
            } else if line == "#xml-coercion" {
                xml_coercion = true;
                i += 1;
            } else if line == "#document-fragment" {
                // Handle fragment context below
                break;
            } else if line == "#document" {
                break;
            } else {
                i += 1;
            }
        }

        // Check for fragment context
        let mut fragment_context = None;
        if i < lines.len() && lines[i] == "#document-fragment" {
            i += 1;
            if i < lines.len() && !lines[i].starts_with('#') {
                fragment_context = Some(lines[i].to_string());
                i += 1;
            }
        }

        // Expect #document
        if i >= lines.len() || lines[i] != "#document" {
            continue;
        }
        i += 1;

        // Collect expected output - continue until we hit a #section marker
        // Empty lines are valid inside quoted text nodes
        let mut expected_lines = Vec::new();
        while i < lines.len() && !lines[i].starts_with('#') {
            expected_lines.push(lines[i]);
            i += 1;
        }
        // Remove trailing empty lines (but not internal ones)
        while expected_lines.last().map_or(false, |l| l.is_empty()) {
            expected_lines.pop();
        }
        // Apply unescape to handle \xNN and \n sequences in expected output
        let expected = unescape(&expected_lines.join("\n"));

        tests.push(TestCase {
            data,
            expected,
            fragment_context,
            scripting,
            iframe_srcdoc,
            xml_coercion,
        });
    }

    tests
}

fn run_test(test: &TestCase) -> (bool, String, String) {
    let fragment_context = test.fragment_context.as_ref().map(|context| {
        // Parse context like "svg path" or "math ms"
        if context.starts_with("svg ") {
            FragmentContext::with_namespace(&context[4..], Namespace::Svg)
        } else if context.starts_with("math ") {
            FragmentContext::with_namespace(&context[5..], Namespace::MathML)
        } else {
            FragmentContext::new(context)
        }
    });

    let options = ParseOptions {
        fragment_context,
        scripting: test.scripting,
        iframe_srcdoc: test.iframe_srcdoc,
        xml_coercion: test.xml_coercion,
    };

    let doc = JustHTML::parse_with_options(&test.data, options);

    let actual = to_test_format(&doc.root);
    let passed = actual.trim() == test.expected.trim();

    (passed, actual, test.expected.clone())
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let test_dir = if args.len() > 1 {
        args[1].clone()
    } else {
        "../html5lib-tests/tree-construction".to_string()
    };

    let filter = args.get(2).map(|s| s.as_str());
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    let stop_on_fail = args.iter().any(|a| a == "-x" || a == "--stop");

    let path = Path::new(&test_dir);
    if !path.exists() {
        eprintln!("Test directory not found: {}", test_dir);
        std::process::exit(1);
    }

    let mut entries: Vec<_> = fs::read_dir(path)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "dat"))
        .collect();
    entries.sort_by_key(|e| e.path());

    let mut total_passed = 0;
    let mut total_failed = 0;
    let mut failed_files = Vec::new();

    for entry in entries {
        let file_path = entry.path();
        let file_name = file_path.file_name().unwrap().to_string_lossy();

        // Apply filter
        if let Some(f) = filter {
            if !file_name.contains(f) {
                continue;
            }
        }

        let content = match fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error reading {}: {}", file_name, e);
                continue;
            }
        };

        let tests = parse_test_file(&content);
        let mut file_passed = 0;
        let mut file_failed = 0;

        for (idx, test) in tests.iter().enumerate() {
            let (passed, actual, expected) = run_test(test);

            if passed {
                file_passed += 1;
                total_passed += 1;
            } else {
                file_failed += 1;
                total_failed += 1;

                if verbose || stop_on_fail {
                    println!("\n=== FAIL: {} test {} ===", file_name, idx + 1);
                    println!("Input: {:?}", test.data);
                    if test.fragment_context.is_some() {
                        println!("Fragment context: {:?}", test.fragment_context);
                    }
                    if test.scripting {
                        println!("Scripting: enabled");
                    }
                    if test.iframe_srcdoc {
                        println!("iframe-srcdoc: true");
                    }
                    if test.xml_coercion {
                        println!("xml-coercion: true");
                    }
                    println!("\nExpected:");
                    println!("{}", expected);
                    println!("\nActual:");
                    println!("{}", actual);
                    println!();
                }

                if stop_on_fail {
                    println!("\nStopping on first failure.");
                    println!("Total: {} passed, {} failed", total_passed, total_failed);
                    std::process::exit(1);
                }
            }
        }

        if file_failed > 0 {
            failed_files.push((file_name.to_string(), file_passed, file_failed));
        }

        let status = if file_failed == 0 { "✓" } else { "✗" };
        println!("{} {} - {} passed, {} failed",
                 status, file_name, file_passed, file_failed);
    }

    println!("\n========================================");
    println!("Total: {} passed, {} failed", total_passed, total_failed);

    if !failed_files.is_empty() {
        println!("\nFailed files:");
        for (name, passed, failed) in &failed_files {
            println!("  {} - {} passed, {} failed", name, passed, failed);
        }
    }

    let pass_rate = if total_passed + total_failed > 0 {
        100.0 * total_passed as f64 / (total_passed + total_failed) as f64
    } else {
        100.0
    };
    println!("\nPass rate: {:.2}%", pass_rate);

    if total_failed > 0 {
        std::process::exit(1);
    }
}
