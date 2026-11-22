//! Private structural corruptions isolate each retained owner from the easier
//! physical Program path. These are adversarial gate controls, not game evidence.
use super::*;
use crate::{
    characteristics::{ResolvedPart, ResolvedParts},
    occurrence::{SupportedAppearance, ZoneOccurrence, ZoneReason},
    program::{TriggerGrouping, TriggeredAbility, TriggeredAbilityId},
    test_support::current_profile::applicability::{
        ApplicabilityError, NativeCaptureValue, tests as fixture,
    },
};

fn old_body() -> ExecutableBody {
    ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::DrawController {
            amount: crate::program::ValueExpr::Literal(0.into()),
        }],
    )
}
fn old_program() -> Program {
    Program::instant(ManaCost::free(), old_body())
}
fn remove_authored_bodies(state: &mut EffectWindow) {
    for program in Arc::make_mut(&mut state.programs).values_mut() {
        *program = fixture::creature();
    }
}
fn remove_captured_bodies(state: &mut EffectWindow) {
    for payload in state.payloads.values_mut() {
        if let StackPayload::Instructions(value) = payload {
            value.body = old_body();
            value.copiable_traits = None;
        }
    }
}

#[test]
fn applicability_queued_and_resolving_bodies_are_checked_after_authored_definition_is_removed() {
    let source = fixture::initialized(fixture::selection_program(false));
    let card = source
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    let mut queued = fixture::step(
        &source,
        fixture::A,
        GameCommand::CastUntargeted {
            spell: card,
            payment: None,
        },
    )
    .into_state()
    .execution()
    .clone();
    remove_authored_bodies(&mut queued);
    assert!(
        queued
            .programs()
            .all(|(_, program)| program.check_current10().is_ok())
    );
    assert!(queued.pending_work_kind().is_none());
    assert_eq!(queued.check_current10(), Err(ApplicabilityError));
    let mut resolving = fixture::pending().execution().clone();
    remove_authored_bodies(&mut resolving);
    resolving.pending_work = None;
    assert!(resolving.window().resolving().is_some());
    assert_eq!(resolving.check_current10(), Err(ApplicabilityError));
}

#[test]
fn applicability_selection_work_is_checked_at_each_stage_without_a_body_or_issued_frame() {
    let mut state = fixture::pending().execution().clone();
    remove_authored_bodies(&mut state);
    remove_captured_bodies(&mut state);
    let stages = vec![
        ResolutionSelectionStage::Choosing {
            requirements: vec![],
            next_player: 0,
            accepted: vec![],
        },
        ResolutionSelectionStage::Ordering {
            replacements: None,
            selections: vec![],
            groups: vec![],
            next_owner: 0,
            orders: vec![],
        },
        ResolutionSelectionStage::Ready(ResolutionSelectionBatch {
            selections: vec![],
            replacements: None,
            logical: vec![],
            application: vec![],
            owner_orders: vec![],
            token_departures: vec![],
        }),
    ];
    for stage in stages {
        let Some(PendingWork::ResolutionSelection(work)) = &mut state.pending_work else {
            unreachable!()
        };
        work.stage = stage;
        assert!(state.pending_resolution_decision().is_none());
        assert_eq!(state.check_current10(), Err(ApplicabilityError));
    }
}

#[test]
fn applicability_copy_owned_body_and_trigger_values_survive_donor_lookup_removal() {
    let mut state = fixture::pending().execution().clone();
    let original = state.window().resolving().unwrap().id();
    let copied = state.payloads[&original].clone();
    state.window = state
        .window
        .record_spell_copy(state.window.revision(), original, fixture::B, vec![])
        .unwrap();
    let copy = state.window.entries().last().unwrap().id();
    remove_authored_bodies(&mut state);
    remove_captured_bodies(&mut state);
    state.pending_work = None;
    state.payloads.insert(copy, copied);
    assert_eq!(state.check_current10(), Err(ApplicabilityError));
    let StackPayload::Instructions(payload) = state.payloads.get_mut(&copy).unwrap() else {
        unreachable!()
    };
    payload.body = old_body();
    payload
        .copiable_traits
        .as_mut()
        .unwrap()
        .triggered
        .push(TriggeredAbility {
            id: TriggeredAbilityId(91),
            condition: crate::program::TriggerSpec::self_cast(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, vec![fixture::selection(true)]),
        });
    assert_eq!(state.check_current10(), Err(ApplicabilityError));
}

#[test]
fn applicability_waiting_capture_and_retained_discovery_do_not_use_later_program_lookup() {
    let mut observer = fixture::creature();
    observer.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(5),
        condition: crate::program::TriggerSpec::enters(crate::program::ObjectFilter::self_object()),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![fixture::selection(false)]),
    });
    let (_, seed) = fixture::seed(observer);
    let source = EffectWindow::new(seed).unwrap().settle_checks(0).unwrap();
    let object = source.window.cards().current_identity(CardId(1)).unwrap();
    let moved = source
        .qualification_zone_moves(
            source.revision(),
            &[(
                object,
                ObjectDestination::Battlefield {
                    controller: fixture::A,
                },
            )],
        )
        .unwrap();
    let mut state = moved.state().clone();
    assert_eq!(state.waiting_triggers().count(), 1);
    remove_authored_bodies(&mut state);
    assert_eq!(state.check_current10(), Err(ApplicabilityError));
    let mut waiting = state.waiting_triggers().next().unwrap().clone();
    waiting.source.printed_text.triggered.clear();
    assert!(waiting.source.check_current10().is_ok());
    assert_eq!(
        RulesJournal {
            records: vec![RulesRecord::TriggerDiscovered(Box::new(waiting))]
        }
        .check_current10(),
        Err(ApplicabilityError)
    );
}

#[test]
fn applicability_all_captured_copy_definitions_are_checked_including_obscured_older_values() {
    let source = fixture::initialized(old_program());
    let spell = source
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    let mut state = fixture::step(
        &source,
        fixture::A,
        GameCommand::CastUntargeted {
            spell,
            payment: None,
        },
    )
    .into_state()
    .execution()
    .clone();
    let entry = state.window.entries()[0].id();
    let object = state.window.cards().current_identity(CardId(2)).unwrap();
    let mut selected = fixture::creature();
    selected.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: crate::program::TriggerSpec::self_cast(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![fixture::selection(false)]),
    });
    for program in [selected, fixture::creature()] {
        state
            .characteristics
            .add_effect(
                entry,
                object,
                ResolvedParts(vec![ResolvedPart::Copy(Box::new(program))]),
                crate::characteristics::ResolvedDuration::UntilEndOfTurn,
            )
            .unwrap();
    }
    assert_eq!(state.continuous_effects().count(), 2);
    assert!(
        state
            .programs()
            .all(|(_, program)| program.check_current10().is_ok())
    );
    assert_eq!(state.check_current10(), Err(ApplicabilityError));
}

fn native_event() -> (
    crate::occurrence::RulesEvent,
    SupportedAppearance,
    StackEntryId,
) {
    let source = fixture::initialized(old_program());
    let spell = source
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    let candidate = fixture::step(
        &source,
        fixture::A,
        GameCommand::CastUntargeted {
            spell,
            payment: None,
        },
    );
    let event = candidate
        .rules_journal()
        .records
        .iter()
        .find_map(|record| match record {
            RulesRecord::Event(event) => Some(event.clone()),
            _ => None,
        })
        .unwrap();
    let state = candidate.state().execution();
    let object = state.window.cards().current_identity(CardId(1)).unwrap();
    let appearance = state
        .rules_snapshot(None)
        .unwrap()
        .view()
        .appearance(object)
        .unwrap();
    (event, appearance, state.window.entries()[0].id())
}

#[test]
fn applicability_append_reasons_distinguish_resolution_discard_from_mechanism_and_cost() {
    let (base, appearance, entry) = native_event();
    for (cause, reason, rejected) in [
        (
            EventCause::MechanismUnit,
            ZoneReason::Sacrifice { player: fixture::A },
            true,
        ),
        (
            EventCause::Instruction {
                entry,
                instruction: 0,
            },
            ZoneReason::Discard { player: fixture::A },
            true,
        ),
        (
            EventCause::MechanismUnit,
            ZoneReason::Discard { player: fixture::A },
            false,
        ),
        (
            EventCause::ActivatedAbilityCost {
                entry,
                ability: crate::activated::ActivatedAbilityId(1),
            },
            ZoneReason::SacrificeCost,
            false,
        ),
    ] {
        let mut event = base.clone();
        event.cause = cause;
        event.occurrences.truncate(1);
        event.occurrences[0].value = RulesOccurrence::ZoneChange(Box::new(ZoneOccurrence {
            additional_arrivals: vec![],
            madness: false,
            before: appearance.clone(),
            after: appearance.clone(),
            reason,
        }));
        assert_eq!(
            RulesJournal {
                records: vec![RulesRecord::Event(event)]
            }
            .check_current10()
            .is_err(),
            rejected
        );
    }
}

#[test]
fn applicability_retained_removed_printed_body_and_token_origin_are_checked() {
    let (mut event, mut appearance, _) = native_event();
    appearance.printed_text.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(71),
        condition: crate::program::TriggerSpec::self_cast(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![fixture::selection(false)]),
    });
    appearance.abilities.triggered.clear();
    event.occurrences.truncate(1);
    event.occurrences[0].value = RulesOccurrence::TokenCreated {
        creator: fixture::A,
        after: Box::new(appearance),
    };
    assert_eq!(
        RulesJournal {
            records: vec![RulesRecord::Event(event)]
        }
        .check_current10(),
        Err(ApplicabilityError)
    );
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let mut token = fixture::creature();
    token.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(73),
        condition: crate::program::TriggerSpec::self_cast(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![fixture::selection(true)]),
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
    let created = crate::object_identity::GameObjects::new(0)
        .insert_token(0, fixture::A, token, &vocabulary)
        .unwrap();
    assert_eq!(created.next.check_current10(), Err(ApplicabilityError));
    let source = crate::damage::DamageSource::Object {
        object: created.after,
        controller: fixture::A,
    };
    assert_eq!(source.check_current10(), Err(ApplicabilityError));
}

#[test]
fn applicability_journal_copy_carriers_and_append_only_calls_cannot_bypass_the_gate() {
    use crate::test_support::current_profile::{
        self, Append, CallPoint, ReplayEvidence, SourceCapture,
    };
    let (mut event, appearance, entry) = native_event();
    let selected = TriggeredAbility {
        id: TriggeredAbilityId(81),
        condition: crate::program::TriggerSpec::self_cast(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![fixture::selection(false)]),
    };
    event.occurrences.truncate(1);
    event.occurrences[0].value = RulesOccurrence::CopyCreated {
        copiable_card_types: appearance.copiable_card_types.clone(),
        copiable_creature_subtypes: appearance.copiable_creature_subtypes.clone(),
        copiable_spell_subtypes: appearance.copiable_spell_subtypes.clone(),
        copiable_artifact_subtypes: appearance.copiable_artifact_subtypes.clone(),
        copiable_enchantment_subtypes: appearance.copiable_enchantment_subtypes.clone(),
        copiable_names: appearance.copiable_names.clone(),
        copiable_supertypes: appearance.copiable_supertypes.clone(),
        copiable_keywords: Default::default(),
        copiable_triggered: vec![selected.clone()],
        entry,
        original: entry,
        controller: fixture::A,
        targets: vec![],
    };
    let mut definition = fixture::creature();
    definition.triggered.push(selected);
    let mut state = fixture::initialized(old_program()).execution().clone();
    let target = state.window.cards().current_identity(CardId(2)).unwrap();
    let effect = state
        .characteristics
        .add_effect(
            entry,
            target,
            ResolvedParts(vec![ResolvedPart::Copy(Box::new(definition))]),
            crate::characteristics::ResolvedDuration::UntilEndOfTurn,
        )
        .unwrap();
    let first = event.clone();
    event.occurrences[0].value = RulesOccurrence::ContinuousEffectCreated {
        effect,
        appearances: vec![],
    };
    for event in [first, event] {
        let journal = RulesJournal {
            records: vec![RulesRecord::Event(event)],
        };
        assert_eq!(journal.check_current10(), Err(ApplicabilityError));
        let mut recorder = fixture::recorder(current_profile::CaptureScope::Current10);
        let scalar = serde_json::json!(0);
        let native: Result<(), ()> = Ok(());
        assert_eq!(
            recorder
                .record_call(
                    CallPoint {
                        call_id: "append-only",
                        before_point: None,
                        after_point: None,
                        adopted: false
                    },
                    &projection::QualificationCall::PureCall {
                        method: "synthetic-append-control",
                        input: &scalar
                    },
                    &native,
                    &native,
                    |_| panic!("append applicability must precede JSON projection"),
                    Append::RulesJournal(&journal),
                    ReplayEvidence {
                        source_eq: Some(true),
                        source_capture: SourceCapture::BeforeInvocation
                    },
                    &AliasContext::default(),
                    None
                )
                .unwrap_err(),
            ApplicabilityError.to_string()
        );
    }
}
