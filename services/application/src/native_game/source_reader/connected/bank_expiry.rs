//! Real owned SQL/Storage bank acceptance. The selected thirty-minute bank is unchanged.
use super::*;
mod preflight;
use spellwake_engine::{priority::PriorityPhase, stack_window::WindowPhase};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
struct EarlySql(Mutex<Vec<(u16, serde_json::Value)>>);
impl fault_proxy::Fault for EarlySql {
    fn response(&self, _: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>) {
        if path == "/rest/v1/rpc/commit_native_host_input" {
            self.0
                .lock()
                .unwrap()
                .push((status, serde_json::from_slice(body).unwrap()));
        }
    }
}

#[derive(Default)]
struct ColdSources(Mutex<Vec<(String, u16, String)>>);
impl fault_proxy::Fault for ColdSources {
    fn response(&self, _: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>) {
        if matches!(
            path,
            "/rest/v1/rpc/read_native_game_source"
                | "/rest/v1/rpc/read_native_opening_source"
                | "/rest/v1/rpc/read_native_transition_sources"
        ) {
            self.0.lock().unwrap().push((
                path.to_owned(),
                status,
                String::from_utf8(body.clone()).unwrap(),
            ));
        }
    }
}
fn evidence(name: &str, bytes: &[u8]) {
    let output = std::env::var_os("SPELLWAKE_NATIVE_SOURCE_OUTPUT").unwrap();
    let directory = std::env::var_os("SPELLWAKE_NATIVE_BANK_EVIDENCE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(&output)
                .parent()
                .unwrap()
                .join("private-bank-evidence")
        });
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(name), bytes).unwrap();
}
fn error_chain(error: &NativeSourceError) -> Vec<String> {
    let mut current: &(dyn std::error::Error + 'static) = error;
    let mut result = vec![current.to_string()];
    while let Some(cause) = current.source() {
        result.push(cause.to_string());
        current = cause;
    }
    result
}

pub(super) fn run(r: &Reservation, store: &NativeArtifactStore, programs: &NativeProgramSource) {
    assert_eq!(
        r.structure,
        crate::rooms::configuration::MatchStructure::Bo3
    );
    let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    let repository = NativeSourceRepository::from_env().unwrap();
    let budget = || NativeRecoveryBudget::new(100, 4_000_000).unwrap();
    let recovered = repository
        .recover_with_history(r.game_id, store, programs, budget())
        .unwrap()
        .unwrap();
    let checkpoint = recovered.checkpoint();
    let native = if let Some(decision) = checkpoint.state().pending_decision() {
        decision.chooser()
    } else {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            checkpoint.state().execution().window().phase()
        else {
            panic!("actual native wait required")
        };
        holder
    };
    let loser = r.seat_ids[usize::try_from(native.0).unwrap()];
    let winner = *r.seat_ids.iter().find(|seat| **seat != loser).unwrap();
    assert!(
        recovered
            .source()
            .clocks()
            .all(
                |(seat, remaining, _, paused, _)| remaining == &Amount::from(1_800_000)
                    && paused == (seat != loser)
                    && recovered.source().clock_policy(seat) == Some("bo3-overall-bank-v1")
            )
    );
    let prepared = NativeScope::new(checkpoint.state(), checkpoint.binding(loser).unwrap())
        .unwrap()
        .prepare_bank_expiry(loser)
        .unwrap();
    let input = store
        .retain(
            r.game_id,
            r.context_id,
            NativeArtifactRole::Input,
            prepared.record(),
        )
        .unwrap();
    let next = checkpoint
        .append(store, input, NativeCheckpointInput::BankExpiry, loser)
        .unwrap();
    next.retain(store).unwrap();
    next.prepare_event().unwrap().retain(store).unwrap();
    next.prepare_results().unwrap().retain(store).unwrap();
    let candidate = recovered.prepare_bank_candidate(&next).unwrap();
    let original =
        NativeSystemOriginal::prepare_bank_expiry(Uuid::new_v4(), &recovered, &next).unwrap();
    // Preserve the immutable original before any write, including the early attempt.
    fs::write(
        std::env::var_os("SPELLWAKE_NATIVE_SOURCE_OUTPUT").unwrap(),
        serde_json::to_vec(&json!({"requestId":original.request_id(),"original":serde_json::from_str::<serde_json::Value>(original.original_json()).unwrap()})).unwrap(),
    )
    .unwrap();
    evidence("original.json", &serde_json::to_vec(&json!({"requestId":original.request_id(),"original":serde_json::from_str::<serde_json::Value>(original.original_json()).unwrap()})).unwrap());
    let artifacts = [
        checkpoint.initial_reference().clone(),
        checkpoint.reference().clone(),
        checkpoint.prepare_event().unwrap().reference().clone(),
        checkpoint.prepare_results().unwrap().reference().clone(),
        next.reference().clone(),
        next.retained_input().unwrap().2.clone(),
        next.prepare_event().unwrap().reference().clone(),
        next.prepare_results().unwrap().reference().clone(),
    ];
    let mut retained = Vec::new();
    for (index, reference) in artifacts.iter().enumerate() {
        let bytes = store
            .read(reference, r.game_id, r.context_id, reference.role())
            .unwrap();
        let file = format!("artifact-{index}.json");
        evidence(&file, bytes.bytes());
        retained.push(json!({"reference":reference,"file":file}));
    }
    evidence(
        "artifact-manifest.json",
        &serde_json::to_vec(&retained).unwrap(),
    );
    preflight::run(r, store, programs, &recovered, &next, &original);
    let fingerprint = || {
        lifecycle_reads::sql(
            &database,
            "SELECT jsonb_build_object('source',app.read_native_game_source($1),'receipts',(SELECT count(*) FROM app.command_receipt WHERE target_ref=$1::text),'results',(SELECT count(*) FROM app.game_result WHERE game_id=$1),'handoffs',(SELECT count(*) FROM app.durable_handoff WHERE game_id=$1))",
            &[&r.game_id],
        )
    };
    let due = || {
        lifecycle_reads::sql(
            &database,
            "SELECT to_jsonb(NOT paused AND remaining_ms<=app.clock_elapsed_ms(last_accounted_at,clock_timestamp())) FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=$1 AND c.match_seat_id=$2",
            &[&r.match_id, &loser],
        )
    };
    assert_eq!(
        due(),
        json!(false),
        "early control must actually precede expiry"
    );
    let before = fingerprint();
    let captured = Arc::new(EarlySql::default());
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let proxy = fault_proxy::Proxy::new(&base, None, captured.clone());
    let early_repository = NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(proxy.url(), &key).unwrap(),
            "bank-early-observer",
            Duration::from_secs(60),
        )
        .unwrap(),
    );
    assert!(
        early_repository
            .commit_bank_expiry(
                store,
                &recovered,
                original.request_id(),
                &next,
                &candidate,
                &original
            )
            .is_err()
    );
    let replies = captured.0.lock().unwrap();
    assert_eq!(
        replies.len(),
        1,
        "early proposal must reach exactly one real mutation RPC"
    );
    assert!(replies[0].0 >= 400);
    assert_eq!(replies[0].1["code"], "40001");
    assert_eq!(
        replies[0].1["message"],
        "bank timeout is not due at this acceptance boundary"
    );
    drop(replies);
    drop(early_repository);
    drop(proxy);
    assert_eq!(
        fingerprint(),
        before,
        "early rejection must leave no accepted or rejected prefix"
    );
    assert!(matches!(
        repository.read_system_disposition(&original).unwrap(),
        NativeSystemDisposition::Unknown
    ));
    println!(
        "OWNED_BANK_EARLY_REJECTED: unchanged thirty-minute policy; waiting for actual SQL due boundary"
    );
    let started = Instant::now();
    while due() != json!(true) {
        assert!(
            started.elapsed() < Duration::from_secs(1900),
            "real selected bank did not become due"
        );
        std::thread::sleep(Duration::from_secs(10));
    }
    let ack = repository
        .commit_bank_expiry(
            store,
            &recovered,
            original.request_id(),
            &next,
            &candidate,
            &original,
        )
        .unwrap();
    assert_eq!(ack.revision, Amount::from(next.basis().revision.get()));
    let accepted = fingerprint();
    evidence(
        "accepted-source.json",
        &serde_json::to_vec(&accepted).unwrap(),
    );
    let retry = repository
        .commit_bank_expiry(
            store,
            &recovered,
            original.request_id(),
            &next,
            &candidate,
            &original,
        )
        .unwrap();
    assert_eq!(ack.resource_id, retry.resource_id);
    assert_eq!(ack.revision, retry.revision);
    assert_eq!(fingerprint(), accepted);
    let capture = Arc::new(ColdSources::default());
    let cold_proxy = fault_proxy::Proxy::new(&base, None, capture.clone());
    let cold_repository = NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(cold_proxy.url(), &key).unwrap(),
            "bank-cold-observer",
            Duration::from_secs(60),
        )
        .unwrap(),
    );
    let result = cold_repository.recover_with_history(r.game_id, store, programs, budget());
    evidence(
        "cold-rpc.json",
        &serde_json::to_vec(&*capture.0.lock().unwrap()).unwrap(),
    );
    if let Err(error) = &result {
        evidence(
            "cold-error-chain.json",
            &serde_json::to_vec(&error_chain(error)).unwrap(),
        );
    }
    let cold = result.unwrap().unwrap();
    assert_eq!(cold.history_evidence(), NativeHistoryEvidence::TransitionV1);
    assert_eq!(cold.checkpoint().basis(), next.basis());
    assert_eq!(
        cold.checkpoint()
            .prepare_results()
            .unwrap()
            .original_bytes(),
        next.prepare_results().unwrap().original_bytes()
    );
    assert!(
        repository
            .recover(r.game_id, store, programs, budget())
            .is_err()
    );
    for (seat, remaining, revision, paused, at) in cold.source().clocks() {
        let (_, prior_remaining, prior_revision, _, prior_at) = recovered
            .source()
            .clocks()
            .find(|(old_seat, ..)| *old_seat == seat)
            .unwrap();
        assert_eq!(
            remaining.clone(),
            if seat == loser {
                Amount::from(0)
            } else {
                prior_remaining.clone()
            }
        );
        assert_eq!(
            revision,
            &prior_revision.checked_add(&Amount::from(1)).unwrap()
        );
        assert!(paused);
        assert!(
            crate::source_protocol::parse_utc_microsecond(at).unwrap()
                > crate::source_protocol::parse_utc_microsecond(prior_at).unwrap()
        );
    }
    let outcome = lifecycle_reads::sql(
        &database,
        "SELECT app.consume_game_outcome((SELECT id FROM app.game_result WHERE game_id=$1),(SELECT revision FROM app.match WHERE id=$2))",
        &[&r.game_id, &r.match_id],
    );
    assert_eq!(outcome["status"], "success");
    let terminal = lifecycle_reads::sql(
        &database,
        "SELECT jsonb_build_object('state',m.state,'kind',mr.terminal_kind,'winners',mr.winner_seat_ids,'games',(SELECT count(*) FROM app.game WHERE match_id=m.id),'reason',(SELECT end_reason FROM app.game_result WHERE game_id=$2),'loser',(SELECT bank_forfeit_seat_id FROM app.game_result WHERE game_id=$2)) FROM app.match m JOIN app.match_result mr ON mr.match_id=m.id WHERE m.id=$1",
        &[&r.match_id, &r.game_id],
    );
    evidence(
        "terminal-match.json",
        &serde_json::to_vec(&terminal).unwrap(),
    );
    assert_eq!(
        terminal,
        json!({"state":"completed","kind":"bank_forfeit","winners":[winner],"games":1,"reason":"native_bank_expired","loser":loser})
    );
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":"bank-expiry","actualEarlyAtomicRefusal":true,"actualDueAcceptance":true,"sameOriginalRetry":true,"actualColdBankHistory":true,"stateOnlyRefused":true,"wholeMatchBankForfeit":true,"selectedBankMs":1800000,"fullyQualifiedCards":0,"multiplayerQualified":false})
    );
}
