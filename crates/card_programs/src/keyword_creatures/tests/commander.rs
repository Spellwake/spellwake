use super::*;
use crate::commander::{CommanderError, prepare_deck};
use spellwake_contracts::names::NameRules;
#[test]
#[ignore = "requires the dated full catalogue and its color identities"]
fn constructed_partner_decks_admit_each_matching_pair_and_enforce_deck_construction() {
    let (inventory, _, _) = current_catalogue();
    let by_name: BTreeMap<_, _> = inventory
        .cards
        .iter()
        .filter_map(|r| {
            Some((
                r.name.as_str(),
                OracleId::try_from(r.oracle_id.clone()?).ok()?,
            ))
        })
        .collect();
    let id = |name: &str| {
        by_name
            .get(name)
            .unwrap_or_else(|| panic!("source missing {name}"))
            .clone()
    };
    let names = NameRules::literal_names();
    let library = vec![id("Wastes"); 98];
    let choices = BTreeMap::new();
    let check =
        |a: &str, b: &str| prepare_deck(inventory, &names, &[id(a), id(b)], &library, &choices);
    for (a, b) in [
        ("Keleth, Sunmane Familiar", "Brinelin, the Moon Kraken"),
        ("Frodo, Adventurous Hobbit", "Sam, Loyal Attendant"),
        ("Kratos, Stoic Father", "Atreus, Impulsive Son"),
        ("Wilson, Refined Grizzly", "Inspiring Leader"),
        ("Nyssa of Traken", "The Ninth Doctor"),
    ] {
        for (first, second) in [(a, b), (b, a)] {
            let deck = check(first, second).unwrap_or_else(|e| panic!("{first} + {second}: {e}"));
            assert_eq!(deck.commanders(), [id(first), id(second)]);
            assert_eq!(deck.library().len(), 98);
        }
    }
    let pair = [
        id("Keleth, Sunmane Familiar"),
        id("Brinelin, the Moon Kraken"),
    ];
    let mut white_and_blue = library.clone();
    white_and_blue[0] = id("Island");
    white_and_blue[1] = id("Plains");
    let admitted = prepare_deck(inventory, &names, &pair, &white_and_blue, &choices).unwrap();
    assert_eq!(
        admitted.color_identity(),
        &[Color::White, Color::Blue].into()
    );
    white_and_blue[1] = id("Mountain");
    assert_eq!(
        prepare_deck(inventory, &names, &pair, &white_and_blue, &choices),
        Err(CommanderError::OffColor(id("Mountain")))
    );
    assert_eq!(
        prepare_deck(inventory, &names, &pair, &library[1..], &choices),
        Err(CommanderError::DeckSize)
    );
    for (a, b) in [
        ("Keleth, Sunmane Familiar", "Frodo, Adventurous Hobbit"),
        ("Kratos, Stoic Father", "Keleth, Sunmane Familiar"),
        ("Nyssa of Traken", "Jenny, Generated Anomaly"),
        ("Inspiring Leader", "The Ninth Doctor"),
    ] {
        assert_eq!(check(a, b), Err(CommanderError::IncompatiblePartners));
    }
    assert!(matches!(
        check("Keleth, Sunmane Familiar", "Keleth, Sunmane Familiar"),
        Err(CommanderError::CopyLimit(_))
    ));
    let piper = id("The Prismatic Piper");
    assert_eq!(
        check("Keleth, Sunmane Familiar", "The Prismatic Piper"),
        Err(CommanderError::ChosenColor(piper.clone()))
    );
    let chosen = [(piper.clone(), Color::Red)].into();
    let deck = prepare_deck(
        inventory,
        &names,
        &[pair[0].clone(), piper],
        &library,
        &chosen,
    )
    .unwrap();
    assert_eq!(deck.color_identity(), &[Color::White, Color::Red].into());
}

#[test]
#[ignore = "requires source-bound partner commanders and matching catalogue mana sources"]
fn rograkh_and_ishai_begin_in_command_and_track_their_casts_separately() {
    use spellwake_engine::{
        game::{DecisionReply, OpeningSetup},
        opening::MulliganChoice,
        shuffle::EntropyTape,
    };
    let (inventory, vocab, registry) = current_catalogue();
    let id = |name: &str| {
        OracleId::try_from(
            inventory
                .cards
                .iter()
                .find(|c| c.name == name)
                .unwrap()
                .oracle_id
                .clone()
                .unwrap(),
        )
        .unwrap()
    };
    let names = Arc::new(NameRules::literal_names());
    let pair = [
        id("Rograkh, Son of Rohgahh"),
        id("Ishai, Ojutai Dragonspeaker"),
    ];
    let lands = [id("Island"), id("Plains")]
        .into_iter()
        .cycle()
        .take(98)
        .collect::<Vec<_>>();
    let deck = prepare_deck(inventory, &names, &pair, &lands, &BTreeMap::new()).unwrap();
    let (a, b) = (SeatId(0), SeatId(1));
    let mut cards = deck.seed_cards(registry, a, CardId(1)).unwrap();
    cards.extend(deck.seed_cards(registry, b, CardId(101)).unwrap());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocab.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "two-commanders".into(),
        "partner-opening".into(),
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
    let mut game = GameState::initialize_commander_opening(GameId(19), context, setup)
        .unwrap()
        .into_state();
    let obj = |g: &GameState, n| {
        g.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    };
    for n in [1, 2, 101, 102] {
        assert_eq!(
            game.execution()
                .window()
                .cards()
                .get(obj(&game, n))
                .unwrap()
                .zone(),
            ObjectZone::Command
        );
        assert_eq!(
            *game
                .execution()
                .commander_record(CardId(n))
                .unwrap()
                .command_zone_casts(),
            0.into()
        );
    }
    for _ in 0..2 {
        let pending = game.pending_decision().unwrap();
        game = advance(
            &game,
            pending.chooser(),
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::Mulligan(MulliganChoice::Keep),
            },
        );
    }
    for _ in 0..15 {
        if game.turn().unwrap().position().step == Step::PrecombatMain {
            break;
        }
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            game.execution().window().phase()
        else {
            panic!("priority")
        };
        game = advance(&game, holder, GameCommand::Pass);
    }
    assert!(
        game.advance(GameInput {
            basis: game.basis().clone(),
            actor: a,
            command: GameCommand::CastUntargeted {
                spell: obj(&game, 101),
                payment: None
            }
        })
        .is_err()
    );
    game = advance(
        &game,
        a,
        GameCommand::CastUntargeted {
            spell: obj(&game, 1),
            payment: None,
        },
    );
    for holder in [a, b] {
        game = advance(&game, holder, GameCommand::Pass);
    }
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(obj(&game, 1))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        *game
            .execution()
            .commander_record(CardId(1))
            .unwrap()
            .command_zone_casts(),
        1.into()
    );
    assert_eq!(
        *game
            .execution()
            .commander_record(CardId(2))
            .unwrap()
            .command_zone_casts(),
        0.into()
    );
    assert_eq!(
        *game
            .execution()
            .commander_record(CardId(101))
            .unwrap()
            .command_zone_casts(),
        0.into()
    );
}

mod commander_mana;
