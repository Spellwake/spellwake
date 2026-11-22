//! Frozen characteristic I recipes through ordinary GameState inputs.
//! Source authoring only; execution requires the separately reviewed input binding.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        ControllerSelector, EffectInstanceKey, NumericExpression, NumericPair, PermanentSelector,
        PowerToughnessCounters, ResolvedDuration, ResolvedPartSpec, StaticAbilityId, StaticPart,
        SubtypeOperation, TypeOperation,
    },
    combat::{AttackDeclaration, CombatDecision},
    effects::CopyTargetChoice,
    game::{DecisionReply, GameCommand, GameState, NativeCandidate, RuleFrame},
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaType},
    object_identity::{CardId, ObjectId, ObjectZone},
    occurrence::{EntryCompletion, EventCause, RulesOccurrence, RulesRecord},
    program::{
        BodyTarget, CardType, CreatureTypeSlot, ExecutableBody, Instruction, Program, ProgramKind,
        TriggerSpec, TriggeredAbility, TriggeredAbilityId,
    },
    targets::TargetRef,
    turn::Step,
};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn cl_type_restore() {
    type_restore();
}

#[test]
fn cl_type_explicit_remove() {
    type_explicit_remove();
}

#[test]
fn cl_type_replace() {
    type_replace();
}

#[test]
fn cl_pt_latent() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Artifact], Some((4, 4)), &[]),
                true,
            ),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::ModifyPT(pt(1, 2))],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![add_creature(ResolvedDuration::NoStatedDuration)],
            ),
            spell(4, BodyTarget::SinglePermanent, vec![counters()]),
            spell(
                5,
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 1.into() }],
            ),
            spell(
                6,
                BodyTarget::SinglePermanent,
                vec![remove_creature(ResolvedDuration::NoStatedDuration)],
            ),
            spell(
                7,
                BodyTarget::SinglePermanent,
                vec![Instruction::ReturnToOwnersHand],
            ),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (latent, _) = resolve(&game, 2, x);
    let latent_id = created(&latent).id;
    // CL-PT-LATENT/a0
    assert_current(latent.state(), &v, x, &[CardType::Artifact], &[], None);
    assert_eq!(
        current(latent.state(), x).defined.printed_pt,
        Some(pt(4, 4).into())
    );
    assert_eq!(
        latent.state().execution().power_toughness_counters(x),
        Some(PowerToughnessCounters::default())
    );
    let (animated, _) = resolve(latent.state(), 3, x);
    // CL-PT-LATENT/a1
    assert_eq!(
        current(animated.state(), x)
            .power_toughness
            .unwrap()
            .derived,
        pt(5, 6)
    );
    let (counted, _) = resolve(animated.state(), 4, x);
    let (marked, _) = resolve(counted.state(), 5, x);
    // CL-PT-LATENT/a2
    assert_eq!(object(marked.state(), 1), x);
    assert_eq!(
        current(marked.state(), x).power_toughness.unwrap().derived,
        pt(6, 7)
    );
    assert_eq!(
        marked.state().execution().battlefield_marked_damage(x),
        Some(1.into())
    );
    assert_eq!(
        marked.state().execution().power_toughness_counters(x),
        Some(PowerToughnessCounters {
            plus_one: 1.into(),
            minus_one: 0.into()
        })
    );
    // CL-PT-LATENT/a3
    assert_eq!(
        marked
            .state()
            .execution()
            .continuous_effect(latent_id)
            .unwrap()
            .affected,
        BTreeSet::from([x])
    );
    let (lost, _) = resolve(marked.state(), 6, x);
    // CL-PT-LATENT/a4
    assert_current(lost.state(), &v, x, &[CardType::Artifact], &[], None);
    assert_eq!(
        lost.state().execution().battlefield_marked_damage(x),
        Some(1.into())
    );
    assert_eq!(
        lost.state().execution().power_toughness_counters(x),
        Some(PowerToughnessCounters {
            plus_one: 1.into(),
            minus_one: 0.into()
        })
    );
    let (exited, _) = resolve(lost.state(), 7, x);
    let hand = object(exited.state(), 1);
    // CL-PT-LATENT/a5
    assert_ne!(hand, x);
    assert_eq!(
        exited
            .state()
            .execution()
            .window()
            .cards()
            .get(hand)
            .unwrap()
            .zone(),
        ObjectZone::Hand(P0)
    );
    assert_eq!(
        current(exited.state(), hand).defined.printed_pt,
        Some(pt(4, 4).into())
    );
    assert_eq!(
        exited.state().execution().copiable_power_toughness(hand),
        Some(pt(4, 4).into())
    );
    assert_eq!(
        current(exited.state(), hand)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4)
    );
    assert!(
        exited
            .state()
            .execution()
            .power_toughness_counters(x)
            .is_none()
    );
    assert!(
        exited
            .state()
            .execution()
            .battlefield_marked_damage(x)
            .is_none()
    );
}

#[test]
fn cl_pt_zero() {
    pt_zero();
}

#[test]
fn cl_pt_multipart() {
    pt_multipart();
}

#[test]
fn cl_pt_core() {
    pt_core();
}

#[test]
fn cl_target_later_bounce() {
    target_later_bounce();
}

#[test]
fn cl_target_initial_illegal() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Creature], Some((2, 3)), &[]),
                true,
            ),
            spell(
                2,
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 3.into() }],
            ),
            spell(
                3,
                BodyTarget::SingleCreature,
                vec![remove_creature(ResolvedDuration::NoStatedDuration)],
            ),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (lower, lower_entry) = cast(&game, 2, x);
    let (upper, _) = resolve(lower.state(), 3, x);
    assert_eq!(upper.state().execution().window().entries().len(), 1);
    let lower_done = pass_pair(upper.state());
    // CL-TARGET-INITIAL-ILLEGAL/a0
    assert_completion(&lower_done, lower_entry, EntryCompletion::AllTargetsIllegal);
    // CL-TARGET-INITIAL-ILLEGAL/a1
    assert_current(lower_done.state(), &v, x, &[], &[], None);
    assert_eq!(
        lower_done
            .state()
            .execution()
            .window()
            .cards()
            .get(x)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        lower_done.state().execution().battlefield_marked_damage(x),
        Some(0.into())
    );
    // CL-TARGET-INITIAL-ILLEGAL/a2
    assert_completion(&lower_done, lower_entry, EntryCompletion::AllTargetsIllegal);
    assert!(lower_done.state().execution().window().entries().is_empty());
    // CL-TARGET-INITIAL-ILLEGAL/a3
    assert!(lower_done.damage_journal().batches.is_empty());
    assert!(lower_done.damage_journal().attempts.is_empty());
}

#[test]
fn cl_typeloss_roster() {
    let v = vocabulary();
    let mut source = permanent(&v, &[CardType::Artifact], None, &[]);
    source.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
    });
    source.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![effect(
                vec![ResolvedPartSpec::ModifyPT(pt(1, 1))],
                ResolvedDuration::UntilEndOfTurn,
            )],
        ),
    });
    let game = world(
        v.clone(),
        vec![
            card(1, source, true),
            card(
                2,
                permanent(&v, &[CardType::Creature], Some((3, 5)), &[]),
                true,
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![type_part(
                        TypeOperation::Remove(BTreeSet::from([CardType::Artifact])),
                        SubtypeOperation::Keep,
                    )],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let source = object(&game, 1);
    let target = object(&game, 2);
    let epoch = game
        .execution()
        .window()
        .cards()
        .get(source)
        .unwrap()
        .control_epoch();
    let (lost, _) = resolve(&game, 3, source);
    // CL-TYPELOSS-ROSTER/a0
    assert_current(lost.state(), &v, source, &[], &[], None);
    let roster = current(lost.state(), source).abilities;
    assert_eq!(roster.mana, vec![ManaAbilityId(1)]);
    assert_eq!(roster.activated, vec![ActivatedAbilityId(1)]);
    assert!(roster.triggered.is_empty());
    assert_eq!(roster.continuous, None);
    // The availability probe is a real ordinary candidate that is retained but not adopted.
    let saved = lost.state().clone();
    let pump_probe = submit(
        lost.state(),
        P0,
        GameCommand::ActivateAbility {
            source,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(target),
            payment: Some(ManaPayment::default()),
        },
    );
    assert_eq!(pump_probe.ability_activation().unwrap().source.id(), source);
    assert_eq!(pump_probe.state().execution().window().entries().len(), 1);
    assert_eq!(
        pump_probe
            .state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(
        current(pump_probe.state(), target)
            .power_toughness
            .unwrap()
            .derived,
        pt(3, 5)
    );
    assert_eq!(lost.state(), &saved);
    let tapped = submit(
        lost.state(),
        P0,
        GameCommand::ActivateMana {
            source,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    // CL-TYPELOSS-ROSTER/a1
    assert_eq!(object(tapped.state(), 1), source);
    assert_eq!(
        tapped
            .state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(
        tapped.state().execution().mana(P0, ManaType::Blue),
        1.into()
    );
    assert!(tapped.state().execution().window().entries().is_empty());
    // CL-TYPELOSS-ROSTER/a2
    assert_eq!(
        lost.state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .control_epoch(),
        epoch
    );
}

#[test]
fn cl_started_nonempty() {
    started_nonempty();
}

#[test]
fn cl_dep_success_not_intent_gc() {
    success_not_intent(true);
}
#[test]
fn cl_dep_success_not_intent_cg() {
    success_not_intent(false);
}

#[test]
fn cl_dep_additive_not_totals() {
    additive_not_totals();
}

fn draw_one_trigger(mut program: Program, condition: TriggerSpec) -> Program {
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
    program
}

fn frozen_entry(land: bool) {
    let v = vocabulary();
    let kind = if land {
        CardType::Land
    } else {
        CardType::Artifact
    };
    let source = static_program(
        &v,
        &[kind],
        None,
        selected_types(&[kind]),
        vec![static_add_creature(), static_set(2, 2)],
    );
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Artifact], Some((4, 4)), &[]),
                true,
            ),
            card(2, source, false),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let old = object(&game, 2);
    let entry = if land {
        submit(&game, P0, GameCommand::PlayLand { land: old })
    } else {
        cast_permanent(&game, 2)
    };
    let source = object(entry.state(), 2);
    let change = zone_change(
        &entry,
        if land {
            old
        } else {
            entry.permanent_entry().unwrap().before.id()
        },
    );
    // CL-ENTRY-LAND/a0; CL-ENTRY-PERMANENT/a0
    assert_ne!(source, old);
    assert_eq!(change.after.object.id(), source);
    assert_eq!(change.after.object.controller(), Some(P0));
    assert_eq!(
        change.after.types,
        BTreeSet::from([kind, CardType::Creature])
    );
    assert_eq!(
        change
            .after
            .current_power_toughness
            .as_ref()
            .unwrap()
            .clone(),
        pt(2, 2)
    );
    assert_eq!(
        change
            .after
            .object
            .has_continuous_control_since_turn_start(),
        Some(false)
    );
    // CL-ENTRY-LAND/a1; CL-ENTRY-PERMANENT/a1
    let stamps = entry
        .state()
        .execution()
        .characteristic_timestamps()
        .collect::<BTreeMap<_, _>>();
    assert!(stamps[&x] < stamps[&source]);
    assert_eq!(
        current(entry.state(), source)
            .power_toughness
            .unwrap()
            .derived,
        pt(2, 2)
    );
    let key = EffectInstanceKey::Static {
        source,
        ability: StaticAbilityId(1),
    };
    assert!(
        entry
            .state()
            .execution()
            .evaluated_characteristics()
            .unwrap()
            .trace
            .applied
            .iter()
            .any(|part| part.key == key)
    );
    // CL-ENTRY-LAND/a2; CL-ENTRY-PERMANENT/a2
    let immediate = if land {
        entry.land_play().unwrap().after.id()
    } else {
        entry.permanent_entry().unwrap().after.id()
    };
    assert_eq!(immediate, source);
    assert_eq!(change.after.object.id(), immediate);
    assert!(
        !entry
            .rules_journal()
            .records
            .iter()
            .any(|record| matches!(record, RulesRecord::TriggerDiscovered(_)))
    );
    // CL-ENTRY-LAND/a3; CL-ENTRY-PERMANENT/a3
    assert_eq!(
        current(entry.state(), x)
            .power_toughness
            .map(|value| value.derived),
        if land { None } else { Some(pt(2, 2)) }
    );
}
#[test]
fn cl_entry_land() {
    frozen_entry(true);
}
#[test]
fn cl_entry_permanent() {
    frozen_entry(false);
}

#[test]
fn cl_dep_disappearing_cab() {
    disappearing(true);
}
#[test]
fn cl_dep_disappearing_bac() {
    disappearing(false);
}

#[test]
fn cl_static_count_controller() {
    static_count_controller();
}

#[test]
fn cl_tap_old_mana() {
    tap_old(true);
}
#[test]
fn cl_tap_old_pump() {
    tap_old(false);
}

#[test]
fn cl_sacrifice_new_creature() {
    let v = vocabulary();
    let mut source = permanent(&v, &[CardType::Land, CardType::Creature], Some((2, 2)), &[]);
    let ProgramKind::Permanent(definition) = &mut source.kind else {
        unreachable!()
    };
    definition.casting_cost = None;
    source.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Sacrifice,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::Damage { amount: 1.into() }],
        ),
    });
    let game = world(
        v.clone(),
        vec![
            card(1, source, false),
            card(
                2,
                permanent(&v, &[CardType::Creature], Some((3, 5)), &[]),
                true,
            ),
        ],
        vec![CardId(2)],
    );
    let land = submit(
        &game,
        P0,
        GameCommand::PlayLand {
            land: object(&game, 1),
        },
    );
    let s = object(land.state(), 1);
    let r = object(land.state(), 2);
    // CL-SACRIFICE-NEW-CREATURE/a0
    assert_eq!(
        land.state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(false)
    );
    // CL-SACRIFICE-NEW-CREATURE/a1
    let activation = activate(land.state(), s, r, false);
    // CL-SACRIFICE-NEW-CREATURE/a2
    let dead = object(activation.state(), 1);
    assert_ne!(dead, s);
    assert_eq!(
        activation
            .state()
            .execution()
            .window()
            .cards()
            .get(dead)
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    // CL-SACRIFICE-NEW-CREATURE/a4
    assert_eq!(activation.ability_activation().unwrap().source.id(), s);
    assert_eq!(
        activation.ability_activation().unwrap().source.zone(),
        ObjectZone::Battlefield
    );
    let completed = pass_pair(activation.state());
    // CL-SACRIFICE-NEW-CREATURE/a3
    assert_eq!(
        completed.state().execution().battlefield_marked_damage(r),
        Some(1.into())
    );
}

#[test]
fn cl_damage_shield() {
    latent_damage(true, false, true);
}
#[test]
fn cl_damage_multiplier() {
    latent_damage(false, true, false);
}
#[test]
fn cl_damage_shield_first() {
    latent_damage(true, true, true);
}
#[test]
fn cl_damage_multiplier_first() {
    latent_damage(true, true, false);
}

#[test]
fn cl_tap_new_mana() {
    new_tap(true);
}
#[test]
fn cl_tap_new_pump() {
    new_tap(false);
}

fn declared_unblocked(game: &GameState, x: ObjectId) -> GameState {
    let beginning = pass_pair(game);
    let attackers = pass_pair(beginning.state());
    let decision = attackers.state().pending_decision().unwrap();
    let RuleFrame::Combat(CombatDecision::Attackers(frame)) = decision.frame() else {
        panic!("declared attacker work")
    };
    assert!(frame.eligible_attackers().contains(&x));
    let declared = reply(
        attackers.state(),
        P0,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: x,
            defender: P1,
            defending_planeswalker: None,
        }]),
    );
    let blockers = pass_pair(declared.state());
    assert_eq!(
        blockers.state().turn().unwrap().position().step,
        Step::DeclareBlockers
    );
    assert_priority(blockers.state(), P0);
    assert!(
        blockers
            .state()
            .execution()
            .combat()
            .unwrap()
            .blockers()
            .next()
            .is_none()
    );
    blockers.into_state()
}
fn assert_combat_removed(candidate: &NativeCandidate, x: ObjectId) {
    let removed = all_occurrences(candidate)
        .into_iter()
        .filter_map(|value| match value {
            RulesOccurrence::CombatantsRemoved { participants } => Some(participants),
            _ => None,
        })
        .flatten()
        .map(|participant| participant.object())
        .collect::<Vec<_>>();
    assert_eq!(removed, vec![x]);
    let combat = candidate.state().execution().combat().unwrap();
    assert!(combat.attackers().next().is_none());
    assert!(combat.blockers().next().is_none());
    assert!(combat.had_attackers());
}
#[test]
fn cl_combat_real_cuts() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Creature], Some((2, 4)), &[]),
                true,
            ),
            spell(
                2,
                BodyTarget::SingleCreature,
                vec![
                    remove_creature(ResolvedDuration::NoStatedDuration),
                    add_creature(ResolvedDuration::NoStatedDuration),
                ],
            ),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let combat = declared_unblocked(&game, x);
    let (completed, _) = resolve(&combat, 2, x);
    // CL-COMBAT-REAL-CUTS/a0
    assert_combat_removed(&completed, x);
    let first = appearance_change(&completed, x);
    assert!(first.after.current_power_toughness.is_none());
    let removal_event = completed
        .rules_journal()
        .records
        .iter()
        .find_map(|record| match record {
            RulesRecord::Event(event)
                if event
                    .occurrences
                    .iter()
                    .any(|row| matches!(row.value, RulesOccurrence::CombatantsRemoved { .. })) =>
            {
                Some(event)
            }
            _ => None,
        })
        .unwrap();
    assert!(matches!(
        removal_event.cause,
        EventCause::Instruction { instruction: 0, .. }
    ));
    // CL-COMBAT-REAL-CUTS/a1
    assert_current(
        completed.state(),
        &v,
        x,
        &[CardType::Creature],
        &[],
        Some((2, 4)),
    );
    assert!(
        completed
            .state()
            .execution()
            .combat()
            .unwrap()
            .had_attackers()
    );
    assert!(
        completed
            .state()
            .execution()
            .combat()
            .unwrap()
            .attackers()
            .next()
            .is_none()
    );
    // CL-COMBAT-REAL-CUTS/a2
    assert!(!completed.rules_journal().records.iter().any(|record|matches!(record,RulesRecord::Event(event) if matches!(event.cause,EventCause::StateBasedActions{..}))));
    // CL-COMBAT-REAL-CUTS/a3
    let damage = pass_pair(completed.state());
    assert_eq!(
        damage.state().turn().unwrap().position().step,
        Step::CombatDamage
    );
    assert!(
        damage
            .combat_assignments()
            .iter()
            .all(|assignment| assignment.assignments.is_empty())
    );
    assert_eq!(damage.state().execution().life_total(P1), Some(&20.into()));
    assert!(
        damage
            .state()
            .execution()
            .combat()
            .unwrap()
            .attackers()
            .next()
            .is_none()
    );
}
#[test]
fn cl_combat_static_exit() {
    let v = vocabulary();
    let mut selector = selected_types(&[CardType::Artifact]);
    selector.exclude_source = true;
    let source = static_program(
        &v,
        &[CardType::Artifact],
        Some((6, 6)),
        selector,
        vec![static_add_creature()],
    );
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Artifact], Some((2, 4)), &[]),
                true,
            ),
            card(2, source, true),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![Instruction::ReturnToOwnersHand],
            ),
            spell(
                4,
                BodyTarget::SinglePermanent,
                vec![add_creature(ResolvedDuration::NoStatedDuration)],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let x = object(&game, 1);
    let s = object(&game, 2);
    let combat = declared_unblocked(&game, x);
    let (bounced, _) = resolve(&combat, 3, s);
    // CL-COMBAT-STATIC-EXIT/a0
    assert_current(bounced.state(), &v, x, &[CardType::Artifact], &[], None);
    assert_combat_removed(&bounced, x);
    let (animated, _) = resolve(bounced.state(), 4, x);
    // CL-COMBAT-STATIC-EXIT/a1
    assert_eq!(
        current(animated.state(), x)
            .power_toughness
            .unwrap()
            .derived,
        pt(2, 4)
    );
    assert!(
        animated
            .state()
            .execution()
            .combat()
            .unwrap()
            .attackers()
            .next()
            .is_none()
    );
    assert!(
        animated
            .state()
            .execution()
            .combat()
            .unwrap()
            .had_attackers()
    );
}

#[test]
fn cl_cleanup_simultaneous() {
    cleanup_simultaneous();
}
#[test]
fn cl_cleanup_noncreature_marks() {
    cleanup_noncreature_marks();
}
#[test]
fn cl_cleanup_repeat() {
    let v = vocabulary();
    let n = draw_one_trigger(
        permanent(&v, &[CardType::Artifact], None, &[]),
        spellwake_engine::program::TriggerSpec::dies(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
    );
    let game = world(
        v.clone(),
        vec![
            card(1, n, true),
            card(
                2,
                permanent(&v, &[CardType::Creature], Some((2, 2)), &[]),
                true,
            ),
            library(3, P0, &v),
            spell(
                4,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::ModifyPT(pt(1, 1))],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
            spell(
                5,
                BodyTarget::SinglePermanent,
                vec![add_creature(ResolvedDuration::NoStatedDuration)],
            ),
            spell(
                6,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::ModifyPT(pt(0, 3))],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let n = object(&game, 1);
    let x = object(&game, 2);
    let d1 = object(&game, 3);
    let (temp, _) = resolve(&game, 4, n);
    let (animated, _) = resolve(temp.state(), 5, n);
    // CL-CLEANUP-REPEAT/a0
    assert_eq!(
        current(animated.state(), n)
            .power_toughness
            .unwrap()
            .derived,
        pt(1, 1)
    );
    let end = reach_end(animated.state(), [P0, P1]);
    let first = pass_pair(&end);
    let group = cleanup_record(&first);
    // CL-CLEANUP-REPEAT/a1
    assert_eq!(
        group
            .2
            .iter()
            .find(|change| change.object == n)
            .unwrap()
            .after
            .current_power_toughness
            .as_ref()
            .unwrap()
            .clone(),
        pt(0, 0)
    );
    // CL-CLEANUP-REPEAT/a2
    assert_eq!(
        first
            .state()
            .execution()
            .window()
            .cards()
            .get(object(first.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    // CL-CLEANUP-REPEAT/a3
    let waiting = first
        .rules_journal()
        .records
        .iter()
        .find_map(|record| match record {
            RulesRecord::TriggerDiscovered(waiting) => Some(waiting),
            _ => None,
        })
        .unwrap();
    assert_eq!(waiting.source.object.id(), n);
    assert_eq!(waiting.controller, P0);
    assert_eq!(
        waiting
            .source
            .current_power_toughness
            .as_ref()
            .unwrap()
            .clone(),
        pt(0, 0)
    );
    assert_eq!(
        waiting.body,
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: spellwake_engine::program::ValueExpr::Literal(1.into())
            }]
        )
    );
    assert_eq!(first.state().execution().window().entries().len(), 1);
    let trigger_entry = first.state().execution().window().entries()[0].id();
    assert_eq!(
        first
            .state()
            .execution()
            .execution_bindings(trigger_entry)
            .unwrap()
            .trigger
            .as_ref()
            .unwrap()
            .source
            .object
            .id(),
        n
    );
    assert_eq!(
        first.state().execution().stack_body(trigger_entry).unwrap(),
        &waiting.body
    );
    let drawn = pass_pair(first.state());
    // CL-CLEANUP-REPEAT/a4
    assert_eq!(
        zone_change(&drawn, d1).after.object.zone(),
        ObjectZone::Hand(P0)
    );
    assert_eq!(drawn.state().turn().unwrap().position().step, Step::Cleanup);
    assert_eq!(
        drawn.state().turn().unwrap().position().cleanup_iteration,
        0
    );
    let (boost, _) = resolve(drawn.state(), 6, x);
    let boost_id = created(&boost).id;
    // CL-CLEANUP-REPEAT/a5
    assert_eq!(
        current(boost.state(), x).power_toughness.unwrap().derived,
        pt(2, 5)
    );
    let repeated = pass_pair(boost.state());
    // CL-CLEANUP-REPEAT/a6
    assert_eq!(
        current(repeated.state(), x)
            .power_toughness
            .unwrap()
            .derived,
        pt(2, 2)
    );
    assert_eq!(
        repeated.state().execution().battlefield_marked_damage(x),
        Some(0.into())
    );
    assert_eq!(
        repeated
            .state()
            .execution()
            .window()
            .cards()
            .get(x)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    // CL-CLEANUP-REPEAT/a7
    let iterations = [&first, &repeated]
        .into_iter()
        .flat_map(|candidate| candidate.rules_journal().records.iter())
        .filter_map(|record| match record {
            RulesRecord::Event(event) => match event.cause {
                EventCause::CleanupCharacteristics { position } => Some(position.cleanup_iteration),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(iterations, vec![0, 1]);
    assert_eq!(*cleanup_record(&repeated).0, vec![boost_id]);
    assert_eq!(repeated.state().turn().unwrap().position().number, 2);
    assert_eq!(
        repeated.state().turn().unwrap().position().step,
        Step::Upkeep
    );
    assert!(
        repeated
            .state()
            .execution()
            .continuous_effect(boost_id)
            .is_none()
    );
}

#[test]
fn cl_lifetimes_lookback() {
    let v = vocabulary();
    let mut source = permanent(&v, &[CardType::Artifact], None, &[]);
    source.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![effect(
                vec![ResolvedPartSpec::ModifyPT(pt(1, 1))],
                ResolvedDuration::NoStatedDuration,
            )],
        ),
    });
    let live = static_program(
        &v,
        &[CardType::Artifact],
        Some((6, 6)),
        PermanentSelector {
            controller: ControllerSelector::SourceController,
            ..selected_types(&[CardType::Creature])
        },
        vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(1.into()),
            toughness: NumericExpression::Literal(1.into()),
        })],
    );
    let target = draw_one_trigger(
        permanent(&v, &[CardType::Creature], Some((2, 3)), &["Elf"]),
        spellwake_engine::program::TriggerSpec::leaves_battlefield(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
    );
    let game = world(
        v.clone(),
        vec![
            card(1, source, true),
            card(2, live, true),
            card(3, target, true),
            library(4, P0, &v),
            spell(
                5,
                BodyTarget::SinglePermanent,
                vec![Instruction::ReturnToOwnersHand],
            ),
            spell(
                6,
                BodyTarget::SinglePermanent,
                vec![Instruction::ReturnToOwnersHand],
            ),
            spell(
                7,
                BodyTarget::SinglePermanent,
                vec![Instruction::ReturnToOwnersHand],
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let s = object(&game, 1);
    let l = object(&game, 2);
    let x = object(&game, 3);
    let d1 = object(&game, 4);
    let old_stamp = game
        .execution()
        .characteristic_timestamps()
        .find(|(object, _)| *object == l)
        .unwrap()
        .1;
    // CL-LIFETIMES-LOOKBACK/a0
    assert_eq!(current(&game, x).power_toughness.unwrap().derived, pt(3, 4));
    let activated = activate(&game, s, x, false);
    let pumped = pass_pair(activated.state());
    let fixed = created(&pumped).id;
    // CL-LIFETIMES-LOOKBACK/a1
    assert_eq!(
        current(pumped.state(), x).power_toughness.unwrap().derived,
        pt(4, 5)
    );
    let (source_gone, _) = resolve(pumped.state(), 5, s);
    // CL-LIFETIMES-LOOKBACK/a2
    assert_eq!(
        current(source_gone.state(), x)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 5)
    );
    assert!(
        source_gone
            .state()
            .execution()
            .continuous_effect(fixed)
            .is_some()
    );
    let (live_gone, _) = resolve(source_gone.state(), 6, l);
    // CL-LIFETIMES-LOOKBACK/a3
    assert_eq!(
        current(live_gone.state(), x)
            .power_toughness
            .unwrap()
            .derived,
        pt(3, 4)
    );
    let returned = cast_permanent(live_gone.state(), 2);
    let new_l = object(returned.state(), 2);
    // CL-LIFETIMES-LOOKBACK/a4
    assert_ne!(new_l, l);
    assert_eq!(
        current(returned.state(), x)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 5)
    );
    let (target_gone, _) = resolve(returned.state(), 7, x);
    let captured = zone_change(&target_gone, x);
    // CL-LIFETIMES-LOOKBACK/a5
    assert_eq!(captured.before.object.id(), x);
    assert_eq!(captured.before.types, BTreeSet::from([CardType::Creature]));
    assert_eq!(captured.before.creature_subtypes, subtypes(&v, &["Elf"]));
    assert_eq!(
        captured
            .before
            .current_power_toughness
            .as_ref()
            .unwrap()
            .clone(),
        pt(4, 5)
    );
    assert_eq!(captured.before.object.controller(), Some(P0));
    assert_eq!(
        captured.before.abilities.triggered,
        vec![TriggeredAbilityId(1)]
    );
    assert!(captured.before.abilities.mana.is_empty());
    assert!(captured.before.abilities.activated.is_empty());
    // CL-LIFETIMES-LOOKBACK/a6
    let waiting = target_gone
        .rules_journal()
        .records
        .iter()
        .find_map(|record| match record {
            RulesRecord::TriggerDiscovered(waiting) => Some(waiting),
            _ => None,
        })
        .unwrap();
    assert_eq!(waiting.source.object.id(), x);
    assert_eq!(waiting.controller, P0);
    assert_eq!(
        waiting.body,
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: spellwake_engine::program::ValueExpr::Literal(1.into())
            }]
        )
    );
    assert!(
        target_gone
            .state()
            .execution()
            .window()
            .cards()
            .get(x)
            .is_none()
    );
    assert_eq!(target_gone.state().execution().window().entries().len(), 1);
    let trigger_entry = target_gone.state().execution().window().entries()[0].id();
    assert_eq!(
        target_gone
            .state()
            .execution()
            .execution_bindings(trigger_entry)
            .unwrap()
            .trigger
            .as_ref()
            .unwrap()
            .source
            .object
            .id(),
        x
    );
    assert_eq!(
        target_gone
            .state()
            .execution()
            .stack_body(trigger_entry)
            .unwrap(),
        &waiting.body
    );
    let drawn = pass_pair(target_gone.state());
    let reentered = cast_permanent(drawn.state(), 3);
    let new_x = object(reentered.state(), 3);
    // CL-LIFETIMES-LOOKBACK/a7
    assert_ne!(new_x, x);
    assert_current(
        reentered.state(),
        &v,
        new_x,
        &[CardType::Creature],
        &["Elf"],
        Some((3, 4)),
    );
    assert!(
        reentered
            .state()
            .execution()
            .continuous_effect(fixed)
            .is_none()
    );
    // CL-LIFETIMES-LOOKBACK/a8
    assert_eq!(
        zone_change(&drawn, d1).after.object.zone(),
        ObjectZone::Hand(P0)
    );
    // CL-LIFETIMES-LOOKBACK/a9
    assert!(
        source_gone
            .state()
            .execution()
            .continuous_effect(fixed)
            .is_some()
    );
    assert!(
        target_gone
            .state()
            .execution()
            .continuous_effect(fixed)
            .is_none()
    );
    let stamps = reentered
        .state()
        .execution()
        .characteristic_timestamps()
        .collect::<BTreeMap<_, _>>();
    assert!(old_stamp < stamps[&new_l]);
    let evaluation = reentered
        .state()
        .execution()
        .evaluated_characteristics()
        .unwrap();
    assert!(evaluation.trace.applied.iter().any(|part| part.key
        == EffectInstanceKey::Static {
            source: new_l,
            ability: StaticAbilityId(1)
        }));
    assert!(!evaluation.trace.applied.iter().any(|part| part.key
        == EffectInstanceKey::Static {
            source: l,
            ability: StaticAbilityId(1)
        }));
}

#[test]
fn cl_chosen_count_draw_cuts() {
    chosen_count_draw_cuts();
}
#[test]
fn cl_copied_body_fresh_locals() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Creature], Some((2, 5)), &["Elf"]),
                true,
            ),
            card(
                2,
                permanent(&v, &[CardType::Creature], Some((1, 4)), &["Goblin"]),
                true,
            ),
            library(3, P0, &v),
            library(4, P0, &v),
            spell(
                5,
                BodyTarget::SingleCreature,
                vec![
                    type_choice_body(),
                    Instruction::Damage { amount: 1.into() },
                    chosen_draw(),
                ],
            ),
            spell(
                6,
                BodyTarget::SingleSpell(
                    spellwake_engine::program::SpellFilter::copyable_instruction(),
                ),
                vec![Instruction::CopySpell],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let x = object(&game, 1);
    let d1 = object(&game, 3);
    let d2 = object(&game, 4);
    let definitions = game
        .execution()
        .programs()
        .map(|(card, program)| (card, program.clone()))
        .collect::<BTreeMap<_, _>>();
    let (original, original_entry) = cast(&game, 5, x);
    let copying = submit(
        original.state(),
        P0,
        GameCommand::CastPaid {
            spell: object(original.state(), 6),
            target: TargetRef::Stack(original_entry),
            payment: ManaPayment::default(),
        },
    );
    let copy_targets = pass_pair(copying.state());
    assert!(matches!(
        copy_targets.state().pending_decision().unwrap().frame(),
        RuleFrame::CopyTargets(_)
    ));
    let copied = reply(
        copy_targets.state(),
        P0,
        DecisionReply::CopyTargets(CopyTargetChoice::Retain),
    );
    let copies = copied
        .state()
        .execution()
        .window()
        .spell_copies()
        .keys()
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(copies.len(), 1);
    let copy = copies[0];
    let copy_pending = pass_pair(copied.state());
    // CL-COPIED-BODY-FRESH-LOCALS/a0
    assert_type_frame(copy_pending.state(), copy);
    let copy_done = reply(
        copy_pending.state(),
        P0,
        DecisionReply::CreatureType(v.resolve_label("Elf").unwrap()),
    );
    let original_pending = pass_pair(copy_done.state());
    // CL-COPIED-BODY-FRESH-LOCALS/a1
    assert_type_frame(original_pending.state(), original_entry);
    let original_done = reply(
        original_pending.state(),
        P0,
        DecisionReply::CreatureType(v.resolve_label("Goblin").unwrap()),
    );
    // CL-COPIED-BODY-FRESH-LOCALS/a2
    assert_eq!(
        zone_change(&copy_done, d1).after.object.zone(),
        ObjectZone::Hand(P0)
    );
    assert_eq!(
        zone_change(&original_done, d2).after.object.zone(),
        ObjectZone::Hand(P0)
    );
    assert_eq!(copy_done.draw_journal().events.len(), 1);
    assert_eq!(original_done.draw_journal().events.len(), 1);
    // CL-COPIED-BODY-FRESH-LOCALS/a3
    assert_eq!(
        original_done
            .state()
            .execution()
            .battlefield_marked_damage(x),
        Some(2.into())
    );
    assert_eq!(
        current(original_done.state(), x)
            .power_toughness
            .unwrap()
            .derived,
        pt(2, 5)
    );
    assert_eq!(
        original_done
            .state()
            .execution()
            .window()
            .cards()
            .get(object(original_done.state(), 5))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    assert!(
        original_done
            .state()
            .execution()
            .window()
            .spell_copies()
            .is_empty()
    );
    assert!(
        original_done
            .state()
            .execution()
            .window()
            .cards()
            .get(object(original_done.state(), 3))
            .unwrap()
            .zone()
            == ObjectZone::Hand(P0)
    );
    assert!(
        original_done
            .state()
            .execution()
            .window()
            .cards()
            .get(object(original_done.state(), 4))
            .unwrap()
            .zone()
            == ObjectZone::Hand(P0)
    );
    // CL-COPIED-BODY-FRESH-LOCALS/a4
    assert_eq!(
        original_done
            .state()
            .execution()
            .programs()
            .map(|(card, program)| (card, program.clone()))
            .collect::<BTreeMap<_, _>>(),
        definitions
    );
    assert!(
        original_done
            .state()
            .execution()
            .resolution_state()
            .is_none()
    );
    let choices = [&copy_done, &original_done]
        .into_iter()
        .flat_map(all_occurrences)
        .filter_map(|value| match value {
            RulesOccurrence::CreatureTypeChosen {
                entry, slot, value, ..
            } => Some((*entry, *slot, *value)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        choices,
        vec![
            (
                copy,
                CreatureTypeSlot::new(0),
                v.resolve_label("Elf").unwrap()
            ),
            (
                original_entry,
                CreatureTypeSlot::new(0),
                v.resolve_label("Goblin").unwrap()
            )
        ]
    );
}
