use super::*;
use spellwake_engine::{
    characteristics::KeywordAbility, stack_window::StackKind, targets::TargetRef,
};
#[test]
#[ignore = "requires the dated Oracle catalogue and matching vocabulary"]
fn swiftwing_assailant_walking_sarcophagus_and_streaking_oilgorger_reach_max_speed_after_three_turns()
 {
    let (inventory, vocab, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let seed = |name: &str, n, owner, destination| {
        let row = inventory.cards.iter().find(|r| r.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry
            .seed(&id, CardId(n), owner, destination)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"))
    };
    let mut cards = vec![];
    let mut stats = vec![];
    for (n, name) in [
        (1, "Swiftwing Assailant"),
        (2, "Walking Sarcophagus"),
        (3, "Streaking Oilgorger"),
    ] {
        let row = inventory.cards.iter().find(|r| r.name == name).unwrap();
        stats.push((
            number(row.faces[0].power.as_deref()).unwrap(),
            number(row.faces[0].toughness.as_deref()).unwrap(),
        ));
        cards.push(seed(
            name,
            n,
            a,
            ObjectDestination::Battlefield { controller: a },
        ));
    }
    for n in 4..10 {
        cards.push(SeedCard {
            card: CardId(n),
            owner: a,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SINGLE_PLAYER,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                            ),
                            spellwake_engine::program::ValueExpr::literal(1),
                        ),
                    ],
                ),
            ),
        });
    }
    for n in 10..30 {
        cards.push(seed(
            "Raging Goblin",
            n,
            if n % 2 == 0 { b } else { a },
            ObjectDestination::LibraryBottom,
        ));
    }
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocab.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "speed".into(),
        "source".into(),
        vocab.id(),
        names.id(),
    )
    .unwrap();
    let mut game = GameState::initialize_scheduled(
        GameId(72),
        context,
        GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(1), CardId(2), CardId(3)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            names,
        ),
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let id = |g: &GameState, n| {
        g.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    };
    let pass = |g: &GameState| {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) = g.execution().window().phase()
        else {
            panic!("priority")
        };
        advance(g, holder, GameCommand::Pass)
    };
    let resolve = |g: &GameState| pass(&pass(g));
    let ping = |g: &GameState, n| {
        advance(
            g,
            a,
            GameCommand::Cast {
                spell: id(g, n),
                target: TargetRef::Player(b),
            },
        )
    };
    let verify = |g: &GameState, speed: u8| {
        assert_eq!(g.execution().speed(a), speed);
        assert_eq!(g.execution().speed(b), 0);
        for (index, (base_power, base_toughness)) in stats.iter().copied().enumerate() {
            let current = g
                .execution()
                .current_characteristics(id(g, index as u64 + 1))
                .unwrap()
                .unwrap();
            let max = speed == 4;
            if index == 0 {
                assert_eq!(current.has_keyword(KeywordAbility::Vigilance), max);
            }
            if index == 2 {
                assert_eq!(current.has_keyword(KeywordAbility::Lifelink), max);
            }
            let pt = current.power_toughness.unwrap().derived;
            let bonus = if max {
                [(0, 1), (1, 2), (0, 0)][index]
            } else {
                (0, 0)
            };
            assert_eq!(
                (pt.power, pt.toughness),
                (
                    (base_power + bonus.0).into(),
                    (base_toughness + bonus.1).into()
                )
            );
        }
    };
    verify(&game, 1);
    for (i, turn) in [(0, 1_u64), (1, 3), (2, 5)] {
        game = super::exile_hand::until(game, |s| {
            s.turn().is_some_and(|t| {
                t.position().number == turn && t.position().step == Step::PrecombatMain
            })
        });
        game = resolve(&ping(&game, 4 + i));
        assert!(
            matches!(game.execution().window().entries(),[entry] if entry.kind()==&StackKind::Speed&&entry.controller()==a)
        );
        if i == 0 {
            game = resolve(&ping(&game, 7));
            assert_eq!(game.execution().window().entries().len(), 1);
        }
        game = resolve(&game);
        verify(&game, 2 + i as u8);
        if i == 0 {
            game = super::exile_hand::until(game, |s| {
                s.turn().is_some_and(|t| {
                    t.position().number == 2 && t.position().step == Step::PrecombatMain
                })
            });
            let priority = pass(&game);
            game = resolve(&ping(&priority, 9));
            assert!(game.execution().window().entries().is_empty());
            verify(&game, 2);
        }
    }
    game = resolve(&ping(&game, 8));
    assert!(game.execution().window().entries().is_empty());
    verify(&game, 4);
}
