use super::*;
use spellwake_engine::{
    characteristics::KeywordAbility, mana::CastingPayment, program::CardType, targets::TargetRef,
};

#[test]
#[ignore = "requires the dated catalogue and matching creature vocabulary"]
fn hopeful_eidolon_nyxborn_shieldmate_and_boon_satyr_bestow_their_printed_host_bonuses() {
    let (inventory, vocabulary, registry) = current_catalogue();
    for name in [
        "Detective's Phoenix",
        "Glyph Elemental",
        "Eidolon of Countless Battles",
    ] {
        let row = inventory
            .cards
            .iter()
            .find(|card| card.name == name)
            .unwrap();
        assert!(
            super::super::attachments::compile_bestow(row, &vocabulary).is_none(),
            "{name}"
        );
    }
    let (a, b) = (SeatId(0), SeatId(1));
    let rules = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    for (name, green, white, generic, power, toughness, lifelink) in [
        ("Hopeful Eidolon", 0, 1, 3, 2, 2, true),
        ("Nyxborn Shieldmate", 0, 1, 2, 2, 3, false),
        ("Boon Satyr", 2, 0, 3, 5, 3, false),
    ] {
        let src = inventory
            .cards
            .iter()
            .find(|card| card.name == name)
            .unwrap();
        let src_id = OracleId::try_from(src.oracle_id.clone().unwrap()).unwrap();
        let goblin = inventory
            .cards
            .iter()
            .find(|card| card.name == "Raging Goblin")
            .unwrap();
        let goblin_id = OracleId::try_from(goblin.oracle_id.clone().unwrap()).unwrap();
        let cards = vec![
            registry
                .seed(&src_id, CardId(1), a, ObjectDestination::Hand)
                .unwrap_or_else(|err| panic!("{name}: {err:?}")),
            registry
                .seed(
                    &goblin_id,
                    CardId(2),
                    a,
                    ObjectDestination::Battlefield { controller: a },
                )
                .unwrap(),
        ];
        let context = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "bestow-fixture".into(),
            name.into(),
            vocabulary.id(),
            rules.id(),
        )
        .unwrap();
        let spend = [
            (ManaType::Green, green),
            (ManaType::White, white),
            (ManaType::Colourless, generic),
        ]
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(mana_type, n)| ManaSpend {
            mana_type,
            amount: n.into(),
        })
        .collect::<Vec<_>>();
        let mana = spend
            .iter()
            .map(|m| FloatingMana {
                player: a,
                mana_type: m.mana_type,
                amount: m.amount.clone(),
            })
            .collect();
        let mut game = GameState::initialize_scheduled(
            GameId(20),
            context,
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                cards,
                vec![CardId(2)],
                mana,
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                rules.clone(),
            ),
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
        let host = obj(&game, 2);
        game = advance(
            &game,
            a,
            GameCommand::CastWithResources {
                spell: obj(&game, 1),
                target: Some(TargetRef::Object(host)),
                payment: CastingPayment {
                    use_alternative: true,
                    mana: Some(ManaPayment {
                        from_lots: Default::default(),
                        spend,
                    }),
                    ..Default::default()
                },
            },
        );
        assert!(game.execution().is_bestowed(obj(&game, 1)));
        assert_eq!(
            game.execution()
                .current_characteristics(obj(&game, 1))
                .unwrap()
                .unwrap()
                .current_types,
            BTreeSet::from([CardType::Enchantment])
        );
        for _ in 0..2 {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                game.execution().window().phase()
            else {
                panic!("expected ready priority for {name}");
            };
            game = advance(&game, holder, GameCommand::Pass);
        }
        assert_eq!(
            game.execution().attached_to(obj(&game, 1)),
            Some(TargetRef::Object(host))
        );
        let boosted = game
            .execution()
            .current_characteristics(host)
            .unwrap()
            .unwrap();
        assert_eq!(boosted.has_keyword(KeywordAbility::Lifelink), lifelink);
        let pt = boosted.power_toughness.unwrap().derived;
        assert_eq!(
            (pt.power, pt.toughness),
            (power.into(), toughness.into()),
            "{name}"
        );
    }
}
