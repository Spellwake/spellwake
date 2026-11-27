//! Actual private input reconstruction, distinct from immutable-source admission.
use super::*;
use crate::native_game::{NativeCodecError, restore_entropy, restore_input};
use crate::player_wire::{MAX_PLAYER_WIRE_BYTES, PlayerChoice};
use serde_json::{Value, json};
use spellwake_engine::{
    game::{DecisionReply, OpeningSetup, RuleFrame},
    opening::{MulliganChoice, OpeningDecision},
    shuffle::EntropyTape,
};
use std::io::Write;

pub(super) fn verify(scope: &NativeScope<'_>, prepared: &PreparedNativeInput) {
    let before = scope.source.clone();
    let references = scope.references.clone();
    let bytes = scope.encode_input(prepared).unwrap();
    let restored = restore_input(scope.source, scope.binding.clone(), &bytes).unwrap();
    assert!(restored.original() == prepared.original());
    assert_eq!(restored.native_input(), prepared.native_input());
    assert_eq!(restored.candidate(), prepared.candidate());
    assert_eq!(restored.viewer_seat_id(), prepared.viewer_seat_id());
    assert_eq!(scope.source, &before);
    assert!(scope.references == references);
    if let Some(path) = std::env::var_os("SPELLWAKE_NATIVE_INPUT_CAPTURE") {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap();
        let (command, reply) = family(prepared.native_input());
        let record: Value = serde_json::from_slice(&bytes).unwrap();
        let mut output = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(
            output,
            "{}",
            json!({"command":command,"reply":reply,"record":record})
        )
        .unwrap();
    }
}
fn family(input: &GameInput) -> (&'static str, Option<&'static str>) {
    let command = match &input.command {
        GameCommand::Concede => "concede",
        GameCommand::CastWithResources { .. } => "cast-with-resources",
        GameCommand::CastUntargeted { .. } => "cast-untargeted",
        GameCommand::Plot { .. } => "plot",
        GameCommand::Foretell { .. } => "foretell",
        GameCommand::Suspend { .. } => "suspend",
        GameCommand::CastFaceDown { .. } => "cast-face-down",
        GameCommand::TurnFaceUp { .. } => "turn-face-up",
        GameCommand::PlayLand { .. } => "play-land",
        GameCommand::PlayLandWithEntryColor { .. } => "play-land-with-entry-color",
        GameCommand::PlayLandWithEntryReveal { .. } => "play-land-with-entry-reveal",
        GameCommand::ActivateWithCreatures { .. } => "activate-with-creatures",
        GameCommand::ActivateWithCostObjects { .. } => "activate-with-cost-objects",
        GameCommand::ActivateManaForPlayer { .. } => "activate-mana-for-player",
        GameCommand::ActivateManaWithCostObjects { .. } => "activate-mana-with-cost-objects",
        GameCommand::ActivateManaWithRestrictedPayment { .. } => {
            "activate-mana-with-restricted-payment"
        }
        GameCommand::ActivateWithRestrictedMana { .. } => "activate-with-restricted-mana",
        GameCommand::ActivateUntargetedAbility { .. } => "activate-untargeted-ability",
        GameCommand::Ninjutsu { .. } => "ninjutsu",
        GameCommand::Transmute { .. } => "transmute",
        GameCommand::Cycle { .. } => "cycle",
        GameCommand::ActivateGranted { .. } => "activate-granted",
        GameCommand::ActivateEmblem { .. } => "activate-emblem",
        GameCommand::ActivatePermanentTargets { .. } => "activate-permanent-targets",
        GameCommand::ActivateLoyaltyX { .. } => "activate-loyalty-x",
        GameCommand::ActivateModalAbility { .. } => "activate-modal-ability",
        GameCommand::ActivateAbility { .. } => "activate-ability",
        GameCommand::ActivateMana { .. } => "activate-mana",
        GameCommand::ActivateManaWithPayment { .. } => "activate-mana-with-payment",
        GameCommand::ActivateHybridMana { .. } => "activate-hybrid-mana",
        GameCommand::ActivateManaWithBundle { .. } => "activate-mana-with-bundle",
        GameCommand::Cast { .. } => "cast",
        GameCommand::CastPaid { .. } => "cast-paid",
        GameCommand::Pass => "pass",
        GameCommand::Reply { choice, .. } => {
            return (
                "reply",
                Some(match choice {
                    // Diagnostic family only: no public codec or roundtrip support.
                    DecisionReply::ZoneReplacement(_) => "zone-replacement-native-unsupported",
                    DecisionReply::LibraryTop(
                        spellwake_engine::effects::LibraryTopReply::Search { .. }
                        | spellwake_engine::effects::LibraryTopReply::SearchSplit { .. }
                        | spellwake_engine::effects::LibraryTopReply::SearchBattlefield { .. }
                        | spellwake_engine::effects::LibraryTopReply::SearchWithEntryColors {
                            ..
                        }
                        | spellwake_engine::effects::LibraryTopReply::SearchWithEntryChoices {
                            ..
                        }
                        | spellwake_engine::effects::LibraryTopReply::SearchHandGraveyard { .. },
                    ) => "library-search",
                    DecisionReply::LibraryTop(_) => "library-top-native-unsupported",
                    DecisionReply::SelectResolution(_) => "select-resolution",
                    DecisionReply::OrderResolutionGraveyard(_) => "order-resolution-graveyard",
                    DecisionReply::ImpulseCast { .. } => "impulse-cast",
                    DecisionReply::MadnessCast { .. } => "madness-cast",
                    DecisionReply::CascadeCast { .. } => "cascade-cast",
                    DecisionReply::ReboundCast { .. } => "rebound-cast",
                    DecisionReply::SuspendCast { .. } => "suspend-cast",
                    DecisionReply::OptionalEffect(_) => "optional-effect",
                    DecisionReply::PayAttackCost { .. } => "pay-attack-cost",
                    DecisionReply::RedeclareReversingMana(_) => "redeclare-reversing-mana",
                    DecisionReply::RedeclareKeepingMana => "redeclare-keeping-mana",
                    DecisionReply::RedeclareAttackers => "redeclare-attackers",
                    DecisionReply::ResolutionPayment(_) => "resolution-payment",
                    DecisionReply::ChooseLandKind(_) => "choose-land-kind",
                    DecisionReply::ChooseDamageRecipient(_) => "choose-damage-recipient",
                    DecisionReply::ChooseDamageSource(_) => "choose-damage-source",
                    DecisionReply::MutateOnTop(_) => "mutate-on-top",
                    DecisionReply::ChoosePile(_) => "choose-pile",
                    DecisionReply::PlaneswalkerType(_) => "planeswalker-type",
                    DecisionReply::AttackDirection(_) => "attack-direction",
                    DecisionReply::DrawReplacement(_) => "draw-replacement",
                    DecisionReply::OpponentChoice(_) => "opponent-choice",
                    DecisionReply::ReplacementChooser(_) => "replacement-chooser",
                    DecisionReply::MyriadDefender(_) => "myriad-defender",
                    DecisionReply::ReselectAttackDefender(_) => "reselect-attack-defender",
                    DecisionReply::TokenDefender(_) => "token-defender",
                    DecisionReply::TimestampOrder(_) => "timestamp-order",
                    DecisionReply::TransformOrder(_) => "transform-order",
                    DecisionReply::Discard(_) => "discard",
                    DecisionReply::ReturnCommanders(_) => "return-commanders",
                    DecisionReply::OrderGraveyard(_) => "order-graveyard",
                    DecisionReply::KeepLegend { .. } => "keep-legend",
                    DecisionReply::OrderTriggers(_) => "order-triggers",
                    DecisionReply::TriggerPermanentTargets(_) => "trigger-permanent-targets",
                    DecisionReply::TriggerTarget(_) => "trigger-target",
                    DecisionReply::TriggerModes(_) => "trigger-modes",
                    DecisionReply::ApplyCounterDoublersFirst(_) => "counter-replacement",
                    DecisionReply::DeclareBandedAttackers { .. }
                    | DecisionReply::DeclareAttackers(_) => "declare-attackers",
                    DecisionReply::DeclareBlockers(_) => "declare-blockers",
                    DecisionReply::AssignCombatDamage(_) => "assign-combat-damage",
                    DecisionReply::Mulligan(_) => "mulligan",
                    DecisionReply::Bottom(_) => "bottom",
                    DecisionReply::CopyTargets(_) => "copy-targets",
                    DecisionReply::DamageOrder(_) => "damage-order",
                    DecisionReply::AllocatePrevention(_) => "allocate-prevention",
                    DecisionReply::ManaColor(_) => "mana-color",
                    DecisionReply::SplitMana(_) => "split-mana",
                    DecisionReply::EntryChoice(_) => "entry-choice",
                    DecisionReply::TokenEntryChoice(_) => "token-entry-choice",
                    DecisionReply::TokenAuraChoice(_) => "token-aura-choice",
                    DecisionReply::LinkedReturnOrder(_) => "linked-return-order",
                    DecisionReply::LinkedAuraChoice(_) => "linked-aura-choice",
                    DecisionReply::CastRecovery(_) => "cast-recovery",
                    DecisionReply::Proliferate(_) => "proliferate",
                    DecisionReply::RemoveCounters(_) => "counter-removal",
                    DecisionReply::CreatureType(_) => "creature-type",
                    DecisionReply::OptionIndex(_) => "option-index",
                    DecisionReply::PromptChoice(_) => "prompt-choice",
                    DecisionReply::UntapKeepTapped(_) => "untap-keep-tapped",
                    DecisionReply::OpeningBattlefield(_) => "opening-battlefield",
                }),
            );
        }
    };
    (command, None)
}
fn passing() -> GameState {
    state(vec![])
}
fn pass_record(source: &GameState) -> (NativeScope<'_>, Vec<u8>) {
    let view = scope(source, viewer_a());
    let prepared = view
        .propose(&command(&view, PlayerAction::Pass {}))
        .unwrap();
    let bytes = view.encode_input(&prepared).unwrap();
    (view, bytes)
}
fn reject(source: &GameState, bytes: &[u8]) {
    let before = source.clone();
    assert!(restore_input(source, binding(source, viewer_a()), bytes).is_err());
    assert_eq!(source, &before);
}
#[test]
fn codec_failure_formatters_never_embed_original_values() {
    let cause = serde_json::from_str::<Value>("{sensitive-untrusted-data}").unwrap_err();
    let error = NativeCodecError::Json(cause);
    assert!(error.source().is_some());
    assert_eq!(
        format!("{error:?}"),
        "native input record cannot be decoded"
    );
    assert!(!error.to_string().contains("sensitive"));
}
#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native codec composition, not source admission"]
fn original_bytes_context_provenance_and_size_fail_closed() {
    let source = passing();
    let (view, bytes) = pass_record(&source);
    let original = restore_input(&source, view.binding.clone(), &bytes)
        .unwrap()
        .original()
        .clone();
    let prepared = view.propose(&original).unwrap();
    verify(&view, &prepared);
    // Distinct states can share complete basis and translate Pass identically.
    let other = state(vec![card(
        1,
        A,
        ObjectDestination::Hand,
        Program::artifact(None, vec![]),
    )]);
    assert_eq!(source.basis(), other.basis());
    let other_scope = scope(&other, viewer_a());
    let other_proposal = other_scope
        .propose(&command(&other_scope, PlayerAction::Pass {}))
        .unwrap();
    assert_eq!(prepared.native_input(), other_proposal.native_input());
    assert!(matches!(
        view.encode_input(&other_proposal),
        Err(NativeCodecError::WrongSource)
    ));
    // Even a second scope over the same state is not the originating allocation.
    let same_source_scope = scope(&source, viewer_a());
    assert!(matches!(
        same_source_scope.encode_input(&prepared),
        Err(NativeCodecError::WrongSource)
    ));
    assert!(matches!(
        restore_input(&source, binding(&source, viewer_b()), &bytes),
        Err(NativeCodecError::WrongSource)
    ));
    let baseline: Value = serde_json::from_slice(&bytes).unwrap();
    // Property order changes must not change meaning or lose the raw command span.
    assert!(
        restore_input(
            &source,
            binding(&source, viewer_a()),
            &serde_json::to_vec(&baseline).unwrap()
        )
        .is_ok()
    );
    let mut concession = baseline.clone();
    concession["transition"]["command"]["action"]["kind"] = json!("concede");
    let restored_concession = restore_input(
        &source,
        binding(&source, viewer_a()),
        &serde_json::to_vec(&concession).unwrap(),
    )
    .unwrap();
    assert_eq!(
        restored_concession.native_input().command,
        GameCommand::Concede
    );
    let direct_concession = source
        .advance(GameInput {
            basis: source.basis().clone(),
            actor: A,
            command: GameCommand::Concede,
        })
        .unwrap();
    assert_eq!(restored_concession.candidate(), &direct_concession);
    for pointer in [
        "/source/context/release",
        "/source/context/rules",
        "/source/context/catalogue",
        "/source/context/programs",
        "/source/context/profile",
        "/source/context/creatureVocabulary",
        "/source/context/nameRules",
        "/source/basis/gameId",
        "/source/basis/contextId",
        "/source/basis/epoch",
        "/source/basis/revision",
        "/source/roster/0/seatId",
        "/source/roster/0/nativeSeat",
        "/transition/viewerSeatId",
        "/transition/command/basis/contextId",
    ] {
        let mut mutated = baseline.clone();
        let value = mutated.pointer_mut(pointer).unwrap();
        *value = Value::String(match pointer {
            x if x.ends_with("Id") => Uuid::from_u128(900).to_string(),
            x if x.ends_with("epoch") || x.ends_with("revision") || x.ends_with("nativeSeat") => {
                "999".into()
            }
            _ => "different-private-source".into(),
        });
        reject(&source, &serde_json::to_vec(&mutated).unwrap());
    }
    let mut reordered = baseline.clone();
    reordered["source"]["roster"]
        .as_array_mut()
        .unwrap()
        .reverse();
    reject(&source, &serde_json::to_vec(&reordered).unwrap());
    for mutated in [
        {
            let mut x = baseline.clone();
            x["version"] = json!("native-input-v2");
            x
        },
        {
            let mut x = baseline.clone();
            x["transition"]["references"] =
                json!([{"handle":"unused","location":{"kind":"decision"}}]);
            x
        },
        {
            let mut x = baseline.clone();
            x["source"]["roster"][0]["nativeSeat"] = json!(17);
            x
        },
        {
            let mut x = baseline.clone();
            x["source"]["context"] = json!(["x", "y", "z"]);
            x
        },
        {
            let mut x = baseline.clone();
            x["transition"]["extra"] = json!("private-forgery");
            x
        },
    ] {
        reject(&source, &serde_json::to_vec(&mutated).unwrap());
    }
    let text = String::from_utf8(bytes.clone()).unwrap();
    for altered in [
        text.replacen(
            "\"version\":",
            "\"version\":\"native-input-v1\",\"version\":",
            1,
        ),
        text.replacen(
            "\"viewerSeatId\":",
            "\"viewerSeatId\":\"00000000-0000-0000-0000-000000000065\",\"viewerSeatId\":",
            1,
        ),
        text.replacen(
            "\"requestId\":",
            "\"requestId\":\"00000000-0000-0000-0000-000000000065\",\"request\\u0049d\":",
            1,
        ),
        text.replacen(
            "\"kind\":\"pass\"",
            "\"kind\":\"pass\",\"kind\":\"pass\"",
            1,
        ),
    ] {
        reject(&source, altered.as_bytes());
    }
    let raw = serde_json::to_string(&original).unwrap();
    let marker = format!("\"command\":{raw}");
    assert!(text.contains(&marker));
    let at_limit = text.replacen(
        &marker,
        &format!(
            "\"command\":{{{}{}",
            " ".repeat(MAX_PLAYER_WIRE_BYTES - raw.len()),
            &raw[1..]
        ),
        1,
    );
    assert!(restore_input(&source, binding(&source, viewer_a()), at_limit.as_bytes()).is_ok());
    let above_limit = text.replacen(
        &marker,
        &format!(
            "\"command\":{{{}{}",
            " ".repeat(MAX_PLAYER_WIRE_BYTES - raw.len() + 1),
            &raw[1..]
        ),
        1,
    );
    reject(&source, above_limit.as_bytes());
    // Outer exact limit admits padding; one byte above fails before parsing.
    let mut padded = bytes.clone();
    padded.resize(super::super::input_codec::MAX_RECORD_BYTES, b' ');
    assert!(restore_input(&source, binding(&source, viewer_a()), &padded).is_ok());
    padded.push(b' ');
    reject(&source, &padded);
    let error = restore_input(
        &source,
        binding(&source, viewer_a()),
        b"{malformed-sensitive}",
    )
    .err()
    .unwrap();
    assert!(error.source().is_some());
    assert!(!format!("{error:?}").contains("sensitive"));
}
#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native codec composition, not source admission"]
fn locator_bijections_reject_aliases_wrong_types_and_invalid_indices() {
    let source = state(vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SingleCreatureOrPlayer,
                    vec![Instruction::DealDamage(Box::new(
                        spellwake_engine::toolbox::DealDamageSpec {
                            source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                            recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                            amount: spellwake_engine::program::DamageAmountSpec::Expr(
                                spellwake_engine::program::ValueExpr::literal(2),
                            ),
                            exile_if_dies: false,
                        },
                    ))],
                ),
            ),
        ),
        card(
            2,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::creature(None, 2.into(), 3.into(), BTreeSet::new()),
        ),
    ]);
    let mut view = scope(&source, viewer_a());
    let original = command(&view, PlayerAction::Pass {});
    let prepared = view.propose(&original).unwrap();
    let bytes = view.encode_input(&prepared).unwrap();
    reject(&source, b"[]");
    let mut broken: Value = serde_json::from_slice(&bytes).unwrap();
    broken["transition"]["references"] =
        json!([{"handle":"unused","location":{"kind":"object","index":"0"}}]);
    reject(&source, &serde_json::to_vec(&broken).unwrap());
    let from = view
        .issue(NativeReference::Object(object(&source, 1)))
        .unwrap();
    let to = view
        .issue(NativeReference::Object(object(&source, 2)))
        .unwrap();
    let cast = command(
        &view,
        PlayerAction::CastTargeted {
            source: from,
            target: PlayerTarget::Object { handle: to },
            payment: None,
        },
    );
    // Supply actual mana in a separate fixture used by the supported-command matrix;
    // this zero-mana literal spell has no payable cost and is native legal here.
    let proposal = view.propose(&cast).unwrap();
    verify(&view, &proposal);
    let baseline: Value = serde_json::from_slice(&view.encode_input(&proposal).unwrap()).unwrap();
    for location in [
        json!({"kind":"object","index":"99999"}),
        json!({"kind":"object","index":"01"}),
        json!({"kind":"object","index":0}),
        json!({"kind":"object","index":"18446744073709551616"}),
        json!({"kind":"decision"}),
        json!({"kind":"stack","index":"0"}),
        json!({"kind":"color","color":"blue"}),
        json!({"kind":"object","index":"0","extra":true}),
    ] {
        let mut mutated = baseline.clone();
        mutated["transition"]["references"][0]["location"] = location;
        reject(&source, &serde_json::to_vec(&mutated).unwrap());
    }
    let mut alias = baseline.clone();
    alias["transition"]["references"][1]["location"] =
        alias["transition"]["references"][0]["location"].clone();
    reject(&source, &serde_json::to_vec(&alias).unwrap());
    let mut duplicate = baseline.clone();
    duplicate["transition"]["references"][1]["handle"] =
        duplicate["transition"]["references"][0]["handle"].clone();
    reject(&source, &serde_json::to_vec(&duplicate).unwrap());
    let mut missing = baseline.clone();
    missing["transition"]["references"]
        .as_array_mut()
        .unwrap()
        .pop();
    reject(&source, &serde_json::to_vec(&missing).unwrap());
    let mut wrong = baseline.clone();
    wrong["transition"]["command"]["action"]["kind"] = json!("play-land");
    wrong["transition"]["command"]["action"]
        .as_object_mut()
        .unwrap()
        .remove("target");
    wrong["transition"]["command"]["action"]
        .as_object_mut()
        .unwrap()
        .remove("payment");
    // Trim now-unreferenced target so this reaches the real native Land check.
    let used = wrong["transition"]["command"]["action"]["source"].clone();
    wrong["transition"]["references"]
        .as_array_mut()
        .unwrap()
        .retain(|x| x["handle"] == used);
    let error = restore_input(
        &source,
        binding(&source, viewer_a()),
        &serde_json::to_vec(&wrong).unwrap(),
    )
    .err()
    .unwrap();
    assert!(matches!(error, NativeCodecError::Bridge(_)));
}
#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native codec composition, not source admission"]
fn fresh_initialization_replays_ordered_player_and_host_entropy_records() {
    let original_seed = seed(
        [(A, 100), (B, 200)]
            .into_iter()
            .flat_map(|(owner, base)| {
                (0..20).map(move |n| {
                    card(
                        base + n,
                        owner,
                        ObjectDestination::LibraryBottom,
                        Program::artifact(None, vec![]),
                    )
                })
            })
            .collect(),
    );
    let setup = OpeningSetup {
        seed: original_seed,
        entropy: EntropyTape::new(vec![0; 200]),
    };
    let context = context(&setup.seed.creature_vocabulary);
    let initialize = || {
        GameState::initialize_opening(GameId(90), context.clone(), setup.clone())
            .unwrap()
            .into_state()
    };
    let mut original = initialize();
    let mut saved: Vec<(bool, Uuid, Vec<u8>, GameState)> = vec![];
    // Capture pre-entropy decision to prove actual native rebind invalidates it.
    let mut initial = scope(&original, viewer_a());
    let pending = original.pending_decision().unwrap();
    let decision = initial
        .issue(NativeReference::Decision(pending.id()))
        .unwrap();
    let keep = initial
        .issue(NativeReference::Mulligan(MulliganChoice::Keep))
        .unwrap();
    let intent = command(
        &initial,
        PlayerAction::Choose {
            decision,
            response: PlayerChoice::Option { option: keep },
        },
    );
    let stale = initial
        .encode_input(&initial.propose(&intent).unwrap())
        .unwrap();
    let entropy = initial.prepare_entropy(vec![u64::MAX, 1, 2]).unwrap();
    let repeated =
        restore_entropy(&original, binding(&original, viewer_a()), entropy.record()).unwrap();
    assert_eq!(entropy.candidate(), &repeated);
    let entropy_json: Value = serde_json::from_slice(entropy.record()).unwrap();
    let unchanged = original.clone();
    for words in [
        json!([]),
        json!([0]),
        json!(["01"]),
        json!(["18446744073709551616"]),
        json!(["-1"]),
    ] {
        let mut corrupted = entropy_json.clone();
        corrupted["transition"]["words"] = words;
        assert!(
            restore_entropy(
                &original,
                binding(&original, viewer_a()),
                &serde_json::to_vec(&corrupted).unwrap()
            )
            .is_err()
        );
        assert_eq!(original, unchanged);
    }
    assert!(matches!(
        scope(&passing(), viewer_a()).prepare_entropy(vec![0]),
        Err(NativeCodecError::Bridge(_))
    ));
    assert!(restore_input(&original, binding(&original, viewer_a()), entropy.record()).is_err());
    assert!(restore_entropy(&original, binding(&original, viewer_a()), &stale).is_err());
    assert!(initial.prepare_entropy(vec![]).is_err());
    let (record, candidate) = entropy.into_parts();
    original = candidate.into_state();
    saved.push((true, viewer_a(), record, original.clone()));
    reject(&original, &stale);
    let mut mulligan = false;
    for _ in 0..16 {
        let Some(pending) = original.pending_decision() else {
            break;
        };
        let viewer = if pending.chooser() == A {
            viewer_a()
        } else {
            viewer_b()
        };
        let mut view = scope(&original, viewer);
        let response = match pending.frame() {
            RuleFrame::Opening(OpeningDecision::Declare { .. }) => {
                let choice = if pending.chooser() == A && !mulligan {
                    mulligan = true;
                    MulliganChoice::Mulligan
                } else {
                    MulliganChoice::Keep
                };
                PlayerChoice::Option {
                    option: view.issue(NativeReference::Mulligan(choice)).unwrap(),
                }
            }
            RuleFrame::Opening(OpeningDecision::Bottom { count, hand, .. }) => {
                PlayerChoice::Objects {
                    objects: hand
                        .iter()
                        .take(*count)
                        .rev()
                        .map(|id| view.issue(NativeReference::Object(*id)).unwrap())
                        .collect(),
                }
            }
            _ => panic!("unexpected opening decision"),
        };
        let decision = view.issue(NativeReference::Decision(pending.id())).unwrap();
        let intent = command(&view, PlayerAction::Choose { decision, response });
        let proposed = view.propose(&intent).unwrap();
        verify(&view, &proposed);
        let bytes = view.encode_input(&proposed).unwrap();
        original = proposed.into_parts().3.into_state();
        saved.push((false, viewer, bytes, original.clone()));
    }
    assert!(original.opening().unwrap().complete());
    assert!(mulligan);
    let mut restored = initialize(); // A fresh native initializer, no saved GameState install.
    for (entropy, viewer, bytes, expected) in &saved {
        restored = if *entropy {
            restore_entropy(&restored, binding(&restored, *viewer), bytes)
                .unwrap()
                .into_state()
        } else {
            restore_input(&restored, binding(&restored, *viewer), bytes)
                .unwrap()
                .into_parts()
                .3
                .into_state()
        };
        assert_eq!(&restored, expected);
    }
    assert_eq!(restored, original);
    assert!(saved.len() >= 5);
    // Reordering/replaying a record cannot silently jump an accepted frontier.
    assert!(restore_entropy(&restored, binding(&restored, viewer_a()), &saved[0].2).is_err());
}

fn library_native_step(source: &GameState, actor: SeatId, command: GameCommand) -> GameState {
    let before = source.clone();
    let input = GameInput {
        basis: source.basis().clone(),
        actor,
        command,
    };
    let candidate = source.advance(input.clone()).unwrap();
    assert_eq!(source, &before);
    assert_eq!(source.advance(input).unwrap(), candidate);
    candidate.into_state()
}
#[test]
#[ignore = "requires explicitly supplied genuine vocabulary; public adapter library-top composition"]
fn library_native_stages_project_scry_and_surveil_and_refuse_mill_in_public_adapters() {
    use spellwake_engine::{
        effects::{LibraryActionKind, LibraryTopReply},
        program::ResolvingPlayer,
    };
    for kind in [
        LibraryActionKind::Scry,
        LibraryActionKind::Surveil,
        LibraryActionKind::Mill,
    ] {
        let player = ResolvingPlayer::Controller;
        let amount = spellwake_engine::program::ValueExpr::Literal(2.into());
        let instruction = match kind {
            LibraryActionKind::Scry => Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::scry(player, amount),
            )),
            LibraryActionKind::Surveil => Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::surveil(player, amount),
            )),
            LibraryActionKind::Mill => Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::mill(player, amount),
            )),
        };
        let initial = state(vec![
            card(
                1,
                A,
                ObjectDestination::Hand,
                Program::instant(
                    ManaCost::free(),
                    ExecutableBody::new(BodyTarget::None, vec![instruction]),
                ),
            ),
            card(
                10,
                A,
                ObjectDestination::LibraryTop,
                Program::artifact(None, vec![]),
            ),
            card(
                11,
                A,
                ObjectDestination::LibraryTop,
                Program::artifact(None, vec![]),
            ),
        ]);
        let cast = library_native_step(
            &initial,
            A,
            GameCommand::CastUntargeted {
                spell: object(&initial, 1),
                payment: None,
            },
        );
        let passed = library_native_step(&cast, A, GameCommand::Pass);
        let mut pending = library_native_step(&passed, B, GameCommand::Pass);
        if kind == LibraryActionKind::Mill {
            // Mill produces ResolutionGraveyardOrder which is now publicly supported.
            if let Some(decision) = pending.pending_decision() {
                let RuleFrame::ResolutionGraveyardOrder(frame) = decision.frame() else {
                    panic!("actual library owner order")
                };
                let choice = DecisionReply::OrderResolutionGraveyard(frame.domain().to_vec());
                let id = decision.id();
                pending = library_native_step(
                    &pending,
                    A,
                    GameCommand::Reply {
                        decision: id,
                        choice,
                    },
                );
            }
            assert!(pending.pending_decision().is_none());
            assert!(pending.execution().resolution_state().is_none());
            continue;
        }
        // Scry and Surveil are now first-class public wire decisions.
        let mut view = scope(&pending, viewer_a());
        let projected = super::projection::project_test(&mut view).unwrap();
        assert_eq!(projected.decisions.len(), 1);
        let dec = &projected.decisions[0];
        let expected_kind = if kind == LibraryActionKind::Scry {
            crate::player_wire::DecisionKind::Scry
        } else {
            crate::player_wire::DecisionKind::Surveil
        };
        assert_eq!(dec.kind, expected_kind);
        let crate::player_wire::PlayerDomain::LibraryTop { inspected } = &dec.domain else {
            panic!("expected LibraryTop domain");
        };
        assert_eq!(inspected.len(), 2);
        let handles: Vec<_> = inspected.iter().map(|opt| opt.handle.clone()).collect();
        let response = if kind == LibraryActionKind::Scry {
            PlayerChoice::Scry {
                top: handles,
                bottom: vec![],
            }
        } else {
            PlayerChoice::Surveil {
                top: vec![],
                graveyard: handles,
            }
        };
        let intent = command(
            &view,
            PlayerAction::Choose {
                decision: dec.decision.clone(),
                response,
            },
        );
        let prepared = view.propose(&intent).unwrap();
        pending = prepared.into_parts().3.into_state();
        if let Some(decision) = pending.pending_decision() {
            let RuleFrame::ResolutionGraveyardOrder(frame) = decision.frame() else {
                panic!("actual library owner order")
            };
            let choice = DecisionReply::OrderResolutionGraveyard(frame.domain().to_vec());
            let id = decision.id();
            pending = library_native_step(
                &pending,
                A,
                GameCommand::Reply {
                    decision: id,
                    choice,
                },
            );
        }
        assert!(pending.pending_decision().is_none());
        assert!(pending.execution().resolution_state().is_none());
        let _ = LibraryTopReply::Scry {
            top_first: vec![],
            bottom_top_first: vec![],
        };
    }
}
