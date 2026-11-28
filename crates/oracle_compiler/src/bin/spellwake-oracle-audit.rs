//! CLI auditing tool for Spellwake's Stage 1 Compositional Oracle Parser (`v2`).
//!
//! Subcommands:
//! - `audit-v2-corpus --snapshot <oracle-cards.jsonl[.gz]> --vocab <vocab.json> [--output <json>] [--dump-failures <jsonl>] [--all-cards]`
//! - `audit-corpus` (alias for `audit-v2-corpus`)
//! - `inspect --vocab <vocab.json> --name <name> --type-line <tl> --oracle-text <text>`

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use serde::Deserialize;
use spellwake_oracle_compiler::v2::{V2CorpusSummary, compile_card_v2};
use spellwake_oracle_compiler::{CardInput, FaceInput, Vocabulary};

#[derive(Deserialize)]
struct RawFace {
    name: String,
    #[serde(default)]
    type_line: Option<String>,
    #[serde(default)]
    oracle_text: Option<String>,
}

#[derive(Deserialize)]
struct RawCard {
    #[serde(default)]
    oracle_id: Option<String>,
    name: String,
    #[serde(default)]
    layout: String,
    #[serde(default)]
    type_line: Option<String>,
    #[serde(default)]
    oracle_text: Option<String>,
    #[serde(default, alias = "faces")]
    card_faces: Option<Vec<RawFace>>,
    #[serde(default)]
    legalities: BTreeMap<String, String>,
}

fn is_ordinary_base_format(legalities: &BTreeMap<String, String>) -> bool {
    [
        "commander",
        "standard",
        "pioneer",
        "modern",
        "legacy",
        "vintage",
    ]
    .iter()
    .any(|fmt| {
        matches!(
            legalities.get(*fmt).map(String::as_str),
            Some("legal" | "restricted")
        )
    })
}

fn run_v2_corpus_audit(
    snapshot: &Path,
    vocab_path: &Path,
    output: Option<&Path>,
    dump_failures: Option<&Path>,
    all_cards: bool,
) -> Result<ExitCode, String> {
    let vocab = Vocabulary::load_from_file(vocab_path).map_err(|e| {
        format!(
            "failed to load creature vocabulary {}: {e}",
            vocab_path.display()
        )
    })?;
    let mut summary = V2CorpusSummary::default();
    let mut failure_writer = match dump_failures {
        Some(p) => {
            let f =
                File::create(p).map_err(|e| format!("failed to create {}: {e}", p.display()))?;
            Some(BufWriter::new(f))
        }
        None => None,
    };

    let mut handle_raw = |raw: RawCard| {
        if !all_cards && !raw.legalities.is_empty() && !is_ordinary_base_format(&raw.legalities) {
            return;
        }
        let owned_faces: Vec<(String, String, String)> = match raw.card_faces {
            Some(fs) if !fs.is_empty() => fs
                .into_iter()
                .map(|f| {
                    (
                        f.name,
                        f.type_line.unwrap_or_default(),
                        f.oracle_text.unwrap_or_default(),
                    )
                })
                .collect(),
            _ => vec![(
                raw.name.clone(),
                raw.type_line.unwrap_or_default(),
                raw.oracle_text.unwrap_or_default(),
            )],
        };
        let face_inputs: Vec<FaceInput<'_>> = owned_faces
            .iter()
            .map(|(n, tl, ot)| FaceInput {
                name: n.as_str(),
                type_line: tl.as_str(),
                oracle_text: ot.as_str(),
            })
            .collect();
        let input = CardInput {
            oracle_id: raw.oracle_id.as_deref().unwrap_or(""),
            name: &raw.name,
            layout: &raw.layout,
            faces: face_inputs,
        };
        let compiled = compile_card_v2(&input, &vocab);
        if !compiled.strictly_compiled
            && let Some(ref mut w) = failure_writer
        {
            let errs: Vec<_> = compiled
                .faces
                .iter()
                .flat_map(|f| f.errors.iter())
                .collect();
            let rec = serde_json::json!({
                "name": compiled.name,
                "layout": compiled.layout,
                "error_count": errs.len(),
                "errors": errs,
            });
            let _ = serde_json::to_writer(&mut *w, &rec);
            let _ = w.write_all(b"\n");
        }
        summary.record(&compiled);
    };

    let mut handle_line = |line: &str, line_num: usize| -> Result<(), String> {
        if line.trim().is_empty() {
            return Ok(());
        }
        let raw = serde_json::from_str::<RawCard>(line)
            .map_err(|e| format!("malformed JSONL card at line {line_num}: {e}"))?;
        handle_raw(raw);
        Ok(())
    };

    let is_gz = snapshot
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("gz"));
    let is_json_array = snapshot
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if is_gz {
        let mut child = Command::new("gzip")
            .args(["-dc"])
            .arg(snapshot)
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to spawn gzip -dc {}: {e}", snapshot.display()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "gzip child missing stdout".to_string())?;
        let reader = BufReader::new(stdout);
        for (idx, line_res) in reader.lines().enumerate() {
            let line = line_res.map_err(|e| format!("i/o error at line {}: {e}", idx + 1))?;
            handle_line(&line, idx + 1)?;
        }
        let status = child
            .wait()
            .map_err(|e| format!("failed waiting on gzip: {e}"))?;
        if !status.success() {
            return Err(format!("gzip exited with status {status}"));
        }
    } else if is_json_array {
        let file = File::open(snapshot)
            .map_err(|e| format!("failed to open snapshot {}: {e}", snapshot.display()))?;
        let reader = BufReader::new(file);
        let cards: Vec<RawCard> = serde_json::from_reader(reader)
            .map_err(|e| format!("malformed JSON array snapshot {}: {e}", snapshot.display()))?;
        for raw in cards {
            handle_raw(raw);
        }
    } else {
        let file = File::open(snapshot)
            .map_err(|e| format!("failed to open snapshot {}: {e}", snapshot.display()))?;
        let reader = BufReader::new(file);
        for (idx, line_res) in reader.lines().enumerate() {
            let line = line_res.map_err(|e| format!("i/o error at line {}: {e}", idx + 1))?;
            handle_line(&line, idx + 1)?;
        }
    }

    let pct = if summary.total_cards == 0 {
        0.0
    } else {
        (summary.strictly_compiled_cards as f64 / summary.total_cards as f64) * 100.0
    };
    println!(
        "v2 strictly-typed corpus audit: {}/{} ({:.2}%) strictly compiled, {} failed",
        summary.strictly_compiled_cards, summary.total_cards, pct, summary.failed_cards
    );
    if !summary.error_stage_counts.is_empty() {
        println!("Error stage counts:");
        for (stage, count) in &summary.error_stage_counts {
            println!("  {count:>5}  {stage}");
        }
    }
    if !summary.error_message_counts.is_empty() {
        let mut buckets: Vec<(&String, &usize)> = summary.error_message_counts.iter().collect();
        buckets.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        println!("Top failure buckets:");
        for (bucket, count) in buckets.into_iter().take(20) {
            println!("  {count:>5}  {bucket}");
        }
    }
    if !summary.sample_failures.is_empty() {
        println!("Sample failures:");
        for sample in summary.sample_failures.iter().take(25) {
            println!("  - {sample}");
        }
    }
    if let Some(out_path) = output {
        let json = serde_json::to_string_pretty(&summary)
            .map_err(|e| format!("failed to serialize v2 summary: {e}"))?;
        std::fs::write(out_path, json)
            .map_err(|e| format!("failed to write {}: {e}", out_path.display()))?;
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(subcmd) = args.first().map(String::as_str) else {
        eprintln!(
            "Usage:\n  spellwake-oracle-audit audit-v2-corpus --snapshot <jsonl[.gz]> --vocab <vocab.json> [--output <json>] [--dump-failures <jsonl>] [--all-cards]\n  spellwake-oracle-audit inspect --vocab <vocab.json> --name <name> --type-line <tl> --oracle-text <text>"
        );
        return ExitCode::FAILURE;
    };

    match subcmd {
        "audit-v2-corpus" | "audit-corpus" => {
            let mut snapshot: Option<PathBuf> = None;
            let mut vocab: Option<PathBuf> = None;
            let mut output: Option<PathBuf> = None;
            let mut dump_failures: Option<PathBuf> = None;
            let mut all_cards = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--snapshot" if i + 1 < args.len() => {
                        snapshot = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--vocab" if i + 1 < args.len() => {
                        vocab = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--output" if i + 1 < args.len() => {
                        output = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--dump-failures" if i + 1 < args.len() => {
                        dump_failures = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--all-cards" => {
                        all_cards = true;
                        i += 1;
                    }
                    other => {
                        eprintln!("Unknown argument: {other}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            let (Some(snap), Some(voc)) = (snapshot, vocab) else {
                eprintln!("--snapshot and --vocab are required");
                return ExitCode::FAILURE;
            };
            match run_v2_corpus_audit(
                &snap,
                &voc,
                output.as_deref(),
                dump_failures.as_deref(),
                all_cards,
            ) {
                Ok(code) => code,
                Err(err) => {
                    eprintln!("Error: {err}");
                    ExitCode::FAILURE
                }
            }
        }
        "inspect" => {
            let mut vocab: Option<PathBuf> = None;
            let mut name = String::new();
            let mut type_line = String::new();
            let mut oracle_text = String::new();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--vocab" if i + 1 < args.len() => {
                        vocab = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--name" if i + 1 < args.len() => {
                        name = args[i + 1].clone();
                        i += 2;
                    }
                    "--type-line" if i + 1 < args.len() => {
                        type_line = args[i + 1].clone();
                        i += 2;
                    }
                    "--oracle-text" if i + 1 < args.len() => {
                        oracle_text = args[i + 1].clone();
                        i += 2;
                    }
                    other => {
                        eprintln!("Unknown argument: {other}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            let Some(voc) = vocab else {
                eprintln!("--vocab is required");
                return ExitCode::FAILURE;
            };
            let Ok(v) = Vocabulary::load_from_file(&voc) else {
                eprintln!("failed to load vocabulary {}", voc.display());
                return ExitCode::FAILURE;
            };
            let input = CardInput {
                oracle_id: "inspect",
                name: &name,
                layout: "normal",
                faces: vec![FaceInput {
                    name: &name,
                    type_line: &type_line,
                    oracle_text: &oracle_text,
                }],
            };
            let compiled = compile_card_v2(&input, &v);
            println!(
                "{}",
                serde_json::to_string_pretty(&compiled).unwrap_or_default()
            );
            if compiled.strictly_compiled {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        other => {
            eprintln!("Unknown subcommand: {other}");
            ExitCode::FAILURE
        }
    }
}
