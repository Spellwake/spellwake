//! Actual SQL scan/cursor checks and causal faults over real accepted native cuts.
use super::*;
use crate::player_wire::PlayerCursor;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

#[derive(Clone, Copy)]
enum Fault {
    MissingReplay,
    NullPosition,
    StateRole,
    EventIdentity,
    Disclosure,
    Limit,
    EmptyBaseline,
    HashShape,
    CursorPurpose,
    CursorForeignReceipt,
    CursorEarlierReceipt(Uuid),
}
struct Mutation {
    fault: Fault,
    hits: AtomicUsize,
}

/// Real SQL-issued cursors with end E=1 and later coordinate A; the anchor
/// may be at E, strictly inside E..A, or at A. Each negative changes only
/// its SQL response receipt and proves the original token still resolves.
pub(super) fn receipt_ranges(repository: &NativeSourceRepository, history: &NativeHumanHistory) {
    let source = history.cut_source();
    let at_index = source.cuts.len().checked_sub(2).unwrap();
    assert!(
        at_index > 3,
        "requires distinct end/interior/coordinate anchors"
    );
    let end = Amount::from(1);
    let before = Amount::from(0);
    let budget = || NativeRecoveryBudget::new(1000, 16000000).unwrap();
    let mut later_cursor = None;
    let mut tokens = std::collections::BTreeSet::new();
    let mut receipts = std::collections::BTreeSet::new();
    for anchor in [1, at_index / 2, at_index] {
        let at = &source.cuts[anchor]
            .projection()
            .snapshot()
            .through_event_sequence;
        let scan = repository
            .read_human_archive(history, at, Some(&before), 1, &mut budget())
            .unwrap();
        assert_eq!(scan.scanned_through(), &end);
        let cursor = repository
            .issue_human_history_cursor(&scan, &mut budget())
            .unwrap();
        assert_eq!(
            cursor.cursor().basis,
            source.cuts[anchor].projection().snapshot().basis
        );
        assert!(tokens.insert(cursor.cursor().token.as_str().to_owned()));
        assert!(receipts.insert(source.cuts[anchor].receipt_id()));
        assert!(
            repository
                .read_cut_history_cursor(&source, at_index, &cursor.cursor(), Some(&mut budget()))
                .unwrap()
                .is_some()
        );
        later_cursor = Some(cursor);
    }
    let cursor = later_cursor.unwrap().cursor();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    for fault in [
        Fault::CursorEarlierReceipt(source.cuts[0].receipt_id()),
        Fault::CursorEarlierReceipt(source.cuts[at_index + 1].receipt_id()),
        Fault::CursorForeignReceipt,
    ] {
        let mutation = Arc::new(Mutation {
            fault,
            hits: AtomicUsize::new(0),
        });
        let proxy = super::fault_proxy::Proxy::new(&base, Some(&key), mutation.clone());
        let faulty = NativeSourceRepository::new(
            BackendClient::new(
                BackendConfig::new(proxy.url(), "sb_secret_test_only").unwrap(),
                "owned-cursor-receipt-range",
                Duration::from_secs(30),
            )
            .unwrap(),
        );
        assert!(
            faulty
                .read_cut_history_cursor(&source, at_index, &cursor, Some(&mut budget()))
                .unwrap()
                .is_none()
        );
        assert_eq!(mutation.hits.load(Ordering::SeqCst), 1);
        assert!(
            repository
                .read_cut_history_cursor(&source, at_index, &cursor, Some(&mut budget()))
                .unwrap()
                .is_some()
        );
    }
    eprintln!("HISTORY_REAL_RECEIPT_RANGE_CONTROLS");
}
impl super::fault_proxy::Fault for Mutation {
    fn response(&self, _: &reqwest::Method, path: &str, status: u16, bytes: &mut Vec<u8>) {
        if matches!(
            self.fault,
            Fault::CursorPurpose | Fault::CursorForeignReceipt | Fault::CursorEarlierReceipt(_)
        ) {
            if path != "/rest/v1/rpc/read_native_cursor_source" {
                return;
            }
            assert_eq!(status, 200);
            let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            assert_eq!(value["data"]["purpose"], "history");
            match self.fault {
                Fault::CursorPurpose => value["data"]["purpose"] = json!("snapshot"),
                Fault::CursorForeignReceipt => {
                    value["data"]["sourceReceiptId"] = json!(Uuid::new_v4())
                }
                Fault::CursorEarlierReceipt(receipt) => {
                    value["data"]["sourceReceiptId"] = json!(receipt)
                }
                _ => unreachable!(),
            }
            *bytes = serde_json::to_vec(&value).unwrap();
        } else {
            if path != "/rest/v1/rpc/read_native_archive_source" {
                return;
            }
            assert_eq!(status, 200);
            let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            assert!(value.get("status").is_none()); // SQL229 returns a raw body.
            match self.fault {
                Fault::MissingReplay => {
                    value.as_object_mut().unwrap().remove("replay");
                }
                Fault::NullPosition => value["scannedThrough"] = serde_json::Value::Null,
                Fault::StateRole => {
                    value["items"][0]["stateRef"] = value["items"][0]["eventRef"].clone()
                }
                Fault::EventIdentity => {
                    assert_ne!(
                        value["items"][0]["eventId"],
                        value["items"][0]["checkpointId"]
                    );
                    value["items"][0]["eventId"] = value["items"][0]["checkpointId"].clone();
                }
                Fault::Disclosure => value["disclosure"] = json!("completed"),
                Fault::Limit => value["limit"] = json!(101),
                Fault::EmptyBaseline => value["items"] = json!([]),
                Fault::HashShape => value["sourceHash"] = json!("invalid"),
                Fault::CursorPurpose
                | Fault::CursorForeignReceipt
                | Fault::CursorEarlierReceipt(_) => unreachable!(),
            }
            *bytes = serde_json::to_vec(&value).unwrap();
        }
        self.hits.fetch_add(1, Ordering::SeqCst);
    }
}
pub(super) fn run(
    repository: &NativeSourceRepository,
    history: &NativeHumanHistory,
    access: &NativeHumanAccess,
    snapshot_cursor: &PlayerCursor,
) {
    let at = &history
        .observations()
        .last()
        .unwrap()
        .projection()
        .snapshot()
        .through_event_sequence;
    let zero = Amount::from(0);
    let mut budget = NativeRecoveryBudget::new(1_000, 16_000_000).unwrap();
    let baseline = repository
        .read_human_archive(history, &zero, None, 100, &mut budget)
        .unwrap();
    assert_eq!(baseline.item_count(), 1);
    assert_eq!(baseline.scanned_through(), &zero);
    assert!(!baseline.has_more());
    let baseline_cursor = repository
        .issue_human_history_cursor(&baseline, &mut budget)
        .unwrap();
    assert_eq!(baseline_cursor.cursor().basis.revision.get(), 0);
    baseline_cursor.validate_human_history(access).unwrap();

    let first = repository
        .read_human_archive(history, at, None, 1, &mut budget)
        .unwrap();
    assert_eq!(first.item_count(), 1);
    assert_eq!(first.scanned_through(), &zero);
    assert_eq!(first.has_more(), !at.is_zero());
    let first_cursor = repository
        .issue_human_history_cursor(&first, &mut budget)
        .unwrap();
    first_cursor.validate_human_history(access).unwrap();
    let all = repository
        .read_human_archive(history, at, None, 100, &mut budget)
        .unwrap();
    assert_eq!(all.item_count(), history.observations().len());
    assert_eq!(all.scanned_through(), at);
    assert!(!all.has_more());
    let all_cursor = repository
        .issue_human_history_cursor(&all, &mut budget)
        .unwrap();
    let tail = repository
        .read_human_archive(history, at, Some(&zero), 100, &mut budget)
        .unwrap();
    assert_eq!(tail.item_count() + 1, all.item_count());
    assert_eq!(tail.after_sequence(), Some(&zero));
    assert_eq!(tail.scanned_through(), at);
    let tail_cursor = repository
        .issue_human_history_cursor(&tail, &mut budget)
        .unwrap();
    // Different immutable scans can intentionally reuse the same SQL position.
    assert!(tail_cursor.cursor() == all_cursor.cursor());
    let resolved = repository
        .read_human_history_cursor(access, &all_cursor.cursor())
        .unwrap()
        .unwrap();
    assert!(resolved.cursor() == all_cursor.cursor());
    assert!(
        repository
            .read_human_history_cursor(access, snapshot_cursor)
            .unwrap()
            .is_none()
    );
    assert!(
        repository
            .read_human_archive(history, at, None, 0, &mut budget)
            .is_err()
    );
    let future = at.checked_add(&Amount::from(1)).unwrap();
    assert!(
        repository
            .read_human_archive(history, &future, None, 1, &mut budget)
            .is_err()
    );
    assert!(
        repository
            .read_human_archive(history, at, Some(&future), 1, &mut budget)
            .is_err()
    );
    assert!(
        repository
            .read_human_archive(
                history,
                at,
                None,
                1,
                &mut NativeRecoveryBudget::new(1, 1).unwrap()
            )
            .is_err()
    );

    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let secret = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    for fault in [
        Fault::MissingReplay,
        Fault::NullPosition,
        Fault::StateRole,
        Fault::EventIdentity,
        Fault::Disclosure,
        Fault::Limit,
        Fault::EmptyBaseline,
        Fault::HashShape,
        Fault::CursorPurpose,
        Fault::CursorForeignReceipt,
        Fault::CursorEarlierReceipt(history.observations()[0].receipt_id()),
    ] {
        let mutation = Arc::new(Mutation {
            fault,
            hits: AtomicUsize::new(0),
        });
        let proxy = super::fault_proxy::Proxy::new(&base, Some(&secret), mutation.clone());
        let faulty = NativeSourceRepository::new(
            BackendClient::new(
                BackendConfig::new(proxy.url(), "sb_secret_test_only").unwrap(),
                "owned-history-scan-fault",
                Duration::from_secs(30),
            )
            .unwrap(),
        );
        if matches!(
            fault,
            Fault::CursorPurpose | Fault::CursorForeignReceipt | Fault::CursorEarlierReceipt(_)
        ) {
            assert!(
                !at.is_zero(),
                "receipt fault requires an advanced real walk"
            );
            assert!(
                faulty
                    .read_verified_history_cursor(history, &all_cursor.cursor())
                    .unwrap()
                    .is_none()
            );
        } else {
            assert!(
                faulty
                    .read_human_archive(
                        history,
                        at,
                        None,
                        100,
                        &mut NativeRecoveryBudget::new(100, 4_000_000).unwrap()
                    )
                    .is_err()
            );
        }
        assert_eq!(mutation.hits.load(Ordering::SeqCst), 1);
        assert!(
            repository
                .read_verified_history_cursor(history, &all_cursor.cursor())
                .unwrap()
                .is_some()
        );
        // Real positive control after every controlled provider fault.
        assert_eq!(
            repository
                .read_human_archive(history, at, None, 100, &mut budget)
                .unwrap()
                .scanned_through(),
            at
        );
    }
}
