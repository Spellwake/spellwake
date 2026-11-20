use super::*;
use crate::ordinary_game::selection::{
    self, SelectionCase,
    data::{SelectionDecisionPlan, SelectionOperation},
    policy_tests,
};
#[test]
#[ignore = "requires genuine native selected frame; complete plan mismatches are pure preparation refusals"]
fn selected_plan_guards_reject_mismatch_without_accepted_progress_or_reply_repair() {
    let run = policy_tests::reached(SelectionCase::Sd04, "primary.select.P1");
    let before = run.policy.selection_observation().unwrap();
    for mutation in 0..7 {
        let mut policy = run.policy.clone();
        let finite = policy.selected_finite_for_test().unwrap();
        let plan = finite
            .recipe
            .decision_plans
            .iter_mut()
            .find_map(|p| match p {
                RuntimeDecisionPlan::Selection(p) => Some(p),
                _ => None,
            })
            .unwrap();
        let SelectionDecisionPlan::ResolutionSelect {
            operation,
            domain,
            required_count,
            chooser,
            entry,
            instruction,
            ..
        } = plan
        else {
            panic!("selection plan")
        };
        match mutation {
            0 => *operation = SelectionOperation::DiscardCards,
            1 => domain.clear(),
            2 => *required_count = 0.into(),
            3 => *chooser = Role::P0,
            4 => {
                *entry = EntryRef::CastEntry {
                    operation: "unbound-entry".into(),
                }
            }
            5 => *instruction = u64::MAX,
            6 => {
                let duplicate = plan.clone();
                finite
                    .recipe
                    .decision_plans
                    .push(RuntimeDecisionPlan::Selection(duplicate));
            }
            _ => unreachable!(),
        }
        assert!(policy.prepare(&run.state).is_err(), "mutation{mutation}");
        assert_eq!(policy.selection_observation().unwrap(), before);
        assert_eq!(policy.consumed(), run.policy.consumed());
    }
}
#[test]
#[ignore = "requires genuine companion accepted history; obsolete handles and per-plan consumed authority"]
fn companion_retains_exact_selection_authority_and_saved_obsolete_handle_after_owner_order() {
    let run = policy_tests::reached(SelectionCase::Sd14RealHistoryCompanion, "primary.select.P1");
    let observed = run.policy.selection_observation().unwrap();
    let history = &observed.consumed["history.select.P1"];
    assert!(matches!(
        history.decision().frame(),
        RuleFrame::ResolutionSelect(_)
    ));
    assert!(
        observed
            .consumed
            .iter()
            .any(|(id, r)| id != "history.select.P1"
                && matches!(r.decision().frame(), RuleFrame::ResolutionGraveyardOrder(_)))
    );
    let prepared = run.policy.prepare(&run.state).unwrap().unwrap();
    let obsolete = prepared
        .recipe_probes()
        .iter()
        .find(|p| p.id == "COMPANION-SD14-GENUINE-OBSOLETE-OBJECT")
        .unwrap();
    let GameCommand::Reply {
        choice: DecisionReply::SelectResolution(objects),
        ..
    } = &obsolete.input.command
    else {
        panic!("obsolete reply")
    };
    assert!(
        objects
            .iter()
            .any(|id| run.state.execution().window().cards().get(*id).is_none())
    );
    let consumed = prepared
        .recipe_probes()
        .iter()
        .find(|p| p.id == "COMPANION-SD14-GENUINE-CONSUMED-D-OLD")
        .unwrap();
    let GameCommand::Reply { decision, .. } = consumed.input.command else {
        panic!("consumed reply")
    };
    assert_eq!(decision, history.decision().id());
    assert_ne!(decision, run.state.pending_decision().unwrap().id());
    let mut missing = run.policy.clone();
    missing
        .selected_finite_for_test()
        .unwrap()
        .progress
        .saved
        .remove("companion-stale-before-history");
    assert!(
        missing
            .prepare(&run.state)
            .unwrap_err()
            .contains("saved actual object absent")
    );
    assert_eq!(missing.selection_observation().unwrap(), observed);
    let mut missing = run.policy.clone();
    missing
        .selected_finite_for_test()
        .unwrap()
        .progress
        .selection_consumed
        .remove("history.select.P1");
    assert!(
        missing
            .prepare(&run.state)
            .unwrap_err()
            .contains("MissingConsumedPlan")
    );
}
#[test]
#[ignore = "requires genuine current/owner-order decisions; missing and colliding authorities are preparation errors"]
fn pure_lowering_refuses_missing_authorities_and_actual_foreign_decision_collision() {
    use selection::{
        data::*,
        probes::{self, ProbeAuthorities, ProbePreparationError},
    };
    let mut run = policy_tests::reached(SelectionCase::Sd14, "primary.owner-order.P1");
    let current = run.state.pending_decision().unwrap().clone();
    let finite = run.policy.selected_finite_for_test().unwrap();
    let initializer = finite.initializer.as_ref().unwrap().clone();
    let prepared = run.policy.prepare(&run.state).unwrap().unwrap();
    let reached = prepared.selection_metadata().unwrap().reached.clone();
    let foreign = initializer.foreign.as_ref().unwrap();
    assert_ne!(foreign.basis().game, current.basis().game);
    assert_eq!(
        foreign.decision().id(),
        current.id(),
        "this control uses two actual owner-order decisions, without identity arithmetic"
    );
    let recipe = selection::recipes::recipe(SelectionCase::Sd14).unwrap();
    let SelectionDecisionPlan::ResolutionSelect {
        probes: declarations,
        ..
    } = &recipe.decision_plans[0]
    else {
        panic!("main declarations")
    };
    let declaration = |id: &str| {
        declarations
            .iter()
            .find(|p| match p {
                SelectionProbeDeclaration::Execute { obligation, .. } => obligation.as_str() == id,
                SelectionProbeDeclaration::PendingExactOriginalHistory { obligation, .. } => {
                    obligation.as_str() == id
                }
            })
            .unwrap()
            .clone()
    };
    let empty = BTreeMap::new();
    let never_object = |_: &ObjectRef| -> Result<selection::probes::ResolvedObject, String> {
        panic!("authority error must precede object resolution")
    };
    let never_target = |_: &Target| -> Result<selection::probes::ResolvedTarget, String> {
        panic!("authority error must precede target resolution")
    };
    let never_ability = |_: &Ability| -> Result<ManaAbilityRef, String> {
        panic!("authority error must precede ability resolution")
    };
    let result = probes::lower(
        &[declaration("SD14-V7")],
        ProbeAuthorities {
            reached: &reached,
            initializer: None,
            consumed: &empty,
            foreign: None,
        },
        never_object,
        never_target,
        never_ability,
    );
    assert_eq!(
        result.unwrap_err(),
        ProbePreparationError::MissingInitializerAuthority
    );
    let result = probes::lower(
        &[declaration("SD14-V10")],
        ProbeAuthorities {
            reached: &reached,
            initializer: Some(&initializer.basis),
            consumed: &empty,
            foreign: None,
        },
        never_object,
        never_target,
        never_ability,
    );
    assert_eq!(
        result.unwrap_err(),
        ProbePreparationError::InvalidForeignHelper
    );
    let result = probes::lower(
        &[declaration("SD14-V10")],
        ProbeAuthorities {
            reached: &reached,
            initializer: Some(&initializer.basis),
            consumed: &empty,
            foreign: Some(foreign),
        },
        never_object,
        never_target,
        never_ability,
    );
    assert_eq!(
        result.unwrap_err(),
        ProbePreparationError::CollidingForeignDecision
    );
    let pending = probes::lower(
        &[declaration("SD14-V9")],
        ProbeAuthorities {
            reached: &reached,
            initializer: None,
            consumed: &empty,
            foreign: None,
        },
        never_object,
        never_target,
        never_ability,
    )
    .unwrap();
    assert_eq!(pending.pending.len(), 1);
    assert!(pending.probes.is_empty() && pending.receipts.is_empty());
}

#[test]
#[ignore = "requires genuine main/helper results; caller-refusal retention mechanism, not a native gameplay failure"]
fn helper_preparation_refusal_retains_main_auxiliary_result_and_is_atomic() {
    use selection::authority::{self, ForeignAuthority, SelectionBindingError};
    let source = policy_tests::genuine();
    let fixture = source
        .fixture(SelectionCase::Sd04, source.vocabulary().clone())
        .unwrap();
    let initializer = authority::initialize(&fixture).unwrap();
    let mut policy = crate::ordinary_game::policy::Policy::new(&fixture).unwrap();
    let before = policy.selection_observation().unwrap();
    let error = policy
        .selected_finite_for_test()
        .unwrap()
        .bind_selection_initializer_using(source, initializer.state(), &initializer, |source| {
            let actual = ForeignAuthority::prepare(source)?;
            assert!(actual.trace.initialization.is_ok());
            assert!(
                actual
                    .trace
                    .calls
                    .iter()
                    .all(|call| call.result.is_ok() && call.installed)
            );
            Err(SelectionBindingError {
                message:
                    "caller mechanism declines genuine helper authority after its actual prefix"
                        .into(),
                validation: None,
                helper: Some(Box::new(actual.trace)),
            })
        })
        .unwrap_err();
    assert_eq!(error.validation.as_deref(), Some(&Ok(initializer.clone())));
    let helper = error.helper.as_ref().unwrap();
    assert_eq!(helper.calls.len(), 4);
    assert!(helper.validation.as_ref().unwrap().is_ok());
    assert!(
        helper
            .calls
            .iter()
            .all(|call| call.result.is_ok() && call.installed)
    );
    assert_eq!(policy.selection_observation().unwrap(), before);
    assert_eq!(policy.consumed(), 0);
    assert!(
        policy
            .selected_finite_for_test()
            .unwrap()
            .initializer
            .is_none()
    );
    assert!(
        policy
            .prepare(initializer.state())
            .unwrap_err()
            .contains("initializer authority")
    );
}

#[test]
#[ignore = "requires genuine current and saved origins; retained typed metadata after commit"]
fn retained_probe_metadata_links_reached_work_attempted_objects_and_committed_ordinals() {
    use selection::probes::{ObjectOrigin, ResolvedAttempt, ResolvedReply};
    let mut run = policy_tests::reached(SelectionCase::Sd04, "primary.select.P1");
    let prepared = run.policy.prepare(&run.state).unwrap().unwrap();
    let original = prepared.selection_metadata().unwrap().receipts.to_vec();
    let outside = original
        .iter()
        .find(|r| r.obligation.as_str() == "SD04-V8")
        .unwrap();
    let ResolvedAttempt::Reply {
        choice: ResolvedReply::SelectResolution(objects),
        ..
    } = &outside.attempted
    else {
        panic!("select attempt")
    };
    let object = &objects[0];
    let RuleFrame::ResolutionSelect(frame) = outside.reached.decision.frame() else {
        panic!("attachment select")
    };
    assert!(!frame.domain().contains(&object.object));
    let ObjectOrigin::CurrentPhysical { actual, .. } = object.origin.as_ref() else {
        panic!("current graveyard origin")
    };
    assert!(matches!(actual.zone(), ObjectZone::Graveyard(_)));
    assert_eq!(actual.id(), object.object);
    let duplicate = original
        .iter()
        .find(|r| r.obligation.as_str() == "SD04-V5")
        .unwrap();
    let ResolvedAttempt::Reply {
        choice: ResolvedReply::SelectResolution(rows),
        ..
    } = &duplicate.attempted
    else {
        panic!("duplicates")
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], rows[1]);
    let reached = outside.reached.clone();
    let before = run.policy.selection_observation().unwrap();
    let input = run.policy.issue(&run.state, prepared).unwrap();
    let rejected = run.state.advance(outside.probe.input.clone());
    assert_eq!(rejected.unwrap_err(), outside.probe.expected);
    assert_eq!(run.policy.selection_observation().unwrap(), before);
    let candidate = run.state.advance(input.clone()).unwrap();
    let ticket = run.policy.prepare_accept(&input, &candidate).unwrap();
    drop(ticket);
    assert_eq!(run.policy.selection_observation().unwrap(), before);
    let ticket = run.policy.prepare_accept(&input, &candidate).unwrap();
    run.state = candidate.into_state();
    ticket.commit();
    let observed = run.policy.selection_observation().unwrap();
    assert_eq!(observed.receipts, original);
    assert_eq!(observed.consumed["primary.select.P1"].reached, reached);
    assert_eq!(
        observed.consumed["primary.select.P1"].accepted.ordinal,
        u64::try_from(run.policy.consumed()).unwrap()
    );
    assert!(
        run.policy
            .selected_finite_for_test()
            .unwrap()
            .progress
            .probes
            .is_empty()
    );
    let mut companion =
        policy_tests::reached(SelectionCase::Sd14RealHistoryCompanion, "primary.select.P1");
    assert!(policy_tests::accept_next(&mut companion, true));
    let observed = companion.policy.selection_observation().unwrap();
    let receipt = observed
        .receipts
        .iter()
        .find(|r| r.obligation.as_str() == "COMPANION-SD14-GENUINE-OBSOLETE-OBJECT")
        .unwrap();
    let ResolvedAttempt::Reply {
        choice: ResolvedReply::SelectResolution(objects),
        ..
    } = &receipt.attempted
    else {
        panic!("obsolete attempt")
    };
    assert!(objects.iter().any(|r|matches!(r.origin.as_ref(),ObjectOrigin::Saved{binding,authority} if binding=="companion-stale-before-history"&&authority.object.id()==r.object&&companion.state.execution().window().cards().get(r.object).is_none())));
    let consumed = observed
        .receipts
        .iter()
        .find(|r| r.obligation.as_str() == "COMPANION-SD14-GENUINE-CONSUMED-D-OLD")
        .unwrap();
    assert_eq!(consumed.consumed_links.len(), 1);
    assert_eq!(
        consumed.consumed_links[0].accepted,
        observed.consumed["history.select.P1"].accepted
    );
    assert_eq!(
        consumed.consumed_links[0].reached,
        observed.consumed["history.select.P1"].reached
    );
}
#[test]
#[ignore = "requires genuine four-call helper; matched owner-order stop and refusal trace retention"]
fn helper_stop_uses_shared_matcher_and_preserves_unconsumed_order_on_refusal() {
    use selection::authority::ForeignAuthority;
    let source = policy_tests::genuine();
    let valid = ForeignAuthority::prepare(source).unwrap();
    let binding = valid.binding();
    assert_eq!(valid.trace.calls.len(), 4);
    assert!(!valid.trace.consumed_plans.contains_key(&binding.plan_id));
    assert!(valid.trace.calls.iter().all(|call| !matches!(
        call.input.command,
        GameCommand::Reply {
            choice: DecisionReply::OrderResolutionGraveyard(_),
            ..
        }
    )));
    for duplicate in [false, true] {
        let error = ForeignAuthority::prepare_with_stop_annotation(source, |policy| {
            let finite = policy.selected_finite_for_test().unwrap();
            let index = finite
                .recipe
                .decision_plans
                .iter()
                .position(|p| {
                    matches!(
                        p,
                        RuntimeDecisionPlan::Selection(
                            SelectionDecisionPlan::ResolutionGraveyardOrder { .. }
                        )
                    )
                })
                .unwrap();
            if duplicate {
                finite
                    .recipe
                    .decision_plans
                    .push(finite.recipe.decision_plans[index].clone());
            } else {
                let RuntimeDecisionPlan::Selection(
                    SelectionDecisionPlan::ResolutionGraveyardOrder { domain, .. },
                ) = &mut finite.recipe.decision_plans[index]
                else {
                    unreachable!()
                };
                domain.clear();
            }
            Ok(())
        })
        .unwrap_err();
        let trace = error.helper.unwrap();
        assert_eq!(trace.calls.len(), 4);
        assert!(
            trace
                .calls
                .iter()
                .all(|call| call.result.is_ok() && call.installed)
        );
        assert!(trace.stop.is_some());
        assert!(trace.stop_binding.is_none());
        assert!(!trace.consumed_plans.contains_key(&binding.plan_id));
        assert_eq!(
            trace.consumed_plans["primary.select.P1"].accepted.ordinal,
            4
        );
    }
}
