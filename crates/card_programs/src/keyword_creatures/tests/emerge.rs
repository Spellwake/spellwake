use super::*;
use spellwake_engine::mana::{CastResource, CastingPayment};

#[test]
#[ignore = "requires the dated catalogue and source-qualified creature vocabulary"]
fn wretched_gryff_and_it_of_the_horrid_swarm_trigger_before_the_emerged_spell_resolves() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let source_id = |name: &str| {
        OracleId::try_from(
            inventory
                .cards
                .iter()
                .find(|c| c.name == name)
                .unwrap()
                .oracle_id
                .clone()
                .unwrap(),
        )
        .unwrap()
    };
    for (name, color, generic) in [
        ("Wretched Gryff", ManaType::Blue, 2),
        ("It of the Horrid Swarm", ManaType::Green, 3),
    ] {
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "emerge".into(),
            "source-games".into(),
            vocabulary.id(),
            names.id(),
        )
        .unwrap();
        let cards = vec![
            registry
                .seed(&source_id(name), CardId(1), a, ObjectDestination::Hand)
                .unwrap(),
            SeedCard {
                card: CardId(2),
                owner: b,
                destination: ObjectDestination::Battlefield { controller: a },
                program: Program::creature(
                    Some(ManaCost::generic(3.into())),
                    2.into(),
                    2.into(),
                    BTreeSet::new(),
                ),
            },
            registry
                .seed(
                    &source_id("Island"),
                    CardId(3),
                    a,
                    ObjectDestination::LibraryBottom,
                )
                .unwrap(),
        ];
        let spend = vec![
            ManaSpend {
                mana_type: color,
                amount: 1.into(),
            },
            ManaSpend {
                mana_type: ManaType::Colourless,
                amount: generic.into(),
            },
        ];
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
            vec![CardId(2)],
            mana,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary.clone(),
            names,
        );
        let game = GameState::initialize_scheduled(
            GameId(24),
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
        let cast = |objects, alternative| GameCommand::CastWithResources {
            spell: id(&game, 1),
            target: None,
            payment: CastingPayment {
                objects,
                use_alternative: alternative,
                mana: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend: spend.clone(),
                }),
                ..Default::default()
            },
        };
        let subject = CastResource::Emerge(id(&game, 2));
        for rejected in [
            cast(vec![], true),
            cast(vec![subject], false),
            cast(vec![subject; 2], true),
        ] {
            assert!(
                game.advance(GameInput {
                    basis: game.basis().clone(),
                    actor: a,
                    command: rejected
                })
                .is_err()
            );
        }
        let resolve =
            |g: &GameState| advance(&advance(g, a, GameCommand::Pass), b, GameCommand::Pass);
        let game = advance(&game, a, cast(vec![subject], true));
        assert_eq!(game.execution().window().entries().len(), 2);
        assert_eq!(
            game.execution()
                .window()
                .cards()
                .get(id(&game, 2))
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(b)
        );
        let mut game = resolve(&game);
        if let Some(pending) = game.pending_decision() {
            let spellwake_engine::game::RuleFrame::TimestampOrder(frame) = pending.frame() else {
                panic!("expected simultaneous Insect entry timestamps")
            };
            game = advance(
                &game,
                pending.chooser(),
                GameCommand::Reply {
                    decision: pending.id(),
                    choice: spellwake_engine::game::DecisionReply::TimestampOrder(
                        frame.members().iter().map(|m| m.member).collect(),
                    ),
                },
            );
        }

        assert_eq!(
            game.execution()
                .window()
                .cards()
                .get(id(&game, 1))
                .unwrap()
                .zone(),
            ObjectZone::Stack
        );
        if name == "Wretched Gryff" {
            assert_eq!(
                game.execution()
                    .window()
                    .cards()
                    .get(id(&game, 3))
                    .unwrap()
                    .zone(),
                ObjectZone::Hand(a)
            );
        } else {
            assert_eq!(
                game.execution()
                    .window()
                    .cards()
                    .iter()
                    .filter(|o| o.zone() == ObjectZone::Battlefield && o.is_token())
                    .count(),
                2
            );
        }
        let game = resolve(&game);
        assert_eq!(
            game.execution()
                .window()
                .cards()
                .get(id(&game, 1))
                .unwrap()
                .zone(),
            ObjectZone::Battlefield
        );
    }
}
