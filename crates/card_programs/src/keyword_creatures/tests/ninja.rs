use super::*;
use spellwake_engine::{
    activated::SourceCost,
    combat::{AttackDeclaration, BlockDeclaration, BlockStatus},
    game::DecisionReply,
    mana::{CastResource, CastingPayment, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput},
};
#[test]
#[ignore = "requires the dated catalogue and matching vocabulary"]
fn mukotai_ambusher_oroku_saki_and_donatellos_technique_distinguish_ninjutsu_from_sneak() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let seed = |name: &str, n, owner, destination| {
        let row = inventory.cards.iter().find(|r| r.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry
            .seed(&id, CardId(n), owner, destination)
            .unwrap_or_else(|err| panic!("{name}: {err:?}"))
    };
    for (name, ninjutsu, sorcery) in [
        ("Mukotai Ambusher", true, false),
        ("Oroku Saki, Shredder Rising", false, false),
        ("Donatello's Technique", false, true),
    ] {
        let source = seed(name, 1, a, ObjectDestination::Hand);
        let ability = source
            .program
            .activated
            .iter()
            .find(|x| {
                x.source_cost
                    == SourceCost::Ninjutsu {
                        from_command: false,
                    }
            })
            .map(|x| x.id);
        let mut mana = Program::artifact(None, vec![]);
        mana.mana.push(ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed(
                [
                    (ManaType::Blue, 1.into()),
                    (ManaType::Black, 1.into()),
                    (ManaType::Colourless, 1.into()),
                ]
                .into(),
            ),
        });
        let mut cards = vec![
            source,
            seed(
                "Raging Goblin",
                2,
                b,
                ObjectDestination::Battlefield { controller: a },
            ),
            seed(
                "Raging Goblin",
                3,
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            seed(
                "Raging Goblin",
                4,
                b,
                ObjectDestination::Battlefield { controller: b },
            ),
            SeedCard {
                card: CardId(5),
                owner: a,
                destination: ObjectDestination::Battlefield { controller: a },
                program: mana,
            },
        ];
        for n in 6..18 {
            cards.push(seed(
                "Raging Goblin",
                n,
                if n % 2 == 0 { a } else { b },
                ObjectDestination::LibraryBottom,
            ));
        }
        let context = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "ninja".into(),
            name.into(),
            vocabulary.id(),
            names.id(),
        )
        .unwrap();
        let mut game = GameState::initialize_scheduled(
            GameId(24),
            context,
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                cards,
                vec![CardId(2), CardId(3), CardId(4), CardId(5)],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                names.clone(),
            ),
            TurnStart {
                number: 1,
                step: Step::DeclareAttackers,
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
        let source = id(&game, 1);
        let returning = id(&game, 2);
        let blocked = id(&game, 3);
        let blocker = id(&game, 4);
        let mana_source = id(&game, 5);
        let command = |attacker| {
            let spend = if sorcery {
                vec![(ManaType::Blue, 1)]
            } else {
                vec![(ManaType::Colourless, 1), (ManaType::Black, 1)]
            };
            let pay = ManaPayment {
                from_lots: Default::default(),
                spend: spend
                    .into_iter()
                    .map(|(mana_type, n)| ManaSpend {
                        mana_type,
                        amount: n.into(),
                    })
                    .collect(),
            };
            if ninjutsu {
                GameCommand::Ninjutsu {
                    source,
                    ability: ability.unwrap(),
                    attacker,
                    payment: Some(pay),
                }
            } else {
                GameCommand::CastWithResources {
                    spell: source,
                    target: None,
                    payment: CastingPayment {
                        use_alternative: true,
                        objects: vec![CastResource::Sneak(attacker)],
                        mana: Some(pay),
                        ..Default::default()
                    },
                }
            }
        };
        let reject = |g: &GameState, attacker| {
            assert!(
                g.advance(GameInput {
                    basis: g.basis().clone(),
                    actor: a,
                    command: command(attacker)
                })
                .is_err()
            )
        };
        let pass = |g: &GameState| {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                g.execution().window().phase()
            else {
                panic!("priority")
            };
            advance(g, holder, GameCommand::Pass)
        };
        let reply = |g: &GameState, choice| {
            let frame = g.pending_decision().unwrap();
            advance(
                g,
                frame.chooser(),
                GameCommand::Reply {
                    decision: frame.id(),
                    choice,
                },
            )
        };
        game = reply(
            &game,
            DecisionReply::DeclareAttackers(vec![
                AttackDeclaration {
                    attacker: returning,
                    defender: b,
                    defending_planeswalker: None,
                },
                AttackDeclaration {
                    attacker: blocked,
                    defender: b,
                    defending_planeswalker: None,
                },
            ]),
        );
        reject(&game, returning);
        game = pass(&pass(&game));
        game = reply(
            &game,
            DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                blocker,
                attacker: blocked,
            }]),
        );
        assert_eq!(
            game.execution()
                .combat()
                .unwrap()
                .attacker(blocked)
                .unwrap()
                .block_status(),
            BlockStatus::Blocked
        );
        game = advance(
            &game,
            a,
            GameCommand::ActivateMana {
                source: mana_source,
                ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
            },
        );
        reject(&game, blocked);
        game = advance(&game, a, command(returning));
        assert_eq!(
            game.execution()
                .window()
                .cards()
                .get(id(&game, 2))
                .unwrap()
                .zone(),
            ObjectZone::Hand(b)
        );
        assert!(
            game.execution()
                .combat()
                .unwrap()
                .attacker(returning)
                .is_none()
        );
        assert_eq!(game.execution().revealed_by_ninjutsu(source), ninjutsu);
        assert_eq!(
            game.execution()
                .window()
                .entries()
                .last()
                .unwrap()
                .attacking_defender(),
            Some(b)
        );
        game = pass(&pass(&game));
        assert!(!game.execution().revealed_by_ninjutsu(source));
        let after = game.execution().window().cards().get(id(&game, 1)).unwrap();
        if sorcery {
            assert_eq!(after.zone(), ObjectZone::Graveyard(a));
            assert_eq!(game.execution().window().cards().library(a).len(), 4);
        } else {
            assert_eq!(after.tapped(), Some(true));
            let entry = game
                .execution()
                .combat()
                .unwrap()
                .attacker(after.id())
                .unwrap();
            assert_eq!(
                (entry.defender(), entry.block_status()),
                (b, BlockStatus::Unblocked)
            );
            // Both blocked Goblins die. Only the replacement attacker hits B.
            game = super::exile_hand::until(game, |s| {
                s.turn().unwrap().position().step == Step::PostcombatMain
            });
            if ninjutsu {
                assert_eq!(game.execution().life_total(a).unwrap(), &23.into());
            } else {
                assert_eq!(game.execution().life_total(a).unwrap(), &19.into());
                assert_eq!(game.execution().window().cards().library(a).len(), 5);
            }
        }
    }
}
