//! Self-contained regressions; frozen packet execution is opt-in and separately captured.
use super::*;

#[test]
fn private_root_token_receipt_obligations_cannot_succeed_at_an_unbound_stop() {
    // Adapter-only synthetic data: no card or rule qualification is inferred.
    let vocabulary = crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr");
    let mut selected = packet::SelectedInput {
        fixture: json!({
            "scope":"native_only_private_fault_at_reached_owner_state",
            "seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),
                "creature_vocabulary":vocabulary.id().to_string(),"game_id":20,"context":["unit","cr","cards","programs","private"],
                "participants":["A","B"],"active":"A","life":{"A":"20","B":"20"},
                "combat_rules":"UnrestrictedFreeForAll","initial_boundary":{"kind":"Interaction"},
                "floating_mana":[],"initial_world_cohorts":[],"initial_battlefield_order":[],"library_order":{"A":[],"B":[]},"cards":[]
            },
            "bindings":[], "actions":[], "stop":{"after_action_ordinal":0,"no_extra_pass":true}
        }),
        definitions: BTreeMap::new(),
        metadata: Value::Null,
        vocabulary,
    };
    assert_eq!(
        root::run(&selected).unwrap().0["status"],
        "completed_recipe"
    );
    for binding in [
        json!({"name":"unbound","when":"matching_actual_token_created","join":{"token_created_by_entry_ref":"missing","instruction":"0","birth_ordinal":"0","field":"after_object","require_unique":true}}),
        json!({"name":"unbound","when":"first_matching_actual_birth_discovery_in_this_recipe","join":{"source_object_ref":"missing","triggered_ability_id":7,"related_token_after_ref":"missing","require_unique":true}}),
    ] {
        selected.fixture["bindings"] = json!([binding]);
        let (report, _) = root::run(&selected).unwrap();
        assert_eq!(report["status"], "failed");
        assert_eq!(report["stop_reached"], false);
        assert_eq!(report["failure"], "unbound exact token receipt unbound");
        assert_eq!(report["records"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn actual_checked_occurrence_helper_has_finite_boundaries_without_a_game_or_vector() {
    assert_eq!(crate::occurrence::next_occurrence_index(0), Ok((0, 1)));
    assert_eq!(
        crate::occurrence::next_occurrence_index(u64::MAX - 1),
        Ok((u64::MAX - 1, u64::MAX))
    );
    assert_eq!(
        crate::occurrence::next_occurrence_index(u64::MAX),
        Err(TriggerError::OccurrenceIndexExhausted)
    );
    assert_eq!(
        crate::occurrence::validate_event_shape(&[]),
        Err(TriggerError::EmptyEvent)
    );
}

#[test]
fn two_actual_pre_color_events_accept_the_max_minus_two_prefix_before_later_failure() {
    use crate::{
        mana::{ManaAbility, ManaOutput, ManaType},
        occurrence::{RulesOccurrence, TriggerError},
        program::{Program, TriggeredAbility, TriggeredAbilityId},
    };
    let a = SeatId(0);
    let b = SeatId(1);
    let mut land = Program::land(
        std::collections::BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::ChooseColor {
                amount: 1.into(),
                colors: BTreeSet::from([ManaType::Green, ManaType::Blue]),
            },
        }],
    );
    land.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition: crate::program::TriggerSpec::becomes_tapped(
            crate::program::ObjectFilter::self_object(),
        ),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![crate::program::Instruction::DrawController {
                amount: crate::program::ValueExpr::Literal(1.into()),
            }],
        ),
    });
    let mut state = GameState::initialize(
        GameId(5138),
        GameContext::new(
            "unit".into(),
            "cr".into(),
            "cards".into(),
            "programs".into(),
            "private-unit".into(),
            crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr").id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap(),
        GameSeed::with_uniform_life(
            vec![a, b],
            a,
            vec![SeedCard {
                card: CardId(1380001),
                owner: a,
                destination: ObjectDestination::Battlefield { controller: a },
                program: land,
            }],
            vec![CardId(1380001)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr"),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ),
    )
    .unwrap()
    .into_state();
    root::fault(&mut state, "TriggerState", "next_event", u64::MAX - 2).unwrap();
    let source = state
        .execution
        .window()
        .cards()
        .current_identity(CardId(1380001))
        .unwrap();
    let paid = state
        .advance(GameInput {
            basis: state.basis.clone(),
            actor: a,
            command: GameCommand::ActivateMana {
                source,
                ability: crate::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
            },
        })
        .unwrap();
    let ids = paid
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(event.id.sequence()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![u64::MAX - 2, u64::MAX - 1]);
    assert!(paid.rules_journal().records.iter().any(|record|matches!(record,RulesRecord::Event(event) if matches!(event.occurrences[0].value,RulesOccurrence::StatusChange { .. }))));
    let pending = paid.into_state();
    assert_eq!(
        pending.execution.operational_counters().next_event,
        u64::MAX
    );
    assert_eq!(pending.execution.waiting_triggers().count(), 1);
    assert_eq!(
        pending
            .execution
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(true)
    );
    let decision = pending.pending_decision().unwrap();
    assert!(matches!(decision.frame(), RuleFrame::ManaColor(_)));
    let saved = pending.clone();
    assert_eq!(
        pending.advance(GameInput {
            basis: pending.basis.clone(),
            actor: a,
            command: GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::ManaColor(ManaType::Blue)
            }
        }),
        Err(GameError::Effect(EffectError::Trigger(
            TriggerError::EventIdentityExhausted
        )))
    );
    assert_eq!(pending, saved);
    assert_eq!(pending.execution.mana(a, ManaType::Blue), 0.into());
    assert!(pending.execution.window().entries().is_empty());
}

#[test]
fn unexpected_adopted_rejection_preserves_actual_result_and_the_accepted_paid_prefix() {
    use crate::{
        mana::{ManaAbility, ManaOutput, ManaType},
        program::Program,
    };
    let land = Program::land(
        std::collections::BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::ChooseColor {
                amount: 1.into(),
                colors: BTreeSet::from([ManaType::Green, ManaType::Blue]),
            },
        }],
    );
    let selected = packet::SelectedInput {
        fixture: json!({
            "scope":"native_only_private_fault_at_reached_owner_state",
            "seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),
                "creature_vocabulary":crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr").id().to_string(),"game_id":19,"context":["unit","cr","cards","programs","private"],
                "participants":["A","B"],"active":"A","life":{"A":"20","B":"20"},
                "combat_rules":"UnrestrictedFreeForAll","initial_boundary":{"kind":"Interaction"},
                "floating_mana":[],"initial_world_cohorts":[],"initial_battlefield_order":["M"],"library_order":{"A":[],"B":[]},
                "cards":[{"alias":"M","card_id":1,"owner":"A","controller":"A","zone":"Battlefield","definition":"land","tapped":false}]
            },
            "bindings":[],
            "actions":[
                {"op":"activate_mana","actor":"A","source":{"current_owned_action_alias":"M"},"ability":{"tag":"Printed","id":1}},
                {"op":"activate_mana","actor":"A","source":{"current_owned_action_alias":"M"},"ability":{"tag":"Printed","id":1}}
            ],
            "stop":{"after_action_ordinal":2,"no_extra_pass":true}
        }),
        definitions: BTreeMap::from([(
            "land".into(),
            json!({"native_program":program::view(&land)}),
        )]),
        metadata: Value::Null,
        vocabulary: crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr"),
    };
    let (report, witness) = root::run(&selected).unwrap();
    assert_eq!(report["status"], "failed");
    assert_eq!(report["completed_action_ordinal"], 1);
    assert_eq!(report["failed_action_ordinal"], 2);
    assert_eq!(report["stop_reached"], false);
    assert_eq!(report["records"].as_array().unwrap().len(), 3);
    assert_eq!(report["records"][2]["result"]["tag"], "Err");
    let Witness::Root {
        initialization,
        attempts,
        final_state,
    } = witness
    else {
        panic!("wrong witness");
    };
    assert!(initialization.is_ok());
    assert_eq!(attempts.len(), 2);
    assert!(attempts[0].2.is_ok());
    assert!(attempts[1].2.is_err());
    let accepted = attempts[0].2.as_ref().unwrap().state();
    assert_eq!(*final_state.unwrap(), *accepted);
    assert!(accepted.pending_decision().is_some());
    assert_eq!(
        accepted
            .execution
            .window()
            .cards()
            .get(
                accepted
                    .execution
                    .window()
                    .cards()
                    .current_identity(CardId(1))
                    .unwrap()
            )
            .unwrap()
            .tapped(),
        Some(true)
    );
}

#[test]
fn standalone_error_observations_keep_the_actual_constructor_and_primitive_domains() {
    use crate::{
        object_identity::GameObjects,
        program::{PermanentDefinition, Program, ProgramKind},
        stack_window::StackWindow,
    };
    let invalid = Program::new(
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: None,
            casting_cost: None,
        }),
    );
    let error = invalid
        .validate(&crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr"))
        .unwrap_err();
    let observed = projection::definition_error(&error);
    assert_eq!(observed["error_domain"], "DefinitionError");
    assert_eq!(observed["error_path"], json!(["CardTypes"]));
    assert_eq!(observed["error"], format!("{error:?}"));

    let a = SeatId(0);
    let error = StackWindow::new(vec![a, a], a, GameObjects::new(0)).unwrap_err();
    let observed = projection::stack_window_error(&error);
    assert_eq!(observed["error_domain"], "StackWindowError");
    assert_eq!(observed["error_path"], json!(["Priority", "InvalidOrder"]));
    assert_eq!(observed["error"], format!("{error:?}"));

    let cards = GameObjects::new(0)
        .seed_card(0, CardId(1), a, ObjectDestination::Hand)
        .unwrap()
        .next;
    let error = cards
        .seed_card(cards.revision(), CardId(1), a, ObjectDestination::Hand)
        .unwrap_err();
    let observed = projection::object_identity_error(&error);
    assert_eq!(observed["error_domain"], "ObjectIdentityError");
    assert_eq!(observed["error_path"], json!(["CardAlreadyPresent"]));
    assert_eq!(observed["error"], format!("{error:?}"));
}

#[test]
fn legend_projection_reply_codec_and_root_preserve_nonfirst_conflict_and_unresolved_domain() {
    use crate::{
        checks::{CheckChoice, CheckDecision},
        program::{Program, Supertype},
    };
    use spellwake_contracts::names::{EnglishCardName, NameRules, ObjectNames};
    let a = SeatId(0);
    let b = SeatId(1);
    let vocabulary = crate::test_support::characteristic_trajectories::vocabulary();
    let labels: [&[&str]; 3] = [&["Alpha"], &["Alpha", "Beta"], &["Beta"]];
    let cards = labels
        .iter()
        .enumerate()
        .map(|(i, labels)| {
            let mut p = Program::artifact(None, vec![]);
            p.supertypes.insert(Supertype::Legendary);
            p.names = ObjectNames::new(
                labels
                    .iter()
                    .map(|n| EnglishCardName::new((*n).into()).unwrap())
                    .collect(),
            )
            .unwrap();
            SeedCard {
                card: CardId(u64::try_from(i).unwrap() + 1),
                owner: a,
                destination: ObjectDestination::Battlefield { controller: a },
                program: p,
            }
        })
        .collect();
    let rules = std::sync::Arc::new(NameRules::literal_names());
    let context = GameContext::new(
        "legend-codec".into(),
        vocabulary.rules_revision().into(),
        "literal".into(),
        "legend".into(),
        "prepared".into(),
        vocabulary.id(),
        rules.id(),
    )
    .unwrap();
    let source = GameState::initialize(
        GameId(7045),
        context,
        GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(3), CardId(2), CardId(1)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            rules,
        ),
    )
    .unwrap()
    .into_state();
    let RuleFrame::Checks(check) = source.pending_decision().unwrap().frame() else {
        panic!()
    };
    let CheckDecision::Legend(f) = check else {
        panic!()
    };
    let conflicts = f.conflicts().to_vec();
    let view = serde_json::to_value(
        crate::test_support::check_trace::check_decision_view(
            check,
            source.execution.window().cards(),
            |c| c.0.to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(view["kind"], "legend_choice");
    assert!(view.get("domain").is_none());
    assert!(view.get("required").is_none());
    assert_eq!(view["conflicts"].as_array().unwrap().len(), 2);
    let roundtripped: Value = serde_json::from_str(&serde_json::to_string(&view).unwrap()).unwrap();
    let fixture = json!({"seed":{"cards":[{"alias":"A","card_id":1},{"alias":"AB","card_id":2},{"alias":"B","card_id":3}]}});
    let joins = input::Joins::default();
    let action = json!({"kind":"KeepLegend","actor":"A","conflict":roundtripped["conflicts"][1]["id"],"keep":{"current_owned_action_alias":"AB"}});
    let decoded = input::reply(&source, &action, &fixture, &joins).unwrap();
    let object2 = source
        .execution
        .window()
        .cards()
        .current_identity(CardId(2))
        .unwrap();
    assert!(
        matches!(decoded.command,GameCommand::Reply{choice:DecisionReply::KeepLegend{conflict,keep},..} if conflict==conflicts[1].id() && keep==object2)
    );
    let accepted = source.advance(decoded).unwrap();
    assert!(accepted.check_journal().moves.is_empty());
    assert!(
        matches!(accepted.check_journal().choices[0].choice,CheckChoice::LegendKeep{keep,..} if keep==object2)
    );
    let record = serde_json::to_value(
        crate::test_support::check_trace::check_journal_view(
            accepted.check_journal(),
            source.execution.window().cards(),
            |c| c.0.to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(record["choices"][0]["kind"], "legend_keep");
    assert!(record["choices"][0].get("ordered").is_none());
    assert_eq!(record["choices"][0]["domain"].as_array().unwrap().len(), 2);
    assert_eq!(record["choices"][0]["keep"]["alias"], "2");
    let RuleFrame::Checks(next) = accepted.state().pending_decision().unwrap().frame() else {
        panic!()
    };
    let CheckDecision::Legend(next) = next else {
        panic!()
    };
    assert_eq!(next.conflicts(), std::slice::from_ref(&conflicts[0]));
    assert!(next.conflicts()[0].domain().contains(&object2));
    for mutation in 0..7 {
        let mut bad = action.clone();
        match mutation {
            0 => {
                bad.as_object_mut().unwrap().remove("conflict");
            }
            1 => {
                bad["conflict"].as_object_mut().unwrap().remove("event");
            }
            2 => {
                bad["conflict"]["extra"] = json!(0);
            }
            3 => {
                bad["conflict"]["ordinal"] = json!(-1);
            }
            4 => {
                bad["conflict"]["ordinal"] = json!("1");
            }
            5 => {
                bad["items"] = json!([{"current_owned_action_alias":"AB"}]);
            }
            6 => {
                bad.as_object_mut().unwrap().remove("keep");
            }
            _ => unreachable!(),
        }
        assert!(
            input::reply(&source, &bad, &fixture, &joins).is_err(),
            "mutation{mutation}"
        );
    }
}
