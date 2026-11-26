#![allow(dead_code, unused_imports)]
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
        BodyTarget, CardType, ExecutableBody, Instruction, PermanentDefinition, PlayerRelation,
        Program, ProgramKind, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
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
        amount: spellwake_engine::program::ValueExpr::Literal(amount.into()),
    }
}
fn damage(amount: i64) -> Instruction {
    Instruction::Damage {
        amount: amount.into(),
    }
}
fn trigger(condition: TriggerSpec) -> TriggeredAbility {
    TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition,
        body: body(BodyTarget::None, vec![draw(1)]),
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
    value.triggered.push(trigger(
        spellwake_engine::program::TriggerSpec::becomes_tapped(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
    ));
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
        spellwake_engine::program::ObjectFilter::self_object(),
        spellwake_engine::program::ObjectFilter::creature(),
        spellwake_engine::program::ObjectFilter::creature().other_than_ability_source(),
        spellwake_engine::program::ObjectFilter::creature().controlled(),
        spellwake_engine::program::ObjectFilter::creature()
            .controlled()
            .other_than_ability_source(),
    ] {
        conditions.extend([
            spellwake_engine::program::TriggerSpec::enters(relation),
            spellwake_engine::program::TriggerSpec::leaves_battlefield(relation),
            spellwake_engine::program::TriggerSpec::dies(relation),
            spellwake_engine::program::TriggerSpec::becomes_tapped(relation),
        ]);
    }
    for player in [
        PlayerRelation::Controller,
        PlayerRelation::Opponent,
        PlayerRelation::AnyParticipant,
    ] {
        conditions.extend([
            spellwake_engine::program::TriggerSpec::player_action(
                player,
                spellwake_engine::program::PlayerTriggerAction::Discarded,
            ),
            spellwake_engine::program::TriggerSpec::step_began(Step::Draw, player),
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
            spellwake_engine::program::SpellFilter::any(),
            spellwake_engine::program::SpellFilter::copyable_instruction(),
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
            if filter == spellwake_engine::program::SpellFilter::copyable_instruction() {
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
                    BodyTarget::SingleSpell(
                        spellwake_engine::program::SpellFilter::copyable_instruction(),
                    ),
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
                    BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
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

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("cr-20260925")
}
