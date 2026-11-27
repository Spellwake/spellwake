//! Opt-in native-only packet writer. Native rule work stays in the existing owners.
//! This module is nested under the private mechanism tests, never a public test hook.
use super::*;
use crate::{
    object_identity::ObjectMovement,
    occurrence::RulesJournal,
    program::DefinitionError,
    test_support::{current_capture as capture, current_profile::*},
};
use serde_json::{Value, json};
use std::{cell::RefCell, fs, io::Write, path::Path};

const PROFILE: &str = "optional-payment-native";
const SCHEMA: &str = "spellwake.optional-payment-native-packets.v2";
const CASES: [&str; 17] = [
    "OP-N03", "OP-N04", "OP-N05", "OP-N06", "OP-N07", "OP-N08", "OP-N10", "OP-N11", "OP-N15",
    "OP-N16", "OP-N18", "OP-N19", "OP-N20", "OP-N21", "OP-N22", "OP-N23", "OP-N24",
];
thread_local! {
    static SUPPLIED_OWNERS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static ROOT_SEGMENTS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static ROOT_LABEL: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Retain an actually returned constructor state and journal as its own native root.
pub(in crate::effects::resolution_choices::tests) fn retain_root(
    initialized: &NativeCandidate,
    helper: &str,
) {
    if !capture::is_active() {
        return;
    }
    ROOT_SEGMENTS.with(|roots| {
        let mut roots = roots.borrow_mut();
        let ordinal = roots.len();
        let point = capture::observation(
            "NativeOnly.ActualInitializerRoot",
            SnapshotOwner::GameState(initialized.state()),
            &AliasContext::default(),
        )
        .unwrap();
        capture::bind_observation(
            &point,
            &format!("/observations/root_segments/{ordinal}/initial_state"),
        )
        .unwrap();
        let state = projection::state(initialized.state(), &AliasContext::default()).unwrap();
        let role_context = json!({"participants":state["participants"],"active":state["active"]});
        roots.push(json!({
            "tag":"ActualNativeInitializerRoot","ordinal":ordinal,
            "label":ROOT_LABEL.with(|label|label.borrow().clone()),"helper":helper,
            "initial_state_point":point,"role_context":role_context,
            "initial_state":state,"initial_journal":projection::journal(initialized.rules_journal())
        }));
    });
}

/// Observe the existing primitive exactly; no reply decoding or rule dispatch lives here.
/// The inactive path is identical to the original native call.
pub(in crate::effects::resolution_choices::tests) fn resume_rule(
    source: &EffectWindow,
    expected: u64,
    frame: &EffectDecision,
    actor: SeatId,
    reply: DecisionReply,
    turn: Option<&crate::turn::TurnScheduler>,
) -> Result<RuleProgress, EffectError> {
    if !capture::is_active() {
        return source.resume_rule(expected, frame, actor, reply, turn);
    }
    let aliases = AliasContext::default();
    let saved = source.clone();
    let call = capture::native_call_id().unwrap();
    let before = capture::native_snapshot(
        Phase::BeforeCall,
        "EffectWindow::resume_rule",
        SnapshotOwner::EffectWindow(source),
        &aliases,
    )
    .unwrap();
    let result = source.resume_rule(expected, frame, actor, reply.clone(), turn);
    let replayed = source.resume_rule(expected, frame, actor, reply.clone(), turn);
    let after = result.as_ref().ok().map(|value| {
        capture::native_snapshot(
            Phase::AfterCall,
            "EffectWindow::resume_rule",
            SnapshotOwner::EffectWindow(value.state()),
            &aliases,
        )
        .unwrap()
    });
    capture::record_native(
        CallPoint {
            call_id: &call,
            before_point: Some(&before),
            after_point: after.as_deref(),
            adopted: result.is_ok(),
        },
        &projection::QualificationCall::EffectCall(projection::EffectCall::ResumeRule {
            expected,
            frame,
            actor,
            reply: &reply,
            turn,
        }),
        &result,
        &replayed,
        |value| match value {
            Ok(value) => {
                Ok(json!({"tag":"Ok","progress":projection::rule_progress(value,&aliases)?}))
            }
            Err(error) => Ok(projection::effect_error(error)),
        },
        result
            .as_ref()
            .map(|value| Append::RulesJournal(value.append()))
            .unwrap_or(Append::NotApplicable),
        ReplayEvidence {
            source_eq: Some(*source == saved),
            source_capture: SourceCapture::BeforeInvocation,
        },
        &aliases,
        Some(source),
    )
    .unwrap();
    result
}

fn validate_program(program: &Program) -> Result<(), DefinitionError> {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let saved = program.clone();
    let aliases = AliasContext::default();
    let call = capture::native_call_id().unwrap();
    let before = capture::native_snapshot(
        Phase::PureInput,
        "Program::validate",
        SnapshotOwner::ProgramInput {
            program,
            source_ref: "actual-complete-authored-synthetic-native-Program",
        },
        &aliases,
    )
    .unwrap();
    let result = program.validate(&vocabulary);
    let replayed = saved.validate(&vocabulary);
    capture::record_native(
        CallPoint {
            call_id: &call,
            before_point: Some(&before),
            after_point: None,
            adopted: false,
        },
        &projection::QualificationCall::ProgramValidate {
            program,
            vocabulary: &vocabulary,
        },
        &result,
        &replayed,
        |value| {
            Ok(match value {
                Ok(()) => json!({"tag":"Ok"}),
                Err(error) => projection::definition_error(error),
            })
        },
        Append::NotApplicable,
        ReplayEvidence {
            source_eq: Some(*program == saved),
            source_capture: SourceCapture::BeforeInvocation,
        },
        &aliases,
        None,
    )
    .unwrap();
    result
}

/// A supplied owner starts a new native-only segment after explicit privileged setup.
/// It is never represented as a root command or as continuous ordinary replay.
pub(super) fn retain_privileged_setup(
    source: &EffectWindow,
    movements: &[ObjectMovement],
    journal: &RulesJournal,
) {
    if !capture::is_active() {
        return;
    }
    SUPPLIED_OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        let index = owners.len();
        let point = capture::observation(
            "NativeOnly.SuppliedEffectWindowAfterPrivilegedReceiverReturn",
            SnapshotOwner::EffectWindow(source),
            &AliasContext::default(),
        )
        .unwrap();
        capture::bind_observation(
            &point,
            &format!("/observations/supplied_owners/{index}/state"),
        )
        .unwrap();
        owners.push(json!({
            "tag":"NativeOnlySuppliedOwnerAfterPrivilegedSetup",
            "owner":"EffectWindow",
            "point":point,
            "root_continuity":"NotClaimed",
            "preceding_root_ordinal":ROOT_SEGMENTS.with(|roots|roots.borrow().len().checked_sub(1).expect("actual preceding native root")),
            "setup":"Existing fixture temporarily removes owned optional work, performs actual qualification_zone_moves, then rederives the optional frame from the restored cursor",
            "state":projection::effect(source,&AliasContext::default()).unwrap(),
            "setup_movements":movements.iter().map(|movement|json!({
                "before":projection::object(&movement.before),
                "after":projection::object(&movement.after)
            })).collect::<Vec<_>>(),
            "setup_journal":projection::journal(journal)
        }));
    });
}

fn zero(choice: ResolutionPaymentReply) {
    let state = pending(offer(0));
    let paid = matches!(&choice, ResolutionPaymentReply::Pay(_));
    let result = reply(&state, DecisionReply::ResolutionPayment(choice));
    assert_eq!(
        result.state().execution().life_total(A),
        Some(&if paid { 20 } else { 22 }.into())
    );
    assert!(result.mana_payment().is_none());
    assert_eq!(
        result
            .rules_journal()
            .records
            .iter()
            .filter(|record| matches!(record,
                RulesRecord::ResolutionPaymentOutcome { outcome, .. }
                    if matches!(outcome, ResolutionPaymentOutcome::Paid(payment)
                        if payment.cost == ManaCost::free() && payment.spend.is_empty()) == paid
            ))
            .count(),
        1
    );
    assert!(
        !occurrences(&result)
            .iter()
            .any(|event| matches!(event, RulesOccurrence::ManaDebited { .. }))
    );
}

fn skipped_local_definition() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let definition = TokenDefinition::new(
        creature(),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &vocabulary,
    )
    .unwrap();
    let mut creation = TokenCreation::one(definition);
    creation.amount = crate::program::ValueExpr::count_battlefield(
        crate::program::ObjectFilter::any()
            .controlled()
            .of_chosen_slot(CreatureTypeSlot::new(0)),
    );
    for first in [
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: 1,
        },
        Instruction::PromptChoice(Box::new(
            crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                ResolvingPlayer::Controller,
                ManaCost::generic(1.into()),
                crate::program::ValueExpr::Literal(0.into()),
                1,
                2,
            ),
        )),
        Instruction::PromptChoice(Box::new(
            crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                ResolvingPlayer::Controller,
                ManaCost::generic(1.into()),
                crate::program::ValueExpr::Literal(0.into()),
                2,
                1,
            ),
        )),
    ] {
        let program = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    first,
                    Instruction::PromptChoice(Box::new(crate::toolbox::PromptChoiceSpec {
                        chooser: crate::toolbox::PlayerSelector::Resolving(
                            crate::program::ResolvingPlayer::Controller,
                        ),
                        domain: crate::toolbox::PromptDomain::Subtype {
                            kind: crate::toolbox::SubtypeChoiceKind::Creature,
                            excluding: BTreeSet::new(),
                            legacy_slot: Some(CreatureTypeSlot::new(0)),
                        },
                        category: crate::toolbox::ChoiceCategory::Generic,
                        bind_to: crate::toolbox::BindingId(0),
                        bind_unchosen: None,
                        tag_partition: None,
                        auto_resolve_trivial: false,
                        option_branches: Vec::new(),
                    })),
                    Instruction::CreateTokens {
                        creations: vec![creation.clone()],
                    },
                ],
            ),
        );
        assert_eq!(
            validate_program(&program),
            Err(DefinitionError::UndefinedCreatureTypeSlot)
        );
    }
    // These complete-Program captures add the existing validator's two branch
    // orientations. The earlier three one-path negatives remain separate.
    for (paid_destination, declined_destination) in [(1, 3), (3, 1)] {
        for (other_slot, expected) in [
            (CreatureTypeSlot::new(0), Ok(())),
            (
                CreatureTypeSlot::new(1),
                Err(DefinitionError::UndefinedCreatureTypeSlot),
            ),
        ] {
            let program = Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::PromptChoice(Box::new(
                            crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                                ResolvingPlayer::Controller,
                                ManaCost::generic(1.into()),
                                crate::program::ValueExpr::Literal(0.into()),
                                paid_destination,
                                declined_destination,
                            ),
                        )),
                        Instruction::PromptChoice(Box::new(crate::toolbox::PromptChoiceSpec {
                            chooser: crate::toolbox::PlayerSelector::Resolving(
                                crate::program::ResolvingPlayer::Controller,
                            ),
                            domain: crate::toolbox::PromptDomain::Subtype {
                                kind: crate::toolbox::SubtypeChoiceKind::Creature,
                                excluding: BTreeSet::new(),
                                legacy_slot: Some(CreatureTypeSlot::new(0)),
                            },
                            category: crate::toolbox::ChoiceCategory::Generic,
                            bind_to: crate::toolbox::BindingId(0),
                            bind_unchosen: None,
                            tag_partition: None,
                            auto_resolve_trivial: false,
                            option_branches: Vec::new(),
                        })),
                        Instruction::Jump { destination: 4 },
                        Instruction::PromptChoice(Box::new(crate::toolbox::PromptChoiceSpec {
                            chooser: crate::toolbox::PlayerSelector::Resolving(
                                crate::program::ResolvingPlayer::Controller,
                            ),
                            domain: crate::toolbox::PromptDomain::Subtype {
                                kind: crate::toolbox::SubtypeChoiceKind::Creature,
                                excluding: BTreeSet::new(),
                                legacy_slot: Some(other_slot),
                            },
                            category: crate::toolbox::ChoiceCategory::Generic,
                            bind_to: crate::toolbox::BindingId(0),
                            bind_unchosen: None,
                            tag_partition: None,
                            auto_resolve_trivial: false,
                            option_branches: Vec::new(),
                        })),
                        Instruction::CreateTokens {
                            creations: vec![creation.clone()],
                        },
                    ],
                ),
            );
            assert_eq!(validate_program(&program), expected);
        }
    }
}

fn singular_related_player_definitions() {
    // The Program unit matrix already establishes these native admission rules.
    // This adds actual complete inputs/results to the existing packet capture;
    // no runtime cause, participant, root or observation is synthesized here.
    let conditions = [
        crate::program::TriggerSpec::self_cast(),
        crate::program::TriggerSpec::spell_cast(
            PlayerRelation::Opponent,
            crate::program::SpellFilter::any(),
        ),
        crate::program::TriggerSpec::player_action(
            PlayerRelation::AnyParticipant,
            crate::program::PlayerTriggerAction::TokenCreated,
        ),
        crate::program::TriggerSpec::life_changed(
            crate::life::LifeChangeKind::Gain,
            PlayerRelation::AnyParticipant,
        ),
        crate::program::TriggerSpec::enters(crate::program::ObjectFilter::creature()),
        crate::program::TriggerSpec::leaves_battlefield(crate::program::ObjectFilter::creature()),
        crate::program::TriggerSpec::dies(crate::program::ObjectFilter::creature()),
        crate::program::TriggerSpec::becomes_tapped(crate::program::ObjectFilter::self_object()),
        crate::program::TriggerSpec::player_action(
            PlayerRelation::AnyParticipant,
            crate::program::PlayerTriggerAction::Discarded,
        ),
        crate::program::TriggerSpec::step_began(
            crate::turn::Step::Upkeep,
            PlayerRelation::AnyParticipant,
        ),
    ];
    for condition in conditions {
        for instructions in [
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::UniqueRelatedPlayer,
                    instructions: 1,
                },
                gain(1),
            ],
            vec![Instruction::PromptChoice(Box::new(
                crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                    ResolvingPlayer::UniqueRelatedPlayer,
                    ManaCost::generic(1.into()),
                    crate::program::ValueExpr::Literal(0.into()),
                    1,
                    1,
                ),
            ))],
        ] {
            for (grouping, expected) in [
                (TriggerGrouping::EachOccurrence, Ok(())),
                (TriggerGrouping::OneOrMoreByPlayer, Ok(())),
                (
                    TriggerGrouping::OneOrMoreTogether,
                    Err(DefinitionError::UnguaranteedRelatedPlayer),
                ),
            ] {
                let mut program = watched(
                    Program::artifact(None, vec![]),
                    condition.clone(),
                    instructions.clone(),
                );
                program.triggered[0].grouping = grouping;
                assert_eq!(
                    validate_program(&program),
                    expected,
                    "actual complete definition: condition {condition:?}, grouping {grouping:?}"
                );
            }
        }
    }
}

fn moved_donor_definition() {
    let program = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![
                    Instruction::OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 1,
                    },
                    crate::program::Instruction::ZoneOp(Box::new(
                        crate::toolbox::ZoneOpSpec::move_all(
                            crate::toolbox::ObjectQuery::Targets,
                            crate::toolbox::ZoneDest::Hand,
                        ),
                    )),
                    Instruction::ApplyContinuous(Box::new(
                        crate::toolbox::ApplyContinuousSpec::become_copy(
                            crate::toolbox::ObjectQuery::Source,
                            crate::toolbox::ObjectQuery::Targets,
                            ResolvedDuration::UntilEndOfTurn,
                            crate::program::CopyExceptions {
                                override_pt: None,
                                ..Default::default()
                            },
                        ),
                    )),
                ],
            ),
        }],
    );
    assert_eq!(
        validate_program(&program),
        Err(DefinitionError::InvalidInstruction)
    );
}

fn zero_count() {
    let state = pending(offer(3));
    let old = payment_frame(&state);
    let original = GameInput {
        basis: state.basis().clone(),
        actor: A,
        command: command_reply(
            &state,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        ),
    };
    let old_id = state.pending_decision().unwrap().id();
    let activated = tap(&state, 13);
    assert!(activated.mana_production().unwrap().produced.is_empty());
    assert_eq!(
        activated.state().execution().mana(A, ManaType::Blue),
        0.into()
    );
    assert_eq!(
        payment_frame(activated.state()).revision(),
        activated.state().execution().revision()
    );
    assert_eq!(payment_frame(activated.state()).entry(), old.entry());
    assert_eq!(payment_frame(activated.state()).cost(), old.cost());
    assert_ne!(activated.state().pending_decision().unwrap().id(), old_id);
    assert_eq!(
        resume_rule(
            activated.state().execution(),
            activated.state().execution().revision(),
            &EffectDecision::ResolutionPayment(old),
            A,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
            None
        ),
        Err(EffectError::WrongResolutionPaymentFrame)
    );
    assert_eq!(
        capture::advance(activated.state(), original),
        Err(GameError::StaleRevision)
    );
    reject(
        activated.state(),
        A,
        GameCommand::Reply {
            decision: old_id,
            choice: DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        },
        GameError::WrongDecision,
    );
    let declined = reply(
        activated.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    assert_tail(&declined);
    assert_eq!(outcomes(&declined), 0);
    assert_eq!(
        declined
            .state()
            .execution()
            .window()
            .cards()
            .get(object(&state, 13))
            .unwrap()
            .tapped(),
        Some(true)
    );
}

fn accept(trace: &mut NativeReplay, actor: SeatId, command: GameCommand) {
    let state = trace.states.last().unwrap();
    let input = GameInput {
        basis: state.basis().clone(),
        actor,
        command,
    };
    let next = capture::advance(state, input.clone()).unwrap();
    trace.inputs.push(input);
    trace.journals.push(next.rules_journal().clone());
    trace.states.push(next.into_state());
}
fn choose(trace: &mut NativeReplay, choice: DecisionReply) {
    let state = trace.states.last().unwrap();
    let actor = state.pending_decision().unwrap().chooser();
    let command = command_reply(state, choice);
    accept(trace, actor, command);
}
struct NativeReplay {
    inputs: Vec<GameInput>,
    journals: Vec<RulesJournal>,
    states: Vec<GameState>,
}
fn observed_state(state: &GameState, pointer: &str) -> Value {
    let point = capture::observation(
        "NativeOnly.SavedTypedReplayCheckpoint",
        SnapshotOwner::GameState(state),
        &AliasContext::default(),
    )
    .unwrap();
    capture::bind_observation(&point, pointer).unwrap();
    projection::state(state, &AliasContext::default()).unwrap()
}
fn saved_replay() -> Value {
    let mut branches = Vec::new();
    for (branch, optional) in [
        Some(OptionalEffectChoice::Take),
        Some(OptionalEffectChoice::Decline),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        let initial = initialize(vec![
            Instruction::PromptChoice(Box::new(
                crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                    ResolvingPlayer::Controller,
                    ManaCost::generic(2.into()),
                    crate::program::ValueExpr::Literal(0.into()),
                    1,
                    4,
                ),
            )),
            Instruction::OptionalSequence {
                chooser: ResolvingPlayer::Controller,
                instructions: 1,
            },
            gain(1),
            gain(2),
        ]);
        let root_anchor = ROOT_SEGMENTS.with(|roots| {
            let roots = roots.borrow();
            let root = roots.last().expect("actual replay initializer retained");
            json!({"ordinal":root["ordinal"],"initial_state_point":root["initial_state_point"]})
        });
        let mut trace = NativeReplay {
            inputs: vec![],
            journals: vec![initial.rules_journal().clone()],
            states: vec![initial.into_state()],
        };
        let spell = object(&trace.states[0], 1);
        accept(
            &mut trace,
            A,
            GameCommand::CastUntargeted {
                spell,
                payment: None,
            },
        );
        accept(&mut trace, A, GameCommand::Pass);
        accept(&mut trace, B, GameCommand::Pass);
        for id in [10, 12] {
            let source = object(trace.states.last().unwrap(), id);
            accept(
                &mut trace,
                A,
                GameCommand::ActivateMana {
                    source,
                    ability: ABILITY,
                },
            );
        }
        choose(&mut trace, DecisionReply::ManaColor(ManaType::Blue));
        choose(
            &mut trace,
            DecisionReply::ResolutionPayment(if optional.is_some() {
                ResolutionPaymentReply::Pay(ManaPayment {
                    from_lots: Default::default(),
                    spend: vec![ManaSpend {
                        mana_type: ManaType::Blue,
                        amount: 2.into(),
                    }],
                })
            } else {
                ResolutionPaymentReply::Decline
            }),
        );
        if let Some(choice) = optional {
            choose(&mut trace, DecisionReply::OptionalEffect(choice));
        }

        // Replay starts at the exact retained initialized native state. The constructor's
        // separate exact supplied-input replay is already retained by current_capture.
        let mut replay = trace.states[0].clone();
        let mut replayed_states = vec![replay.clone()];
        let mut replayed_journals = Vec::new();
        for (index, original) in trace.inputs.iter().enumerate() {
            let next = capture::without_capture(|| replay.advance(original.clone())).unwrap();
            assert_eq!(next.rules_journal(), &trace.journals[index + 1]);
            assert_eq!(next.state(), &trace.states[index + 1]);
            replayed_journals.push(next.rules_journal().clone());
            replay = next.into_state();
            replayed_states.push(replay.clone());
        }
        assert_eq!(&replay, trace.states.last().unwrap());

        let mut changed = trace.inputs[5].clone();
        let GameCommand::Reply { choice, .. } = &mut changed.command else {
            unreachable!()
        };
        *choice = DecisionReply::ManaColor(ManaType::Red);
        let changed_result =
            capture::advance_with_adoption(&trace.states[5], changed.clone(), false).unwrap();
        assert_ne!(changed_result.rules_journal(), &trace.journals[6]);
        assert_ne!(changed_result.state(), &trace.states[6]);
        let mut wrong = trace.inputs[4].clone();
        wrong.actor = B;
        let wrong_result = capture::advance(&trace.states[4], wrong.clone());
        assert_eq!(wrong_result, Err(GameError::WrongChooser));

        let saved_states = trace
            .states
            .iter()
            .enumerate()
            .map(|(index, state)| {
                observed_state(
                    state,
                    &format!("/observations/replay/branches/{branch}/saved_states/{index}"),
                )
            })
            .collect::<Vec<_>>();
        let replayed_states = replayed_states
            .iter()
            .enumerate()
            .map(|(index, state)| {
                observed_state(
                    state,
                    &format!("/observations/replay/branches/{branch}/replayed_states/{index}"),
                )
            })
            .collect::<Vec<_>>();
        branches.push(json!({
            "optional":optional.map(|value|format!("{value:?}")),
            "root_anchor":root_anchor,
            "start":"RetainedNativeStateAfterActualInitializer",
            "saved_inputs":trace.inputs.iter().map(projection::input).collect::<Vec<_>>(),
            "saved_journals":trace.journals.iter().map(projection::journal).collect::<Vec<_>>(),
            "saved_states":saved_states,
            "replayed_journals":replayed_journals.iter().map(projection::journal).collect::<Vec<_>>(),
            "replayed_states":replayed_states,
            "probes":[
                {"kind":"LegalChangedColor","input":projection::input(&changed),"result":projection::candidate(&changed_result,Some(&trace.states[5]),&AliasContext::default()).unwrap(),"expected_relation":"DifferentJournalAndWholeState"},
                {"kind":"WrongActor","input":projection::input(&wrong),"error":projection::game_error(wrong_result.as_ref().unwrap_err()),"expected_relation":"AtomicTypedErr"}
            ],
            "final_state":observed_state(&replay,&format!("/observations/replay/branches/{branch}/final_state"))
        }));
    }
    json!({"schema":"spellwake.native-saved-input-replay.v2","branches":branches})
}

fn roles(case: &str) -> Value {
    match case {
        "OP-N11" | "OP-N21" => Value::Null,
        "OP-N03" | "OP-N04" => {
            json!({"active":C.0,"captured_controller":A.0,"caster_and_payer":B.0})
        }
        "OP-N10" => json!({"active":C.0,"captured_controller":B.0,"creator_and_payer":A.0}),
        "OP-N08" => {
            json!({"participants":[A.0,B.0],"active":A.0,"source_owner":B.0,"source_controller":A.0})
        }
        _ => json!({"participants":[A.0,B.0],"active":A.0}),
    }
}
fn run(case: &str) -> Option<Value> {
    match case {
        "OP-N03" | "OP-N04" => {
            active_controller_and_caster_keep_three_distinct_roles_after_source_change()
        }
        "OP-N05" => zero(ResolutionPaymentReply::Pay(ManaPayment::default())),
        "OP-N06" => zero(ResolutionPaymentReply::Decline),
        "OP-N07" => {
            positive_counted_production_restores_quote_then_fixed_production_pays_exactly_three()
        }
        "OP-N08" => {
            tap_watcher_waits_through_real_color_child_and_payment_until_the_rules_boundary()
        }
        "OP-N11" => skipped_local_definition(),
        "OP-N15" => {
            ROOT_LABEL
                .with(|label| *label.borrow_mut() = "OP-N15/nonempty-local-continuation".into());
            nonempty_resolution_locals_survive_each_mana_prefix_and_both_optional_outcomes();
            ROOT_LABEL.with(|label| {
                *label.borrow_mut() =
                    "OP-N15/existing-required-cost-and-unavailable-prefix-adjunct".into()
            });
            depth_completion::exact_total_unavailable_amount_and_required_colorless_denials_keep_the_funded_prefix();
        }
        "OP-N16" => return Some(saved_replay()),
        "OP-N18" => {
            optional_copy_preserves_actual_copy_targets_child_and_rejects_reused_or_foreign_replies(
            )
        }
        "OP-N19" => optional_type_literal_zero_draw_continuation_assigns_only_on_take(),
        "OP-N20" => earlier_bounce_removes_take_from_the_real_optional_counter_domain(),
        "OP-N21" => moved_donor_definition(),
        "OP-N22" => {
            optional_token_batch_preserves_real_timestamp_child_then_creates_once_or_skips_it();
            optional_damage_preserves_the_real_order_child_and_consumes_the_offer_once();
        }
        "OP-N23" => zero_count(),
        "OP-N24" => {
            optional_copy_receiver_departure_removes_take_while_the_donor_is_still_legal();
            fresh_receiver_incarnation_cannot_replace_the_exact_captured_copy_source();
        }
        "OP-N10" => {
            singular_related_player_definitions();
            actual_by_player_group_retains_two_entry_causes_for_distinct_payer_and_optional_controller();
        }
        _ => unreachable!("closed private case inventory"),
    }
    None
}
fn write_new(path: &Path, value: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.write_all(&bytes)
        .map_err(|error| format!("{}: {error}", path.display()))
}

#[test]
#[ignore = "explicit native-only packet export with retained publication and a fresh output directory"]
fn export_optional_payment_native_packets() -> Result<(), String> {
    let output = std::env::var_os("SPELLWAKE_OPTIONAL_NATIVE_OUTPUT")
        .ok_or("SPELLWAKE_OPTIONAL_NATIVE_OUTPUT required")?;
    let output = Path::new(&output);
    fs::create_dir(output)
        .map_err(|error| format!("fresh output directory {}: {error}", output.display()))?;
    let repo = capture::repository();
    let sources = [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "crates/engine/src/effects/resolution_choices.rs",
        "crates/engine/src/effects/resolution_choices/tests.rs",
        "crates/engine/src/effects/resolution_choices/native_mechanisms.rs",
        "crates/engine/src/effects/resolution_choices/native_packets.rs",
        "crates/engine/src/effects/resolution_choices/depth_completion.rs",
        "crates/engine/src/effects/resolution_choices/depth_completion_v2.rs",
        "crates/engine/src/effects/resolution_choices/resolving_inspection.rs",
        "crates/engine/test_support/mod.rs",
        "crates/reference_tools/src/native_packet_wire.rs",
        "crates/engine/src/program/optional_payment_tests.rs",
    ]
    .map(|path| repo.join(path));
    let mut artifacts = Vec::new();
    for case in CASES {
        SUPPLIED_OWNERS.with(|owners| owners.borrow_mut().clear());
        ROOT_SEGMENTS.with(|roots| roots.borrow_mut().clear());
        ROOT_LABEL.with(|label| *label.borrow_mut() = format!("{case}/selected-native-trajectory"));
        capture::start_with_registry_witness(PROFILE, &sources, RegistryWitness::NotApplicable)?;
        let replay = run(case);
        let supplied_owners =
            SUPPLIED_OWNERS.with(|owners| std::mem::take(&mut *owners.borrow_mut()));
        let root_segments = ROOT_SEGMENTS.with(|roots| std::mem::take(&mut *roots.borrow_mut()));
        let observations = json!({
            "schema":SCHEMA,"scope":"NativeOnly","case_id":case,
            "roles":roles(case),
            "provenance":{"tag":"SyntheticNativeOnly","ordinary_credit":false,"source_card_credit":false,"full_game_credit":false},
            "root_segments":root_segments,"supplied_owners":supplied_owners,"replay":replay
        });
        let value = capture::finish(PROFILE, observations)?;
        let path = output.join(format!("{case}.json"));
        write_new(&path, &value)?;
        artifacts.push(capture::source(&path)?);
    }
    write_new(
        &output.join("index.json"),
        &json!({
            "schema":"spellwake.optional-payment-native-packet-index.v2",
            "scope":"NativeOnly","profile":PROFILE,"artifacts":artifacts
        }),
    )
}

#[path = "native_role_fault_packets.rs"]
mod native_role_fault_packets;
