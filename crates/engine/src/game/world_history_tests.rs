//! Actual root failures after admission preserve the accepted whole-world prefix.
use super::*;
use crate::{
    characteristics::{
        CharacteristicsError, ResolvedDuration, ResolvedPartSpec, SubtypeOperation, TypeChangeSpec,
        TypeOperation,
    },
    effects::private_qualification::QualificationFault,
    object_identity::CardId,
    occurrence::TriggerError,
    program::Supertype,
    test_support::characteristic_trajectories as tr,
};
use std::collections::BTreeSet;

#[test]
fn world_allocator_and_late_publisher_faults_drop_the_tentative_gain_and_keep_the_paid_prefix() {
    let game = tr::world(
        tr::vocabulary(),
        vec![
            tr::card(1, crate::program::Program::artifact(None, vec![]), true),
            tr::spell(
                2,
                BodyTarget::SINGLE_PERMANENT,
                vec![tr::effect(
                    vec![ResolvedPartSpec::Type(TypeChangeSpec {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Add(BTreeSet::from([Supertype::World])),
                        types: TypeOperation::Keep,
                        subtypes: SubtypeOperation::Keep,
                    })],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let cast = tr::submit(
        &game,
        tr::P0,
        GameCommand::CastPaid {
            spell: tr::object(&game, 2),
            target: TargetRef::Object(tr::object(&game, 1)),
            payment: crate::mana::ManaPayment::default(),
        },
    );
    let passed = tr::submit(cast.state(), tr::P0, GameCommand::Pass);
    for (fault, expected) in [
        (
            QualificationFault::WorldTenure(u64::MAX),
            EffectError::Characteristics(CharacteristicsError::WorldTenureExhausted),
        ),
        (
            QualificationFault::Event(u64::MAX),
            EffectError::Trigger(TriggerError::EventIdentityExhausted),
        ),
    ] {
        let mut source = passed.state().clone();
        source.execution.qualification_set_fault(fault);
        let before = source.clone();
        let resources = source.execution.operational_counters();
        assert_eq!(
            source.advance(GameInput {
                basis: source.basis().clone(),
                actor: tr::P1,
                command: GameCommand::Pass
            }),
            Err(GameError::Effect(expected))
        );
        assert_eq!(source, before);
        assert_eq!(source.execution.operational_counters(), resources);
        assert_eq!(source.execution.world_tenures().count(), 0);
        assert!(source.execution.continuous_effects().next().is_none());
        assert_eq!(source.execution.window().entries().len(), 1);
    }
}
