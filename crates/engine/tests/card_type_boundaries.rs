#![allow(dead_code, unused_imports)]
//! Composed boundaries for the shared Creature/Kindred family.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        DefiningPart, DefiningParts, NumericExpression, PermanentSelector, ResolvedDuration,
        StaticAbilityId, StaticContinuousAbility, SubtypeOperation, TypeOperation,
    },
    effects::{CopyTargetChoice, EffectError, EffectWindow},
    game::{DecisionReply, GameCommand, GameError, GameInput, RuleFrame},
    life::LifeAdjustment,
    mana::{ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    occurrence::RulesOccurrence,
    program::{
        BodyTarget, CardType, ExecutableBody, Instruction, Program, TriggeredAbility,
        TriggeredAbilityId,
    },
    targets::TargetRef,
};
use std::collections::BTreeSet;
use trajectories::*;

#[test]
fn defining_numeric_count_includes_kindred_and_reacts_to_last_carrier_loss() {
    let vocabulary = vocabulary();
    let mut counter = permanent(&vocabulary, &[CardType::Creature], Some((0, 3)), &["Elf"]);
    counter.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(1),
        parts: DefiningParts(vec![DefiningPart::PowerToughness {
            power: Some(Box::new(NumericExpression::count(PermanentSelector {
                creature_subtype: Some(vocabulary.resolve_label("Elf").unwrap()),
                ..PermanentSelector::default()
            }))),
            toughness: None,
        }]),
    });
    let game = world(
        vocabulary.clone(),
        vec![
            card(1, counter, true),
            card(
                2,
                permanent(
                    &vocabulary,
                    &[CardType::Kindred, CardType::Enchantment],
                    None,
                    &["Elf"],
                ),
                true,
            ),
            spell(
                3,
                BodyTarget::SINGLE_PERMANENT,
                vec![effect(
                    vec![type_part(
                        TypeOperation::Remove(BTreeSet::from([CardType::Kindred])),
                        SubtypeOperation::Keep,
                    )],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let source = object(&game, 1);
    assert_eq!(
        current(&game, source).power_toughness.unwrap().derived,
        pt(2, 3)
    );
    let (changed, _) = resolve(&game, 3, object(&game, 2));
    assert_eq!(
        current(changed.state(), source)
            .power_toughness
            .unwrap()
            .derived,
        pt(1, 3)
    );
    assert!(
        current(changed.state(), object(changed.state(), 2))
            .current_creature_subtypes
            .is_empty()
    );
}

#[test]
fn kindred_combat_eligibility_and_actual_membership_require_creature_type() {
    use spellwake_engine::combat::{AttackDeclaration, BlockDeclaration, CombatDecision};
    let vocabulary = vocabulary();
    let mut defender_kindred = card(
        3,
        permanent(
            &vocabulary,
            &[CardType::Kindred, CardType::Enchantment],
            None,
            &["Elf"],
        ),
        true,
    );
    defender_kindred.owner = P1;
    defender_kindred.destination = ObjectDestination::Battlefield { controller: P1 };
    let mut defender_creature = card(
        4,
        permanent(&vocabulary, &[CardType::Creature], Some((2, 3)), &[]),
        true,
    );
    defender_creature.owner = P1;
    defender_creature.destination = ObjectDestination::Battlefield { controller: P1 };
    let game = world(
        vocabulary.clone(),
        vec![
            card(
                1,
                permanent(
                    &vocabulary,
                    &[CardType::Creature, CardType::Kindred, CardType::Enchantment],
                    Some((2, 3)),
                    &["Elf"],
                ),
                true,
            ),
            card(
                2,
                permanent(
                    &vocabulary,
                    &[CardType::Kindred, CardType::Enchantment],
                    None,
                    &["Elf"],
                ),
                true,
            ),
            defender_kindred,
            defender_creature,
            spell(
                5,
                BodyTarget::SINGLE_PERMANENT,
                vec![effect(
                    vec![type_part(
                        TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
                        SubtypeOperation::Keep,
                    )],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3), CardId(4)],
    );
    let beginning = pass_pair(&game);
    let attackers = pass_pair(beginning.state());
    let decision = attackers.state().pending_decision().unwrap();
    let RuleFrame::Combat(CombatDecision::Attackers(frame)) = decision.frame() else {
        panic!("attackers decision");
    };
    let attacking = object(attackers.state(), 1);
    let noncreature = object(attackers.state(), 2);
    assert!(frame.eligible_attackers().contains(&attacking));
    assert!(!frame.eligible_attackers().contains(&noncreature));
    let before = attackers.state().clone();
    assert!(matches!(
        attackers.state().advance(GameInput {
            basis: attackers.state().basis().clone(),
            actor: P0,
            command: GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                    attacker: noncreature,
                    defender: P1,
                    defending_planeswalker: None,
                }])
            }
        }),
        Err(GameError::Effect(EffectError::Combat(_)))
    ));
    assert_eq!(attackers.state(), &before);
    let declared = reply(
        attackers.state(),
        P0,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: attacking,
            defender: P1,
            defending_planeswalker: None,
        }]),
    );
    // One branch reaches a real blocker chooser: Kindred does not confer eligibility.
    let blockers = pass_pair(declared.state());
    let decision = blockers.state().pending_decision().unwrap();
    let RuleFrame::Combat(CombatDecision::Blockers(frame)) = decision.frame() else {
        panic!("blockers decision");
    };
    let nonblocker = object(blockers.state(), 3);
    let blocker = object(blockers.state(), 4);
    assert!(!frame.eligible_blockers().contains(&nonblocker));
    assert!(frame.eligible_blockers().contains(&blocker));
    let before = blockers.state().clone();
    assert!(matches!(
        blockers.state().advance(GameInput {
            basis: blockers.state().basis().clone(),
            actor: P1,
            command: GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                    blocker: nonblocker,
                    attacker: attacking
                }])
            }
        }),
        Err(GameError::Effect(EffectError::Combat(_)))
    ));
    assert_eq!(blockers.state(), &before);
    let blocked = reply(
        blockers.state(),
        P1,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker,
            attacker: attacking,
        }]),
    );
    assert_eq!(
        blocked
            .state()
            .execution()
            .combat()
            .unwrap()
            .blockers()
            .count(),
        1
    );
    // Independent branch: the actual type-change cut removes an attacking
    // object while retaining its shared family membership.
    let (changed, _) = resolve(declared.state(), 5, attacking);
    assert!(!current(changed.state(), attacking).is_creature());
    assert_eq!(
        current(changed.state(), attacking).current_creature_subtypes,
        subtypes(&vocabulary, &["Elf"])
    );
    assert!(
        changed
            .state()
            .execution()
            .combat()
            .unwrap()
            .attackers()
            .next()
            .is_none()
    );
    assert!(all_occurrences(&changed).iter().any(
        |event| matches!(event,RulesOccurrence::CombatantsRemoved { participants }
        if participants.iter().any(|value| value.object() == attacking))
    ));
}
