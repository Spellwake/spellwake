//! Private binder mechanisms using genuine vocabulary and actual native owners.
//! These literal Programs are not source-card or full-game qualification.
use super::*;
use crate::{
    game::private_qualification::input::{command, reply},
    mana::ManaCost,
    program::{Instruction, Program},
    test_support::characteristic_trajectories as tr,
};

fn player_spell() -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePlayer,
            vec![crate::program::Instruction::AdjustLife(Box::new(
                crate::toolbox::AdjustLifeSpec::lose(
                    crate::toolbox::PlayerSelector::Resolving(
                        crate::program::ResolvingPlayer::TargetPlayer,
                    ),
                    crate::program::ValueExpr::Literal(1.into()),
                ),
            ))],
        ),
    )
}
fn copier() -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleSpell(crate::program::SpellFilter::any()),
            vec![Instruction::StackOp(Box::new(
                crate::toolbox::StackOpSpec::copy_targeted_spell(),
            ))],
        ),
    )
}
fn player_world() -> GameState {
    tr::world(
        tr::vocabulary(),
        vec![
            tr::card(1, player_spell(), false),
            tr::card(2, copier(), false),
        ],
        vec![],
    )
}
fn root_cast(target: Value) -> Value {
    json!({"op":"cast","actor":"A","source":{"captured_object":"source"},"target":target,"payment":[]})
}

#[test]
fn private_player_target_singleton_binds_recognized_alias_without_participant_prefilter() {
    let game = player_world();
    let saved = game.clone();
    let joins = Joins::default();
    for (name, seat) in [("A", 0), ("B", 1), ("C", 2), ("D", 3), ("E", 4), ("F", 5)] {
        assert_eq!(
            joins.target(&json!({"player":name}), &json!({}), game.execution()),
            Ok(TargetRef::Player(SeatId(seat)))
        );
    }
    assert!(
        !game
            .execution()
            .window()
            .participants()
            .contains(&SeatId(2))
    );
    assert_eq!(game, saved);
}

#[test]
fn private_player_target_rejects_malformed_mixed_and_unknown_alias_references() {
    let game = player_world();
    let source = game.clone();
    let mut joins = Joins::default();
    joins.bind_object("source", tr::object(&game, 1)).unwrap();
    for value in [
        json!({"player":null}),
        json!({"player":0}),
        json!({"player":true}),
        json!({"player":[]}),
        json!({"player":{"seat":"A"}}),
        json!({"player":"A","entry_ref":"bound"}),
        json!({"player":"A","captured_object":"source"}),
        json!({"player":"A","current_owned_action_alias":"P"}),
        json!({"player":"A","actual_draw_from_latest_opening_batch":{}}),
        json!({"player":"A","extra":false}),
    ] {
        assert!(
            joins.target(&value, &json!({}), game.execution()).is_err(),
            "{value}"
        );
        assert!(command(&game, &root_cast(value), &json!({}), &joins).is_err());
    }
    for alias in ["", "G", "a", "A ", "0", "unknown"] {
        assert_eq!(
            joins.target(&json!({"player":alias}), &json!({}), game.execution()),
            Err(format!("unknown participant {alias}"))
        );
    }
    for value in [
        json!({"entry_ref":"missing"}),
        json!({"captured_object":"missing"}),
        json!({"current_owned_action_alias":"missing"}),
    ] {
        assert!(
            joins
                .target(&value, &json!({"seed":{"cards":[]}}), game.execution())
                .is_err()
        );
    }
    assert!(packet::unique_json(br#"{"player":"A","player":"B"}"#).is_err());
    assert_eq!(game, source);
}

#[test]
fn private_root_player_cast_reaches_native_membership_gate_and_preserves_rejected_source() {
    let game = player_world();
    let saved = game.clone();
    let mut joins = Joins::default();
    joins.bind_object("source", tr::object(&game, 1)).unwrap();
    for (name, seat) in [("A", 0), ("B", 1), ("C", 2), ("F", 5)] {
        let input = command(
            &game,
            &root_cast(json!({"player":name})),
            &json!({}),
            &joins,
        )
        .unwrap();
        assert!(
            matches!(&input.command, GameCommand::CastPaid{target,..} if *target == TargetRef::Player(SeatId(seat)))
        );
        let first = game.advance(input.clone());
        assert_eq!(first, game.advance(input));
        if seat < 2 {
            let actual = first.unwrap();
            assert_eq!(
                actual.state().execution().window().entries()[0]
                    .targets()
                    .targets()[0]
                    .choice()
                    .target,
                TargetRef::Player(SeatId(seat))
            );
        } else {
            assert_eq!(first, Err(GameError::Effect(EffectError::IllegalTarget)));
        }
        assert_eq!(game, saved);
    }
}

#[test]
fn private_copy_player_target_reply_preserves_original_binding_and_native_rejection() {
    let game = player_world();
    let original = tr::submit(
        &game,
        SeatId(0),
        GameCommand::CastPaid {
            spell: tr::object(&game, 1),
            target: TargetRef::Player(SeatId(1)),
            payment: ManaPayment::default(),
        },
    );
    let entry = original.state().execution().window().entries()[0].id();
    let copied = tr::submit(
        original.state(),
        SeatId(0),
        GameCommand::CastPaid {
            spell: tr::object(original.state(), 2),
            target: TargetRef::Stack(entry),
            payment: ManaPayment::default(),
        },
    );
    let choosing = tr::pass_pair(copied.state());
    let pending = choosing.state();
    assert!(matches!(
        pending.pending_decision().unwrap().frame(),
        RuleFrame::CopyTargets(_)
    ));
    let saved = pending.clone();
    for (alias, seat) in [("A", 0), ("F", 5)] {
        let input = reply(
            pending,
            &json!({"copy_choice":{"tag":"Change","target":{"player":alias}}}),
            &json!({}),
            &Joins::default(),
        )
        .unwrap();
        assert!(
            matches!(&input.command,GameCommand::Reply{choice:DecisionReply::CopyTargets(crate::effects::CopyTargetChoice::Change(target)),..} if *target==TargetRef::Player(SeatId(seat)))
        );
        let result = pending.advance(input.clone());
        assert_eq!(result, pending.advance(input));
        if seat == 0 {
            let actual = result.unwrap();
            let entries = actual.state().execution().window().entries();
            assert_eq!(
                entries[0].targets().targets()[0].choice().target,
                TargetRef::Player(SeatId(1))
            );
            assert_eq!(
                entries.last().unwrap().targets().targets()[0]
                    .choice()
                    .target,
                TargetRef::Player(SeatId(0))
            );
        } else {
            assert_eq!(result, Err(GameError::Effect(EffectError::IllegalTarget)));
        }
        assert_eq!(pending, &saved);
    }
}

fn selected(actions: Value) -> packet::SelectedInput {
    let vocabulary = tr::vocabulary();
    let names = spellwake_contracts::names::NameRules::literal_names();
    packet::SelectedInput {
        fixture: json!({"scope":"trusted_executor_precheck","seed":{"name_rules":[],"name_rules_id":names.id().to_string(),
            "creature_vocabulary":vocabulary.id().to_string(),"game_id":1,"context":["binder-unit",vocabulary.rules_revision(),"cards","programs","private"],
            "participants":["A","B"],"active":"A","life":{"A":"20","B":"20"},"combat_rules":"UnrestrictedFreeForAll",
            "initial_boundary":{"kind":"TrustedUnitPrecheck"},"floating_mana":[],"initial_world_cohorts":[],"initial_battlefield_order":[],
            "library_order":{"A":[],"B":[]},"cards":[
                {"alias":"P","card_id":1,"owner":"A","controller":null,"zone":"Hand","definition":"P","tapped":null},
                {"alias":"C","card_id":2,"owner":"A","controller":null,"zone":"Hand","definition":"C","tapped":null}]},
            "bindings":[],"actions":actions,"stop":{"after_action_ordinal":actions.as_array().unwrap().len(),"no_extra_pass":true}}),
        definitions: BTreeMap::from([
            (
                "P".into(),
                json!({"native_program":program::view(&player_spell())}),
            ),
            (
                "C".into(),
                json!({"native_program":program::view(&copier())}),
            ),
        ]),
        metadata: Value::Null,
        vocabulary,
    }
}

#[test]
fn private_body_caller_player_cast_reuses_binder_and_retains_actual_native_rejection() {
    for (alias, accepted) in [("B", true), ("C", false)] {
        let input = selected(
            json!([{"op":"trusted_run_existing_body","source_alias":"P","actor":"A","target":{"player":alias},"payment":[],"pass_order":["A","B"],"bind_entry":"original","exact_instruction_reply_table":[],"stop_before_followon_checks":true,"return_at":"first_actual_Pending_or_Complete","no_unlisted_reply":true}]),
        );
        let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
            panic!("wrong actual witness")
        };
        let (source, EffectAttempt::Cast(result), _) = witness
            .attempts
            .iter()
            .find(|(_, attempt, _)| matches!(attempt, EffectAttempt::Cast(_)))
            .unwrap()
        else {
            panic!("adapter intercepted native cast")
        };
        assert_eq!(witness.attempts.len(), witness.repeated_attempts.len());
        for ((_, actual, _), repeated) in witness.attempts.iter().zip(&witness.repeated_attempts) {
            assert_eq!(actual, repeated);
        }
        if accepted {
            assert!(result.is_ok());
            assert_eq!(report["status"], "completed_recipe");
            assert_eq!(
                witness.final_state.unwrap().life_total(SeatId(1)),
                Some(&19.into())
            );
        } else {
            assert_eq!(result.as_ref(), &Err(EffectError::IllegalTarget));
            assert_eq!(witness.final_state.as_deref(), Some(source));
            assert_eq!(report["status"], "failed");
            assert_eq!(
                report["failure"],
                "prescribed actual executor call rejected; actual Result retained"
            );
        }
        for row in report["records"].as_array().unwrap() {
            assert_eq!(row["retained_source_full_state_eq"], true);
            assert_eq!(row["same_source_typed_result_replay_eq"], true);
            assert_eq!(row["retained_source_after_replay_eq"], true);
        }
    }
}

#[test]
fn private_body_caller_copy_player_target_uses_shared_binder_without_adapter_membership_gate() {
    for (alias, accepted) in [("A", true), ("F", false)] {
        let input = selected(
            json!([{"op":"trusted_interaction_inputs","exact_unlisted_instruction_reply_table":[],"stop_before_followon_checks":true,"sequence":[
            {"op":"cast","actor":"A","source":"P","target":{"player":"B"},"payment":[],"bind_entry":"original"},
            {"op":"cast","actor":"A","source":"C","target":{"entry_ref":"original"},"payment":[],"bind_entry":"copier"},
            {"op":"resolve_one","entry_ref":"copier","pass_order":["A","B"]},
            {"op":"reply_copy","actor":"A","choice":"Change","target":{"player":alias},"bind_created_copy":"copy"}]}]),
        );
        let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
            panic!("wrong actual witness")
        };
        let (pending, EffectAttempt::Progress(result), _) = witness.attempts.last().unwrap() else {
            panic!("missing actual CopyTargets reply")
        };
        assert!(witness.attempts.iter().rev().skip(1).any(|(_,attempt,_)|matches!(attempt,EffectAttempt::Progress(result) if matches!(result.as_ref().as_ref().unwrap().result(), RuleResult::Pending{frame:EffectDecision::CopyTargets(_),..}))));
        assert_eq!(witness.attempts.len(), witness.repeated_attempts.len());
        for ((_, actual, _), repeated) in witness.attempts.iter().zip(&witness.repeated_attempts) {
            assert_eq!(actual, repeated);
        }
        if accepted {
            assert!(result.is_ok());
            assert_eq!(report["status"], "completed_recipe");
            let entries = witness.final_state.as_ref().unwrap().window().entries();
            assert_eq!(
                entries[0].targets().targets()[0].choice().target,
                TargetRef::Player(SeatId(1))
            );
            assert_eq!(
                entries.last().unwrap().targets().targets()[0]
                    .choice()
                    .target,
                TargetRef::Player(SeatId(0))
            );
        } else {
            assert_eq!(result.as_ref(), &Err(EffectError::IllegalTarget));
            assert_eq!(witness.final_state.as_deref(), Some(pending));
            assert_eq!(report["status"], "failed");
            assert_eq!(
                report["failure"],
                "prescribed actual executor call rejected; actual Result retained"
            );
        }
    }
}

#[test]
fn private_body_caller_mixed_player_reference_stops_before_any_native_cast() {
    let input = selected(
        json!([{"op":"trusted_run_existing_body","source_alias":"P","actor":"A",
        "target":{"player":"B","extra":true},"payment":[],"pass_order":["A","B"],"bind_entry":"original",
        "exact_instruction_reply_table":[],"stop_before_followon_checks":true,
        "return_at":"first_actual_Pending_or_Complete","no_unlisted_reply":true}]),
    );
    let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
        panic!("wrong actual witness")
    };
    assert_eq!(report["status"], "failed");
    assert_eq!(
        report["failure"],
        "Player target requires exactly one player field"
    );
    assert!(
        witness
            .attempts
            .iter()
            .all(|(_, attempt, _)| !matches!(attempt, EffectAttempt::Cast(_)))
    );
    assert!(witness.final_state.unwrap().window().entries().is_empty());
}

#[test]
fn private_missing_stack_entry_preserves_root_and_body_legacy_diagnostics() {
    let game = player_world();
    let saved = game.clone();
    let mut joins = Joins::default();
    joins.bind_object("source", tr::object(&game, 1)).unwrap();
    let missing = json!({"entry_ref":"missing"});
    assert_eq!(
        joins.target(&missing, &json!({}), game.execution()),
        Err("unbound actual entry".into())
    );
    assert_eq!(
        command(&game, &root_cast(missing.clone()), &json!({}), &joins),
        Err("unbound actual entry".into())
    );
    assert_eq!(game, saved);
    let input = selected(
        json!([{"op":"trusted_run_existing_body","source_alias":"P","actor":"A",
        "target":missing,"payment":[],"pass_order":["A","B"],"bind_entry":"original",
        "exact_instruction_reply_table":[],"stop_before_followon_checks":true,
        "return_at":"first_actual_Pending_or_Complete","no_unlisted_reply":true}]),
    );
    let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
        panic!("wrong actual witness")
    };
    assert_eq!(report["status"], "failed");
    assert_eq!(report["failure"], "unbound actual entry receipt");
    assert!(
        witness
            .attempts
            .iter()
            .all(|(_, attempt, _)| !matches!(attempt, EffectAttempt::Cast(_)))
    );
    assert!(witness.final_state.unwrap().window().entries().is_empty());
}
