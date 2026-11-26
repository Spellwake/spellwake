#![allow(dead_code, unused_imports)]
//! Captured face-up permanent definitions, not complete copy-card qualification.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        AbilityChange, AbilityRemoval, Color, ColorOperation, ControllerSelector, DefiningPart,
        DefiningParts, NumericExpression, PermanentSelector, PowerToughness, PrintedPowerToughness,
        PrintedStat, ResolvedDuration, ResolvedPartSpec, StaticAbilityId, StaticColorOperand,
        StaticContinuousAbility, StaticPart, StaticParts, SubtypeOperation, TypeOperation,
    },
    game::{GameCommand, GameState, NativeCandidate},
    life::LifeAdjustment,
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaType},
    object_identity::{CardId, ObjectZone},
    occurrence::{RulesOccurrence, RulesRecord},
    program::{
        BodyTarget, CardType, ExecutableBody, Instruction, Program, ProgramKind, TriggeredAbility,
        TriggeredAbilityId,
    },
    targets::TargetRef,
};
use std::collections::{BTreeMap, BTreeSet};
use trajectories::*;

fn copy_op(duration: ResolvedDuration, power_toughness: Option<PowerToughness>) -> Instruction {
    Instruction::CopyTargetToSource {
        duration,
        power_toughness,
    }
}
fn copier(
    cost: SourceCost,
    duration: ResolvedDuration,
    exception: Option<PowerToughness>,
) -> Program {
    Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: cost,
            body: ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![copy_op(duration, exception)],
            ),
        }],
    )
}
fn copy(game: &GameState, receiver: u64, donor: u64) -> NativeCandidate {
    let activated = activate(game, object(game, receiver), object(game, donor), false);
    pass_pair(activated.state())
}
fn creature(power: i64, toughness: i64, label: &str) -> Program {
    Program::creature(
        None,
        power.into(),
        toughness.into(),
        subtypes(&vocabulary(), &[label]),
    )
}
fn no_duration() -> ResolvedDuration {
    ResolvedDuration::NoStatedDuration
}
fn values(game: &GameState, id: u64) -> PowerToughness {
    current(game, object(game, id))
        .power_toughness
        .unwrap()
        .derived
}
fn occurrences(candidate: &NativeCandidate) -> Vec<&RulesOccurrence> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|r| match r {
            RulesRecord::Event(e) => Some(&e.occurrences),
            _ => None,
        })
        .flatten()
        .map(|o| &o.value)
        .collect()
}

#[test]
fn snapshot_excludes_donor_animation_and_retains_receiver_effect_and_physical_state() {
    let v = vocabulary();
    let mut donor = creature(2, 3, "Elf");
    donor.color_indicator = Some(BTreeSet::from([Color::Green]));
    let game = world(v.clone(), vec![
        card(1, copier(SourceCost::Tap, no_duration(), None), true), card(2, donor.clone(), true),
        spell(3, BodyTarget::SinglePermanent, vec![effect(vec![
            type_part(TypeOperation::Keep, SubtypeOperation::Set(spellwake_engine::characteristics::InstructionSubtypeOperand::Literal(subtypes(&v, &["Vampire"])))),
            ResolvedPartSpec::SetPT(pt(9, 9)), ResolvedPartSpec::Color(ColorOperation::Set(BTreeSet::from([Color::Blue]))),
        ], no_duration())]),
        spell(4, BodyTarget::SinglePermanent, vec![effect(vec![ResolvedPartSpec::ModifyPT(pt(1, 2))], no_duration())]),
    ], vec![CardId(1), CardId(2)]);
    let (animated, _) = resolve(&game, 3, object(&game, 2));
    let (modified, _) = resolve(animated.state(), 4, object(&game, 1));
    let before = modified
        .state()
        .execution()
        .window()
        .cards()
        .get(object(&game, 1))
        .unwrap()
        .clone();
    let completed = copy(modified.state(), 1, 2);
    let state = completed.state();
    let receiver = object(state, 1);
    assert_eq!(values(state, 1), pt(3, 5));
    assert_eq!(values(state, 2), pt(9, 9));
    let current = current(state, receiver);
    assert_eq!(current.current_creature_subtypes, subtypes(&v, &["Elf"]));
    assert_eq!(current.current_colors, BTreeSet::from([Color::Green]));
    assert!(current.abilities.activated.is_empty());
    assert_eq!(state.execution().copiable_program(receiver), Some(&donor));
    let physical = state.execution().window().cards().get(receiver).unwrap();
    assert_eq!(
        physical.card_id().expect("physical fixture card"),
        before.card_id().expect("physical fixture card")
    );
    assert_eq!(physical.owner(), before.owner());
    assert_eq!(physical.controller(), before.controller());
    assert_eq!(physical.tapped(), Some(true));
    assert_eq!(
        physical.has_continuous_control_since_turn_start(),
        before.has_continuous_control_since_turn_start()
    );
    assert_eq!(created(&completed).affected, BTreeSet::from([receiver]));
    assert!(
        !occurrences(&completed)
            .iter()
            .any(|o| matches!(o, RulesOccurrence::ZoneChange(_)))
    );
}

#[test]
fn queued_bodies_survive_copying_and_copy_of_copy_is_a_fixed_snapshot() {
    let game = world(
        vocabulary(),
        vec![
            card(1, copier(SourceCost::None, no_duration(), None), true),
            card(2, copier(SourceCost::None, no_duration(), None), true),
            card(3, creature(2, 3, "Elf"), true),
            card(4, creature(6, 7, "Vampire"), true),
        ],
        (1..=4).map(CardId).collect(),
    );
    let later = activate(&game, object(&game, 1), object(&game, 4), false);
    let earlier = activate(later.state(), object(&game, 1), object(&game, 3), false);
    let first = pass_pair(earlier.state());
    assert_eq!(values(first.state(), 1), pt(2, 3));
    let second = copy(first.state(), 2, 1);
    assert_eq!(values(second.state(), 2), pt(2, 3));
    let changed = pass_pair(second.state());
    assert_eq!(values(changed.state(), 1), pt(6, 7));
    assert_eq!(values(changed.state(), 2), pt(2, 3));
    let saved = changed.state().clone();
    let a = changed
        .state()
        .execution()
        .evaluated_characteristics()
        .unwrap();
    assert_eq!(
        a,
        changed
            .state()
            .execution()
            .evaluated_characteristics()
            .unwrap()
    );
    assert_eq!(changed.state(), &saved);
}

#[test]
fn self_copy_uses_the_preinsertion_definition_and_expires_without_identity_change() {
    let game = world(
        vocabulary(),
        vec![card(
            1,
            copier(SourceCost::None, ResolvedDuration::UntilEndOfTurn, None),
            true,
        )],
        vec![CardId(1)],
    );
    let original = game
        .execution()
        .copiable_program(object(&game, 1))
        .unwrap()
        .clone();
    let copied = copy(&game, 1, 1);
    assert_eq!(copied.state().execution().continuous_effects().count(), 1);
    assert_eq!(
        copied
            .state()
            .execution()
            .copiable_program(object(&game, 1)),
        Some(&original)
    );
    let restored = cleanup(copied.state());
    assert_eq!(restored.state().execution().continuous_effects().count(), 0);
    assert_eq!(object(restored.state(), 1), object(&game, 1));
    assert_eq!(
        restored
            .state()
            .execution()
            .copiable_program(object(&game, 1)),
        Some(&original)
    );
}

fn land_cda(multipart: bool) -> Program {
    let mut donor = Program::creature(None, 0.into(), 0.into(), BTreeSet::new());
    if let ProgramKind::Permanent(d) = &mut donor.kind {
        d.printed_pt = Some(PrintedPowerToughness {
            power: PrintedStat::Star {
                multiplier: 1.into(),
                offset: 0.into(),
            },
            toughness: PrintedStat::Star {
                multiplier: 1.into(),
                offset: 0.into(),
            },
        });
    }
    let count = NumericExpression::count(PermanentSelector {
        required_supertypes: std::collections::BTreeSet::new(),
        forbidden_supertypes: std::collections::BTreeSet::new(),
        required_types: BTreeSet::from([CardType::Land]),
        controller: ControllerSelector::SourceController,
        ..PermanentSelector::default()
    });
    let mut parts = vec![DefiningPart::PowerToughness {
        power: Some(Box::new(count.clone())),
        toughness: Some(Box::new(count)),
    }];
    if multipart {
        parts.push(DefiningPart::CreatureTypes(
            spellwake_engine::characteristics::DefiningCreatureTypes::All,
        ));
    }
    donor.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(1),
        parts: DefiningParts(parts),
    });
    donor
}
#[test]
fn copied_star_definition_counts_for_the_receiver_and_exception_excludes_whole_cda() {
    let mut donor = card(3, land_cda(true), true);
    donor.owner = P1;
    donor.destination =
        spellwake_engine::object_identity::ObjectDestination::Battlefield { controller: P1 };
    let mut other_land = card(
        6,
        Program::land(std::collections::BTreeSet::new(), vec![]),
        true,
    );
    other_land.owner = P1;
    other_land.destination =
        spellwake_engine::object_identity::ObjectDestination::Battlefield { controller: P1 };
    let mut another_land = other_land.clone();
    another_land.card = CardId(7);
    let game = world(
        vocabulary(),
        vec![
            card(1, copier(SourceCost::None, no_duration(), None), true),
            card(
                2,
                copier(SourceCost::None, no_duration(), Some(pt(7, 7))),
                true,
            ),
            donor,
            card(4, copier(SourceCost::None, no_duration(), None), true),
            card(
                5,
                Program::land(std::collections::BTreeSet::new(), vec![]),
                true,
            ),
            other_land,
            another_land,
        ],
        (1..=7).map(CardId).collect(),
    );
    assert_eq!(values(&game, 3), pt(2, 2));
    let copied = copy(&game, 1, 3);
    assert_eq!(values(copied.state(), 1), pt(1, 1));
    assert!(matches!(
        copied
            .state()
            .execution()
            .copiable_power_toughness(object(&game, 1))
            .unwrap()
            .power,
        PrintedStat::Star { .. }
    ));
    let exception = copy(copied.state(), 2, 3);
    assert_eq!(values(exception.state(), 2), pt(7, 7));
    let appearance = current(exception.state(), object(&game, 2));
    assert!(appearance.current_creature_subtypes.is_empty());
    assert!(appearance.abilities.continuous.is_none());
    let next = copy(exception.state(), 4, 2);
    assert_eq!(values(next.state(), 4), pt(7, 7));
    assert!(
        current(next.state(), object(&game, 4))
            .current_creature_subtypes
            .is_empty()
    );
}

#[test]
fn no_box_noncreature_copy_exception_is_latent_then_copiable_into_another_object() {
    let game = world(
        vocabulary(),
        vec![
            card(
                1,
                copier(SourceCost::None, no_duration(), Some(pt(7, 7))),
                true,
            ),
            card(2, Program::artifact(None, vec![]), true),
            card(3, copier(SourceCost::None, no_duration(), None), true),
            spell(
                4,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![type_part(
                        TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                        SubtypeOperation::Keep,
                    )],
                    no_duration(),
                )],
            ),
        ],
        (1..=3).map(CardId).collect(),
    );
    let copied = copy(&game, 1, 2);
    assert!(
        current(copied.state(), object(&game, 1))
            .power_toughness
            .is_none()
    );
    assert_eq!(
        copied
            .state()
            .execution()
            .copiable_power_toughness(object(&game, 1)),
        Some(pt(7, 7).into())
    );
    let next = copy(copied.state(), 3, 1);
    assert!(
        current(next.state(), object(&game, 3))
            .power_toughness
            .is_none()
    );
    let (animated, _) = resolve(next.state(), 4, object(&game, 3));
    assert_eq!(values(animated.state(), 3), pt(7, 7));
}

#[test]
fn sacrificed_receiver_is_a_successful_noop_and_later_body_instruction_runs() {
    let mut source = copier(SourceCost::Sacrifice, no_duration(), None);
    source.activated[0]
        .body
        .instructions
        .push(Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(5.into()),
        });
    let game = world(
        vocabulary(),
        vec![card(1, source, true), card(2, creature(2, 3, "Elf"), true)],
        vec![CardId(1), CardId(2)],
    );
    let activated = activate(&game, object(&game, 1), object(&game, 2), false);
    assert_eq!(
        activated
            .state()
            .execution()
            .window()
            .cards()
            .get(object(activated.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    let timestamps: Vec<_> = activated
        .state()
        .execution()
        .characteristic_timestamps()
        .collect();
    let completed = pass_pair(activated.state());
    assert_eq!(
        completed.state().execution().life_total(P0),
        Some(&25.into())
    );
    assert_eq!(
        completed.state().execution().continuous_effects().count(),
        0
    );
    assert_eq!(
        completed
            .state()
            .execution()
            .characteristic_timestamps()
            .collect::<Vec<_>>(),
        timestamps
    );
    assert!(!occurrences(&completed).iter().any(|o| matches!(
        o,
        RulesOccurrence::ContinuousEffectCreated { .. } | RulesOccurrence::ZoneChange(_)
    )));
}

#[test]
fn donor_type_word_changes_are_not_copied_but_receiver_text_effects_still_apply() {
    let v = vocabulary();
    let donor = creature(2, 3, "Elf");
    let change = |from: &str, to: &str| {
        effect(
            vec![ResolvedPartSpec::Text(
                spellwake_engine::characteristics::CreatureTypeTextChangeSpec {
                    from: spellwake_engine::characteristics::CreatureTypeWord::Literal(
                        v.resolve_label(from).unwrap(),
                    ),
                    to: spellwake_engine::characteristics::CreatureTypeWord::Literal(
                        v.resolve_label(to).unwrap(),
                    ),
                },
            )],
            no_duration(),
        )
    };
    let game = world(
        v.clone(),
        vec![
            card(1, copier(SourceCost::None, no_duration(), None), true),
            card(2, donor.clone(), true),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![change("Elf", "Vampire")],
            ),
            spell(
                4,
                BodyTarget::SinglePermanent,
                vec![change("Elf", "Goblin")],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let (changed, _) = resolve(&game, 3, object(&game, 2));
    let (receiver_changed, _) = resolve(changed.state(), 4, object(&game, 1));
    let copied = copy(receiver_changed.state(), 1, 2);
    assert_eq!(
        current(copied.state(), object(&game, 1)).current_creature_subtypes,
        subtypes(&v, &["Goblin"])
    );
    assert_eq!(
        current(copied.state(), object(&game, 2)).current_creature_subtypes,
        subtypes(&v, &["Vampire"])
    );
    assert_eq!(
        copied
            .state()
            .execution()
            .copiable_program(object(&game, 1)),
        Some(&donor)
    );
}

#[test]
fn losing_creature_during_a_body_does_not_recheck_the_donor_target_restriction() {
    let mut source = copier(SourceCost::None, no_duration(), None);
    source.activated[0].body.target = BodyTarget::SingleCreature;
    source.activated[0].body.instructions.insert(
        0,
        effect(
            vec![type_part(
                TypeOperation::Set(BTreeSet::from([CardType::Artifact])),
                SubtypeOperation::Keep,
            )],
            no_duration(),
        ),
    );
    let game = world(
        vocabulary(),
        vec![card(1, source, true), card(2, creature(2, 3, "Elf"), true)],
        vec![CardId(1), CardId(2)],
    );
    let copied = copy(&game, 1, 2);
    assert_eq!(values(copied.state(), 1), pt(2, 3));
    assert!(!current(copied.state(), object(&game, 2)).is_creature());
}

#[test]
fn cost_indicator_and_non_pt_defining_ability_are_captured_without_duplicate_abilities() {
    let mut donor = creature(2, 3, "Elf");
    if let ProgramKind::Permanent(d) = &mut donor.kind {
        d.casting_cost = Some(ManaCost {
            generic: 2.into(),
            required: BTreeMap::from([(ManaType::Blue, 1.into())]),
        });
    }
    donor.color_indicator = Some(BTreeSet::from([Color::Green]));
    donor.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(1),
        parts: DefiningParts(vec![DefiningPart::Colors(BTreeSet::from([Color::Red]))]),
    });
    let game = world(
        vocabulary(),
        vec![
            card(
                1,
                copier(SourceCost::None, no_duration(), Some(pt(7, 7))),
                true,
            ),
            card(2, donor.clone(), true),
        ],
        vec![CardId(1), CardId(2)],
    );
    let copied = copy(&game, 1, 2);
    let receiver = object(&game, 1);
    let selected = copied
        .state()
        .execution()
        .copiable_program(receiver)
        .unwrap();
    assert_eq!(
        selected.permanent().unwrap().casting_cost,
        donor.permanent().unwrap().casting_cost
    );
    assert_eq!(selected.color_indicator, donor.color_indicator);
    assert_eq!(selected.continuous, donor.continuous);
    let current = current(copied.state(), receiver);
    assert_eq!(current.defined.colors, BTreeSet::from([Color::Green]));
    assert_eq!(current.current_colors, BTreeSet::from([Color::Red]));
    assert_eq!(current.abilities.identities().len(), 1);
    assert_eq!(values(copied.state(), 1), pt(7, 7));
}

#[test]
fn mana_only_source_cost_debits_nonzero_mana_and_rejection_is_atomic() {
    let mut source = copier(SourceCost::None, no_duration(), None);
    source.activated[0].mana_cost = ManaCost::one(ManaType::Green);
    let game = world(
        vocabulary(),
        vec![
            card(1, source, true),
            card(2, creature(2, 3, "Elf"), true),
            card(
                3,
                Program::land(
                    std::collections::BTreeSet::new(),
                    vec![ManaAbility {
                        id: ManaAbilityId(1),
                        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
                    }],
                ),
                true,
            ),
        ],
        (1..=3).map(CardId).collect(),
    );
    let command = GameCommand::ActivateAbility {
        source: object(&game, 1),
        ability: ActivatedAbilityId(1),
        target: TargetRef::Object(object(&game, 2)),
        payment: Some(ManaPayment {
            from_lots: Default::default(),
            spend: vec![spellwake_engine::mana::ManaSpend {
                mana_type: ManaType::Green,
                amount: 1.into(),
            }],
        }),
    };
    let saved = game.clone();
    assert_eq!(
        game.advance(spellwake_engine::game::GameInput {
            basis: game.basis().clone(),
            actor: P0,
            command: command.clone()
        })
        .unwrap_err(),
        spellwake_engine::game::GameError::Effect(
            spellwake_engine::effects::EffectError::InsufficientMana
        )
    );
    assert_eq!(game, saved);
    let funded = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source: object(&game, 3),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let physical = funded.state().execution().window().cards().clone();
    let announced = submit(funded.state(), P0, command);
    assert_eq!(
        announced.state().execution().mana(P0, ManaType::Green),
        0.into()
    );
    assert_eq!(announced.state().execution().window().cards(), &physical);
    assert_eq!(
        announced.mana_payment().unwrap().cost,
        ManaCost::one(ManaType::Green)
    );
    assert!(!occurrences(&announced).iter().any(|o| matches!(
        o,
        RulesOccurrence::StatusChange { .. } | RulesOccurrence::ZoneChange(_)
    )));
    let copied = pass_pair(announced.state());
    assert_eq!(values(copied.state(), 1), pt(2, 3));
}
