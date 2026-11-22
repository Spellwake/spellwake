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
    Instruction::Damage {
        amount: amount.into(),
    }
}
fn shield(amount: i64) -> Instruction {
    Instruction::CreatePreventionShield {
        amount: amount.into(),
    }
}
fn multiplier(factor: i64) -> Instruction {
    Instruction::CreateDamageMultiplier {
        factor: factor.into(),
    }
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
fn world_with(
    source_cost: SourceCost,
    cost: ManaCost,
    instructions: Vec<Instruction>,
    body_target: BodyTarget,
) -> GameState {
    setup(
        vec![
            artifact(
                1,
                A,
                A,
                vec![ability(7, source_cost, cost, instructions, body_target)],
            ),
            artifact(
                2,
                A,
                A,
                vec![ability(
                    7,
                    SourceCost::Tap,
                    ManaCost::free(),
                    vec![damage(4)],
                    BodyTarget::SingleCreature,
                )],
            ),
            artifact(
                3,
                A,
                B,
                vec![ability(
                    7,
                    SourceCost::Tap,
                    ManaCost::free(),
                    vec![damage(1)],
                    BodyTarget::SingleCreature,
                )],
            ),
            creature(TARGET, B, B, 100),
            creature(OTHER, A, A, 3),
            instant(
                90,
                A,
                ManaCost::one(ManaType::Red),
                vec![damage(3)],
                BodyTarget::SingleCreature,
            ),
            instant(
                91,
                B,
                ManaCost::one(ManaType::Blue),
                vec![Instruction::ReturnToOwnersHand],
                BodyTarget::SingleCreature,
            ),
            instant(
                92,
                A,
                ManaCost::free(),
                vec![Instruction::CounterSpell],
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
            ),
            instant(
                93,
                A,
                ManaCost::free(),
                vec![Instruction::CopySpell],
                BodyTarget::SingleSpell(
                    spellwake_engine::program::SpellFilter::copyable_instruction(),
                ),
            ),
            SeedCard {
                card: CardId(10),
                owner: A,
                destination: ObjectDestination::Battlefield { controller: A },
                program: Program::land(
                    std::collections::BTreeSet::new(),
                    vec![ManaAbility {
                        id: ManaAbilityId(7),
                        output: ManaOutput::ChooseColor {
                            amount: 2.into(),
                            colors: BTreeSet::from([ManaType::Red, ManaType::Blue]),
                        },
                    }],
                ),
            },
        ],
        pools(),
    )
}
fn world() -> GameState {
    world_with(
        SourceCost::Tap,
        ManaCost::free(),
        vec![damage(1)],
        BodyTarget::SingleCreature,
    )
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
fn free_tap_ability_waits_for_every_pass_and_keeps_pre_cost_snapshot() {
    let game = world();
    let source = object(&game, 1);
    let target = object(&game, TARGET);
    let activated = activate(&game, A, 1, TargetRef::Object(target), None);
    let entry = check_activation(&game, &activated, 1, A, SourceCost::Tap);
    let saved_record = activated.ability_activation().unwrap().clone();
    assert_eq!(saved_record.source.tapped(), Some(false));
    assert_eq!(object(activated.state(), 1), source);
    assert_eq!(physical(activated.state(), 1).tapped(), Some(true));
    assert_eq!(balances(activated.state()), balances(&game));
    assert_eq!(activated.mana_payment().unwrap().cost, ManaCost::free());
    assert!(activated.mana_payment().unwrap().spend.is_empty());
    assert_eq!(
        activated
            .state()
            .execution()
            .creature(target)
            .unwrap()
            .unwrap()
            .damage,
        0.into()
    );
    let first = pass(activated.state());
    assert_eq!(
        first
            .state()
            .execution()
            .creature(target)
            .unwrap()
            .unwrap()
            .damage,
        0.into()
    );
    let finished = pass(first.state());
    assert!(finished.state().execution().window().entries().is_empty());
    assert_eq!(physical(finished.state(), 1).id(), source);
    assert_eq!(physical(finished.state(), 1).tapped(), Some(true));
    assert_eq!(
        finished
            .state()
            .execution()
            .creature(target)
            .unwrap()
            .unwrap()
            .damage,
        1.into()
    );
    damage_support::assert_started_origin(finished.damage_journal(), entry);
    assert_eq!(
        finished.damage_journal().applied[0].source,
        DamageSource::Object {
            object: physical(finished.state(), 1).clone(),
            controller: A,
        }
    );
    assert_eq!(saved_record.source.tapped(), Some(false));
}

#[test]
fn every_typed_cost_debits_only_controllers_required_mana() {
    for mana_type in ManaType::ALL {
        let game = world_with(
            SourceCost::Tap,
            ManaCost::specific(mana_type, 3.into()),
            vec![damage(1)],
            BodyTarget::SingleCreature,
        );
        let target = object(&game, TARGET);
        let activated = activate(&game, A, 1, TargetRef::Object(target), None);
        check_activation(&game, &activated, 1, A, SourceCost::Tap);
        for player in [A, B] {
            for current_type in ManaType::ALL {
                let expected = game
                    .execution()
                    .mana(player, current_type)
                    .minus(&if player == A && current_type == mana_type {
                        3.into()
                    } else {
                        0.into()
                    });
                assert_eq!(
                    activated.state().execution().mana(player, current_type),
                    expected
                );
            }
        }
        assert_eq!(
            activated.mana_payment().unwrap().spend,
            vec![ManaSpend {
                mana_type,
                amount: 3.into()
            }]
        );
    }
}

#[test]
fn mixed_generic_payment_preserves_distinct_explicit_resource_choices() {
    let cost = ManaCost {
        generic: 3.into(),
        required: BTreeMap::from([(ManaType::Blue, 1.into()), (ManaType::Colourless, 1.into())]),
    };
    let game = world_with(
        SourceCost::Sacrifice,
        cost.clone(),
        vec![damage(2)],
        BodyTarget::SingleCreature,
    );
    let target = object(&game, TARGET);
    for chosen in [
        payment(&[
            (ManaType::Red, 3),
            (ManaType::Colourless, 1),
            (ManaType::Blue, 1),
        ]),
        payment(&[
            (ManaType::Blue, 3),
            (ManaType::White, 1),
            (ManaType::Colourless, 1),
        ]),
    ] {
        let candidate = activate(&game, A, 1, TargetRef::Object(target), Some(chosen.clone()));
        check_activation(&game, &candidate, 1, A, SourceCost::Sacrifice);
        let record = candidate.mana_payment().unwrap();
        assert_eq!(record.cost, cost);
        let mut normalized = chosen.spend.clone();
        normalized.sort_by_key(|part| part.mana_type);
        assert_eq!(record.spend, normalized);
        for player in [A, B] {
            for mana_type in ManaType::ALL {
                let spent = if player == A {
                    chosen
                        .spend
                        .iter()
                        .find(|part| part.mana_type == mana_type)
                        .map(|part| part.amount.clone())
                        .unwrap_or_else(|| 0.into())
                } else {
                    0.into()
                };
                assert_eq!(
                    candidate.state().execution().mana(player, mana_type),
                    game.execution().mana(player, mana_type).minus(&spent)
                );
            }
        }
    }
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
                    BodyTarget::SingleCreature,
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
fn explicit_zero_required_cost_accepts_omitted_or_empty_spend() {
    let cost = ManaCost {
        generic: 0.into(),
        required: BTreeMap::from([(ManaType::Green, 0.into())]),
    };
    let game = world_with(
        SourceCost::Tap,
        cost.clone(),
        vec![damage(1)],
        BodyTarget::SingleCreature,
    );
    for chosen in [None, Some(ManaPayment::default())] {
        let candidate = activate(
            &game,
            A,
            1,
            TargetRef::Object(object(&game, TARGET)),
            chosen,
        );
        check_activation(&game, &candidate, 1, A, SourceCost::Tap);
        assert_eq!(balances(candidate.state()), balances(&game));
        assert_eq!(candidate.mana_payment().unwrap().cost, cost);
        assert!(candidate.mana_payment().unwrap().spend.is_empty());
    }
}

#[test]
fn omitted_generic_and_insufficient_mana_leave_source_cost_unpaid() {
    for (cost, error) in [
        (
            ManaCost::generic(3.into()),
            EffectError::ManaPaymentRequired,
        ),
        (
            ManaCost::generic(1000.into()),
            EffectError::InsufficientMana,
        ),
        (
            ManaCost::specific(ManaType::Blue, 1000.into()),
            EffectError::InsufficientMana,
        ),
    ] {
        let game = world_with(
            SourceCost::Sacrifice,
            cost,
            vec![damage(1)],
            BodyTarget::SingleCreature,
        );
        reject_effect(
            &game,
            A,
            command(&game, 1, 7, TargetRef::Object(object(&game, TARGET)), None),
            error,
        );
        assert_eq!(physical(&game, 1).zone(), ObjectZone::Battlefield);
    }
}

#[test]
fn malformed_payment_is_atomic_before_tap_or_sacrifice() {
    let cost = ManaCost {
        generic: 2.into(),
        required: BTreeMap::from([(ManaType::Blue, 1.into())]),
    };
    for source_cost in [SourceCost::Tap, SourceCost::Sacrifice] {
        for (chosen, error) in [
            (
                payment(&[(ManaType::Blue, 1), (ManaType::Red, 1), (ManaType::Red, 1)]),
                ManaPaymentError::DuplicateType,
            ),
            (
                payment(&[
                    (ManaType::Blue, 1),
                    (ManaType::Red, 2),
                    (ManaType::Green, 0),
                ]),
                ManaPaymentError::NonPositiveSpend,
            ),
            (
                payment(&[
                    (ManaType::Blue, 1),
                    (ManaType::Red, 3),
                    (ManaType::Green, -1),
                ]),
                ManaPaymentError::NonPositiveSpend,
            ),
            (
                payment(&[(ManaType::Red, 3)]),
                ManaPaymentError::MissingRequired,
            ),
            (
                payment(&[(ManaType::Blue, 1), (ManaType::Red, 1)]),
                ManaPaymentError::WrongTotal,
            ),
            (
                payment(&[(ManaType::Blue, 1), (ManaType::Red, 3)]),
                ManaPaymentError::WrongTotal,
            ),
        ] {
            let game = world_with(
                source_cost,
                cost.clone(),
                vec![damage(1)],
                BodyTarget::SingleCreature,
            );
            reject_effect(
                &game,
                A,
                command(
                    &game,
                    1,
                    7,
                    TargetRef::Object(object(&game, TARGET)),
                    Some(chosen),
                ),
                EffectError::InvalidManaPayment(error),
            );
        }
    }
    let game = world_with(
        SourceCost::Sacrifice,
        ManaCost::specific(ManaType::Blue, 50.into()),
        vec![damage(1)],
        BodyTarget::SingleCreature,
    );
    reject_effect(
        &game,
        A,
        command(
            &game,
            1,
            7,
            TargetRef::Object(object(&game, TARGET)),
            Some(payment(&[(ManaType::Blue, 50)])),
        ),
        EffectError::InsufficientMana,
    );
}

#[test]
fn setup_rejects_invalid_ability_definitions_and_later_instructions() {
    let free = || {
        ability(
            7,
            SourceCost::Tap,
            ManaCost::free(),
            vec![damage(1)],
            BodyTarget::SingleCreature,
        )
    };
    // Plain artifacts now have an intentional empty-ability representation.
    assert!(
        EffectWindow::new(GameSeed::with_uniform_life(
            vec![A, B],
            A,
            vec![artifact(1, A, A, vec![])],
            vec![CardId(1)],
            pools(),
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .is_ok()
    );
    let mut definitions = vec![
        vec![free(), free()],
        vec![ability(
            7,
            SourceCost::Tap,
            ManaCost::generic((-1).into()),
            vec![damage(1)],
            BodyTarget::SingleCreature,
        )],
        vec![ability(
            7,
            SourceCost::Tap,
            ManaCost::specific(ManaType::Red, (-1).into()),
            vec![damage(1)],
            BodyTarget::SingleCreature,
        )],
    ];
    for instructions in [
        vec![],
        vec![damage(1), Instruction::CopySpell],
        vec![Instruction::CounterSpell, shield(1)],
        vec![damage(1), damage(0)],
        vec![damage(1), shield(-1)],
        vec![damage(1), multiplier(0)],
        vec![
            damage(1),
            Instruction::PutPowerToughnessCounters {
                counters: PowerToughnessCounters {
                    plus_one: (-1).into(),
                    minus_one: 1.into(),
                },
            },
        ],
        vec![
            damage(1),
            Instruction::PutPowerToughnessCounters {
                counters: PowerToughnessCounters {
                    plus_one: 0.into(),
                    minus_one: 0.into(),
                },
            },
        ],
    ] {
        definitions.push(vec![ability(
            7,
            SourceCost::Sacrifice,
            ManaCost::free(),
            instructions,
            BodyTarget::SingleCreature,
        )]);
    }
    for abilities in definitions {
        assert_eq!(
            EffectWindow::new(GameSeed::with_uniform_life(
                vec![A, B],
                A,
                vec![artifact(1, A, A, abilities)],
                vec![CardId(1)],
                pools(),
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                test_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )),
            Err(EffectError::InvalidSetup)
        );
    }
    let mut source = artifact(1, A, A, vec![free()]);
    source.destination = ObjectDestination::Stack { controller: A };
    assert_eq!(
        EffectWindow::new({
            let cards: Vec<SeedCard> = vec![source];
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
        Err(EffectError::InvalidSetup)
    );
    assert!(
        EffectWindow::new(GameSeed::with_uniform_life(
            vec![A, B],
            A,
            vec![
                artifact(1, A, A, vec![free()]),
                artifact(2, A, A, vec![free()])
            ],
            vec![CardId(1), CardId(2)],
            pools(),
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .is_ok()
    );
}

#[test]
fn unknown_ability_and_nonartifact_sources_reject_without_costs() {
    let game = world();
    let target = TargetRef::Object(object(&game, TARGET));
    reject_effect(
        &game,
        A,
        command(&game, 1, 999, target, None),
        EffectError::UnknownActivatedAbility,
    );
    for card in [10, 90, TARGET] {
        reject_effect(
            &game,
            A,
            command(&game, card, 7, target, None),
            EffectError::NotAbilitySource,
        );
    }
}

#[test]
fn unknown_and_sacrificed_source_handles_never_follow_card_identity() {
    let game = world_with(
        SourceCost::Sacrifice,
        ManaCost::free(),
        vec![damage(1)],
        BodyTarget::SingleCreature,
    );
    let old = object(&game, 1);
    let mut foreign = GameObjects::new(0);
    let mut absent = old;
    for id in 0..50 {
        let changed = foreign
            .seed_card(foreign.revision(), CardId(id), A, ObjectDestination::Hand)
            .unwrap();
        absent = changed.after.id();
        foreign = changed.next;
    }
    assert!(game.execution().window().cards().get(absent).is_none());
    reject_effect(
        &game,
        A,
        GameCommand::ActivateAbility {
            source: absent,
            ability: ActivatedAbilityId(7),
            target: TargetRef::Object(object(&game, TARGET)),
            payment: None,
        },
        EffectError::UnknownObject,
    );
    let candidate = activate(&game, A, 1, TargetRef::Object(object(&game, TARGET)), None);
    reject_effect(
        candidate.state(),
        A,
        GameCommand::ActivateAbility {
            source: old,
            ability: ActivatedAbilityId(7),
            target: TargetRef::Object(object(candidate.state(), TARGET)),
            payment: None,
        },
        EffectError::UnknownObject,
    );
    assert_ne!(object(candidate.state(), 1), old);
}

#[test]
fn owner_does_not_authorize_foreign_controlled_source() {
    let game = world();
    reject_effect(
        &game,
        A,
        command(&game, 3, 7, TargetRef::Object(object(&game, TARGET)), None),
        EffectError::Window(StackWindowError::WrongController),
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
                    BodyTarget::SingleCreature,
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
fn illegal_creature_and_stack_targets_do_not_pay_costs() {
    let game = world();
    let mut foreign = GameObjects::new(0);
    let mut absent = object(&game, TARGET);
    for id in 0..50 {
        let changed = foreign
            .seed_card(foreign.revision(), CardId(id), A, ObjectDestination::Hand)
            .unwrap();
        absent = changed.after.id();
        foreign = changed.next;
    }
    assert!(game.execution().window().cards().get(absent).is_none());
    for target in [
        TargetRef::Player(A),
        TargetRef::Object(object(&game, 2)),
        TargetRef::Object(object(&game, 90)),
        TargetRef::Object(absent),
    ] {
        reject_effect(
            &game,
            A,
            command(&game, 1, 7, target, None),
            EffectError::IllegalTarget,
        );
    }
    let activated = activate(&game, A, 2, TargetRef::Object(object(&game, TARGET)), None);
    let entry = activated.ability_activation().unwrap().entry;
    reject_effect(
        activated.state(),
        A,
        command(activated.state(), 1, 7, TargetRef::Stack(entry), None),
        EffectError::IllegalTarget,
    );
    let game = world_with(
        SourceCost::Tap,
        ManaCost::free(),
        vec![Instruction::CopySpell],
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction()),
    );
    reject_effect(
        &game,
        A,
        command(&game, 1, 7, TargetRef::Object(object(&game, TARGET)), None),
        EffectError::IllegalTarget,
    );
}

#[test]
fn already_tapped_source_rejects_second_tap_without_erasing_first_entry() {
    let game = world();
    let candidate = activate(&game, A, 1, TargetRef::Object(object(&game, TARGET)), None);
    reject_effect(
        candidate.state(),
        A,
        command(
            candidate.state(),
            1,
            7,
            TargetRef::Object(object(candidate.state(), TARGET)),
            None,
        ),
        EffectError::Window(StackWindowError::SourceTapped),
    );
    assert_eq!(candidate.state().execution().window().entries().len(), 1);
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
                    BodyTarget::SingleCreature,
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
                        BodyTarget::SingleCreature,
                    ),
                    ability(
                        8,
                        SourceCost::Sacrifice,
                        ManaCost::one(ManaType::Red),
                        vec![damage(4)],
                        BodyTarget::SingleCreature,
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
fn exact_basis_unknown_actor_and_nonholder_are_rejected() {
    let game = world();
    let base = input(
        &game,
        A,
        command(&game, 1, 7, TargetRef::Object(object(&game, TARGET)), None),
    );
    let mut wrong = base.clone();
    wrong.basis.game = GameId(999);
    reject(&game, wrong, GameError::WrongGame);
    let mut wrong = base.clone();
    wrong.basis.context = GameContext::new(
        "other".into(),
        "rules".into(),
        "catalogue".into(),
        "programs".into(),
        "activations".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap();
    reject(&game, wrong, GameError::WrongContext);
    let mut wrong = base.clone();
    wrong.basis.epoch += 1;
    reject(&game, wrong, GameError::WrongEpoch);
    let mut wrong = base.clone();
    wrong.basis.revision += 1;
    reject(&game, wrong, GameError::StaleRevision);
    let mut wrong = base;
    wrong.actor = SeatId(99);
    reject(&game, wrong, GameError::UnknownActor);
    reject_effect(
        &game,
        B,
        command(&game, 3, 7, TargetRef::Object(object(&game, TARGET)), None),
        EffectError::Window(StackWindowError::Priority(PriorityError::WrongPlayer)),
    );
    reject(
        &game,
        input(
            &game,
            A,
            GameCommand::Reply {
                decision: DecisionId(1),
                choice: DecisionReply::ManaColor(ManaType::Red),
            },
        ),
        GameError::NoDecision,
    );
}

#[test]
fn direct_executor_and_game_admission_share_actual_transition() {
    let game = world_with(
        SourceCost::Sacrifice,
        ManaCost::generic(2.into()),
        vec![damage(2)],
        BodyTarget::SingleCreature,
    );
    let execution = game.execution();
    let saved = execution.clone();
    let target = TargetRef::Object(object(&game, TARGET));
    let chosen = payment(&[(ManaType::Green, 2)]);
    let direct = execution
        .activate_ability(
            execution.revision(),
            A,
            object(&game, 1),
            ActivatedAbilityId(7),
            target,
            Some(chosen.clone()),
        )
        .unwrap();
    assert_eq!(execution, &saved);
    let candidate = activate(&game, A, 1, target, Some(chosen));
    assert_eq!(
        candidate.ability_activation(),
        Some(&direct.result().activation)
    );
    assert_eq!(candidate.mana_payment(), Some(&direct.result().payment));
    let next = direct.into_state();
    assert_eq!(
        candidate.state().execution(),
        &next.settle_checks(next.revision()).unwrap()
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
                    BodyTarget::SingleCreature,
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

#[test]
fn paid_response_target_departure_leaves_ability_and_source_costs_paid() {
    for source_cost in [SourceCost::Tap, SourceCost::Sacrifice] {
        let game = world_with(
            source_cost,
            ManaCost::one(ManaType::Red),
            vec![damage(2)],
            BodyTarget::SingleCreature,
        );
        let old_source = object(&game, 1);
        let target = object(&game, TARGET);
        let activated = activate(&game, A, 1, TargetRef::Object(target), None);
        let entry = check_activation(&game, &activated, 1, A, source_cost);
        let source_after_cost = physical(activated.state(), 1).clone();
        let response = cast_spell(activated.state(), B, 91, TargetRef::Object(target));
        assert_eq!(
            response.mana_payment().unwrap().cost,
            ManaCost::one(ManaType::Blue)
        );
        assert_eq!(
            response.mana_payment().unwrap().spend,
            vec![ManaSpend {
                mana_type: ManaType::Blue,
                amount: 1.into()
            }]
        );
        let response_resolved = resolve(response.state());
        assert_eq!(
            physical(response_resolved.state(), TARGET).zone(),
            ObjectZone::Hand(B)
        );
        assert_ne!(object(response_resolved.state(), TARGET), target);
        let before_failure = balances(response_resolved.state());
        let failed = resolve(response_resolved.state());
        assert_eq!(failed.damage_journal(), &DamageJournal::default());
        assert_eq!(balances(failed.state()), before_failure);
        assert!(failed.state().execution().window().entries().is_empty());
        assert_eq!(physical(failed.state(), 1), &source_after_cost);
        assert!(
            failed
                .state()
                .execution()
                .window()
                .entries()
                .iter()
                .all(|item| item.id() != entry)
        );
        assert_eq!(
            failed.state().execution().mana(A, ManaType::Red),
            game.execution().mana(A, ManaType::Red).minus(&1.into())
        );
        if source_cost == SourceCost::Tap {
            assert_eq!(physical(failed.state(), 1).id(), old_source);
        } else {
            assert_ne!(physical(failed.state(), 1).id(), old_source);
        }
        reject_effect(
            failed.state(),
            holder(failed.state()),
            command(failed.state(), 2, 7, TargetRef::Object(target), None),
            EffectError::IllegalTarget,
        );
    }
}

#[test]
fn sacrifice_bounce_uses_frozen_payload_after_source_is_gone() {
    let game = world_with(
        SourceCost::Sacrifice,
        ManaCost::one(ManaType::Blue),
        vec![Instruction::ReturnToOwnersHand],
        BodyTarget::SingleCreature,
    );
    let target = object(&game, TARGET);
    let activated = activate(&game, A, 1, TargetRef::Object(target), None);
    let grave = physical(activated.state(), 1).clone();
    let finished = resolve(activated.state());
    assert_eq!(physical(finished.state(), 1), &grave);
    assert_eq!(
        physical(finished.state(), TARGET).zone(),
        ObjectZone::Hand(B)
    );
    assert_ne!(object(finished.state(), TARGET), target);
    assert_eq!(finished.damage_journal(), &DamageJournal::default());
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
fn counter_and_copy_spells_cannot_target_an_ability_with_a_real_payload() {
    let game = world();
    let activated = activate(&game, A, 1, TargetRef::Object(object(&game, TARGET)), None);
    let entry = activated.ability_activation().unwrap().entry;
    for card in [92, 93] {
        reject_effect(
            activated.state(),
            A,
            GameCommand::Cast {
                spell: object(activated.state(), card),
                target: TargetRef::Stack(entry),
            },
            EffectError::IllegalTarget,
        );
    }
    assert_eq!(activated.state().execution().window().entries().len(), 1);
    let finished = resolve(activated.state());
    damage_support::assert_started_origin(finished.damage_journal(), entry);
}

#[test]
fn counter_ability_removes_real_spell_without_refunding_either_cost() {
    let game = world_with(
        SourceCost::Sacrifice,
        ManaCost::one(ManaType::Blue),
        vec![Instruction::CounterSpell],
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
    );
    let cast = cast_spell(&game, A, 90, TargetRef::Object(object(&game, TARGET)));
    let spell = cast.mana_payment().unwrap().source;
    let activated = activate(cast.state(), A, 1, TargetRef::Stack(spell), None);
    let entry = check_activation(cast.state(), &activated, 1, A, SourceCost::Sacrifice);
    let grave = physical(activated.state(), 1).clone();
    let finished = resolve(activated.state());
    assert_eq!(finished.damage_journal(), &DamageJournal::default());
    assert_eq!(
        physical(finished.state(), 90).zone(),
        ObjectZone::Graveyard(A)
    );
    assert_eq!(physical(finished.state(), 1), &grave);
    assert!(finished.state().execution().window().entries().is_empty());
    assert_ne!(entry, spell);
    assert_eq!(
        finished
            .state()
            .execution()
            .creature(object(finished.state(), TARGET))
            .unwrap()
            .unwrap()
            .damage,
        0.into()
    );
    for mana_type in [ManaType::Red, ManaType::Blue] {
        assert_eq!(
            finished.state().execution().mana(A, mana_type),
            game.execution().mana(A, mana_type).minus(&1.into())
        );
    }
}

#[test]
fn copy_ability_uses_same_exact_decision_and_creates_an_independent_spell() {
    let game = world_with(
        SourceCost::Sacrifice,
        ManaCost::one(ManaType::Blue),
        vec![Instruction::CopySpell],
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction()),
    );
    let cast = cast_spell(&game, A, 90, TargetRef::Object(object(&game, TARGET)));
    let original = cast.mana_payment().unwrap().source;
    let activated = activate(cast.state(), A, 1, TargetRef::Stack(original), None);
    let entry = activated.ability_activation().unwrap().entry;
    let grave = physical(activated.state(), 1).clone();
    let pending = resolve(activated.state());
    let game = pending.state();
    let decision = game.pending_decision().unwrap();
    let RuleFrame::CopyTargets(frame) = decision.frame() else {
        panic!("copy frame")
    };
    assert_eq!(frame.entry(), entry);
    assert_eq!(frame.original(), original);
    assert_eq!(frame.instruction_index(), 0);
    assert_eq!(frame.chooser(), A);
    assert_eq!(decision.basis(), game.basis());
    block_activation(game);
    reject(
        game,
        input(
            game,
            B,
            GameCommand::Reply {
                decision: decision.id(),
                choice: CopyTargetChoice::Retain.into(),
            },
        ),
        GameError::WrongChooser,
    );
    reject(
        game,
        input(
            game,
            A,
            GameCommand::Reply {
                decision: DecisionId(999),
                choice: CopyTargetChoice::Retain.into(),
            },
        ),
        GameError::WrongDecision,
    );
    reject(
        game,
        input(
            game,
            A,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::ManaColor(ManaType::Blue),
            },
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    let completed = reply(
        game,
        CopyTargetChoice::Change(TargetRef::Object(object(game, OTHER))).into(),
    );
    assert!(completed.state().pending_decision().is_none());
    assert_eq!(physical(completed.state(), 1), &grave);
    let entries = completed.state().execution().window().entries();
    assert_eq!(entries.len(), 2);
    let copy = entries.last().unwrap();
    assert!(
        matches!(copy.kind(), StackKind::SpellCopy { copied_from, owner } if *copied_from == original && *owner == A)
    );
    assert_ne!(copy.id(), original);
    assert_ne!(copy.id(), entry);
    assert_eq!(
        copy.targets().targets()[0].choice().target,
        TargetRef::Object(object(game, OTHER))
    );
    assert_eq!(
        entries[0].targets().targets()[0].choice().target,
        TargetRef::Object(object(game, TARGET))
    );
    assert_eq!(balances(completed.state()), balances(game));
    let resolved_copy = resolve(completed.state());
    assert_eq!(
        physical(resolved_copy.state(), OTHER).zone(),
        ObjectZone::Graveyard(A)
    );
    damage_support::assert_started_origin(resolved_copy.damage_journal(), copy.id());
    assert_eq!(
        resolved_copy.damage_journal().applied[0].source.id(),
        DamageSourceId::SpellCopy(copy.id())
    );
    let resolved_original = resolve(resolved_copy.state());
    damage_support::assert_started_origin(resolved_original.damage_journal(), original);
    assert_eq!(
        resolved_original
            .state()
            .execution()
            .creature(object(resolved_original.state(), TARGET))
            .unwrap()
            .unwrap()
            .damage,
        3.into()
    );
    assert_eq!(physical(resolved_original.state(), 1), &grave);
}

#[test]
fn two_copy_instructions_resume_indices_without_repeating_first_copy() {
    let game = world_with(
        SourceCost::Sacrifice,
        ManaCost::free(),
        vec![Instruction::CopySpell, Instruction::CopySpell],
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction()),
    );
    let cast = cast_spell(&game, A, 90, TargetRef::Object(object(&game, TARGET)));
    let original = cast.mana_payment().unwrap().source;
    let activated = activate(cast.state(), A, 1, TargetRef::Stack(original), None);
    let entry = activated.ability_activation().unwrap().entry;
    let initial = resolve(activated.state());
    let first_decision = initial.state().pending_decision().unwrap().clone();
    let second = reply(initial.state(), CopyTargetChoice::Retain.into());
    let next_decision = second.state().pending_decision().unwrap();
    let RuleFrame::CopyTargets(frame) = next_decision.frame() else {
        panic!("copy frame")
    };
    assert_ne!(first_decision.id(), next_decision.id());
    assert_eq!(frame.entry(), entry);
    assert_eq!(frame.instruction_index(), 1);
    assert_eq!(second.state().execution().window().spell_copies().len(), 1);
    reject(
        second.state(),
        input(
            second.state(),
            A,
            GameCommand::Reply {
                decision: first_decision.id(),
                choice: CopyTargetChoice::Retain.into(),
            },
        ),
        GameError::WrongDecision,
    );
    let first_copy = *second
        .state()
        .execution()
        .window()
        .spell_copies()
        .keys()
        .next()
        .unwrap();
    let completed = reply(
        second.state(),
        CopyTargetChoice::Change(TargetRef::Object(object(second.state(), OTHER))).into(),
    );
    assert!(completed.state().pending_decision().is_none());
    assert_eq!(
        completed.state().execution().window().spell_copies().len(),
        2
    );
    assert_eq!(completed.state().execution().window().entries().len(), 3);
    let entries = completed.state().execution().window().entries();
    assert_eq!(entries[0].id(), original);
    assert_eq!(entries[1].id(), first_copy);
    assert_ne!(entries[2].id(), first_copy);
    assert_eq!(
        entries[1].targets().targets()[0].choice().target,
        TargetRef::Object(object(&game, TARGET))
    );
    assert_eq!(
        entries[2].targets().targets()[0].choice().target,
        TargetRef::Object(object(&game, OTHER))
    );
    assert_eq!(balances(completed.state()), balances(activated.state()));
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
                    vec![Instruction::CopySpell],
                    BodyTarget::SingleSpell(
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
                    vec![Instruction::CounterSpell],
                    BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                )],
            ),
            instant(
                90,
                A,
                ManaCost::one(ManaType::Red),
                vec![damage(3)],
                BodyTarget::SingleCreature,
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
fn ordered_damage_ability_uses_shared_choices_and_current_journal_appends() {
    let game = world_with(
        SourceCost::Sacrifice,
        ManaCost::one(ManaType::Red),
        vec![
            shield(1),
            multiplier(2),
            multiplier(3),
            damage(3),
            damage(2),
        ],
        BodyTarget::SingleCreature,
    );
    let target = object(&game, TARGET);
    let activated = activate(&game, A, 1, TargetRef::Object(target), None);
    let entry = activated.ability_activation().unwrap().entry;
    let grave = physical(activated.state(), 1).clone();
    let paid_balances = balances(activated.state());
    let pending = resolve(activated.state());
    let journal = pending.damage_journal();
    assert_eq!(journal.created.len(), 3);
    assert_eq!(journal.attempts.len(), 1);
    assert!(journal.preventions.is_empty());
    assert!(journal.applied.is_empty());
    for creation in &journal.created {
        match creation {
            DamageEffectCreation::Prevention {
                shield,
                instruction,
            } => {
                assert_eq!(shield.creator, entry);
                assert_eq!(*instruction, 0);
            }
            DamageEffectCreation::Multiplier {
                multiplier,
                instruction,
            } => {
                assert_eq!(multiplier.creator, entry);
                assert!([1, 2].contains(instruction));
            }
        }
    }
    let first_attempt = journal.attempts[0].id;
    damage_support::assert_started_origin(journal, entry);
    assert_eq!(
        journal.attempts[0].source.id(),
        DamageSourceId::Object(activated.ability_activation().unwrap().source.id())
    );
    let first_decision = pending.state().pending_decision().unwrap();
    let RuleFrame::Damage(spellwake_engine::effects::DamageDecision::Order(first_frame)) =
        first_decision.frame()
    else {
        panic!("damage frame")
    };
    assert_eq!(damage_support::resolution_origin(first_frame).0, entry);
    assert_eq!(damage_support::resolution_origin(first_frame).1, 3);
    assert_eq!(first_frame.chooser(), B);
    assert_eq!(first_frame.applications().len(), 3);
    block_activation(pending.state());
    reject(
        pending.state(),
        input(
            pending.state(),
            A,
            GameCommand::Reply {
                decision: first_decision.id(),
                choice: DecisionReply::DamageOrder(first_frame.applications()[0]),
            },
        ),
        GameError::WrongChooser,
    );
    let selected_shield = *first_frame
        .applications()
        .iter()
        .find(|effect| matches!(effect.effect, DamageEffectId::Prevention(_)))
        .unwrap();
    reject(
        pending.state(),
        input(
            pending.state(),
            B,
            GameCommand::Reply {
                decision: first_decision.id(),
                choice: CopyTargetChoice::Retain.into(),
            },
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    let after_shield = reply(pending.state(), DecisionReply::DamageOrder(selected_shield));
    let journal = after_shield.damage_journal();
    assert!(journal.created.is_empty());
    assert!(journal.attempts.is_empty());
    assert_eq!(journal.preventions.len(), 1);
    assert_eq!(journal.preventions[0].creator, entry);
    assert_eq!(journal.preventions[0].allocations[0].attempt, first_attempt);
    assert_eq!(journal.preventions[0].amount, 1.into());
    assert!(journal.applied.is_empty());
    let second_decision = after_shield.state().pending_decision().unwrap();
    assert_ne!(second_decision.id(), first_decision.id());
    let RuleFrame::Damage(spellwake_engine::effects::DamageDecision::Order(second_frame)) =
        second_decision.frame()
    else {
        panic!("damage frame")
    };
    assert_eq!(damage_support::resolution_origin(second_frame).1, 3);
    assert_eq!(second_frame.applications().len(), 2);
    let after_first_damage = reply(
        after_shield.state(),
        DecisionReply::DamageOrder(second_frame.applications()[0]),
    );
    let journal = after_first_damage.damage_journal();
    assert!(journal.created.is_empty());
    assert!(journal.preventions.is_empty());
    assert_eq!(journal.multiplications.len(), 2);
    assert_eq!(journal.applied.len(), 1);
    damage_support::assert_started_origin(journal, entry);
    assert_eq!(
        journal.applied[0].source.id(),
        DamageSourceId::Object(activated.ability_activation().unwrap().source.id())
    );
    assert_eq!(journal.applied[0].amount, 12.into());
    assert_eq!(
        journal.completions[0].outcome,
        DamageOutcome::Dealt(12.into())
    );
    assert_eq!(journal.attempts.len(), 1);
    assert_ne!(journal.attempts[0].id, first_attempt);
    damage_support::assert_started_origin(journal, entry);
    assert_eq!(
        journal.attempts[0].source.id(),
        DamageSourceId::Object(activated.ability_activation().unwrap().source.id())
    );
    for event in &journal.multiplications {
        assert_eq!(event.creator, entry);
        assert_eq!(event.attempt, first_attempt);
    }
    let third_decision = after_first_damage.state().pending_decision().unwrap();
    let RuleFrame::Damage(spellwake_engine::effects::DamageDecision::Order(third_frame)) =
        third_decision.frame()
    else {
        panic!("damage frame")
    };
    assert_eq!(damage_support::resolution_origin(third_frame).0, entry);
    assert_eq!(damage_support::resolution_origin(third_frame).1, 4);
    assert_eq!(third_frame.chooser(), B);
    let finished = reply(
        after_first_damage.state(),
        DecisionReply::DamageOrder(third_frame.applications()[1]),
    );
    let journal = finished.damage_journal();
    assert!(journal.created.is_empty());
    assert!(journal.attempts.is_empty());
    assert!(journal.preventions.is_empty());
    assert_eq!(journal.multiplications.len(), 2);
    assert_eq!(journal.applied.len(), 1);
    damage_support::assert_started_origin(journal, entry);
    assert_eq!(
        journal.applied[0].source.id(),
        DamageSourceId::Object(activated.ability_activation().unwrap().source.id())
    );
    assert_eq!(journal.applied[0].amount, 12.into());
    assert_eq!(
        journal.completions[0].outcome,
        DamageOutcome::Dealt(12.into())
    );
    assert!(finished.state().pending_decision().is_none());
    assert_eq!(
        finished
            .state()
            .execution()
            .creature(target)
            .unwrap()
            .unwrap()
            .damage,
        24.into()
    );
    assert_eq!(physical(finished.state(), 1), &grave);
    assert_eq!(balances(finished.state()), paid_balances);
    assert!(finished.state().execution().window().entries().is_empty());
}

#[test]
fn ability_program_defers_zero_toughness_and_lethal_checks_until_completion() {
    let game = world_with(
        SourceCost::Sacrifice,
        ManaCost::free(),
        vec![
            Instruction::CreateContinuousEffect {
                parts: spellwake_engine::characteristics::ResolvedPartsSpec(vec![
                    ResolvedPartSpec::SetPT(pt(1, 0)),
                ]),
                duration: spellwake_engine::characteristics::ResolvedDuration::NoStatedDuration,
            },
            Instruction::CreateContinuousEffect {
                parts: spellwake_engine::characteristics::ResolvedPartsSpec(vec![
                    ResolvedPartSpec::ModifyPT(pt(0, 2)),
                ]),
                duration: spellwake_engine::characteristics::ResolvedDuration::NoStatedDuration,
            },
        ],
        BodyTarget::SingleCreature,
    );
    let target = object(&game, OTHER);
    let activated = activate(&game, A, 1, TargetRef::Object(target), None);
    let entry = activated.ability_activation().unwrap().entry;
    let finished = resolve(activated.state());
    let derived = finished
        .state()
        .execution()
        .power_toughness(target)
        .unwrap()
        .unwrap();
    assert_eq!(object(finished.state(), OTHER), target);
    assert_eq!(derived.base, pt(1, 0));
    assert_eq!(derived.derived, pt(1, 2));
    assert_eq!(derived.applied_effects.len(), 2);
    for key in derived.applied_effects {
        let EffectInstanceKey::Resolved(id) = key else {
            panic!("this fixture has only resolved effects")
        };
        assert_eq!(
            finished
                .state()
                .execution()
                .continuous_effect(id)
                .unwrap()
                .creator,
            entry
        );
    }
    let game = world_with(
        SourceCost::Sacrifice,
        ManaCost::free(),
        vec![damage(3), Instruction::ReturnToOwnersHand],
        BodyTarget::SingleCreature,
    );
    let target = object(&game, OTHER);
    let activated = activate(&game, A, 1, TargetRef::Object(target), None);
    let finished = resolve(activated.state());
    assert_eq!(
        physical(finished.state(), OTHER).zone(),
        ObjectZone::Hand(A)
    );
    assert_ne!(object(finished.state(), OTHER), target);
    assert_eq!(
        finished.damage_journal().applied[0].target,
        DamageRecipient::Creature(target)
    );
    assert_eq!(finished.damage_journal().applied[0].amount, 3.into());
}

#[test]
fn paid_mana_color_choice_blocks_nonmana_activation_and_reply_emits_only_production() {
    let game = world();
    let chosen = accept(
        &game,
        A,
        GameCommand::ActivateMana {
            source: object(&game, 10),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
        },
    );
    no_records(&chosen);
    block_activation(chosen.state());
    let pending = chosen.state().pending_decision().unwrap();
    let completed = accept(
        chosen.state(),
        A,
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::ManaColor(ManaType::Red),
        },
    );
    assert!(completed.ability_activation().is_none());
    assert!(completed.mana_payment().is_none());
    assert_eq!(
        completed.mana_production().unwrap().source,
        object(&game, 10)
    );
    assert_eq!(
        completed.state().execution().mana(A, ManaType::Red),
        game.execution().mana(A, ManaType::Red).plus(&2.into())
    );
    assert_eq!(physical(completed.state(), 2).tapped(), Some(false));
}

#[test]
fn existing_spell_copy_and_damage_choices_block_new_activation_without_breaking_resume() {
    let game = world();
    let cast = cast_spell(&game, A, 90, TargetRef::Object(object(&game, TARGET)));
    let original = cast.mana_payment().unwrap().source;
    let copy = cast_spell(cast.state(), A, 93, TargetRef::Stack(original));
    let pending = resolve(copy.state());
    block_activation(pending.state());
    let completed = reply(pending.state(), CopyTargetChoice::Retain.into());
    assert_eq!(
        completed.state().execution().window().spell_copies().len(),
        1
    );
    assert_eq!(physical(completed.state(), 2).tapped(), Some(false));
    let game = setup(
        vec![
            artifact(
                2,
                A,
                A,
                vec![ability(
                    7,
                    SourceCost::Tap,
                    ManaCost::free(),
                    vec![damage(1)],
                    BodyTarget::SingleCreature,
                )],
            ),
            instant(
                90,
                A,
                ManaCost::one(ManaType::Red),
                vec![shield(1), multiplier(2), damage(3)],
                BodyTarget::SingleCreature,
            ),
            creature(TARGET, B, B, 100),
        ],
        pools(),
    );
    let cast = cast_spell(&game, A, 90, TargetRef::Object(object(&game, TARGET)));
    let pending = resolve(cast.state());
    block_activation(pending.state());
    let RuleFrame::Damage(spellwake_engine::effects::DamageDecision::Order(frame)) =
        pending.state().pending_decision().unwrap().frame()
    else {
        panic!("damage frame")
    };
    let completed = reply(
        pending.state(),
        DecisionReply::DamageOrder(frame.applications()[0]),
    );
    assert!(completed.state().pending_decision().is_none());
    assert_eq!(physical(completed.state(), 2).tapped(), Some(false));
}

#[test]
fn target_checking_blocks_activation_without_cost_or_entry_changes() {
    let game = world();
    let cast = cast_spell(&game, A, 90, TargetRef::Object(object(&game, TARGET)));
    let mut execution = cast.state().execution().clone();
    execution = execution.pass(execution.revision(), A).unwrap();
    execution = execution.settle_checks(execution.revision()).unwrap();
    execution = execution.pass(execution.revision(), B).unwrap();
    assert!(matches!(
        execution.window().phase(),
        WindowPhase::CheckingTargets(_)
    ));
    let saved = execution.clone();
    assert_eq!(
        execution.activate_ability(
            execution.revision(),
            A,
            object(cast.state(), 2),
            ActivatedAbilityId(7),
            TargetRef::Object(object(cast.state(), TARGET)),
            None
        ),
        Err(EffectError::Window(StackWindowError::NotInPriority))
    );
    assert_eq!(execution, saved);
}

#[test]
fn unrelated_cast_and_mana_activation_never_repeat_an_ability_record() {
    let game = world();
    let activated = activate(&game, A, 1, TargetRef::Object(object(&game, TARGET)), None);
    let saved_record = activated.ability_activation().unwrap().clone();
    let finished = resolve(activated.state());
    no_records(&finished);
    let cast = cast_spell(
        finished.state(),
        A,
        90,
        TargetRef::Object(object(finished.state(), TARGET)),
    );
    assert!(cast.ability_activation().is_none());
    assert!(cast.mana_payment().is_some());
    let finished = resolve(cast.state());
    no_records(&finished);
    let mana = accept(
        finished.state(),
        A,
        GameCommand::ActivateMana {
            source: object(finished.state(), 10),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
        },
    );
    no_records(&mana);
    let completed = accept(
        mana.state(),
        A,
        GameCommand::Reply {
            decision: mana.state().pending_decision().unwrap().id(),
            choice: DecisionReply::ManaColor(ManaType::Blue),
        },
    );
    assert!(completed.ability_activation().is_none());
    assert!(completed.mana_payment().is_none());
    assert!(completed.mana_production().is_some());
    assert_eq!(saved_record.source.tapped(), Some(false));
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
                    vec![Instruction::CopySpell],
                    BodyTarget::SingleSpell(
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
                    vec![Instruction::CopySpell],
                    BodyTarget::SingleSpell(
                        spellwake_engine::program::SpellFilter::copyable_instruction(),
                    ),
                )],
            ),
            instant(
                90,
                A,
                ManaCost::one(ManaType::Red),
                vec![damage(3)],
                BodyTarget::SingleCreature,
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
