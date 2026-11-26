use super::*;
use spellwake_engine::{
    mana::CastingPayment,
    program::{BodyTarget, ExecutableBody},
    targets::TargetRef,
};

#[test]
#[ignore = "requires the dated catalogue and source-qualified creature vocabulary"]
fn wash_away_uses_the_zone_of_casting_and_removes_its_restriction_when_cleaved() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let oracle = OracleId::try_from(
        inventory
            .cards
            .iter()
            .find(|c| c.name == "Wash Away")
            .unwrap()
            .oracle_id
            .clone()
            .unwrap(),
    )
    .unwrap();
    let (a, b) = (SeatId(0), SeatId(1));
    for from_hand in [true, false] {
        let mut test_spell = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ),
                ],
            ),
        );
        test_spell.flashback = Some(ManaCost::free());
        let seeds = vec![
            registry
                .seed(&oracle, CardId(1), b, ObjectDestination::Hand)
                .unwrap(),
            SeedCard {
                card: CardId(2),
                owner: a,
                destination: if from_hand {
                    ObjectDestination::Hand
                } else {
                    ObjectDestination::Graveyard
                },
                program: test_spell,
            },
        ];
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "cleave".into(),
            "source-games".into(),
            vocabulary.id(),
            names.id(),
        )
        .unwrap();
        let pool = vec![
            FloatingMana {
                player: b,
                mana_type: ManaType::Blue,
                amount: 2.into(),
            },
            FloatingMana {
                player: b,
                mana_type: ManaType::Colourless,
                amount: 1.into(),
            },
        ];
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            seeds,
            vec![],
            pool,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary.clone(),
            names,
        );
        let mut game = GameState::initialize_scheduled(
            GameId(25),
            context,
            seed,
            TurnStart {
                number: 1,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let object = |g: &GameState, n| {
            g.execution()
                .window()
                .cards()
                .current_identity(CardId(n))
                .unwrap()
        };
        game = advance(
            &game,
            a,
            GameCommand::CastUntargeted {
                spell: object(&game, 2),
                payment: None,
            },
        );
        game = advance(&game, a, GameCommand::Pass);
        let target = TargetRef::Stack(game.execution().window().entries()[0].id());
        for cleave in [false, true] {
            let mut mana = vec![ManaSpend {
                mana_type: ManaType::Blue,
                amount: if cleave { 2.into() } else { 1.into() },
            }];
            if cleave {
                mana.push(ManaSpend {
                    mana_type: ManaType::Colourless,
                    amount: 1.into(),
                });
            }
            let command = GameCommand::CastWithResources {
                spell: object(&game, 1),
                target: Some(target),
                payment: CastingPayment {
                    mana: Some(ManaPayment {
                        from_lots: Default::default(),
                        spend: mana,
                    }),
                    use_alternative: cleave,
                    ..Default::default()
                },
            };
            let attempt = game.advance(GameInput {
                basis: game.basis().clone(),
                actor: b,
                command,
            });
            if from_hand && !cleave {
                assert!(attempt.is_err());
                continue;
            }
            let paid = attempt.unwrap().into_state();
            let after = advance(&advance(&paid, b, GameCommand::Pass), a, GameCommand::Pass);
            assert!(after.execution().window().entries().is_empty());
            assert_eq!(
                after
                    .execution()
                    .window()
                    .cards()
                    .get(object(&after, 2))
                    .unwrap()
                    .zone(),
                if from_hand {
                    ObjectZone::Graveyard(a)
                } else {
                    ObjectZone::Exile
                }
            );
        }
    }
}
