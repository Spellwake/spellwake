//! Admission and strict syntax checks; public cast trajectories qualify separately.
use super::*;
use crate::test_support::{creature_types, program as codec};
use serde_json::json;

fn body() -> ExecutableBody {
    ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(1.into()),
        }],
    )
}
fn ability(condition: OrdinaryCondition) -> TriggeredAbility {
    TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: body(),
    }
}
fn watched(mut program: Program, condition: OrdinaryCondition) -> Program {
    program.triggered.push(ability(condition));
    program
}

#[test]
fn self_cast_is_admitted_on_physical_permanent_and_instruction_definitions() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    for program in [
        Program::instant(ManaCost::free(), body()),
        Program::sorcery(ManaCost::free(), body()),
        Program::artifact(Some(ManaCost::free()), vec![]),
        Program::creature(Some(ManaCost::free()), 1.into(), 1.into(), BTreeSet::new()),
    ] {
        let program = watched(program, OrdinaryCondition::SelfCast);
        assert_eq!(program.validate(&vocabulary), Ok(()));
        assert_eq!(
            codec::decode(codec::view(&program), &vocabulary).unwrap(),
            program
        );
    }
}

#[test]
fn battlefield_cast_watchers_do_not_bypass_instruction_source_admission() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    for player in [
        PlayerRelation::Controller,
        PlayerRelation::Opponent,
        PlayerRelation::AnyParticipant,
    ] {
        for filter in [
            SpellCastFilter::AnySpell,
            SpellCastFilter::Noncreature,
            SpellCastFilter::InstantOrSorcery,
            SpellCastFilter::HasType(CardType::Kindred),
        ] {
            let condition = OrdinaryCondition::SpellCast { player, filter };
            let permanent = watched(Program::artifact(None, vec![]), condition);
            assert_eq!(permanent.validate(&vocabulary), Ok(()));
            assert_eq!(
                codec::decode(codec::view(&permanent), &vocabulary).unwrap(),
                permanent
            );
            let spell = watched(Program::instant(ManaCost::free(), body()), condition);
            let wire = codec::view(&spell);
            assert_eq!(
                codec::decode_for_owner(wire.clone(), &vocabulary).unwrap(),
                spell
            );
            assert_eq!(
                spell.validate(&vocabulary),
                Err(DefinitionError::TriggeredSource)
            );
            assert!(codec::decode(wire, &vocabulary).is_err());
        }
    }
}

#[test]
fn self_cast_still_uses_shared_unique_id_target_and_amount_admission() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let mut spell = watched(
        Program::instant(ManaCost::free(), body()),
        OrdinaryCondition::SelfCast,
    );
    spell.triggered.push(spell.triggered[0].clone());
    assert_eq!(
        spell.validate(&vocabulary),
        Err(DefinitionError::DuplicateAbility)
    );
    spell.triggered.pop();
    spell.triggered[0].body = ExecutableBody::new(
        BodyTarget::SinglePlayer,
        vec![Instruction::AdjustTargetLife {
            adjustment: LifeAdjustment::Gain(1.into()),
        }],
    );
    assert_eq!(spell.validate(&vocabulary), Ok(()));
    let counted = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::DrawController {
            amount: InstructionAmount::MatchedOccurrences,
        }],
    );
    spell.triggered[0].body = counted.clone();
    assert_eq!(spell.validate(&vocabulary), Ok(()));
    let ProgramKind::Spell(definition) = &mut spell.kind else {
        unreachable!()
    };
    definition.body = counted;
    assert_eq!(
        spell.validate(&vocabulary),
        Err(DefinitionError::UnboundTriggerAmount)
    );
}

#[test]
fn cast_condition_and_filter_fields_are_required_tagged_and_strict() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let program = watched(Program::artifact(None, vec![]), OrdinaryCondition::SelfCast);
    let original = codec::view(&program);
    for condition in [
        json!({"tag":"SelfCast","zone":"Hand"}),
        json!({"tag":"SpellCast","player":"Controller"}),
        json!({"tag":"SpellCast","player":"Team","filter":{"tag":"AnySpell"}}),
        json!({"tag":"SpellCast","player":"Controller","filter":"AnySpell"}),
        json!({"tag":"SpellCast","player":"Controller","filter":{"tag":"HasType"}}),
        json!({"tag":"SpellCast","player":"Controller","filter":{"tag":"HasType","card_type":"Unknown"}}),
        json!({"tag":"SpellCast","player":"Controller","filter":{"tag":"AnySpell","default":true}}),
        json!({"tag":"SpellCast","player":"Controller","filter":{"tag":"CopyableInstructionSpell"}}),
    ] {
        let mut wire = original.clone();
        wire["triggered"][0]["condition"] = condition;
        assert!(codec::decode_for_owner(wire, &vocabulary).is_err());
    }
}

#[test]
fn cast_type_filters_use_membership_and_instant_or_sorcery_is_disjunctive() {
    for kind in [CardType::Instant, CardType::Sorcery] {
        let types = BTreeSet::from([kind, CardType::Kindred]);
        assert!(SpellCastFilter::InstantOrSorcery.matches(&types));
        assert!(SpellCastFilter::HasType(CardType::Kindred).matches(&types));
        assert!(SpellCastFilter::Noncreature.matches(&types));
        assert!(SpellCastFilter::AnySpell.matches(&types));
    }
    let creature = BTreeSet::from([CardType::Artifact, CardType::Creature]);
    assert!(!SpellCastFilter::InstantOrSorcery.matches(&creature));
    assert!(!SpellCastFilter::Noncreature.matches(&creature));
    assert!(!SpellCastFilter::HasType(CardType::Kindred).matches(&creature));
    assert!(SpellCastFilter::HasType(CardType::Artifact).matches(&creature));
}
