//! Diagnose reusable Oracle paragraphs on incomplete cards without admitting them for play.
use super::*;
use std::io::{BufWriter, Write};

fn tsv(field: &str) -> String {
    format!("\"{}\"", field.replace('"', "\"\""))
}

#[test]
#[ignore = "requires the dated catalogue and SPELLWAKE_CLAUSE_AUDIT_OUTPUT"]
fn clause_support_probe() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let file = std::fs::File::create(input("SPELLWAKE_CLAUSE_AUDIT_OUTPUT")).unwrap();
    let mut output = BufWriter::new(file);
    writeln!(output, "oracle_id\tface_name\tclause\tstatus\tcontext").unwrap();
    let mut examined = 0;
    for card in inventory
        .cards
        .iter()
        .filter(|card| card.required_by_base_formats)
    {
        let id = OracleId::try_from(card.oracle_id.clone().unwrap()).unwrap();
        if registry.resolve(&id).is_some() {
            continue;
        }
        examined += 1;
        for face in &card.faces {
            let mut individual = card.clone();
            individual.name = face.name.clone();
            individual.layout = "normal".to_owned();
            individual.faces = vec![face.clone()];
            let text = without_reminder(face.oracle_text.as_deref().unwrap_or(""))
                .unwrap_or_else(|| face.oracle_text.clone().unwrap_or_default());
            let paragraphs: Vec<_> = text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect();
            let context = if card.layout == "normal" {
                "original face"
            } else {
                "isolated normal-layout face; layout unverified"
            };
            if paragraphs.is_empty() {
                writeln!(
                    output,
                    "{}\t{}\t\"\"\tempty\t{}",
                    id.as_str(),
                    tsv(&face.name),
                    context
                )
                .unwrap();
            }
            for clause in paragraphs {
                individual.faces[0].oracle_text = Some(clause.to_owned());
                let status = if compile(&individual, &vocabulary, Profile::Current)
                    .is_ok_and(|program| program.validate(&vocabulary).is_ok())
                {
                    "compiled"
                } else {
                    "unmatched"
                };
                writeln!(
                    output,
                    "{}\t{}\t{}\t{}\t{}",
                    id.as_str(),
                    tsv(&face.name),
                    tsv(clause),
                    status,
                    context
                )
                .unwrap();
            }
        }
    }
    output.flush().unwrap();
    assert_eq!(examined, registry.coverage().required_without_binding);
    eprintln!("Examined all {examined} unbound identities");
}

#[test]
#[ignore = "requires the dated catalogue and SPELLWAKE_PROGRAM_AUDIT_OUTPUT"]
fn compiled_program_inventory() {
    let (_, _, registry) = current_catalogue();
    let file = std::fs::File::create(input("SPELLWAKE_PROGRAM_AUDIT_OUTPUT")).unwrap();
    let mut output = BufWriter::new(file);
    for card in &registry.coverage().cards {
        let id = OracleId::try_from(card.oracle_id.clone().unwrap()).unwrap();
        let Some(binding) = registry.resolve(&id) else {
            continue;
        };
        let program = binding.program();
        let record = serde_json::json!({
            "oracle_id":id.as_str(),"name":card.name,"kind":format!("{:?}",program.kind),
            "keywords":program.printed_keywords.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),
            "mana":program.mana.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),
            "activated":program.activated.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),
            "triggered":program.triggered.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),
            "complete_engine_definition":format!("{program:?}")
        });
        serde_json::to_writer(&mut output, &record).unwrap();
        writeln!(output).unwrap();
    }
    output.flush().unwrap();
}
