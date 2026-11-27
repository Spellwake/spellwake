//! Admission and strict-codec boundaries for the forward-only conditional IR.
use super::*;
use crate::test_support::{creature_types, program as codec};
use serde_json::json;

fn condition(left: ValueExpr) -> ResolutionCondition {
    ResolutionCondition {
        left,
        comparison: Comparison::GreaterOrEqual,
        right: crate::program::ValueExpr::Literal(20.into()),
    }
}
fn branch(destination: usize) -> Instruction {
    Instruction::BranchIfFalse {
        condition: condition(crate::program::ValueExpr::PlayerStat(
            crate::program::PlayerStatExpr::ControllerLife,
        )),
        destination,
    }
}
fn choose() -> Instruction {
    Instruction::ChooseCreatureType {
        into: CreatureTypeSlot::new(0),
        excluding: BTreeSet::new(),
    }
}
fn read() -> Instruction {
    crate::program::Instruction::DrawCards(Box::new(crate::toolbox::DrawCardsSpec::simple(
        crate::toolbox::PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
        crate::program::ValueExpr::count_battlefield(
            crate::program::ObjectFilter::any()
                .controlled()
                .of_chosen_slot(CreatureTypeSlot::new(0)),
        ),
    )))
}
fn gain() -> Instruction {
    crate::program::Instruction::AdjustLife(Box::new(crate::toolbox::AdjustLifeSpec::gain(
        crate::toolbox::PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
        crate::program::ValueExpr::Literal(1.into()),
    )))
}
fn check(target: BodyTarget, instructions: Vec<Instruction>) -> Result<(), DefinitionError> {
    ExecutableBody::new(target, instructions)
        .validate(&creature_types::selected_vocabulary().unwrap())
}
#[test]
fn forward_destinations_reject_cycles_out_of_range_and_accept_body_end() {
    for destination in [0, 3, usize::MAX] {
        for op in [branch(destination), Instruction::Jump { destination }] {
            assert_eq!(
                check(BodyTarget::None, vec![op, gain()]),
                Err(DefinitionError::InvalidControlFlow)
            );
        }
    }
    assert_eq!(
        check(
            BodyTarget::None,
            vec![gain(), Instruction::Jump { destination: 1 }]
        ),
        Err(DefinitionError::InvalidControlFlow)
    );
    assert_eq!(check(BodyTarget::None, vec![branch(2), gain()]), Ok(()));
    assert_eq!(
        check(BodyTarget::None, vec![Instruction::Jump { destination: 1 }]),
        Ok(())
    );
}
#[test]
fn join_requires_definite_reads_and_rejects_possible_duplicate_producers() {
    assert_eq!(
        check(BodyTarget::None, vec![branch(2), choose(), read()]),
        Err(DefinitionError::UndefinedCreatureTypeSlot)
    );
    assert_eq!(
        check(BodyTarget::None, vec![branch(2), choose(), choose()]),
        Err(DefinitionError::DuplicateCreatureTypeSlot)
    );
    assert_eq!(
        check(
            BodyTarget::None,
            vec![
                branch(3),
                choose(),
                Instruction::Jump { destination: 4 },
                choose(),
                read()
            ]
        ),
        Ok(())
    );
    assert_eq!(
        check(BodyTarget::None, vec![choose(), branch(3), gain(), read()]),
        Ok(())
    );
    assert_eq!(
        check(BodyTarget::None, vec![choose(), choose()]),
        Err(DefinitionError::DuplicateCreatureTypeSlot)
    );
}
#[test]
fn moved_donor_at_a_join_is_rejected_but_disjoint_paths_remain_admitted() {
    let copy = Instruction::CopyTargetToSource {
        duration: ResolvedDuration::UntilEndOfTurn,
        power_toughness: None,
    };
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![
                branch(2),
                crate::program::Instruction::ZoneOp(Box::new(
                    crate::toolbox::ZoneOpSpec::move_all(
                        crate::toolbox::ObjectQuery::Targets,
                        crate::toolbox::ZoneDest::Hand
                    )
                )),
                copy.clone()
            ]
        ),
        Err(DefinitionError::InvalidInstruction)
    );
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![
                branch(3),
                crate::program::Instruction::ZoneOp(Box::new(
                    crate::toolbox::ZoneOpSpec::move_all(
                        crate::toolbox::ObjectQuery::Targets,
                        crate::toolbox::ZoneDest::Hand
                    )
                )),
                Instruction::Jump { destination: 4 },
                copy,
                gain()
            ]
        ),
        Ok(())
    );
}
#[test]
fn trigger_count_conditions_require_actual_bound_admission_even_when_unreachable() {
    let instructions = vec![
        Instruction::BranchIfFalse {
            condition: condition(crate::program::ValueExpr::event(
                crate::program::EventValueKind::MatchedOccurrences,
            )),
            destination: 2,
        },
        gain(),
    ];
    let body = ExecutableBody::new(BodyTarget::None, instructions);
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    assert_eq!(
        body.validate(&vocabulary),
        Err(DefinitionError::UnboundTriggerCondition)
    );
    assert_eq!(
        body.validate_in(
            &vocabulary,
            BodyContext::TriggerBound {
                condition: &crate::program::TriggerSpec::self_cast(),
                grouping: TriggerGrouping::EachOccurrence,
            }
        ),
        Ok(())
    );
    let body = ExecutableBody::new(
        BodyTarget::None,
        vec![
            Instruction::Jump { destination: 2 },
            body.instructions[0].clone(),
        ],
    );
    assert_eq!(
        body.validate(&vocabulary),
        Err(DefinitionError::UnboundTriggerCondition)
    );
}
#[test]
fn all_signed_comparisons_are_exact_including_huge_values() {
    let pairs = [
        ((-5).into(), (-5).into()),
        ((-6).into(), (-5).into()),
        (30.into(), (-100).into()),
        (
            RulesInteger::try_from("999999999999999999999999999999999999999999999999".to_owned())
                .unwrap(),
            20.into(),
        ),
    ];
    for (left, right) in pairs {
        for (comparison, expected) in [
            (Comparison::Equal, left == right),
            (Comparison::NotEqual, left != right),
            (Comparison::Less, left < right),
            (Comparison::LessOrEqual, left <= right),
            (Comparison::Greater, left > right),
            (Comparison::GreaterOrEqual, left >= right),
        ] {
            assert_eq!(comparison.evaluate(&left, &right), expected);
        }
    }
}
#[test]
fn strict_control_codec_roundtrips_and_rejects_missing_unknown_noncanonical_fields() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let program = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![branch(2), gain(), Instruction::Jump { destination: 3 }],
        ),
    );
    let wire = codec::view(&program);
    assert_eq!(codec::decode(wire.clone(), &vocabulary).unwrap(), program);
    let pointer = "/kind/body/instructions/0";
    // ProgramKind's actual wire shape is asserted rather than silently selecting a missing field.
    assert_eq!(wire.pointer(pointer).unwrap()["op"], "BranchIfFalse");
    for value in [
        json!(null),
        json!(2),
        json!("02"),
        json!("-1"),
        json!("+2"),
        json!(" 2"),
        json!("99999999999999999999999999999999999999999"),
    ] {
        let mut invalid = wire.clone();
        invalid.pointer_mut(pointer).unwrap()["destination"] = value;
        assert!(codec::decode(invalid, &vocabulary).is_err());
    }
    for field in ["condition", "destination"] {
        let mut invalid = wire.clone();
        invalid
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(codec::decode(invalid, &vocabulary).is_err());
    }
    for (field, value) in [
        ("comparison", json!("Approximately")),
        ("left", json!({"tag":"SourceLife"})),
        ("right", json!(null)),
        ("extra", json!(true)),
    ] {
        let mut invalid = wire.clone();
        invalid.pointer_mut(pointer).unwrap()["condition"][field] = value;
        assert!(codec::decode(invalid, &vocabulary).is_err());
    }
    let mut invalid = wire;
    invalid.pointer_mut(pointer).unwrap()["condition"] =
        json!([{"tag":"ControllerLife"},"Equal",{"tag":"Literal","value":"0"}]);
    assert!(codec::decode(invalid, &vocabulary).is_err());
}

#[test]
fn nested_continuous_effect_slot_reads_use_the_same_definite_join() {
    use crate::characteristics::{CreatureTypeTextChangeSpec, CreatureTypeWord, ResolvedPartSpec};
    let human = creature_types::selected_vocabulary()
        .unwrap()
        .resolve_label("Human")
        .unwrap();
    let effect = || {
        struct _Tmp {
            parts: crate::characteristics::ResolvedPartsSpec,
            duration: crate::characteristics::ResolvedDuration,
        }
        let _t = _Tmp {
            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Text(CreatureTypeTextChangeSpec {
                from: CreatureTypeWord::Chosen(CreatureTypeSlot::new(0)),
                to: CreatureTypeWord::Literal(human),
            })]),
            duration: ResolvedDuration::UntilEndOfTurn,
        };
        crate::program::Instruction::ApplyContinuous(Box::new(
            crate::toolbox::ApplyContinuousSpec {
                selector: Some(crate::toolbox::ObjectQuery::Targets),
                payload: crate::toolbox::ContinuousPayload::ResolvedParts(Box::new(_t.parts)),
                duration: _t.duration,
                condition: None,
                one_shot: false,
                as_emblem: None,
                emblem_label: None,
            },
        ))
    };
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![branch(2), choose(), effect()]
        ),
        Err(DefinitionError::UndefinedCreatureTypeSlot)
    );
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![
                branch(3),
                choose(),
                Instruction::Jump { destination: 4 },
                choose(),
                effect()
            ]
        ),
        Ok(())
    );
}
