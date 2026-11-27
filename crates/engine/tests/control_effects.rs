#![allow(dead_code, unused_imports)]
//! Layer-2 control through actual casting, resolution, cleanup and combat cuts.
//! Stack-spell control, attachments, conditional durations and player departures
//! remain unqualified; these are authored programs, not complete-card claims.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        AbilityChange, ControlRecipient, ControllerSelector, KeywordAbility, NumericExpression,
        NumericPair, PermanentSelector, ResolvedDuration, ResolvedPartSpec, StaticAbilityId,
        StaticContinuousAbility, StaticPart, StaticParts,
    },
    combat::AttackDeclaration,
    effects::{CopyTargetChoice, SeedCard},
    game::{DecisionReply, GameCommand, GameState, NativeCandidate},
    mana::{ManaAbilityId, ManaCost, ManaPayment, ManaType},
    object_identity::{CardId, ControllerChange, GameObject, ObjectId},
    occurrence::RulesOccurrence,
    priority::SeatId,
    program::{BodyTarget, CardType, ExecutableBody, Instruction, Program},
    targets::TargetRef,
};
use std::collections::BTreeSet;

fn control(recipient: ControlRecipient, duration: ResolvedDuration) -> Instruction {
    effect(vec![ResolvedPartSpec::Control(recipient)], duration)
}
fn control_spell(
    id: u64,
    owner: SeatId,
    recipient: ControlRecipient,
    duration: ResolvedDuration,
) -> SeedCard {
    let mut seed = spell(
        id,
        BodyTarget::SINGLE_PERMANENT,
        vec![control(recipient, duration)],
    );
    seed.owner = owner;
    seed
}
fn target(id: u64, owner: SeatId, controller: SeatId) -> SeedCard {
    controlled(
        id,
        owner,
        controller,
        Program::creature(Some(ManaCost::free()), 4.into(), 4.into(), BTreeSet::new()),
    )
}
fn physical(game: &GameState, object: ObjectId) -> &GameObject {
    game.execution().window().cards().get(object).unwrap()
}
/// These scenarios stay in P0's turn; an opponent acts after one real pass.
fn resolve_as(game: &GameState, actor: SeatId, id: u64, target: ObjectId) -> NativeCandidate {
    let ready = if actor == P1 {
        submit(game, P0, GameCommand::Pass).into_state()
    } else {
        game.clone()
    };
    let cast = submit(
        &ready,
        actor,
        GameCommand::CastPaid {
            spell: object(&ready, id),
            target: TargetRef::Object(target),
            payment: ManaPayment::default(),
        },
    );
    pass_in_order(cast.state(), if actor == P0 { [P0, P1] } else { [P1, P0] })
}
fn controller_changes(candidate: &NativeCandidate) -> Vec<&ControllerChange> {
    all_occurrences(candidate)
        .into_iter()
        .filter_map(|occurrence| match occurrence {
            RulesOccurrence::ControllerChanged(change) => Some(change),
            _ => None,
        })
        .collect()
}

#[test]
fn expiry_returns_to_the_default_controller_even_when_that_player_is_not_the_owner() {
    let game = world(
        vocabulary(),
        vec![
            target(1, P0, P1),
            control_spell(
                2,
                P0,
                ControlRecipient::ResolvingController,
                ResolvedDuration::UntilEndOfTurn,
            ),
        ],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    let (stolen, _) = resolve(&game, 2, target);
    assert_eq!(physical(stolen.state(), target).owner(), P0);
    assert_eq!(physical(stolen.state(), target).controller(), Some(P0));
    assert_eq!(
        physical(stolen.state(), target).default_controller(),
        Some(P1)
    );
    assert_eq!(
        physical(stolen.state(), target).has_continuous_control_since_turn_start(),
        Some(false)
    );
    assert_eq!(controller_changes(&stolen).len(), 1);
    let ended = cleanup(stolen.state());
    assert_eq!(physical(ended.state(), target).controller(), Some(P1));
    assert_eq!(physical(ended.state(), target).owner(), P0);
    assert_eq!(controller_changes(&ended).len(), 1);
    assert_eq!(
        controller_changes(&ended)[0]
            .after
            .has_continuous_control_since_turn_start(),
        Some(false)
    );
    // P1's actual new turn then establishes control continuously since its start.
    assert_eq!(
        physical(ended.state(), target).has_continuous_control_since_turn_start(),
        Some(true)
    );
}

#[test]
fn expiring_the_newer_effect_reveals_an_older_control_effect_instead_of_the_default() {
    let game = world(
        vocabulary(),
        vec![
            target(1, P0, P0),
            control_spell(
                2,
                P1,
                ControlRecipient::ResolvingController,
                ResolvedDuration::NoStatedDuration,
            ),
            control_spell(
                3,
                P0,
                ControlRecipient::ResolvingController,
                ResolvedDuration::UntilEndOfTurn,
            ),
        ],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    let first = resolve_as(&game, P1, 2, target);
    assert_eq!(physical(first.state(), target).controller(), Some(P1));
    let (second, _) = resolve(first.state(), 3, target);
    assert_eq!(physical(second.state(), target).controller(), Some(P0));
    assert_eq!(
        physical(second.state(), target).default_controller(),
        Some(P0)
    );
    let ended = cleanup(second.state());
    assert_eq!(physical(ended.state(), target).controller(), Some(P1));
    assert_eq!(
        physical(ended.state(), target).default_controller(),
        Some(P0)
    );
    assert_eq!(ended.state().execution().continuous_effects().count(), 1);
}

#[test]
fn an_effect_that_keeps_the_same_controller_does_not_reset_control_history() {
    let game = world(
        vocabulary(),
        vec![
            target(1, P0, P0),
            control_spell(
                2,
                P0,
                ControlRecipient::ResolvingController,
                ResolvedDuration::UntilEndOfTurn,
            ),
        ],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    let before = physical(&game, target).clone();
    let (controlled, _) = resolve(&game, 2, target);
    assert_eq!(physical(controlled.state(), target), &before);
    assert!(controller_changes(&controlled).is_empty());
    let ended = cleanup(controlled.state());
    assert_eq!(physical(ended.state(), target), &before);
    assert!(controller_changes(&ended).is_empty());
}

#[test]
fn owner_control_operand_is_bound_to_the_target_owner_not_the_spell_controller() {
    let game = world(
        vocabulary(),
        vec![
            target(1, P0, P1),
            control_spell(
                2,
                P1,
                ControlRecipient::TargetOwner,
                ResolvedDuration::NoStatedDuration,
            ),
        ],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    let controlled = resolve_as(&game, P1, 2, target);
    assert_eq!(physical(controlled.state(), target).controller(), Some(P0));
    assert_eq!(
        physical(controlled.state(), target).default_controller(),
        Some(P1)
    );
}

#[test]
fn control_effect_from_a_sacrificed_source_keeps_the_captured_ability_controller() {
    let source = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::Sacrifice,
            body: ExecutableBody::new(
                BodyTarget::SINGLE_PERMANENT,
                vec![control(
                    ControlRecipient::ResolvingController,
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        }],
    );
    let game = world(
        vocabulary(),
        vec![card(1, source, true), target(2, P1, P1)],
        vec![CardId(1), CardId(2)],
    );
    let target = object(&game, 2);
    let activated = submit(
        &game,
        P0,
        GameCommand::ActivateAbility {
            source: object(&game, 1),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(target),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        activated
            .state()
            .execution()
            .window()
            .cards()
            .get(object(&game, 1))
            .is_none()
    );
    let controlled = pass_pair(activated.state());
    assert_eq!(physical(controlled.state(), target).controller(), Some(P0));
    assert_eq!(
        physical(controlled.state(), target).default_controller(),
        Some(P1)
    );
    let saved = controlled.state().clone();
    for _ in 0..5 {
        assert_eq!(current(controlled.state(), target).controller, Some(P0));
    }
    assert_eq!(controlled.state(), &saved);
}

#[test]
fn changing_control_away_and_back_within_one_body_removes_the_attacker_permanently() {
    let mut change_twice = spell(
        2,
        BodyTarget::SINGLE_PERMANENT,
        vec![
            control(
                ControlRecipient::ResolvingController,
                ResolvedDuration::NoStatedDuration,
            ),
            control(
                ControlRecipient::TargetOwner,
                ResolvedDuration::NoStatedDuration,
            ),
        ],
    );
    change_twice.owner = P1;
    let game = world(
        vocabulary(),
        vec![target(1, P0, P0), change_twice],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    let initial_epoch = physical(&game, target).control_epoch();
    let beginning = pass_pair(&game);
    let choosing = pass_pair(beginning.state());
    let attacking = reply(
        choosing.state(),
        P0,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: target,
            defender: P1,
            defending_planeswalker: None,
        }]),
    );
    let controlled = resolve_as(attacking.state(), P1, 2, target);
    assert_eq!(physical(controlled.state(), target).controller(), Some(P0));
    assert_ne!(
        physical(controlled.state(), target).control_epoch(),
        initial_epoch
    );
    assert_eq!(
        physical(controlled.state(), target).has_continuous_control_since_turn_start(),
        Some(false)
    );
    assert_eq!(physical(controlled.state(), target).tapped(), Some(true));
    assert!(
        controlled
            .state()
            .execution()
            .combat()
            .unwrap()
            .attacker(target)
            .is_none()
    );
    assert_eq!(controller_changes(&controlled).len(), 2);
    assert_eq!(
        all_occurrences(&controlled)
            .iter()
            .filter(|occurrence| matches!(occurrence, RulesOccurrence::CombatantsRemoved { .. }))
            .count(),
        1
    );
}

fn controller_static() -> Program {
    let mut source = Program::artifact(None, vec![]);
    let count = PermanentSelector {
        required_supertypes: std::collections::BTreeSet::new(),
        forbidden_supertypes: std::collections::BTreeSet::new(),
        required_types: BTreeSet::from([CardType::Creature]),
        controller: ControllerSelector::SourceController,
        ..PermanentSelector::default()
    };
    source.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_supertypes: std::collections::BTreeSet::new(),
            forbidden_supertypes: std::collections::BTreeSet::new(),
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![
            StaticPart::ControlSourceController,
            StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::count(count.clone()),
                toughness: NumericExpression::count(count),
            }),
        ]),
    });
    source
}

#[test]
fn a_changed_source_controller_regenerates_its_static_control_operand() {
    let game = world(
        vocabulary(),
        vec![
            card(1, controller_static(), true),
            target(2, P1, P1),
            control_spell(
                3,
                P1,
                ControlRecipient::ResolvingController,
                ResolvedDuration::NoStatedDuration,
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let target = object(&game, 2);
    let controlled = resolve_as(&game, P1, 3, object(&game, 1));
    assert_eq!(
        physical(controlled.state(), object(controlled.state(), 1)).controller(),
        Some(P1)
    );
    assert_eq!(physical(controlled.state(), target).controller(), Some(P1));
    assert_eq!(
        current(controlled.state(), target)
            .power_toughness
            .unwrap()
            .derived,
        pt(5, 5)
    );
    assert_eq!(controller_changes(&controlled).len(), 2);
}

#[test]
fn permanent_entry_receipts_include_the_controller_after_live_static_evaluation() {
    let game = world(
        vocabulary(),
        vec![
            controlled(1, P1, P1, controller_static()),
            card(
                2,
                Program::creature(Some(ManaCost::free()), 4.into(), 4.into(), BTreeSet::new()),
                false,
            ),
        ],
        vec![CardId(1)],
    );
    let entered = cast_permanent(&game, 2);
    let target = object(entered.state(), 2);
    let receipt = entered.permanent_entry().unwrap();
    assert_eq!(&receipt.after, physical(entered.state(), target));
    assert_eq!(receipt.after.controller(), Some(P1));
    assert_eq!(receipt.after.default_controller(), Some(P0));
    assert_eq!(
        current(entered.state(), target)
            .power_toughness
            .unwrap()
            .derived,
        pt(5, 5)
    );
}

#[test]
fn a_newer_control_record_changes_the_count_without_changing_the_started_affected_set() {
    let game = world(
        vocabulary(),
        vec![
            card(1, controller_static(), true),
            target(2, P1, P1),
            control_spell(
                3,
                P1,
                ControlRecipient::ResolvingController,
                ResolvedDuration::NoStatedDuration,
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let target = object(&game, 2);
    let controlled = resolve_as(&game, P1, 3, target);
    assert_eq!(physical(controlled.state(), target).controller(), Some(P1));
    assert_eq!(
        current(controlled.state(), target)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4)
    );
}

#[test]
fn control_and_haste_share_one_effect_but_do_not_rewrite_control_history() {
    let mut source = land_source();
    let replacement = Program::creature(None, 3.into(), 3.into(), BTreeSet::new());
    source.card_types = replacement.card_types;
    source.creature_subtypes = replacement.creature_subtypes;
    source.kind = replacement.kind;
    let game = world(
        vocabulary(),
        vec![
            controlled(1, P1, P1, source),
            spell(
                2,
                BodyTarget::SINGLE_PERMANENT,
                vec![effect(
                    vec![
                        ResolvedPartSpec::Control(ControlRecipient::ResolvingController),
                        ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([
                            KeywordAbility::Haste,
                        ])),
                    ],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    let (controlled, _) = resolve(&game, 2, target);
    assert_eq!(created(&controlled).parts.0.len(), 2);
    assert_eq!(
        physical(controlled.state(), target).has_continuous_control_since_turn_start(),
        Some(false)
    );
    let mana = submit(
        controlled.state(),
        P0,
        GameCommand::ActivateMana {
            source: target,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(mana.state().execution().mana(P0, ManaType::Blue), 1.into());
    assert_eq!(
        physical(mana.state(), target).has_continuous_control_since_turn_start(),
        Some(false)
    );
}

#[test]
fn copying_an_opponents_control_spell_binds_the_copy_controller_and_new_target() {
    let game = world(
        vocabulary(),
        vec![
            target(1, P0, P0),
            target(2, P1, P1),
            control_spell(
                3,
                P1,
                ControlRecipient::ResolvingController,
                ResolvedDuration::NoStatedDuration,
            ),
            spell(
                4,
                BodyTarget::single_spell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                ))],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let first_target = object(&game, 1);
    let second_target = object(&game, 2);
    let handed = submit(&game, P0, GameCommand::Pass);
    let original = submit(
        handed.state(),
        P1,
        GameCommand::CastPaid {
            spell: object(handed.state(), 3),
            target: TargetRef::Object(first_target),
            payment: ManaPayment::default(),
        },
    );
    let entry = original.state().execution().window().entries()[0].id();
    let handed = submit(original.state(), P1, GameCommand::Pass);
    let copy = submit(
        handed.state(),
        P0,
        GameCommand::CastPaid {
            spell: object(handed.state(), 4),
            target: TargetRef::Stack(entry),
            payment: ManaPayment::default(),
        },
    );
    let choosing = pass_pair(copy.state());
    let copied = reply(
        choosing.state(),
        P0,
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Object(second_target))),
    );
    let copy_effect = pass_pair(copied.state());
    assert_eq!(
        physical(copy_effect.state(), second_target).controller(),
        Some(P0)
    );
    assert_eq!(
        physical(copy_effect.state(), first_target).controller(),
        Some(P0)
    );
    let original_effect = pass_pair(copy_effect.state());
    assert_eq!(
        physical(original_effect.state(), first_target).controller(),
        Some(P1)
    );
    assert_eq!(
        physical(original_effect.state(), second_target).controller(),
        Some(P0)
    );
}
