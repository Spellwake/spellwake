//! Three-seat native checkpoint/recovery regression using genuine Forest and Grizzly Bears data.
use serde_json::json;
use spellwake_application::{native_game::*, player_wire::*};
use spellwake_card_programs::OracleId;
use spellwake_engine::{
    checks::CheckDecision,
    combat::CombatDecision,
    game::RuleFrame,
    land::BasicLandType,
    mana::ManaAbilityRef,
    object_identity::ObjectZone,
    opening::{MulliganChoice, OpeningDecision},
    priority::{PriorityPhase, SeatId},
    stack_window::WindowPhase,
    turn::Step,
};
use std::{env, fs};
use uuid::Uuid;

const BEARS: &str = "14c8f55d-d177-4c25-a931-ebeb9e6062a0";
const FOREST: &str = "b34bb2dc-c1af-4d77-b0b3-a0fb342a5fc6";
const CARDS: usize = 36;
fn seats() -> [Uuid; 3] {
    [
        Uuid::from_u128(101),
        Uuid::from_u128(102),
        Uuid::from_u128(103),
    ]
}
fn is_bear(object: &spellwake_engine::object_identity::GameObject) -> bool {
    (object.card_id().unwrap().0 - 1).is_multiple_of(3)
}
fn priority_action(
    cp: &NativeCheckpoint,
    scope: &mut NativeScope<'_>,
    holder: SeatId,
) -> PlayerAction {
    let state = cp.state();
    let window = state.execution().window();
    let turn = state.turn().unwrap();
    if holder.0 == 2
        || window.active_player() != Some(holder)
        || turn.position().step != Step::PrecombatMain
    {
        return PlayerAction::Pass {};
    }
    let hand: Vec<_> = window
        .cards()
        .iter()
        .filter(|o| o.zone() == ObjectZone::Hand(holder))
        .collect();
    let board: Vec<_> = window
        .cards()
        .iter()
        .filter(|o| o.zone() == ObjectZone::Battlefield && o.controller() == Some(holder))
        .collect();
    if turn.lands_played() == &0.into()
        && let Some(land) = hand.iter().find(|o| !is_bear(o))
    {
        return PlayerAction::PlayLand {
            source: scope.issue(NativeReference::Object(land.id())).unwrap(),
        };
    }
    let target = if holder.0 == 0 { 3 } else { 2 };
    if board.iter().filter(|o| is_bear(o)).count() >= target {
        return PlayerAction::Pass {};
    }
    let Some(bear) = hand.iter().find(|o| is_bear(o)) else {
        return PlayerAction::Pass {};
    };
    let cast = PlayerAction::CastUntargeted {
        source: scope.issue(NativeReference::Object(bear.id())).unwrap(),
        payment: Some(PlayerPayment {
            spend: vec![ManaSpend {
                color: ManaColor::Green,
                amount: RulesCount::try_from("2".to_owned()).unwrap(),
            }],
        }),
    };
    let can = |action: &PlayerAction| {
        scope
            .propose(&PlayerCommand {
                request_id: Uuid::new_v4(),
                basis: scope.basis().clone(),
                action: action.clone(),
            })
            .is_ok()
    };
    if can(&cast) {
        return cast;
    }
    for land in board
        .into_iter()
        .filter(|o| !is_bear(o) && o.tapped() == Some(false))
    {
        let act = PlayerAction::ActivateMana {
            source: scope.issue(NativeReference::Object(land.id())).unwrap(),
            ability: scope
                .issue(NativeReference::ManaAbility {
                    source: land.id(),
                    ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
                })
                .unwrap(),
        };
        if scope
            .propose(&PlayerCommand {
                request_id: Uuid::new_v4(),
                basis: scope.basis().clone(),
                action: act.clone(),
            })
            .is_ok()
        {
            return act;
        }
    }
    PlayerAction::Pass {}
}
fn retain(
    store: &NativeArtifactStore,
    cp: &NativeCheckpoint,
    role: NativeArtifactRole,
    bytes: &[u8],
) -> NativeArtifactRef {
    store
        .retain(cp.basis().game_id, cp.basis().context_id, role, bytes)
        .unwrap()
}
#[test]
#[ignore = "requires the genuine catalogue/vocabulary and an isolated private native artifact store"]
fn lethal_combat_and_graveyard_timeout_recover_one_terminal_from_original_inputs() {
    let read = |key: &str| fs::read(env::var(key).unwrap()).unwrap();
    let programs = NativeProgramSource::from_bytes(
        NativeProgramPool::ResolutionSelectionNaturalMana,
        &read("SPELLWAKE_CATALOGUE_METADATA"),
        read("SPELLWAKE_CATALOGUE_INPUT"),
        &read("SPELLWAKE_CREATURE_VOCABULARY_INPUT"),
    )
    .unwrap();
    let ids = NativeInitialIds {
        game_id: Uuid::from_u128(90),
        context_id: Uuid::from_u128(91),
        release_id: Uuid::from_u128(92),
        profile_id: Uuid::from_u128(93),
    };
    let seats = seats();
    let spec = NativeInitialSpec {
        first_player_seat_id: seats[0],
        participants: seats
            .iter()
            .enumerate()
            .map(|(i, &seat_id)| NativeInitialParticipant {
                seat_id,
                life: if i == 2 { 2.into() } else { 20.into() },
                cards: (0..CARDS)
                    .map(|n| NativeInitialCard {
                        oracle_id: OracleId::try_from(
                            if n % 3 == 0 { BEARS } else { FOREST }.to_owned(),
                        )
                        .unwrap(),
                        zone: NativeInitialZone::Library,
                    })
                    .collect(),
            })
            .collect(),
        entropy: vec![0.into(); 500],
    };
    let opening = prepare_initial(ids, &programs, spec).unwrap();
    let store = NativeArtifactStore::from_env().unwrap();
    let original = store
        .retain(
            ids.game_id,
            ids.context_id,
            NativeArtifactRole::Initial,
            opening.original_bytes(),
        )
        .unwrap();
    assert!(&original == opening.reference());
    let root = NativeInitialExpected::new(original, ids.release_id, ids.profile_id, seats.to_vec())
        .unwrap();
    let mut cp = NativeCheckpoint::initial(opening).unwrap();
    cp.retain(&store).unwrap();
    let mut attacked = false;
    let mut blocked = false;
    let mut ordered = false;
    let mut preserved = vec![];
    for _ in 0..1200 {
        let state = cp.state();
        let window = state.execution().window();
        let pending = state.pending_decision();
        let holder = match pending {
            Some(p) => p.chooser(),
            None => match window.phase() {
                WindowPhase::Priority(PriorityPhase::Ready { holder }) => holder,
                other => panic!("unexpected protocol phase {other:?}"),
            },
        };
        let viewer = seats[usize::try_from(holder.0).unwrap()];
        let mut scope = NativeScope::new(state, cp.binding(viewer).unwrap()).unwrap();
        let (kind, bytes) = if let Some(p) = pending {
            let decision = scope.issue(NativeReference::Decision(p.id())).unwrap();
            let choice = match p.frame() {
                RuleFrame::Opening(OpeningDecision::Declare { .. }) => PlayerChoice::Option {
                    option: scope
                        .issue(NativeReference::Mulligan(MulliganChoice::Keep))
                        .unwrap(),
                },
                RuleFrame::Combat(CombatDecision::Attackers(domain)) => {
                    let defenders = window
                        .cards()
                        .iter()
                        .filter(|o| {
                            o.zone() == ObjectZone::Battlefield
                                && o.controller() == Some(SeatId(1))
                                && is_bear(o)
                                && o.tapped() == Some(false)
                        })
                        .count();
                    let mut assignments = vec![];
                    if holder == SeatId(0)
                        && domain.eligible_attackers().len() >= 3
                        && defenders >= 2
                    {
                        attacked = true;
                        for (index, id) in domain.eligible_attackers().iter().take(3).enumerate() {
                            assignments.push(AttackerAssignment {
                                attacker: scope.issue(NativeReference::Object(*id)).unwrap(),
                                defender: PlayerTarget::Seat {
                                    seat_id: seats[if index == 2 { 2 } else { 1 }],
                                },
                            });
                        }
                    }
                    PlayerChoice::Attackers { assignments }
                }
                RuleFrame::Combat(CombatDecision::Blockers(domain)) => {
                    let mut assignments = vec![];
                    if attacked && holder == SeatId(1) {
                        for (&blocker, &attacker) in domain
                            .eligible_blockers()
                            .iter()
                            .zip(domain.attackers())
                            .take(2)
                        {
                            assignments.push(BlockerAssignment {
                                blocker: scope.issue(NativeReference::Object(blocker)).unwrap(),
                                attackers: vec![
                                    scope.issue(NativeReference::Object(attacker)).unwrap(),
                                ],
                            });
                        }
                        assert_eq!(assignments.len(), 2);
                        blocked = true;
                    }
                    PlayerChoice::Blockers { assignments }
                }
                RuleFrame::Checks(CheckDecision::GraveyardOrder(order)) if blocked => {
                    assert_eq!(window.participants().len(), 3);
                    if ordered {
                        assert_eq!(holder, SeatId(1));
                        assert_eq!(state.execution().life_total(SeatId(2)), Some(&0.into()));
                        break;
                    }
                    assert_eq!(holder, SeatId(0));
                    assert_eq!(order.domain().len(), 2);
                    ordered = true;
                    preserved = order
                        .domain()
                        .iter()
                        .rev()
                        .map(|id| window.cards().get(*id).unwrap().card_id().unwrap())
                        .collect();
                    PlayerChoice::Ordering {
                        ordered: order
                            .domain()
                            .iter()
                            .rev()
                            .map(|id| scope.issue(NativeReference::Object(*id)).unwrap())
                            .collect(),
                    }
                }
                RuleFrame::Checks(CheckDecision::CleanupDiscard(c)) => PlayerChoice::Objects {
                    objects: c
                        .domain()
                        .iter()
                        .take(c.required())
                        .map(|id| scope.issue(NativeReference::Object(*id)).unwrap())
                        .collect(),
                },
                other => panic!("unhandled fixture decision {other:?}"),
            };
            let action = PlayerAction::Choose {
                decision,
                response: choice,
            };
            let proposal = scope
                .propose(&PlayerCommand {
                    request_id: Uuid::new_v4(),
                    basis: scope.basis().clone(),
                    action,
                })
                .unwrap();
            (
                NativeCheckpointInput::Player,
                scope.encode_input(&proposal).unwrap(),
            )
        } else {
            let action = priority_action(&cp, &mut scope, holder);
            let proposal = scope
                .propose(&PlayerCommand {
                    request_id: Uuid::new_v4(),
                    basis: scope.basis().clone(),
                    action,
                })
                .unwrap();
            (
                NativeCheckpointInput::Player,
                scope.encode_input(&proposal).unwrap(),
            )
        };
        let input = retain(&store, &cp, NativeArtifactRole::Input, &bytes);
        cp = cp.append(&store, input, kind, viewer).unwrap();
        cp.retain(&store).unwrap();
    }
    assert!(attacked && blocked && ordered);
    let before = cp.state().pending_decision().unwrap();
    assert_eq!(before.chooser(), SeatId(1));
    let scope = NativeScope::new(cp.state(), cp.binding(seats[1]).unwrap()).unwrap();
    let due = scope.prepare_response_timeout(seats[1]).unwrap();
    let input = retain(&store, &cp, NativeArtifactRole::Input, due.record());
    let next = cp
        .append(
            &store,
            input.clone(),
            NativeCheckpointInput::ResponseTimeout,
            seats[1],
        )
        .unwrap();
    next.retain(&store).unwrap();
    assert_eq!(
        next.original_bytes(),
        cp.append(
            &store,
            input,
            NativeCheckpointInput::ResponseTimeout,
            seats[1]
        )
        .unwrap()
        .original_bytes()
    );
    let expected =
        NativeCheckpointExpected::new(next.reference().clone(), next.basis().clone()).unwrap();
    let cold = load_checkpoint(
        &store,
        &expected,
        &root,
        &programs,
        NativeRecoveryBudget::new(2000, 100_000_000).unwrap(),
    )
    .unwrap();
    assert_eq!(next.candidate(), cold.candidate());
    assert_eq!(next.original_bytes(), cold.original_bytes());
    for tip in [&next, &cold] {
        let terminal = tip.terminal().unwrap().unwrap();
        assert_eq!(terminal.reason(), "native_checked_losses");
        assert_eq!(terminal.winner_seat_ids(), &[seats[0]]);
        assert_eq!(terminal.losing_seat_ids().len(), 2);
        let cards = tip.state().execution().window().cards();
        let ordered: Vec<_> = cards
            .graveyard(SeatId(0))
            .iter()
            .map(|id| cards.get(*id).unwrap().card_id().unwrap())
            .collect();
        assert_eq!(ordered, preserved);
    }
    assert_eq!(
        next.prepare_event().unwrap().original_bytes(),
        cold.prepare_event().unwrap().original_bytes()
    );
    assert_eq!(
        next.prepare_results().unwrap().original_bytes(),
        cold.prepare_results().unwrap().original_bytes()
    );
    println!(
        "CHECKED_LOSS {}",
        json!({"runtime":native_runtime_identity(),"acceptedInputCount":cold.basis().revision,"state":cold.reference(),"results":cold.prepare_results().unwrap().reference(),"event":cold.prepare_event().unwrap().reference()})
    );
}
