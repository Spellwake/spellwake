use super::*;
use spellwake_engine::{
    mana::{CastResource, CastingPayment},
    targets::TargetRef,
};

#[test]
#[ignore = "requires the dated catalogue and source-qualified creature vocabulary"]
fn candy_grapple_and_archons_glory_resolve_with_or_without_a_sacrifice() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    for (name, color, generic) in [
        ("Candy Grapple", ManaType::Black, 1),
        ("Archon's Glory", ManaType::White, 0),
    ] {
        let oracle = OracleId::try_from(
            inventory
                .cards
                .iter()
                .find(|card| card.name == name)
                .unwrap()
                .oracle_id
                .clone()
                .unwrap(),
        )
        .unwrap();
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "bargain".into(),
            "source-games".into(),
            vocabulary.id(),
            names.id(),
        )
        .unwrap();
        let cards = vec![
            registry
                .seed(&oracle, CardId(1), a, ObjectDestination::Hand)
                .unwrap(),
            SeedCard {
                card: CardId(2),
                owner: b,
                destination: ObjectDestination::Battlefield { controller: b },
                program: Program::creature(None, 8.into(), 8.into(), BTreeSet::new()),
            },
            SeedCard {
                card: CardId(3),
                owner: b,
                destination: ObjectDestination::Battlefield { controller: a },
                program: Program::artifact(None, vec![]),
            },
            SeedCard {
                card: CardId(4),
                owner: a,
                destination: ObjectDestination::Battlefield { controller: b },
                program: Program::artifact(None, vec![]),
            },
        ];
        let mut spend = vec![ManaSpend {
            mana_type: color,
            amount: 1.into(),
        }];
        if generic > 0 {
            spend.push(ManaSpend {
                mana_type: ManaType::Colourless,
                amount: generic.into(),
            });
        }
        let mana = spend
            .iter()
            .map(|s| FloatingMana {
                player: a,
                mana_type: s.mana_type,
                amount: s.amount.clone(),
            })
            .collect();
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(2), CardId(3), CardId(4)],
            mana,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary.clone(),
            names,
        );
        let base = GameState::initialize_scheduled(
            GameId(23),
            context,
            seed,
            TurnStart {
                number: 1,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let id = |game: &GameState, n| {
            game.execution()
                .window()
                .cards()
                .current_identity(CardId(n))
                .unwrap()
        };
        let cast = |objects| GameCommand::CastWithResources {
            spell: id(&base, 1),
            target: Some(TargetRef::Object(id(&base, 2))),
            payment: CastingPayment {
                objects,
                mana: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend: spend.clone(),
                }),
                ..Default::default()
            },
        };
        for objects in [
            vec![CastResource::Bargain(id(&base, 2))],
            vec![CastResource::Bargain(id(&base, 4))],
            vec![CastResource::Bargain(id(&base, 3)); 2],
        ] {
            assert!(
                base.advance(GameInput {
                    basis: base.basis().clone(),
                    actor: a,
                    command: cast(objects)
                })
                .is_err()
            );
        }
        for paid in [false, true] {
            let resources = if paid {
                vec![CastResource::Bargain(id(&base, 3))]
            } else {
                vec![]
            };
            let game = advance(&base, a, cast(resources));
            assert_eq!(
                game.execution()
                    .window()
                    .entries()
                    .last()
                    .unwrap()
                    .bargained(),
                paid
            );
            let zone = game
                .execution()
                .window()
                .cards()
                .get(id(&game, 3))
                .unwrap()
                .zone();
            assert_eq!(
                zone,
                if paid {
                    ObjectZone::Graveyard(b)
                } else {
                    ObjectZone::Battlefield
                }
            );
            let game = advance(&advance(&game, a, GameCommand::Pass), b, GameCommand::Pass);
            let target = game
                .execution()
                .current_characteristics(id(&game, 2))
                .unwrap()
                .unwrap();
            let expected_power = match (name, paid) {
                ("Candy Grapple", true) => 3,
                ("Candy Grapple", false) => 5,
                _ => 10,
            };
            assert_eq!(
                target.power_toughness.as_ref().unwrap().derived.power,
                expected_power.into()
            );
            for keyword in [KeywordAbility::Flying, KeywordAbility::Lifelink] {
                assert_eq!(
                    target.has_keyword(keyword),
                    name == "Archon's Glory" && paid
                );
            }
        }
    }
}
