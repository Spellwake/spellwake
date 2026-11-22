//! Real rebound spells are cast twice with independent targets and real upkeep triggers.
use super::*;
use spellwake_engine::{
    effects::ExileCastAbility,
    game::{DecisionReply, RuleFrame},
    mana::CastingPayment,
    program::ProgramKind,
    targets::TargetRef,
};

fn object(game: &GameState, id: u64) -> spellwake_engine::object_identity::ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(id))
        .unwrap()
}
fn zone(game: &GameState, id: u64) -> ObjectZone {
    game.execution()
        .window()
        .cards()
        .get(object(game, id))
        .unwrap()
        .zone()
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn real_rebound_spells_apply_their_bodies_twice_with_new_choices() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let mut verified = BTreeSet::new();
    for row in inventory
        .cards
        .iter()
        .filter(|r| r.required_by_base_formats)
    {
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        let Some(bound) = registry.resolve(&id) else {
            continue;
        };
        if !bound
            .instruction
            .printed_keywords
            .contains(&KeywordAbility::Rebound)
        {
            continue;
        }
        let face = &row.faces[0];
        let printed = without_reminder(face.oracle_text.as_deref().unwrap()).unwrap();
        assert!(printed.lines().any(|l| l.trim() == "Rebound"));
        let ProgramKind::Spell(spell) = &bound.instruction.kind else {
            panic!("rebound spell");
        };
        let printed_price = fixed_cost(face.mana_cost.as_deref().unwrap()).unwrap();
        assert_eq!(spell.cost, printed_price);
        let mut price = printed_price.required;
        if printed_price.generic.is_positive() {
            price.insert(ManaType::Colourless, printed_price.generic);
        }
        let funded = price
            .iter()
            .map(|(&mana_type, amount)| FloatingMana {
                player: a,
                mana_type,
                amount: amount.clone(),
            })
            .collect();
        let payment = ManaPayment {
            from_lots: Default::default(),
            spend: price
                .into_iter()
                .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
                .collect(),
        };
        let mut cards = vec![
            registry
                .seed(&id, CardId(1), a, ObjectDestination::Hand)
                .unwrap(),
        ];
        let mut add = |id, owner, destination, program| {
            cards.push(SeedCard {
                card: CardId(id),
                owner,
                destination,
                program,
            })
        };
        let vanilla = Program::creature(None, 1.into(), 10.into(), BTreeSet::new());
        for n in [2, 3] {
            add(
                n,
                b,
                ObjectDestination::Battlefield { controller: b },
                vanilla.clone(),
            );
        }
        for n in [4, 5] {
            add(n, a, ObjectDestination::Hand, vanilla.clone());
        }
        for n in 0..4 {
            for (offset, owner) in [(10, a), (20, b)] {
                add(
                    offset + n,
                    owner,
                    ObjectDestination::LibraryBottom,
                    vanilla.clone(),
                );
            }
        }
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "registry16".into(),
            "rebound".into(),
            vocabulary.id(),
            names.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(2), CardId(3)],
            funded,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary.clone(),
            names,
        );
        let state = GameState::initialize_scheduled(
            GameId(16),
            context,
            seed,
            TurnStart {
                number: 1,
                step: Step::PostcombatMain,
            },
        )
        .unwrap()
        .into_state();
        let has_target = printed.lines().any(|l| l.starts_with("Target creature"));
        let command = if has_target {
            GameCommand::CastPaid {
                spell: object(&state, 1),
                target: TargetRef::Object(object(&state, 2)),
                payment,
            }
        } else {
            GameCommand::CastUntargeted {
                spell: object(&state, 1),
                payment: Some(payment),
            }
        };
        let announced = advance(&state, a, command);
        let first = exile_hand::until(announced, |s| zone(s, 1) == ObjectZone::Exile);
        let check_effect = |state: &GameState, target: u64, count: usize| {
            let name = row.name.as_str();
            match name {
                "Into the Time Vortex" => {
                    // The optional cascade cast was declined. Each trigger returned the one
                    // exiled nonland card to the bottom of its controller's library.
                    assert_eq!(state.execution().window().cards().library(a).len(), 4);
                    let bottom = state
                        .execution()
                        .window()
                        .cards()
                        .library(a)
                        .last()
                        .copied()
                        .unwrap();
                    assert_eq!(bottom, object(state, 9 + count as u64));
                }
                "Faithless Salvaging" => {
                    let discarded = state
                        .execution()
                        .window()
                        .cards()
                        .iter()
                        .filter(|c| {
                            c.zone() == ObjectZone::Graveyard(a) && c.card_id() != Some(CardId(1))
                        })
                        .count();
                    assert_eq!(discarded, count);
                    assert_eq!(zone(state, 9 + count as u64), ObjectZone::Hand(a));
                }
                "Trumpeting Herd" | "Ojutai's Summons" => {
                    let flying = name == "Ojutai's Summons";
                    let tokens: Vec<_> = state
                        .execution()
                        .window()
                        .cards()
                        .iter()
                        .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
                        .collect();
                    assert_eq!(tokens.len(), count);
                    for token in tokens {
                        let sheet = state
                            .execution()
                            .current_characteristics(token.id())
                            .unwrap()
                            .unwrap();
                        let p = if flying { 2i64 } else { 3 };
                        assert_eq!(
                            sheet.current_colors,
                            BTreeSet::from([if flying { Color::Blue } else { Color::Green }])
                        );
                        assert_eq!(sheet.has_keyword(KeywordAbility::Flying), flying);
                        let subtypes = if flying {
                            vec!["Djinn", "Monk"]
                        } else {
                            vec!["Elephant"]
                        };
                        let expected: BTreeSet<_> = subtypes
                            .into_iter()
                            .map(|l| vocabulary.resolve_label(l).unwrap())
                            .collect();
                        assert_eq!(sheet.current_creature_subtypes, expected);
                        let actual = sheet.power_toughness.unwrap().derived;
                        assert_eq!((actual.power, actual.toughness), (p.into(), p.into()));
                    }
                }
                "Artful Maneuver" | "Prey's Vengeance" | "Virulent Swipe" => {
                    let venom = name == "Virulent Swipe";
                    let sheet = state
                        .execution()
                        .current_characteristics(object(state, target))
                        .unwrap()
                        .unwrap();
                    assert_eq!(sheet.has_keyword(KeywordAbility::Deathtouch), venom);
                    let pt = sheet.power_toughness.unwrap().derived;
                    assert_eq!(
                        (pt.power, pt.toughness),
                        (3.into(), if venom { 10.into() } else { 12.into() })
                    );
                }
                _ => panic!("add independent outcome expectation for {name}"),
            }
        };
        check_effect(&first, 2, 1);
        let ready = exile_hand::until(
            first,
            |s| matches!(s.pending_decision().map(|p|p.frame()),Some(RuleFrame::OptionalEffect(f)) if f.exile_cast().is_some()),
        );
        assert_eq!(ready.turn().unwrap().position().number, 3);
        let pending = ready.pending_decision().unwrap();
        let RuleFrame::OptionalEffect(frame) = pending.frame() else {
            panic!("rebound offer");
        };
        assert_eq!(
            frame.exile_cast().unwrap().ability,
            ExileCastAbility::Rebound
        );
        let target = has_target.then(|| TargetRef::Object(object(&ready, 3)));
        let second = advance(
            &ready,
            a,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::ReboundCast {
                    target,
                    payment: CastingPayment::default(),
                },
            },
        );
        assert_eq!(zone(&second, 1), ObjectZone::Stack);
        let done = exile_hand::until(second, |s| zone(s, 1) == ObjectZone::Graveyard(a));
        check_effect(&done, 3, 2);
        verified.insert(row.name.clone());
    }
    assert_eq!(verified.len(), 7);
    eprintln!(
        "{} real rebound cards: {}",
        verified.len(),
        verified.into_iter().collect::<Vec<_>>().join(", ")
    );
}
