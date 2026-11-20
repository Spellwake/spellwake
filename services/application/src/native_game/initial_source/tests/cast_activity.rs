//! Genuine catalogue programs, controlled initial deck, actual input execution/cold replay.
//! This finite physical-cast mechanism is not arbitrary deck/card/format admission.
use super::results::{install, replay};
use super::*;
use crate::native_game::cast_activity::NativeCastAccounts;
use crate::player_wire::{ManaColor, ManaSpend, PlayerPayment};
use spellwake_engine::{
    land::BasicLandType, mana::ManaAbilityRef, object_identity::ObjectZone,
    priority::PriorityPhase, stack_window::WindowPhase, turn::Step as TurnStep,
};
const BEARS: &str = "14c8f55d-d177-4c25-a931-ebeb9e6062a0";
fn is_spell(object: &spellwake_engine::object_identity::GameObject) -> bool {
    ((object.card_id().unwrap().0 - 1) % 36).is_multiple_of(3)
}
fn verify_land_play(f: &CheckpointFixture, viewer: Uuid) {
    use crate::admission_ports::NativeEventSchema::{ActivityV1, LandPlayV1};
    let human = Uuid::from_u128(0xc001);
    let other = seats().into_iter().find(|seat| *seat != viewer).unwrap();
    let accounts = NativeCastAccounts::new(
        ids().game_id,
        ids().context_id,
        [(viewer, Some(human)), (other, None)],
    )
    .unwrap();
    let ai = NativeCastAccounts::new(
        ids().game_id,
        ids().context_id,
        [(viewer, None), (other, Some(human))],
    )
    .unwrap();
    let recovered = replay(f);
    let expected = json!([{"eventIndex":"0","accountId":human,"kind":"native:land-play","revision":"native-land-play-v1","value":"1"}]);
    for tip in [&f.tip, &recovered] {
        assert_eq!(tip.activity_schema(), LandPlayV1);
        assert_eq!(tip.land_player().unwrap(), viewer);
        assert_eq!(
            serde_json::to_value(accounts.facts_for_schema(tip, LandPlayV1).unwrap()).unwrap(),
            expected
        );
        assert!(ai.facts_for_schema(tip, LandPlayV1).unwrap().is_empty());
        assert!(
            accounts
                .facts_for_schema(tip, ActivityV1)
                .unwrap()
                .is_empty()
        );
        let legacy: Value = serde_json::from_slice(
            tip.prepare_results_schema(ActivityV1)
                .unwrap()
                .original_bytes(),
        )
        .unwrap();
        assert_eq!(legacy["version"], "native-results-activity-v1");
        assert!(legacy.get("landPlayer").is_none());
        let result: Value =
            serde_json::from_slice(tip.prepare_activity_results().unwrap().original_bytes())
                .unwrap();
        assert_eq!(result["landPlayer"], json!(viewer));
        assert_eq!(result["version"], "native-results-land-play-v1");
    }
    assert_eq!(
        f.tip.prepare_activity_results().unwrap().original_bytes(),
        recovered
            .prepare_activity_results()
            .unwrap()
            .original_bytes()
    );
}
pub(in crate::native_game) fn cast_fixture(
    spell: &str,
    land: &str,
    basic: BasicLandType,
    color: ManaColor,
    targeted: bool,
) -> CheckpointFixture {
    let programs = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let mut setup = spec();
    for participant in &mut setup.participants {
        participant.life = 20.into();
        participant.cards = (0..36)
            .map(|n| NativeInitialCard {
                oracle_id: OracleId::try_from(if n % 3 == 0 { spell } else { land }.to_owned())
                    .unwrap(),
                zone: NativeInitialZone::Library,
            })
            .collect();
    }
    let mut f =
        checkpoint_fixture_with_opening(prepare_initial(ids(), programs, setup).unwrap(), None);
    let mut rejected_without_mana = 0;
    let mut cast = false;
    for _ in 0..500 {
        assert!(f.tip.state().pending_decision().is_none());
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            f.tip.state().execution().window().phase()
        else {
            panic!("actual priority required")
        };
        let viewer = seats()[usize::try_from(holder.0).unwrap()];
        let window = f.tip.state().execution().window();
        let turn = f.tip.state().turn().unwrap();
        let main = holder == window.turn_owner() && turn.position().step == TurnStep::PrecombatMain;
        let mut scope = NativeScope::new(f.tip.state(), f.tip.binding(viewer).unwrap()).unwrap();
        let mut action = PlayerAction::Pass {};
        if main {
            let hand = window
                .cards()
                .iter()
                .filter(|o| o.zone() == ObjectZone::Hand(holder))
                .collect::<Vec<_>>();
            if turn.lands_played() == &0.into() {
                if let Some(land) = hand.iter().find(|o| !is_spell(o)) {
                    action = PlayerAction::PlayLand {
                        source: scope.issue(NativeReference::Object(land.id())).unwrap(),
                    };
                }
            } else if let Some(bear) = hand.iter().find(|o| is_spell(o)) {
                let lands = window
                    .cards()
                    .iter()
                    .filter(|o| {
                        o.zone() == ObjectZone::Battlefield
                            && o.controller() == Some(holder)
                            && !is_spell(o)
                    })
                    .collect::<Vec<_>>();
                if lands.len() >= 2 {
                    let handle = scope.issue(NativeReference::Object(bear.id())).unwrap();
                    let payment = Some(PlayerPayment {
                        spend: vec![ManaSpend {
                            color,
                            amount: crate::player_wire::RulesCount::try_from("2".to_owned())
                                .unwrap(),
                        }],
                    });
                    let cast_action = if targeted {
                        PlayerAction::CastTargeted {
                            source: handle,
                            target: crate::player_wire::PlayerTarget::Seat {
                                seat_id: seats()[0],
                            },
                            payment,
                        }
                    } else {
                        PlayerAction::CastUntargeted {
                            source: handle,
                            payment,
                        }
                    };
                    let command = PlayerCommand {
                        request_id: Uuid::new_v4(),
                        basis: scope.basis().clone(),
                        action: cast_action,
                    };
                    let prior = f.tip.original_bytes().to_vec();
                    match scope.propose(&command) {
                        Ok(proposal) => {
                            let bytes = scope.encode_input(&proposal).unwrap();
                            install(&mut f, viewer, bytes);
                            cast = true;
                            break;
                        }
                        Err(_) => {
                            rejected_without_mana += 1;
                            assert_eq!(f.tip.original_bytes(), prior);
                        }
                    }
                    for land in lands {
                        let trial = PlayerAction::ActivateMana {
                            source: scope.issue(NativeReference::Object(land.id())).unwrap(),
                            ability: scope
                                .issue(NativeReference::ManaAbility {
                                    source: land.id(),
                                    ability: ManaAbilityRef::BasicLand(basic),
                                })
                                .unwrap(),
                        };
                        let command = PlayerCommand {
                            request_id: Uuid::new_v4(),
                            basis: scope.basis().clone(),
                            action: trial.clone(),
                        };
                        if scope.propose(&command).is_ok() {
                            action = trial;
                            break;
                        }
                    }
                }
            }
        }
        let command = PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: scope.basis().clone(),
            action,
        };
        let proposal = scope.propose(&command).unwrap();
        let bytes = scope.encode_input(&proposal).unwrap();
        install(&mut f, viewer, bytes);
        if matches!(command.action, PlayerAction::PlayLand { .. }) {
            verify_land_play(&f, viewer);
        }
        assert!(
            serde_json::from_slice::<Value>(
                f.tip.prepare_activity_results().unwrap().original_bytes()
            )
            .unwrap()["activity"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    assert!(
        cast,
        "controlled genuine deck must complete the selected cast"
    );
    assert!(rejected_without_mana >= 2);
    f
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; physical cast activity and replay mechanism only"]
fn completed_green_cast_counts_once_in_both_families_and_never_credits_ai_sponsor() {
    let mut f = cast_fixture(
        BEARS,
        FOREST,
        BasicLandType::Forest,
        ManaColor::Green,
        false,
    );
    let legacy = f.tip.prepare_results().unwrap();
    let activity = f.tip.prepare_activity_results().unwrap();
    let event = f.tip.prepare_activity_event().unwrap();
    let value: Value = serde_json::from_slice(activity.original_bytes()).unwrap();
    assert_eq!(value["version"], "native-results-activity-v1");
    assert_eq!(
        value["result"],
        serde_json::from_slice::<Value>(legacy.original_bytes()).unwrap()
    );
    assert_eq!(
        value["activity"],
        json!([{"seatId":seats()[1],"redOrGreen":"1","blackOrGreen":"1"}])
    );
    let human = Uuid::from_u128(0xc001);
    let sponsor = Uuid::from_u128(0xc002);
    let humans = NativeCastAccounts::new(
        ids().game_id,
        ids().context_id,
        [(seats()[0], Some(sponsor)), (seats()[1], Some(human))],
    )
    .unwrap();
    let facts = humans.facts(&f.tip).unwrap();
    assert_eq!(facts.len(), 2);
    assert!(
        facts.iter().all(|f| f.account_id == human
            && f.value == 1u64.into()
            && f.event_index == 0u64.into())
    );

    // Exercise the actual native frame matcher, not a permissive JSON equality.
    // Clock/source acceptance is independently checked by lifecycle/SQL tests.
    let body = json!({"eventSchemaRevision":"native-transition-activity-v1","contextId":ids().context_id,"stateRef":f.tip.reference(),"factsRef":activity.reference(),"events":[event.reference()],"facts":facts,"clocks":[{"seatId":seats()[0],"clockRevision":"0","remainingMs":"0","paused":true},{"seatId":seats()[1],"clockRevision":"0","remainingMs":"0","paused":true}],"participation":[]});
    let parse = |value: &Value| {
        crate::admission_ports::HostCandidate::from_bytes(&serde_json::to_vec(value).unwrap())
            .unwrap()
    };
    assert!(parse(&body).matches_native_activity_results(None, &[], &facts));
    assert!(!parse(&body).matches_native_results(None, &[]));
    for (field, value) in [
        ("accountId", json!(sponsor)),
        ("kind", json!("native:cast-other")),
        ("revision", json!("native-cast-colors-v2")),
        ("value", json!("2")),
        ("value", json!("0")),
    ] {
        let mut bad = body.clone();
        bad["facts"][0][field] = value;
        assert!(!parse(&bad).matches_native_activity_results(None, &[], &facts));
    }
    let mut missing = body.clone();
    missing["facts"].as_array_mut().unwrap().pop();
    assert!(!parse(&missing).matches_native_activity_results(None, &[], &facts));
    let mut extra = body.clone();
    extra["facts"]
        .as_array_mut()
        .unwrap()
        .push(body["facts"][0].clone());
    assert!(!parse(&extra).matches_native_activity_results(None, &[], &facts));
    let mut duplicate = body.clone();
    duplicate["facts"][1] = body["facts"][0].clone();
    assert!(!parse(&duplicate).matches_native_activity_results(None, &[], &facts));
    let mut index = body.clone();
    index["events"]
        .as_array_mut()
        .unwrap()
        .push(json!(event.reference()));
    index["facts"][0]["eventIndex"] = json!("1");
    assert!(!parse(&index).matches_native_activity_results(None, &[], &facts));
    let ai = NativeCastAccounts::new(
        ids().game_id,
        ids().context_id,
        [(seats()[0], Some(sponsor)), (seats()[1], None)],
    )
    .unwrap();
    assert!(ai.facts(&f.tip).unwrap().is_empty());
    let recovered = replay(&f);
    assert_eq!(
        recovered
            .prepare_activity_results()
            .unwrap()
            .original_bytes(),
        activity.original_bytes()
    );
    assert_eq!(
        recovered.prepare_activity_event().unwrap().original_bytes(),
        event.original_bytes()
    );
    assert_eq!(
        serde_json::to_value(humans.facts(&recovered).unwrap()).unwrap(),
        serde_json::to_value(facts).unwrap()
    );
    for _ in 0..2 {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            f.tip.state().execution().window().phase()
        else {
            panic!("actual priority")
        };
        let viewer = seats()[usize::try_from(holder.0).unwrap()];
        let scope = NativeScope::new(f.tip.state(), f.tip.binding(viewer).unwrap()).unwrap();
        let command = PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: scope.basis().clone(),
            action: PlayerAction::Pass {},
        };
        let proposal = scope.propose(&command).unwrap();
        let bytes = scope.encode_input(&proposal).unwrap();
        install(&mut f, viewer, bytes);
        assert!(humans.facts(&f.tip).unwrap().is_empty());
    }
    assert!(
        f.tip
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .any(|o| o.zone() == ObjectZone::Battlefield && is_spell(o))
    );
    assert!(humans.facts(&replay(&f)).unwrap().is_empty());
}

#[test]
#[ignore = "requires genuine catalogue/vocabulary; physical black and white casts, not full cards or formats"]
fn completed_black_and_nonmatching_white_casts_use_actual_cast_time_colors() {
    let cases = [
        (
            "058917c1-21ab-488a-9f9c-591c55f3c596",
            "56719f6a-1a6c-4c0a-8d21-18f7d7350b68",
            BasicLandType::Swamp,
            ManaColor::Black,
            true,
            json!([{"seatId":seats()[1],"redOrGreen":"0","blackOrGreen":"1"}]),
        ),
        (
            "5b2364d7-a811-4595-a1b4-224c70555ffa",
            "bc71ebf6-2056-41f7-be35-b2e5c34afa99",
            BasicLandType::Plains,
            ManaColor::White,
            false,
            json!([]),
        ),
    ];
    for (spell, land, basic, color, targeted, expected) in cases {
        let f = cast_fixture(spell, land, basic, color, targeted);
        let evidence = f.tip.prepare_activity_results().unwrap();
        let result: Value = serde_json::from_slice(evidence.original_bytes()).unwrap();
        assert_eq!(result["activity"], expected);
        assert_eq!(
            replay(&f)
                .prepare_activity_results()
                .unwrap()
                .original_bytes(),
            evidence.original_bytes()
        );
    }
}
