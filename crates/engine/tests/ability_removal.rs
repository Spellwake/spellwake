//! Layer-6 behavior through real GameState commands, not asserted ability facts.
//! Rules basis: CR 113.7a, 603.6c/603.10, 613.1f, 613.6 and 611.2a.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{AbilityChange, AbilityRemoval, ResolvedDuration, ResolvedPartSpec},
    effects::EffectError,
    game::{GameCommand, GameError, GameInput},
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    occurrence::RulesRecord,
    program::{
        BodyTarget, ExecutableBody, Instruction, Program, TriggerSpec, TriggeredAbility,
        TriggeredAbilityId,
    },
    targets::TargetRef,
};
use std::collections::{BTreeMap, BTreeSet};

fn source(condition: Option<TriggerSpec>) -> Program {
    let mut program =
        Program::creature(Some(ManaCost::free()), 3.into(), 3.into(), BTreeSet::new());
    program.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
    });
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::Damage { amount: 1.into() }],
        ),
    });
    if let Some(condition) = condition {
        program.triggered.push(TriggeredAbility {
            grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
            id: TriggeredAbilityId(1),
            condition,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                }],
            ),
        });
    }
    program
}
fn removal(removal: AbilityRemoval, duration: ResolvedDuration) -> Instruction {
    effect(
        vec![ResolvedPartSpec::Abilities(AbilityChange::remove(removal))],
        duration,
    )
}

#[test]
fn removed_mana_and_activated_abilities_reject_before_paying_any_cost() {
    let game = world(
        vocabulary(),
        vec![
            card(1, source(None), true),
            card(
                2,
                Program::creature(None, 4.into(), 4.into(), BTreeSet::new()),
                true,
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![removal(
                    AbilityRemoval::All,
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let source = object(&game, 1);
    let target = object(&game, 2);
    let (removed, _) = resolve(&game, 3, source);
    let game = removed.state();
    let saved = game.clone();
    assert_eq!(
        observe_advance(
            game,
            GameInput {
                basis: game.basis().clone(),
                actor: P0,
                command: GameCommand::ActivateMana {
                    source,
                    ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1))
                }
            }
        )
        .unwrap_err(),
        GameError::Effect(EffectError::NotManaSource)
    );
    assert_eq!(
        observe_advance(
            game,
            GameInput {
                basis: game.basis().clone(),
                actor: P0,
                command: GameCommand::ActivateAbility {
                    source,
                    ability: ActivatedAbilityId(1),
                    target: TargetRef::Object(target),
                    payment: Some(ManaPayment::default())
                }
            }
        )
        .unwrap_err(),
        GameError::Effect(EffectError::NotAbilitySource)
    );
    assert_eq!(game, &saved);
    assert_eq!(game.execution().mana(P0, ManaType::Green), 0.into());
    assert_eq!(current(game, source).abilities.identities(), vec![]);
    // Cleanup expires the effect through the existing combined cleanup cut.
    let restored = cleanup(game);
    assert_eq!(
        current(restored.state(), source).abilities.mana,
        vec![ManaAbilityId(1)]
    );
    assert_eq!(
        current(restored.state(), source).abilities.activated,
        vec![ActivatedAbilityId(1)]
    );
}

#[test]
fn nonmana_removal_keeps_mana_but_prevents_tap_trigger_discovery() {
    let game = world(
        vocabulary(),
        vec![
            card(
                1,
                source(Some(
                    spellwake_engine::program::TriggerSpec::becomes_tapped(
                        spellwake_engine::program::ObjectFilter::self_object(),
                    ),
                )),
                true,
            ),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![removal(
                    AbilityRemoval::NonMana,
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let source = object(&game, 1);
    let (removed, _) = resolve(&game, 2, source);
    let current = current(removed.state(), source);
    assert_eq!(current.abilities.mana, vec![ManaAbilityId(1)]);
    assert!(current.abilities.activated.is_empty());
    assert!(current.abilities.triggered.is_empty());
    let tapped = submit(
        removed.state(),
        P0,
        GameCommand::ActivateMana {
            source,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(
        tapped.state().execution().mana(P0, ManaType::Green),
        1.into()
    );
    assert!(tapped.state().execution().window().entries().is_empty());
    assert!(
        !tapped
            .rules_journal()
            .records
            .iter()
            .any(|record| matches!(record, RulesRecord::TriggerDiscovered(_)))
    );
}

#[test]
fn removing_abilities_does_not_erase_an_activation_already_on_the_stack() {
    let game = world(
        vocabulary(),
        vec![
            card(1, source(None), true),
            card(
                2,
                Program::creature(None, 4.into(), 4.into(), BTreeSet::new()),
                true,
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![removal(
                    AbilityRemoval::All,
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let source = object(&game, 1);
    let target = object(&game, 2);
    let activation = activate(&game, source, target, false);
    let entry = activation.state().execution().window().entries()[0].id();
    let (removed, _) = resolve(activation.state(), 3, source);
    assert!(
        current(removed.state(), source)
            .abilities
            .activated
            .is_empty()
    );
    assert_eq!(
        removed.state().execution().window().entries()[0].id(),
        entry
    );
    let completed = pass_pair(removed.state());
    assert_eq!(
        completed
            .state()
            .execution()
            .battlefield_marked_damage(target)
            .unwrap(),
        1.into()
    );
    assert!(completed.state().execution().window().entries().is_empty());
}

#[test]
fn removing_a_dies_trigger_before_lethal_damage_prevents_lookback_discovery() {
    let game = world(
        vocabulary(),
        vec![
            card(
                1,
                source(Some(spellwake_engine::program::TriggerSpec::dies(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ))),
                true,
            ),
            spell(
                2,
                BodyTarget::SingleCreature,
                vec![
                    removal(AbilityRemoval::All, ResolvedDuration::NoStatedDuration),
                    Instruction::Damage { amount: 10.into() },
                ],
            ),
        ],
        vec![CardId(1)],
    );
    let source = object(&game, 1);
    let (completed, _) = resolve(&game, 2, source);
    let physical = completed.state().execution().window().cards();
    assert!(physical.get(source).is_none());
    assert_eq!(
        physical
            .get(physical.current_identity(CardId(1)).unwrap())
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    assert!(completed.state().execution().window().entries().is_empty());
    assert!(
        !completed
            .rules_journal()
            .records
            .iter()
            .any(|record| matches!(record, RulesRecord::TriggerDiscovered(_)))
    );
}

#[test]
fn captured_tap_trigger_resolves_after_the_source_loses_all_abilities() {
    let mut library = card(9, Program::artifact(None, vec![]), false);
    library.destination = ObjectDestination::LibraryBottom;
    let game = world(
        vocabulary(),
        vec![
            card(
                1,
                source(Some(
                    spellwake_engine::program::TriggerSpec::becomes_tapped(
                        spellwake_engine::program::ObjectFilter::self_object(),
                    ),
                )),
                true,
            ),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![removal(
                    AbilityRemoval::All,
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            library,
        ],
        vec![CardId(1)],
    );
    let source = object(&game, 1);
    let tapped = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let trigger = tapped.state().execution().window().entries()[0].id();
    let (removed, _) = resolve(tapped.state(), 2, source);
    assert!(
        current(removed.state(), source)
            .abilities
            .triggered
            .is_empty()
    );
    assert_eq!(
        removed.state().execution().window().entries()[0].id(),
        trigger
    );
    let completed = pass_pair(removed.state());
    let drawn = object(completed.state(), 9);
    assert_eq!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .get(drawn)
            .unwrap()
            .zone(),
        ObjectZone::Hand(P0)
    );
    assert!(completed.state().execution().window().entries().is_empty());
}
