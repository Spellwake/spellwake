//! Execute the two published card programs used by the constructed admission tests.
use super::*;
use spellwake_engine::{
    combat::{AttackDeclaration, BlockDeclaration, CombatDecision},
    game::{DecisionReply, GameCommand, GameContext},
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaPayment, ManaSpend, ManaType},
    object_identity::{GameObject, ObjectZone},
    priority::PriorityPhase,
    stack_window::WindowPhase,
    turn::{Step as TurnStep, TurnStart},
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const BEARS: &str = "14c8f55d-d177-4c25-a931-ebeb9e6062a0";

fn start() -> GameState {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let battlefield = |controller| ObjectDestination::Battlefield { controller };
    let mut slots = vec![
        (FOREST, A, ObjectDestination::Hand),
        (FOREST, A, ObjectDestination::Hand),
        (FOREST, A, battlefield(A)),
        (BEARS, A, ObjectDestination::Hand),
        (BEARS, A, battlefield(A)),
        (BEARS, B, battlefield(B)),
    ];
    for seat in [A, B] {
        slots.extend([(FOREST, seat, ObjectDestination::LibraryBottom); 4]);
    }
    let cards = slots
        .into_iter()
        .enumerate()
        .map(|(index, (oracle, seat, dest))| {
            source
                .registry
                .seed(
                    &OracleId::try_from(oracle.to_owned()).unwrap(),
                    CardId(1 + index as u64),
                    seat,
                    dest,
                )
                .unwrap()
        })
        .collect();
    let context = GameContext::new(
        "two-card-behavior-v1".into(),
        source.vocabulary.rules_revision().into(),
        source.snapshot.as_str().into(),
        source.identity().as_str().into(),
        "two-seat-ffa".into(),
        source.vocabulary.id(),
        source.names.id(),
    )
    .unwrap();
    GameState::initialize_scheduled(
        GameId(51),
        context,
        GameSeed::with_uniform_life(
            vec![A, B],
            A,
            cards,
            vec![CardId(3), CardId(5), CardId(6)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            source.vocabulary.clone(),
            source.names.clone(),
        ),
        TurnStart {
            number: 1,
            step: TurnStep::Untap,
        },
    )
    .unwrap()
    .into_state()
}

fn physical(game: &GameState, card: u64) -> &GameObject {
    let instances = game.execution().window().cards();
    instances
        .get(instances.current_identity(CardId(card)).unwrap())
        .unwrap()
}
fn apply(game: &GameState, actor: SeatId, command: GameCommand) -> Result<GameState, GameError> {
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    })
    .map(NativeCandidate::into_state)
}
fn reply(game: &GameState, choice: DecisionReply) -> GameState {
    let d = game.pending_decision().unwrap();
    apply(
        game,
        d.chooser(),
        GameCommand::Reply {
            decision: d.id(),
            choice,
        },
    )
    .unwrap()
}
fn until(mut game: GameState, at: impl Fn(&GameState) -> bool) -> GameState {
    for _ in 0..120 {
        if at(&game) {
            return game;
        }
        game = if let Some(d) = game.pending_decision() {
            match d.frame() {
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    reply(&game, DecisionReply::DeclareAttackers(vec![]))
                }
                other => panic!("unexpected decision while passing: {other:?}"),
            }
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                game.execution().window().phase()
            else {
                panic!("expected priority")
            };
            apply(&game, holder, GameCommand::Pass).unwrap()
        };
    }
    panic!("requested turn or combat boundary was never reached")
}
fn main_phase(game: &GameState) -> bool {
    game.turn().unwrap().position().step == TurnStep::PrecombatMain
        && game.execution().window().active_player() == Some(A)
        && matches!(
            game.execution().window().phase(),
            WindowPhase::Priority(PriorityPhase::Ready { holder: A })
        )
}
fn forest(card: u64, game: &GameState) -> GameCommand {
    GameCommand::PlayLand {
        land: physical(game, card).id(),
    }
}
fn tap(card: u64, game: &GameState) -> GameCommand {
    GameCommand::ActivateMana {
        source: physical(game, card).id(),
        ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
    }
}
fn cast(game: &GameState) -> GameCommand {
    GameCommand::CastUntargeted {
        spell: physical(game, 4).id(),
        payment: Some(ManaPayment {
            spend: vec![ManaSpend {
                mana_type: ManaType::Green,
                amount: 2.into(),
            }],
        }),
    }
}

#[test]
#[ignore = "requires the retained Oracle catalogue and creature vocabulary"]
fn published_forest_obeys_timing_land_allowance_tapping_and_untap() {
    let start = start();
    assert!(apply(&start, A, forest(1, &start)).is_err());
    let main = until(start, main_phase);
    let land = apply(&main, A, forest(1, &main)).unwrap();
    assert_eq!(physical(&land, 1).zone(), ObjectZone::Battlefield);
    assert!(apply(&land, A, forest(2, &land)).is_err());
    assert_eq!(physical(&land, 2).zone(), ObjectZone::Hand(A));
    let tapped = apply(&land, A, tap(1, &land)).unwrap();
    assert_eq!(physical(&tapped, 1).tapped(), Some(true));
    assert_eq!(tapped.execution().mana(A, ManaType::Green), 1.into());
    assert!(apply(&tapped, A, tap(1, &tapped)).is_err());
    let next = until(tapped, |s| {
        main_phase(s) && s.turn().unwrap().lands_played() == &0.into()
    });
    assert_eq!(physical(&next, 1).tapped(), Some(false));
    assert_eq!(next.execution().mana(A, ManaType::Green), 0.into());
    assert!(apply(&next, A, forest(2, &next)).is_ok());
}

#[test]
#[ignore = "requires the retained Oracle catalogue and creature vocabulary"]
fn published_grizzly_bears_pays_resolves_waits_to_attack_and_trades_in_combat() {
    let main = until(start(), main_phase);
    let land = apply(&main, A, forest(1, &main)).unwrap();
    let first = apply(&land, A, tap(1, &land)).unwrap();
    assert!(apply(&first, A, cast(&first)).is_err());
    assert_eq!(physical(&first, 4).zone(), ObjectZone::Hand(A));
    let both = apply(&first, A, tap(3, &first)).unwrap();
    let stack = apply(&both, A, cast(&both)).unwrap();
    assert_eq!(physical(&stack, 4).zone(), ObjectZone::Stack);
    assert_eq!(stack.execution().mana(A, ManaType::Green), 0.into());
    let resolved = until(stack, |s| physical(s, 4).zone() == ObjectZone::Battlefield);
    let stats = resolved
        .execution()
        .power_toughness(physical(&resolved, 4).id())
        .unwrap()
        .unwrap()
        .derived;
    assert_eq!((stats.power, stats.toughness), (2.into(), 2.into()));
    let sick = until(resolved, |s| s.pending_decision().is_some());
    let d = sick.pending_decision().unwrap();
    let RuleFrame::Combat(CombatDecision::Attackers(options)) = d.frame() else {
        panic!("expected attackers")
    };
    assert!(
        options
            .eligible_attackers()
            .contains(&physical(&sick, 5).id())
    );
    assert!(
        !options
            .eligible_attackers()
            .contains(&physical(&sick, 4).id())
    );
    let attack = |s: &GameState| AttackDeclaration {
        attacker: physical(s, 4).id(),
        defender: B,
    };
    assert!(
        apply(
            &sick,
            A,
            GameCommand::Reply {
                decision: d.id(),
                choice: DecisionReply::DeclareAttackers(vec![attack(&sick)])
            }
        )
        .is_err()
    );
    let pass = reply(&sick, DecisionReply::DeclareAttackers(vec![]));
    let attack_turn = until(pass, |s| {
        s.pending_decision().is_some_and(|d| {
            d.chooser() == A && matches!(d.frame(), RuleFrame::Combat(CombatDecision::Attackers(_)))
        })
    });
    let declared = reply(
        &attack_turn,
        DecisionReply::DeclareAttackers(vec![attack(&attack_turn)]),
    );
    assert_eq!(physical(&declared, 4).tapped(), Some(true));
    let blockers = until(declared, |s| s.pending_decision().is_some());
    assert_eq!(blockers.pending_decision().unwrap().chooser(), B);
    let blocked = reply(
        &blockers,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker: physical(&blockers, 6).id(),
            attacker: physical(&blockers, 4).id(),
        }]),
    );
    let after = until(blocked, |s| {
        physical(s, 4).zone() == ObjectZone::Graveyard(A)
    });
    assert_eq!(physical(&after, 6).zone(), ObjectZone::Graveyard(B));
}
