//! Independent closed-subset PT scenarios frozen in pt-layer-scenarios.md.
//! Native observations do not qualify full layers, cards, durations or reentry.
use spellwake_engine::program::{BodyTarget, ExecutableBody, SpellTargetFilter};
#[path = "support/damage.rs"]
pub mod damage_support;
use spellwake_contracts::RulesInteger;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    characteristics::{
        ContinuousEffectId, EffectInstanceKey, PowerToughness, PowerToughnessCounters,
        PowerToughnessEvaluation, ResolvedPartSpec,
    },
    effects::{CopyTargetChoice, EffectError, EffectWindow, Instruction, Program, SeedCard},
    game::{CandidateBase, GameCommand, GameContext, GameError, GameId, GameInput, GameState},
    mana::{FloatingMana, ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityError, PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackWindowError, WindowPhase},
    targets::TargetRef,
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const FIRST: u64 = 100;
const SECOND: u64 = 101;
const HUGE: &str = "900719925474099312345678901234567890";
const HUGE_MINUS_ONE: &str = "900719925474099312345678901234567889";
const HUGE_PLUS_ONE: &str = "900719925474099312345678901234567891";

fn exact(value: &str) -> RulesInteger {
    value.to_owned().try_into().unwrap()
}

fn pt(power: i64, toughness: i64) -> PowerToughness {
    PowerToughness {
        power: power.into(),
        toughness: toughness.into(),
    }
}

fn counts(plus: i64, minus: i64) -> PowerToughnessCounters {
    PowerToughnessCounters {
        plus_one: plus.into(),
        minus_one: minus.into(),
    }
}

fn continuous(operation: ResolvedPartSpec) -> Instruction {
    Instruction::CreateContinuousEffect {
        parts: spellwake_engine::characteristics::ResolvedPartsSpec(vec![operation]),
        duration: spellwake_engine::characteristics::ResolvedDuration::NoStatedDuration,
    }
}

fn counters(plus: i64, minus: i64) -> Instruction {
    Instruction::PutPowerToughnessCounters {
        counters: counts(plus, minus),
    }
}

fn context() -> GameContext {
    GameContext::new(
        "pt-native-tests".into(),
        "rules-20260925".into(),
        "synthetic-definitions".into(),
        "literal-pt-programs".into(),
        "individual-turn".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}

fn instant(id: u64, owner: SeatId, instruction: ExecutableBody) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::instant(ManaCost::one(ManaType::Blue), instruction),
    }
}

fn creature(id: u64, owner: SeatId, controller: SeatId, values: PowerToughness) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Battlefield { controller },
        program: Program::creature(
            None,
            values.power,
            values.toughness,
            std::collections::BTreeSet::new(),
        ),
    }
}

fn seeds(
    values: PowerToughness,
    instructions: Vec<(u64, SeatId, ExecutableBody)>,
) -> Vec<SeedCard> {
    let mut cards = vec![
        creature(FIRST, B, B, values.clone()),
        creature(SECOND, A, A, values),
    ];
    cards.extend(
        instructions
            .into_iter()
            .map(|(id, owner, instruction)| instant(id, owner, instruction)),
    );
    cards
}

fn initialize(cards: Vec<SeedCard>, mana: Vec<FloatingMana>) -> GameState {
    GameState::initialize(GameId(51), context(), {
        // Fixture premise: seeded BF timestamps follow this authored card sequence.
        let initial_battlefield_order = cards
            .iter()
            .filter(|card| {
                matches!(
                    card.destination,
                    spellwake_engine::object_identity::ObjectDestination::Battlefield { .. }
                )
            })
            .map(|card| card.card)
            .collect();
        GameSeed::with_uniform_life(
            vec![A, B],
            A,
            cards,
            initial_battlefield_order,
            mana,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    })
    .unwrap()
    .into_state()
}

fn setup(values: PowerToughness, instructions: Vec<(u64, SeatId, ExecutableBody)>) -> GameState {
    initialize(
        seeds(values, instructions),
        [A, B]
            .into_iter()
            .map(|player| FloatingMana {
                player,
                mana_type: ManaType::Blue,
                amount: 100.into(),
            })
            .collect(),
    )
}

fn object(game: &GameState, card: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}

fn zone(game: &GameState, card: u64) -> ObjectZone {
    game.execution()
        .window()
        .cards()
        .get(object(game, card))
        .unwrap()
        .zone()
}

fn evaluation(game: &GameState, card: u64) -> PowerToughnessEvaluation {
    game.execution()
        .power_toughness(object(game, card))
        .unwrap()
        .unwrap()
}

fn input(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    }
}

fn advance(game: &GameState, actor: SeatId, command: GameCommand) -> GameState {
    let before = game.clone();
    let next = game.advance(input(game, actor, command)).unwrap();
    damage_support::assert_transition_damage(game, &next);
    assert_eq!(next.base(), &CandidateBase::Advance(game.basis().clone()));
    assert_eq!(next.state().basis().revision, game.basis().revision + 1);
    assert_eq!(next.state().basis().game, game.basis().game);
    assert_eq!(next.state().basis().context, game.basis().context);
    assert_eq!(next.state().basis().epoch, game.basis().epoch);
    assert_eq!(game, &before);
    next.into_state()
}

fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected settled priority")
    };
    holder
}

fn holding(game: GameState, actor: SeatId) -> GameState {
    if holder(&game) == actor {
        game
    } else {
        let game = advance(&game, holder(&game), GameCommand::Pass);
        assert_eq!(holder(&game), actor);
        game
    }
}

fn cast(game: GameState, actor: SeatId, card: u64, target: TargetRef) -> (GameState, StackEntryId) {
    let game = holding(game, actor);
    let game = advance(
        &game,
        actor,
        GameCommand::Cast {
            spell: object(&game, card),
            target,
        },
    );
    let entry = game.execution().window().entries().last().unwrap().id();
    (game, entry)
}

fn pass_round(game: GameState) -> GameState {
    let game = advance(&game, holder(&game), GameCommand::Pass);
    advance(&game, holder(&game), GameCommand::Pass)
}

fn apply(game: GameState, card: u64, target: ObjectId) -> (GameState, StackEntryId) {
    let (game, source) = cast(game, A, card, TargetRef::Object(target));
    let game = pass_round(game);
    assert!(game.pending_decision().is_none());
    (game, source)
}

fn effect_for(state: &EffectWindow, object: ObjectId, source: StackEntryId) -> ContinuousEffectId {
    state
        .power_toughness(object)
        .unwrap()
        .unwrap()
        .applied_effects
        .into_iter()
        .map(|key| {
            let EffectInstanceKey::Resolved(id) = key else {
                panic!("this fixture has only resolved effects")
            };
            id
        })
        .find(|id| state.continuous_effect(*id).unwrap().creator == source)
        .unwrap()
}

fn reject(game: &GameState, request: GameInput, error: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(request), Err(error));
    assert_eq!(game, &saved);
}

/// Preserve the real executor's interval that GameState intentionally drives past.
/// The last pass and resolution are ordinary kernel operations, never registry edits.
fn resolve_before_checks(game: GameState) -> EffectWindow {
    let game = advance(&game, holder(&game), GameCommand::Pass);
    let execution = game
        .execution()
        .pass(game.execution().revision(), holder(&game))
        .unwrap();
    let (execution, spellwake_engine::effects::RuleResult::Complete { .. }, _) = execution
        .begin_resolution(execution.revision())
        .unwrap()
        .into_parts()
    else {
        panic!("expected a literal instruction without a resolution choice")
    };
    assert!(matches!(
        execution.window().phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
    ));
    execution
}

#[test]
fn untouched_queries_are_exact_read_only_and_have_no_runtime_effects() {
    let game = setup(pt(2, 3), vec![]);
    let before = game.clone();
    let expected = PowerToughnessEvaluation {
        object: object(&game, FIRST),
        copiable: Some(pt(2, 3).into()),
        base: pt(2, 3),
        derived: pt(2, 3),
        applied_effects: vec![],
    };
    assert_eq!(evaluation(&game, FIRST), expected);
    assert_eq!(evaluation(&game.clone(), FIRST), expected);
    assert_eq!(
        game.execution()
            .power_toughness_counters(object(&game, FIRST)),
        Some(counts(0, 0))
    );
    assert_eq!(game, before);
}

#[test]
fn setting_precedes_modification_in_both_creation_orders() {
    for order in [[1, 2], [2, 1]] {
        let game = setup(
            pt(2, 3),
            vec![
                (
                    1,
                    A,
                    ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![continuous(ResolvedPartSpec::ModifyPT(pt(4, 5)))],
                    ),
                ),
                (
                    2,
                    A,
                    ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![continuous(ResolvedPartSpec::SetPT(pt(1, 1)))],
                    ),
                ),
            ],
        );
        let target = object(&game, FIRST);
        let (game, first_source) = apply(game, order[0], target);
        let (game, second_source) = apply(game, order[1], target);
        let value = evaluation(&game, FIRST);
        assert_eq!(value.copiable, Some(pt(2, 3).into()));
        assert_eq!(value.base, pt(1, 1));
        assert_eq!(value.derived, pt(5, 6));
        let first = effect_for(game.execution(), target, first_source);
        let second = effect_for(game.execution(), target, second_source);
        assert_eq!(
            value.applied_effects,
            if order[0] == 1 {
                vec![
                    EffectInstanceKey::Resolved(second),
                    EffectInstanceKey::Resolved(first),
                ]
            } else {
                vec![
                    EffectInstanceKey::Resolved(first),
                    EffectInstanceKey::Resolved(second),
                ]
            }
        );
    }
}

#[test]
fn setters_receive_timestamps_when_resolved_instead_of_when_cast() {
    let game = setup(
        pt(2, 3),
        vec![
            (
                1,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::SetPT(pt(5, 7)))],
                ),
            ),
            (
                2,
                B,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::SetPT(pt(3, 4)))],
                ),
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, original) = cast(game, A, 1, TargetRef::Object(target));
    let (game, response) = cast(game, B, 2, TargetRef::Object(target));
    assert_eq!(evaluation(&game, FIRST).derived, pt(2, 3));
    assert!(evaluation(&game, FIRST).applied_effects.is_empty());
    let game = pass_round(game);
    assert_eq!(evaluation(&game, FIRST).derived, pt(3, 4));
    let response_effect = effect_for(game.execution(), target, response);
    let game = pass_round(game);
    assert_eq!(evaluation(&game, FIRST).base, pt(5, 7));
    assert_eq!(evaluation(&game, FIRST).derived, pt(5, 7));
    let original_effect = effect_for(game.execution(), target, original);
    assert_ne!(original_effect, response_effect);
    assert!(
        game.execution()
            .continuous_effect(response_effect)
            .unwrap()
            .timestamp
            < game
                .execution()
                .continuous_effect(original_effect)
                .unwrap()
                .timestamp
    );
    assert_eq!(
        evaluation(&game, FIRST).applied_effects,
        vec![
            EffectInstanceKey::Resolved(response_effect),
            EffectInstanceKey::Resolved(original_effect),
        ]
    );
    assert_eq!(zone(&game, 1), ObjectZone::Graveyard(A));
    assert_eq!(zone(&game, 2), ObjectZone::Graveyard(B));
}

#[test]
fn newer_asymmetric_modifiers_still_apply_before_an_older_switch() {
    let game = setup(
        pt(1, 3),
        vec![
            (
                1,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::ModifyPT(pt(0, 1)))],
                ),
            ),
            (
                2,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::SwitchPT)],
                ),
            ),
            (
                3,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::ModifyPT(pt(5, 0)))],
                ),
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, first) = apply(game, 1, target);
    let (game, switch) = apply(game, 2, target);
    assert_eq!(evaluation(&game, FIRST).derived, pt(4, 1));
    let (game, last) = apply(game, 3, target);
    assert_eq!(evaluation(&game, FIRST).derived, pt(4, 6));
    assert_eq!(evaluation(&game, FIRST).base, pt(1, 3));
    assert_eq!(
        evaluation(&game, FIRST).applied_effects,
        vec![
            EffectInstanceKey::Resolved(effect_for(game.execution(), target, first)),
            EffectInstanceKey::Resolved(effect_for(game.execution(), target, last)),
            EffectInstanceKey::Resolved(effect_for(game.execution(), target, switch)),
        ]
    );
}

#[test]
fn each_switch_applies_once_and_queries_do_not_toggle_canonical_state() {
    let game = setup(
        pt(1, 3),
        vec![
            (
                1,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::ModifyPT(pt(0, 1)))],
                ),
            ),
            (
                2,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::SwitchPT)],
                ),
            ),
            (
                3,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::SwitchPT)],
                ),
            ),
            (
                4,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::SwitchPT)],
                ),
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, _) = apply(game, 1, target);
    let (game, _) = apply(game, 2, target);
    let (game, _) = apply(game, 3, target);
    assert_eq!(evaluation(&game, FIRST).derived, pt(1, 4));
    let saved = game.clone();
    assert_eq!(evaluation(&game, FIRST).derived, pt(1, 4));
    assert_eq!(game, saved);
    let (game, _) = apply(game, 4, target);
    assert_eq!(evaluation(&game, FIRST).derived, pt(4, 1));
    assert_eq!(evaluation(&game, FIRST).applied_effects.len(), 4);
}

#[test]
fn signed_modifiers_accumulate_without_changing_defined_or_base_values() {
    for order in [[1, 2], [2, 1]] {
        let game = setup(
            pt(2, 3),
            vec![
                (
                    1,
                    A,
                    ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![continuous(ResolvedPartSpec::ModifyPT(pt(4, -1)))],
                    ),
                ),
                (
                    2,
                    A,
                    ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![continuous(ResolvedPartSpec::ModifyPT(pt(-3, 5)))],
                    ),
                ),
            ],
        );
        let target = object(&game, FIRST);
        let (game, _) = apply(game, order[0], target);
        let (game, _) = apply(game, order[1], target);
        let value = evaluation(&game, FIRST);
        assert_eq!(value.derived, pt(3, 7));
        assert_eq!(value.base, pt(2, 3));
        assert_eq!(value.copiable, Some(pt(2, 3).into()));
        assert_eq!(value.applied_effects.len(), 2);
    }
}

#[test]
fn defined_base_counters_and_final_switched_values_remain_distinct() {
    let game = setup(
        pt(2, 3),
        vec![
            (
                1,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::SwitchPT)],
                ),
            ),
            (
                2,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::ModifyPT(pt(2, -1)))],
                ),
            ),
            (
                3,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::SetPT(pt(3, 8)))],
                ),
            ),
            (
                4,
                A,
                ExecutableBody::new(BodyTarget::SingleCreature, vec![counters(2, 1)]),
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, switch) = apply(game, 1, target);
    let (game, modifier) = apply(game, 2, target);
    let (game, setter) = apply(game, 3, target);
    let (game, _) = apply(game, 4, target);
    let value = evaluation(&game, FIRST);
    assert_eq!(value.copiable, Some(pt(2, 3).into()));
    assert_eq!(value.base, pt(3, 8));
    assert_eq!(value.derived, pt(8, 6));
    assert_eq!(
        game.execution().copiable_power_toughness(target),
        Some(pt(2, 3).into())
    );
    assert_eq!(
        game.execution().power_toughness_counters(target),
        Some(counts(1, 0))
    );
    assert_eq!(
        value.applied_effects,
        vec![
            EffectInstanceKey::Resolved(effect_for(game.execution(), target, setter)),
            EffectInstanceKey::Resolved(effect_for(game.execution(), target, modifier)),
            EffectInstanceKey::Resolved(effect_for(game.execution(), target, switch)),
        ]
    );
    let visible = game.execution().creature(target).unwrap().unwrap();
    assert_eq!(visible.power, 8.into());
    assert_eq!(visible.toughness, 6.into());
    assert_eq!(visible.damage, 0.into());
    assert_eq!(evaluation(&game, SECOND).derived, pt(2, 3));
}

#[test]
fn opposite_counters_are_physical_until_the_actual_sba_boundary() {
    let game = setup(
        pt(2, 3),
        vec![(
            1,
            A,
            ExecutableBody::new(BodyTarget::SingleCreature, vec![counters(3, 1)]),
        )],
    );
    let target = object(&game, FIRST);
    let (game, _) = cast(game, A, 1, TargetRef::Object(target));
    let execution = resolve_before_checks(game);
    let saved = execution.clone();
    assert_eq!(
        execution.power_toughness_counters(target),
        Some(counts(3, 1))
    );
    assert_eq!(
        execution.power_toughness(target).unwrap().unwrap().derived,
        pt(4, 5)
    );
    assert!(execution.pass(execution.revision(), A).is_err());
    let next = execution.settle_checks(execution.revision()).unwrap();
    assert_eq!(execution, saved);
    assert_eq!(next.power_toughness_counters(target), Some(counts(2, 0)));
    assert_eq!(
        next.power_toughness(target).unwrap().unwrap().derived,
        pt(4, 5)
    );
    assert!(matches!(
        next.window().phase(),
        WindowPhase::Priority(PriorityPhase::Ready { holder: A })
    ));
}

#[test]
fn huge_counter_batches_cancel_exactly_without_narrowing_counts() {
    let game = setup(
        pt(1, 2),
        vec![(
            1,
            A,
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::PutPowerToughnessCounters {
                    counters: PowerToughnessCounters {
                        plus_one: exact(HUGE),
                        minus_one: exact(HUGE_MINUS_ONE),
                    },
                }],
            ),
        )],
    );
    let target = object(&game, FIRST);
    let (game, _) = cast(game, A, 1, TargetRef::Object(target));
    let execution = resolve_before_checks(game.clone());
    assert_eq!(
        execution.power_toughness_counters(target),
        Some(PowerToughnessCounters {
            plus_one: exact(HUGE),
            minus_one: exact(HUGE_MINUS_ONE),
        })
    );
    assert_eq!(
        execution.power_toughness(target).unwrap().unwrap().derived,
        pt(2, 3)
    );
    let game = pass_round(game);
    assert_eq!(
        game.execution().power_toughness_counters(target),
        Some(counts(1, 0))
    );
    assert_eq!(evaluation(&game, FIRST).derived, pt(2, 3));
}

#[test]
fn repeated_counter_placements_accumulate_instead_of_overwriting_a_kind() {
    let game = setup(
        pt(2, 3),
        vec![
            (
                1,
                A,
                ExecutableBody::new(BodyTarget::SingleCreature, vec![counters(2, 0)]),
            ),
            (
                2,
                A,
                ExecutableBody::new(BodyTarget::SingleCreature, vec![counters(3, 0)]),
            ),
            (
                3,
                A,
                ExecutableBody::new(BodyTarget::SingleCreature, vec![counters(0, 1)]),
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, _) = apply(game, 1, target);
    let (game, _) = apply(game, 2, target);
    assert_eq!(
        game.execution().power_toughness_counters(target),
        Some(counts(5, 0))
    );
    let (game, _) = apply(game, 3, target);
    assert_eq!(
        game.execution().power_toughness_counters(target),
        Some(counts(4, 0))
    );
    assert_eq!(evaluation(&game, FIRST).derived, pt(6, 7));
}

#[test]
fn huge_literal_values_survive_setting_modification_and_switching_exactly() {
    let game = setup(
        pt(2, 3),
        vec![
            (
                1,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::SetPT(PowerToughness {
                        power: exact(HUGE),
                        toughness: exact(HUGE_PLUS_ONE),
                    }))],
                ),
            ),
            (
                2,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::ModifyPT(pt(1, -1)))],
                ),
            ),
            (
                3,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::SwitchPT)],
                ),
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, _) = apply(game, 1, target);
    let (game, _) = apply(game, 2, target);
    let (game, _) = apply(game, 3, target);
    let expected = PowerToughness {
        power: exact(HUGE),
        toughness: exact(HUGE_PLUS_ONE),
    };
    assert_eq!(evaluation(&game, FIRST).base, expected);
    assert_eq!(evaluation(&game, FIRST).derived, expected);
    assert_eq!(evaluation(&game, FIRST).copiable, Some(pt(2, 3).into()));
}

#[test]
fn negative_power_and_pre_check_negative_toughness_are_never_clamped() {
    let game = setup(
        pt(2, 3),
        vec![
            (
                1,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::SetPT(PowerToughness {
                        power: exact("-900719925474099312345678901234567890"),
                        toughness: 5.into(),
                    }))],
                ),
            ),
            (
                2,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::SwitchPT)],
                ),
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, _) = apply(game, 1, target);
    assert_eq!(
        evaluation(&game, FIRST).derived.power,
        exact("-900719925474099312345678901234567890")
    );
    let (game, _) = cast(game, A, 2, TargetRef::Object(target));
    let execution = resolve_before_checks(game.clone());
    assert_eq!(
        execution.power_toughness(target).unwrap().unwrap().derived,
        PowerToughness {
            power: 5.into(),
            toughness: exact("-900719925474099312345678901234567890"),
        }
    );
    let game = pass_round(game);
    assert_eq!(zone(&game, FIRST), ObjectZone::Graveyard(B));
    assert!(game.execution().creature(target).unwrap().is_none());
}

#[test]
fn damage_checks_use_current_derived_toughness_and_damage_stays_separate() {
    for (defined, operation, damage, expected, dies) in [
        (
            pt(2, 5),
            ResolvedPartSpec::ModifyPT(pt(0, -3)),
            2,
            pt(2, 2),
            true,
        ),
        (
            pt(2, 2),
            ResolvedPartSpec::ModifyPT(pt(0, 2)),
            3,
            pt(2, 4),
            false,
        ),
        (
            pt(2, 2),
            ResolvedPartSpec::ModifyPT(pt(0, 2)),
            4,
            pt(2, 4),
            true,
        ),
        (pt(1, 5), ResolvedPartSpec::SwitchPT, 1, pt(5, 1), true),
    ] {
        let game = setup(
            defined,
            vec![
                (
                    1,
                    A,
                    ExecutableBody::new(BodyTarget::SingleCreature, vec![continuous(operation)]),
                ),
                (
                    2,
                    A,
                    ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::Damage {
                            amount: damage.into(),
                        }],
                    ),
                ),
            ],
        );
        let target = object(&game, FIRST);
        let (game, _) = apply(game, 1, target);
        let (game, _) = cast(game, A, 2, TargetRef::Object(target));
        let execution = resolve_before_checks(game.clone());
        assert_eq!(
            execution.power_toughness(target).unwrap().unwrap().derived,
            expected
        );
        assert_eq!(
            execution.creature(target).unwrap().unwrap().damage,
            damage.into()
        );
        let game = pass_round(game);
        if dies {
            assert_eq!(zone(&game, FIRST), ObjectZone::Graveyard(B));
            assert_ne!(object(&game, FIRST), target);
            assert!(game.execution().creature(target).unwrap().is_none());
        } else {
            assert_eq!(zone(&game, FIRST), ObjectZone::Battlefield);
            assert_eq!(evaluation(&game, FIRST).derived, expected);
            assert_eq!(
                game.execution().creature(target).unwrap().unwrap().damage,
                damage.into()
            );
        }
        assert_eq!(holder(&game), A);
    }
}

#[test]
fn huge_derived_toughness_uses_exact_lethal_damage_equality() {
    for (toughness, dies) in [(HUGE_PLUS_ONE, false), (HUGE, true)] {
        let game = setup(
            pt(2, 3),
            vec![
                (
                    1,
                    A,
                    ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![continuous(ResolvedPartSpec::SetPT(PowerToughness {
                            power: 2.into(),
                            toughness: exact(toughness),
                        }))],
                    ),
                ),
                (
                    2,
                    A,
                    ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::Damage {
                            amount: exact(HUGE),
                        }],
                    ),
                ),
            ],
        );
        let target = object(&game, FIRST);
        let (game, _) = apply(game, 1, target);
        let (game, _) = apply(game, 2, target);
        if dies {
            assert_eq!(zone(&game, FIRST), ObjectZone::Graveyard(B));
        } else {
            assert_eq!(zone(&game, FIRST), ObjectZone::Battlefield);
            assert_eq!(
                evaluation(&game, FIRST).derived.toughness,
                exact(HUGE_PLUS_ONE)
            );
            assert_eq!(
                game.execution().creature(target).unwrap().unwrap().damage,
                exact(HUGE)
            );
            assert_eq!(evaluation(&game, FIRST).copiable, Some(pt(2, 3).into()));
        }
    }
}

#[test]
fn zero_or_negative_derived_toughness_moves_to_owner_before_priority() {
    for operation in [
        ResolvedPartSpec::SetPT(pt(2, 0)),
        ResolvedPartSpec::ModifyPT(pt(0, -4)),
    ] {
        let cards = vec![
            creature(FIRST, B, A, pt(2, 3)),
            instant(
                1,
                A,
                ExecutableBody::new(BodyTarget::SingleCreature, vec![continuous(operation)]),
            ),
        ];
        let game = initialize(
            cards,
            vec![FloatingMana {
                player: A,
                mana_type: ManaType::Blue,
                amount: 1.into(),
            }],
        );
        let target = object(&game, FIRST);
        let (game, _) = cast(game, A, 1, TargetRef::Object(target));
        let execution = resolve_before_checks(game.clone());
        assert!(
            execution
                .power_toughness(target)
                .unwrap()
                .unwrap()
                .derived
                .toughness
                <= 0.into()
        );
        assert_eq!(
            execution.creature(target).unwrap().unwrap().damage,
            0.into()
        );
        let game = pass_round(game);
        assert_eq!(zone(&game, FIRST), ObjectZone::Graveyard(B));
        assert_eq!(zone(&game, 1), ObjectZone::Graveyard(A));
        assert_eq!(holder(&game), A);
    }
}

#[test]
fn lethal_death_and_opposite_counter_cancellation_share_pre_check_state() {
    let game = setup(
        pt(2, 2),
        vec![
            (
                1,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::ModifyPT(pt(1, 0)))],
                ),
            ),
            (
                2,
                A,
                ExecutableBody::new(BodyTarget::SingleCreature, vec![counters(1, 0)]),
            ),
            (
                3,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Damage { amount: 2.into() }],
                ),
            ),
            (
                4,
                A,
                ExecutableBody::new(BodyTarget::SingleCreature, vec![counters(0, 1)]),
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, source) = apply(game, 1, target);
    let id = effect_for(game.execution(), target, source);
    let (game, _) = apply(game, 2, target);
    let (game, _) = apply(game, 3, target);
    assert_eq!(evaluation(&game, FIRST).derived, pt(4, 3));
    let (game, _) = cast(game, A, 4, TargetRef::Object(target));
    let execution = resolve_before_checks(game.clone());
    assert_eq!(
        execution.power_toughness_counters(target),
        Some(counts(1, 1))
    );
    assert_eq!(
        execution.power_toughness(target).unwrap().unwrap().derived,
        pt(3, 2)
    );
    assert_eq!(
        execution.creature(target).unwrap().unwrap().damage,
        2.into()
    );
    let game = pass_round(game);
    assert_eq!(zone(&game, FIRST), ObjectZone::Graveyard(B));
    assert!(game.execution().power_toughness_counters(target).is_none());
    assert!(game.execution().continuous_effect(id).is_none());
    assert_eq!(evaluation(&game, SECOND).derived, pt(2, 2));
}

#[test]
fn bounce_prunes_effects_counters_and_damage_without_modifying_copiable_definition() {
    let game = setup(
        pt(2, 3),
        vec![
            (
                1,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![continuous(ResolvedPartSpec::ModifyPT(pt(2, 1)))],
                ),
            ),
            (
                2,
                A,
                ExecutableBody::new(BodyTarget::SingleCreature, vec![counters(3, 0)]),
            ),
            (
                3,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Damage { amount: 1.into() }],
                ),
            ),
            (
                4,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::ReturnToOwnersHand],
                ),
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, source) = apply(game, 1, target);
    let id = effect_for(game.execution(), target, source);
    let (game, _) = apply(game, 2, target);
    let (game, _) = apply(game, 3, target);
    assert_eq!(evaluation(&game, FIRST).derived, pt(7, 7));
    assert_eq!(
        game.execution().creature(target).unwrap().unwrap().damage,
        1.into()
    );
    let (game, _) = apply(game, 4, target);
    let hand = object(&game, FIRST);
    assert_ne!(hand, target);
    assert_eq!(zone(&game, FIRST), ObjectZone::Hand(B));
    assert!(game.execution().power_toughness(target).unwrap().is_none());
    assert!(game.execution().copiable_power_toughness(target).is_none());
    assert_eq!(
        game.execution()
            .power_toughness(hand)
            .unwrap()
            .map(|value| value.derived),
        Some(pt(2, 3))
    );
    assert!(game.execution().power_toughness_counters(hand).is_none());
    assert!(game.execution().continuous_effect(id).is_none());
    assert_eq!(
        game.execution().copiable_power_toughness(hand),
        Some(pt(2, 3).into())
    );
    assert_eq!(evaluation(&game, SECOND).derived, pt(2, 3));
    // Reentry is not a supported executor action; do not splice in a second owner.
}

#[test]
fn departed_targets_create_neither_continuous_effects_nor_counters() {
    for instruction in [
        continuous(ResolvedPartSpec::ModifyPT(pt(3, 3))),
        counters(3, 1),
    ] {
        let game = setup(
            pt(2, 3),
            vec![
                (
                    1,
                    A,
                    ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction]),
                ),
                (
                    2,
                    B,
                    ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::ReturnToOwnersHand],
                    ),
                ),
            ],
        );
        let old = object(&game, FIRST);
        let (game, _) = cast(game, A, 1, TargetRef::Object(old));
        let (game, _) = cast(game, B, 2, TargetRef::Object(old));
        let game = pass_round(game);
        let hand = object(&game, FIRST);
        assert_ne!(hand, old);
        let game = pass_round(game);
        assert_eq!(zone(&game, FIRST), ObjectZone::Hand(B));
        assert_eq!(zone(&game, 1), ObjectZone::Graveyard(A));
        assert!(game.execution().window().entries().is_empty());
        assert!(game.execution().power_toughness(old).unwrap().is_none());
        assert!(game.execution().power_toughness_counters(hand).is_none());
        assert_eq!(game.execution().mana(A, ManaType::Blue), 99.into());
        assert!(evaluation(&game, SECOND).applied_effects.is_empty());
        assert_eq!(evaluation(&game, SECOND).derived, pt(2, 3));
    }
}

fn pending_copy(instruction: Instruction) -> (GameState, StackEntryId) {
    let game = setup(
        pt(2, 3),
        vec![
            (
                1,
                A,
                ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction]),
            ),
            (
                2,
                B,
                ExecutableBody::new(
                    BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
                    vec![Instruction::CopySpell],
                ),
            ),
            (
                3,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
                    vec![Instruction::CounterSpell],
                ),
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, original) = cast(game, A, 1, TargetRef::Object(target));
    let (game, _) = cast(game, B, 2, TargetRef::Stack(original));
    let game = pass_round(game);
    assert!(game.pending_decision().is_some());
    (game, original)
}

fn reply_copy(game: &GameState, target: TargetRef) -> GameState {
    advance(
        game,
        B,
        GameCommand::Reply {
            decision: game.pending_decision().unwrap().id(),
            choice: (CopyTargetChoice::Change(target)).into(),
        },
    )
}

#[test]
fn copied_modifiers_get_independent_effect_identity_target_and_source_cause() {
    let (pending, original) = pending_copy(continuous(ResolvedPartSpec::ModifyPT(pt(2, 1))));
    let decision = pending.pending_decision().unwrap().id();
    for command in [
        GameCommand::Pass,
        GameCommand::Cast {
            spell: object(&pending, 3),
            target: TargetRef::Stack(original),
        },
    ] {
        reject(
            &pending,
            input(&pending, A, command),
            GameError::DecisionRequired,
        );
    }
    reject(
        &pending,
        input(
            &pending,
            B,
            GameCommand::Reply {
                decision,
                choice: (CopyTargetChoice::Change(TargetRef::Player(A))).into(),
            },
        ),
        GameError::Effect(EffectError::IllegalTarget),
    );
    let first = object(&pending, FIRST);
    let second = object(&pending, SECOND);
    let game = reply_copy(&pending, TargetRef::Object(second));
    let copy = *game
        .execution()
        .window()
        .spell_copies()
        .keys()
        .next()
        .unwrap();
    assert_ne!(copy, original);
    assert_eq!(game.execution().mana(B, ManaType::Blue), 99.into());
    let (countered, _) = cast(game.clone(), A, 3, TargetRef::Stack(original));
    let countered = pass_round(pass_round(countered));
    assert_eq!(evaluation(&countered, FIRST).derived, pt(2, 3));
    assert_eq!(evaluation(&countered, SECOND).derived, pt(4, 4));
    let retained_copy_effect = effect_for(countered.execution(), second, copy);
    assert_eq!(
        countered
            .execution()
            .continuous_effect(retained_copy_effect)
            .unwrap()
            .creator,
        copy
    );

    let game = pass_round(game);
    let copy_effect = effect_for(game.execution(), second, copy);
    assert_eq!(evaluation(&game, FIRST).derived, pt(2, 3));
    assert_eq!(evaluation(&game, SECOND).derived, pt(4, 4));
    let game = pass_round(game);
    let original_effect = effect_for(game.execution(), first, original);
    assert_ne!(copy_effect, original_effect);
    let copy_record = game.execution().continuous_effect(copy_effect).unwrap();
    let original_record = game.execution().continuous_effect(original_effect).unwrap();
    assert_eq!(copy_record.creator, copy);
    assert_eq!(
        copy_record.affected,
        std::collections::BTreeSet::from([second])
    );
    assert_eq!(original_record.creator, original);
    assert_eq!(
        original_record.affected,
        std::collections::BTreeSet::from([first])
    );
    assert!(copy_record.timestamp < original_record.timestamp);
    assert_eq!(evaluation(&game, FIRST).derived, pt(4, 4));
    assert_eq!(evaluation(&game, SECOND).derived, pt(4, 4));
    assert_eq!(zone(&game, 1), ObjectZone::Graveyard(A));
    assert_eq!(zone(&game, 2), ObjectZone::Graveyard(B));
}

#[test]
fn copied_counter_batch_retains_exact_payload_after_original_is_countered() {
    let (game, original) = pending_copy(Instruction::PutPowerToughnessCounters {
        counters: PowerToughnessCounters {
            plus_one: exact(HUGE),
            minus_one: exact(HUGE_MINUS_ONE),
        },
    });
    let second = object(&game, SECOND);
    let game = reply_copy(&game, TargetRef::Object(second));
    let (game, _) = cast(game, A, 3, TargetRef::Stack(original));
    let game = pass_round(pass_round(game));
    assert_eq!(evaluation(&game, FIRST).derived, pt(2, 3));
    assert_eq!(evaluation(&game, SECOND).derived, pt(3, 4));
    assert_eq!(
        game.execution().power_toughness_counters(second),
        Some(counts(1, 0))
    );
    assert_eq!(game.execution().window().cards().iter().count(), 5);
    assert!(game.execution().window().spell_copies().is_empty());
}

#[test]
fn empty_or_negative_counter_batches_reject_but_one_kind_zero_field_is_valid() {
    for invalid in [counts(0, 0), counts(-1, 2), counts(2, -1)] {
        assert_eq!(
            GameState::initialize(GameId(51), context(), {
                let cards: Vec<SeedCard> = seeds(
                    pt(2, 3),
                    vec![(
                        1,
                        A,
                        ExecutableBody::new(
                            BodyTarget::SingleCreature,
                            vec![Instruction::PutPowerToughnessCounters { counters: invalid }],
                        ),
                    )],
                );
                // Fixture premise: seeded BF timestamps follow this authored card sequence.
                let initial_battlefield_order = cards
                    .iter()
                    .filter(|card| {
                        matches!(
                            card.destination,
                            spellwake_engine::object_identity::ObjectDestination::Battlefield { .. }
                        )
                    })
                    .map(|card| card.card)
                    .collect();
                GameSeed::with_uniform_life(
                    vec![A, B],
                    A,
                    cards,
                    initial_battlefield_order,
                    vec![],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    test_vocabulary(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                )
            }),
            Err(GameError::Effect(EffectError::InvalidSetup))
        );
    }
    for (placement, expected) in [(counters(1, 0), pt(3, 4)), (counters(0, 1), pt(1, 2))] {
        let game = setup(
            pt(2, 3),
            vec![(
                1,
                A,
                ExecutableBody::new(BodyTarget::SingleCreature, vec![placement]),
            )],
        );
        let target = object(&game, FIRST);
        let (game, _) = apply(game, 1, target);
        assert_eq!(evaluation(&game, FIRST).derived, expected);
    }
}

#[test]
fn invalid_pt_and_counter_casts_preserve_the_entire_last_accepted_state() {
    for instruction in [continuous(ResolvedPartSpec::SwitchPT), counters(1, 0)] {
        let game = setup(
            pt(2, 3),
            vec![
                (
                    1,
                    A,
                    ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction.clone()]),
                ),
                (
                    2,
                    B,
                    ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction.clone()]),
                ),
            ],
        );
        let spell = object(&game, 1);
        let target = object(&game, FIRST);
        let mut stale = input(
            &game,
            A,
            GameCommand::Cast {
                spell,
                target: TargetRef::Object(target),
            },
        );
        stale.basis.revision += 1;
        reject(&game, stale, GameError::StaleRevision);
        for target in [TargetRef::Player(A), TargetRef::Object(object(&game, 2))] {
            reject(
                &game,
                input(&game, A, GameCommand::Cast { spell, target }),
                GameError::Effect(EffectError::IllegalTarget),
            );
        }
        reject(
            &game,
            input(
                &game,
                B,
                GameCommand::Cast {
                    spell: object(&game, 2),
                    target: TargetRef::Object(target),
                },
            ),
            GameError::Effect(EffectError::Window(StackWindowError::Priority(
                PriorityError::WrongPlayer,
            ))),
        );
        let without_mana = initialize(
            seeds(
                pt(2, 3),
                vec![(
                    1,
                    A,
                    ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction]),
                )],
            ),
            vec![],
        );
        reject(
            &without_mana,
            input(
                &without_mana,
                A,
                GameCommand::Cast {
                    spell: object(&without_mana, 1),
                    target: TargetRef::Object(object(&without_mana, FIRST)),
                },
            ),
            GameError::Effect(EffectError::InsufficientMana),
        );
    }
}

#[test]
fn frozen_numeric_examples_and_window_end_do_not_imply_duration_support() {
    for (instructions, expected) in [
        (
            vec![
                continuous(ResolvedPartSpec::ModifyPT(pt(3, 3))),
                continuous(ResolvedPartSpec::SetPT(pt(1, 1))),
            ],
            pt(4, 4),
        ),
        (
            vec![
                continuous(ResolvedPartSpec::SwitchPT),
                continuous(ResolvedPartSpec::ModifyPT(pt(4, 0))),
            ],
            pt(3, 5),
        ),
        (
            vec![counters(1, 0), continuous(ResolvedPartSpec::SwitchPT)],
            pt(4, 2),
        ),
    ] {
        let game = setup(
            pt(1, 3),
            vec![
                (
                    1,
                    A,
                    ExecutableBody::new(BodyTarget::SingleCreature, vec![instructions[0].clone()]),
                ),
                (
                    2,
                    A,
                    ExecutableBody::new(BodyTarget::SingleCreature, vec![instructions[1].clone()]),
                ),
            ],
        );
        let target = object(&game, FIRST);
        let (game, _) = apply(game, 1, target);
        let (game, _) = apply(game, 2, target);
        assert_eq!(evaluation(&game, FIRST).derived, expected);
        let effects = evaluation(&game, FIRST).applied_effects;
        let game = pass_round(game);
        assert_eq!(game.execution().window().phase(), WindowPhase::StepEnded);
        assert_eq!(evaluation(&game, FIRST).derived, expected);
        for key in effects {
            let EffectInstanceKey::Resolved(id) = key else {
                panic!("this fixture has only resolved effects")
            };
            assert!(game.execution().continuous_effect(id).is_some());
        }
        // The authored effects have no stated duration; StepEnded does not expire them.
    }
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("rules-20260925")
}
