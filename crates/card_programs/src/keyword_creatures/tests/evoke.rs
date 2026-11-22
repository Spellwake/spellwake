use super::*;
use spellwake_engine::{
    game::{DecisionReply, RuleFrame},
    mana::CastingPayment,
};

#[test]
#[ignore = "requires the dated catalogue and qualified vocabulary"]
fn mulldrifter_orders_evoke_and_draw_and_walker_of_the_grove_leaves_its_token() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let rules = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let seed_card = |name: &str, card_id, zone| {
        let row = inventory.cards.iter().find(|c| c.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry
            .seed(&id, CardId(card_id), a, zone)
            .unwrap_or_else(|err| panic!("{name}: {err:?}"))
    };
    let mut cards = vec![
        seed_card("Mulldrifter", 1, ObjectDestination::Hand),
        seed_card("Walker of the Grove", 2, ObjectDestination::Hand),
    ];
    for n in 3..7 {
        cards.push(seed_card(
            "Raging Goblin",
            n,
            ObjectDestination::LibraryBottom,
        ));
    }
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "evoke".into(),
        "named-evoke".into(),
        vocabulary.id(),
        rules.id(),
    )
    .unwrap();
    let mana = ManaType::ALL
        .into_iter()
        .map(|mana_type| FloatingMana {
            player: a,
            mana_type,
            amount: 12.into(),
        })
        .collect();
    let game = GameState::initialize_scheduled(
        GameId(21),
        context,
        GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![],
            mana,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            rules,
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
    let pay = |items: &[(ManaType, i64)]| {
        Some(ManaPayment {
            from_lots: Default::default(),
            spend: items
                .iter()
                .map(|&(mana_type, n)| ManaSpend {
                    mana_type,
                    amount: n.into(),
                })
                .collect(),
        })
    };
    let cast = |s: &GameState, n, alt, items| {
        advance(
            s,
            a,
            GameCommand::CastWithResources {
                spell: obj(s, n),
                target: None,
                payment: CastingPayment {
                    use_alternative: alt,
                    mana: pay(items),
                    ..Default::default()
                },
            },
        )
    };
    let pass = |s: &GameState| {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) = s.execution().window().phase()
        else {
            panic!("expected priority")
        };
        advance(s, holder, GameCommand::Pass)
    };
    let resolve = |s: &GameState| pass(&pass(s));
    let entered = resolve(&cast(
        &game,
        1,
        false,
        &[(ManaType::Colourless, 4), (ManaType::Blue, 1)],
    ));
    assert!(entered.pending_decision().is_none());
    let full = resolve(&entered);
    assert_eq!(
        full.execution()
            .window()
            .cards()
            .get(obj(&full, 1))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    for draw_first in [false, true] {
        let entered = resolve(&cast(
            &game,
            1,
            true,
            &[(ManaType::Colourless, 2), (ManaType::Blue, 1)],
        ));
        let decision = entered.pending_decision().unwrap();
        let RuleFrame::Triggers(frame) = decision.frame() else {
            panic!("expected both ETB triggers")
        };
        assert_eq!(frame.instances().len(), 2);
        let mut ids = entered.execution().waiting_triggers().collect::<Vec<_>>();
        ids.sort_by_key(|instance| {
            matches!(
                instance.body.instructions.as_slice(),
                [Instruction::DrawController { .. }]
            ) == draw_first
        });
        let stacked = advance(
            &entered,
            a,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::OrderTriggers(ids.iter().map(|i| i.id).collect()),
            },
        );
        let first = resolve(&stacked);
        let zone = first
            .execution()
            .window()
            .cards()
            .get(obj(&first, 1))
            .unwrap()
            .zone();
        assert_eq!(
            zone,
            if draw_first {
                ObjectZone::Battlefield
            } else {
                ObjectZone::Graveyard(a)
            }
        );
        let second = resolve(&first);
        assert_eq!(
            second
                .execution()
                .window()
                .cards()
                .get(obj(&second, 1))
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(a)
        );
        assert_eq!(second.execution().window().cards().library(a).len(), 2);
    }
    let entered = resolve(&cast(
        &game,
        2,
        true,
        &[(ManaType::Colourless, 4), (ManaType::Green, 1)],
    ));
    let sacrificed = resolve(&entered);
    let result = resolve(&sacrificed);
    let token = result
        .execution()
        .window()
        .cards()
        .iter()
        .find(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
        .unwrap();
    assert_eq!(
        result
            .execution()
            .power_toughness(token.id())
            .unwrap()
            .unwrap()
            .derived
            .power,
        4.into()
    );
}
