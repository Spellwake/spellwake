//! Real Commander admission and printed mana-card paths.
use super::*;
use spellwake_engine::{
    game::{DecisionReply, OpeningSetup, RuleFrame},
    land::BasicLandType,
    mana::{ManaAbilityId, ManaAbilityRef},
    opening::MulliganChoice,
    shuffle::EntropyTape,
};

fn object(game: &GameState, card: u64) -> spellwake_engine::object_identity::ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn resolve(game: &GameState) -> GameState {
    advance(
        &advance(game, SeatId(0), GameCommand::Pass),
        SeatId(1),
        GameCommand::Pass,
    )
}
fn prepared(target: &str) -> GameState {
    let (inventory, vocab, registry) = current_catalogue();
    let id = |name: &str| {
        OracleId::try_from(
            inventory
                .cards
                .iter()
                .find(|c| c.name == name && c.required_by_base_formats)
                .unwrap()
                .oracle_id
                .clone()
                .unwrap(),
        )
        .unwrap()
    };
    let names = Arc::new(NameRules::literal_names());
    let commanders = [
        id("Rograkh, Son of Rohgahh"),
        id("Ishai, Ojutai Dragonspeaker"),
    ];
    let mut library = vec![id("Plains"); 98];
    library[1] = id(target);
    let deck = prepare_deck(inventory, &names, &commanders, &library, &BTreeMap::new()).unwrap();
    assert_eq!(
        deck.color_identity(),
        &BTreeSet::from([Color::White, Color::Blue, Color::Red])
    );
    let opposing = prepare_deck(
        inventory,
        &names,
        &commanders,
        &vec![id("Wastes"); 98],
        &BTreeMap::new(),
    )
    .unwrap();
    let (a, b) = (SeatId(0), SeatId(1));
    let mut cards = deck.seed_cards(registry, a, CardId(1)).unwrap();
    cards.extend(opposing.seed_cards(registry, b, CardId(101)).unwrap());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocab.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "commander-identity-mana".into(),
        "commander-mana".into(),
        vocab.id(),
        names.id(),
    )
    .unwrap();
    let setup = OpeningSetup {
        seed: GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![],
            vec![],
            40.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            names,
        ),
        entropy: EntropyTape::new(vec![0; 400]),
    };
    let identities = BTreeMap::from([
        (a, deck.color_identity().clone()),
        (b, opposing.color_identity().clone()),
    ]);
    let mut state = GameState::initialize_commander_opening_with_identities(
        GameId(152),
        context,
        setup,
        identities,
    )
    .unwrap()
    .into_state();
    for _ in 0..2 {
        let decision = state.pending_decision().unwrap();
        state = advance(
            &state,
            decision.chooser(),
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::Mulligan(MulliganChoice::Keep),
            },
        );
    }
    let state = exile_hand::until(state, |g| {
        g.turn()
            .is_some_and(|t| t.position().number == 1 && t.position().step == Step::PrecombatMain)
    });
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 4))
            .unwrap()
            .zone(),
        ObjectZone::Hand(a),
        "target must start in hand"
    );
    state
}
fn play_plains(mut game: GameState) -> (GameState, u64) {
    let id = (3..=100)
        .find(|&n| {
            n != 4
                && game
                    .execution()
                    .window()
                    .cards()
                    .get(object(&game, n))
                    .unwrap()
                    .zone()
                    == ObjectZone::Hand(SeatId(0))
        })
        .unwrap();
    game = advance(
        &game,
        SeatId(0),
        GameCommand::PlayLand {
            land: object(&game, id),
        },
    );
    (game, id)
}
fn reach_turn(game: GameState, turn: u64) -> GameState {
    exile_hand::until(game, |g| {
        g.turn().is_some_and(|t| {
            t.position().number == turn && t.position().step == Step::PrecombatMain
        })
    })
}

#[test]
#[ignore = "requires dated Oracle catalogue and source-bound Commander decks"]
fn cmm_commander_identity_mana_uses_admitted_partner_union_and_sphere_draws() {
    use ManaType::{Blue, Red, White};
    let a = SeatId(0);
    for (name, cost) in [
        ("Command Tower", 0),
        ("Arcane Signet", 2),
        ("Commander's Sphere", 3),
    ] {
        let mut game = prepared(name);
        if cost == 0 {
            game = advance(
                &game,
                a,
                GameCommand::PlayLand {
                    land: object(&game, 4),
                },
            );
        } else {
            let mut lands = vec![];
            for n in 0..cost {
                if n > 0 {
                    game = reach_turn(game, 1 + 2 * n as u64);
                }
                let (next, land) = play_plains(game);
                game = next;
                lands.push(land);
            }
            for land in lands {
                game = advance(
                    &game,
                    a,
                    GameCommand::ActivateMana {
                        source: object(&game, land),
                        ability: ManaAbilityRef::BasicLand(BasicLandType::Plains),
                    },
                );
            }
            game = advance(
                &game,
                a,
                GameCommand::CastUntargeted {
                    spell: object(&game, 4),
                    payment: Some(ManaPayment {
                        from_lots: Default::default(),
                        spend: vec![ManaSpend {
                            mana_type: White,
                            amount: cost.into(),
                        }],
                    }),
                },
            );
            game = resolve(&game);
        }
        let tapped = advance(
            &game,
            a,
            GameCommand::ActivateMana {
                source: object(&game, 4),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        );
        let decision = tapped.pending_decision().unwrap();
        let RuleFrame::ManaColor(frame) = decision.frame() else {
            panic!("{name}: expected color choice")
        };
        assert_eq!(frame.colors(), &BTreeSet::from([White, Blue, Red]));
        assert!(
            tapped
                .advance(GameInput {
                    basis: tapped.basis().clone(),
                    actor: a,
                    command: GameCommand::Reply {
                        decision: decision.id(),
                        choice: DecisionReply::ManaColor(ManaType::Green)
                    }
                })
                .is_err()
        );
        let mut chosen = advance(
            &tapped,
            a,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::ManaColor(Blue),
            },
        );
        assert_eq!(chosen.execution().mana(a, Blue), 1.into());
        if name == "Commander's Sphere" {
            let top = chosen.execution().window().cards().library(a)[0];
            chosen = advance(
                &chosen,
                a,
                GameCommand::ActivateUntargetedAbility {
                    source: object(&chosen, 4),
                    ability: ActivatedAbilityId(0),
                    payment: None,
                },
            );
            assert_eq!(
                chosen
                    .execution()
                    .window()
                    .cards()
                    .get(object(&chosen, 4))
                    .unwrap()
                    .zone(),
                ObjectZone::Graveyard(a)
            );
            chosen = resolve(&chosen);
            assert!(chosen.execution().window().cards().get(top).is_none());
        }
    }
}
