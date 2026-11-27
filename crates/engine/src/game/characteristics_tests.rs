//! Frozen private faults at actual accepted root boundaries. These do not
//! represent player commands or historical save restoration.
use super::*;
use crate::{
    characteristics::{
        CharacteristicsError, InstructionSubtypeOperand, ResolvedDuration, ResolvedPartSpec,
        ResolvedPartsSpec, SubtypeOperation, TypeChangeSpec, TypeOperation,
    },
    effects::{
        DecisionReply, Instruction, Program, SeedCard, private_qualification::QualificationFault,
    },
    mana::ManaCost,
    object_identity::{CardId, ObjectDestination},
    program::{CardType, CreatureTypeSlot, ProgramKind},
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn context(seed: &GameSeed) -> GameContext {
    GameContext::new(
        "characteristic-faults".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "explicit-native-fixtures".into(),
        "characteristic-fault-v1".into(),
        "individual-turn".into(),
        seed.creature_vocabulary.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}
fn seed(program: Option<Program>) -> GameSeed {
    let mut cards = vec![SeedCard {
        card: CardId(1),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: Program::creature(Some(ManaCost::free()), 2.into(), 4.into(), BTreeSet::new()),
    }];
    if let Some(program) = program {
        cards.push(SeedCard {
            card: CardId(2),
            owner: A,
            destination: ObjectDestination::Hand,
            program,
        });
    }
    GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        vec![CardId(1)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        crate::test_support::creature_types::selected_vocabulary()
            .expect("explicit qualification vocabulary"),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    )
}
fn main_phase(seed: GameSeed) -> GameState {
    let mut game = GameState::initialize_scheduled(
        GameId(6160),
        context(&seed),
        seed,
        TurnStart {
            number: 1,
            step: Step::Untap,
        },
    )
    .unwrap()
    .into_state();
    for actor in [A, B] {
        assert_eq!(game.turn().unwrap().position().step, Step::Upkeep);
        game = accept(&game, actor, GameCommand::Pass);
    }
    assert_eq!(game.turn().unwrap().position().step, Step::PrecombatMain);
    game
}
fn input(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: game.basis.clone(),
        actor,
        command,
    }
}
fn accept(game: &GameState, actor: SeatId, command: GameCommand) -> GameState {
    let saved = game.clone();
    let next = game
        .advance(input(game, actor, command))
        .unwrap()
        .into_state();
    assert_eq!(game, &saved);
    next
}
fn object(game: &GameState, card: u64) -> ObjectId {
    game.execution
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn choice_reply(game: &GameState, label: &str) -> GameInput {
    let decision = game.pending_decision().unwrap();
    assert!(matches!(decision.frame(), RuleFrame::PromptChoice(_)));
    let value = game
        .execution
        .creature_vocabulary()
        .resolve_label(label)
        .unwrap();
    input(
        game,
        A,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(
                value,
            )),
        },
    )
}
fn accepted_first_choice() -> GameState {
    let q0 = CreatureTypeSlot::new(0);
    let q1 = CreatureTypeSlot::new(1);
    let program = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SINGLE_CREATURE,
            vec![
                Instruction::PromptChoice(Box::new(crate::toolbox::PromptChoiceSpec {
                    chooser: crate::toolbox::PlayerSelector::Resolving(
                        crate::program::ResolvingPlayer::Controller,
                    ),
                    domain: crate::toolbox::PromptDomain::Subtype {
                        kind: crate::toolbox::SubtypeChoiceKind::Creature,
                        excluding: BTreeSet::new(),
                        legacy_slot: Some(q0),
                    },
                    category: crate::toolbox::ChoiceCategory::Generic,
                    bind_to: crate::toolbox::BindingId(0),
                    bind_unchosen: None,
                    tag_partition: None,
                    auto_resolve_trivial: false,
                    option_branches: Vec::new(),
                })),
                Instruction::PromptChoice(Box::new(crate::toolbox::PromptChoiceSpec {
                    chooser: crate::toolbox::PlayerSelector::Resolving(
                        crate::program::ResolvingPlayer::Controller,
                    ),
                    domain: crate::toolbox::PromptDomain::Subtype {
                        kind: crate::toolbox::SubtypeChoiceKind::Creature,
                        excluding: BTreeSet::new(),
                        legacy_slot: Some(q1),
                    },
                    category: crate::toolbox::ChoiceCategory::Generic,
                    bind_to: crate::toolbox::BindingId(0),
                    bind_unchosen: None,
                    tag_partition: None,
                    auto_resolve_trivial: false,
                    option_branches: Vec::new(),
                })),
                {
                    struct _Tmp {
                        parts: crate::characteristics::ResolvedPartsSpec,
                        duration: crate::characteristics::ResolvedDuration,
                    }
                    let _t = _Tmp {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {
                            land_subtypes: SubtypeOperation::Keep,
                            supertypes: TypeOperation::Keep,
                            types: TypeOperation::Keep,
                            subtypes: SubtypeOperation::Set(InstructionSubtypeOperand::Chosen(q0)),
                        })]),
                        duration: ResolvedDuration::NoStatedDuration,
                    };
                    crate::program::Instruction::ApplyContinuous(Box::new(
                        crate::toolbox::ApplyContinuousSpec {
                            selector: Some(crate::toolbox::ObjectQuery::Targets),
                            payload: crate::toolbox::ContinuousPayload::ResolvedParts(Box::new(
                                _t.parts,
                            )),
                            duration: _t.duration,
                            condition: None,
                            one_shot: false,
                            as_emblem: None,
                            emblem_label: None,
                        },
                    ))
                },
            ],
        ),
    );
    let game = main_phase(seed(Some(program)));
    let game = accept(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 2),
            target: TargetRef::Object(object(&game, 1)),
        },
    );
    let game = accept(&game, A, GameCommand::Pass);
    let game = accept(&game, B, GameCommand::Pass);
    game.advance(choice_reply(&game, "Elf"))
        .unwrap()
        .into_state()
}
fn assert_failed_creation(fault: QualificationFault, expected: CharacteristicsError) {
    let mut game = accepted_first_choice();
    game.execution.qualification_set_fault(fault);
    let saved = game.clone();
    let decision = game.pending_decision().unwrap().clone();
    assert_eq!(
        game.advance(choice_reply(&game, "Goblin")),
        Err(GameError::Effect(EffectError::Characteristics(expected)))
    );
    assert_eq!(game, saved);
    assert_eq!(game.pending_decision(), Some(&decision));
    let locals = game.execution.resolution_state().unwrap();
    assert_eq!(locals.instruction(), 1);
    assert_eq!(locals.locals().len(), 1);
    assert_eq!(
        locals.locals().get(&CreatureTypeSlot::new(0)),
        game.execution
            .creature_vocabulary()
            .resolve_label("Elf")
            .as_ref()
    );
    assert!(!locals.locals().contains_key(&CreatureTypeSlot::new(1)));
    assert_eq!(game.execution.continuous_effects().count(), 0);
    assert!(game.outcome().is_none());
}
#[test]
fn cl_fault_effect_id_keeps_accepted_first_choice_and_original_second_frame() {
    assert_failed_creation(
        QualificationFault::ContinuousEffect(u64::MAX),
        CharacteristicsError::EffectIdentityExhausted,
    );
}
#[test]
fn cl_fault_effect_timestamp_keeps_accepted_first_choice_and_original_second_frame() {
    assert_failed_creation(
        QualificationFault::CharacteristicTimestamp(u64::MAX),
        CharacteristicsError::TimestampExhausted,
    );
}
#[test]
fn cl_fault_late_battlefield_timestamp_keeps_real_stack_entry_and_final_passer() {
    let mut program = Program::artifact(Some(ManaCost::free()), vec![]);
    let ProgramKind::Permanent(definition) = &mut program.kind else {
        unreachable!()
    };
    assert_eq!(program.card_types, BTreeSet::from([CardType::Artifact]));
    definition.printed_pt = Some(
        crate::characteristics::PowerToughness {
            power: 2.into(),
            toughness: 2.into(),
        }
        .into(),
    );
    let game = main_phase(seed(Some(program)));
    let game = accept(
        &game,
        A,
        GameCommand::CastUntargeted {
            spell: object(&game, 2),
            payment: None,
        },
    );
    let mut game = accept(&game, A, GameCommand::Pass);
    let stack_object = object(&game, 2);
    assert_eq!(
        game.execution
            .window()
            .cards()
            .get(stack_object)
            .unwrap()
            .zone(),
        ObjectZone::Stack
    );
    game.execution
        .qualification_set_fault(QualificationFault::CharacteristicTimestamp(u64::MAX));
    let saved = game.clone();
    assert_eq!(
        game.advance(input(&game, B, GameCommand::Pass)),
        Err(GameError::Effect(EffectError::Characteristics(
            CharacteristicsError::TimestampExhausted
        )))
    );
    assert_eq!(game, saved);
    assert_eq!(object(&game, 2), stack_object);
    assert_eq!(game.execution.window().entries().len(), 1);
    assert_eq!(
        game.execution.window().phase(),
        WindowPhase::Priority(PriorityPhase::Ready { holder: B })
    );
    assert!(game.outcome().is_none());
}
#[test]
fn cl_seed_order_duplicate_missing_and_foreign_are_constructor_errors() {
    for order in [
        vec![CardId(1), CardId(1)],
        vec![CardId(1)],
        vec![CardId(1), CardId(2), CardId(99)],
    ] {
        let mut seed = seed(None);
        seed.cards[0].program.card_types = BTreeSet::from([CardType::Artifact]);
        let ProgramKind::Permanent(definition) = &mut seed.cards[0].program.kind else {
            unreachable!()
        };
        definition.printed_pt = Some(
            crate::characteristics::PowerToughness {
                power: 4.into(),
                toughness: 4.into(),
            }
            .into(),
        );
        seed.cards.push(SeedCard {
            card: CardId(2),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program: Program::creature(Some(ManaCost::free()), 1.into(), 3.into(), BTreeSet::new()),
        });
        seed.initial_battlefield_order = order;
        let saved = seed.clone();
        assert_eq!(
            GameState::initialize(GameId(6161), context(&seed), seed.clone()),
            Err(GameError::Effect(EffectError::Characteristics(
                CharacteristicsError::InvalidInitialBattlefieldOrder
            )))
        );
        assert_eq!(seed, saved);
    }
}
