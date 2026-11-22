use super::*;
use spellwake_engine::{
    effects::EffectError, game::GameError, land::BasicLandType, mana::ManaAbilityRef,
    object_identity::ObjectId,
};
#[test]
#[ignore = "uses the dated catalogue and source-qualified creature vocabulary"]
fn disowned_ancestor_and_prowcatcher_specialist_pay_outlast_and_exhaust() {
    let (inventory, vocabulary, registry) = crate::keyword_creatures::tests::extended_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let home = ObjectDestination::Battlefield { controller: a };
    let mut cards = vec![];
    let mut battlefield = vec![CardId(1), CardId(2)];
    for (n, name, owner, zone) in [
        (1, "Disowned Ancestor", a, home),
        (2, "Prowcatcher Specialist", a, home),
        (3, "Disowned Ancestor", a, ObjectDestination::Hand),
        (4, "Disowned Ancestor", a, ObjectDestination::LibraryBottom),
        (5, "Disowned Ancestor", b, ObjectDestination::LibraryBottom),
    ] {
        let row = inventory.cards.iter().find(|row| row.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        cards.push(
            registry
                .seed(&id, CardId(n), owner, zone)
                .unwrap_or_else(|err| panic!("{name}: {err:?}")),
        );
    }
    let lands = (10..19)
        .map(|n| {
            (
                n,
                if n < 14 {
                    BasicLandType::Mountain
                } else {
                    BasicLandType::Swamp
                },
            )
        })
        .collect::<Vec<_>>();
    for &(n, kind) in &lands {
        cards.push(SeedCard {
            card: CardId(n),
            owner: a,
            destination: home,
            program: Program::land(BTreeSet::from([kind.land_type()]), vec![]),
        });
        battlefield.push(CardId(n));
    }
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        KEYWORD_GAMEPLAY_V3_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "limited-activations".into(),
        "outlast-exhaust".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![a, b],
        a,
        cards,
        battlefield,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let mut game = GameState::initialize_scheduled(
        GameId(16),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::Untap,
        },
    )
    .unwrap()
    .into_state();
    let pass = |g: &GameState| {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) = g.execution().window().phase()
        else {
            panic!("expected priority")
        };
        advance(g, holder, GameCommand::Pass)
    };
    let resolve = |g: &GameState| pass(&pass(g));
    for _ in 0..30 {
        if game.turn().unwrap().position().step == Step::PrecombatMain {
            break;
        }
        game = pass(&game);
    }
    let obj = |g: &GameState, n| -> ObjectId {
        g.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    };
    for (n, kind) in lands {
        game = advance(
            &game,
            a,
            GameCommand::ActivateMana {
                source: obj(&game, n),
                ability: ManaAbilityRef::BasicLand(kind),
            },
        );
    }
    let pay = |mana_type, n: i64| {
        Some(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type,
                amount: n.into(),
            }],
        })
    };
    let activate = |source, price| GameCommand::ActivateUntargetedAbility {
        source,
        ability: ActivatedAbilityId(1),
        payment: price,
    };
    let error = |g: &GameState, command| {
        g.advance(GameInput {
            basis: g.basis().clone(),
            actor: a,
            command,
        })
        .err()
        .unwrap()
    };
    let ancestor = obj(&game, 1);
    let goblin = obj(&game, 2);
    game = advance(&game, a, activate(goblin, pay(ManaType::Red, 4)));
    assert!(matches!(
        error(&game, activate(ancestor, pay(ManaType::Black, 2))),
        GameError::Effect(EffectError::Timing(_))
    ));
    assert_eq!(
        error(&game, activate(goblin, None)),
        GameError::Effect(EffectError::AlreadyExhausted)
    );
    game = resolve(&game);
    assert_eq!(
        game.execution()
            .power_toughness_counters(goblin)
            .unwrap()
            .plus_one,
        2.into()
    );
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(goblin)
            .unwrap()
            .tapped(),
        Some(false)
    );
    game = advance(&game, a, activate(ancestor, pay(ManaType::Black, 2)));
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(ancestor)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(
        game.execution()
            .power_toughness_counters(ancestor)
            .unwrap()
            .plus_one,
        0.into()
    );
    game = resolve(&game);
    assert_eq!(
        game.execution()
            .power_toughness_counters(ancestor)
            .unwrap()
            .plus_one,
        1.into()
    );
    game = advance(
        &game,
        a,
        GameCommand::CastUntargeted {
            spell: obj(&game, 3),
            payment: pay(ManaType::Black, 1),
        },
    );
    game = resolve(&game);
    assert_eq!(
        error(&game, activate(obj(&game, 3), pay(ManaType::Black, 2))),
        GameError::Effect(EffectError::CreatureTapRestricted)
    );
}
