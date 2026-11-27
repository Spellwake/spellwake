//! Granted haste/vigilance through layer 6, actual tap costs and declarations.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use KeywordAbility::{Haste, Vigilance};
use spellwake_engine::{
    characteristics::{
        AbilityChange, AbilityRemoval, KeywordAbility, PermanentSelector, ResolvedDuration,
        ResolvedPartSpec, StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
    },
    combat::{AttackDeclaration, CombatDecision},
    effects::EffectError,
    game::{DecisionReply, GameCommand, GameError, GameInput, GameState, RuleFrame},
    mana::{ManaAbilityId, ManaCost, ManaType},
    object_identity::{CardId, ObjectId},
    occurrence::RulesRecord,
    program::{BodyTarget, Program},
};
use std::collections::BTreeSet;

fn grant(
    keywords: &[KeywordAbility],
    duration: ResolvedDuration,
) -> spellwake_engine::program::Instruction {
    effect(
        vec![ResolvedPartSpec::Abilities(AbilityChange::grant_keywords(
            keywords.iter().copied(),
        ))],
        duration,
    )
}
fn new_creature_game() -> GameState {
    let mut source = land_source();
    let replacement =
        Program::creature(Some(ManaCost::free()), 3.into(), 3.into(), BTreeSet::new());
    source.card_types = replacement.card_types;
    source.creature_subtypes = replacement.creature_subtypes;
    source.kind = replacement.kind;
    let game = world(
        vocabulary(),
        vec![
            card(1, source, false),
            spell(
                2,
                BodyTarget::SINGLE_PERMANENT,
                vec![grant(&[Haste, Vigilance], ResolvedDuration::UntilEndOfTurn)],
            ),
            spell(
                3,
                BodyTarget::SINGLE_PERMANENT,
                vec![effect(
                    vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                        AbilityRemoval::All,
                    ))],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            spell(
                5,
                BodyTarget::SINGLE_PERMANENT,
                vec![effect(
                    vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                        AbilityRemoval::NonMana,
                    ))],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            card(
                4,
                Program::creature(None, 4.into(), 4.into(), BTreeSet::new()),
                true,
            ),
        ],
        vec![CardId(4)],
    );
    cast_permanent(&game, 1).into_state()
}
fn history(game: &GameState, source: ObjectId) -> bool {
    game.execution()
        .window()
        .cards()
        .get(source)
        .unwrap()
        .has_continuous_control_since_turn_start()
        .unwrap()
}

#[test]
fn grant_and_nonmana_removal_follow_timestamp_order_without_restoring_printed_abilities() {
    for grant_first in [true, false] {
        let game = new_creature_game();
        let source = object(&game, 1);
        let order = if grant_first { [2, 5] } else { [5, 2] };
        let (first, _) = resolve(&game, order[0], source);
        let (second, _) = resolve(first.state(), order[1], source);
        let current = current(second.state(), source);
        assert_eq!(current.has_keyword(Haste), !grant_first);
        assert_eq!(current.abilities.mana, vec![ManaAbilityId(1)]);
        assert!(current.abilities.activated.is_empty());
        assert!(!history(second.state(), source));
        if grant_first {
            assert_eq!(
                second.state().advance(GameInput {
                    basis: second.state().basis().clone(),
                    actor: P0,
                    command: GameCommand::ActivateMana {
                        source,
                        ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1))
                    }
                }),
                Err(GameError::Effect(EffectError::CreatureTapRestricted))
            );
        } else {
            let paid = activate(second.state(), source, source, true);
            assert_eq!(paid.state().execution().mana(P0, ManaType::Blue), 1.into());
        }
    }
}
fn attack(game: &GameState, source: ObjectId) -> spellwake_engine::game::NativeCandidate {
    let beginning = pass_pair(game);
    let choosing = pass_pair(beginning.state());
    let RuleFrame::Combat(CombatDecision::Attackers(frame)) =
        choosing.state().pending_decision().unwrap().frame()
    else {
        panic!("attackers frame")
    };
    assert!(frame.eligible_attackers().contains(&source));
    reply(
        choosing.state(),
        P0,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: source,
            defender: P1,
            defending_planeswalker: None,
        }]),
    )
}

#[test]
fn granted_haste_allows_new_creature_mana_and_tap_abilities_without_rewriting_history() {
    let game = new_creature_game();
    let source = object(&game, 1);
    assert!(!history(&game, source));
    let saved = game.clone();
    assert_eq!(
        game.advance(GameInput {
            basis: game.basis().clone(),
            actor: P0,
            command: GameCommand::ActivateMana {
                source,
                ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1))
            }
        }),
        Err(GameError::Effect(EffectError::CreatureTapRestricted))
    );
    assert_eq!(game, saved);
    let (granted, _) = resolve(&game, 2, source);
    assert!(!history(granted.state(), source));
    assert!(current(granted.state(), source).has_keyword(Haste));
    let mana = activate(granted.state(), source, source, true);
    assert_eq!(mana.state().execution().mana(P0, ManaType::Blue), 1.into());
    assert!(!history(mana.state(), source));
    let ability = activate(granted.state(), source, source, false);
    let resolved = pass_pair(ability.state());
    assert_eq!(
        current(resolved.state(), source)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4)
    );
    assert!(!history(resolved.state(), source));
}

#[test]
fn haste_allows_attacking_and_vigilance_prevents_the_actual_declaration_tap() {
    let game = new_creature_game();
    let source = object(&game, 1);
    let (granted, _) = resolve(&game, 2, source);
    let attacking = attack(granted.state(), source);
    assert!(!history(attacking.state(), source));
    assert_eq!(
        attacking
            .state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(false)
    );
    assert!(
        attacking
            .state()
            .execution()
            .combat()
            .unwrap()
            .attacker(source)
            .is_some()
    );
    assert!(!attacking.rules_journal().records.iter().any(|record| matches!(record, RulesRecord::Event(event) if matches!(event.cause, spellwake_engine::occurrence::EventCause::AttackTaps { .. }))));
    let (removed, _) = resolve(attacking.state(), 3, source);
    assert!(current(removed.state(), source).keywords.is_empty());
    assert!(
        removed
            .state()
            .execution()
            .combat()
            .unwrap()
            .attacker(source)
            .is_some()
    );
    assert_eq!(
        removed
            .state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(false)
    );
}

#[test]
fn expiring_haste_restores_the_restriction_until_the_controllers_next_turn() {
    let game = new_creature_game();
    let source = object(&game, 1);
    let (granted, _) = resolve(&game, 2, source);
    let expired = cleanup(granted.state());
    assert!(current(expired.state(), source).keywords.is_empty());
    assert!(!history(expired.state(), source));
    let priority = submit(expired.state(), P1, GameCommand::Pass);
    let saved = priority.state().clone();
    assert_eq!(
        priority.state().advance(GameInput {
            basis: priority.state().basis().clone(),
            actor: P0,
            command: GameCommand::ActivateMana {
                source,
                ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1))
            }
        }),
        Err(GameError::Effect(EffectError::CreatureTapRestricted))
    );
    assert_eq!(priority.state(), &saved);
}

#[test]
fn one_ability_component_removes_printed_abilities_then_grants_its_own_keywords() {
    let mut source = land_source();
    let replacement = Program::creature(None, 3.into(), 3.into(), BTreeSet::new());
    source.card_types = replacement.card_types;
    source.creature_subtypes = replacement.creature_subtypes;
    source.kind = replacement.kind;
    let game = world(
        vocabulary(),
        vec![
            card(1, source, true),
            spell(
                2,
                BodyTarget::SINGLE_PERMANENT,
                vec![effect(
                    vec![ResolvedPartSpec::Abilities(AbilityChange {
                        removal: Some(AbilityRemoval::All),
                        granted_exalted: 0,
                        granted_toxic: None,
                        grants_unblockable: false,
                        granted_triggers: vec![],
                        granted_combat_keywords: vec![],
                        granted_keywords: BTreeSet::from([Haste, Vigilance]),
                        suppressed_keywords: BTreeSet::new(),
                        suppressed_activations: None,
                        doesnt_untap: false,
                    })],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let source = object(&game, 1);
    let (completed, _) = resolve(&game, 2, source);
    let current = current(completed.state(), source);
    assert!(current.abilities.identities().is_empty());
    assert_eq!(current.keywords, BTreeSet::from([Haste, Vigilance]));
    assert_eq!(created(&completed).parts.0.len(), 1);
}

#[test]
fn removing_a_live_keyword_generator_recomputes_grants_on_other_creatures() {
    let mut generator = Program::artifact(None, vec![]);
    generator.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_supertypes: std::collections::BTreeSet::new(),
            forbidden_supertypes: std::collections::BTreeSet::new(),
            required_types: BTreeSet::from([spellwake_engine::program::CardType::Creature]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
            [Vigilance],
        ))]),
    });
    let game = world(
        vocabulary(),
        vec![
            card(1, generator, true),
            card(
                2,
                Program::creature(None, 3.into(), 3.into(), BTreeSet::new()),
                true,
            ),
            spell(
                3,
                BodyTarget::SINGLE_PERMANENT,
                vec![effect(
                    vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                        AbilityRemoval::All,
                    ))],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let target = object(&game, 2);
    assert!(current(&game, target).has_keyword(Vigilance));
    let (removed, _) = resolve(&game, 3, object(&game, 1));
    assert!(!current(removed.state(), target).has_keyword(Vigilance));
}
