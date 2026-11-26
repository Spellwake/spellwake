use super::*;
use spellwake_engine::{
    activated::ActivatedAbilityId,
    characteristics::{
        KeywordAbility, PowerToughness, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    },
    effects::LibraryTopReply,
    game::{DecisionReply, RuleFrame},
    targets::TargetRef,
};
#[test]
#[ignore = "requires the dated catalogue and matching vocabulary"]
fn galvanizing_sawship_and_wurmwall_sweeper_use_the_contributors_last_battlefield_power() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let seed = |name: &str, n, owner, destination| {
        let row = inventory.cards.iter().find(|r| r.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry
            .seed(&id, CardId(n), owner, destination)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"))
    };
    for (name, red, generic, power, toughness) in [
        ("Galvanizing Sawship", 1_i64, 5_i64, 6_i64, 5_i64),
        ("Wurmwall Sweeper", 0, 2, 2, 2),
    ] {
        let mut cards = vec![
            seed(name, 1, a, ObjectDestination::Hand),
            seed("Savannah Lions", 2, a, ObjectDestination::Hand),
        ];
        for (n, op) in [
            (
                3,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(PowerToughness {
                        power: 2.into(),
                        toughness: 0.into(),
                    })]),
                    ResolvedDuration::UntilEndOfTurn,
                ),
            ),
            (
                4,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand(),
            ),
        ] {
            cards.push(SeedCard {
                card: CardId(n),
                owner: a,
                destination: ObjectDestination::Hand,
                program: Program::instant(
                    ManaCost::free(),
                    ExecutableBody::new(BodyTarget::SingleCreature, vec![op]),
                ),
            });
        }
        for n in 5..13 {
            cards.push(seed(
                "Raging Goblin",
                n,
                if n % 2 == 0 { b } else { a },
                ObjectDestination::LibraryBottom,
            ));
        }
        let context = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "station".into(),
            name.into(),
            vocabulary.id(),
            names.id(),
        )
        .unwrap();
        let mut game = GameState::initialize_scheduled(
            GameId(71),
            context,
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                cards,
                vec![],
                vec![
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Red,
                        amount: 1.into(),
                    },
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::White,
                        amount: 1.into(),
                    },
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Colourless,
                        amount: generic.into(),
                    },
                ],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                names.clone(),
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
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                g.execution().window().phase()
            else {
                panic!("priority")
            };
            advance(g, holder, GameCommand::Pass)
        };
        let resolve = |g: &GameState| pass(&pass(g));
        game = advance(
            &game,
            a,
            GameCommand::CastUntargeted {
                spell: id(&game, 2),
                payment: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend: vec![ManaSpend {
                        mana_type: ManaType::White,
                        amount: 1.into(),
                    }],
                }),
            },
        );
        game = resolve(&game);
        let cost = [(ManaType::Red, red), (ManaType::Colourless, generic)]
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .map(|(mana_type, n)| ManaSpend {
                mana_type,
                amount: n.into(),
            })
            .collect();
        game = advance(
            &game,
            a,
            GameCommand::CastUntargeted {
                spell: id(&game, 1),
                payment: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend: cost,
                }),
            },
        );
        assert!(
            game.execution()
                .current_characteristics(id(&game, 1))
                .unwrap()
                .unwrap()
                .power_toughness
                .is_none()
        );
        game = resolve(&game);
        if name == "Wurmwall Sweeper" {
            game = resolve(&game);
            let frame = game.pending_decision().unwrap();
            let RuleFrame::LibraryTop(domain) = frame.frame() else {
                panic!("surveil")
            };
            let looked = domain.domain_top_first();
            assert_eq!(looked.len(), 2);
            game = advance(
                &game,
                a,
                GameCommand::Reply {
                    decision: frame.id(),
                    choice: DecisionReply::LibraryTop(LibraryTopReply::Surveil {
                        top_first: vec![looked[0]],
                        to_graveyard: vec![looked[1]],
                    }),
                },
            );
            assert_eq!(game.execution().window().cards().library(a).len(), 3);
        }
        let ship = id(&game, 1);
        let contributor = id(&game, 2);
        let activate = |creatures| GameCommand::ActivateWithCreatures {
            source: ship,
            ability: ActivatedAbilityId(1),
            creatures,
        };
        for invalid in [vec![], vec![ship], vec![contributor, contributor]] {
            assert!(
                game.advance(GameInput {
                    basis: game.basis().clone(),
                    actor: a,
                    command: activate(invalid)
                })
                .is_err()
            );
        }
        game = advance(&game, a, activate(vec![contributor]));
        assert_eq!(
            game.execution()
                .window()
                .cards()
                .get(contributor)
                .unwrap()
                .tapped(),
            Some(true)
        );
        assert!(
            !game
                .execution()
                .current_characteristics(ship)
                .unwrap()
                .unwrap()
                .is_creature()
        );
        // Let the buff resolve while Station remains on the stack, then remove the creature.
        for n in [3, 4] {
            game = advance(
                &game,
                a,
                GameCommand::Cast {
                    spell: id(&game, n),
                    target: TargetRef::Object(contributor),
                },
            );
            game = resolve(&game);
        }
        assert_eq!(
            game.execution()
                .window()
                .cards()
                .get(id(&game, 2))
                .unwrap()
                .zone(),
            ObjectZone::Hand(a)
        );
        game = resolve(&game);
        assert_eq!(
            game.execution().charge_counters().get(&ship),
            Some(&4.into())
        );
        let active = game
            .execution()
            .current_characteristics(ship)
            .unwrap()
            .unwrap();
        assert!(active.is_creature() && active.has_keyword(KeywordAbility::Flying));
        assert_eq!(
            active.has_keyword(KeywordAbility::Haste),
            name == "Galvanizing Sawship"
        );
        let pt = active.power_toughness.unwrap().derived;
        assert_eq!((pt.power, pt.toughness), (power.into(), toughness.into()));
    }
}
