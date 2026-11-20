//! Process-level ordinary gameplay checks use only compiled public fixture inputs.
//! External Forge/provenance qualification remains a separate local workflow.
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{BufReader, Read},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

fn array<'a>(value: &'a Value, key: &str) -> &'a Vec<Value> {
    value[key].as_array().expect(key)
}
fn named<'a>(run: &'a Value, name: &str) -> &'a Value {
    let rows = array(run, "observations")
        .iter()
        .filter(|row| row["name"] == name)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1, "selector {name}");
    rows[0]
}
fn header(run: &Value, ordinal: usize) -> &Value {
    if ordinal == 0 {
        &run["initialization"]["header"]
    } else {
        &run["accepted"][ordinal - 1]["header"]
    }
}
fn checkpoint<'a>(run: &'a Value, name: &str) -> &'a Value {
    let ordinal = named(run, name)["candidate_ordinal"]
        .as_u64()
        .expect("ordinal");
    &run["checkpoints"][ordinal.to_string()]
}
fn pile<'a>(world: &'a Value, kind: &str, owner: u32) -> Vec<&'a str> {
    let rows = array(world, "ordered_piles")
        .iter()
        .filter(|v| v["pile"]["kind"] == kind && v["pile"]["owner"] == owner)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    array(rows[0], "objects_top_to_bottom")
        .iter()
        .map(|v| v["alias"].as_str().expect("alias"))
        .collect()
}
fn current_card<'a>(world: &'a Value, alias: &str) -> &'a Value {
    let rows = world["cards"]
        .as_object()
        .expect("cards")
        .values()
        .filter(|v| v["physical"]["alias"] == alias)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    rows[0]
}

#[test]
#[ignore = "full current-source capture requires explicit SPELLWAKE_CREATURE_VOCABULARY_INPUT and actual D06 SPELLWAKE_CREATURE_VOCABULARY_PUBLICATION; run separately and retain evidence"]
fn all_ordinary_games_execute_real_actions_checks_and_saved_input_reexecution() {
    let vocabulary = std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
        .expect("explicit selected source-qualified V input required");
    let publication = std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_PUBLICATION")
        .expect("explicit actual native D06 publication required");
    // The complete raw witness remains on disk. This test inspects its existing
    // coverage flag and bounded gameplay observations; full witness validation
    // belongs to the separately retained strict binder.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Envelope {
        schema_version: u64,
        profile: String,
        observations: Value,
        type_context_witness: WitnessCoverage,
    }
    #[derive(Deserialize)]
    struct WitnessCoverage {
        coverage: Coverage,
    }
    #[derive(Deserialize)]
    struct Coverage {
        complete: bool,
    }
    let capture_dir = std::env::var_os("SPELLWAKE_ORDINARY_TEST_CAPTURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "spellwake-ordinary-source-test-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
        });
    fs::create_dir(&capture_dir)
        .expect("create new retained capture directory without overwriting prior evidence");
    let stdout_path = capture_dir.join("ordinary-game.stdout.json");
    let stderr_path = capture_dir.join("ordinary-game.stderr");
    let stdout = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stdout_path)
        .unwrap();
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stderr_path)
        .unwrap();
    let started = SystemTime::now();
    let timer = Instant::now();
    let status = Command::new(env!("CARGO_BIN_EXE_ordinary-game"))
        .arg("--creature-vocabulary-input")
        .arg(&vocabulary)
        .env("SPELLWAKE_CREATURE_VOCABULARY_PUBLICATION", &publication)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .status()
        .expect("run ordinary-game");
    let elapsed = timer.elapsed();
    let finished = SystemTime::now();
    let mut hash = Sha256::new();
    let mut reader = BufReader::new(File::open(&stdout_path).unwrap());
    let mut buffer = [0; 65536];
    loop {
        let size = reader.read(&mut buffer).unwrap();
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    let raw_hash = format!("{:x}", hash.finalize());
    let current: Envelope =
        serde_json::from_reader(BufReader::new(File::open(&stdout_path).unwrap()))
            .expect("complete structured native stdout with only bounded observation allocation");
    let metadata = serde_json::json!({"kind":"actual-opt-in-ordinary-process-test-capture","stdout":stdout_path,"stdout_sha256":raw_hash,"stdout_bytes":fs::metadata(&stdout_path).unwrap().len(),"stderr":stderr_path,"exit_code":status.code(),"started_unix_ns":started.duration_since(UNIX_EPOCH).unwrap().as_nanos().to_string(),"finished_unix_ns":finished.duration_since(UNIX_EPOCH).unwrap().as_nanos().to_string(),"process_wall_elapsed_ns":elapsed.as_nanos().to_string(),"timing_scope":"around actual subprocess status including startup/wait; not an engine benchmark","validated_fields":["schema_version","profile","type_context_witness.coverage.complete","all existing bounded observations assertions in this test"],"unmaterialized_witness_fields":"All witness fields except coverage.complete are structurally consumed by serde; they are not semantically validated here. Complete raw bytes are retained for the independent strict binder.","vocabulary_input":vocabulary,"actual_publication":publication});
    serde_json::to_writer_pretty(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(capture_dir.join("process.json"))
            .unwrap(),
        &metadata,
    )
    .unwrap();
    eprintln!(
        "retained complete ordinary capture {} sha256={raw_hash}",
        stdout_path.display()
    );
    assert_eq!(current.schema_version, 10);
    assert_eq!(current.profile, "ordinary");
    assert!(current.type_context_witness.coverage.complete);
    let packet = current.observations;
    let runs = array(&packet["data"], "runs");
    let failures = runs
        .iter()
        .map(|run| (&run["fixture_ref"], &run["failure"]))
        .collect::<Vec<_>>();
    assert!(status.success(), "native fixture failure: {failures:?}");
    assert_eq!(packet["status"], "success");
    assert_eq!(runs.len(), 4);
    let mut names = 0;
    let mut inputs = 0;
    let mut probes = 0;
    for (run, (case, count)) in
        runs.iter()
            .zip([("G01", 438), ("G02a", 573), ("G02b", 573), ("G03", 1820)])
    {
        assert_eq!(run["fixture_ref"], case);
        assert_eq!(run["completed"], true);
        assert!(run["failure"].is_null());
        let accepted = array(run, "accepted");
        assert_eq!(accepted.len(), count);
        inputs += accepted.len();
        for (index, row) in accepted.iter().enumerate() {
            assert_eq!(row["ordinal"], index + 1);
            assert_eq!(row["input"]["basis"], row["base"]["basis"]);
            let prior = if index == 0 {
                &run["initialization"]["after_basis"]
            } else {
                &accepted[index - 1]["after_basis"]
            };
            assert_eq!(&row["input"]["basis"], prior);
            assert_eq!(row["after_basis"], row["header"]["basis"]);
            assert_eq!(row["current_facts"].as_object().expect("facts").len(), 12);
        }
        let actual_names = array(run, "observations");
        names += actual_names.len();
        assert_eq!(
            actual_names
                .iter()
                .map(|v| v["name"].as_str().expect("name"))
                .collect::<BTreeSet<_>>()
                .len(),
            actual_names.len()
        );
        let rerun = &run["rerun"];
        assert_eq!(rerun["initialization_equal"], true);
        assert_eq!(rerun["final_state_equal"], true);
        assert_eq!(rerun["all_inputs_consumed"], true);
        assert_eq!(array(rerun, "compared_ordinals").len(), count);
        for (index, value) in array(rerun, "compared_ordinals").iter().enumerate() {
            assert_eq!(*value, index + 1);
        }
        assert_eq!(
            array(rerun, "checkpoint_ordinals_compared").len(),
            run["checkpoints"].as_object().expect("checkpoints").len()
        );
        let rejects = array(run, "rejection_probes");
        probes += rejects.len();
        assert_eq!(array(rerun, "probe_ids_compared").len(), rejects.len());
        for probe in rejects {
            assert_eq!(probe["no_candidate"], true);
            assert_eq!(probe["state_retained"], true);
            assert_eq!(probe["expected_error"], probe["actual_error"]);
            if let Some(next) = probe["continuation_ordinal"].as_u64() {
                assert_eq!(
                    next,
                    probe["attached_candidate_ordinal"]
                        .as_u64()
                        .expect("attachment")
                        + 1
                );
                assert!(next as usize <= count);
            }
        }
        let terminal = header(run, count);
        assert_eq!(terminal["outcome"]["kind"], "winner");
        assert_eq!(terminal["outcome"]["seat"], 41);
        assert!(terminal["priority_holder"].is_null());
        assert_eq!(terminal["losses"], serde_json::json!([3]));
        assert_eq!(run["final"]["entropy_remaining"], 0);
        assert_eq!(run["final"]["policy_complete"], true);
        assert_eq!(
            checkpoint(run, "terminal")["cards"]
                .as_object()
                .expect("terminal inventory")
                .len(),
            120
        );
    }
    assert_eq!((inputs, names, probes), (3404, 334, 90));

    let duel = &runs[0];
    let terminal = header(duel, 438);
    assert_eq!(terminal["life"]["41"], "20");
    assert_eq!(terminal["life"]["3"], "0");
    assert_eq!(
        array(duel, "accepted")
            .iter()
            .filter(|v| !v["current_facts"]["land_play"].is_null())
            .count(),
        12
    );
    assert_eq!(
        array(duel, "accepted")
            .iter()
            .filter(|v| !v["current_facts"]["mana_payment"].is_null())
            .count(),
        1
    );
    let cast = named(duel, "cast.A.2")["candidate_ordinal"]
        .as_u64()
        .expect("cast") as usize;
    let row = &duel["accepted"][cast - 1];
    let before = &row["input"]["command"]["spell"]["object"];
    let after = &row["header"]["stack"][0]["kind"]["card"]["object"];
    assert_ne!(before, after, "casting must renew the hand incarnation");
    assert_eq!(row["header"]["mana"]["41"]["white"], "0");

    for (run, a_order, b_order) in [
        (&runs[1], vec!["A.L1", "A.L2"], vec!["B.L2", "B.L1", "B.P6"]),
        (&runs[2], vec!["A.L2", "A.L1"], vec!["B.L1", "B.L2", "B.P6"]),
    ] {
        let a = checkpoint(run, "trade.owner.A");
        let b = checkpoint(run, "trade.owner.B");
        for world in [a, b] {
            for alias in ["A.L1", "A.L2", "B.L1", "B.L2"] {
                let card = current_card(world, alias);
                assert_eq!(card["physical"]["zone"]["kind"], "battlefield");
                assert_eq!(card["creature"]["marked_damage"], "2");
            }
            assert!(pile(world, "graveyard", 41).is_empty());
            assert_eq!(pile(world, "graveyard", 3), vec!["B.P6"]);
        }
        let settled = checkpoint(run, "trade.settled");
        assert_eq!(pile(settled, "graveyard", 41), a_order);
        assert_eq!(pile(settled, "graveyard", 3), b_order);
        let ordinal = named(run, "trade.settled")["candidate_ordinal"]
            .as_u64()
            .expect("settlement") as usize;
        assert_eq!(
            array(
                &run["accepted"][ordinal - 1]["current_facts"]["check_journal"],
                "moves"
            )
            .len(),
            4
        );
        assert_eq!(
            array(run, "accepted")
                .iter()
                .filter(|v| !v["current_facts"]["mana_payment"].is_null())
                .count(),
            5
        );
    }
    let exhausted = &runs[3];
    for (name, holder) in [("draw.last.B", 3), ("draw.last.A", 41)] {
        let ordinal = named(exhausted, name)["candidate_ordinal"]
            .as_u64()
            .expect("draw") as usize;
        assert!(header(exhausted, ordinal)["outcome"].is_null());
        assert_eq!(header(exhausted, ordinal)["priority_holder"], holder);
    }
    let last = &exhausted["accepted"][1819];
    assert_eq!(last["input"]["actor"], 41);
    assert_eq!(
        last["current_facts"]["draw_journal"],
        serde_json::json!([{"kind":"empty_library","player":3,"attempts":"1"}])
    );
    let world = checkpoint(exhausted, "terminal");
    for owner in [41, 3] {
        assert!(pile(world, "library", owner).is_empty());
        assert_eq!(pile(world, "graveyard", owner).len(), 53);
    }
    assert_eq!(
        header(exhausted, 1820)["life"],
        serde_json::json!({"3":"20","41":"20"})
    );
}

#[test]
fn ordinary_cli_rejects_unknown_cases_and_runtime_file_arguments() {
    let temporary = tempfile::tempdir().unwrap();
    let vocabulary = spellwake_reference_tools::creature_types::synthetic_vocabulary_for_rules(
        "synthetic-CLI-argument-unit",
    );
    let input = temporary.path().join("explicit-synthetic-vocabulary.json");
    std::fs::write(
        &input,
        serde_json::to_vec(&spellwake_reference_tools::projection::vocabulary(
            &vocabulary,
        ))
        .unwrap(),
    )
    .unwrap();
    for args in [
        vec!["--case", "unknown"],
        vec!["--expected", "/no/runtime/expected.json"],
        vec!["--case"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_ordinary-game"))
            .args(args)
            .arg("--creature-vocabulary-input")
            .arg(&input)
            .env_remove("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
            .output()
            .expect("invalid ordinary arguments");
        assert!(!output.status.success());
        let packet: Value =
            serde_json::from_slice(&output.stdout).expect("argument failure envelope");
        assert_eq!(packet["status"], "fail");
        assert!(packet["data"]["arguments"].is_string());
    }
}
