//! Benchmark tool for rust-justhtml
//!
//! Usage: benchmark [--json] < file.html
//!        benchmark [--json] --file path/to/file.html
//!        benchmark [--json] --dir path/to/html/files --limit 100
//!        benchmark --batch < files.jsonl  (reads JSON lines with {"html": "..."})

use std::env;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::time::Instant;

use justhtml::JustHTML;

fn parse_and_time(html: &str) -> (f64, usize) {
    let start = Instant::now();
    let doc = JustHTML::parse(html);
    let elapsed = start.elapsed().as_secs_f64();

    // Count nodes to ensure the tree was actually built
    fn count_nodes(node: &justhtml::Node) -> usize {
        1 + node.children.iter().map(count_nodes).sum::<usize>()
    }
    let node_count = count_nodes(&doc.root);

    (elapsed, node_count)
}

fn benchmark_stdin(json_output: bool) {
    let mut html = String::new();
    io::stdin().read_to_string(&mut html).expect("Failed to read stdin");

    let (elapsed, node_count) = parse_and_time(&html);

    if json_output {
        println!(r#"{{"time": {}, "bytes": {}, "nodes": {}}}"#, elapsed, html.len(), node_count);
    } else {
        println!("Parsed {} bytes in {:.6}s ({} nodes)", html.len(), elapsed, node_count);
    }
}

/// Batch mode: reads HTML content from stdin, one document per line (length-prefixed)
/// Protocol: Each document is sent as: <length>\n<html_content>
/// After all documents, sends: 0\n to signal end
/// Returns JSON with aggregate stats
fn benchmark_batch() {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    let mut total_time = 0.0;
    let mut total_bytes = 0usize;
    let mut total_nodes = 0usize;
    let mut file_count = 0usize;
    let mut times: Vec<f64> = Vec::new();

    loop {
        // Read length line
        let mut len_line = String::new();
        if reader.read_line(&mut len_line).unwrap() == 0 {
            break;
        }

        let len: usize = match len_line.trim().parse() {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };

        // Read exact number of bytes
        let mut html = vec![0u8; len];
        reader.read_exact(&mut html).expect("Failed to read HTML content");

        let html = String::from_utf8_lossy(&html);
        let (elapsed, node_count) = parse_and_time(&html);

        total_time += elapsed;
        total_bytes += len;
        total_nodes += node_count;
        file_count += 1;
        times.push(elapsed);
    }

    let mean_time = if file_count > 0 { total_time / file_count as f64 } else { 0.0 };
    let min_time = times.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_time = times.iter().cloned().fold(0.0f64, f64::max);

    let result = format!(
        r#"{{"total_time": {}, "mean_time": {}, "min_time": {}, "max_time": {}, "total_bytes": {}, "total_nodes": {}, "file_count": {}, "errors": 0}}"#,
        total_time, mean_time,
        if min_time.is_finite() { min_time } else { 0.0 },
        max_time, total_bytes, total_nodes, file_count
    );

    writeln!(writer, "{}", result).unwrap();
}

fn benchmark_file(path: &Path, json_output: bool) {
    let html = fs::read_to_string(path).expect("Failed to read file");
    let (elapsed, node_count) = parse_and_time(&html);

    if json_output {
        println!(r#"{{"file": "{}", "time": {}, "bytes": {}, "nodes": {}}}"#,
            path.display(), elapsed, html.len(), node_count);
    } else {
        println!("{}: {} bytes in {:.6}s ({} nodes)",
            path.display(), html.len(), elapsed, node_count);
    }
}

fn benchmark_dir(dir: &Path, limit: Option<usize>, json_output: bool) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("Failed to read directory")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension()
                .map_or(false, |ext| ext == "html" || ext == "htm")
        })
        .collect();

    entries.sort_by_key(|e| e.path());

    if let Some(limit) = limit {
        entries.truncate(limit);
    }

    let mut total_time = 0.0;
    let mut total_bytes = 0usize;
    let mut total_nodes = 0usize;
    let mut file_count = 0usize;
    let mut errors = 0usize;

    for entry in &entries {
        let path = entry.path();
        match fs::read_to_string(&path) {
            Ok(html) => {
                let (elapsed, node_count) = parse_and_time(&html);
                total_time += elapsed;
                total_bytes += html.len();
                total_nodes += node_count;
                file_count += 1;
            }
            Err(_) => {
                errors += 1;
            }
        }
    }

    if json_output {
        println!(r#"{{"total_time": {}, "total_bytes": {}, "total_nodes": {}, "file_count": {}, "errors": {}, "mean_time": {}}}"#,
            total_time, total_bytes, total_nodes, file_count, errors,
            if file_count > 0 { total_time / file_count as f64 } else { 0.0 });
    } else {
        println!("Parsed {} files ({} bytes) in {:.6}s", file_count, total_bytes, total_time);
        println!("Mean time: {:.6}s per file", total_time / file_count.max(1) as f64);
        println!("Throughput: {:.2} MB/s", (total_bytes as f64 / 1024.0 / 1024.0) / total_time);
        if errors > 0 {
            println!("Errors: {}", errors);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut json_output = false;
    let mut batch_mode = false;
    let mut file_path: Option<String> = None;
    let mut dir_path: Option<String> = None;
    let mut limit: Option<usize> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json_output = true,
            "--batch" => batch_mode = true,
            "--file" => {
                i += 1;
                file_path = args.get(i).cloned();
            }
            "--dir" => {
                i += 1;
                dir_path = args.get(i).cloned();
            }
            "--limit" => {
                i += 1;
                limit = args.get(i).and_then(|s| s.parse().ok());
            }
            "--help" | "-h" => {
                println!("Usage: benchmark [OPTIONS]");
                println!();
                println!("Options:");
                println!("  --json         Output results as JSON");
                println!("  --batch        Batch mode: read length-prefixed HTML from stdin");
                println!("  --file PATH    Parse a single file");
                println!("  --dir PATH     Parse all HTML files in directory");
                println!("  --limit N      Limit number of files to parse");
                println!();
                println!("Without --file or --dir, reads HTML from stdin.");
                println!();
                println!("Batch mode protocol:");
                println!("  Send: <length>\\n<html_content> for each document");
                println!("  Send: 0\\n to signal end");
                println!("  Receives: JSON with aggregate stats");
                return;
            }
            _ => {}
        }
        i += 1;
    }

    if batch_mode {
        benchmark_batch();
    } else if let Some(dir) = dir_path {
        benchmark_dir(Path::new(&dir), limit, json_output);
    } else if let Some(file) = file_path {
        benchmark_file(Path::new(&file), json_output);
    } else {
        benchmark_stdin(json_output);
    }
}
