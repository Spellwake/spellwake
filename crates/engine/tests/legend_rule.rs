//! CR201.2a/201.3/704.5j/704.3: actual frozen choices and one simultaneous event.
//! Initial battlefield premises are prepared; no full-card/format claim follows.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod tr;
use spellwake_contracts::names::{EnglishCardName, NameRules, ObjectNames};
use spellwake_engine::{
    characteristics::{NameOperation, ResolvedDuration, ResolvedPartSpec},
    checks::{CheckChoice, CheckDecision, CheckError, LegendChoice, LegendConflictId},
    combat::CombatRules,
    effects::{EffectError, SeedCard},
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    mana::ManaCost,
    object_identity::{CardId, ObjectDestination, ObjectZone},
    occurrence::{RulesOccurrence, StateBasedMoveReason, ZoneReason},
    priority::SeatId,
    program::{BodyTarget, Instruction, Program, Supertype},
    setup::GameSeed,
    turn::{Step, TurnStart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tr::{P0, P1};
const P2: SeatId = SeatId(2);
fn names(labels: &[&str]) -> ObjectNames {
    ObjectNames::new(
        labels
            .iter()
            .map(|s| EnglishCardName::new((*s).into()).unwrap())
            .collect(),
    )
    .unwrap()
}
fn card(n: u64, owner: SeatId, controller: SeatId, labels: &[&str]) -> SeedCard {
    let mut p = Program::artifact(Some(ManaCost::free()), vec![]);
    p.names = names(labels);
    p.supertypes.insert(Supertype::Legendary);
    tr::controlled(n, owner, controller, p)
}
fn initial(
    cards: Vec<SeedCard>,
    players: Vec<SeatId>,
    rules: NameRules,
    cohorts: Vec<Vec<CardId>>,
) -> NativeCandidate {
    let order = cards
        .iter()
        .rev()
        .filter(|c| matches!(c.destination, ObjectDestination::Battlefield { .. }))
        .map(|c| c.card)
        .collect();
    let mut seed = GameSeed::with_uniform_life(
        players,
        P0,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        tr::vocabulary(),
        Arc::new(rules),
    );
    seed.initial_world_cohorts = cohorts;
    let context = GameContext::new(
        "legend-root".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "literal".into(),
        "legend-foundation".into(),
        "prepared-main".into(),
        seed.creature_vocabulary.id(),
        seed.name_rules.id(),
    )
    .unwrap();
    GameState::initialize_scheduled(
        GameId(7045),
        context,
        seed,
        TurnStart {
            number: 3,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
}
fn literal(cards: Vec<SeedCard>) -> NativeCandidate {
    initial(cards, vec![P0, P1], NameRules::literal_names(), vec![])
}
fn legend(game: &GameState) -> &LegendChoice {
    let RuleFrame::Checks(CheckDecision::Legend(f)) =
        game.pending_decision().expect("pending legend").frame()
    else {
        panic!("legend frame required")
    };
    f
}
fn keep(game: &GameState, conflict: LegendConflictId, card: u64) -> NativeCandidate {
    tr::reply(
        game,
        legend(game).chooser(),
        DecisionReply::KeepLegend {
            conflict,
            keep: tr::object(game, card),
        },
    )
}
fn order(game: &GameState, expected: &[u64]) -> NativeCandidate {
    let RuleFrame::Checks(CheckDecision::GraveyardOrder(f)) =
        game.pending_decision().unwrap().frame()
    else {
        panic!("graveyard frame required")
    };
    let selected: Vec<_> = expected.iter().map(|n| tr::object(game, *n)).collect();
    assert_eq!(
        f.domain().iter().copied().collect::<BTreeSet<_>>(),
        selected.iter().copied().collect()
    );
    tr::reply(game, f.chooser(), DecisionReply::OrderGraveyard(selected))
}
fn zone(game: &GameState, n: u64) -> ObjectZone {
    game.execution()
        .window()
        .cards()
        .get(tr::object(game, n))
        .unwrap()
        .zone()
}
fn assert_unchanged(game: &GameState, input: GameInput, expected: Option<GameError>) {
    let saved = game.clone();
    let result = game.advance(input);
    if let Some(error) = expected {
        assert_eq!(result, Err(error));
    } else {
        assert!(result.is_err());
    }
    assert_eq!(game, &saved);
    assert_eq!(
        game.execution().operational_counters(),
        saved.execution().operational_counters()
    );
}
fn reply_input(game: &GameState, actor: SeatId, choice: DecisionReply) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor,
        command: GameCommand::Reply {
            decision: game.pending_decision().unwrap().id(),
            choice,
        },
    }
}

#[test]
fn one_common_name_cohort_collapses_equal_and_strict_subset_witnesses() {
    for labels in [
        vec![vec!["Alpha"], vec!["Alpha"], vec!["Alpha"]],
        vec![vec!["Alpha", "Beta"], vec!["Alpha", "Beta"], vec!["Alpha"]],
    ] {
        let start = literal(
            labels
                .iter()
                .enumerate()
                .map(|(i, n)| card(u64::try_from(i).unwrap() + 1, P0, P0, n))
                .collect(),
        );
        let f = legend(start.state());
        assert_eq!(f.conflicts().len(), 1);
        assert_eq!(f.conflicts()[0].domain().len(), 3);
        let kept = keep(start.state(), f.conflicts()[0].id(), 1);
        assert!(kept.check_journal().moves.is_empty());
        assert!(
            matches!(&kept.check_journal().choices[0].choice,CheckChoice::LegendKeep { keep, domain, .. } if *keep == tr::object(start.state(),1) && domain.len()==3)
        );
        let done = order(kept.state(), &[3, 2]);
        assert_eq!(done.check_journal().moves.len(), 2);
        assert_eq!(zone(done.state(), 1), ObjectZone::Battlefield);
        assert_eq!(
            done.state()
                .execution()
                .window()
                .cards()
                .graveyard(P0)
                .iter()
                .map(|o| done
                    .state()
                    .execution()
                    .window()
                    .cards()
                    .get(*o)
                    .unwrap()
                    .card_id()
                    .expect("physical fixture card"))
                .collect::<Vec<_>>(),
            vec![CardId(3), CardId(2)]
        );
    }
}

#[test]
fn nameless_nonlegend_and_other_controllers_do_not_join_a_conflict() {
    let mut ordinary = card(4, P0, P0, &["Alpha"]);
    ordinary.program.supertypes.clear();
    let start = literal(vec![
        card(1, P0, P0, &[]),
        card(2, P0, P0, &[]),
        card(3, P1, P1, &["Alpha"]),
        ordinary,
    ]);
    assert!(start.state().pending_decision().is_none());
    assert!(start.check_journal().moves.is_empty());
    let start = literal(vec![
        card(1, P0, P0, &["Alpha"]),
        card(2, P0, P0, &["Alpha"]),
        card(3, P1, P1, &["Alpha"]),
    ]);
    let done = keep(start.state(), legend(start.state()).conflicts()[0].id(), 2);
    assert_eq!(zone(done.state(), 1), ObjectZone::Graveyard(P0));
    assert_eq!(zone(done.state(), 2), ObjectZone::Battlefield);
    assert_eq!(zone(done.state(), 3), ObjectZone::Battlefield);
}

#[test]
fn supplied_aliases_collapse_comparison_forms_without_changing_actual_names() {
    let rules = NameRules::new(vec![names(&["Alpha", "Alternate"])]).unwrap();
    let start = initial(
        vec![
            card(1, P0, P0, &["Alpha", "Alternate"]),
            card(2, P0, P0, &["Alternate"]),
        ],
        vec![P0, P1],
        rules,
        vec![],
    );
    let f = legend(start.state());
    assert_eq!(f.conflicts().len(), 1);
    assert_eq!(f.conflicts()[0].domain().len(), 2);
    assert_eq!(f.conflicts()[0].comparison_names().len(), 1);
    let done = keep(start.state(), f.conflicts()[0].id(), 2);
    assert_eq!(
        tr::current(done.state(), tr::object(done.state(), 2)).current_names,
        names(&["Alternate"])
    );
    assert_eq!(zone(done.state(), 1), ObjectZone::Graveyard(P0));
}

#[test]
fn choosing_nonfirst_overlapping_conflict_keeps_every_other_original_domain_even_if_doomed() {
    let start = literal(vec![
        card(1, P0, P0, &["Alpha"]),
        card(2, P0, P0, &["Alpha", "Beta"]),
        card(3, P0, P0, &["Beta"]),
    ]);
    let f = legend(start.state());
    assert_eq!(f.conflicts().len(), 2);
    let first = f.conflicts()[0].clone();
    let second = f.conflicts()[1].clone();
    let prefix = keep(start.state(), second.id(), 2);
    assert!(prefix.check_journal().moves.is_empty());
    for n in [1, 2, 3] {
        assert_eq!(zone(prefix.state(), n), ObjectZone::Battlefield);
    }
    let next = legend(prefix.state());
    assert_eq!(next.conflicts(), std::slice::from_ref(&first));
    assert!(
        next.conflicts()[0]
            .domain()
            .contains(&tr::object(prefix.state(), 2))
    );
    let kept = keep(prefix.state(), first.id(), 1);
    let done = order(kept.state(), &[2, 3]);
    assert_eq!(done.check_journal().moves.len(), 2);
    assert_eq!(zone(done.state(), 1), ObjectZone::Battlefield);
    assert_eq!(zone(done.state(), 2), ObjectZone::Graveyard(P0));
    assert_eq!(zone(done.state(), 3), ObjectZone::Graveyard(P0));
}

#[test]
fn pairwise_triangle_is_three_frozen_pairs_and_can_move_every_object_once() {
    let start = literal(vec![
        card(1, P0, P0, &["Alpha", "Beta"]),
        card(2, P0, P0, &["Beta", "Gamma"]),
        card(3, P0, P0, &["Gamma", "Alpha"]),
    ]);
    let conflicts = legend(start.state()).conflicts().to_vec();
    assert_eq!(conflicts.len(), 3);
    assert!(conflicts.iter().all(|c| c.domain().len() == 2));
    let selected = BTreeMap::from([
        (BTreeSet::from([1, 2]), 1),
        (BTreeSet::from([2, 3]), 2),
        (BTreeSet::from([1, 3]), 3),
    ]);
    let mut game = start.into_state();
    for c in conflicts.iter().rev() {
        let domain = c
            .domain()
            .iter()
            .map(|id| {
                game.execution()
                    .window()
                    .cards()
                    .get(*id)
                    .unwrap()
                    .card_id()
                    .expect("physical fixture card")
                    .0
            })
            .collect::<BTreeSet<_>>();
        let next = keep(&game, c.id(), selected[&domain]);
        assert!(next.check_journal().moves.is_empty());
        game = next.into_state();
    }
    let done = order(&game, &[3, 1, 2]);
    assert_eq!(done.check_journal().moves.len(), 3);
    assert_eq!(
        done.check_journal()
            .moves
            .iter()
            .map(|m| m.before.id())
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );
    for n in [1, 2, 3] {
        assert_eq!(zone(done.state(), n), ObjectZone::Graveyard(P0));
    }
}

#[test]
fn controller_apnap_keeps_precede_actual_owner_apnap_arrangements() {
    let start = initial(
        vec![
            card(1, P2, P0, &["Alpha"]),
            card(2, P1, P0, &["Alpha"]),
            card(3, P0, P1, &["Alpha"]),
            card(4, P2, P1, &["Alpha"]),
            card(5, P0, P2, &["Alpha"]),
            card(6, P1, P2, &["Alpha"]),
        ],
        vec![P2, P0, P1],
        NameRules::literal_names(),
        vec![],
    );
    let mut game = start.into_state();
    for (controller, selected) in [(P0, 1), (P1, 3), (P2, 5)] {
        assert_eq!(legend(&game).chooser(), controller);
        let next = keep(&game, legend(&game).conflicts()[0].id(), selected);
        assert!(next.check_journal().moves.is_empty());
        game = next.into_state();
    }
    let RuleFrame::Checks(CheckDecision::GraveyardOrder(f)) =
        game.pending_decision().unwrap().frame()
    else {
        panic!()
    };
    assert_eq!(f.chooser(), P1);
    for n in 1..=6 {
        assert_eq!(zone(&game, n), ObjectZone::Battlefield);
    }
    let done = order(&game, &[6, 2]);
    assert_eq!(done.check_journal().moves.len(), 3);
    assert_eq!(zone(done.state(), 4), ObjectZone::Graveyard(P2));
    assert_eq!(zone(done.state(), 1), ObjectZone::Battlefield);
}

#[test]
fn automatic_causes_and_legend_union_without_duplicate_moves_or_sparing_doomed_keeps() {
    for chosen in [1, 2] {
        let mut first = card(1, P0, P0, &["Alpha"]);
        first.program =
            Program::creature(Some(ManaCost::free()), 1.into(), 0.into(), BTreeSet::new());
        first.program.names = names(&["Alpha"]);
        first.program.supertypes = BTreeSet::from([Supertype::Legendary, Supertype::World]);
        let mut third = card(3, P1, P1, &["Other"]);
        third.program.supertypes = BTreeSet::from([Supertype::World]);
        let start = initial(
            vec![first, card(2, P0, P0, &["Alpha"]), third],
            vec![P0, P1],
            NameRules::literal_names(),
            vec![vec![CardId(1)], vec![CardId(3)]],
        );
        let next = keep(
            start.state(),
            legend(start.state()).conflicts()[0].id(),
            chosen,
        );
        let done = if chosen == 1 {
            order(next.state(), &[2, 1])
        } else {
            next
        };
        let moved = tr::all_occurrences(&done)
            .into_iter()
            .filter_map(|o| match o {
                RulesOccurrence::ZoneChange(z)
                    if z.before.object.card_id().expect("physical fixture card") == CardId(1) =>
                {
                    Some(z)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(moved.len(), 1);
        let ZoneReason::StateBased(causes) = moved[0].reason else {
            panic!()
        };
        assert!(causes.contains(StateBasedMoveReason::ZeroToughness));
        assert!(causes.contains(StateBasedMoveReason::World));
        assert_eq!(causes.contains(StateBasedMoveReason::Legend), chosen == 2);
        assert_eq!(zone(done.state(), 1), ObjectZone::Graveyard(P0));
        assert_eq!(
            zone(done.state(), 2),
            if chosen == 1 {
                ObjectZone::Graveyard(P0)
            } else {
                ObjectZone::Battlefield
            }
        );
        assert_eq!(zone(done.state(), 3), ObjectZone::Battlefield);
    }
}

#[test]
fn invalid_actor_kind_keep_replayed_conflict_basis_and_token_preserve_the_whole_prefix() {
    let start = literal(vec![
        card(1, P0, P0, &["Alpha"]),
        card(2, P0, P0, &["Alpha", "Beta"]),
        card(3, P0, P0, &["Beta"]),
    ]);
    let game = start.state();
    let f = legend(game);
    let first = f.conflicts()[0].id();
    let second = f.conflicts()[1].id();
    let valid = DecisionReply::KeepLegend {
        conflict: second,
        keep: tr::object(game, 2),
    };
    assert_unchanged(game, reply_input(game, P1, valid.clone()), None);
    assert_unchanged(
        game,
        reply_input(
            game,
            P0,
            DecisionReply::OrderGraveyard(vec![tr::object(game, 2)]),
        ),
        Some(GameError::Effect(EffectError::WrongDecisionKind)),
    );
    assert_unchanged(
        game,
        reply_input(
            game,
            P0,
            DecisionReply::KeepLegend {
                conflict: first,
                keep: tr::object(game, 3),
            },
        ),
        Some(GameError::Effect(EffectError::Check(
            CheckError::InvalidLegendKeep,
        ))),
    );
    let old = reply_input(game, P0, valid.clone());
    let prefix = tr::submit(game, P0, old.command.clone());
    assert_unchanged(prefix.state(), old.clone(), None);
    let mut stale = old;
    stale.basis = prefix.state().basis().clone();
    assert_unchanged(prefix.state(), stale, None);
    assert_unchanged(
        prefix.state(),
        reply_input(prefix.state(), P0, valid),
        Some(GameError::Effect(EffectError::Check(
            CheckError::InvalidLegendConflict,
        ))),
    );
    assert_eq!(legend(prefix.state()).conflicts()[0].id(), first);
}

#[test]
fn cleanup_expiration_exposes_a_legend_conflict_and_keeps_priority_before_repeat() {
    let existing = card(1, P0, P0, &["Alpha"]);
    let mut incoming = card(2, P0, P0, &["Alpha"]);
    incoming.destination = ObjectDestination::Hand;
    let start = literal(vec![
        existing,
        incoming,
        tr::spell(
            10,
            BodyTarget::SinglePermanent,
            vec![tr::effect(
                vec![ResolvedPartSpec::Names(NameOperation::Set(names(&[
                    "Beta",
                ])))],
                ResolvedDuration::UntilEndOfTurn,
            )],
        ),
    ]);
    let (cast, _) = tr::cast(start.state(), 10, tr::object(start.state(), 1));
    let renamed = tr::pass_pair(cast.state());
    let entered = tr::cast_permanent(renamed.state(), 2);
    assert!(entered.state().pending_decision().is_none());
    let end = tr::reach_end(entered.state(), [P0, P1]);
    let cleanup = tr::pass_pair(&end);
    assert_eq!(
        legend(cleanup.state()).boundary(),
        spellwake_engine::checks::CheckBoundary::Cleanup(
            cleanup.state().turn().unwrap().position()
        )
    );
    let done = keep(
        cleanup.state(),
        legend(cleanup.state()).conflicts()[0].id(),
        1,
    );
    assert_eq!(zone(done.state(), 2), ObjectZone::Graveyard(P0));
    assert_eq!(done.state().turn().unwrap().position().step, Step::Cleanup);
    assert_eq!(done.state().turn().unwrap().position().number, 3);
    let repeated = tr::pass_pair(done.state());
    assert_eq!(
        repeated.state().turn().unwrap().position().step,
        Step::Upkeep
    );
    assert_eq!(repeated.state().turn().unwrap().position().number, 4);
    assert_eq!(zone(repeated.state(), 1), ObjectZone::Battlefield);
}

#[test]
fn actual_supertype_control_copy_and_name_add_operations_create_current_legend_conflicts() {
    use spellwake_engine::{
        activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
        characteristics::{ControlRecipient, SubtypeOperation, TypeChangeSpec, TypeOperation},
        program::ExecutableBody,
    };
    for route in 0..4 {
        let first = card(1, P0, P0, &["Alpha"]);
        let mut second = card(2, P0, P0, &["Alpha"]);
        let action = match route {
            0 => {
                second.program.supertypes.clear();
                Some(tr::spell(
                    20,
                    BodyTarget::SinglePermanent,
                    vec![tr::effect(
                        vec![ResolvedPartSpec::Type(TypeChangeSpec {
                            land_subtypes: SubtypeOperation::Keep,
                            supertypes: TypeOperation::Add(BTreeSet::from([Supertype::Legendary])),
                            types: TypeOperation::Keep,
                            subtypes: SubtypeOperation::Keep,
                        })],
                        ResolvedDuration::NoStatedDuration,
                    )],
                ))
            }
            1 => {
                second.owner = P1;
                second.destination = ObjectDestination::Battlefield { controller: P1 };
                Some(tr::spell(
                    20,
                    BodyTarget::SinglePermanent,
                    vec![tr::effect(
                        vec![ResolvedPartSpec::Control(
                            ControlRecipient::ResolvingController,
                        )],
                        ResolvedDuration::NoStatedDuration,
                    )],
                ))
            }
            2 => {
                second.program.supertypes.clear();
                second.program.names = names(&["Beta"]);
                second.program.activated.push(ActivatedAbility {
                    id: ActivatedAbilityId(1),
                    mana_cost: ManaCost::free(),
                    source_cost: SourceCost::None,
                    body: ExecutableBody::new(
                        BodyTarget::SinglePermanent,
                        vec![Instruction::ApplyContinuous(Box::new(
                            spellwake_engine::toolbox::ApplyContinuousSpec::become_copy(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                    spellwake_engine::program::EffectSubject::AbilitySource,
                                )),
                                spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                                    spellwake_engine::program::MoveSubjectRef::TargetObject,
                                ),
                                ResolvedDuration::NoStatedDuration,
                                spellwake_engine::program::CopyExceptions {
                                    override_pt: None,
                                    ..Default::default()
                                },
                            ),
                        ))],
                    ),
                });
                None
            }
            3 => {
                second.program.names = names(&["Beta"]);
                Some(tr::spell(
                    20,
                    BodyTarget::SinglePermanent,
                    vec![tr::effect(
                        vec![ResolvedPartSpec::Names(NameOperation::Add(names(&[
                            "Alpha",
                        ])))],
                        ResolvedDuration::NoStatedDuration,
                    )],
                ))
            }
            _ => unreachable!(),
        };
        let mut cards = vec![first, second];
        if let Some(action) = action {
            cards.push(action);
        }
        let start = literal(cards);
        assert!(start.state().pending_decision().is_none());
        let announced = if route == 2 {
            tr::activate(
                start.state(),
                tr::object(start.state(), 2),
                tr::object(start.state(), 1),
                false,
            )
        } else {
            tr::cast(start.state(), 20, tr::object(start.state(), 2)).0
        };
        let pending = tr::pass_pair(announced.state());
        let f = legend(pending.state());
        assert_eq!(f.conflicts().len(), 1);
        assert_eq!(
            f.conflicts()[0]
                .domain()
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([tr::object(start.state(), 1), tr::object(start.state(), 2)])
        );
        let done = keep(pending.state(), f.conflicts()[0].id(), 2);
        assert_eq!(zone(done.state(), 1), ObjectZone::Graveyard(P0));
        assert_eq!(zone(done.state(), 2), ObjectZone::Battlefield);
    }
}

#[test]
fn the_next_observation_finds_a_legend_conflict_exposed_by_a_simultaneous_source_death() {
    use spellwake_engine::{
        characteristics::{PermanentSelector, StaticNameOperand, StaticPart},
        program::CardType,
    };
    let first = card(1, P0, P0, &["Alpha"]);
    let mut second = card(2, P0, P0, &["Alpha"]);
    second.program = Program::creature(None, 2.into(), 3.into(), BTreeSet::new());
    second.program.names = names(&["Alpha"]);
    second.program.supertypes.insert(Supertype::Legendary);
    let source = tr::card(
        3,
        tr::static_program(
            &tr::vocabulary(),
            &[CardType::Creature],
            Some((0, 0)),
            PermanentSelector {
                required_types: BTreeSet::from([CardType::Creature]),
                exclude_source: true,
                ..Default::default()
            },
            vec![StaticPart::Names(NameOperation::Set(
                StaticNameOperand::Literal(names(&["Beta"])),
            ))],
        ),
        true,
    );
    let start = literal(vec![first, second, source]);
    assert_eq!(start.check_journal().moves.len(), 1);
    assert_eq!(
        start.check_journal().moves[0]
            .before
            .card_id()
            .expect("physical fixture card"),
        CardId(3)
    );
    assert_eq!(start.check_journal().moves[0].event.pass(), 0);
    assert_eq!(zone(start.state(), 3), ObjectZone::Graveyard(P0));
    let f = legend(start.state());
    assert_eq!(f.event().pass(), 1);
    assert_eq!(f.conflicts().len(), 1);
    assert_eq!(
        tr::current(start.state(), tr::object(start.state(), 2)).current_names,
        names(&["Alpha"])
    );
    let done = keep(start.state(), f.conflicts()[0].id(), 2);
    assert_eq!(done.check_journal().moves.len(), 1);
    assert_eq!(done.check_journal().moves[0].event.pass(), 1);
    assert_eq!(zone(done.state(), 1), ObjectZone::Graveyard(P0));
    assert_eq!(zone(done.state(), 2), ObjectZone::Battlefield);
}
