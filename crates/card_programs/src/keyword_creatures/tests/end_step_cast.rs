use super::exile_hand::until;
use super::*;
use spellwake_engine::{
    characteristics::KeywordAbility,
    effects::EffectError,
    game::{DecisionReply, GameError},
    mana::{CastingPayment, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput},
    targets::TargetRef,
};
#[test]
#[ignore = "requires the dated catalogue and matching vocabulary"]
fn aleshas_vanguard_caldaia_strongarm_and_bygone_colossus_resolve_their_paid_end_step_abilities() {
    let (catalogue, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let rules = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let seed_card = |name: &str, n, owner, destination| {
        let row = catalogue.cards.iter().find(|r| r.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry
            .seed(&id, CardId(n), owner, destination)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"))
    };
    for (name, price) in [
        (
            "Alesha's Vanguard",
            vec![(ManaType::Colourless, 2), (ManaType::Black, 1)],
        ),
        (
            "Caldaia Strongarm",
            vec![(ManaType::Colourless, 3), (ManaType::Green, 1)],
        ),
        ("Bygone Colossus", vec![(ManaType::Colourless, 3)]),
    ] {
        let mut cards = vec![seed_card(name, 1, a, ObjectDestination::Hand)];
        for n in 3..17 {
            cards.push(seed_card(
                "Raging Goblin",
                n,
                if n % 2 == 0 { a } else { b },
                ObjectDestination::LibraryBottom,
            ));
        }
        let mut mana_source = Program::artifact(None, vec![]);
        mana_source.mana.push(ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed([(ManaType::Colourless, 9.into())].into()),
        });
        cards.push(SeedCard {
            card: CardId(2),
            owner: a,
            destination: ObjectDestination::Battlefield { controller: a },
            program: mana_source,
        });
        let init = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "end-step".into(),
            name.into(),
            vocabulary.id(),
            rules.id(),
        )
        .unwrap();
        let floating = ManaType::ALL
            .into_iter()
            .map(|mana_type| FloatingMana {
                player: a,
                mana_type,
                amount: 12.into(),
            })
            .collect();
        let initial = GameState::initialize_scheduled(
            GameId(22),
            init,
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                cards,
                vec![CardId(2)],
                floating,
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
        let zone = |s: &GameState| {
            s.execution()
                .window()
                .cards()
                .get(obj(s, 1))
                .unwrap()
                .zone()
        };
        let cast = |s: &GameState, alt, price: &[(ManaType, i64)]| GameCommand::CastWithResources {
            spell: obj(s, 1),
            target: None,
            payment: CastingPayment {
                use_alternative: alt,
                mana: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend: price
                        .iter()
                        .map(|&(mana_type, n)| ManaSpend {
                            mana_type,
                            amount: n.into(),
                        })
                        .collect(),
                }),
                ..Default::default()
            },
        };
        let casted = advance(&initial, a, cast(&initial, true, &price));
        let mut entered = until(casted, |s| zone(s) == ObjectZone::Battlefield);
        let creature = obj(&entered, 1);
        assert_eq!(
            entered
                .execution()
                .current_characteristics(creature)
                .unwrap()
                .unwrap()
                .has_keyword(KeywordAbility::Haste),
            name != "Bygone Colossus"
        );
        if name == "Caldaia Strongarm" {
            if let Some(choice) = entered.pending_decision() {
                entered = advance(
                    &entered,
                    a,
                    GameCommand::Reply {
                        decision: choice.id(),
                        choice: DecisionReply::TriggerTarget(TargetRef::Object(creature)),
                    },
                );
            }
            entered = until(entered, |s| s.execution().window().entries().is_empty());
            assert_eq!(
                entered
                    .execution()
                    .power_toughness_counters(creature)
                    .unwrap()
                    .plus_one,
                2.into()
            );
        }
        let end = until(entered, |s| {
            s.turn().unwrap().position().step == Step::End
                && !s.execution().window().entries().is_empty()
        });
        assert_eq!(end.execution().window().entries().len(), 1);
        let departed = until(end, |s| {
            zone(s) != ObjectZone::Battlefield && s.execution().window().entries().is_empty()
        });
        match name {
            "Alesha's Vanguard" => assert_eq!(zone(&departed), ObjectZone::Hand(a)),
            "Caldaia Strongarm" => {
                assert_eq!(zone(&departed), ObjectZone::Graveyard(a));
                assert_eq!(departed.execution().window().cards().library(a).len(), 6);
            }
            _ => {
                assert_eq!(zone(&departed), ObjectZone::Exile);
                assert_eq!(departed.execution().warped_on(obj(&departed, 1)), Some(1));
                assert_eq!(
                    departed
                        .advance(GameInput {
                            basis: departed.basis().clone(),
                            actor: a,
                            command: cast(&departed, false, &[(ManaType::Colourless, 9)])
                        })
                        .unwrap_err(),
                    GameError::Effect(EffectError::InvalidCastResource)
                );
                let later = until(departed, |s| {
                    s.turn().unwrap().position().number == 3
                        && s.turn().unwrap().position().step == Step::PrecombatMain
                });
                assert_eq!(
                    later
                        .advance(GameInput {
                            basis: later.basis().clone(),
                            actor: a,
                            command: cast(&later, true, &price)
                        })
                        .unwrap_err(),
                    GameError::Effect(EffectError::InvalidCastResource)
                );
                let funded = advance(
                    &later,
                    a,
                    GameCommand::ActivateMana {
                        source: obj(&later, 2),
                        ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
                    },
                );
                let casted = advance(
                    &funded,
                    a,
                    cast(&funded, false, &[(ManaType::Colourless, 9)]),
                );
                let stable = until(casted, |s| s.turn().unwrap().position().step == Step::End);
                assert_eq!(zone(&stable), ObjectZone::Battlefield);
                assert!(stable.execution().window().entries().is_empty());
            }
        }
    }
}
