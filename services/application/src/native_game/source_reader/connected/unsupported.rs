//! A genuinely reached unsupported work boundary, without forcing its acceptance.
use super::*;
use spellwake_engine::{
    effects::EffectError, game::GameError, object_identity::ObjectIdentityError,
    shuffle::EntropyError, stack_window::StackWindowError,
};
/// A typed native entropy refusal, not a guessed unsupported work label.
pub(super) fn error_kind(error: &NativeBridgeError) -> Option<&'static str> {
    match error.native_error() {
        Some(GameError::Effect(EffectError::Window(StackWindowError::Objects(
            ObjectIdentityError::Entropy(EntropyError::Exhausted),
        )))) => Some("entropy-refill"),
        _ => None,
    }
}
pub(super) fn refuse(
    repository: &NativeSourceRepository,
    http: &http::HttpProbe,
    actor: &crate::actor::RequestActor,
    token: &str,
    original: &PlayerCommand,
    kind: &str,
) {
    println!(
        "OWNED_NATIVE_UNSUPPORTED_BOUNDARY {}",
        json!({"kind":kind,"basis":original.basis})
    );
    let before = repository.read(original.basis.game_id).unwrap().unwrap();
    http.command(Some(token), original, 503);
    assert!(
        repository
            .human_original(actor, original)
            .unwrap()
            .is_none()
    );
    let after = repository.read(original.basis.game_id).unwrap().unwrap();
    assert_eq!(before.checkpoint_ids(), after.checkpoint_ids());
    assert_eq!(before.accepted_revision(), after.accepted_revision());
    assert_eq!(before.last_event_sequence(), after.last_event_sequence());
    let clocks = |source: &NativeGameSource| {
        source
            .clocks()
            .map(|(id, remaining, revision, paused, at)| {
                (
                    id,
                    remaining.clone(),
                    revision.clone(),
                    paused,
                    at.to_owned(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(clocks(&before), clocks(&after));
    fs::write(std::env::var_os("SPELLWAKE_NATIVE_SOURCE_OUTPUT").unwrap(),serde_json::to_vec(&json!({
        "contextId":original.basis.context_id,"requestId":original.request_id,"basis":original.basis,"unsupportedKind":kind,
    })).unwrap()).unwrap();
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":"results","actualNativeTransitionEvidence":true,
        "actualClockIntentDerived":true,"actualUnsupportedWorkRefused":true,"unsupportedKind":kind,
        "originalUnknown":true,"acceptedSourceUnchanged":true,"initialVersion2":true,"selectedPrintingProvenance":true,
        "selectedSourceActualSql":true,"immutableSourceDisplay":true,"fullyQualifiedCards":0,
        "semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}
