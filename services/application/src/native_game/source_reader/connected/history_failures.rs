//! Causal record/byte starvation, unchanged-error retention and late-auth precedence.
use super::delivery::source_fingerprint;
use super::*;
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
fn unavailable() -> Value {
    json!({"status":"error","message":"Application service is temporarily unavailable","code":"unavailable"})
}
pub(super) fn run(r: &Reservation, phase: &str) {
    let operation = match phase {
        "history-error-history" => "history",
        "history-error-replay" => "replay",
        _ => panic!("unknown composed-error case"),
    };
    let repository = NativeSourceRepository::from_env().unwrap();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    let normal = http::HttpProbe::new(&base, &key);
    let before = source_fingerprint(&repository, r.game_id);
    for (axis, records, bytes) in [("records", 1, 100_000_000), ("bytes", 10_000, 1)] {
        let token = super::history_delivery::fresh_owner(&base, &key, &public);
        super::history_delivery::read(&normal, &token, r, operation, 200);
        let armed = Arc::new(AtomicBool::new(false));
        let hits = Arc::new(AtomicUsize::new(0));
        let (enabled, count) = (armed.clone(), hits.clone());
        let (origin, publishable, bearer) = (base.clone(), public.clone(), token.clone());
        let bad = http::HttpProbe::configure(&base, &key, move |runtime| {
            runtime
                .with_test_recovery(NativeRecoveryBudget::new(records, bytes).unwrap())
                .with_read_delivery_hook(
                    super::history_delivery::stage(operation),
                    move |_, recovered, _, _| {
                        assert!(
                            recovered.is_none(),
                            "bounded original recovery failed before a composed view escaped"
                        );
                        if enabled.swap(false, Ordering::SeqCst) {
                            super::history_delivery::logout(&origin, &publishable, &bearer);
                            count.fetch_add(1, Ordering::SeqCst);
                        }
                    },
                )
        });
        assert_eq!(
            super::history_delivery::read(&bad, &token, r, operation, 503),
            unavailable(),
            "unchanged authority preserves {axis} starvation failure"
        );
        armed.store(true, Ordering::SeqCst);
        assert_eq!(
            super::history_delivery::read(&bad, &token, r, operation, 401),
            json!({"status":"fail","data":{"session":"unauthorized"}})
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let restored = super::history_delivery::fresh_owner(&base, &key, &public);
        assert_ne!(restored, token);
        assert_eq!(
            super::history_delivery::read(&bad, &restored, r, operation, 503),
            unavailable(),
            "restored authority exposes the original bounded failure"
        );
        super::history_delivery::read(&normal, &restored, r, operation, 200);
        assert_eq!(source_fingerprint(&repository, r.game_id), before);
    }
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":phase,"actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"actualBudgetFailures":6,"unchangedAuthSafe503":4,"lateAuthOverrides503":2,"restoredFullBudgetSuccesses":2,"budgetRecordFailures":3,"budgetByteFailures":3,"sourceUnchanged":true,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}

/// Cross a genuine original deadline after a real bounded recovery error.
pub(super) fn expiry(r: &Reservation, phase: &str) {
    let operation = match phase {
        "history-error-expiry-history" => "history",
        "history-error-expiry-replay" => "replay",
        _ => panic!("unknown expired-error case"),
    };
    let repository = NativeSourceRepository::from_env().unwrap();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    let normal = http::HttpProbe::new(&base, &key);
    let deadline = Arc::new(Mutex::new(None));
    let hits = Arc::new(AtomicUsize::new(0));
    let (observed, count) = (deadline.clone(), hits.clone());
    let bad = http::HttpProbe::configure(&base, &key, move |runtime| {
        runtime
            .with_test_recovery(NativeRecoveryBudget::new(10_000, 1).unwrap())
            .with_read_delivery_hook(
                super::history_delivery::stage(operation),
                move |_, recovered, _, _| {
                    assert!(recovered.is_none());
                    let at = observed
                        .lock()
                        .unwrap()
                        .expect("genuine deadline installed after setup");
                    assert!(super::history_delivery::now() < at);
                    super::history_delivery::wait_until(at + 1);
                    assert_eq!(count.fetch_add(1, Ordering::SeqCst), 0);
                },
            )
    });
    let warm = super::history_delivery::fresh_owner(&base, &key, &public);
    let positive = super::history_delivery::read(&normal, &warm, r, operation, 200);
    let before = source_fingerprint(&repository, r.game_id);
    let (token, at) = super::history_delivery::fresh_owner_deadline(&base, &key, &public);
    *deadline.lock().unwrap() = Some(at);
    super::history_delivery::wait_until(at - 6);
    assert_eq!(
        super::history_delivery::read(&bad, &token, r, operation, 401),
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(super::history_delivery::now() > at);
    let restored = super::history_delivery::fresh_owner(&base, &key, &public);
    assert_eq!(
        super::history_delivery::read(&normal, &restored, r, operation, 200),
        positive
    );
    assert_eq!(source_fingerprint(&repository, r.game_id), before);
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":phase,"actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"actualBoundedErrorBeforeExpiry":true,"actualExpiredError401":true,"expiryCredentialAfterPreparation":true,"sourceUnchanged":true,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}
