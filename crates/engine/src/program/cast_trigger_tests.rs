//! Admission and strict syntax checks; public cast trajectories qualify separately.
use super::*;
use crate::test_support::{creature_types, program as codec};
use serde_json::json;

fn body() -> ExecutableBody {
    ExecutableBody::new(
        BodyTarget::None,
        vec![crate::program::Instruction::AdjustLife(Box::new(
            crate::toolbox::AdjustLifeSpec::gain(
                crate::toolbox::PlayerSelector::Resolving(
                    crate::program::ResolvingPlayer::Controller,
                ),
                crate::program::ValueExpr::Literal(1.into()),
            ),
        ))],
    )
}
fn ability(condition: TriggerSpec) -> TriggeredAbility {
    TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: body(),
    }
}
fn watched(mut program: Program, condition: TriggerSpec) -> Program {
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
        let program = watched(program, crate::program::TriggerSpec::self_cast());
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
            crate::program::SpellFilter::any(),
            crate::program::SpellFilter::noncreature(),
            crate::program::SpellFilter::instant_or_sorcery(),
            crate::program::SpellFilter::of_type(CardType::Kindred),
        ] {
            let condition = crate::program::TriggerSpec::spell_cast(player, filter);
            let permanent = watched(Program::artifact(None, vec![]), condition.clone());
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
fn cast_condition_and_filter_fields_are_required_tagged_and_strict() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let program = watched(
        Program::artifact(None, vec![]),
        crate::program::TriggerSpec::self_cast(),
    );
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
        assert!(crate::program::SpellFilter::instant_or_sorcery().matches_card_types(&types));
        assert!(crate::program::SpellFilter::of_type(CardType::Kindred).matches_card_types(&types));
        assert!(crate::program::SpellFilter::noncreature().matches_card_types(&types));
        assert!(crate::program::SpellFilter::any().matches_card_types(&types));
    }
    let creature = BTreeSet::from([CardType::Artifact, CardType::Creature]);
    assert!(!crate::program::SpellFilter::instant_or_sorcery().matches_card_types(&creature));
    assert!(!crate::program::SpellFilter::noncreature().matches_card_types(&creature));
    assert!(!crate::program::SpellFilter::of_type(CardType::Kindred).matches_card_types(&creature));
    assert!(crate::program::SpellFilter::of_type(CardType::Artifact).matches_card_types(&creature));
}
