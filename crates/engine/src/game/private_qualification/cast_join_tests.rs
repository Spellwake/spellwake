//! Actual cast receipts at the private adapter; no sealed-recipe reissue is implied.
use super::*;
use crate::{
    game::GameCommand,
    life::LifeAdjustment,
    mana::ManaCost,
    program::{
        Instruction, PlayerRelation, Program, TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
    },
    test_support::characteristic_trajectories as tr,
};

#[test]
fn private_discovery_join_selects_the_captured_stack_identity_and_keeps_its_policies() {
    let body = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::AdjustControllerLife {
            adjustment: crate::life::LifeAdjustment::Gain(1.into()),
        }],
    );
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(7),
        condition: crate::program::TriggerSpec::spell_cast(
            PlayerRelation::Controller,
            crate::program::SpellFilter::any(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: body.clone(),
    });
    let game = tr::world(
        tr::vocabulary(),
        vec![
            tr::card(1, Program::instant(ManaCost::free(), body), false),
            tr::card(2, watcher, true),
        ],
        vec![CardId(2)],
    );
    let hand = tr::object(&game, 1);
    let cast = tr::submit(
        &game,
        SeatId(0),
        GameCommand::CastUntargeted {
            spell: hand,
            payment: None,
        },
    );
    let stack = tr::object(cast.state(), 1);
    assert_ne!(hand, stack);
    let source = tr::object(&game, 2);
    let fixture = json!({"bindings":[{"name":"cast_trigger","when":"first_matching_actual_discovery_in_this_recipe","join":{"source_object_ref":"watcher","triggered_ability_id":7,"related_object_ref":"spell","require_unique":true}}]});
    let joins = |related| Joins {
        objects: BTreeMap::from([("watcher".into(), source), ("spell".into(), related)]),
        ..Joins::default()
    };
    let mut stale = joins(hand);
    stale.observe(&fixture, cast.rules_journal()).unwrap();
    assert!(stale.instances.is_empty());
    let mut actual = joins(stack);
    actual.observe(&fixture, cast.rules_journal()).unwrap();
    assert_eq!(actual.instances.len(), 1);
    let id = actual.instances["cast_trigger"];
    assert!(cast.rules_journal().records.iter().any(|record|matches!(record,crate::occurrence::RulesRecord::TriggerDiscovered(value) if value.id==id)));
    assert_eq!(
        actual.observe(&fixture, cast.rules_journal()),
        Err("nonunique discovery join cast_trigger".into())
    );
    let mut missing = joins(stack);
    missing.objects.remove("spell");
    assert_eq!(
        missing.observe(&fixture, cast.rules_journal()),
        Err("unknown related join".into())
    );
    let mut duplicated = cast.rules_journal().clone();
    let record = duplicated
        .records
        .iter()
        .find(|record| matches!(record, crate::occurrence::RulesRecord::TriggerDiscovered(_)))
        .unwrap()
        .clone();
    duplicated.records.push(record);
    let mut ambiguous = joins(stack);
    assert_eq!(
        ambiguous.observe(&fixture, &duplicated),
        Err("nonunique discovery join cast_trigger".into())
    );
    assert!(ambiguous.instances.is_empty());
}

#[test]
fn actual_private_target_replies_reuse_object_and_entry_joins_and_leave_player_legality_native() {
    use crate::{stack_window::StackKind, targets::TargetRef};
    for stack_target in [false, true] {
        let mut source = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        );
        source.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: crate::program::TriggerSpec::self_cast(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                if stack_target {
                    BodyTarget::SingleSpell(crate::program::SpellFilter::any())
                } else {
                    BodyTarget::SingleCreature
                },
                vec![if stack_target {
                    Instruction::CounterSpell
                } else {
                    Instruction::ReturnToOwnersHand
                }],
            ),
        });
        let game = tr::world(
            tr::vocabulary(),
            vec![
                tr::card(1, source, false),
                tr::card(
                    2,
                    Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
                    true,
                ),
            ],
            vec![CardId(2)],
        );
        let object = tr::object(&game, 2);
        let cast = tr::submit(
            &game,
            SeatId(0),
            GameCommand::CastUntargeted {
                spell: tr::object(&game, 1),
                payment: None,
            },
        );
        let state = cast.state();
        let original = state
            .execution()
            .window()
            .entries()
            .iter()
            .find(|entry| matches!(entry.kind(), StackKind::CardSpell(_)))
            .unwrap()
            .id();
        let mut joins = Joins::default();
        joins.objects.insert("target".into(), object);
        joins.entries.insert("spell".into(), original);
        let descriptor = if stack_target {
            json!({"entry_ref":"spell"})
        } else {
            json!({"captured_object":"target"})
        };
        let action = json!({"reply_kind":"TriggerTarget","target":descriptor});
        let input = reply(state, &action, &json!({}), &joins).unwrap();
        let expected = if stack_target {
            TargetRef::Stack(original)
        } else {
            TargetRef::Object(object)
        };
        assert!(
            matches!(input.command,GameCommand::Reply{choice:DecisionReply::TriggerTarget(target),..} if target==expected)
        );
        let accepted = state.advance(input).unwrap();
        assert!(accepted.state().pending_decision().is_none());
        let saved = state.clone();
        let player = reply(
            state,
            &json!({"reply_kind":"TriggerTarget","target":{"player":"A"}}),
            &json!({}),
            &joins,
        )
        .unwrap();
        assert!(matches!(
            &player.command,
            GameCommand::Reply {
                choice: DecisionReply::TriggerTarget(TargetRef::Player(SeatId(0))),
                ..
            }
        ));
        let result = state.advance(player.clone());
        assert_eq!(result, state.advance(player));
        assert_eq!(result, Err(GameError::Effect(EffectError::IllegalTarget)));
        assert_eq!(state, &saved);
    }
}
