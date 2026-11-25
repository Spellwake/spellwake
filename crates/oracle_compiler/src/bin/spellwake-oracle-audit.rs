//! CLI auditing tool for Spellwake's Stage 1 Oracle Parser.
//!
//! Subcommands:
//! - `audit-corpus --snapshot <oracle-cards.jsonl[.gz]> --vocab <vocab.json> [--output <json>] [--max-critical <N>] [--max-structural <N>] [--max-opaque <N>]`
//! - `inspect --vocab <vocab.json> --name <name> --type-line <tl> --oracle-text <text>`

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use serde::Deserialize;
use spellwake_oracle_compiler::audit::{CorpusAuditSummary, audit_card};
use spellwake_oracle_compiler::{
    CardInput, CompileOptions, FaceInput, Vocabulary, compile_card_with_options,
};

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
    #[serde(default)]
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

fn process_line(
    line: &str,
    line_num: usize,
    vocab: &Vocabulary,
    all_cards: bool,
    summary: &mut CorpusAuditSummary,
    critical_reports: &mut Option<Vec<spellwake_oracle_compiler::audit::CardAuditReport>>,
) -> Result<(), String> {
    if line.trim().is_empty() {
        return Ok(());
    }
    let raw = serde_json::from_str::<RawCard>(line)
        .map_err(|e| format!("malformed JSONL card at line {line_num}: {e}"))?;
    if !all_cards && !is_ordinary_base_format(&raw.legalities) {
        return Ok(());
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
    let opts = CompileOptions::default();
    let ast = compile_card_with_options(&input, vocab, opts);
    let report = audit_card(&input, &ast, opts);
    summary.record(&report);
    if let Some(list) = critical_reports
        && !report.critical_defects.is_empty()
    {
        list.push(report);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_corpus_audit(
    snapshot: &Path,
    vocab_path: &Path,
    output: Option<&Path>,
    dump_critical: Option<&Path>,
    all_cards: bool,
    max_critical: Option<usize>,
    max_structural: Option<usize>,
    max_opaque: Option<usize>,
) -> Result<ExitCode, String> {
    let vocab = Vocabulary::load_from_file(vocab_path).map_err(|e| {
        format!(
            "failed to load creature vocabulary {}: {e}",
            vocab_path.display()
        )
    })?;
    let mut summary = CorpusAuditSummary::default();
    let mut critical_reports = dump_critical.map(|_| Vec::new());

    let is_gz = snapshot
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("gz"));

    if is_gz {
        let mut child = Command::new("gzip")
            .args(["-dc"])
            .arg(snapshot)
            .stdout(Stdio::piped())
            .spawn()
            .or_else(|_| {
                Command::new("python3")
                    .args([
                        "-c",
                        "import gzip,sys,shutil; shutil.copyfileobj(gzip.open(sys.argv[1],'rb'), sys.stdout.buffer)",
                    ])
                    .arg(snapshot)
                    .stdout(Stdio::piped())
                    .spawn()
            })
            .map_err(|e| format!("failed to decompress {}: {e}", snapshot.display()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "missing stdout pipe from gzip decompressor".to_string())?;
        for (idx, line) in BufReader::new(stdout).lines().enumerate() {
            let line = line.map_err(|e| format!("read error: {e}"))?;
            process_line(
                &line,
                idx + 1,
                &vocab,
                all_cards,
                &mut summary,
                &mut critical_reports,
            )?;
        }
        let status = child
            .wait()
            .map_err(|e| format!("decompressor wait failed: {e}"))?;
        if !status.success() {
            return Err(format!("decompressor exited with status {status}"));
        }
    } else {
        let file = File::open(snapshot)
            .map_err(|e| format!("failed to open {}: {e}", snapshot.display()))?;
        for (idx, line) in BufReader::new(file).lines().enumerate() {
            let line = line.map_err(|e| format!("read error: {e}"))?;
            process_line(
                &line,
                idx + 1,
                &vocab,
                all_cards,
                &mut summary,
                &mut critical_reports,
            )?;
        }
    }

    let json = serde_json::to_string_pretty(&summary)
        .map_err(|e| format!("failed to serialize audit summary: {e}"))?;
    if let Some(out_path) = output {
        if let Some(parent) = out_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        std::fs::write(out_path, format!("{json}\n"))
            .map_err(|e| format!("failed to write {}: {e}", out_path.display()))?;
    }
    if let (Some(crit_path), Some(reports)) = (dump_critical, critical_reports) {
        if let Some(parent) = crit_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        let crit_json = serde_json::to_string_pretty(&reports)
            .map_err(|e| format!("failed to serialize critical reports: {e}"))?;
        std::fs::write(crit_path, format!("{crit_json}\n"))
            .map_err(|e| format!("failed to write {}: {e}", crit_path.display()))?;
    }

    let total = summary.total_cards.max(1) as f64;
    eprintln!(
        "=== SPELLWAKE ORACLE PARSER AUDIT ({} cards) ===",
        summary.total_cards
    );
    eprintln!(
        "  Strict Parse:         {:>6} / {:<6} ({:.2}%)",
        summary.strictly_compiled_cards,
        summary.total_cards,
        summary.strictly_compiled_cards as f64 / total * 100.0
    );
    eprintln!(
        "  Zero Critical Defect:   {:>6} / {:<6} ({:.2}%)",
        summary.zero_critical_defect_cards,
        summary.total_cards,
        summary.zero_critical_defect_cards as f64 / total * 100.0
    );
    eprintln!(
        "  Zero Structural Defect: {:>6} / {:<6} ({:.2}%)",
        summary.zero_structural_defect_cards,
        summary.total_cards,
        summary.zero_structural_defect_cards as f64 / total * 100.0
    );
    eprintln!(
        "  Pure Typed AST:         {:>6} / {:<6} ({:.2}%)",
        summary.fully_typed_pure_cards,
        summary.total_cards,
        summary.fully_typed_pure_cards as f64 / total * 100.0
    );
    if !summary.critical_defect_counts.is_empty() {
        eprintln!("  Critical defects by code:");
        for (code, count) in &summary.critical_defect_counts {
            eprintln!("    {code}: {count}");
        }
    }
    if !summary.opaque_bucket_counts.is_empty() {
        eprintln!("  Opaque string buckets:");
        for (code, count) in &summary.opaque_bucket_counts {
            eprintln!("    {code}: {count}");
        }
    }
    if output.is_none() {
        println!("{json}");
    }

    if summary.total_cards == 0 {
        eprintln!("FAIL: audited 0 cards (empty or mismatched snapshot input)");
        return Ok(ExitCode::FAILURE);
    }
    if let Some(limit) = max_critical
        && summary.cards_with_critical_defects > limit
    {
        eprintln!(
            "FAIL: {} cards have critical parser defects (limit: {limit})",
            summary.cards_with_critical_defects
        );
        return Ok(ExitCode::FAILURE);
    }
    if let Some(limit) = max_structural
        && summary.cards_with_structural_defects > limit
    {
        eprintln!(
            "FAIL: {} cards have structural parser corruption defects (limit: {limit})",
            summary.cards_with_structural_defects
        );
        return Ok(ExitCode::FAILURE);
    }
    if let Some(limit) = max_opaque
        && summary.cards_with_opaque_strings > limit
    {
        eprintln!(
            "FAIL: {} cards have opaque string fallbacks (limit: {limit})",
            summary.cards_with_opaque_strings
        );
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(subcmd) = args.first().map(String::as_str) else {
        eprintln!("Usage: spellwake-oracle-audit <audit-corpus|inspect> [options]");
        return ExitCode::FAILURE;
    };

    match subcmd {
        "audit-corpus" => {
            let mut snapshot: Option<PathBuf> = None;
            let mut vocab: Option<PathBuf> = None;
            let mut output: Option<PathBuf> = None;
            let mut dump_critical: Option<PathBuf> = None;
            let mut all_cards = false;
            let mut max_critical: Option<usize> = None;
            let mut max_structural: Option<usize> = None;
            let mut max_opaque: Option<usize> = None;

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
                    "--dump-critical" if i + 1 < args.len() => {
                        dump_critical = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--all-cards" => {
                        all_cards = true;
                        i += 1;
                    }
                    "--max-critical" if i + 1 < args.len() => {
                        match args[i + 1].parse::<usize>() {
                            Ok(v) => max_critical = Some(v),
                            Err(_) => {
                                eprintln!("Invalid --max-critical value: {:?}", args[i + 1]);
                                return ExitCode::FAILURE;
                            }
                        }
                        i += 2;
                    }
                    "--max-structural" if i + 1 < args.len() => {
                        match args[i + 1].parse::<usize>() {
                            Ok(v) => max_structural = Some(v),
                            Err(_) => {
                                eprintln!("Invalid --max-structural value: {:?}", args[i + 1]);
                                return ExitCode::FAILURE;
                            }
                        }
                        i += 2;
                    }
                    "--max-opaque" if i + 1 < args.len() => {
                        match args[i + 1].parse::<usize>() {
                            Ok(v) => max_opaque = Some(v),
                            Err(_) => {
                                eprintln!("Invalid --max-opaque value: {:?}", args[i + 1]);
                                return ExitCode::FAILURE;
                            }
                        }
                        i += 2;
                    }
                    other => {
                        eprintln!("Unknown option: {other}");
                        return ExitCode::FAILURE;
                    }
                }
            }

            let (Some(snap), Some(voc)) = (snapshot, vocab) else {
                eprintln!("audit-corpus requires --snapshot <path> and --vocab <path>");
                return ExitCode::FAILURE;
            };
            match run_corpus_audit(
                &snap,
                &voc,
                output.as_deref(),
                dump_critical.as_deref(),
                all_cards,
                max_critical,
                max_structural,
                max_opaque,
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
                        eprintln!("Unknown option: {other}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            let Some(voc) = vocab else {
                eprintln!(
                    "inspect requires --vocab <path> --name <n> --type-line <tl> --oracle-text <ot>"
                );
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
            let opts = CompileOptions::default();
            let ast = compile_card_with_options(&input, &v, opts);
            let report = audit_card(&input, &ast, opts);
            let payload = serde_json::json!({ "ast": ast, "audit": report });
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_default()
            );
            if report.has_zero_critical_defects() {
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
