//! Actual native/component controls for predecessor capability refusal.
//! Mutable matching fixtures are intentionally not treated as admission authority.
use super::*;
use spellwake_engine::{
    combat::CombatRules,
    effects::{LibraryActionKind, LibraryTopReply, SeedCard},
    object_identity::ObjectDestination,
    priority::SeatId,
    program::{
        BodyTarget, ExecutableBody, Instruction, PlayerRelation, Program, ResolvingPlayer,
        TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
    },
    setup::GameSeed,
    token::{TokenCreation, TokenDefinition, TokenName},
};
const DENIED: &str = "ordinary predecessor projection excludes unrepresented native capabilities";
fn gain() -> Instruction {
    Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::gain(
        spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
        spellwake_engine::program::ValueExpr::literal(1),
    )))
}
fn library(kind: LibraryActionKind, amount: i64) -> Instruction {
    let player = ResolvingPlayer::Controller;
    let amount = spellwake_engine::program::ValueExpr::Literal(amount.into());
    match kind {
        LibraryActionKind::Scry => Instruction::MutatePlayer(Box::new(
            spellwake_engine::toolbox::MutatePlayerSpec::scry(player, amount),
        )),
        LibraryActionKind::Surveil => Instruction::MutatePlayer(Box::new(
            spellwake_engine::toolbox::MutatePlayerSpec::surveil(player, amount),
        )),
        LibraryActionKind::Mill => Instruction::MutatePlayer(Box::new(
            spellwake_engine::toolbox::MutatePlayerSpec::mill(player, amount),
        )),
    }
}
fn spell(body: Vec<Instruction>) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(BodyTarget::None, body),
    )
}
fn watcher(condition: TriggerSpec, body: Vec<Instruction>) -> Program {
    let mut program = Program::artifact(None, vec![]);
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, body),
    });
    program
}
fn initialize_control(
    program: Program,
    count: u64,
    watch: Option<Program>,
    copy: bool,
) -> (Fixture, NativeCandidate) {
    let mut fixture = Fixture::new(
        super::super::fixture::CaseId::G01,
        crate::creature_types::synthetic_vocabulary_for_rules("synthetic-ordinary-unit"),
    )
    .unwrap();
    let mut cards = vec![SeedCard {
        card: CardId(1),
        owner: A,
        destination: ObjectDestination::Hand,
        program,
    }];
    for n in 0..count {
        cards.push(SeedCard {
            card: CardId(10 + n),
            owner: A,
            destination: ObjectDestination::LibraryTop,
            program: Program::artifact(None, vec![]),
        });
    }
    let mut battlefield = vec![];
    if let Some(program) = watch {
        cards.push(SeedCard {
            card: CardId(100),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program,
        });
        battlefield.push(CardId(100));
    }
    if copy {
        cards.push(SeedCard {
            card: CardId(2),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::single_spell(
                        spellwake_engine::program::SpellFilter::copyable_instruction(),
                    ),
                    vec![Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                    ))],
                ),
            ),
        });
    }
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        battlefield,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        fixture.setup.seed.creature_vocabulary.clone(),
        fixture.setup.seed.name_rules.clone(),
    );
    fixture.setup.seed = seed.clone();
    let native = GameState::initialize(fixture.game, fixture.context.clone(), seed).unwrap();
    (fixture, native)
}
fn step(source: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    let before = source.clone();
    let input = GameInput {
        basis: source.basis().clone(),
        actor,
        command,
    };
    let first = source.advance(input.clone()).unwrap();
    assert_eq!(source, &before);
    assert_eq!(source.advance(input).unwrap(), first);
    first
}
fn cast_control(source: &GameState, card: u64, target: Option<TargetRef>) -> NativeCandidate {
    let spell = source
        .execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap();
    step(
        source,
        A,
        match target {
            Some(target) => GameCommand::Cast { spell, target },
            None => GameCommand::CastUntargeted {
                spell,
                payment: None,
            },
        },
    )
}
fn resolve_control(source: &GameState) -> NativeCandidate {
    let first = step(source, A, GameCommand::Pass);
    step(first.state(), B, GameCommand::Pass)
}
fn assert_all_predecessors(fixture: &Fixture, candidate: &NativeCandidate) {
    use super::super::{
        fixture::CaseId, interaction::InteractionCase, mandatory::MandatoryCase,
        optional::OptionalCase, teaching::TeachingCase,
    };
    for case in [
        CaseId::G01,
        CaseId::Teaching(TeachingCase::ALL[0]),
        CaseId::Interaction(InteractionCase::ALL[0]),
        CaseId::Mandatory(MandatoryCase::ALL[0]),
        CaseId::Optional(OptionalCase::ALL[0]),
    ] {
        let mut mutable_fixture = fixture.clone();
        mutable_fixture.case = case;
        assert!(
            candidate
                .state()
                .execution()
                .programs()
                .all(|(card, program)| mutable_fixture
                    .setup
                    .seed
                    .cards
                    .iter()
                    .any(|seed| seed.card == card && &seed.program == program)),
            "matching mutable fixture is not capability authority"
        );
        assert_eq!(
            check_ordinary_projection_scope(&mutable_fixture, candidate).unwrap_err(),
            DENIED
        );
        assert_eq!(
            header(&mutable_fixture, &History::default(), candidate.state()).unwrap_err(),
            DENIED
        );
        assert_eq!(
            facts(&mutable_fixture, &History::default(), None, candidate).unwrap_err(),
            DENIED
        );
    }
}

#[test]
fn completed_positive_empty_actions_refuse_report1_through4_before_generic_serialization() {
    for kind in [
        LibraryActionKind::Scry,
        LibraryActionKind::Surveil,
        LibraryActionKind::Mill,
    ] {
        let (fixture, initial) = initialize_control(spell(vec![library(kind, 1)]), 0, None, false);
        let cast = cast_control(initial.state(), 1, None);
        let complete = resolve_control(cast.state());
        assert!(complete.state().pending_decision().is_none());
        assert!(complete.state().execution().resolution_state().is_none());
        let action = complete
            .rules_journal()
            .records
            .iter()
            .filter_map(|r| match r {
                RulesRecord::Event(e) => Some(e),
                _ => None,
            })
            .flat_map(|e| &e.occurrences)
            .find_map(|o| match &o.value {
                RulesOccurrence::LibraryAction(a) => Some(a),
                _ => None,
            })
            .unwrap();
        assert_eq!(action.kind, kind);
        assert!(action.looked_top_first.is_empty());
        assert_all_predecessors(&fixture, &complete);
    }
}

#[test]
fn zero_unreachable_and_inactive_condition_programs_refuse_without_new_frames_or_facts() {
    for kind in [
        LibraryActionKind::Scry,
        LibraryActionKind::Surveil,
        LibraryActionKind::Mill,
    ] {
        for body in [
            vec![library(kind, 0)],
            vec![
                Instruction::Jump { destination: 2 },
                library(kind, 0),
                gain(),
            ],
        ] {
            let (fixture, initial) = initialize_control(spell(body), 0, None, false);
            assert!(initial.state().pending_decision().is_none());
            assert!(!initial.rules_journal().records.iter().any(|r|matches!(r,RulesRecord::Event(e) if e.occurrences.iter().any(|o|matches!(o.value,RulesOccurrence::LibraryAction(_))))));
            assert_all_predecessors(&fixture, &initial);
        }
    }
    for condition in [
        spellwake_engine::program::TriggerSpec::player_action(
            PlayerRelation::Controller,
            spellwake_engine::program::PlayerTriggerAction::Scried,
        ),
        spellwake_engine::program::TriggerSpec::player_action(
            PlayerRelation::Opponent,
            spellwake_engine::program::PlayerTriggerAction::Surveilled,
        ),
    ] {
        let (fixture, initial) =
            initialize_control(watcher(condition, vec![gain()]), 0, None, false);
        assert_all_predecessors(&fixture, &initial);
    }
}

#[test]
fn nested_uncreated_token_templates_are_checked_as_full_typed_programs() {
    let vocabulary =
        crate::creature_types::synthetic_vocabulary_for_rules("synthetic-ordinary-unit");
    let name = || {
        TokenName::Specified(
            spellwake_contracts::names::EnglishCardName::new("Capability control".into()).unwrap(),
        )
    };
    let inner = TokenDefinition::new(
        watcher(
            spellwake_engine::program::TriggerSpec::player_action(
                PlayerRelation::Controller,
                spellwake_engine::program::PlayerTriggerAction::Scried,
            ),
            vec![library(LibraryActionKind::Scry, 0)],
        ),
        name(),
        &vocabulary,
    )
    .unwrap();
    let middle = TokenDefinition::new(
        watcher(
            spellwake_engine::program::TriggerSpec::player_action(
                PlayerRelation::Controller,
                spellwake_engine::program::PlayerTriggerAction::TokenCreated,
            ),
            vec![Instruction::CreateTokens {
                creations: vec![TokenCreation::one(inner)],
            }],
        ),
        name(),
        &vocabulary,
    )
    .unwrap();
    let mut creation = TokenCreation::one(middle);
    creation.amount = spellwake_engine::program::ValueExpr::Literal(0.into());
    let (fixture, initial) = initialize_control(
        spell(vec![
            Instruction::Jump { destination: 2 },
            Instruction::CreateTokens {
                creations: vec![creation],
            },
            gain(),
        ]),
        0,
        None,
        false,
    );
    assert_all_predecessors(&fixture, &initial);
}

#[test]
fn completed_mill_reason_and_completed_action_trigger_causes_refuse_without_library_frames() {
    let (fixture, initial) = initialize_control(
        spell(vec![library(LibraryActionKind::Mill, 1)]),
        1,
        None,
        false,
    );
    let cast = cast_control(initial.state(), 1, None);
    let complete = resolve_control(cast.state());
    assert!(complete.state().pending_decision().is_none());
    let movement=complete.rules_journal().records.iter().filter_map(|r|match r{RulesRecord::Event(e)=>Some(e),_=>None}).flat_map(|e|&e.occurrences).find(|o|matches!(&o.value,RulesOccurrence::ZoneChange(z) if matches!(z.reason,spellwake_engine::occurrence::ZoneReason::Mill{..}))).unwrap();
    assert!(movement.value.check_current10().is_err());
    assert_all_predecessors(&fixture, &complete);
    for kind in [LibraryActionKind::Scry, LibraryActionKind::Surveil] {
        let condition = if kind == LibraryActionKind::Scry {
            spellwake_engine::program::TriggerSpec::player_action(
                PlayerRelation::Controller,
                spellwake_engine::program::PlayerTriggerAction::Scried,
            )
        } else {
            spellwake_engine::program::TriggerSpec::player_action(
                PlayerRelation::Controller,
                spellwake_engine::program::PlayerTriggerAction::Surveilled,
            )
        };
        let (fixture, initial) = initialize_control(
            spell(vec![library(kind, 1)]),
            0,
            Some(watcher(condition, vec![gain()])),
            false,
        );
        let cast = cast_control(initial.state(), 1, None);
        let complete = resolve_control(cast.state());
        assert!(complete.state().pending_decision().is_none());
        let trigger = complete
            .rules_journal()
            .records
            .iter()
            .find_map(|r| match r {
                RulesRecord::TriggerDiscovered(w) => Some(w),
                _ => None,
            })
            .unwrap();
        assert!(trigger.causes.check_current10().is_err());
        assert!(
            complete
                .state()
                .execution()
                .retained_stack_payloads()
                .any(|p| p.bindings.is_some_and(|b| b.trigger.is_some())
                    && p.check_current10().is_err())
        );
        assert_all_predecessors(&fixture, &complete);
    }
}

#[test]
fn explicit_library_frame_and_reply_still_refuse_while_native_result_is_unchanged() {
    let (fixture, initial) = initialize_control(
        spell(vec![library(LibraryActionKind::Scry, 1)]),
        2,
        None,
        false,
    );
    let cast = cast_control(initial.state(), 1, None);
    let pending = resolve_control(cast.state());
    let source = pending.state();
    let RuleFrame::LibraryTop(frame) = source.pending_decision().unwrap().frame() else {
        panic!("actual library frame")
    };
    let choice = DecisionReply::LibraryTop(LibraryTopReply::Scry {
        top_first: frame.domain_top_first().to_vec(),
        bottom_top_first: vec![],
    });
    let expected = step(
        source,
        A,
        GameCommand::Reply {
            decision: source.pending_decision().unwrap().id(),
            choice: choice.clone(),
        },
    );
    let saved = source.clone();
    assert_eq!(
        reply(&fixture, &History::default(), &choice).unwrap_err(),
        DENIED
    );
    assert_all_predecessors(&fixture, &pending);
    assert_eq!(source, &saved);
    assert_eq!(
        step(
            source,
            A,
            GameCommand::Reply {
                decision: source.pending_decision().unwrap().id(),
                choice
            }
        ),
        expected
    );
}

#[test]
fn owned_copied_trigger_definitions_are_borrowed_from_actual_retained_payloads() {
    let mut program = spell(vec![gain()]);
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::self_cast(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![library(LibraryActionKind::Scry, 0)]),
    });
    let (fixture, initial) = initialize_control(program, 0, None, true);
    let original = cast_control(initial.state(), 1, None);
    let entry = original.state().execution().window().entries()[0].id();
    let copier = cast_control(original.state(), 2, Some(TargetRef::Stack(entry)));
    let created = resolve_control(copier.state());
    let copy = created
        .state()
        .execution()
        .window()
        .entries()
        .iter()
        .find(|e| matches!(e.kind(), StackKind::SpellCopy { .. }))
        .unwrap()
        .id();
    let payload = created
        .state()
        .execution()
        .retained_stack_payloads()
        .find(|p| p.entry == copy)
        .unwrap();
    assert!(
        payload.body.unwrap().check_current10().is_ok(),
        "clean main body alone is insufficient"
    );
    assert_eq!(payload.copied_triggered.unwrap().len(), 1);
    assert!(payload.check_current10().is_err());
    assert_all_predecessors(&fixture, &created);
}
