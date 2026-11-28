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

            // 6. v2-specific fail-closed integrity checks.
            let rel_norm = rel.replace('\\', "/");
            if rel_norm.starts_with("v2/") && !trimmed.starts_with("//") {
                // 6a. No empty no-op replacements, empty riders, or empty additional costs.
                const BANNED_V2_NOOPS: &[&str] = &[
                    "EnterReplacementNoop",
                    "AS_ENTERS_EMPTY_SEQS",
                    "instead: Vec::new()",
                    "AsEnters(Vec::new())",
                    "rider: Vec::new()",
                    "AdditionalCost(Vec::new())",
                ];
                for &pat in BANNED_V2_NOOPS {
                    if line.contains(pat) {
                        violations.push(format!("{rel}:{line_no}: banned v2 empty no-op `{pat}`"));
                    }
                }

                // 6b. No direct `ts.pos =` token-skipping outside `v2/lexer.rs` and `v2/parser.rs`.
                let is_core_stream = rel_norm == "v2/lexer.rs" || rel_norm == "v2/parser.rs";
                if !is_core_stream && has_direct_pos_assign(line) {
                    violations.push(format!(
                        "{rel}:{line_no}: banned direct `pos =` token-skipping (use `commit_substream_eof` after strict sub-stream EOF parse)"
                    ));
                }

                // 6c. No fabricated `GlobalTriggerKind::DayNightChanged` outside `v2/ir/` and `v2/trigger/`.
                if !rel_norm.starts_with("v2/ir/")
                    && !rel_norm.starts_with("v2/trigger/")
                    && line.contains("DayNightChanged")
                {
                    violations.push(format!(
                        "{rel}:{line_no}: banned `DayNightChanged` trigger fabrication outside `v2/trigger/`"
                    ));
                }

                // 6d. No verbatim single-card `eat_seq` / `eat_rep_seq` strings with >= 10 literal words.
                if (line.contains("eat_seq(") || line.contains("eat_rep_seq("))
                    && has_long_seq_literal(line, 10)
                {
                    violations.push(format!(
                        "{rel}:{line_no}: banned verbatim single-card `eat_seq`/`eat_rep_seq` matcher (>= 10 literal words)"
                    ));
                }
            }
        }

        // Multi-line check for `eat_seq` / `eat_rep_seq`, `eat_phrase(&[>=10 words])`, and empty `ForEach`/`BindVariable` bodies.
        let rel_norm = rel.replace('\\', "/");
        if rel_norm.starts_with("v2/") {
            for (line_no, snippet) in find_multiline_long_seq_literals(&content, 10) {
                violations.push(format!(
                    "{rel}:{line_no}: banned multiline verbatim `eat_seq`/`eat_rep_seq` matcher (>= 10 literal words): `{snippet}`"
                ));
            }
            if !rel_norm.starts_with("v2/ir/") {
                for (line_no, word_count, snippet) in find_long_eat_phrase_calls(&content, 10) {
                    violations.push(format!(
                        "{rel}:{line_no}: banned verbatim `eat_phrase(&[...])` matcher ({word_count} literal words >= 10): `{snippet}`"
                    ));
                }
                for (line_no, kind) in find_empty_effect_container_bodies(&content) {
                    violations.push(format!(
                        "{rel}:{line_no}: banned empty `body: Vec::new()` inside `Effect::{kind}`"
                    ));
                }
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

fn has_direct_pos_assign(line: &str) -> bool {
    let bytes = line.as_bytes();
    let needle = b".pos";
    let mut i = 0;
    while i + needle.len() < bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let mut j = i + needle.len();
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'=' && bytes.get(j + 1).copied() != Some(b'=') {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn has_long_seq_literal(line: &str, min_words: usize) -> bool {
    let mut rest = line;
    while let Some(q1) = rest.find('"') {
        let after = &rest[q1 + 1..];
        let Some(q2) = after.find('"') else { break };
        let lit = &after[..q2];
        if lit.split_whitespace().count() >= min_words {
            return true;
        }
        rest = &after[q2 + 1..];
    }
    false
}

fn find_multiline_long_seq_literals(content: &str, min_words: usize) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        if (line.contains("eat_seq(") || line.contains("eat_rep_seq(")) && !line.contains('"') {
            // Check next line for the string literal argument
            if let Some(next) = lines.get(idx + 1)
                && has_long_seq_literal(next, min_words)
            {
                out.push((idx + 2, next.trim().chars().take(60).collect()));
            }
        }
        // Also scan const arrays of seq strings passed to `eat_seq` / `eat_rep_seq`
        if (line.contains("_SEQS") || line.contains("SPECIAL_IF_WOULD"))
            && let Some(block_end) = lines[idx..].iter().position(|l| l.trim() == "];")
        {
            for (off, bl) in lines[idx..=idx + block_end].iter().enumerate() {
                if !bl.trim_start().starts_with("//") && has_long_seq_literal(bl, min_words) {
                    out.push((idx + off + 1, bl.trim().chars().take(60).collect()));
                }
            }
        }
    }
    out
}

fn find_long_eat_phrase_calls(content: &str, min_words: usize) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let bytes = content.as_bytes();
    let mut pos = 0;

    // 1. Scan single AND consecutively chained `.eat_phrase(&[...]) && <var>.eat_phrase(&[...])` runs.
    while pos < bytes.len() {
        let Some(rel_idx) = content[pos..].find(".eat_phrase(") else {
            break;
        };
        let call_start = pos + rel_idx;
        let line_no = content[..call_start]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1;
        let line_start = content[..call_start].rfind('\n').map_or(0, |i| i + 1);
        if content[line_start..call_start]
            .trim_start()
            .starts_with("//")
        {
            pos = call_start + ".eat_phrase(".len();
            continue;
        }
        let mut cursor = call_start;
        let mut total_words: Vec<&str> = Vec::new();
        loop {
            let after_open = cursor + ".eat_phrase(".len();
            let Some(close_rel) = content[after_open..].find(')') else {
                cursor = after_open;
                break;
            };
            let arg = &content[after_open..after_open + close_rel];
            if let (Some(lb), Some(rb)) = (arg.find('['), arg.rfind(']'))
                && rb > lb
            {
                extract_quoted_words(&arg[lb + 1..rb], &mut total_words);
            }
            cursor = after_open + close_rel + 1;
            let trimmed = content[cursor..].trim_start();
            let Some(after_and) = trimmed.strip_prefix("&&") else {
                break;
            };
            let after_and_ws = after_and.trim_start();
            let ident_len = after_and_ws
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                .count();
            if ident_len > 0 && after_and_ws[ident_len..].starts_with(".eat_phrase(") {
                let offset = (after_and_ws.as_ptr() as usize) - (content.as_ptr() as usize);
                cursor = offset + ident_len;
            } else {
                break;
            }
        }
        if total_words.len() >= min_words {
            let snippet = total_words.join(" ").chars().take(72).collect();
            out.push((line_no, total_words.len(), snippet));
        }
        pos = cursor;
    }

    // 2. Scan individual `&[...]` phrase entries inside `.eat_any_phrase(&[...])`.
    let mut apos = 0;
    while apos < bytes.len() {
        let Some(rel_idx) = content[apos..].find(".eat_any_phrase(") else {
            break;
        };
        let call_start = apos + rel_idx;
        let line_no = content[..call_start]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1;
        let line_start = content[..call_start].rfind('\n').map_or(0, |i| i + 1);
        if content[line_start..call_start]
            .trim_start()
            .starts_with("//")
        {
            apos = call_start + ".eat_any_phrase(".len();
            continue;
        }
        let after_open = call_start + ".eat_any_phrase(".len();
        if let Some(close_rel) = content[after_open..].find(')') {
            let arg = &content[after_open..after_open + close_rel];
            let mut inner_pos = arg.find("&[").map_or(0, |i| i + 2);
            while let Some(sub_lb) = arg[inner_pos..].find("&[") {
                let start = inner_pos + sub_lb + 2;
                let Some(sub_rb) = arg[start..].find(']') else {
                    break;
                };
                let slice_text = &arg[start..start + sub_rb];
                if !slice_text.contains("&[") {
                    let mut words = Vec::new();
                    extract_quoted_words(slice_text, &mut words);
                    if words.len() >= min_words {
                        let snippet = words.join(" ").chars().take(72).collect();
                        out.push((line_no, words.len(), snippet));
                    }
                }
                inner_pos = start + sub_rb + 1;
            }
        }
        apos = after_open;
    }

    out
}

fn extract_quoted_words<'a>(mut rest: &'a str, out: &mut Vec<&'a str>) {
    while let Some(q1) = rest.find('"') {
        let s = &rest[q1 + 1..];
        let Some(q2) = s.find('"') else { break };
        out.push(&s[..q2]);
        rest = &s[q2 + 1..];
    }
}

fn find_empty_effect_container_bodies(content: &str) -> Vec<(usize, &'static str)> {
    let mut out = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        let kind = if line.contains("Effect::ForEach") {
            Some("ForEach")
        } else if line.contains("Effect::BindVariable") {
            Some("BindVariable")
        } else {
            None
        };
        if let Some(kind) = kind {
            // Scan forward within this struct literal (up to 8 lines or closing brace)
            let end = (idx + 8).min(lines.len());
            for (off, l) in lines[idx..end].iter().enumerate() {
                let lt = l.trim();
                if lt.starts_with("//") {
                    continue;
                }
                if lt.contains("body: Vec::new()") || lt.contains("body: vec![]") {
                    out.push((idx + off + 1, kind));
                    break;
                }
                if off > 0 && (lt.starts_with('}') || lt.starts_with("})")) {
                    break;
                }
            }
        }
    }
    out
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

#[test]
fn gate_eat_any_phrase_flags_first_entry_over_threshold() {
    let sample = r#"
        let _ = p.eat_any_phrase(&[
            &["one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven"],
            &["short", "entry"],
        ]);
    "#;
    let hits = find_long_eat_phrase_calls(sample, 10);
    assert_eq!(
        hits.len(),
        1,
        "expected entry #0 with 11 words to be flagged, got {hits:?}"
    );
    assert_eq!(hits[0].1, 11);
}
