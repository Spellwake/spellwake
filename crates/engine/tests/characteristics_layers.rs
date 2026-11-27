#![allow(dead_code, unused_imports)]
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
            vec![spellwake_engine::program::Instruction::DrawCards(Box::new(
                spellwake_engine::toolbox::DrawCardsSpec::simple(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
            ))],
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
fn cl_cleanup_simultaneous() {
    cleanup_simultaneous();
}
#[test]
fn cl_cleanup_noncreature_marks() {
    cleanup_noncreature_marks();
}

#[test]
fn cl_chosen_count_draw_cuts() {
    chosen_count_draw_cuts();
}
