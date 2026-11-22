//! Dated card games for creature-entry life-gain watchers.
use super::*;
use crate::keyword_creatures::tests::current_catalogue;
use spellwake_engine::{
    combat::CombatRules,
    game::{DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, RuleFrame},
    mana::{FloatingMana, ManaPayment, ManaSpend},
    object_identity::ObjectId,
    priority::PriorityPhase,
    setup::GameSeed,
    stack_window::WindowPhase,
    turn::{Step, TurnStart},
};
use std::sync::Arc;

fn object(g: &GameState, n: u64) -> ObjectId {
    g.execution()
        .window()
        .cards()
        .current_identity(CardId(n))
        .unwrap()
}
fn command(g: &GameState, actor: SeatId, cmd: GameCommand) -> GameState {
    g.advance(GameInput {
        basis: g.basis().clone(),
        actor,
        command: cmd,
    })
    .unwrap()
    .into_state()
}
fn settle(mut g: GameState) -> GameState {
    for _ in 0..40 {
        if let Some(p) = g.pending_decision() {
            let choice = match p.frame() {
                RuleFrame::Triggers(f) => DecisionReply::OrderTriggers(f.instances().to_vec()),
                RuleFrame::TimestampOrder(f) => {
                    DecisionReply::TimestampOrder(f.members().iter().map(|m| m.member).collect())
                }
                f => panic!("unexpected decision {f:?}"),
            };
            g = command(
                &g,
                p.chooser(),
                GameCommand::Reply {
                    decision: p.id(),
                    choice,
                },
            );
        } else if g.execution().window().entries().is_empty() {
            return g;
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                g.execution().window().phase()
            else {
                panic!("priority")
            };
            g = command(&g, holder, GameCommand::Pass);
        }
    }
    panic!("stack did not settle")
}

fn game(name: &str, active: SeatId, self_entry: bool) -> GameState {
    let (inventory, vocab, registry) = current_catalogue();
    let card = inventory.cards.iter().find(|c| c.name == name).unwrap();
    let id = OracleId::try_from(card.oracle_id.clone().unwrap()).unwrap();
    assert!(compile(card, &vocab).is_some(), "{name}");
    let cost = fixed_cost(card.faces[0].mana_cost.as_deref().unwrap()).unwrap();
    let mut required = cost.required;
    if cost.generic.is_positive() {
        required.insert(ManaType::Colourless, cost.generic);
    }
    let mana = required
        .iter()
        .map(|(&mana_type, amount)| FloatingMana {
            player: SeatId(0),
            mana_type,
            amount: amount.clone(),
        })
        .collect();
    let pay = ManaPayment {
        from_lots: Default::default(),
        spend: required
            .into_iter()
            .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
            .collect(),
    };
    let source = registry
        .seed(
            &id,
            CardId(1),
            SeatId(0),
            if self_entry {
                ObjectDestination::Hand
            } else {
                ObjectDestination::Battlefield {
                    controller: SeatId(0),
                }
            },
        )
        .unwrap();
    let creature = SeedCard {
        card: CardId(2),
        owner: active,
        destination: ObjectDestination::Hand,
        program: Program::creature(Some(ManaCost::free()), 1.into(), 1.into(), BTreeSet::new()),
    };
    let artifact = SeedCard {
        card: CardId(3),
        owner: active,
        destination: ObjectDestination::Hand,
        program: Program::artifact(Some(ManaCost::free()), vec![]),
    };
    let rules = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let ctx = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocab.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "current".into(),
        "watcher".into(),
        vocab.id(),
        rules.id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![SeatId(0), SeatId(1)],
        active,
        vec![source, creature, artifact],
        if self_entry { vec![] } else { vec![CardId(1)] },
        mana,
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocab,
        rules,
    );
    let mut g = GameState::initialize_scheduled(
        GameId(904),
        ctx,
        seed,
        TurnStart {
            number: 1,
            step: Step::PostcombatMain,
        },
    )
    .unwrap()
    .into_state();
    if self_entry {
        g = command(
            &g,
            SeatId(0),
            GameCommand::CastUntargeted {
                spell: object(&g, 1),
                payment: Some(pay),
            },
        );
        return settle(g);
    }
    g = command(
        &g,
        active,
        GameCommand::CastUntargeted {
            spell: object(&g, 3),
            payment: None,
        },
    );
    g = settle(g);
    assert_eq!(
        g.execution().life_total(SeatId(0)),
        Some(&20.into()),
        "artifact must not trigger {name}"
    );
    g = command(
        &g,
        active,
        GameCommand::CastUntargeted {
            spell: object(&g, 2),
            payment: None,
        },
    );
    settle(g)
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn life_gain_watchers_use_actual_cards_and_entry_relations() {
    for (name, gain, opposing, self_gain) in [
        ("Healer of the Pride", 2, false, false),
        ("Hinterland Sanctifier", 1, false, false),
        ("Essence Warden", 1, true, false),
        ("Bogwater Lumaret", 1, false, true),
        ("Kor Celebrant", 1, false, true),
        ("Dazzling Angel", 1, false, false),
        ("Lifecreed Duo", 1, false, false),
    ] {
        for (active, expected) in [
            (SeatId(0), 20 + gain),
            (SeatId(1), 20 + if opposing { gain } else { 0 }),
        ] {
            let g = game(name, active, false);
            assert_eq!(
                g.execution().life_total(SeatId(0)),
                Some(&expected.into()),
                "{name}, active {active:?}"
            );
            assert_eq!(g.execution().life_total(SeatId(1)), Some(&20.into()));
        }
        let g = game(name, SeatId(0), true);
        assert_eq!(
            g.execution().life_total(SeatId(0)),
            Some(&(20 + if self_gain { gain } else { 0 }).into()),
            "{name} self entry"
        );
    }
}
