#![forbid(unsafe_code)]
//! Targeted ability bodies may act on their captured source without retargeting it.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    game::GameCommand,
    mana::{ManaCost, ManaPayment},
    program::{BodyTarget, ExecutableBody, Instruction, Program},
    targets::TargetRef,
};
use std::collections::BTreeSet;
use trajectories::*;

fn transforming_source() -> Program {
    let mut source = Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
    source.transform_back = Some(Box::new(Program::creature(
        None,
        5.into(),
        5.into(),
        BTreeSet::new(),
    )));
    source.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![
                Instruction::ReturnToOwnersHand,
                Instruction::TransformSource,
            ],
        ),
    });
    source
}

#[test]
fn targeted_activation_transforms_only_its_captured_source() {
    let vocab = vocabulary();
    let source = transforming_source();
    source.validate(&vocab).unwrap();
    let target = Program::creature(Some(ManaCost::free()), 3.into(), 3.into(), BTreeSet::new());
    let game = world(
        vocab,
        vec![card(1, source, true), card(2, target, true)],
        vec![
            spellwake_engine::object_identity::CardId(1),
            spellwake_engine::object_identity::CardId(2),
        ],
    );
    let source_id = object(&game, 1);
    let target_id = object(&game, 2);
    let activated = submit(
        &game,
        P0,
        GameCommand::ActivateAbility {
            source: source_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(target_id),
            payment: Some(ManaPayment::default()),
        },
    );
    let resolved = pass_pair(activated.state());
    assert_eq!(
        current(resolved.state(), source_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(5, 5)
    );

    assert_eq!(
        resolved
            .state()
            .execution()
            .window()
            .cards()
            .get(object(resolved.state(), 2))
            .unwrap()
            .zone(),
        spellwake_engine::object_identity::ObjectZone::Hand(P0)
    );
}

#[test]
fn source_only_actions_still_reject_missing_sources_and_phantom_targets() {
    let vocab = vocabulary();
    for instruction in [
        Instruction::TransformSource,
        Instruction::BecomePreparedSource,
    ] {
        let spell = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(BodyTarget::None, vec![instruction.clone()]),
        );
        assert!(spell.validate(&vocab).is_err());
        let mut source = Program::artifact(None, vec![]);
        source.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction]),
        });
        assert!(
            source.validate(&vocab).is_err(),
            "a source operation cannot justify an unused target"
        );
    }
}

#[test]
fn illegal_only_target_prevents_source_transformation() {
    let vocab = vocabulary();
    let source = transforming_source();
    let target = Program::creature(Some(ManaCost::free()), 3.into(), 3.into(), BTreeSet::new());
    let bounce = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::ReturnToOwnersHand],
        ),
    );
    let game = world(
        vocab,
        vec![
            card(1, source, true),
            card(2, target, true),
            card(3, bounce, false),
        ],
        vec![
            spellwake_engine::object_identity::CardId(1),
            spellwake_engine::object_identity::CardId(2),
        ],
    );
    let source_id = object(&game, 1);
    let target_id = object(&game, 2);
    let activated = submit(
        &game,
        P0,
        GameCommand::ActivateAbility {
            source: source_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(target_id),
            payment: Some(ManaPayment::default()),
        },
    );
    let response = submit(
        activated.state(),
        P0,
        GameCommand::CastPaid {
            spell: object(activated.state(), 3),
            target: TargetRef::Object(target_id),
            payment: ManaPayment::default(),
        },
    );
    let bounced = pass_pair(response.state());
    let resolved = pass_pair(bounced.state());
    assert_eq!(
        current(resolved.state(), source_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(2, 2)
    );
    assert!(
        !resolved
            .state()
            .execution()
            .window()
            .cards()
            .get(source_id)
            .unwrap()
            .transformed()
    );
}
