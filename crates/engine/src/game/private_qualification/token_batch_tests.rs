//! Actual private dispatch/serializer regressions; synthetic data is not card qualification.
use super::*;
use crate::{
    mana::ManaCost,
    program::{Instruction, Program},
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients},
};
use spellwake_contracts::names::EnglishCardName;

pub(super) fn fixture(boundary: &str, actions: Value) -> packet::SelectedInput {
    let vocabulary = creature_types::synthetic_vocabulary_for_rules("cr");
    let definition = TokenDefinition::new(
        Program::artifact(None, vec![]),
        TokenName::Specified(EnglishCardName::new("Private Fixture Token".into()).unwrap()),
        &vocabulary,
    )
    .unwrap();
    let producer = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens {
                creations: vec![TokenCreation {
                    definition,
                    amount: crate::program::ValueExpr::Literal(2.into()),
                    base_size: None,
                    enters_tapped: false,
                    recipients: TokenRecipients::Controller,
                }],
            }],
        ),
    );
    packet::SelectedInput {
        fixture: json!({
            "scope":"native_only_private_fault_at_reached_owner_state",
            "seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),
                "creature_vocabulary":vocabulary.id().to_string(),"game_id":21,"context":["unit","cr","cards","programs","private"],
                "participants":["A","B"],"active":"A","life":{"A":"20","B":"20"},
                "combat_rules":"UnrestrictedFreeForAll","initial_boundary":{"kind":boundary},
                "floating_mana":[],"initial_world_cohorts":[],"initial_battlefield_order":[],"library_order":{"A":[],"B":[]},
                "cards":[{"alias":"P","card_id":1,"owner":"A","controller":null,"zone":"Hand","definition":"producer"}]
            },
            "bindings":[{"name":"second","when":"matching_actual_token_created","join":{"token_created_by_entry_ref":"producer","instruction":"0","birth_ordinal":"1","field":"after_object","require_unique":true}}],
            "stop":{"after_action_ordinal":actions.as_array().unwrap().len(),"no_extra_pass":true},
            "actions":actions
        }),
        definitions: BTreeMap::from([(
            "producer".into(),
            json!({"native_program":program::view(&producer)}),
        )]),
        metadata: Value::Null,
        vocabulary,
    }
}

#[test]
fn private_root_timestamp_order_serializes_the_actual_frame_and_binds_second_birth() {
    let selected = fixture(
        "Interaction",
        json!([
            {"op":"cast","actor":"A","source":{"current_owned_action_alias":"P"},"target":null,"payment":[],"bind_entry":"producer"},
            {"op":"pass_round","actors":["A","B"]},
            {"op":"reply_exact_current_frame","actor":"A","kind":"TimestampOrder","items":[1,0]}
        ]),
    );
    let (
        report,
        Witness::Root {
            attempts,
            final_state,
            ..
        },
        _,
    ) = root::run_extended(&selected).unwrap()
    else {
        panic!("wrong actual private witness")
    };
    assert_eq!(report["status"], "completed_recipe", "{report}");
    let (pending, input, result) = attempts.last().unwrap();
    let decision = pending.pending_decision().unwrap();
    let RuleFrame::TimestampOrder(frame) = decision.frame() else {
        panic!("not the actual timestamp frame")
    };
    assert_eq!(
        pending
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token())
            .count(),
        0
    );
    let encoded = projection::frame(decision.frame(), pending).unwrap();
    assert_eq!(
        encoded,
        json!({
            "tag":"TimestampOrder","revision":frame.revision().to_string(),
            "entry":format!("{:?}",frame.entry()),"instruction":frame.instruction(),"chooser":frame.chooser().0,
            "members":frame.members().iter().map(|m|json!({"ordinal":m.member.ordinal(),"creator":m.creator.0,"definition":program::view(&m.definition)})).collect::<Vec<_>>()
        })
    );
    assert_eq!(
        projection::state(pending, &packet::AliasContext::default()).unwrap()["decision"]["frame"],
        encoded
    );
    assert_eq!(
        projection::input(input)["command"]["choice"],
        json!({"tag":"TimestampOrder","order":[1,0]})
    );
    let candidate = result.as_ref().unwrap();
    let births = crate::test_support::receipt_joins::token_creations(
        candidate.rules_journal(),
        frame.entry(),
        0,
    )
    .unwrap()
    .unwrap();
    assert_eq!(births.len(), 2);
    let mut joins = Joins::default();
    joins.bind_entry("producer", frame.entry()).unwrap();
    joins
        .observe(&selected.fixture, candidate.rules_journal())
        .unwrap();
    assert_eq!(
        joins
            .object(
                &json!({"captured_object":"second"}),
                &selected.fixture,
                candidate.state().execution()
            )
            .unwrap(),
        births[1]
    );
    assert_eq!(
        final_state
            .unwrap()
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token())
            .count(),
        2
    );
    for bad_order in [json!(["1", "0"]), json!([1.5, 0]), json!([-1, 0])] {
        let mut bad = selected.clone();
        bad.fixture["actions"][2]["items"] = bad_order;
        assert!(
            root::run_extended(&bad)
                .unwrap_err()
                .contains("timestamp member")
        );
    }
    let mut bad = selected;
    bad.fixture["bindings"][0]["join"]["birth_ordinal"] = json!("2");
    assert_eq!(root::run_extended(&bad).unwrap().0["status"], "failed");
}

pub(super) fn grouped_fixture(boundary: &str, actions: Value) -> packet::SelectedInput {
    let mut input = fixture(boundary, actions);
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(crate::program::TriggeredAbility {
        id: crate::program::TriggeredAbilityId(9),
        condition: crate::program::TriggerSpec::player_action(
            crate::program::PlayerRelation::Controller,
            crate::program::PlayerTriggerAction::TokenCreated,
        ),
        grouping: crate::program::TriggerGrouping::OneOrMoreTogether,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![crate::program::Instruction::AdjustLife(Box::new(
                crate::toolbox::AdjustLifeSpec::gain(
                    crate::toolbox::PlayerSelector::Resolving(
                        crate::program::ResolvingPlayer::Controller,
                    ),
                    crate::program::ValueExpr::Literal(1.into()),
                ),
            ))],
        ),
    });
    input.definitions.insert(
        "watcher".into(),
        json!({"native_program":program::view(&watcher)}),
    );
    input.fixture["seed"]["cards"].as_array_mut().unwrap().push(json!({"alias":"W","card_id":2,"owner":"A","controller":"A","zone":"Battlefield","tapped":false,"definition":"watcher"}));
    input.fixture["seed"]["initial_battlefield_order"] = json!(["W"]);
    input.fixture["bindings"].as_array_mut().unwrap().extend([
        json!({"name":"watcher_source","when":"after_seed_before_any_turn_work","join":{"card_id":2,"require_exact_current_incarnation":true}}),
        json!({"name":"first","when":"matching_actual_token_created","join":{"token_created_by_entry_ref":"producer","instruction":"0","birth_ordinal":"0","field":"after_object","require_unique":true}}),
        json!({"name":"first_group","when":"first_matching_actual_birth_discovery_in_this_recipe","join":{"source_object_ref":"watcher_source","triggered_ability_id":9,"related_token_after_ref":"first","require_unique":true}}),
        json!({"name":"second_group","when":"first_matching_actual_birth_discovery_in_this_recipe","join":{"source_object_ref":"watcher_source","triggered_ability_id":9,"related_token_after_ref":"second","require_unique":true}}),
    ]);
    input
}

#[test]
fn private_root_grouped_births_bind_both_members_and_serialize_complete_installed_causes() {
    let input = grouped_fixture(
        "Interaction",
        json!([
            {"op":"cast","actor":"A","source":{"current_owned_action_alias":"P"},"target":null,"payment":[],"bind_entry":"producer"},
            {"op":"pass_round","actors":["A","B"]},
            {"op":"reply_exact_current_frame","actor":"A","kind":"TimestampOrder","items":[1,0]}
        ]),
    );
    let (
        report,
        Witness::Root {
            attempts,
            final_state,
            ..
        },
        _,
    ) = root::run_extended(&input).unwrap()
    else {
        panic!("root witness");
    };
    assert_eq!(report["status"], "completed_recipe", "{report}");
    let candidate = attempts.last().unwrap().2.as_ref().unwrap();
    let captures = candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|r| {
            if let crate::occurrence::RulesRecord::TriggerDiscovered(w) = r {
                Some(w.as_ref())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].causes.len(), 2);
    let row = projection::waiting(captures[0]);
    let state = final_state.unwrap();
    let entry = state.execution().window().entries()[0].id();
    assert_eq!(
        state
            .execution()
            .execution_bindings(entry)
            .unwrap()
            .trigger
            .as_ref()
            .unwrap()
            .causes,
        captures[0].causes
    );
    assert_eq!(
        report["final_state"]["stack"][0]["bindings"]["trigger"]["causes"],
        row["causes"]
    );
    assert!(
        report["final_state"]["waiting"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
