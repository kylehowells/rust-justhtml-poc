use justhtml::JustHTML;
use justhtml::serialize::to_test_format;
use std::fs;

fn main() {
    let content = fs::read_to_string("/home/kyle/Development/justhtml/html5lib-tests/tree-construction/treebuilder_coverage.dat")
        .expect("Failed to read test file");

    let mut in_data = false;
    let mut in_errors = false;
    let mut in_document = false;
    let mut current_input = String::new();
    let mut expected_output = String::new();
    let mut test_num = 0;

    for line in content.lines() {
        if line == "#data" {
            // Start of new test
            in_data = true;
            in_errors = false;
            in_document = false;
            current_input.clear();
            expected_output.clear();
            test_num += 1;
        } else if line == "#errors" {
            in_data = false;
            in_errors = true;
        } else if line == "#document" {
            in_errors = false;
            in_document = true;
        } else if line.starts_with("#") {
            // Some other directive (like #script-off, #document-fragment)
            if in_document {
                // End of this test
                let actual = {
                    let doc = JustHTML::parse(&current_input);
                    to_test_format(&doc.root)
                };

                let expected_trimmed = expected_output.trim();
                let actual_trimmed = actual.trim();

                if expected_trimmed != actual_trimmed {
                    println!("=== Test {} FAILED ===", test_num);
                    println!("Input: {:?}", current_input);
                    println!("\nExpected:");
                    println!("{}", expected_trimmed);
                    println!("\nActual:");
                    println!("{}", actual_trimmed);
                    println!();
                }

                in_document = false;
            }
        } else if in_data {
            if !current_input.is_empty() {
                current_input.push('\n');
            }
            current_input.push_str(line);
        } else if in_document {
            if !expected_output.is_empty() {
                expected_output.push('\n');
            }
            expected_output.push_str(line);
        }
    }

    // Handle last test
    if in_document && !expected_output.is_empty() {
        let actual = {
            let doc = JustHTML::parse(&current_input);
            to_test_format(&doc.root)
        };

        let expected_trimmed = expected_output.trim();
        let actual_trimmed = actual.trim();

        if expected_trimmed != actual_trimmed {
            println!("=== Test {} FAILED ===", test_num);
            println!("Input: {:?}", current_input);
            println!("\nExpected:");
            println!("{}", expected_trimmed);
            println!("\nActual:");
            println!("{}", actual_trimmed);
            println!();
        }
    }
}
