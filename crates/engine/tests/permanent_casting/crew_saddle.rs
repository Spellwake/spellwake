use super::*;
use spellwake_engine::{
    activated::PowerAbility,
    characteristics::{PowerToughness, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec},
};

#[test]
fn crew_uses_signed_layer_adjusted_power_and_taps_the_whole_group_atomically() {
    let mut cards = seeds();
    let mut ship = Program::artifact(
        Some(ManaCost::free()),
        vec![ActivatedAbility::tap_for_power(
            ActivatedAbilityId(1),
            PowerAbility::Crew,
            4,
        )],
    );
    let spellwake_engine::program::ProgramKind::Permanent(def) = &mut ship.kind else {
        unreachable!()
    };
    def.printed_pt = Some(
        PowerToughness {
            power: 5.into(),
            toughness: 6.into(),
        }
        .into(),
    );
    cards.push(SeedCard {
        card: CardId(100),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: ship,
    });
    for n in [101, 102] {
        cards.push(creature(
            n,
            A,
            ObjectDestination::Battlefield { controller: A },
            None,
            4,
        ));
    }
    for (n, p) in [(103, 3), (104, -5)] {
        cards.push(spell(
            n,
            A,
            Instruction::CreateContinuousEffect {
                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(PowerToughness {
                    power: p.into(),
                    toughness: 0.into(),
                })]),
                duration: ResolvedDuration::UntilEndOfTurn,
            },
            ManaCost::free(),
            BodyTarget::SingleCreature,
        ));
    }
    let game = setup(Some(Step::PrecombatMain), cards);
    let ship = object(&game, 100);
    let first = object(&game, 101);
    let second = object(&game, 102);
    let pay = |creatures| GameCommand::ActivateWithCreatures {
        source: ship,
        ability: ActivatedAbilityId(1),
        creatures,
    };
    reject(
        &game,
        A,
        pay(vec![first]),
        EffectError::InvalidPowerContributors,
    );
    let plus = targeted(&game, A, 103, TargetRef::Object(first));
    let game = resolve(plus.state()).into_state();
    let minus = targeted(&game, A, 104, TargetRef::Object(second));
    let game = resolve(minus.state()).into_state();
    assert_eq!(
        game.execution()
            .power_toughness(first)
            .unwrap()
            .unwrap()
            .derived
            .power,
        5.into()
    );
    assert_eq!(
        game.execution()
            .power_toughness(second)
            .unwrap()
            .unwrap()
            .derived
            .power,
        (-3).into()
    );
    reject(
        &game,
        A,
        pay(vec![first, second]),
        EffectError::InvalidPowerContributors,
    );
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(first)
            .unwrap()
            .tapped(),
        Some(false)
    );
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(second)
            .unwrap()
            .tapped(),
        Some(false)
    );
    let activation = accept(&game, A, pay(vec![first]));
    let game = resolve(activation.state()).into_state();
    assert!(
        game.execution()
            .current_characteristics(ship)
            .unwrap()
            .unwrap()
            .is_creature()
    );
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(second)
            .unwrap()
            .tapped(),
        Some(false)
    );
}
