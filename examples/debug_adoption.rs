use justhtml::{JustHTML, FragmentContext, Namespace};
use justhtml::serialize::to_test_format;
use std::fs;

fn main() {
    let test_dir = "/home/kyle/Development/justhtml/html5lib-tests/tree-construction";
    
    let files = ["tests19.dat", "tests26.dat", "tricky01.dat"];
    
    for file_name in &files {
        let path = format!("{}/{}", test_dir, file_name);
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        
        let mut in_data = false;
        let mut in_document = false;
        let mut is_fragment = false;
        let mut fragment_context = String::new();
        let mut fragment_ns: Option<Namespace> = None;
        let mut current_input = String::new();
        let mut expected_output = String::new();
        let mut test_num = 0;
        
        for line in content.lines() {
            if line == "#data" {
                in_data = true;
                in_document = false;
                is_fragment = false;
                fragment_context.clear();
                fragment_ns = None;
                current_input.clear();
                expected_output.clear();
                test_num += 1;
            } else if line == "#errors" || line == "#new-errors" {
                in_data = false;
            } else if line.starts_with("#document-fragment") {
                is_fragment = true;
                let parts: Vec<&str> = line.strip_prefix("#document-fragment").unwrap_or("").trim().split_whitespace().collect();
                if parts.len() >= 2 {
                    fragment_ns = match parts[0] {
                        "svg" => Some(Namespace::Svg),
                        "math" => Some(Namespace::MathML),
                        _ => None,
                    };
                    fragment_context = parts[1].to_string();
                } else if !parts.is_empty() {
                    fragment_context = parts[0].to_string();
                }
            } else if line == "#document" {
                in_document = true;
            } else if line.starts_with("#") && in_document {
                // End of test - check result
                let actual = if is_fragment {
                    let ctx = if let Some(ns) = fragment_ns {
                        FragmentContext::with_namespace(&fragment_context, ns)
                    } else {
                        FragmentContext::new(&fragment_context)
                    };
                    let doc = JustHTML::parse_fragment(&current_input, ctx);
                    to_test_format(&doc.root)
                } else {
                    let doc = JustHTML::parse(&current_input);
                    to_test_format(&doc.root)
                };
                
                let expected_trimmed = expected_output.trim();
                let actual_trimmed = actual.trim();
                
                if expected_trimmed != actual_trimmed {
                    println!("=== {} Test {} FAILED ===", file_name, test_num);
                    println!("Input: {:?}", current_input);
                    if is_fragment {
                        println!("Fragment context: {:?} {}", fragment_ns, fragment_context);
                    }
                    println!("\nExpected:\n{}", expected_trimmed);
                    println!("\nActual:\n{}", actual_trimmed);
                    println!("\n");
                }
                
                in_document = false;
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
            let actual = if is_fragment {
                let ctx = if let Some(ns) = fragment_ns {
                    FragmentContext::with_namespace(&fragment_context, ns)
                } else {
                    FragmentContext::new(&fragment_context)
                };
                let doc = JustHTML::parse_fragment(&current_input, ctx);
                to_test_format(&doc.root)
            } else {
                let doc = JustHTML::parse(&current_input);
                to_test_format(&doc.root)
            };
            
            let expected_trimmed = expected_output.trim();
            let actual_trimmed = actual.trim();
            
            if expected_trimmed != actual_trimmed {
                println!("=== {} Test {} FAILED ===", file_name, test_num);
                println!("Input: {:?}", current_input);
                if is_fragment {
                    println!("Fragment context: {:?} {}", fragment_ns, fragment_context);
                }
                println!("\nExpected:\n{}", expected_trimmed);
                println!("\nActual:\n{}", actual_trimmed);
                println!("\n");
            }
        }
    }
}
