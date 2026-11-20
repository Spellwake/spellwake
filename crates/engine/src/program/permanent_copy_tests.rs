use super::*;
use crate::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    test_support::{creature_types, program as codec},
};
fn source(instructions: Vec<Instruction>) -> Program {
    Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            source_cost: SourceCost::None,
            mana_cost: ManaCost::free(),
            body: ExecutableBody::new(BodyTarget::SinglePermanent, instructions),
        }],
    )
}
fn copy() -> Instruction {
    Instruction::CopyTargetToSource {
        duration: ResolvedDuration::UntilEndOfTurn,
        power_toughness: None,
    }
}
#[test]
fn copy_origin_and_lki_composition_are_admitted_explicitly() {
    let v = creature_types::selected_vocabulary().unwrap();
    assert!(source(vec![copy()]).validate(&v).is_ok());
    assert!(
        source(vec![copy(), Instruction::ReturnToOwnersHand])
            .validate(&v)
            .is_ok()
    );
    assert_eq!(
        source(vec![Instruction::ReturnToOwnersHand, copy()]).validate(&v),
        Err(DefinitionError::InvalidInstruction)
    );
    let body = ExecutableBody::new(BodyTarget::SinglePermanent, vec![copy()]);
    for spell in [
        Program::instant(ManaCost::free(), body.clone()),
        Program::sorcery(ManaCost::free(), body),
    ] {
        assert_eq!(spell.validate(&v), Err(DefinitionError::InvalidInstruction));
    }
    let mut invalid = source(vec![copy()]);
    invalid.activated[0].body.target = BodyTarget::SinglePlayer;
    assert_eq!(invalid.validate(&v), Err(DefinitionError::OperandFamily));
}
#[test]
fn strict_copy_codec_requires_explicit_nullable_exception_and_duration() {
    let v = creature_types::selected_vocabulary().unwrap();
    for exception in [
        None,
        Some(PowerToughness {
            power: (-7).into(),
            toughness: 12.into(),
        }),
    ] {
        let p = source(vec![Instruction::CopyTargetToSource {
            duration: ResolvedDuration::UntilEndOfTurn,
            power_toughness: exception,
        }]);
        let wire = codec::view(&p);
        assert_eq!(codec::decode(wire.clone(), &v).unwrap(), p);
        for field in ["duration", "power_toughness"] {
            let mut missing = wire.clone();
            missing["activated"][0]["body"]["instructions"][0]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(codec::decode(missing, &v).is_err());
        }
        let mut unknown = wire;
        unknown["activated"][0]["body"]["instructions"][0]["keep_abilities"] = true.into();
        assert!(codec::decode(unknown, &v).is_err());
    }
}
#[test]
fn copied_snapshot_cannot_be_authored_as_a_resolved_part_spec_or_mixed_record() {
    use crate::characteristics::{ResolvedPart, ResolvedParts};
    let v = creature_types::selected_vocabulary().unwrap();
    let p = source(vec![copy()]);
    assert_eq!(
        ResolvedParts(vec![
            ResolvedPart::Copy(Box::new(p.clone())),
            ResolvedPart::SwitchPT
        ])
        .validate(),
        Err(CharacteristicDefinitionError::MixedCopyParts)
    );
    let mut wire = codec::view(&p);
    wire["activated"][0]["body"]["instructions"][0] = serde_json::json!({"op":"CreateContinuousEffect","duration":"NoStatedDuration","parts":[{"tag":"Copy","program":codec::view(&p)}]});
    assert!(codec::decode(wire, &v).is_err());
}
