//! Synthetic structural controls using the explicit genuine vocabulary input.
//! Receipt labels are synthetic; these tests confer no source-card qualification.
use super::super::super::{creature_types, packet::AliasContext};
use super::super::{
    CallPoint, CaptureScope, Phase, Recorder, RegistryWitness, ReplayEvidence, SourceCapture,
    SourceRef,
};
use super::*;
use serde_json::json;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    combat::CombatRules,
    effects::{DecisionReply, SeedCard},
    game::{GameContext, GameId},
    mana::ManaCost,
    object_identity::{CardId, ObjectDestination},
    priority::{PriorityPhase, SeatId},
    program::{
        AffectedPlayers, PermanentTypeFilter, PlayerRelation, TriggerGrouping, TriggeredAbilityId,
    },
    stack_window::WindowPhase,
    token::{TokenCreation, TokenDefinition, TokenName},
    turn::{Step, TurnStart},
};
use std::{collections::BTreeSet, sync::Arc};

pub(crate) const A: SeatId = SeatId(41);
pub(crate) const B: SeatId = SeatId(3);

pub(crate) fn selection(sacrifice: bool) -> Instruction {
    if sacrifice {
        Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
            AffectedPlayers::EachPlayer,
            spellwake_engine::program::ValueExpr::Literal(1.into()),
            PermanentTypeFilter {
                any_types: Default::default(),
                required_types: BTreeSet::new(),
                forbidden_types: BTreeSet::new(),
                object_filter: None,
            },
        )))
    } else {
        Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
            AffectedPlayers::EachPlayer,
            spellwake_engine::program::ValueExpr::Literal(1.into()),
        )))
    }
}
pub(crate) fn creature() -> Program {
    Program::creature(None, 2.into(), 2.into(), BTreeSet::new())
}
pub(crate) fn seed(program: Program) -> (GameContext, GameSeed) {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "synthetic-applicability".into(),
        vocabulary.rules_revision().into(),
        "synthetic-not-catalogue".into(),
        "native-observer-control".into(),
        "mechanism-only".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let cards = vec![
        SeedCard {
            card: CardId(1),
            owner: A,
            destination: ObjectDestination::Hand,
            program,
        },
        SeedCard {
            card: CardId(2),
            owner: A,
            destination: ObjectDestination::Hand,
            program: creature(),
        },
        SeedCard {
            card: CardId(3),
            owner: B,
            destination: ObjectDestination::Hand,
            program: creature(),
        },
    ];
    (
        context,
        GameSeed::with_uniform_life(
            vec![A, B],
            A,
            cards,
            vec![],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            names,
        ),
    )
}
pub(crate) fn selection_program(sacrifice: bool) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            spellwake_engine::program::BodyTarget::None,
            vec![selection(sacrifice)],
        ),
    )
}
pub(crate) fn initialized(program: Program) -> GameState {
    let (context, seed) = seed(program);
    GameState::initialize(GameId(95101), context, seed)
        .unwrap()
        .into_state()
}
pub(crate) fn step(state: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    state
        .advance(GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        })
        .unwrap()
}
pub(crate) fn pending() -> GameState {
    let mut state = initialized(selection_program(false));
    let spell = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    state = step(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell,
            payment: None,
        },
    )
    .into_state();
    for _ in 0..2 {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        else {
            panic!("actual ready priority required");
        };
        state = step(&state, holder, GameCommand::Pass).into_state();
    }
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionSelect(_)
    ));
    state
}
pub(crate) fn recorder(scope: CaptureScope) -> Recorder {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let source = |path: &str| SourceRef {
        path: path.into(),
        sha256: "0".repeat(64),
        bytes: 0,
    };
    Recorder::new_scoped(
        scope,
        &vocabulary,
        source("synthetic-input-receipt"),
        source("synthetic-publication-receipt"),
        vec![],
        RegistryWitness::NotApplicable,
    )
    .unwrap()
}
fn observe_result<R: Eq + NativeCaptureValue>(
    recorder: &mut Recorder,
    value: R,
) -> Result<(), String> {
    let input = json!(0);
    let result: Result<R, ()> = Ok(value);
    recorder.record_call(
        CallPoint {
            call_id: "result",
            before_point: None,
            after_point: None,
            adopted: false,
        },
        &QualificationCall::PureCall {
            method: "synthetic-native-result-control",
            input: &input,
        },
        &result,
        &result,
        |_| Ok(json!({"tag":"Ok"})),
        Append::NotApplicable,
        ReplayEvidence {
            source_eq: Some(true),
            source_capture: SourceCapture::BeforeInvocation,
        },
        &AliasContext::default(),
        None,
    )
}

#[test]
fn applicability_rejects_both_authored_operations_in_each_body_family() {
    for sacrifice in [false, true] {
        let program = selection_program(sacrifice);
        assert_eq!(program.check_current10(), Err(ApplicabilityError));
        let body = match program.kind {
            ProgramKind::Spell(value) => value.body,
            _ => unreachable!(),
        };
        let mut permanent = creature();
        permanent.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: body.clone(),
        });
        assert_eq!(permanent.check_current10(), Err(ApplicabilityError));
        permanent.activated.clear();
        permanent.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: spellwake_engine::program::TriggerSpec::player_action(
                PlayerRelation::Controller,
                spellwake_engine::program::PlayerTriggerAction::Discarded,
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body,
        });
        assert_eq!(permanent.check_current10(), Err(ApplicabilityError));
    }
}

#[test]
fn applicability_visits_unreachable_instructions_and_zero_count_nested_token_templates() {
    let mut program = selection_program(false);
    let ProgramKind::Spell(value) = &mut program.kind else {
        unreachable!()
    };
    value
        .body
        .instructions
        .insert(0, Instruction::Jump { destination: 2 });
    assert_eq!(program.check_current10(), Err(ApplicabilityError));
    let mut token = creature();
    token.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            spellwake_engine::program::BodyTarget::None,
            vec![selection(false)],
        ),
    });
    let definition = TokenDefinition::new(
        token,
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &creature_types::selected_vocabulary().unwrap(),
    )
    .unwrap();
    let outer = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            spellwake_engine::program::BodyTarget::None,
            vec![Instruction::CreateTokens {
                creations: vec![TokenCreation {
                    definition,
                    amount: spellwake_engine::program::ValueExpr::Literal(0.into()),
                    base_size: None,
                    enters_tapped: false,
                    recipients: spellwake_engine::token::TokenRecipients::Controller,
                    attacking_defender: None,
                }],
            }],
        ),
    );
    assert_eq!(outer.check_current10(), Err(ApplicabilityError));
}

#[test]
fn applicability_snapshot_failure_poison_is_sticky_and_inserts_nothing() {
    let mut recorder = recorder(CaptureScope::Current10);
    let error = recorder
        .snapshot(
            "new",
            &[],
            Phase::PureInput,
            "synthetic",
            SnapshotOwner::ProgramInput {
                program: &selection_program(false),
                source_ref: "actual-native-control",
            },
            &AliasContext::default(),
        )
        .unwrap_err();
    assert_eq!(error, ApplicabilityError.to_string());
    assert!(recorder.snapshots.is_empty());
    assert_eq!(
        recorder
            .finish_coverage(vec![], vec![], vec![], vec![])
            .unwrap_err(),
        error
    );
    assert_eq!(recorder.witness(&json!({})).unwrap_err(), error);
}

#[test]
fn applicability_checks_failed_constructor_and_program_validation_operands() {
    let (context, seed) = seed(selection_program(false));
    for call in [
        QualificationCall::EffectInitialize(&seed),
        QualificationCall::GameInitialize {
            game: GameId(1),
            context: &context,
            seed: &seed,
        },
        QualificationCall::GameInitializeScheduled {
            game: GameId(1),
            context: &context,
            seed: &seed,
            start: TurnStart {
                number: 1,
                step: Step::Cleanup,
            },
        },
        QualificationCall::ProgramValidate {
            program: &seed.cards[0].program,
            vocabulary: &seed.creature_vocabulary,
        },
    ] {
        assert_eq!(super::call(&call), Err(ApplicabilityError));
    }
    let mut recorder = recorder(CaptureScope::Current10);
    let result: Result<(), ()> = Err(());
    assert_eq!(
        recorder
            .record_call(
                CallPoint {
                    call_id: "failed",
                    before_point: None,
                    after_point: None,
                    adopted: false
                },
                &QualificationCall::EffectInitialize(&seed),
                &result,
                &result,
                |_| panic!("must reject before JSON projection"),
                Append::NotApplicable,
                ReplayEvidence {
                    source_eq: None,
                    source_capture: SourceCapture::InputOnlyForInitializer
                },
                &AliasContext::default(),
                None
            )
            .unwrap_err(),
        ApplicabilityError.to_string()
    );
}

#[test]
fn applicability_rejects_new_replies_on_old_sources_even_when_native_returns_err() {
    let state = initialized(creature());
    assert!(state.check_current10().is_ok());
    for choice in [
        DecisionReply::SelectResolution(vec![]),
        DecisionReply::OrderResolutionGraveyard(vec![]),
    ] {
        let input = GameInput {
            basis: state.basis().clone(),
            actor: A,
            command: GameCommand::Reply {
                decision: spellwake_engine::game::DecisionId(999),
                choice,
            },
        };
        let retained = state.clone();
        let result = state.advance(input.clone());
        assert!(result.is_err());
        let repeated = state.advance(input.clone());
        assert_eq!(result, repeated);
        let mut recorder = recorder(CaptureScope::Current10);
        assert_eq!(
            recorder
                .record_call(
                    CallPoint {
                        call_id: "reject",
                        before_point: None,
                        after_point: None,
                        adopted: false
                    },
                    &QualificationCall::GameInput(&input),
                    &result,
                    &repeated,
                    |_| panic!("no projection before applicability"),
                    Append::NotApplicable,
                    ReplayEvidence {
                        source_eq: Some(true),
                        source_capture: SourceCapture::BeforeInvocation
                    },
                    &AliasContext::default(),
                    Some(state.execution())
                )
                .unwrap_err(),
            ApplicabilityError.to_string()
        );
        assert_eq!(state, retained);
    }
}

#[test]
fn applicability_checks_result_frames_without_state_or_adoption_and_selection11_preserves_value() {
    let state = pending();
    let frame = state.pending_decision().unwrap().frame().clone();
    let mut old = recorder(CaptureScope::Current10);
    assert_eq!(
        observe_result(&mut old, frame.clone()).unwrap_err(),
        ApplicabilityError.to_string()
    );
    assert!(old.calls.is_empty());
    let mut new = recorder(CaptureScope::ResolutionSelection11);
    observe_result(&mut new, frame).unwrap();
    assert_eq!(new.calls.len(), 1);
    assert!(new.registry["tag"] == "NotApplicable");
    assert!(super::super::envelope("ordinary", json!({}), &new).is_err());
}

#[test]
fn applicability_visits_root_candidates_and_effect_rule_result_frames() {
    let state = pending();
    let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() else {
        unreachable!()
    };
    let result = RuleResult::Pending {
        checks: Default::default(),
        draws: Default::default(),
        frame: EffectDecision::ResolutionSelect(frame.clone()),
        journal: Default::default(),
        declarations: vec![],
        assignments: vec![],
    };
    assert_eq!(
        observe_result(&mut recorder(CaptureScope::Current10), result).unwrap_err(),
        ApplicabilityError.to_string()
    );
    let decision = state.pending_decision().unwrap();
    let candidate = step(
        &state,
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::SelectResolution(vec![frame.domain()[0]]),
        },
    );
    assert_eq!(
        observe_result(&mut recorder(CaptureScope::Current10), candidate).unwrap_err(),
        ApplicabilityError.to_string()
    );
}

#[test]
fn applicability_actual_owner_order_frame_and_effect_operands_are_excluded() {
    let mut program = selection_program(false);
    let ProgramKind::Spell(spell) = &mut program.kind else {
        unreachable!()
    };
    spell.body.instructions[0] =
        Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
            AffectedPlayers::EachPlayer,
            spellwake_engine::program::ValueExpr::Literal(2.into()),
        )));
    let (context, mut seed) = seed(program);
    seed.cards.push(SeedCard {
        card: CardId(4),
        owner: A,
        destination: ObjectDestination::Hand,
        program: creature(),
    });
    let mut state = GameState::initialize(GameId(95105), context, seed)
        .unwrap()
        .into_state();
    let spell = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    state = step(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell,
            payment: None,
        },
    )
    .into_state();
    for _ in 0..2 {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        else {
            unreachable!()
        };
        state = step(&state, holder, GameCommand::Pass).into_state();
    }
    while let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() {
        let pending = state.pending_decision().unwrap();
        state = step(
            &state,
            pending.chooser(),
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::SelectResolution(
                    frame.domain()[..frame.required()].to_vec(),
                ),
            },
        )
        .into_state();
    }
    let frame = state.pending_decision().unwrap().frame().clone();
    let RuleFrame::ResolutionGraveyardOrder(order) = &frame else {
        panic!("real physical owner order required")
    };
    assert_eq!(
        observe_result(&mut recorder(CaptureScope::Current10), frame.clone()).unwrap_err(),
        ApplicabilityError.to_string()
    );
    let frame = EffectDecision::ResolutionGraveyardOrder(order.clone());
    assert_eq!(
        super::call(&QualificationCall::EffectCall(EffectCall::ResumeRule {
            expected: state.execution().revision(),
            frame: &frame,
            actor: A,
            reply: &DecisionReply::OrderGraveyard(vec![]),
            turn: None
        })),
        Err(ApplicabilityError)
    );
}

#[test]
fn applicability_real_cleanup_discard_and_legacy_replies_remain_admissible() {
    let (context, mut seed) = seed(creature());
    seed.cards = (1..=8)
        .map(|id| SeedCard {
            card: CardId(id),
            owner: A,
            destination: ObjectDestination::Hand,
            program: creature(),
        })
        .collect();
    let source = GameState::initialize_scheduled(
        GameId(95103),
        context,
        seed,
        TurnStart {
            number: 2,
            step: Step::Cleanup,
        },
    )
    .unwrap()
    .into_state();
    assert!(source.check_current10().is_ok());
    let pending = source.pending_decision().unwrap();
    let RuleFrame::Checks(spellwake_engine::checks::CheckDecision::CleanupDiscard(frame)) =
        pending.frame()
    else {
        panic!("real cleanup decision required")
    };
    let candidate = step(
        &source,
        A,
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::Discard(vec![frame.domain()[0]]),
        },
    );
    assert!(candidate.rules_journal().records.iter().any(|record| matches!(record,RulesRecord::Event(event) if matches!(event.cause,EventCause::CleanupDiscard {..}))));
    assert!(candidate.check_current10().is_ok());
    assert!(DecisionReply::Discard(vec![]).check_current10().is_ok());
    assert!(
        DecisionReply::OrderGraveyard(vec![])
            .check_current10()
            .is_ok()
    );
}

#[test]
fn applicability_real_activation_sacrifice_cost_remains_admissible() {
    let artifact = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(7),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::Sacrifice,
            body: ExecutableBody::new(
                spellwake_engine::program::BodyTarget::SINGLE_CREATURE,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Hand,
                    ),
                ))],
            ),
        }],
    );
    let (context, mut seed) = seed(artifact);
    seed.cards[0].destination = ObjectDestination::Battlefield { controller: A };
    seed.cards[1].destination = ObjectDestination::Battlefield { controller: A };
    seed.initial_battlefield_order = vec![CardId(1), CardId(2)];
    let state = GameState::initialize(GameId(95104), context, seed)
        .unwrap()
        .into_state();
    let source = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    let target = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(2))
        .unwrap();
    let candidate = step(
        &state,
        A,
        GameCommand::ActivateAbility {
            source,
            ability: ActivatedAbilityId(7),
            target: spellwake_engine::targets::TargetRef::Object(target),
            payment: None,
        },
    );
    assert!(candidate.rules_journal().records.iter().any(|record| matches!(record,RulesRecord::Event(event) if event.occurrences.iter()
        .any(|row| matches!(&row.value,RulesOccurrence::ZoneChange(change) if change.reason==ZoneReason::SacrificeCost)))));
    assert!(candidate.check_current10().is_ok());
}

#[test]
fn applicability_opaque_json_words_do_not_claim_native_selection_semantics() {
    let mut recorder = recorder(CaptureScope::Current10);
    let value = json!({"Sacrifice":"ResolutionSelection","Discard":{"op":"Sacrifice"}});
    recorder
        .snapshot(
            "words",
            &[],
            Phase::PureInput,
            "opaque-test",
            SnapshotOwner::PureValue(&value),
            &AliasContext::default(),
        )
        .unwrap();
    observe_result(&mut recorder, value).unwrap();
    assert!(recorder.applicability_failure.is_none());
    assert_eq!(recorder.snapshots.len(), 1);
}

fn declaration_carrier(selected: bool) -> Vec<spellwake_engine::combat::CombatDeclarationRecord> {
    use spellwake_engine::combat::{
        AttackDeclaration, AttackTap, CombatDeclarationRecord, DeclarationOrigin,
    };
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let mut token = creature();
    token.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(86),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            spellwake_engine::program::BodyTarget::None,
            vec![if selected {
                selection(false)
            } else {
                Instruction::DrawCards(Box::new(spellwake_engine::toolbox::DrawCardsSpec::simple(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::Literal(0.into()),
                )))
            }],
        ),
    });
    let token = TokenDefinition::new(
        token,
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &vocabulary,
    )
    .unwrap()
    .materialize(&vocabulary)
    .unwrap();
    let created = GameObjects::new(0)
        .insert_token(0, A, token, &vocabulary)
        .unwrap();
    let tapped = created
        .next
        .set_tapped(created.next.revision(), created.after.id(), true)
        .unwrap();
    vec![CombatDeclarationRecord::Attackers {
        player: A,
        origin: DeclarationOrigin::PlayerReply,
        attackers: vec![AttackDeclaration {
            attacker: tapped.after.id(),
            defender: B,
            defending_planeswalker: None,
        }],
        taps: vec![AttackTap {
            before: tapped.before.unwrap(),
            after: tapped.after,
        }],
    }]
}

#[test]
fn applicability_declaration_only_result_retains_token_program_visibility() {
    for selected in [false, true] {
        let result = RuleResult::Complete {
            checks: Default::default(),
            draws: Default::default(),
            completion: spellwake_engine::effects::RuleCompletion::Resolution,
            journal: Default::default(),
            declarations: declaration_carrier(selected),
            assignments: vec![],
            permanent_entry: None,
        };
        assert_eq!(
            observe_result(&mut recorder(CaptureScope::Current10), result).is_err(),
            selected,
            "selection-bearing attack tap must not hide inside an otherwise old result"
        );
    }
}

#[test]
fn applicability_declaration_only_input_retains_token_program_visibility() {
    let state = pending();
    let entry = state.execution().resolution_state().unwrap().entry();
    let damage = spellwake_engine::damage::DamageJournal::default();
    let draws = spellwake_engine::draw::DrawJournal::default();
    for selected in [false, true] {
        let declarations = declaration_carrier(selected);
        let call = QualificationCall::EffectCall(EffectCall::BeginDamageBatch {
            origin: spellwake_engine::damage::DamageOrigin::Resolution {
                entry,
                instruction: 0,
            },
            kind: spellwake_engine::damage::DamageKind::Noncombat,
            inputs: &[],
            damage: &damage,
            draws: &draws,
            declarations: &declarations,
            assignments: &[],
        });
        assert_eq!(
            super::call(&call).is_err(),
            selected,
            "selection-bearing attack tap must not hide inside otherwise old operands"
        );
    }
}

#[test]
fn current10_refuses_printed_keywords_and_new_grants() {
    use spellwake_engine::characteristics::{ResolvedDuration, ResolvedPartsSpec};
    for (keyword, old_grant) in [
        (KeywordAbility::Haste, true),
        (KeywordAbility::Vigilance, true),
        (KeywordAbility::Flying, false),
    ] {
        assert_eq!(
            creature()
                .with_printed_keywords([keyword])
                .check_current10(),
            Err(ApplicabilityError)
        );
        let change = AbilityChange::grant_keywords([keyword]);
        let program = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                spellwake_engine::program::BodyTarget::SINGLE_PERMANENT,
                vec![Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec::resolved_parts(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(change)]),
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                ))],
            ),
        );
        assert_eq!(program.check_current10().is_ok(), old_grant);
    }
}
