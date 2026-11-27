#![allow(dead_code, unused_imports)]
//! Independent bounded activation acceptance, frozen before implementation in
//! activated-abilities/reviews/native-scenarios.md. No complete-card qualification.
use spellwake_engine::program::{BodyTarget, ExecutableBody};
#[path = "support/damage.rs"]
pub mod damage_support;
use spellwake_contracts::RulesInteger;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        EffectInstanceKey, PowerToughness, PowerToughnessCounters, ResolvedPartSpec,
    },
    damage::{
        DamageEffectCreation, DamageEffectId, DamageJournal, DamageOutcome, DamageRecipient,
        DamageSource, DamageSourceId,
    },
    effects::{CopyTargetChoice, EffectError, EffectWindow, Instruction, Program, SeedCard},
    game::{
        CandidateBase, DecisionId, DecisionReply, GameCommand, GameContext, GameError, GameId,
        GameInput, GameState, NativeCandidate, RuleFrame,
    },
    mana::{
        FloatingMana, ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment,
        ManaPaymentError, ManaSpend, ManaType,
    },
    object_identity::{
        CardId, GameObject, GameObjects, ObjectDestination, ObjectId, ObjectIdentityError,
        ObjectZone,
    },
    priority::{PriorityError, PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackKind, StackWindowError, WindowPhase},
    targets::TargetRef,
};
use std::collections::{BTreeMap, BTreeSet};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const TARGET: u64 = 99;
const OTHER: u64 = 98;

fn context() -> GameContext {
    GameContext::new(
        "release".into(),
        "rules".into(),
        "catalogue".into(),
        "programs".into(),
        "activations".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}
fn exact(value: &str) -> RulesInteger {
    value.to_owned().try_into().unwrap()
}
fn damage(amount: i64) -> Instruction {
    spellwake_engine::program::Instruction::DealDamage(Box::new(
        spellwake_engine::toolbox::DealDamageSpec {
            source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
            recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
            amount: spellwake_engine::program::DamageAmountSpec::Expr(
                spellwake_engine::program::ValueExpr::Literal(amount.into()),
            ),
            exile_if_dies: false,
        },
    ))
}
fn shield(amount: i64) -> Instruction {
    Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec {
        selector: None,
        payload: spellwake_engine::toolbox::ContinuousPayload::TargetPreventionShield {
            amount: amount.into(),
        },
        duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
        condition: None,
        one_shot: true,
        as_emblem: None,
        emblem_label: None,
    }))
}
fn multiplier(factor: i64) -> Instruction {
    Instruction::ApplyContinuous(Box::new(
        spellwake_engine::toolbox::ApplyContinuousSpec::damage_multiplier(factor.into()),
    ))
}
fn pt(power: i64, toughness: i64) -> PowerToughness {
    PowerToughness {
        power: power.into(),
        toughness: toughness.into(),
    }
}
fn ability(
    id: u64,
    source_cost: SourceCost,
    mana_cost: ManaCost,
    instructions: Vec<Instruction>,
    body_target: BodyTarget,
) -> ActivatedAbility {
    ActivatedAbility {
        id: ActivatedAbilityId(id),
        mana_cost,
        source_cost,
        body: ExecutableBody::new(body_target, instructions),
    }
}
fn artifact(
    card: u64,
    owner: SeatId,
    controller: SeatId,
    abilities: Vec<ActivatedAbility>,
) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination: ObjectDestination::Battlefield { controller },
        program: Program::artifact(None, abilities),
    }
}
fn instant(
    card: u64,
    owner: SeatId,
    cost: ManaCost,
    instructions: Vec<Instruction>,
    body_target: BodyTarget,
) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::instant(cost, ExecutableBody::new(body_target, instructions)),
    }
}
fn creature(card: u64, owner: SeatId, controller: SeatId, toughness: i64) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination: ObjectDestination::Battlefield { controller },
        program: Program::creature(
            None,
            2.into(),
            toughness.into(),
            std::collections::BTreeSet::new(),
        ),
    }
}
fn pools() -> Vec<FloatingMana> {
    [A, B]
        .into_iter()
        .flat_map(|player| {
            ManaType::ALL
                .into_iter()
                .enumerate()
                .map(move |(index, mana_type)| FloatingMana {
                    player,
                    mana_type,
                    amount: (index as i64 + if player == A { 20 } else { 40 }).into(),
                })
        })
        .collect()
}
fn setup(cards: Vec<SeedCard>, mana: Vec<FloatingMana>) -> GameState {
    let candidate = GameState::initialize(GameId(116), context(), {
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
    .unwrap();
    no_records(&candidate);
    candidate.into_state()
}
fn object(game: &GameState, card: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn physical(game: &GameState, card: u64) -> &GameObject {
    game.execution()
        .window()
        .cards()
        .get(object(game, card))
        .unwrap()
}
fn balances(game: &GameState) -> Vec<RulesInteger> {
    [A, B]
        .into_iter()
        .flat_map(|player| ManaType::ALL.map(|mana_type| game.execution().mana(player, mana_type)))
        .collect()
}
fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected ready priority")
    };
    holder
}
fn input(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    }
}
fn accept(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    let saved = game.clone();
    let candidate = game.advance(input(game, actor, command)).unwrap();
    damage_support::assert_transition_damage(game, &candidate);
    assert_eq!(game, &saved);
    assert_eq!(
        candidate.base(),
        &CandidateBase::Advance(game.basis().clone())
    );
    let mut basis = game.basis().clone();
    basis.revision += 1;
    assert_eq!(candidate.state().basis(), &basis);
    candidate
}
fn reject(game: &GameState, request: GameInput, error: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(request), Err(error));
    assert_eq!(game, &saved);
}
fn reject_effect(game: &GameState, actor: SeatId, command: GameCommand, error: EffectError) {
    reject(game, input(game, actor, command), GameError::Effect(error));
}
fn command(
    game: &GameState,
    card: u64,
    ability: u64,
    target: TargetRef,
    payment: Option<ManaPayment>,
) -> GameCommand {
    GameCommand::ActivateAbility {
        source: object(game, card),
        ability: ActivatedAbilityId(ability),
        target,
        payment,
    }
}
fn activate(
    game: &GameState,
    actor: SeatId,
    card: u64,
    target: TargetRef,
    payment: Option<ManaPayment>,
) -> NativeCandidate {
    accept(game, actor, command(game, card, 7, target, payment))
}
fn payment(spend: &[(ManaType, i64)]) -> ManaPayment {
    ManaPayment {
        from_lots: Default::default(),
        spend: spend
            .iter()
            .map(|&(mana_type, amount)| ManaSpend {
                mana_type,
                amount: amount.into(),
            })
            .collect(),
    }
}
fn no_records(candidate: &NativeCandidate) {
    assert!(candidate.ability_activation().is_none());
    assert!(candidate.mana_payment().is_none());
    assert!(candidate.mana_production().is_none());
}
fn pass(game: &GameState) -> NativeCandidate {
    let candidate = accept(game, holder(game), GameCommand::Pass);
    no_records(&candidate);
    candidate
}
fn resolve(game: &GameState) -> NativeCandidate {
    let first = pass(game);
    assert_eq!(first.damage_journal(), &DamageJournal::default());
    pass(first.state())
}
fn cast_spell(game: &GameState, actor: SeatId, card: u64, target: TargetRef) -> NativeCandidate {
    let game = if holder(game) == actor {
        game.clone()
    } else {
        pass(game).into_state()
    };
    let candidate = accept(
        &game,
        actor,
        GameCommand::Cast {
            spell: object(&game, card),
            target,
        },
    );
    assert!(candidate.ability_activation().is_none());
    assert!(candidate.mana_production().is_none());
    assert!(candidate.mana_payment().is_some());
    candidate
}
fn reply(game: &GameState, choice: DecisionReply) -> NativeCandidate {
    let pending = game.pending_decision().unwrap();
    let candidate = accept(
        game,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    );
    no_records(&candidate);
    candidate
}
fn check_activation(
    before: &GameState,
    candidate: &NativeCandidate,
    card: u64,
    actor: SeatId,
    source_cost: SourceCost,
) -> StackEntryId {
    let record = candidate.ability_activation().expect("current activation");
    let actual = candidate
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap();
    assert_eq!(record.entry, actual.id());
    assert_eq!(record.ability, ActivatedAbilityId(7));
    assert_eq!(record.controller, actor);
    assert_eq!(&record.source, physical(before, card));
    assert_eq!(record.source_cost, source_cost);
    assert_eq!(record.resulting_source, object(candidate.state(), card));
    assert_eq!(actual.controller(), actor);
    assert_eq!(
        actual.kind(),
        &StackKind::ActivatedAbility {
            source: physical(before, card).clone()
        }
    );
    assert_eq!(candidate.mana_payment().unwrap().source, actual.id());
    assert_eq!(candidate.mana_payment().unwrap().payer, actor);
    assert!(candidate.mana_production().is_none());
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    assert_ne!(physical(candidate.state(), card).zone(), ObjectZone::Stack);
    assert_eq!(holder(candidate.state()), actor);
    actual.id()
}

#[test]
fn huge_cost_debit_and_receipt_are_exact() {
    let huge = exact("900719925474099312345678901234567890");
    let cost = ManaCost {
        generic: huge.clone(),
        required: BTreeMap::from([(ManaType::Red, huge.clone())]),
    };
    let mut mana = pools();
    for pool in &mut mana {
        if pool.player == A && matches!(pool.mana_type, ManaType::Red | ManaType::Colourless) {
            pool.amount = huge.plus(&7.into());
        }
    }
    let game = setup(
        vec![
            artifact(
                1,
                A,
                A,
                vec![ability(
                    7,
                    SourceCost::Tap,
                    cost.clone(),
                    vec![damage(1)],
                    BodyTarget::SINGLE_CREATURE,
                )],
            ),
            creature(TARGET, B, B, 100),
        ],
        mana,
    );
    let candidate = activate(
        &game,
        A,
        1,
        TargetRef::Object(object(&game, TARGET)),
        Some(ManaPayment {
            from_lots: Default::default(),
            spend: vec![
                ManaSpend {
                    mana_type: ManaType::Colourless,
                    amount: huge.clone(),
                },
                ManaSpend {
                    mana_type: ManaType::Red,
                    amount: huge.clone(),
                },
            ],
        }),
    );
    check_activation(&game, &candidate, 1, A, SourceCost::Tap);
    assert_eq!(candidate.mana_payment().unwrap().cost, cost);
    assert_eq!(
        candidate.state().execution().mana(A, ManaType::Red),
        7.into()
    );
    assert_eq!(
        candidate.state().execution().mana(A, ManaType::Colourless),
        7.into()
    );
    assert!(
        candidate
            .mana_payment()
            .unwrap()
            .spend
            .iter()
            .all(|part| part.amount == huge)
    );
}

#[test]
fn source_cost_requires_actual_battlefield_placement() {
    for destination in [
        ObjectDestination::LibraryBottom,
        ObjectDestination::Hand,
        ObjectDestination::Graveyard,
        ObjectDestination::Exile,
        ObjectDestination::Command,
    ] {
        for source_cost in [SourceCost::Tap, SourceCost::Sacrifice] {
            let mut source = artifact(
                1,
                A,
                A,
                vec![ability(
                    7,
                    source_cost,
                    ManaCost::free(),
                    vec![damage(1)],
                    BodyTarget::SINGLE_CREATURE,
                )],
            );
            source.destination = destination;
            let game = setup(vec![source, creature(TARGET, B, B, 100)], pools());
            reject_effect(
                &game,
                A,
                command(&game, 1, 7, TargetRef::Object(object(&game, TARGET)), None),
                EffectError::Window(StackWindowError::Objects(ObjectIdentityError::NotPermanent)),
            );
        }
    }
}

#[test]
fn nonactive_controller_sacrifices_to_owners_graveyard_and_keeps_priority() {
    let game = setup(
        vec![
            artifact(
                1,
                A,
                B,
                vec![ability(
                    7,
                    SourceCost::Sacrifice,
                    ManaCost::one(ManaType::Red),
                    vec![damage(2)],
                    BodyTarget::SINGLE_CREATURE,
                )],
            ),
            creature(TARGET, B, B, 100),
        ],
        pools(),
    );
    let game = pass(&game).into_state();
    let old = object(&game, 1);
    let before = physical(&game, 1).clone();
    let candidate = activate(&game, B, 1, TargetRef::Object(object(&game, TARGET)), None);
    check_activation(&game, &candidate, 1, B, SourceCost::Sacrifice);
    assert_eq!(candidate.ability_activation().unwrap().source, before);
    let source = physical(candidate.state(), 1);
    assert_eq!(source.zone(), ObjectZone::Graveyard(A));
    assert_eq!(source.owner(), A);
    assert_eq!(source.controller(), None);
    assert_eq!(source.tapped(), None);
    assert_ne!(source.id(), old);
    assert_eq!(
        candidate.state().execution().mana(B, ManaType::Red),
        game.execution().mana(B, ManaType::Red).minus(&1.into())
    );
    assert_eq!(
        candidate.state().execution().mana(A, ManaType::Red),
        game.execution().mana(A, ManaType::Red)
    );
    let first = pass(candidate.state());
    assert_eq!(
        first
            .state()
            .execution()
            .creature(object(first.state(), TARGET))
            .unwrap()
            .unwrap()
            .damage,
        0.into()
    );
    let finished = pass(first.state());
    assert_eq!(
        finished
            .state()
            .execution()
            .creature(object(finished.state(), TARGET))
            .unwrap()
            .unwrap()
            .damage,
        2.into()
    );
    assert_eq!(physical(finished.state(), 1), source);
}

#[test]
fn tapped_source_can_be_sacrificed_and_both_frozen_entries_resolve_independently() {
    let game = setup(
        vec![
            artifact(
                1,
                B,
                A,
                vec![
                    ability(
                        7,
                        SourceCost::Tap,
                        ManaCost::one(ManaType::Blue),
                        vec![damage(1)],
                        BodyTarget::SINGLE_CREATURE,
                    ),
                    ability(
                        8,
                        SourceCost::Sacrifice,
                        ManaCost::one(ManaType::Red),
                        vec![damage(4)],
                        BodyTarget::SINGLE_CREATURE,
                    ),
                ],
            ),
            creature(TARGET, B, B, 100),
        ],
        pools(),
    );
    let target = object(&game, TARGET);
    let first = activate(&game, A, 1, TargetRef::Object(target), None);
    let first_id = check_activation(&game, &first, 1, A, SourceCost::Tap);
    let first_record = first.ability_activation().unwrap().clone();
    let second = accept(
        first.state(),
        A,
        command(first.state(), 1, 8, TargetRef::Object(target), None),
    );
    let second_record = second.ability_activation().unwrap();
    assert_eq!(second_record.ability, ActivatedAbilityId(8));
    assert_eq!(second_record.source.tapped(), Some(true));
    assert_ne!(second_record.entry, first_id);
    assert_eq!(second_record.source.id(), first_record.source.id());
    let grave = physical(second.state(), 1).clone();
    assert_eq!(grave.zone(), ObjectZone::Graveyard(B));
    assert_ne!(grave.id(), first_record.source.id());
    assert_eq!(second.state().execution().window().entries().len(), 2);
    let second_id = second_record.entry;
    let resolved_second = resolve(second.state());
    damage_support::assert_started_origin(resolved_second.damage_journal(), second_id);
    assert_eq!(
        resolved_second.damage_journal().applied[0].source,
        DamageSource::Object {
            object: second_record.source.clone(),
            controller: A,
        }
    );
    assert_eq!(resolved_second.damage_journal().applied[0].amount, 4.into());
    assert_eq!(physical(resolved_second.state(), 1), &grave);
    let resolved_first = resolve(resolved_second.state());
    damage_support::assert_started_origin(resolved_first.damage_journal(), first_id);
    assert_eq!(
        resolved_first.damage_journal().applied[0].source,
        DamageSource::Object {
            object: first_record.source.clone(),
            controller: A,
        }
    );
    assert_eq!(resolved_first.damage_journal().applied[0].amount, 1.into());
    assert_eq!(
        resolved_first
            .state()
            .execution()
            .creature(target)
            .unwrap()
            .unwrap()
            .damage,
        5.into()
    );
    assert_eq!(physical(resolved_first.state(), 1), &grave);
    assert_eq!(first_record.source.tapped(), Some(false));
    assert_eq!(
        resolved_first.state().execution().mana(A, ManaType::Blue),
        game.execution().mana(A, ManaType::Blue).minus(&1.into())
    );
    assert_eq!(
        resolved_first.state().execution().mana(A, ManaType::Red),
        game.execution().mana(A, ManaType::Red).minus(&1.into())
    );
}

#[test]
fn direct_executor_rejects_stale_checks_pending_and_closed_step() {
    let mut state = EffectWindow::new({
        let cards: Vec<SeedCard> = vec![
            artifact(
                1,
                A,
                A,
                vec![ability(
                    7,
                    SourceCost::Tap,
                    ManaCost::free(),
                    vec![damage(1)],
                    BodyTarget::SINGLE_CREATURE,
                )],
            ),
            creature(TARGET, B, B, 100),
        ];
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
            pools(),
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    })
    .unwrap();
    let source = state.window().cards().current_identity(CardId(1)).unwrap();
    let target = TargetRef::Object(
        state
            .window()
            .cards()
            .current_identity(CardId(TARGET))
            .unwrap(),
    );
    let saved = state.clone();
    assert_eq!(
        state.activate_ability(
            state.revision(),
            A,
            source,
            ActivatedAbilityId(7),
            target,
            None
        ),
        Err(EffectError::Window(StackWindowError::Priority(
            PriorityError::ChecksRequired
        )))
    );
    assert_eq!(state, saved);
    state = state.settle_checks(state.revision()).unwrap();
    let saved = state.clone();
    assert_eq!(
        state.activate_ability(0, A, source, ActivatedAbilityId(7), target, None),
        Err(EffectError::Window(StackWindowError::StaleRevision))
    );
    assert_eq!(state, saved);
    state = state.pass(state.revision(), A).unwrap();
    state = state.settle_checks(state.revision()).unwrap();
    state = state.pass(state.revision(), B).unwrap();
    assert_eq!(state.window().phase(), WindowPhase::StepEnded);
    let saved = state.clone();
    assert_eq!(
        state.activate_ability(
            state.revision(),
            A,
            source,
            ActivatedAbilityId(7),
            target,
            None
        ),
        Err(EffectError::Window(StackWindowError::NotInPriority))
    );
    assert_eq!(state, saved);
}

fn block_activation(game: &GameState) {
    for actor in [A, B] {
        reject(
            game,
            input(
                game,
                actor,
                command(game, 2, 7, TargetRef::Object(object(game, TARGET)), None),
            ),
            GameError::DecisionRequired,
        );
    }
    let execution = game.execution();
    let saved = execution.clone();
    assert!(
        execution
            .activate_ability(
                execution.revision(),
                A,
                object(game, 2),
                ActivatedAbilityId(7),
                TargetRef::Object(object(game, TARGET)),
                None
            )
            .is_err()
    );
    assert_eq!(execution, &saved);
}

#[test]
fn ability_can_counter_a_real_spell_copy_while_original_remains() {
    let game = setup(
        vec![
            artifact(
                1,
                A,
                A,
                vec![ability(
                    7,
                    SourceCost::Tap,
                    ManaCost::free(),
                    vec![Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                    ))],
                    BodyTarget::single_spell(
                        spellwake_engine::program::SpellFilter::copyable_instruction(),
                    ),
                )],
            ),
            artifact(
                2,
                A,
                A,
                vec![ability(
                    7,
                    SourceCost::Sacrifice,
                    ManaCost::free(),
                    vec![Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                            spellwake_engine::toolbox::CounterDestination::Graveyard,
                        ),
                    ))],
                    BodyTarget::single_spell(spellwake_engine::program::SpellFilter::any()),
                )],
            ),
            instant(
                90,
                A,
                ManaCost::one(ManaType::Red),
                vec![damage(3)],
                BodyTarget::SINGLE_CREATURE,
            ),
            creature(TARGET, B, B, 100),
        ],
        pools(),
    );
    let cast = cast_spell(&game, A, 90, TargetRef::Object(object(&game, TARGET)));
    let original = cast.mana_payment().unwrap().source;
    let activated = activate(cast.state(), A, 1, TargetRef::Stack(original), None);
    let pending = resolve(activated.state());
    let copied = reply(pending.state(), CopyTargetChoice::Retain.into());
    let copy = copied
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let counter = activate(copied.state(), A, 2, TargetRef::Stack(copy), None);
    let finished_counter = resolve(counter.state());
    assert_eq!(
        finished_counter
            .state()
            .execution()
            .window()
            .entries()
            .len(),
        1
    );
    assert_eq!(
        finished_counter.state().execution().window().entries()[0].id(),
        original
    );
    assert!(
        finished_counter
            .state()
            .execution()
            .window()
            .spell_copies()
            .is_empty()
    );
    let finished_original = resolve(finished_counter.state());
    assert_eq!(
        finished_original
            .state()
            .execution()
            .creature(object(finished_original.state(), TARGET))
            .unwrap()
            .unwrap()
            .damage,
        3.into()
    );
}

#[test]
fn ability_can_copy_a_spell_copy_with_another_independent_payload() {
    let game = setup(
        vec![
            artifact(
                1,
                A,
                A,
                vec![ability(
                    7,
                    SourceCost::Tap,
                    ManaCost::free(),
                    vec![Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                    ))],
                    BodyTarget::single_spell(
                        spellwake_engine::program::SpellFilter::copyable_instruction(),
                    ),
                )],
            ),
            artifact(
                2,
                B,
                A,
                vec![ability(
                    7,
                    SourceCost::Sacrifice,
                    ManaCost::free(),
                    vec![Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                    ))],
                    BodyTarget::single_spell(
                        spellwake_engine::program::SpellFilter::copyable_instruction(),
                    ),
                )],
            ),
            instant(
                90,
                A,
                ManaCost::one(ManaType::Red),
                vec![damage(3)],
                BodyTarget::SINGLE_CREATURE,
            ),
            creature(TARGET, B, B, 100),
        ],
        pools(),
    );
    let original = cast_spell(&game, A, 90, TargetRef::Object(object(&game, TARGET)));
    let original_id = original.mana_payment().unwrap().source;
    let first_ability = activate(original.state(), A, 1, TargetRef::Stack(original_id), None);
    let pending = resolve(first_ability.state());
    let first_copy = reply(pending.state(), CopyTargetChoice::Retain.into());
    let first_copy_id = first_copy
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let second_ability = activate(
        first_copy.state(),
        A,
        2,
        TargetRef::Stack(first_copy_id),
        None,
    );
    let grave = physical(second_ability.state(), 2).clone();
    let pending = resolve(second_ability.state());
    let RuleFrame::CopyTargets(frame) = pending.state().pending_decision().unwrap().frame() else {
        panic!("copy frame")
    };
    assert_eq!(frame.original(), first_copy_id);
    assert_eq!(
        frame.entry(),
        second_ability.ability_activation().unwrap().entry
    );
    let second_copy = reply(pending.state(), CopyTargetChoice::Retain.into());
    let copied = second_copy
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap();
    assert!(
        matches!(copied.kind(), StackKind::SpellCopy { copied_from, .. } if *copied_from == first_copy_id)
    );
    let second_copy_id = copied.id();
    assert_ne!(second_copy_id, first_copy_id);
    assert_ne!(second_copy_id, original_id);
    assert_eq!(second_copy.state().execution().window().entries().len(), 3);
    let resolved_second = resolve(second_copy.state());
    damage_support::assert_started_origin(resolved_second.damage_journal(), second_copy_id);
    assert_eq!(
        resolved_second.damage_journal().applied[0].source.id(),
        DamageSourceId::SpellCopy(second_copy_id)
    );
    let resolved_first = resolve(resolved_second.state());
    damage_support::assert_started_origin(resolved_first.damage_journal(), first_copy_id);
    assert_eq!(
        resolved_first.damage_journal().applied[0].source.id(),
        DamageSourceId::SpellCopy(first_copy_id)
    );
    let resolved_original = resolve(resolved_first.state());
    damage_support::assert_started_origin(resolved_original.damage_journal(), original_id);
    assert_eq!(
        resolved_original
            .state()
            .execution()
            .creature(object(resolved_original.state(), TARGET))
            .unwrap()
            .unwrap()
            .damage,
        9.into()
    );
    assert_eq!(physical(resolved_original.state(), 2), &grave);
    assert_eq!(grave.zone(), ObjectZone::Graveyard(B));
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("rules")
}
