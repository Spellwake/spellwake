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
fn every_admitted_type_set_obeys_the_separate_seed_destination_gate() {
    use spellwake_engine::{combat::CombatRules, setup::GameSeed};
    use std::sync::Arc;
    let vocabulary = vocabulary();
    let labels = [
        CardType::Land,
        CardType::Artifact,
        CardType::Creature,
        CardType::Enchantment,
        CardType::Kindred,
        CardType::Instant,
        CardType::Sorcery,
    ];
    let mut cases = 0;
    for mask in 0_u8..128 {
        for instruction in [false, true] {
            let mut program = if instruction {
                Program::instant(
                    ManaCost::free(),
                    ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(1.into()),
                        }],
                    ),
                )
            } else {
                Program::artifact(None, vec![])
            };
            program.card_types = labels
                .iter()
                .enumerate()
                .filter(|(bit, _)| mask & (1 << bit) != 0)
                .map(|(_, label)| *label)
                .collect();
            if program.validate(&vocabulary).is_err() {
                continue;
            }
            for destination in [
                ObjectDestination::LibraryTop,
                ObjectDestination::LibraryBottom,
                ObjectDestination::Hand,
                ObjectDestination::Graveyard,
                ObjectDestination::Exile,
                ObjectDestination::Command,
                ObjectDestination::Battlefield { controller: P0 },
                ObjectDestination::Stack { controller: P0 },
            ] {
                let on_battlefield = matches!(destination, ObjectDestination::Battlefield { .. });
                let on_stack = matches!(destination, ObjectDestination::Stack { .. });
                // Independent finite owner contract: the three explicit prohibited
                // permanent labels override Creature even in mixed type sets.
                let forbidden = on_battlefield && instruction || on_stack && mask & 0b0001011 != 0;
                let mut seed_card = card(1, program.clone(), false);
                seed_card.destination = destination;
                let seed = GameSeed::with_uniform_life(
                    vec![P0, P1],
                    P0,
                    vec![seed_card],
                    if on_battlefield {
                        vec![CardId(1)]
                    } else {
                        vec![]
                    },
                    vec![],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    vocabulary.clone(),
                    Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                );
                let result = EffectWindow::new(seed);
                if forbidden {
                    assert_eq!(result, Err(EffectError::InvalidSetup));
                } else if on_stack {
                    assert_eq!(
                        result,
                        Err(EffectError::Window(
                            spellwake_engine::stack_window::StackWindowError::UntrackedInitialStack
                        ))
                    );
                } else {
                    assert!(
                        result.is_ok(),
                        "mask={mask:07b} destination={destination:?}"
                    );
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 34 * 8);
}

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
                BodyTarget::SinglePermanent,
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
fn scheduled_kindred_sorcery_copies_keep_owned_traits_after_original_counter() {
    let vocabulary = vocabulary();
    let expected_types = BTreeSet::from([CardType::Kindred, CardType::Sorcery]);
    let expected_subtypes = subtypes(&vocabulary, &["Elf"]);
    let mut sorcery = Program::sorcery(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePlayer,
            vec![Instruction::AdjustTargetLife {
                adjustment: LifeAdjustment::Gain(1.into()),
            }],
        ),
    );
    sorcery.card_types = expected_types.clone();
    sorcery.creature_subtypes = expected_subtypes.clone();
    let copy_card = |id| {
        spell(
            id,
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction()),
            vec![Instruction::CopySpell],
        )
    };
    let game = world(
        vocabulary,
        vec![
            card(1, sorcery, false),
            copy_card(2),
            copy_card(3),
            spell(
                4,
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::CounterSpell],
            ),
        ],
        vec![],
    );
    // Scheduled own main phase; calendar-free instant-only helpers are not used.
    let cast = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 1),
            target: TargetRef::Player(P0),
            payment: ManaPayment::default(),
        },
    );
    let original = cast
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let cast_copy = submit(
        cast.state(),
        P0,
        GameCommand::Cast {
            spell: object(cast.state(), 2),
            target: TargetRef::Stack(original),
        },
    );
    let pending = pass_pair(cast_copy.state());
    assert!(matches!(
        pending.state().pending_decision().unwrap().frame(),
        RuleFrame::CopyTargets(_)
    ));
    let first = reply(
        pending.state(),
        P0,
        DecisionReply::CopyTargets(CopyTargetChoice::Retain),
    );
    assert!(first.mana_payment().is_none());
    let first_id = first
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let counter = submit(
        first.state(),
        P0,
        GameCommand::Cast {
            spell: object(first.state(), 4),
            target: TargetRef::Stack(original),
        },
    );
    let countered = pass_pair(counter.state());
    assert_eq!(
        countered
            .state()
            .execution()
            .stack_copiable_card_types(original),
        None
    );
    let cast_second = submit(
        countered.state(),
        P0,
        GameCommand::Cast {
            spell: object(countered.state(), 3),
            target: TargetRef::Stack(first_id),
        },
    );
    let pending_second = pass_pair(cast_second.state());
    let second = reply(
        pending_second.state(),
        P0,
        DecisionReply::CopyTargets(CopyTargetChoice::Retain),
    );
    let second_id = second
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    assert_ne!(first_id, second_id);
    for id in [first_id, second_id] {
        assert_eq!(
            second.state().execution().stack_copiable_card_types(id),
            Some(&expected_types)
        );
        assert_eq!(
            second
                .state()
                .execution()
                .stack_copiable_creature_subtypes(id),
            Some(&expected_subtypes)
        );
    }
    for created in [&first, &second] {
        assert!(created.mana_payment().is_none());
        assert!(
            all_occurrences(created)
                .iter()
                .all(|event| !matches!(event, RulesOccurrence::CastCompleted(_)))
        );
        assert!(all_occurrences(created).iter().any(|event| matches!(event,
            RulesOccurrence::CopyCreated { copiable_card_types,copiable_creature_subtypes,.. }
            if *copiable_card_types == expected_types && *copiable_creature_subtypes == expected_subtypes)));
    }
    let resolved_second = pass_pair(second.state());
    assert_eq!(
        resolved_second.state().execution().life_total(P0),
        Some(&21.into())
    );
    assert_eq!(
        resolved_second
            .state()
            .execution()
            .stack_copiable_card_types(second_id),
        None
    );
    assert_eq!(
        resolved_second
            .state()
            .execution()
            .stack_copiable_card_types(first_id),
        Some(&expected_types)
    );
    let resolved_first = pass_pair(resolved_second.state());
    assert_eq!(
        resolved_first.state().execution().life_total(P0),
        Some(&22.into())
    );
    assert!(
        resolved_first
            .state()
            .execution()
            .window()
            .entries()
            .is_empty()
    );
}

#[test]
fn kindred_noncreature_damage_targets_and_zero_lethal_checks_use_creature_status() {
    let vocabulary = vocabulary();
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
                    Some((0, 0)),
                    &["Elf"],
                ),
                true,
            ),
            spell(
                3,
                BodyTarget::SingleCreature,
                vec![
                    Instruction::Damage { amount: 3.into() },
                    effect(
                        vec![type_part(
                            TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
                            SubtypeOperation::Keep,
                        )],
                        ResolvedDuration::NoStatedDuration,
                    ),
                ],
            ),
            spell(
                4,
                BodyTarget::SingleCreatureOrPlayer,
                vec![Instruction::Damage { amount: 1.into() }],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let target = object(&game, 1);
    let (changed, _) = resolve(&game, 3, target);
    assert_eq!(
        changed
            .state()
            .execution()
            .battlefield_marked_damage(target),
        Some(3.into())
    );
    for id in [1, 2] {
        let value = current(changed.state(), object(changed.state(), id));
        assert_eq!(value.zone, ObjectZone::Battlefield);
        assert!(!value.is_creature());
        assert!(value.power_toughness.is_none());
        assert_eq!(
            value.current_creature_subtypes,
            subtypes(&vocabulary, &["Elf"])
        );
    }
    let saved = changed.state().clone();
    for id in [1, 2] {
        assert_eq!(
            changed.state().advance(GameInput {
                basis: changed.state().basis().clone(),
                actor: P0,
                command: GameCommand::Cast {
                    spell: object(changed.state(), 4),
                    target: TargetRef::Object(object(changed.state(), id))
                }
            }),
            Err(GameError::Effect(EffectError::IllegalTarget))
        );
        assert_eq!(changed.state(), &saved);
    }
    // Positive control: the same CreatureOrPlayer contract still accepts a player.
    let cast = submit(
        changed.state(),
        P0,
        GameCommand::Cast {
            spell: object(changed.state(), 4),
            target: TargetRef::Player(P1),
        },
    );
    let damaged = pass_pair(cast.state());
    assert_eq!(damaged.state().execution().life_total(P1), Some(&19.into()));
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
                BodyTarget::SinglePermanent,
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
