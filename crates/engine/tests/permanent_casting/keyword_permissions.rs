#![allow(dead_code, unused_imports)]
use super::*;
use spellwake_engine::characteristics::KeywordAbility::{
    self, Flash, Hexproof, Shroud, SplitSecond,
};
use spellwake_engine::mana::ManaAbilityRef;

fn printed(cards: &mut [SeedCard], id: u64, keyword: KeywordAbility) {
    let definition = cards
        .iter_mut()
        .find(|card| card.card == CardId(id))
        .unwrap();
    definition.program.printed_keywords.insert(keyword);
}

fn target(game: &GameState, spell: u64, permanent: u64) -> GameCommand {
    GameCommand::Cast {
        spell: object(game, spell),
        target: TargetRef::Object(object(game, permanent)),
    }
}

#[test]
fn nonactive_player_can_cast_a_flash_creature_above_a_paid_spell() {
    let mut cards = seeds();
    printed(&mut cards, 3, Flash);
    let game = setup(Some(Step::PrecombatMain), cards);
    let first = cast(&game, A, 1, Some(ordinary_payment()));
    let priority = pass(first.state()).into_state();
    let response = cast(&priority, B, 3, None);
    let resolved = resolve(response.state());
    assert_eq!(
        physical(resolved.state(), 3).zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(resolved.state().execution().window().entries().len(), 1);
    assert_eq!(physical(resolved.state(), 1).zone(), ObjectZone::Stack);
}

#[test]
fn flash_land_uses_the_turns_land_allowance_during_upkeep() {
    let mut cards = seeds();
    printed(&mut cards, 8, Flash);
    let game = setup(Some(Step::Upkeep), cards);
    let land = accept(
        &game,
        A,
        GameCommand::PlayLand {
            land: object(&game, 8),
        },
    );
    assert_eq!(land.state().turn().unwrap().lands_played(), &1.into());
    assert_eq!(physical(land.state(), 8).zone(), ObjectZone::Battlefield);
}

#[test]
fn shroud_bars_both_controllers_and_hexproof_allows_the_controller() {
    for (keyword, controller, allowed) in [
        (Shroud, A, false),
        (Shroud, B, false),
        (Hexproof, B, false),
        (Hexproof, A, true),
    ] {
        let mut cards = seeds();
        printed(&mut cards, 90, keyword);
        cards
            .iter_mut()
            .find(|c| c.card == CardId(90))
            .unwrap()
            .destination = ObjectDestination::Battlefield { controller };
        let game = setup(Some(Step::PrecombatMain), cards);
        let command = target(&game, 10, 90);
        if allowed {
            let spell = accept(&game, A, command);
            let resolved = resolve(spell.state());
            assert_eq!(
                resolved
                    .state()
                    .execution()
                    .creature(object(&game, 90))
                    .unwrap()
                    .unwrap()
                    .damage,
                2.into()
            );
        } else {
            reject(&game, A, command, EffectError::IllegalTarget);
        }
    }
}

#[test]
fn split_second_blocks_casts_and_nonmana_activations_until_the_spell_leaves() {
    let mut cards = seeds();
    printed(&mut cards, 10, SplitSecond);
    let active = cards.iter_mut().find(|c| c.card == CardId(2)).unwrap();
    active.destination = ObjectDestination::Battlefield { controller: A };
    cards.push(land(
        92,
        ObjectDestination::Battlefield { controller: A },
        false,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let spell = accept(&game, A, target(&game, 10, 90)).into_state();
    let activation = GameCommand::ActivateAbility {
        source: object(&spell, 2),
        ability: ActivatedAbilityId(7),
        target: TargetRef::Object(object(&spell, 90)),
        payment: None,
    };
    reject(&spell, A, activation.clone(), EffectError::SplitSecond);
    reject(&spell, A, target(&spell, 13, 90), EffectError::SplitSecond);
    let mana = accept(
        &spell,
        A,
        GameCommand::ActivateMana {
            source: object(&spell, 92),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert!(mana.mana_production().is_some());
    let other = pass(mana.state()).into_state();
    let entry = other.execution().window().entries()[0].id();
    reject(
        &other,
        B,
        GameCommand::Cast {
            spell: object(&other, 11),
            target: TargetRef::Stack(entry),
        },
        EffectError::SplitSecond,
    );
    let resolved = pass(&other);
    assert!(resolved.state().execution().window().entries().is_empty());
    assert!(
        accept(resolved.state(), A, activation)
            .ability_activation()
            .is_some()
    );
}

#[test]
fn split_second_allows_a_cast_trigger_to_resolve_above_the_spell() {
    use spellwake_engine::{
        life::LifeAdjustment,
        program::{TriggerGrouping, TriggeredAbility, TriggeredAbilityId},
    };
    let mut cards = seeds();
    printed(&mut cards, 10, SplitSecond);
    cards
        .iter_mut()
        .find(|seed| seed.card == CardId(10))
        .unwrap()
        .program
        .triggered
        .push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: spellwake_engine::program::TriggerSpec::self_cast(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        });
    let game = setup(Some(Step::PrecombatMain), cards);
    let cast = accept(&game, A, target(&game, 10, 90)).into_state();
    assert_eq!(cast.execution().window().entries().len(), 2);
    reject(&cast, A, target(&cast, 13, 90), EffectError::SplitSecond);
    let triggered = resolve(&cast).into_state();
    assert_eq!(
        triggered.execution().life_total(A),
        Some(&game.execution().life_total(A).unwrap().plus(&1.into()))
    );
    reject(
        &triggered,
        A,
        target(&triggered, 13, 90),
        EffectError::SplitSecond,
    );
    let completed = resolve(&triggered).into_state();
    assert!(completed.execution().window().entries().is_empty());
}
