//! Typed fault-injection inventory, not proof that every case is publicly reachable.
use super::*;
use spellwake_engine::{shuffle::EntropyError, turn::TurnWork};

fn exact(error: NativeBridgeError, field: &str, reason: &str) {
    let rejection = RuleRejection::from_bridge(&error).expect("explicit client error");
    assert_eq!(
        rejection.errors(),
        &BTreeMap::from([(field.to_owned(), reason.to_owned())])
    );
    let original = crate::player_wire::PlayerCommand {
        request_id: Uuid::from_u128(3),
        basis: crate::player_wire::PlayerBasis {
            game_id: Uuid::from_u128(1),
            context_id: Uuid::from_u128(2),
            epoch: 0.into(),
            revision: 0.into(),
        },
        action: crate::player_wire::PlayerAction::Pass {},
    };
    crate::player_wire::PlayerReceipt {
        actor_seat_id: Uuid::from_u128(4),
        original: original.clone(),
        disposition: crate::commands::CommandDisposition::Rejected {
            errors: rejection.errors().clone(),
        },
    }
    .validate(&original, Uuid::from_u128(4))
    .unwrap();
}
fn native(error: GameError, field: &str, reason: &str) {
    exact(NativeBridgeError::Native(Box::new(error)), field, reason);
}
fn effects(errors: impl IntoIterator<Item = EffectError>, field: &str, reason: &str) {
    for error in errors {
        native(GameError::Effect(error), field, reason);
    }
}
#[test]
fn every_explicit_client_category_has_an_exact_safe_pair() {
    for (error, field, reason) in [
        (NativeBridgeError::InvalidIntent, "action", "invalid"),
        (NativeBridgeError::InvalidReference, "source", "invalid"),
        (NativeBridgeError::WrongReferenceKind, "source", "invalid"),
        (NativeBridgeError::WrongDecision, "decision", "stale"),
        (NativeBridgeError::Unsupported, "action", "unsupported"),
    ] {
        exact(error, field, reason);
    }
    for (error, field, reason) in [
        (
            GameError::UnsupportedConcessionScope,
            "action",
            "unsupported",
        ),
        (GameError::DecisionRequired, "decision", "not permitted"),
        (GameError::NoDecision, "decision", "unavailable"),
        (GameError::WrongDecision, "decision", "stale"),
        (GameError::WrongChooser, "actor", "not permitted"),
        (
            GameError::Turn(TurnError::LandLimitReached),
            "action",
            "not permitted",
        ),
    ] {
        native(error, field, reason);
    }
    for error in [
        OpeningError::IllegalMulligan,
        OpeningError::InvalidBottom,
        OpeningError::WrongReply,
    ] {
        native(GameError::Opening(error), "response", "invalid");
    }
    effects(
        [
            EffectError::UnknownObject,
            EffectError::NotOwnHand,
            EffectError::NotLand,
            EffectError::NotCastableSpell,
            EffectError::NotAbilitySource,
            EffectError::UnknownActivatedAbility,
            EffectError::NotManaSource,
            EffectError::UnknownManaAbility,
        ],
        "source",
        "invalid",
    );
    effects(
        [
            EffectError::TargetRequired,
            EffectError::UnexpectedTarget,
            EffectError::IllegalTarget,
        ],
        "target",
        "invalid",
    );
    effects(
        [
            EffectError::InsufficientMana,
            EffectError::InvalidPowerContributors,
        ],
        "payment",
        "not permitted",
    );
    effects([EffectError::AlreadyExhausted], "action", "not permitted");
    for error in [
        ManaPaymentError::NonPositiveSpend,
        ManaPaymentError::DuplicateType,
        ManaPaymentError::MissingRequired,
        ManaPaymentError::WrongTotal,
        ManaPaymentError::InsufficientMana,
        ManaPaymentError::PaymentRequired,
    ] {
        effects(
            [EffectError::InvalidManaPayment(error)],
            "payment",
            "invalid",
        );
    }
    effects(
        [
            EffectError::SplitSecond,
            EffectError::LegendarySpellPrerequisite,
            EffectError::CreatureTapRestricted,
        ],
        "action",
        "not permitted",
    );
    for error in [
        TimingError::PriorityRequired,
        TimingError::NotPriorityHolder,
        TimingError::NotActivePlayer,
        TimingError::MainPhaseRequired,
        TimingError::StackNotEmpty,
    ] {
        effects([EffectError::Timing(error)], "action", "not permitted");
    }
    effects(
        [
            EffectError::InvalidSelectionCount,
            EffectError::DuplicateSelection,
            EffectError::SelectionOutsideDomain,
            EffectError::InvalidGraveyardPermutation,
            EffectError::IllegalOptionalEffect,
            EffectError::IllegalTimestampOrder,
            EffectError::IllegalManaColor,
            EffectError::IllegalCreatureType,
        ],
        "response",
        "invalid",
    );
    effects([EffectError::WrongChooser], "actor", "not permitted");
    effects(
        [
            EffectError::WrongSelectionFrame,
            EffectError::WrongLibraryTopFrame,
            EffectError::WrongOptionalEffectFrame,
            EffectError::WrongResolutionPaymentFrame,
            EffectError::WrongTimestampOrderFrame,
            EffectError::WrongManaColorFrame,
        ],
        "decision",
        "stale",
    );
    effects(
        [EffectError::Combat(CombatError::WrongFrame)],
        "decision",
        "stale",
    );
    for error in [
        CombatError::DuplicateAttacker,
        CombatError::IllegalAttacker,
        CombatError::IllegalDefender,
        CombatError::DuplicateBlocker,
        CombatError::IllegalBlocker,
        CombatError::IllegalBlockTarget,
        CombatError::AlreadyDeclared,
        CombatError::DuplicateAssignment,
        CombatError::IllegalDamageSource,
        CombatError::IllegalDamageRecipient,
        CombatError::InvalidDamageAmount,
        CombatError::InsufficientTrampleAssignment,
        CombatError::IncorrectDamageTotal,
    ] {
        effects([EffectError::Combat(error)], "response", "invalid");
    }
    for error in [
        StackWindowError::NotOwnHand,
        StackWindowError::WrongController,
        StackWindowError::SourceTapped,
        StackWindowError::NotInPriority,
    ] {
        effects([EffectError::Window(error)], "action", "not permitted");
    }
}
#[test]
fn source_observer_invariant_capacity_and_pending_work_are_not_player_rejections() {
    for error in [
        NativeBridgeError::InvalidBinding,
        NativeBridgeError::StaleBasis,
        NativeBridgeError::Observation(Box::new(EffectError::UnknownObject)),
        NativeBridgeError::Observation(Box::new(EffectError::InconsistentProgram)),
    ] {
        assert!(RuleRejection::from_bridge(&error).is_none());
    }
    for error in [
        GameError::InvalidContext,
        GameError::WrongGame,
        GameError::WrongContext,
        GameError::WrongEpoch,
        GameError::StaleRevision,
        GameError::UnknownActor,
        GameError::GameEnded,
        GameError::RevisionExhausted,
        GameError::DecisionIdentityExhausted,
        GameError::PlayerEliminationRequired,
        GameError::UnexpectedKernelPhase,
    ] {
        assert!(RuleRejection::from_bridge(&NativeBridgeError::Native(Box::new(error))).is_none());
    }
    for work in [
        TurnWork::Untap,
        TurnWork::Draw,
        TurnWork::DeclareAttackers,
        TurnWork::DeclareBlockers,
        TurnWork::CombatDamage { additional: false },
        TurnWork::Cleanup,
    ] {
        assert!(
            RuleRejection::from_bridge(&NativeBridgeError::Native(Box::new(
                GameError::TurnWorkRequired(work)
            )))
            .is_none()
        );
    }
    for error in [
        EntropyError::Exhausted,
        EntropyError::InvalidBound,
        EntropyError::Capacity,
        EntropyError::EmptySupply,
    ] {
        assert!(
            RuleRejection::from_bridge(&NativeBridgeError::Native(Box::new(GameError::Entropy(
                error
            ))))
            .is_none()
        );
    }
    for error in [
        EffectError::InvalidManaPayment(ManaPaymentError::InvalidCost),
        EffectError::Timing(TimingError::TurnRequired),
        EffectError::Combat(CombatError::UnknownPlayer),
        EffectError::Combat(CombatError::InvalidContext),
        EffectError::InconsistentProgram,
        EffectError::TokenCapacity,
        EffectError::SelectionRequired,
        EffectError::InconsistentSelectionWork,
        EffectError::OptionalEffectChoiceRequired,
        EffectError::ResolutionPaymentChoiceRequired,
        EffectError::ManaColorChoiceRequired,
        EffectError::PlayerLossPending,
        EffectError::Window(StackWindowError::RevisionExhausted),
        EffectError::Window(StackWindowError::EntryIdentityExhausted),
        EffectError::Window(StackWindowError::InconsistentStack),
    ] {
        assert!(
            RuleRejection::from_bridge(&NativeBridgeError::Native(Box::new(GameError::Effect(
                error
            ))))
            .is_none()
        );
    }
    for error in [
        OpeningError::InvalidSetup,
        OpeningError::NotOpening,
        OpeningError::WrongStage,
    ] {
        assert!(
            RuleRejection::from_bridge(&NativeBridgeError::Native(Box::new(GameError::Opening(
                error
            ))))
            .is_none()
        );
    }
    for error in [
        TurnError::InvalidStart,
        TurnError::WorkRequired,
        TurnError::PriorityRequired,
        TurnError::WrongWork,
        TurnError::WrongPriorityOutcome,
        TurnError::TurnExhausted,
        TurnError::CleanupExhausted,
    ] {
        assert!(
            RuleRejection::from_bridge(&NativeBridgeError::Native(Box::new(GameError::Turn(
                error
            ))))
            .is_none()
        );
    }
}
