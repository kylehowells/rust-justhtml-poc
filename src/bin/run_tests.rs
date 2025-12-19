//! Run html5lib tree construction tests

use std::env;
use std::fs;
use std::path::Path;

use justhtml::{JustHTML, FragmentContext};
use justhtml::serialize::to_test_format;

struct TestCase {
    data: String,
    expected: String,
    fragment_context: Option<String>,
    scripting: bool,
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

        // Collect data until next section
        let mut data_lines = Vec::new();
        while i < lines.len() && !lines[i].starts_with('#') {
            data_lines.push(lines[i]);
            i += 1;
        }
        let data = data_lines.join("\n");

        // Skip #errors section
        if i < lines.len() && lines[i] == "#errors" {
            i += 1;
            while i < lines.len() && !lines[i].starts_with('#') {
                i += 1;
            }
        }

        // Check for #new-errors section
        if i < lines.len() && lines[i] == "#new-errors" {
            i += 1;
            while i < lines.len() && !lines[i].starts_with('#') {
                i += 1;
            }
        }

        // Check for scripting flag
        let mut scripting = false;
        if i < lines.len() && lines[i] == "#script-on" {
            scripting = true;
            i += 1;
        }
        if i < lines.len() && lines[i] == "#script-off" {
            scripting = false;
            i += 1;
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

        // Collect expected output
        let mut expected_lines = Vec::new();
        while i < lines.len() && !lines[i].is_empty() && !lines[i].starts_with('#') {
            expected_lines.push(lines[i]);
            i += 1;
        }
        let expected = expected_lines.join("\n");

        tests.push(TestCase {
            data,
            expected,
            fragment_context,
            scripting,
        });
    }

    tests
}

fn run_test(test: &TestCase) -> (bool, String, String) {
    let doc = if let Some(ref context) = test.fragment_context {
        let ctx = FragmentContext::new(context);
        JustHTML::parse_fragment(&test.data, ctx)
    } else {
        JustHTML::parse(&test.data)
    };

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

        // Skip unsafe tests
        if file_name.contains("unsafe") {
            continue;
        }

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
            // Skip scripting tests
            if test.scripting {
                continue;
            }

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
