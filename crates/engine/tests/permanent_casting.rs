//! Independent public-boundary checks for authored creature/artifact casting.
//! Prepared phases and literal programs do not certify complete cards or games.
use spellwake_contracts::RulesInteger;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    actions::TimingError,
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    effects::{CopyTargetChoice, EffectError, Instruction, Program, SeedCard},
    game::{
        CandidateBase, DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput,
        GameState, NativeCandidate, OpeningSetup, RuleFrame,
    },
    mana::{
        FloatingMana, ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment,
        ManaPaymentError, ManaSpend, ManaType,
    },
    object_identity::{CardId, GameObject, ObjectDestination, ObjectId, ObjectZone},
    opening::MulliganChoice,
    priority::{PriorityPhase, SeatId},
    shuffle::EntropyTape,
    stack_window::{StackKind, WindowPhase},
    targets::TargetRef,
    turn::{Step, TurnStart, TurnWork},
};
use std::collections::{BTreeMap, BTreeSet};

const A: SeatId = SeatId(17);
const B: SeatId = SeatId(4);

fn context() -> GameContext {
    GameContext::new(
        "permanent-test".into(),
        "cr-20260925".into(),
        "literal-fixtures".into(),
        "partial-permanents".into(),
        "ordinary-individual-turn".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}

fn creature(
    id: u64,
    owner: SeatId,
    destination: ObjectDestination,
    casting_cost: Option<ManaCost>,
    toughness: i64,
) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program: Program::creature(
            casting_cost,
            2.into(),
            toughness.into(),
            std::collections::BTreeSet::new(),
        ),
    }
}

fn artifact(id: u64, owner: SeatId, casting_cost: Option<ManaCost>) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::artifact(casting_cost, vec![]),
    }
}

fn tap_ability() -> ActivatedAbility {
    ActivatedAbility {
        id: ActivatedAbilityId(7),
        mana_cost: ManaCost::one(ManaType::Green),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::Damage { amount: 2.into() }],
        ),
    }
}

fn spell(
    id: u64,
    owner: SeatId,
    instruction: Instruction,
    cost: ManaCost,
    body_target: BodyTarget,
) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::instant(cost, ExecutableBody::new(body_target, vec![instruction])),
    }
}

fn land(id: u64, destination: ObjectDestination, choice: bool) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: A,
        destination,
        program: Program::land(
            std::collections::BTreeSet::new(),
            vec![ManaAbility {
                id: ManaAbilityId(1),
                output: if choice {
                    ManaOutput::ChooseColor {
                        amount: 1.into(),
                        colors: BTreeSet::from([ManaType::Blue, ManaType::Green]),
                    }
                } else {
                    ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())]))
                },
            }],
        ),
    }
}

fn cost() -> ManaCost {
    ManaCost {
        generic: 1.into(),
        required: BTreeMap::from([(ManaType::White, 1.into())]),
    }
}

fn payment(entries: &[(ManaType, i64)]) -> ManaPayment {
    ManaPayment {
        from_lots: Default::default(),
        spend: entries
            .iter()
            .map(|&(mana_type, amount)| ManaSpend {
                mana_type,
                amount: amount.into(),
            })
            .collect(),
    }
}

fn ordinary_payment() -> ManaPayment {
    payment(&[(ManaType::White, 1), (ManaType::Red, 1)])
}

fn seeds() -> Vec<SeedCard> {
    let mut active_artifact = artifact(2, A, Some(ManaCost::generic(1.into())));
    active_artifact.program.activated.push(tap_ability());
    vec![
        creature(1, A, ObjectDestination::Hand, Some(cost()), 3),
        active_artifact,
        creature(3, B, ObjectDestination::Hand, Some(ManaCost::free()), 3),
        artifact(4, B, Some(ManaCost::free())),
        artifact(5, A, Some(ManaCost::free())),
        creature(6, A, ObjectDestination::Hand, None, 3),
        artifact(7, A, None),
        land(8, ObjectDestination::Hand, false),
        land(9, ObjectDestination::Battlefield { controller: A }, true),
        spell(
            10,
            A,
            Instruction::Damage { amount: 2.into() },
            ManaCost::one(ManaType::Red),
            BodyTarget::SingleCreature,
        ),
        spell(
            11,
            B,
            Instruction::CounterSpell,
            ManaCost::specific(ManaType::Blue, 2.into()),
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
        ),
        spell(
            12,
            A,
            Instruction::CopySpell,
            ManaCost::specific(ManaType::Blue, 2.into()),
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction()),
        ),
        spell(
            13,
            A,
            Instruction::ReturnToOwnersHand,
            ManaCost::one(ManaType::Blue),
            BodyTarget::SingleCreature,
        ),
        SeedCard {
            card: CardId(14),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::sorcery(
                ManaCost::one(ManaType::Green),
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Damage { amount: 2.into() }],
                ),
            ),
        },
        spell(
            15,
            B,
            Instruction::CopySpell,
            ManaCost::specific(ManaType::Blue, 2.into()),
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction()),
        ),
        creature(
            90,
            B,
            ObjectDestination::Battlefield { controller: B },
            None,
            20,
        ),
        creature(98, A, ObjectDestination::LibraryBottom, None, 3),
        creature(99, B, ObjectDestination::LibraryBottom, None, 3),
    ]
}

fn pools() -> Vec<FloatingMana> {
    [A, B]
        .into_iter()
        .flat_map(|player| {
            ManaType::ALL.map(move |mana_type| FloatingMana {
                player,
                mana_type,
                amount: if player == A { 20.into() } else { 30.into() },
            })
        })
        .collect()
}

fn initialize(
    step: Option<Step>,
    cards: Vec<SeedCard>,
    mana: Vec<FloatingMana>,
) -> NativeCandidate {
    match step {
        Some(step) => GameState::initialize_scheduled(
            GameId(6083),
            context(),
            {
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
            },
            TurnStart { number: 3, step },
        ),
        None => GameState::initialize(GameId(6083), context(), {
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
        }),
    }
    .unwrap()
}

fn setup(step: Option<Step>, cards: Vec<SeedCard>) -> GameState {
    let initial = initialize(step, cards, pools());
    no_action_records(&initial);
    initial.into_state()
}

fn main() -> GameState {
    setup(Some(Step::PrecombatMain), seeds())
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

fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!(
            "expected ready fixture, got {:?}",
            game.execution().window().phase()
        );
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
    assert_eq!(
        game, &saved,
        "building a candidate mutated the accepted state"
    );
    assert_eq!(
        candidate.base(),
        &CandidateBase::Advance(game.basis().clone())
    );
    let mut basis = game.basis().clone();
    basis.revision += 1;
    assert_eq!(candidate.state().basis(), &basis);
    candidate
}

fn reject_input(game: &GameState, request: GameInput, error: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(request), Err(error));
    assert_eq!(game, &saved, "rejection changed the accepted state");
}

fn reject(game: &GameState, actor: SeatId, command: GameCommand, error: EffectError) {
    reject_input(game, input(game, actor, command), GameError::Effect(error));
}

fn untargeted(game: &GameState, card: u64, selected: Option<ManaPayment>) -> GameCommand {
    GameCommand::CastUntargeted {
        spell: object(game, card),
        payment: selected,
    }
}

fn cast(
    game: &GameState,
    actor: SeatId,
    card: u64,
    selected: Option<ManaPayment>,
) -> NativeCandidate {
    let candidate = accept(game, actor, untargeted(game, card, selected));
    assert!(candidate.permanent_entry().is_none());
    assert!(candidate.mana_payment().is_some());
    assert!(candidate.ability_activation().is_none());
    assert!(candidate.mana_production().is_none());
    assert!(candidate.land_play().is_none());
    assert!(candidate.damage_journal().applied.is_empty());
    candidate
}

fn targeted(game: &GameState, actor: SeatId, card: u64, target: TargetRef) -> NativeCandidate {
    accept(
        game,
        actor,
        GameCommand::Cast {
            spell: object(game, card),
            target,
        },
    )
}

fn pass(game: &GameState) -> NativeCandidate {
    accept(game, holder(game), GameCommand::Pass)
}

fn resolve(game: &GameState) -> NativeCandidate {
    let first = pass(game);
    no_action_records(&first);
    pass(first.state())
}

fn balances(game: &GameState) -> Vec<RulesInteger> {
    [A, B]
        .into_iter()
        .flat_map(|player| ManaType::ALL.map(|mana_type| game.execution().mana(player, mana_type)))
        .collect()
}

fn no_action_records(candidate: &NativeCandidate) {
    assert!(candidate.permanent_entry().is_none());
    assert!(candidate.mana_payment().is_none());
    assert!(candidate.ability_activation().is_none());
    assert!(candidate.mana_production().is_none());
    assert!(candidate.land_play().is_none());
}

#[test]
fn paid_creature_in_both_mains_has_distinct_hand_stack_and_battlefield_objects() {
    for step in [Step::PrecombatMain, Step::PostcombatMain] {
        let game = setup(Some(step), seeds());
        let hand = physical(&game, 1).clone();
        let cast = cast(&game, A, 1, Some(ordinary_payment()));
        let stacked = physical(cast.state(), 1).clone();
        let entry = cast.state().execution().window().entries().last().unwrap();
        assert!(matches!(entry.kind(), StackKind::CardSpell(id) if *id == stacked.id()));
        assert!(entry.targets().targets().is_empty());
        assert_eq!(entry.controller(), A);
        assert_eq!(stacked.zone(), ObjectZone::Stack);
        assert_eq!(stacked.owner(), A);
        assert_eq!(stacked.controller(), Some(A));
        assert_eq!(stacked.tapped(), None);
        assert_ne!(stacked.id(), hand.id());
        assert!(
            cast.state()
                .execution()
                .window()
                .cards()
                .get(hand.id())
                .is_none()
        );
        assert_eq!(cast.mana_payment().unwrap().source, entry.id());
        assert_eq!(cast.mana_payment().unwrap().cost, cost());
        assert_eq!(cast.mana_payment().unwrap().payer, A);
        assert_eq!(cast.state().execution().mana(A, ManaType::White), 19.into());
        assert_eq!(cast.state().execution().mana(A, ManaType::Red), 19.into());
        assert_eq!(holder(cast.state()), A);
        let paid_balances = balances(cast.state());
        let done = resolve(cast.state());
        let record = done.permanent_entry().unwrap();
        assert_eq!(record.entry, entry.id());
        assert_eq!(record.before, stacked);
        assert_eq!(record.after, *physical(done.state(), 1));
        assert_ne!(record.after.id(), stacked.id());
        assert_ne!(record.after.id(), hand.id());
        assert_eq!(record.after.zone(), ObjectZone::Battlefield);
        assert_eq!(record.after.owner(), hand.owner());
        assert_eq!(record.after.controller(), Some(A));
        assert_eq!(record.after.tapped(), Some(false));
        assert!(
            done.state()
                .execution()
                .window()
                .cards()
                .get(stacked.id())
                .is_none()
        );
        let creature = done
            .state()
            .execution()
            .creature(record.after.id())
            .unwrap()
            .unwrap();
        assert_eq!(creature.power, 2.into());
        assert_eq!(creature.toughness, 3.into());
        assert_eq!(creature.damage, 0.into());
        assert_eq!(balances(done.state()), paid_balances);
        assert!(done.mana_payment().is_none());
        assert!(done.ability_activation().is_none());
        assert!(done.mana_production().is_none());
        assert!(done.land_play().is_none());
        assert!(done.turn_transitions().is_empty());
        assert!(done.state().execution().window().entries().is_empty());
        assert_eq!(done.state().turn().unwrap().position().step, step);
        assert_eq!(done.state().turn().unwrap().lands_played(), &0.into());
        assert_eq!(holder(done.state()), A);
    }
}

#[test]
fn plain_free_artifact_resolves_without_instructions_or_an_ability_requirement() {
    let game = main();
    let before_balances = balances(&game);
    let cast = cast(&game, A, 5, None);
    assert!(cast.mana_payment().unwrap().spend.is_empty());
    let done = resolve(cast.state());
    let entry = done.permanent_entry().unwrap();
    assert_eq!(entry.before.zone(), ObjectZone::Stack);
    assert_eq!(entry.after.zone(), ObjectZone::Battlefield);
    assert_eq!(entry.after.tapped(), Some(false));
    assert!(
        done.state()
            .execution()
            .creature(entry.after.id())
            .unwrap()
            .is_none()
    );
    assert_eq!(balances(done.state()), before_balances);
    no_action_records(&pass(done.state()));
}

#[test]
fn zero_or_negative_toughness_enters_then_dies_before_priority_with_entry_preserved() {
    for toughness in [0, -1] {
        let mut cards = seeds();
        cards[0] = creature(
            1,
            A,
            ObjectDestination::Hand,
            Some(ManaCost::free()),
            toughness,
        );
        let game = setup(Some(Step::PrecombatMain), cards);
        let cast = cast(&game, A, 1, None);
        let done = resolve(cast.state());
        let record = done.permanent_entry().unwrap();
        assert_eq!(record.after.zone(), ObjectZone::Battlefield);
        assert_eq!(record.after.controller(), Some(A));
        assert_eq!(record.after.tapped(), Some(false));
        let grave = physical(done.state(), 1);
        assert_eq!(grave.zone(), ObjectZone::Graveyard(A));
        assert_eq!(grave.owner(), A);
        assert_eq!(grave.controller(), None);
        assert_eq!(grave.tapped(), None);
        assert_ne!(grave.id(), record.after.id());
        assert!(
            done.state()
                .execution()
                .window()
                .cards()
                .get(record.after.id())
                .is_none()
        );
        assert_eq!(holder(done.state()), A);
        assert!(done.state().pending_decision().is_none());
        assert!(done.damage_journal().applied.is_empty());
        no_action_records(&pass(done.state()));
    }
}

#[test]
fn raw_complete_progress_reports_entry_before_the_shared_state_check_boundary() {
    let mut cards = seeds();
    cards[0] = creature(1, A, ObjectDestination::Hand, Some(ManaCost::free()), 0);
    let game = setup(Some(Step::PrecombatMain), cards);
    let cast = cast(&game, A, 1, None);
    let execution = cast.state().execution();
    let first = execution.pass(execution.revision(), A).unwrap();
    let first = first.settle_checks(first.revision()).unwrap();
    let resolving = first.pass(first.revision(), B).unwrap();
    let saved = resolving.clone();
    assert!(matches!(
        resolving.window().phase(),
        WindowPhase::Resolving(_)
    ));
    let (
        state,
        spellwake_engine::effects::RuleResult::Complete {
            permanent_entry,
            journal,
            ..
        },
        _,
    ) = resolving
        .begin_resolution(resolving.revision())
        .unwrap()
        .into_parts()
    else {
        panic!("ordinary targetless permanent suspended");
    };
    assert_eq!(resolving, saved);
    let record = permanent_entry.unwrap();
    assert_eq!(record.before.zone(), ObjectZone::Stack);
    assert_eq!(record.after.zone(), ObjectZone::Battlefield);
    assert!(state.creature(record.after.id()).unwrap().is_some());
    assert!(matches!(
        state.window().phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
    ));
    assert!(journal.applied.is_empty());
    let checked = state.settle_checks(state.revision()).unwrap();
    assert!(checked.creature(record.after.id()).unwrap().is_none());
    let current = checked
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    assert_eq!(
        checked.window().cards().get(current).unwrap().zone(),
        ObjectZone::Graveyard(A)
    );
}

#[test]
fn newly_resolved_artifact_can_pay_a_real_tap_ability_without_becoming_its_stack_source() {
    let game = main();
    let cast = cast(&game, A, 2, Some(payment(&[(ManaType::Red, 1)])));
    let entered = resolve(cast.state());
    let source = object(entered.state(), 2);
    let before_pools = balances(entered.state());
    let activated = accept(
        entered.state(),
        A,
        GameCommand::ActivateAbility {
            source,
            ability: ActivatedAbilityId(7),
            target: TargetRef::Object(object(entered.state(), 90)),
            payment: None,
        },
    );
    assert!(activated.permanent_entry().is_none());
    let record = activated.ability_activation().unwrap();
    assert_eq!(record.source.id(), source);
    assert_eq!(record.source.zone(), ObjectZone::Battlefield);
    assert_eq!(record.source.tapped(), Some(false));
    assert_eq!(record.resulting_source, source);
    assert_eq!(record.source_cost, SourceCost::Tap);
    assert_eq!(physical(activated.state(), 2).id(), source);
    assert_eq!(physical(activated.state(), 2).tapped(), Some(true));
    assert_eq!(
        activated.mana_payment().unwrap().cost,
        ManaCost::one(ManaType::Green)
    );
    assert_eq!(
        activated.state().execution().mana(A, ManaType::Green),
        19.into()
    );
    assert_ne!(balances(activated.state()), before_pools);
    assert!(matches!(
        activated.state().execution().window().entries()[0].kind(),
        StackKind::ActivatedAbility { .. }
    ));
    let done = resolve(activated.state());
    no_action_records(&done);
    assert_eq!(done.damage_journal().applied.len(), 1);
    assert_eq!(done.damage_journal().applied[0].amount, 2.into());
    assert_eq!(
        done.state()
            .execution()
            .creature(object(done.state(), 90))
            .unwrap()
            .unwrap()
            .damage,
        2.into()
    );
    assert_eq!(physical(done.state(), 2).id(), source);
    assert_eq!(physical(done.state(), 2).tapped(), Some(true));
}

#[test]
fn new_creature_is_an_actual_later_damage_and_bounce_target_then_can_be_recast() {
    let game = main();
    let admission = cast(&game, A, 1, Some(ordinary_payment()));
    let entered = resolve(admission.state());
    let battlefield = object(entered.state(), 1);
    let damage = targeted(entered.state(), A, 10, TargetRef::Object(battlefield));
    assert!(damage.permanent_entry().is_none());
    let damaged = resolve(damage.state());
    assert!(damaged.permanent_entry().is_none());
    assert_eq!(
        damaged
            .state()
            .execution()
            .creature(battlefield)
            .unwrap()
            .unwrap()
            .damage,
        2.into()
    );
    let bounce = targeted(damaged.state(), A, 13, TargetRef::Object(battlefield));
    let returned = resolve(bounce.state());
    no_action_records(&returned);
    let new_hand = physical(returned.state(), 1);
    assert_eq!(new_hand.zone(), ObjectZone::Hand(A));
    assert_eq!(new_hand.controller(), None);
    assert_eq!(new_hand.tapped(), None);
    assert_ne!(new_hand.id(), battlefield);
    reject(
        returned.state(),
        A,
        GameCommand::CastUntargeted {
            spell: battlefield,
            payment: Some(ordinary_payment()),
        },
        EffectError::UnknownObject,
    );
    let recast = cast(returned.state(), A, 1, Some(ordinary_payment()));
    let reentered = resolve(recast.state());
    let record = reentered.permanent_entry().unwrap();
    assert_ne!(record.after.id(), battlefield);
    assert_eq!(
        reentered
            .state()
            .execution()
            .creature(record.after.id())
            .unwrap()
            .unwrap()
            .damage,
        0.into()
    );
}

#[test]
fn nonactive_instant_response_counters_permanent_without_entry_or_payment_refund() {
    for card in [1, 2] {
        let game = main();
        let selected = if card == 1 {
            ordinary_payment()
        } else {
            payment(&[(ManaType::Red, 1)])
        };
        let cast = cast(&game, A, card, Some(selected));
        let permanent = cast.state().execution().window().entries()[0].id();
        let paid_balances = balances(cast.state());
        let to_b = pass(cast.state());
        no_action_records(&to_b);
        assert_eq!(holder(to_b.state()), B);
        let response = targeted(to_b.state(), B, 11, TargetRef::Stack(permanent));
        assert!(response.permanent_entry().is_none());
        assert_eq!(response.mana_payment().unwrap().payer, B);
        let done = resolve(response.state());
        no_action_records(&done);
        assert_eq!(
            physical(done.state(), card).zone(),
            ObjectZone::Graveyard(A)
        );
        assert_eq!(physical(done.state(), 11).zone(), ObjectZone::Graveyard(B));
        for mana_type in ManaType::ALL {
            assert_eq!(
                done.state().execution().mana(A, mana_type),
                cast.state().execution().mana(A, mana_type)
            );
        }
        assert_ne!(balances(done.state()), paid_balances);
        assert_eq!(done.state().execution().mana(B, ManaType::Blue), 28.into());
        assert!(done.state().execution().window().entries().is_empty());
        assert_eq!(holder(done.state()), A);
        no_action_records(&pass(done.state()));
    }
}

#[test]
fn copying_permanent_is_rejected_before_payment_or_a_copy_decision_exists() {
    for card in [1, 5] {
        let game = main();
        let selected = (card == 1).then(ordinary_payment);
        let cast = cast(&game, A, card, selected);
        let permanent = cast.state().execution().window().entries()[0].id();
        reject(
            cast.state(),
            A,
            GameCommand::Cast {
                spell: object(cast.state(), 12),
                target: TargetRef::Stack(permanent),
            },
            EffectError::IllegalTarget,
        );
        let to_b = pass(cast.state());
        reject(
            to_b.state(),
            B,
            GameCommand::CastPaid {
                spell: object(to_b.state(), 15),
                target: TargetRef::Stack(permanent),
                payment: payment(&[(ManaType::Blue, 2)]),
            },
            EffectError::IllegalTarget,
        );
        assert!(to_b.state().pending_decision().is_none());
        assert_eq!(physical(to_b.state(), 15).zone(), ObjectZone::Hand(B));
        let done = pass(to_b.state());
        assert!(done.permanent_entry().is_some());
    }
}

#[test]
fn nonpermanent_copy_can_suspend_above_permanent_and_only_final_permanent_pass_records_entry() {
    let game = main();
    let permanent = cast(&game, A, 5, None);
    let instant = targeted(
        permanent.state(),
        A,
        10,
        TargetRef::Object(object(permanent.state(), 90)),
    );
    let original = instant
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let copying = targeted(instant.state(), A, 12, TargetRef::Stack(original));
    let pending = resolve(copying.state());
    no_action_records(&pending);
    let decision = pending.state().pending_decision().unwrap();
    assert!(matches!(decision.frame(), RuleFrame::CopyTargets(_)));
    reject_input(
        pending.state(),
        input(
            pending.state(),
            A,
            untargeted(pending.state(), 1, Some(ordinary_payment())),
        ),
        GameError::DecisionRequired,
    );
    let replied = accept(
        pending.state(),
        A,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::CopyTargets(CopyTargetChoice::Retain),
        },
    );
    no_action_records(&replied);
    let copied_damage = resolve(replied.state());
    no_action_records(&copied_damage);
    assert_eq!(copied_damage.damage_journal().applied[0].amount, 2.into());
    let original_damage = resolve(copied_damage.state());
    no_action_records(&original_damage);
    assert_eq!(original_damage.damage_journal().applied[0].amount, 2.into());
    let entered = resolve(original_damage.state());
    assert_eq!(
        entered
            .permanent_entry()
            .unwrap()
            .after
            .card_id()
            .expect("physical fixture card"),
        CardId(5)
    );
    assert_eq!(
        entered
            .state()
            .execution()
            .creature(object(entered.state(), 90))
            .unwrap()
            .unwrap()
            .damage,
        4.into()
    );
    assert_eq!(physical(entered.state(), 1).zone(), ObjectZone::Hand(A));
}

#[test]
fn ability_entries_are_neither_counter_nor_copy_spell_targets_after_artifact_entry() {
    let game = main();
    let cast = cast(&game, A, 2, Some(payment(&[(ManaType::Red, 1)])));
    let entered = resolve(cast.state());
    let activation = accept(
        entered.state(),
        A,
        GameCommand::ActivateAbility {
            source: object(entered.state(), 2),
            ability: ActivatedAbilityId(7),
            target: TargetRef::Object(object(entered.state(), 90)),
            payment: None,
        },
    );
    let ability = activation.state().execution().window().entries()[0].id();
    reject(
        activation.state(),
        A,
        GameCommand::Cast {
            spell: object(activation.state(), 12),
            target: TargetRef::Stack(ability),
        },
        EffectError::IllegalTarget,
    );
    let to_b = pass(activation.state());
    reject(
        to_b.state(),
        B,
        GameCommand::Cast {
            spell: object(to_b.state(), 11),
            target: TargetRef::Stack(ability),
        },
        EffectError::IllegalTarget,
    );
}

#[test]
fn missing_cost_is_unsupported_and_explicit_free_cost_is_castable_for_both_permanent_kinds() {
    let game = main();
    for card in [6, 7, 8] {
        for selected in [None, Some(ManaPayment::default())] {
            reject(
                &game,
                A,
                untargeted(&game, card, selected),
                EffectError::NotCastableSpell,
            );
        }
    }
    for mut seed in [
        creature(1, A, ObjectDestination::Hand, Some(ManaCost::free()), 3),
        artifact(1, A, Some(ManaCost::free())),
    ] {
        seed.destination = ObjectDestination::Hand;
        let game = setup(Some(Step::PrecombatMain), vec![seed]);
        let cast = cast(&game, A, 1, Some(ManaPayment::default()));
        assert_eq!(balances(cast.state()), balances(&game));
        assert!(resolve(cast.state()).permanent_entry().is_some());
    }
}

#[test]
fn invalid_executable_costs_reject_at_setup_without_relaxing_artifact_ability_validation() {
    for invalid in [
        ManaCost::generic((-1).into()),
        ManaCost::specific(ManaType::Blue, (-1).into()),
    ] {
        for seed in [
            creature(1, A, ObjectDestination::Hand, Some(invalid.clone()), 3),
            artifact(1, A, Some(invalid)),
        ] {
            assert_eq!(
                GameState::initialize(GameId(6083), context(), {
                    let cards: Vec<SeedCard> = vec![seed];
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
                }),
                Err(GameError::Effect(EffectError::InvalidSetup))
            );
        }
    }
    let mut duplicate = tap_ability();
    let duplicate_ids = vec![duplicate.clone(), duplicate.clone()];
    duplicate.mana_cost = ManaCost::generic((-1).into());
    let negative = vec![duplicate];
    let mut empty = tap_ability();
    empty.body.instructions.clear();
    for abilities in [duplicate_ids, negative, vec![empty]] {
        let seed = SeedCard {
            card: CardId(1),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::artifact(Some(ManaCost::free()), abilities),
        };
        assert_eq!(
            GameState::initialize(GameId(6083), context(), {
                let cards: Vec<SeedCard> = vec![seed];
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
            }),
            Err(GameError::Effect(EffectError::InvalidSetup))
        );
    }
}

#[test]
fn target_shape_is_explicit_for_targeted_and_untargeted_programs() {
    let game = main();
    for card in [10, 14] {
        reject(
            &game,
            A,
            untargeted(&game, card, None),
            EffectError::TargetRequired,
        );
        reject(
            &game,
            A,
            untargeted(&game, card, Some(ManaPayment::default())),
            EffectError::TargetRequired,
        );
    }
    for card in [1, 2, 5] {
        let target = TargetRef::Object(object(&game, 90));
        reject(
            &game,
            A,
            GameCommand::Cast {
                spell: object(&game, card),
                target,
            },
            EffectError::UnexpectedTarget,
        );
        reject(
            &game,
            A,
            GameCommand::CastPaid {
                spell: object(&game, card),
                target,
                payment: ordinary_payment(),
            },
            EffectError::UnexpectedTarget,
        );
    }
}

#[test]
fn exact_generic_payment_errors_preserve_all_nonzero_resources_and_hand_objects() {
    let game = main();
    reject(
        &game,
        A,
        untargeted(&game, 1, None),
        EffectError::ManaPaymentRequired,
    );
    for (selected, error) in [
        (
            payment(&[(ManaType::Red, 2)]),
            ManaPaymentError::MissingRequired,
        ),
        (
            payment(&[(ManaType::White, 1)]),
            ManaPaymentError::WrongTotal,
        ),
        (
            payment(&[(ManaType::White, 1), (ManaType::Red, 2)]),
            ManaPaymentError::WrongTotal,
        ),
        (
            payment(&[(ManaType::White, 1), (ManaType::Red, 1), (ManaType::Red, 1)]),
            ManaPaymentError::DuplicateType,
        ),
        (
            payment(&[
                (ManaType::White, 1),
                (ManaType::Red, 1),
                (ManaType::Blue, 0),
            ]),
            ManaPaymentError::NonPositiveSpend,
        ),
        (
            payment(&[(ManaType::White, 1), (ManaType::Red, -1)]),
            ManaPaymentError::NonPositiveSpend,
        ),
    ] {
        reject(
            &game,
            A,
            untargeted(&game, 1, Some(selected)),
            EffectError::InvalidManaPayment(error),
        );
    }
    let accepted = cast(&game, A, 1, Some(ordinary_payment()));
    assert_eq!(
        accepted.mana_payment().unwrap().spend,
        ordinary_payment().spend
    );
}

#[test]
fn insufficient_permanent_payment_does_not_borrow_an_opponents_pool() {
    let game = initialize(
        Some(Step::PrecombatMain),
        seeds(),
        vec![
            FloatingMana {
                player: A,
                mana_type: ManaType::White,
                amount: 1.into(),
            },
            FloatingMana {
                player: B,
                mana_type: ManaType::Red,
                amount: 100.into(),
            },
        ],
    )
    .into_state();
    reject(
        &game,
        A,
        untargeted(&game, 1, None),
        EffectError::InsufficientMana,
    );
    reject(
        &game,
        A,
        untargeted(&game, 1, Some(ordinary_payment())),
        EffectError::InsufficientMana,
    );
    assert_eq!(physical(&game, 1).zone(), ObjectZone::Hand(A));
    assert_eq!(game.execution().mana(B, ManaType::Red), 100.into());
}

#[test]
fn required_only_payment_is_unique_and_arbitrary_precision_costs_remain_exact() {
    let exact: RulesInteger = "90071992547409931234567890".to_owned().try_into().unwrap();
    let cards = vec![creature(
        1,
        A,
        ObjectDestination::Hand,
        Some(ManaCost::specific(ManaType::White, exact.clone())),
        3,
    )];
    let game = initialize(
        Some(Step::PrecombatMain),
        cards,
        vec![FloatingMana {
            player: A,
            mana_type: ManaType::White,
            amount: exact.plus(&3.into()),
        }],
    )
    .into_state();
    let cast = cast(&game, A, 1, None);
    assert_eq!(cast.mana_payment().unwrap().cost.total(), exact);
    assert_eq!(cast.state().execution().mana(A, ManaType::White), 3.into());
    let done = resolve(cast.state());
    assert_eq!(done.state().execution().mana(A, ManaType::White), 3.into());
    assert!(done.permanent_entry().is_some());
}

#[test]
fn free_permanent_rejects_overpayment_and_never_treats_missing_cost_as_free() {
    let game = main();
    reject(
        &game,
        A,
        untargeted(&game, 5, Some(payment(&[(ManaType::Red, 1)]))),
        EffectError::InvalidManaPayment(ManaPaymentError::WrongTotal),
    );
    reject(
        &game,
        A,
        untargeted(&game, 7, Some(ManaPayment::default())),
        EffectError::NotCastableSpell,
    );
    let cast = cast(&game, A, 5, None);
    assert_eq!(balances(cast.state()), balances(&game));
}

#[test]
fn holder_active_player_and_empty_stack_are_read_from_the_actual_game() {
    let game = main();
    let b_holds = pass(&game);
    for card in [1, 2] {
        reject(
            b_holds.state(),
            A,
            untargeted(b_holds.state(), card, Some(ordinary_payment())),
            EffectError::Timing(TimingError::NotPriorityHolder),
        );
    }
    for card in [3, 4] {
        reject(
            b_holds.state(),
            B,
            untargeted(b_holds.state(), card, None),
            EffectError::Timing(TimingError::NotActivePlayer),
        );
    }
    let occupied = targeted(&game, A, 10, TargetRef::Object(object(&game, 90)));
    for card in [1, 2, 5] {
        reject(
            occupied.state(),
            A,
            untargeted(occupied.state(), card, None),
            EffectError::Timing(TimingError::StackNotEmpty),
        );
    }
}

#[test]
fn all_nonmain_ready_positions_reject_permanents_while_instant_casting_remains_available() {
    for step in [
        Step::Upkeep,
        Step::Draw,
        Step::BeginningCombat,
        Step::EndCombat,
        Step::End,
    ] {
        let game = setup(Some(step), seeds());
        for card in [1, 2, 5] {
            reject(
                &game,
                A,
                untargeted(&game, card, Some(ordinary_payment())),
                EffectError::Timing(TimingError::MainPhaseRequired),
            );
        }
        let response = targeted(&game, A, 10, TargetRef::Object(object(&game, 90)));
        assert!(response.permanent_entry().is_none());
        assert_eq!(response.state().execution().window().entries().len(), 1);
    }
}

#[test]
fn root_and_direct_fixture_calls_cannot_invent_a_permanent_calendar() {
    for step in [None, Some(Step::PrecombatMain)] {
        let game = setup(step, seeds());
        let execution = game.execution();
        let saved = execution.clone();
        let target = TargetRef::Object(object(&game, 90));
        for card in [1, 2, 5] {
            if step.is_none() {
                reject(
                    &game,
                    A,
                    untargeted(&game, card, None),
                    EffectError::Timing(TimingError::TurnRequired),
                );
            }
            assert_eq!(
                execution.cast(execution.revision(), A, object(&game, card), target),
                Err(EffectError::Timing(TimingError::TurnRequired))
            );
            assert_eq!(
                execution.cast_with_payment(
                    execution.revision(),
                    A,
                    object(&game, card),
                    target,
                    Some(ordinary_payment())
                ),
                Err(EffectError::Timing(TimingError::TurnRequired))
            );
        }
        assert_eq!(execution, &saved);
    }
}

#[test]
fn only_actual_own_hand_and_current_incarnations_can_cast() {
    let game = main();
    reject(
        &game,
        A,
        untargeted(&game, 3, None),
        EffectError::NotOwnHand,
    );
    for destination in [
        ObjectDestination::LibraryTop,
        ObjectDestination::Graveyard,
        ObjectDestination::Exile,
        ObjectDestination::Command,
        ObjectDestination::Battlefield { controller: A },
    ] {
        let seed = creature(1, A, destination, Some(ManaCost::free()), 3);
        let game = setup(Some(Step::PrecombatMain), vec![seed]);
        reject(
            &game,
            A,
            untargeted(&game, 1, None),
            EffectError::NotOwnHand,
        );
    }
    let old = object(&game, 1);
    let cast = cast(&game, A, 1, Some(ordinary_payment()));
    reject(
        cast.state(),
        A,
        GameCommand::CastUntargeted {
            spell: old,
            payment: Some(ordinary_payment()),
        },
        EffectError::UnknownObject,
    );
    reject(
        cast.state(),
        A,
        untargeted(cast.state(), 1, Some(ordinary_payment())),
        EffectError::NotOwnHand,
    );
}

#[test]
fn exact_basis_and_actor_checks_prevent_repeated_or_foreign_casts() {
    let game = main();
    let request = input(&game, A, untargeted(&game, 1, Some(ordinary_payment())));
    let cast = accept(&game, A, request.command.clone());
    reject_input(cast.state(), request, GameError::StaleRevision);
    let base_request = input(&game, A, untargeted(&game, 5, None));
    let mut wrong_game = base_request.clone();
    wrong_game.basis.game = GameId(999);
    reject_input(&game, wrong_game, GameError::WrongGame);
    let mut wrong_context = base_request.clone();
    wrong_context.basis.context = GameContext::new(
        "other".into(),
        "rules".into(),
        "catalogue".into(),
        "programs".into(),
        "profile".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap();
    reject_input(&game, wrong_context, GameError::WrongContext);
    let mut wrong_epoch = base_request.clone();
    wrong_epoch.basis.epoch += 1;
    reject_input(&game, wrong_epoch, GameError::WrongEpoch);
    let mut wrong_revision = base_request.clone();
    wrong_revision.basis.revision += 1;
    reject_input(&game, wrong_revision, GameError::StaleRevision);
    let mut wrong_actor = base_request;
    wrong_actor.actor = SeatId(999);
    reject_input(&game, wrong_actor, GameError::UnknownActor);
}

#[test]
fn real_paid_mana_choice_blocks_permanent_cast_and_reply_does_not_forge_entry() {
    let game = main();
    let pending = accept(
        &game,
        A,
        GameCommand::ActivateMana {
            source: object(&game, 9),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    no_action_records(&pending);
    assert_eq!(physical(pending.state(), 9).tapped(), Some(true));
    let decision = pending.state().pending_decision().unwrap();
    assert!(matches!(decision.frame(), RuleFrame::ManaColor(_)));
    reject_input(
        pending.state(),
        input(
            pending.state(),
            A,
            untargeted(pending.state(), 1, Some(ordinary_payment())),
        ),
        GameError::DecisionRequired,
    );
    let reply = accept(
        pending.state(),
        A,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::ManaColor(ManaType::Green),
        },
    );
    assert!(reply.permanent_entry().is_none());
    assert!(reply.mana_payment().is_none());
    assert_eq!(reply.mana_production().unwrap().source, object(&game, 9));
    assert_eq!(
        reply.state().execution().mana(A, ManaType::Green),
        21.into()
    );
    let cast = cast(reply.state(), A, 1, Some(ordinary_payment()));
    assert!(cast.mana_production().is_none());
    assert!(resolve(cast.state()).permanent_entry().is_some());
}

#[test]
fn mandatory_combat_or_cleanup_work_blocks_both_permanent_kinds() {
    for (step, work) in [
        (Step::DeclareBlockers, TurnWork::DeclareBlockers),
        (
            Step::CombatDamage,
            TurnWork::CombatDamage { additional: false },
        ),
        (Step::Cleanup, TurnWork::Cleanup),
    ] {
        let game = setup(Some(step), seeds());
        assert_eq!(game.pending_turn_work(), Some(work));
        for card in [1, 2, 5] {
            reject_input(
                &game,
                input(&game, A, untargeted(&game, card, None)),
                GameError::TurnWorkRequired(work),
            );
        }
    }
}

#[test]
fn terminal_and_multiplayer_elimination_barriers_precede_permanent_admission() {
    for seats in [vec![A, B], vec![A, B, SeatId(99)]] {
        let mut cards = seeds();
        cards.retain(|seed| seed.card != CardId(98));
        let initial = GameState::initialize_scheduled(
            GameId(6083),
            context(),
            {
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
                    seats.clone(),
                    A,
                    cards,
                    initial_battlefield_order,
                    pools(),
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    test_vocabulary(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                )
            },
            TurnStart {
                number: 3,
                step: Step::Draw,
            },
        )
        .unwrap();
        assert!(initial.permanent_entry().is_none());
        let game = initial.state();
        if seats.len() == 2 {
            for card in [1, 2, 5] {
                reject_input(
                    game,
                    input(game, A, untargeted(game, card, None)),
                    GameError::GameEnded,
                );
            }
        } else {
            assert_eq!(game.outcome(), None);
            assert!(game.pending_eliminations().is_empty());
            assert_eq!(game.execution().window().participants(), &[B, SeatId(99)]);
            for card in [1, 2, 5] {
                assert!(
                    game.execution()
                        .window()
                        .cards()
                        .current_identity(CardId(card))
                        .is_none()
                );
            }
            reject_input(
                game,
                input(game, A, GameCommand::Pass),
                GameError::UnknownActor,
            );
        }
    }
}

#[test]
fn opening_choices_and_host_entropy_candidates_never_emit_permanent_entry() {
    let cards: Vec<SeedCard> = [A, B]
        .into_iter()
        .enumerate()
        .flat_map(|(index, owner)| {
            (0..8).map(move |ordinal| {
                creature(
                    1000 + index as u64 * 100 + ordinal,
                    owner,
                    ObjectDestination::LibraryBottom,
                    Some(ManaCost::free()),
                    3,
                )
            })
        })
        .collect();
    let initial = GameState::initialize_opening(
        GameId(6083),
        context(),
        OpeningSetup {
            seed: {
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
            },
            entropy: EntropyTape::new(vec![0; 100]),
        },
    )
    .unwrap();
    no_action_records(&initial);
    let game = initial.state();
    let hand = game
        .execution()
        .window()
        .cards()
        .iter()
        .find(|card| card.zone() == ObjectZone::Hand(A))
        .unwrap()
        .id();
    reject_input(
        game,
        input(
            game,
            A,
            GameCommand::CastUntargeted {
                spell: hand,
                payment: None,
            },
        ),
        GameError::DecisionRequired,
    );
    let refill = game.supply_entropy(game.basis(), vec![0, 1]).unwrap();
    no_action_records(&refill);
    let decision = refill.state().pending_decision().unwrap();
    let keep = accept(
        refill.state(),
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::Mulligan(MulliganChoice::Keep),
        },
    );
    no_action_records(&keep);
    assert!(keep.state().pending_decision().is_some());
}

#[test]
fn later_land_and_production_candidates_preserve_their_own_records_without_repeating_entry() {
    let game = main();
    let cast = cast(&game, A, 5, None);
    let entered = resolve(cast.state());
    let entry = entered.permanent_entry().unwrap().clone();
    let land = accept(
        entered.state(),
        A,
        GameCommand::PlayLand {
            land: object(entered.state(), 8),
        },
    );
    assert!(land.permanent_entry().is_none());
    assert!(land.land_play().is_some());
    assert!(land.mana_payment().is_none());
    let production = accept(
        land.state(),
        A,
        GameCommand::ActivateMana {
            source: object(land.state(), 8),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert!(production.permanent_entry().is_none());
    assert!(production.land_play().is_none());
    assert!(production.mana_production().is_some());
    assert_eq!(entry.after, *physical(production.state(), 5));
    assert_eq!(production.state().turn().unwrap().lands_played(), &1.into());
}

#[test]
fn final_pass_only_resolves_targetless_permanent_for_two_through_six_seats() {
    for count in 2..=6 {
        let seats: Vec<_> = (0..count).map(SeatId).collect();
        let active = seats[0];
        let game = GameState::initialize_scheduled(
            GameId(6083),
            context(),
            GameSeed::with_uniform_life(
                seats.clone(),
                active,
                vec![artifact(1, active, Some(ManaCost::free()))],
                vec![],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                test_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
            TurnStart {
                number: 3,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let cast = cast(&game, active, 1, None);
        let mut game = cast.into_state();
        for (index, seat) in seats.iter().enumerate() {
            assert_eq!(holder(&game), *seat);
            let candidate = pass(&game);
            if index + 1 == seats.len() {
                assert!(candidate.permanent_entry().is_some());
                assert_eq!(holder(candidate.state()), active);
                assert_eq!(
                    physical(candidate.state(), 1).zone(),
                    ObjectZone::Battlefield
                );
            } else {
                no_action_records(&candidate);
                assert_eq!(physical(candidate.state(), 1).zone(), ObjectZone::Stack);
            }
            game = candidate.into_state();
        }
    }
}

#[path = "permanent_casting/cast_copies.rs"]
mod cast_copies;
#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("cr-20260925")
}

#[path = "permanent_casting/keyword_permissions.rs"]
mod keyword_permissions;

#[path = "permanent_casting/cycling.rs"]
mod cycling;

#[path = "permanent_casting/cascade.rs"]
mod cascade;
#[path = "permanent_casting/escape.rs"]
mod escape;
#[path = "permanent_casting/exile_hand.rs"]
mod exile_hand;
#[path = "permanent_casting/keyword_costs.rs"]
mod keyword_costs;
#[path = "permanent_casting/madness.rs"]
mod madness;

#[path = "permanent_casting/attachments.rs"]
mod attachment_rules;

#[path = "permanent_casting/crew_saddle.rs"]
mod crew_saddle;
#[path = "permanent_casting/end_step_cast.rs"]
mod end_step_cast;
#[path = "permanent_casting/evoke.rs"]
mod evoke;
#[path = "permanent_casting/face_down.rs"]
mod face_down;

#[path = "permanent_casting/exhaust.rs"]
mod exhaust;

#[path = "permanent_casting/ninja.rs"]
mod ninja;

#[path = "permanent_casting/day_night.rs"]
mod day_night;
#[path = "permanent_casting/power_up.rs"]
mod power_up;
#[path = "permanent_casting/speed.rs"]
mod speed;

#[path = "permanent_casting/basic_land_search.rs"]
mod basic_land_search;
