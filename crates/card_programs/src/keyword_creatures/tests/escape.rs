use super::*;
use spellwake_engine::{
    mana::{CastResource, CastingPayment},
    program::ConditionalCastCost,
    targets::TargetRef,
};

#[test]
#[ignore = "requires the saved Oracle snapshot and matching creature vocabulary"]
fn real_escape_bindings_pay_the_alternative_and_apply_the_entire_printed_effect() {
    let (inventory, vocab, registry) = crate::keyword_creatures::tests::extended_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
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
        let Some(ConditionalCastCost::Escape {
            mana,
            exile,
            entry_counters,
        }) = &bound.instruction.conditional_cost
        else {
            continue;
        };
        let (price, count, bonus) = match card.name.as_str() {
            "Glimpse of Freedom" => ("{2}{U}", 5, 0),
            "Sweet Oblivion" => ("{3}{U}", 4, 0),
            "Fruit of Tizerus" => ("{3}{B}", 3, 0),
            "Loathsome Chimera" => ("{4}{G}", 3, 1),
            "Voracious Typhon" => ("{5}{G}{G}", 4, 3),
            "Mogis's Favor" => ("{2}{B}", 2, 0),
            "Sentinel's Eyes" => ("{W}", 2, 0),
            "Escape Velocity" => ("{1}{R}", 2, 0),
            name => panic!("add the printed outcome for {name}"),
        };
        assert_eq!(mana, &fixed_cost(price).unwrap(), "{}", card.name);
        assert_eq!((*exile, *entry_counters), (count, bonus));
        let seeded = registry
            .seed(&id, CardId(1), a, ObjectDestination::Graveyard)
            .unwrap();
        let mut pool = mana.required.clone();
        if mana.generic.is_positive() {
            pool.insert(ManaType::Colourless, mana.generic.clone());
        }
        let funding = pool
            .iter()
            .map(|(&mana_type, n)| FloatingMana {
                player: a,
                mana_type,
                amount: n.clone(),
            })
            .collect();
        let mut cards = vec![seeded];
        let mut add = |id, owner, destination| {
            cards.push(SeedCard {
                card: CardId(id),
                owner,
                destination,
                program: Program::creature(None, 2.into(), 5.into(), BTreeSet::new()),
            })
        };
        for id in 10..10 + u64::from(count) {
            add(id, a, ObjectDestination::Graveyard);
        }
        add(2, b, ObjectDestination::Battlefield { controller: b });
        add(3, a, ObjectDestination::LibraryBottom);
        for id in 20..25 {
            add(id, b, ObjectDestination::LibraryBottom);
        }
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let ctx = GameContext::new(
            KEYWORD_GAMEPLAY_V3_PROFILE.into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "keyword-gameplay-registry14".into(),
            "escape".into(),
            vocab.id(),
            names.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(2)],
            funding,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab.clone(),
            names,
        );
        let mut state = GameState::initialize_scheduled(
            GameId(14),
            ctx,
            seed,
            TurnStart {
                number: 1,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let oid = |s: &GameState, n| {
            s.execution()
                .window()
                .cards()
                .current_identity(CardId(n))
                .unwrap()
        };
        let zone = |s: &GameState, n| {
            s.execution()
                .window()
                .cards()
                .get(oid(s, n))
                .unwrap()
                .zone()
        };
        let target = match card.name.as_str() {
            "Sentinel's Eyes" | "Mogis's Favor" | "Escape Velocity" => {
                Some(TargetRef::Object(oid(&state, 2)))
            }
            "Fruit of Tizerus" | "Sweet Oblivion" => Some(TargetRef::Player(b)),
            _ => None,
        };
        let objects = (10..10 + u64::from(count))
            .map(|n| CastResource::Escape(oid(&state, n)))
            .collect();
        let command = GameCommand::CastWithResources {
            spell: oid(&state, 1),
            target,
            payment: CastingPayment {
                mana: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend: pool
                        .into_iter()
                        .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
                        .collect(),
                }),
                use_alternative: true,
                objects,
                ..Default::default()
            },
        };
        state = advance(&state, a, command);
        for n in 10..10 + u64::from(count) {
            assert_eq!(zone(&state, n), ObjectZone::Exile);
        }
        for seat in [a, b] {
            state = advance(&state, seat, GameCommand::Pass);
        }
        if let Some(pending) = state.pending_decision() {
            assert_eq!(card.name, "Sweet Oblivion");
            let reply = GameCommand::Reply {
                decision: pending.id(),
                choice: spellwake_engine::game::DecisionReply::OrderResolutionGraveyard(
                    (20..24).map(|n| oid(&state, n)).collect(),
                ),
            };
            state = advance(&state, b, reply);
        }
        match card.name.as_str() {
            "Glimpse of Freedom" => assert_eq!(zone(&state, 3), ObjectZone::Hand(a)),
            "Fruit of Tizerus" => assert_eq!(state.execution().life_total(b), Some(&18.into())),
            "Sweet Oblivion" => {
                for n in 20..24 {
                    assert_eq!(zone(&state, n), ObjectZone::Graveyard(b));
                }
                assert_eq!(zone(&state, 24), ObjectZone::Library(b));
            }
            "Loathsome Chimera" | "Voracious Typhon" => assert_eq!(
                state
                    .execution()
                    .power_toughness_counters(oid(&state, 1))
                    .unwrap()
                    .plus_one,
                i64::from(bonus).into()
            ),
            name => {
                assert_eq!(state.execution().attached_to(oid(&state, 1)), target);
                let host = state
                    .execution()
                    .current_characteristics(oid(&state, 2))
                    .unwrap()
                    .unwrap();
                let (power, toughness, ability) = match name {
                    "Mogis's Favor" => (4, 4, None),
                    "Escape Velocity" => (3, 5, Some(KeywordAbility::Haste)),
                    _ => (3, 6, Some(KeywordAbility::Vigilance)),
                };
                if let Some(key) = ability {
                    assert!(host.has_keyword(key));
                }
                let pt = &host.power_toughness.unwrap().derived;
                assert_eq!(
                    (pt.power.clone(), pt.toughness.clone()),
                    (power.into(), toughness.into())
                );
            }
        }
        let expected = if matches!(
            card.name.as_str(),
            "Glimpse of Freedom" | "Fruit of Tizerus" | "Sweet Oblivion"
        ) {
            ObjectZone::Graveyard(a)
        } else {
            ObjectZone::Battlefield
        };
        assert_eq!(zone(&state, 1), expected);
        tested.insert(card.name.clone());
    }
    assert_eq!(tested.len(), 8, "{tested:?}");
    eprintln!(
        "{} real Escape cards: {}",
        tested.len(),
        tested.into_iter().collect::<Vec<_>>().join(", ")
    );
}
