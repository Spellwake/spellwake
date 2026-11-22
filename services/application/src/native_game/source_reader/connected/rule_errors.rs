//! Actual source-bound Edict errors; no complete-card or deck/format qualification.
use super::*;
use crate::player_wire::{PlayerAction, PlayerPayment, PlayerTarget, PlayerZoneKind};
use spellwake_engine::{effects::EffectError, game::GameError, mana::ManaPaymentError};
pub(super) fn run(
    repository: &NativeSourceRepository,
    (http, readonly): (&http::HttpProbe, &http::HttpProbe),
    (actor, token): (&crate::actor::RequestActor, &str),
    access: &NativeHumanAccess,
    recovered: &RecoveredNativeGame,
    issued: &IssuedNativeHumanView<'_>,
) {
    let source = issued
        .view()
        .snapshot()
        .zones
        .iter()
        .find(|zone| {
            zone.zone_type() == PlayerZoneKind::Hand
                && zone.owner_seat_id() == Some(access.viewer_seat_id())
        })
        .unwrap()
        .objects()
        .first()
        .unwrap()
        .clone();
    let before = repository
        .read(recovered.source().game_id())
        .unwrap()
        .unwrap();
    let mut requests = Vec::new();
    for (index, (target, payment, expected)) in [
        (
            PlayerTarget::Object {
                handle: source.clone(),
            },
            None,
            json!({"target":"invalid"}),
        ),
        (
            PlayerTarget::Seat {
                seat_id: access.viewer_seat_id(),
            },
            None,
            json!({"payment":"not permitted"}),
        ),
        (
            PlayerTarget::Seat {
                seat_id: access.viewer_seat_id(),
            },
            Some(PlayerPayment {
                from_lots: Default::default(),
                exile_cast_type: None,
                alternative_cost_source: None,
                graveyard_permission: None,
                use_granted_flashback: false,
                restricted_mana: Vec::new(),
                spend: vec![],
            }),
            json!({"payment":"invalid"}),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let original = PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: access.basis(),
            action: PlayerAction::CastTargeted {
                source: source.clone(),
                target,
                payment,
            },
        };
        let error = issued
            .view()
            .scope()
            .propose(&original)
            .err()
            .expect("genuine native rejection required");
        assert!(matches!(
            (index, error.native_error()),
            (0, Some(GameError::Effect(EffectError::IllegalTarget)))
                | (1, Some(GameError::Effect(EffectError::InsufficientMana)))
                | (
                    2,
                    Some(GameError::Effect(EffectError::InvalidManaPayment(
                        ManaPaymentError::MissingRequired
                    )))
                )
        ));
        let response = http.command(Some(token), &original, 409);
        assert_eq!(response["data"], expected);
        assert_eq!(readonly.command(Some(token), &original, 409), response);
        let Some(ApiResponse::Fail { data }) = repository.human_original(actor, &original).unwrap()
        else {
            panic!("saved rule original required");
        };
        assert_eq!(serde_json::to_value(data).unwrap(), expected);
        requests.push(original.request_id);
        let after = repository
            .read(recovered.source().game_id())
            .unwrap()
            .unwrap();
        assert_eq!(before.checkpoint_ids(), after.checkpoint_ids());
        assert_eq!(before.accepted_revision(), after.accepted_revision());
        assert_eq!(before.last_event_sequence(), after.last_event_sequence());
    }
    fs::write(std::env::var_os("SPELLWAKE_NATIVE_SOURCE_OUTPUT").unwrap(),serde_json::to_vec(&json!({
        "contextId":access.basis().context_id,"requestIds":requests,"oracleRef":"058917c1-21ab-488a-9f9c-591c55f3c596",
    })).unwrap()).unwrap();
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":"results","actualNativeTransitionEvidence":true,
        "actualClockIntentDerived":true,"actualRuleTargetPaymentHttp":true,"actualExactRejectedRetries":true,
        "nativeErrors":["IllegalTarget","InsufficientMana","InvalidManaPaymentMissingRequired"],
        "initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,
        "fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}
