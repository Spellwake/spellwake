//! CI Anti-Gaming Integrity Gate for `spellwake-oracle-compiler`.
//!
//! Enforces non-negotiable parser hygiene invariants across all source files in
//! `crates/oracle_compiler/src/` so audit-metric gaming, silent predicate erasure,
//! single-line pipe-packed phrase blobs, literal card-name hardcoding, and space-stripping
//! prose laundering can never re-enter the compiler.

use std::fs;
use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).expect("read_dir crates/oracle_compiler/src");
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn anti_gaming_integrity_gate() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&src_root, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "expected Rust source files under {}",
        src_root.display()
    );

    const BANNED_CARD_NAMES: &[&str] = &[
        "shang-chi",
        "jaces",
        "peer through depths",
        "reach through mists",
        "eye of vecna",
        "snitch",
    ];

    let mut violations = Vec::new();

    for path in &files {
        let rel = path
            .strip_prefix(&src_root)
            .unwrap_or(path)
            .display()
            .to_string();
        let content = fs::read_to_string(path).expect("read source file");

        for (line_idx, line) in content.lines().enumerate() {
            let line_no = line_idx + 1;
            let low = line.to_ascii_lowercase();

            // 1. No DROP_SUFFIXES or silent predicate-erasure suffix lists.
            if line.contains("DROP_SUFFIXES") {
                violations.push(format!(
                    "{rel}:{line_no}: banned `DROP_SUFFIXES` silent predicate-erasure list"
                ));
            }

            // 2. No `*_pipe` combinators.
            if has_pipe_combinator_ident(line) {
                violations.push(format!(
                    "{rel}:{line_no}: banned `*_pipe` combinator identifier"
                ));
            }

            // 3. No single-line `|`-packed string blobs (> 200 chars with >= 4 pipes).
            let trimmed = line.trim_start();
            if !trimmed.starts_with("//") && line.len() > 200 {
                let pipe_count = line.bytes().filter(|&b| b == b'|').count();
                if pipe_count >= 4 && line.contains('"') {
                    violations.push(format!(
                        "{rel}:{line_no}: banned pipe-packed string blob ({} chars, {pipe_count} pipes)",
                        line.len()
                    ));
                }
            }

            // 4. No literal card names hardcoded in parser source.
            for &card_name in BANNED_CARD_NAMES {
                if low.contains(card_name) {
                    violations.push(format!(
                        "{rel}:{line_no}: banned literal card name `{card_name}`"
                    ));
                }
            }

            // 5. No `.replace(' ', "_")` prose-laundering hacks.
            if line.contains(".replace(' ', \"_\")") || line.contains(".replace(\" \", \"_\")") {
                violations.push(format!(
                    "{rel}:{line_no}: banned `.replace(' ', \"_\")` prose-laundering hack"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Anti-gaming integrity gate FAILED with {} violation(s):\n{}",
        violations.len(),
        violations.join("\n")
    );
}

fn has_pipe_combinator_ident(line: &str) -> bool {
    let bytes = line.as_bytes();
    let needle = b"_pipe";
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let prev_ident =
                i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            let next_boundary = i + needle.len() == bytes.len()
                || !(bytes[i + needle.len()].is_ascii_alphanumeric()
                    || bytes[i + needle.len()] == b'_');
            if prev_ident && next_boundary {
                return true;
            }
        }
        i += 1;
    }
    false
}
