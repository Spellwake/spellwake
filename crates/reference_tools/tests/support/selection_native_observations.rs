//! Optional, privileged finite observations from saved actual native values.
//! One maintained projection owner supplies every state/input/result codec. This
//! module neither executes rules nor interprets Debug or expected/reference data.
use super::{Run, source_input};
use serde_json::{Value, json};
use spellwake_reference_tools::{packet::AliasContext, projection};
use std::{io::Write as _, path::PathBuf};

/// Export only when explicitly selected, to a separate pre-created directory.
/// Codec and I/O failures fail the opt-in observation rather than inventing facts.
pub(super) fn write_if_requested(
    run: &Run<'_>,
    name: &str,
    source_case: &str,
) -> Result<(), String> {
    let Some(directory) = std::env::var_os("SPELLWAKE_SELECTION_STRUCTURED_EVIDENCE_OUTPUT") else {
        return Ok(());
    };
    let directory = PathBuf::from(directory);
    if !directory.is_dir() {
        return Err(
            "explicit fresh structured native evidence directory must already exist".into(),
        );
    }
    let legacy = source_input::required_path("SPELLWAKE_SELECTION_CASE_EVIDENCE_OUTPUT")?;
    if directory
        .canonicalize()
        .map_err(|error| error.to_string())?
        == legacy.canonicalize().map_err(|error| error.to_string())?
    {
        return Err("structured and existing native evidence directories must be separate".into());
    }
    let aliases = AliasContext::from_card_aliases(
        &run.fixture
            .cards
            .iter()
            .map(|(alias, card)| (*card, alias.clone()))
            .collect::<Vec<_>>(),
    )?;
    let identities = run
        .fixture
        .seeded_identities
        .iter()
        .map(|seeded| {
            let objects = run.initial.execution().window().cards();
            let object = objects
                .current_identity(seeded.card)
                .and_then(|id| objects.get(id))
                .ok_or_else(|| {
                    format!(
                        "actual seeded physical identity missing for {}",
                        seeded.alias
                    )
                })?;
            Ok(json!({
                "alias":seeded.alias,"card_id":seeded.card.0.to_string(),
                "oracle_id":seeded.oracle,"owner":seeded.owner.0,
                "seed_destination":projection::seed_destination(seeded.destination),
                "initialized_object":projection::object(object),
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let initial = projection::candidate(&run.fixture.initialized, None, &aliases)?;
    let mut source = &run.initial;
    let mut accepted_source = json!({"kind":"Initializer"});
    let mut calls = Vec::with_capacity(run.calls.len());
    for (index, call) in run.calls.iter().enumerate() {
        let source_before = accepted_source.clone();
        let source_before_basis = projection::basis(source.basis());
        let result = match &call.result {
            Ok(candidate) => {
                let result = json!({"tag":"Ok","candidate":projection::candidate(candidate, Some(source), &aliases)?});
                source = candidate.state();
                accepted_source = json!({"kind":"Call","index":index});
                result
            }
            Err(error) => projection::game_error(error),
        };
        calls.push(json!({
            "index":index,"input":projection::input(&call.input),
            "accepted_source_before":source_before,"source_before_basis":source_before_basis,
            "result":result,"adopted":call.result.is_ok(),
            "accepted_source_after":accepted_source,
            "source_after_basis":projection::basis(source.basis()),
        }));
    }
    let participants = run.initial.execution().window().participants();
    let source_player_labels = [
        ("P1", source_input::P1),
        ("P2", source_input::P2),
        ("P3", source_input::P3),
    ]
    .into_iter()
    .filter(|(_, seat)| participants.contains(seat))
    .map(|(label, seat)| json!({"label":label,"seat":seat.0}))
    .collect::<Vec<_>>();
    // Stack-entry aliases bind actual CastCompleted or TriggerPlaced receipts.
    let report: Value = json!({
        "kind":"SOURCE9_STRUCTURED_ACTUAL_NATIVE_OBSERVATIONS_DEVELOPMENT_V1",
        "execution_name":name,"source_case":source_case,
        "projection_owner":"spellwake_reference_tools::projection",
        "identity_declarations":{
            "seeded_cards_in_construction_order":identities,
            "participants_in_native_order":participants.iter().map(|seat|seat.0).collect::<Vec<_>>(),
            "source_player_labels":source_player_labels,
            "token_birth_aliases":run.tokens.iter().map(|(alias,object)|json!({"alias":alias,"object":projection::opaque(object)})).collect::<Vec<_>>(),
            "stack_entry_aliases":run.entries.iter().map(|(alias,entry)|json!({"alias":alias,"entry":projection::opaque(entry)})).collect::<Vec<_>>(),
            "consumed_decision_aliases":run.decisions.iter().map(|(alias,id)|json!({"alias":alias,"decision":id.0.to_string()})).collect::<Vec<_>>(),
        },
        "initializer_candidate":initial,"calls":calls,
        "final_accepted_source":accepted_source,"final_state":projection::state(&run.state,&aliases)?,
        "typed_replay":"PASS_IN_MEMORY_ORIGINAL_INPUT_FULL_RESULT_AND_FINAL_STATE_NO_REPLY_REPAIR",
        "limits":[
            "Privileged finite test observations; not current11 witness, Report5, CLI, host, image, privacy, durable replay or full-card qualification",
            "Only saved actual typed inputs/results are projected; rejection preserves preceding accepted source",
            "Seed aliases/Oracle IDs/owners come from actual Registry::seed arguments; token aliases come from actual birth receipts",
            "Shared projection opaque fields remain opaque; no Debug parsing or replacement codec",
            "No Forge execution or numerical comparison is performed",
        ],
    });
    let bytes = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
    let path = directory.join(format!("{name}.native-observations.json"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.write_all(&bytes).map_err(|error| error.to_string())
}
