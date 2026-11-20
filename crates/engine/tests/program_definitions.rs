//! Common definition admission and explicit body contracts, without full-card claims.
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{PowerToughness, PowerToughnessCounters},
    combat::CombatRules,
    effects::{EffectError, EffectWindow, SeedCard},
    game::{GameCommand, GameContext, GameError, GameId, GameInput, GameState},
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::SeatId,
    program::{
        BodyTarget, CardType, ExecutableBody, Instruction, ObjectRelation, OrdinaryCondition,
        PermanentDefinition, PlayerRelation, Program, ProgramKind, SpellTargetFilter,
        TriggeredAbility, TriggeredAbilityId,
    },
    setup::GameSeed,
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use std::collections::{BTreeMap, BTreeSet};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn body(target: BodyTarget, instructions: Vec<Instruction>) -> ExecutableBody {
    ExecutableBody::new(target, instructions)
}
fn draw(amount: i64) -> Instruction {
    Instruction::DrawController {
        amount: spellwake_engine::program::InstructionAmount::Literal(amount.into()),
    }
}
fn damage(amount: i64) -> Instruction {
    Instruction::Damage {
        amount: amount.into(),
    }
}
fn trigger(condition: OrdinaryCondition) -> TriggeredAbility {
    TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition,
        body: body(BodyTarget::None, vec![draw(1)]),
    }
}
fn activation(source_cost: SourceCost) -> ActivatedAbility {
    ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost,
        body: body(
            BodyTarget::SingleCreature,
            vec![Instruction::ReturnToOwnersHand],
        ),
    }
}
fn forest() -> Program {
    Program::land(
        std::collections::BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
        }],
    )
}
fn creature() -> Program {
    Program::creature(None, 1.into(), 1.into(), std::collections::BTreeSet::new())
}
fn reach() -> Program {
    Program::instant(
        ManaCost::one(ManaType::Blue),
        body(BodyTarget::None, vec![draw(1)]),
    )
}
fn leave_sac() -> Program {
    let mut value = Program::artifact(None, vec![activation(SourceCost::Sacrifice)]);
    value
        .triggered
        .push(trigger(OrdinaryCondition::LeavesBattlefield(
            ObjectRelation::SelfObject,
        )));
    value
}
fn tap_self() -> Program {
    let mut value = Program::land(
        std::collections::BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::ChooseColor {
                amount: 1.into(),
                colors: BTreeSet::from([ManaType::Green, ManaType::Blue]),
            },
        }],
    );
    value
        .triggered
        .push(trigger(OrdinaryCondition::BecomesTapped(
            ObjectRelation::SelfObject,
        )));
    value
}
fn staff() -> Program {
    let mut ability = activation(SourceCost::Tap);
    ability.body = body(BodyTarget::SingleCreature, vec![damage(1)]);
    let mut value = Program::artifact(Some(ManaCost::generic(6.into())), vec![ability]);
    value.triggered.push(trigger(OrdinaryCondition::StepBegan {
        step: Step::Upkeep,
        player: PlayerRelation::Controller,
    }));
    value
}
fn permanent(value: &mut Program) -> &mut PermanentDefinition {
    let ProgramKind::Permanent(value) = &mut value.kind else {
        panic!("permanent fixture")
    };
    value
}
fn seed(program: Program) -> Result<EffectWindow, EffectError> {
    EffectWindow::new(GameSeed::with_uniform_life(
        vec![A, B],
        A,
        vec![SeedCard {
            card: CardId(1),
            owner: A,
            destination: ObjectDestination::Hand,
            program,
        }],
        vec![],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        test_vocabulary(),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    ))
}

#[test]
fn nineteen_admitted_definitions_keep_their_distinct_characteristics_and_capabilities() {
    let mut sage = Program::creature(
        Some(ManaCost {
            generic: 1.into(),
            required: BTreeMap::from([(ManaType::Blue, 1.into())]),
        }),
        1.into(),
        1.into(),
        std::collections::BTreeSet::new(),
    );
    sage.triggered
        .push(trigger(OrdinaryCondition::Dies(ObjectRelation::SelfObject)));
    let mut cases = vec![
        ("DA-P01", forest()),
        ("DA-P02", creature()),
        ("DA-P03", leave_sac()),
        ("DA-P04", tap_self()),
        ("DA-P05", reach()),
        ("DA-P06", staff()),
        ("DA-P07", sage),
        (
            "DA-P08",
            Program::instant(
                ManaCost::one(ManaType::Blue),
                body(
                    BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
                    vec![Instruction::CounterSpell],
                ),
            ),
        ),
        (
            "DA-P09",
            Program::creature(
                None,
                (-1).into(),
                (-1).into(),
                std::collections::BTreeSet::new(),
            ),
        ),
    ];
    // Reviewed successors retain the ten historical DA negative logical roles.
    for (id, types) in [
        ("CL-ADMIT-DA-P02", [CardType::Artifact, CardType::Creature]),
        ("CL-ADMIT-DA-P03", [CardType::Land, CardType::Creature]),
    ] {
        let mut value = creature();
        value.card_types = types.into();
        cases.push((id, value));
    }
    let mut value = creature();
    permanent(&mut value).printed_pt = None;
    cases.push(("CL-ADMIT-DA-P04", value));
    for (id, mut value) in [("CL-ADMIT-DA-P05", staff()), ("CL-ADMIT-DA-P06", forest())] {
        permanent(&mut value).printed_pt = Some(
            PowerToughness {
                power: 1.into(),
                toughness: 1.into(),
            }
            .into(),
        );
        cases.push((id, value));
    }
    let mut value = forest();
    value.mana.clear();
    cases.push(("CL-ADMIT-DA-P08", value));
    let mut value = forest();
    value.card_types = [CardType::Artifact].into();
    cases.push(("CL-ADMIT-DA-P09", value));
    let mut value = forest();
    let donor = creature();
    value.card_types = donor.card_types.clone();
    value.creature_subtypes = donor.creature_subtypes.clone();
    *permanent(&mut value) = donor.permanent().unwrap().clone();
    cases.push(("CL-ADMIT-DA-P10", value));
    let mut value = leave_sac();
    value.card_types = [CardType::Land].into();
    value.mana = forest().mana;
    cases.push(("CL-ADMIT-DA-P11", value));
    let mut value = leave_sac();
    let donor = creature();
    value.card_types = donor.card_types.clone();
    value.creature_subtypes = donor.creature_subtypes.clone();
    *permanent(&mut value) = donor.permanent().unwrap().clone();
    cases.push(("CL-ADMIT-DA-P12", value));
    assert_eq!(cases.len(), 19);
    for (id, program) in cases {
        assert!(seed(program).is_ok(), "{id}");
    }
}

#[test]
fn thirteen_malformed_or_unsupported_definitions_reject_the_entire_seed() {
    let mut cases = Vec::new();
    let mut value = creature();
    value.card_types.clear();
    cases.push(("DA-N01", value));
    let mut value = forest();
    permanent(&mut value).casting_cost = Some(ManaCost::free());
    cases.push(("DA-N07", value));
    let mut value = reach();
    value.mana = forest().mana;
    cases.push(("DA-N13", value));
    let mut value = reach();
    value.activated = leave_sac().activated;
    cases.push(("DA-N14", value));
    let mut value = reach();
    value.triggered = tap_self().triggered;
    cases.push(("DA-N15", value));
    let mut value = forest();
    value.mana.push(value.mana[0].clone());
    cases.push(("DA-N16", value));
    let mut value = leave_sac();
    value.activated.push(value.activated[0].clone());
    cases.push(("DA-N17", value));
    let mut value = tap_self();
    value.triggered.push(value.triggered[0].clone());
    cases.push(("DA-N18", value));
    let mut value = leave_sac();
    value.activated[0].body = body(BodyTarget::None, vec![draw(1)]);
    cases.push(("DA-N19", value));
    cases.push((
        "DA-N21",
        Program::instant(ManaCost::free(), body(BodyTarget::None, vec![draw(-1)])),
    ));
    let mut value = tap_self();
    value.triggered[0].body.instructions.clear();
    cases.push(("DA-N22", value));
    let mut value = creature();
    permanent(&mut value).casting_cost = Some(ManaCost::generic((-1).into()));
    cases.push(("DA-N23", value));
    let mut value = leave_sac();
    value.activated[0].mana_cost = ManaCost::generic((-1).into());
    cases.push(("DA-N24", value));
    assert_eq!(cases.len(), 13);
    for (id, value) in cases {
        assert_eq!(seed(value), Err(EffectError::InvalidSetup), "{id}");
    }
}

#[test]
fn historical_da_n20_has_a_current_targeted_trigger_admission_successor() {
    // The historical input remains a negative in its sealed source epoch.
    let mut value = tap_self();
    value.triggered[0].body = body(BodyTarget::SingleCreature, vec![damage(1)]);
    assert!(seed(value).is_ok());
}

#[test]
fn typed_ordinary_conditions_allow_only_the_shared_mandatory_body_contract() {
    let mut conditions = Vec::new();
    for relation in [
        ObjectRelation::SelfObject,
        ObjectRelation::AnyCreature,
        ObjectRelation::AnotherCreature,
        ObjectRelation::ControlledCreature,
        ObjectRelation::AnotherControlledCreature,
    ] {
        conditions.extend([
            OrdinaryCondition::Enters(relation),
            OrdinaryCondition::LeavesBattlefield(relation),
            OrdinaryCondition::Dies(relation),
            OrdinaryCondition::BecomesTapped(relation),
        ]);
    }
    for player in [
        PlayerRelation::Controller,
        PlayerRelation::Opponent,
        PlayerRelation::AnyParticipant,
    ] {
        conditions.extend([
            OrdinaryCondition::Discarded(player),
            OrdinaryCondition::StepBegan {
                step: Step::Draw,
                player,
            },
        ]);
    }
    for condition in conditions {
        // A noncreature Dies(Self) definition is admitted but does not make it a creature.
        let mut value = Program::artifact(None, vec![]);
        value.triggered.push(trigger(condition));
        let state = seed(value).unwrap();
        let id = state.window().cards().current_identity(CardId(1)).unwrap();
        assert!(state.copiable_power_toughness(id).is_none());
    }
    let mut distinct = tap_self();
    distinct.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(2),
        ..distinct.triggered[0].clone()
    });
    assert!(seed(distinct).is_ok());
}

#[test]
fn body_validation_checks_every_scalar_and_all_consuming_operands() {
    for (target, instructions) in [
        (BodyTarget::None, vec![draw(0)]),
        (
            BodyTarget::SingleCreature,
            vec![draw(2), damage(1), draw(0)],
        ),
        (
            BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
            vec![draw(1), Instruction::CounterSpell, Instruction::CopySpell],
        ),
        (
            BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
            vec![Instruction::CounterSpell, draw(1)],
        ),
    ] {
        assert!(
            body(target, instructions)
                .validate(&test_vocabulary())
                .is_ok()
        );
    }
    for (target, instructions) in [
        (BodyTarget::None, vec![]),
        (BodyTarget::None, vec![damage(1)]),
        (BodyTarget::SingleCreature, vec![draw(1)]),
        (
            BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
            vec![draw(1)],
        ),
        (
            BodyTarget::SingleCreature,
            vec![draw(1), Instruction::CopySpell],
        ),
        (
            BodyTarget::SingleCreature,
            vec![damage(1), Instruction::CounterSpell],
        ),
        (
            BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
            vec![Instruction::CopySpell, damage(1)],
        ),
        (BodyTarget::SingleCreature, vec![damage(1), damage(0)]),
        (
            BodyTarget::SingleCreature,
            vec![
                damage(1),
                Instruction::CreatePreventionShield {
                    amount: (-1).into(),
                },
            ],
        ),
        (
            BodyTarget::SingleCreature,
            vec![
                damage(1),
                Instruction::CreateDamageMultiplier { factor: 0.into() },
            ],
        ),
        (
            BodyTarget::SingleCreature,
            vec![
                damage(1),
                Instruction::PutPowerToughnessCounters {
                    counters: PowerToughnessCounters {
                        plus_one: (-1).into(),
                        minus_one: 0.into(),
                    },
                },
            ],
        ),
        (
            BodyTarget::SingleCreature,
            vec![
                damage(1),
                Instruction::PutPowerToughnessCounters {
                    counters: PowerToughnessCounters {
                        plus_one: 0.into(),
                        minus_one: 0.into(),
                    },
                },
            ],
        ),
        (BodyTarget::None, vec![draw(1), draw(-1)]),
    ] {
        assert!(
            body(target, instructions)
                .validate(&test_vocabulary())
                .is_err()
        );
    }
}

fn game(cards: Vec<SeedCard>) -> GameState {
    GameState::initialize_scheduled(
        GameId(891),
        GameContext::new(
            "common-program-tests".into(),
            "cr-20260925".into(),
            "synthetic".into(),
            "explicit-body".into(),
            "unrestricted-ffa".into(),
            test_vocabulary().id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap(),
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
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                test_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
        TurnStart {
            number: 3,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state()
}
fn card(id: u64, owner: SeatId, program: Program, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program,
    }
}
fn object(game: &GameState, id: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(id))
        .unwrap()
}
fn act(game: &GameState, actor: SeatId, command: GameCommand) -> Result<GameState, GameError> {
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    })
    .map(|v| v.into_state())
}

#[test]
fn spell_support_filter_is_explicit_and_independent_of_counter_copy_order() {
    for instructions in [
        vec![Instruction::CounterSpell, Instruction::CopySpell],
        vec![Instruction::CopySpell, Instruction::CounterSpell],
    ] {
        for filter in [
            SpellTargetFilter::AnySpell,
            SpellTargetFilter::CopyableInstructionSpell,
        ] {
            let game = game(vec![
                card(
                    1,
                    A,
                    Program::artifact(Some(ManaCost::free()), vec![]),
                    ObjectDestination::Hand,
                ),
                card(
                    2,
                    B,
                    Program::instant(
                        ManaCost::free(),
                        body(BodyTarget::SingleSpell(filter), instructions.clone()),
                    ),
                    ObjectDestination::Hand,
                ),
            ]);
            let game = act(
                &game,
                A,
                GameCommand::CastUntargeted {
                    spell: object(&game, 1),
                    payment: None,
                },
            )
            .unwrap();
            let original = game.execution().window().entries()[0].id();
            let game = act(&game, A, GameCommand::Pass).unwrap();
            let saved = game.clone();
            let result = act(
                &game,
                B,
                GameCommand::Cast {
                    spell: object(&game, 2),
                    target: TargetRef::Stack(original),
                },
            );
            if filter == SpellTargetFilter::CopyableInstructionSpell {
                assert_eq!(result, Err(GameError::Effect(EffectError::IllegalTarget)));
                assert_eq!(game, saved);
            } else {
                let game = result.unwrap();
                let game = act(&game, B, GameCommand::Pass).unwrap();
                let game = act(&game, A, GameCommand::Pass).unwrap();
                assert_eq!(
                    game.execution()
                        .window()
                        .cards()
                        .get(object(&game, 1))
                        .unwrap()
                        .zone(),
                    ObjectZone::Graveyard(A)
                );
                assert!(game.execution().window().entries().is_empty());
            }
        }
    }
}

#[test]
fn targetless_spell_rejects_supplied_target_but_is_copyable_and_counterable() {
    let game = game(vec![
        card(
            1,
            A,
            Program::instant(ManaCost::free(), body(BodyTarget::None, vec![draw(1)])),
            ObjectDestination::Hand,
        ),
        card(
            2,
            B,
            Program::instant(
                ManaCost::free(),
                body(
                    BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
                    vec![Instruction::CopySpell],
                ),
            ),
            ObjectDestination::Hand,
        ),
        card(
            3,
            B,
            Program::instant(
                ManaCost::free(),
                body(
                    BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
                    vec![Instruction::CounterSpell],
                ),
            ),
            ObjectDestination::Hand,
        ),
    ]);
    let saved = game.clone();
    assert_eq!(
        act(
            &game,
            A,
            GameCommand::Cast {
                spell: object(&game, 1),
                target: TargetRef::Player(B)
            }
        ),
        Err(GameError::Effect(EffectError::UnexpectedTarget))
    );
    assert_eq!(game, saved);
    let game = act(
        &game,
        A,
        GameCommand::CastUntargeted {
            spell: object(&game, 1),
            payment: None,
        },
    )
    .unwrap();
    let original = game.execution().window().entries()[0].id();
    let game = act(&game, A, GameCommand::Pass).unwrap();
    let saved = game.clone();
    let copied = act(
        &game,
        B,
        GameCommand::Cast {
            spell: object(&game, 2),
            target: TargetRef::Stack(original),
        },
    )
    .unwrap();
    let copied = act(&copied, B, GameCommand::Pass).unwrap();
    let copied = act(&copied, A, GameCommand::Pass).unwrap();
    assert!(copied.pending_decision().is_none());
    assert_eq!(copied.execution().window().entries().len(), 2);
    assert_eq!(copied.execution().window().spell_copies().len(), 1);
    assert!(
        copied.execution().window().entries()[1]
            .targets()
            .targets()
            .is_empty()
    );
    assert_eq!(game, saved);
    let game = act(
        &game,
        B,
        GameCommand::Cast {
            spell: object(&game, 3),
            target: TargetRef::Stack(original),
        },
    )
    .unwrap();
    let game = act(&game, B, GameCommand::Pass).unwrap();
    let game = act(&game, A, GameCommand::Pass).unwrap();
    assert!(game.execution().window().entries().is_empty());
    assert!(
        game.execution()
            .window()
            .cards()
            .iter()
            .all(|v| v.zone() != ObjectZone::Stack)
    );
}

#[test]
fn all_illegal_body_gate_precedes_even_a_leading_controller_instruction() {
    let game = game(vec![
        card(
            1,
            A,
            Program::instant(
                ManaCost::free(),
                body(BodyTarget::SingleCreature, vec![draw(1), damage(2)]),
            ),
            ObjectDestination::Hand,
        ),
        card(
            2,
            B,
            Program::instant(
                ManaCost::free(),
                body(
                    BodyTarget::SingleCreature,
                    vec![Instruction::ReturnToOwnersHand],
                ),
            ),
            ObjectDestination::Hand,
        ),
        card(
            3,
            B,
            creature(),
            ObjectDestination::Battlefield { controller: B },
        ),
    ]);
    let target = TargetRef::Object(object(&game, 3));
    let game = act(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 1),
            target,
        },
    )
    .unwrap();
    let game = act(&game, A, GameCommand::Pass).unwrap();
    let game = act(
        &game,
        B,
        GameCommand::Cast {
            spell: object(&game, 2),
            target,
        },
    )
    .unwrap();
    let game = act(&game, B, GameCommand::Pass).unwrap();
    let game = act(&game, A, GameCommand::Pass).unwrap();
    let game = act(&game, A, GameCommand::Pass).unwrap();
    let game = act(&game, B, GameCommand::Pass).unwrap();
    assert!(game.execution().window().entries().is_empty());
    assert!(game.execution().state_based_losses().is_empty());
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("cr-20260925")
}
