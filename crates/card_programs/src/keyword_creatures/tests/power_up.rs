//! The pinned Oracle faces supply the printed cast costs, Power-up prices and effects.
use super::*;
use spellwake_engine::{
    activated::ActivationRestriction, mana::ManaSpend, priority::PriorityPhase,
    stack_window::WindowPhase,
};
#[test]
#[ignore = "requires pinned Oracle and creature vocabulary inputs"]
fn aerial_doombot_bold_biochemist_brave_brawler_serpent_specialist_and_volcanic_villain_pay_power_up()
 {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    for (name, color, cast_total, full_total, discounted, counters, draw) in [
        ("Aerial Doombot", ManaType::Blue, 1, 6, 5, 3, 0),
        ("Bold Biochemist", ManaType::Blue, 2, 6, 4, 1, 2),
        ("Brave Brawler", ManaType::White, 2, 5, 3, 2, 0),
        ("Serpent Specialist", ManaType::Green, 1, 4, 3, 2, 0),
        ("Volcanic Villain", ManaType::Red, 3, 6, 3, 2, 0),
    ] {
        let row = inventory.cards.iter().find(|r| r.name == name).unwrap();
        let oracle = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        let home = ObjectDestination::Battlefield { controller: a };
        let mut cards = Vec::new();
        for (n, owner, zone) in [
            (1, a, ObjectDestination::Hand),
            (2, a, home),
            (3, a, ObjectDestination::LibraryBottom),
            (4, a, ObjectDestination::LibraryBottom),
            (5, b, ObjectDestination::LibraryBottom),
        ] {
            cards.push(
                registry
                    .seed(&oracle, CardId(n), owner, zone)
                    .unwrap_or_else(|e| panic!("{name}: {e:?}")),
            );
        }
        let ability = *cards[0]
            .program
            .activation_restrictions
            .iter()
            .find(|(_, r)| **r == ActivationRestriction::PowerUp)
            .unwrap()
            .0;
        let names = std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let ctx = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "fixed-power-up".into(),
            "power-up-sources".into(),
            vocabulary.id(),
            names.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(2)],
            vec![FloatingMana {
                player: a,
                mana_type: color,
                amount: 20.into(),
            }],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary.clone(),
            names,
        );
        let mut game = GameState::initialize_scheduled(
            GameId(193),
            ctx,
            seed,
            TurnStart {
                number: 1,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let obj = |s: &GameState, n| {
            s.execution()
                .window()
                .cards()
                .current_identity(CardId(n))
                .unwrap()
        };
        let pay = |amount: i64| {
            Some(ManaPayment {
                from_lots: Default::default(),
                spend: vec![ManaSpend {
                    mana_type: color,
                    amount: amount.into(),
                }],
            })
        };
        let resolve = |s: &GameState| {
            let pass = |s: &GameState| {
                let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                    s.execution().window().phase()
                else {
                    panic!("priority")
                };
                advance(s, holder, GameCommand::Pass)
            };
            pass(&pass(s))
        };
        game = advance(
            &game,
            a,
            GameCommand::CastUntargeted {
                spell: obj(&game, 1),
                payment: pay(cast_total),
            },
        );
        game = resolve(&game);
        let ability_command = |s: &GameState, n, amount| GameCommand::ActivateUntargetedAbility {
            source: obj(s, n),
            ability,
            payment: pay(amount),
        };
        assert!(
            game.advance(GameInput {
                basis: game.basis().clone(),
                actor: a,
                command: ability_command(&game, 2, discounted)
            })
            .is_err(),
            "{name}"
        );
        let before = game.execution().window().cards().library(a).len();
        let receipt = game
            .advance(GameInput {
                basis: game.basis().clone(),
                actor: a,
                command: ability_command(&game, 1, discounted),
            })
            .unwrap();
        assert_eq!(
            receipt.mana_payment().unwrap().cost,
            ManaCost::generic(discounted.into()),
            "{name}"
        );
        game = receipt.into_state();
        assert!(
            game.advance(GameInput {
                basis: game.basis().clone(),
                actor: a,
                command: ability_command(&game, 1, discounted)
            })
            .is_err()
        );
        game = resolve(&game);
        assert_eq!(
            game.execution().window().cards().library(a).len(),
            before - draw
        );
        assert_eq!(
            game.execution()
                .power_toughness_counters(obj(&game, 1))
                .unwrap()
                .plus_one,
            counters.into(),
            "{name}"
        );
        let full = game
            .advance(GameInput {
                basis: game.basis().clone(),
                actor: a,
                command: ability_command(&game, 2, full_total),
            })
            .unwrap();
        assert_eq!(
            full.mana_payment().unwrap().cost.total(),
            full_total.into(),
            "{name}"
        );
        assert_eq!(
            full.mana_payment().unwrap().cost.required.get(&color),
            Some(&1.into()),
            "{name}"
        );
    }
}
