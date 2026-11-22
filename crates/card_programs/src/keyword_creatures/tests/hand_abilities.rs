use super::*;
use spellwake_engine::{
    activated::SourceCost,
    effects::LibraryTopReply,
    game::{DecisionReply, RuleFrame},
    land::LandType,
};

#[test]
#[ignore = "requires the saved Oracle snapshot and matching creature vocabulary"]
fn actual_hand_keyword_bindings_pay_the_printed_cost_and_deliver_the_printed_reward() {
    let cards = input("SPELLWAKE_CATALOGUE_INPUT");
    let vocab = vocabulary::load_vocabulary(&input("SPELLWAKE_CREATURE_VOCABULARY_INPUT")).unwrap();
    let inventory =
        spellwake_content::snapshot::load(&input("SPELLWAKE_CATALOGUE_METADATA"), &cards).unwrap();
    let registry = Registry::from_keyword_gameplay_inventory(&inventory, &vocab).unwrap();
    let mut verified = BTreeSet::new();
    for row in inventory
        .cards
        .iter()
        .filter(|c| c.required_by_base_formats)
    {
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        let (a, b) = (SeatId(0), SeatId(1));
        let Ok(seeded) = registry.seed(&id, CardId(1), a, ObjectDestination::Hand) else {
            continue;
        };
        let printed = row.faces[0].oracle_text.as_deref().unwrap_or("");
        let rules = without_reminder(printed).unwrap();
        let lines: Vec<_> = rules
            .split([',', '\n'])
            .map(str::trim)
            .filter(|s| {
                s.split_once(" {").is_some_and(|(word, _)| {
                    word.eq_ignore_ascii_case("transmute")
                        || word.to_ascii_lowercase().ends_with("cycling")
                })
            })
            .collect();
        let abilities: Vec<_> = seeded
            .program
            .activated
            .iter()
            .filter(|ability| {
                matches!(
                    ability.source_cost,
                    SourceCost::HandDiscardSelf(
                        spellwake_engine::activated::HandDiscardKeyword::Cycling
                            | spellwake_engine::activated::HandDiscardKeyword::Transmute,
                    )
                )
            })
            .collect();
        assert_eq!(abilities.len(), lines.len(), "{}", row.name);
        for (ability, line) in abilities.into_iter().zip(lines) {
            let (word, cost) = line.split_once(" {").unwrap();
            let price = super::super::fixed_cost(&format!("{{{cost}")).unwrap();
            assert_eq!(price, ability.mana_cost, "{}", row.name);
            let mut pool = price.required.clone();
            if price.generic > 0.into() {
                let generic = pool.entry(ManaType::Colourless).or_insert_with(|| 0.into());
                *generic = generic.plus(&price.generic);
            }
            let mana = pool
                .iter()
                .map(|(&mana_type, amount)| FloatingMana {
                    player: a,
                    mana_type,
                    amount: amount.clone(),
                })
                .collect();
            let payment = Some(ManaPayment {
                from_lots: Default::default(),
                spend: pool
                    .into_iter()
                    .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
                    .collect(),
            });
            let lower = word.to_ascii_lowercase();
            let transmute = lower == "transmute";
            let mut prize = Program::artifact(Some(ManaCost::free()), vec![]);
            if transmute {
                prize =
                    Program::artifact(Some(ManaCost::generic(seeded.program.mana_value())), vec![]);
            } else if let Some(qualifier) = lower.strip_suffix("cycling").filter(|q| !q.is_empty())
            {
                if qualifier == "basic land" || qualifier == "land" {
                    prize = Program::land(BTreeSet::new(), vec![]);
                    prize.supertypes.insert(Supertype::Basic);
                } else if let Some(kind) = LandType::ALL
                    .into_iter()
                    .find(|k| k.wire_label().eq_ignore_ascii_case(qualifier))
                {
                    prize = Program::land(BTreeSet::from([kind]), vec![]);
                } else {
                    let kind = vocab
                        .members()
                        .iter()
                        .find(|k| k.eq_ignore_ascii_case(qualifier))
                        .unwrap();
                    prize = Program::creature(
                        None,
                        1.into(),
                        1.into(),
                        BTreeSet::from([vocab.resolve_label(kind).unwrap()]),
                    );
                }
            }
            let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
            let context = GameContext::new(
                KEYWORD_GAMEPLAY_PROFILE.into(),
                vocab.rules_revision().into(),
                TEACHING_SOURCE_SHA256.into(),
                "keyword-gameplay-registry13".into(),
                "hand-abilities".into(),
                vocab.id(),
                names.id(),
            )
            .unwrap();
            let seed = GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![
                    seeded.clone(),
                    SeedCard {
                        card: CardId(2),
                        owner: a,
                        destination: ObjectDestination::LibraryBottom,
                        program: prize,
                    },
                ],
                vec![],
                mana,
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocab.clone(),
                names,
            );
            let mut state = GameState::initialize_scheduled(
                GameId(13),
                context,
                seed,
                TurnStart {
                    number: 1,
                    step: Step::PrecombatMain,
                },
            )
            .unwrap()
            .into_state();
            let object = |s: &GameState, n| {
                s.execution()
                    .window()
                    .cards()
                    .current_identity(CardId(n))
                    .unwrap()
            };
            let source = object(&state, 1);
            let command = if transmute {
                GameCommand::Transmute {
                    source,
                    ability: ability.id,
                    payment,
                }
            } else {
                GameCommand::Cycle {
                    source,
                    ability: ability.id,
                    payment,
                }
            };
            state = advance(&state, a, command);
            assert_eq!(
                state
                    .execution()
                    .window()
                    .cards()
                    .get(object(&state, 1))
                    .unwrap()
                    .zone(),
                ObjectZone::Graveyard(a)
            );
            for seat in [a, b] {
                state = advance(&state, seat, GameCommand::Pass);
            }
            if lower != "cycling" {
                let pending = state.pending_decision().unwrap();
                let RuleFrame::LibraryTop(frame) = pending.frame() else {
                    panic!("{} expected a search", row.name)
                };
                let prize = object(&state, 2);
                assert_eq!(frame.eligible_results(), &[prize], "{}", row.name);
                state = advance(
                    &state,
                    a,
                    GameCommand::Reply {
                        decision: pending.id(),
                        choice: DecisionReply::LibraryTop(LibraryTopReply::Search {
                            selected: Some(prize),
                        }),
                    },
                );
                assert!(state.search_reveal().is_some());
            }
            assert_eq!(
                state
                    .execution()
                    .window()
                    .cards()
                    .get(object(&state, 2))
                    .unwrap()
                    .zone(),
                ObjectZone::Hand(a),
                "{}",
                row.name
            );
            verified.insert(row.name.clone());
        }
    }
    for name in [
        "Drift of Phantasms",
        "Striped Riverwinder",
        "Shoreline Ranger",
    ] {
        assert!(verified.contains(name), "{name}");
    }
    eprintln!(
        "{} verified hand keyword cards: {}",
        verified.len(),
        verified.into_iter().collect::<Vec<_>>().join(", ")
    );
}

#[test]
#[ignore = "requires the saved Oracle snapshot and matching creature vocabulary"]
fn real_reinforce_and_unearth_bindings_complete_their_activated_abilities() {
    let (inventory, vocab, registry) = crate::keyword_creatures::tests::extended_catalogue();
    let mut tested = BTreeSet::new();
    for card in inventory
        .cards
        .iter()
        .filter(|c| c.required_by_base_formats)
    {
        let id = OracleId::try_from(card.oracle_id.clone().unwrap()).unwrap();
        let Some(bound) = registry.resolve(&id) else {
            continue;
        };
        let (a, b) = (SeatId(0), SeatId(1));
        for ability in &bound.instruction.activated {
            let unearth = match ability.source_cost {
                SourceCost::GraveyardSelf { unearth: true } => true,
                SourceCost::HandDiscardSelf(
                    spellwake_engine::activated::HandDiscardKeyword::Reinforce,
                ) => false,
                _ => continue,
            };
            let raw = without_reminder(card.faces[0].oracle_text.as_deref().unwrap()).unwrap();
            let label = raw
                .split([',', '\n'])
                .map(str::trim)
                .find(|s| {
                    s.to_ascii_lowercase().starts_with(if unearth {
                        "unearth "
                    } else {
                        "reinforce "
                    })
                })
                .unwrap();
            let (printed_number, price) = if unearth {
                (None, label.split_once(' ').unwrap().1)
            } else {
                let (left, price) = label.split_once('—').unwrap();
                (
                    Some(left.split_once(' ').unwrap().1.parse::<i64>().unwrap()),
                    price,
                )
            };
            let cost = super::super::fixed_cost(price.trim()).unwrap();
            assert_eq!(ability.mana_cost, cost, "{}", card.name);
            let mut pool = cost.required.clone();
            if cost.generic > 0.into() {
                let c = pool.entry(ManaType::Colourless).or_insert_with(|| 0.into());
                *c = c.plus(&cost.generic);
            }
            let mana = pool
                .iter()
                .map(|(&mana_type, amount)| FloatingMana {
                    player: a,
                    mana_type,
                    amount: amount.clone(),
                })
                .collect();
            let payment = Some(ManaPayment {
                from_lots: Default::default(),
                spend: pool
                    .into_iter()
                    .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
                    .collect(),
            });
            let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
            let context = GameContext::new(
                KEYWORD_GAMEPLAY_V3_PROFILE.into(),
                vocab.rules_revision().into(),
                TEACHING_SOURCE_SHA256.into(),
                "keyword-gameplay-registry14".into(),
                "extended-abilities".into(),
                vocab.id(),
                names.id(),
            )
            .unwrap();
            let seeded = registry
                .seed(
                    &id,
                    CardId(1),
                    a,
                    if unearth {
                        ObjectDestination::Graveyard
                    } else {
                        ObjectDestination::Hand
                    },
                )
                .unwrap();
            let target = SeedCard {
                card: CardId(2),
                owner: b,
                destination: ObjectDestination::Battlefield { controller: b },
                program: Program::creature(None, 1.into(), 20.into(), BTreeSet::new()),
            };
            let seed = GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![seeded, target],
                vec![CardId(2)],
                mana,
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocab.clone(),
                names,
            );
            let mut state = GameState::initialize_scheduled(
                GameId(14),
                context,
                seed,
                TurnStart {
                    number: 1,
                    step: Step::PostcombatMain,
                },
            )
            .unwrap()
            .into_state();
            let object = |s: &GameState, n| {
                s.execution()
                    .window()
                    .cards()
                    .current_identity(CardId(n))
                    .unwrap()
            };
            let source = object(&state, 1);
            let target = object(&state, 2);
            let command = if unearth {
                GameCommand::ActivateUntargetedAbility {
                    source,
                    ability: ability.id,
                    payment,
                }
            } else {
                GameCommand::ActivateAbility {
                    source,
                    ability: ability.id,
                    target: spellwake_engine::targets::TargetRef::Object(target),
                    payment,
                }
            };
            state = advance(&state, a, command);
            for seat in [a, b] {
                state = advance(&state, seat, GameCommand::Pass);
            }
            if unearth {
                assert!(
                    state
                        .execution()
                        .window()
                        .cards()
                        .get(object(&state, 1))
                        .unwrap()
                        .is_unearthed()
                );
                assert!(
                    state
                        .execution()
                        .current_characteristics(object(&state, 1))
                        .unwrap()
                        .unwrap()
                        .has_keyword(KeywordAbility::Haste)
                );
                for seat in [a, b, a, b] {
                    state = advance(&state, seat, GameCommand::Pass);
                }
                assert_eq!(
                    state
                        .execution()
                        .window()
                        .cards()
                        .get(object(&state, 1))
                        .unwrap()
                        .zone(),
                    ObjectZone::Exile,
                    "{}",
                    card.name
                );
            } else {
                assert_eq!(
                    state
                        .execution()
                        .power_toughness_counters(target)
                        .unwrap()
                        .plus_one,
                    printed_number.unwrap().into(),
                    "{}",
                    card.name
                );
                assert_eq!(
                    state
                        .execution()
                        .window()
                        .cards()
                        .get(object(&state, 1))
                        .unwrap()
                        .zone(),
                    ObjectZone::Graveyard(a)
                );
            }
            tested.insert(card.name.clone());
        }
    }
    for name in ["Burrenton Bombardier", "Dregscape Zombie"] {
        assert!(tested.contains(name), "{name}");
    }
    eprintln!(
        "{} verified Reinforce/Unearth cards: {}",
        tested.len(),
        tested.into_iter().collect::<Vec<_>>().join(", ")
    );
}
