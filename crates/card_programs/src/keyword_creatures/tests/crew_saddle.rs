use super::*;
use spellwake_engine::{
    activated::ActivatedAbilityId,
    combat::{AttackDeclaration, CombatDecision},
    game::{DecisionReply, RuleFrame},
    object_identity::ObjectId,
};

#[test]
#[ignore = "uses the dated catalogue and source-qualified creature vocabulary"]
fn sleek_schooner_and_quilled_charger_pay_creature_power_and_lose_their_turn_effects() {
    let (inventory, vocabulary, registry) = crate::keyword_creatures::tests::extended_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let initial = ObjectDestination::Battlefield { controller: a };
    let mut seeds = Vec::new();
    for (n, name, owner, destination) in [
        (1, "Sleek Schooner", a, initial),
        (2, "Quilled Charger", a, initial),
        (3, "Brushstrider", a, ObjectDestination::Hand),
        (4, "Raging Goblin", a, initial),
        (10, "Raging Goblin", a, initial),
        (
            5,
            "Raging Goblin",
            b,
            ObjectDestination::Battlefield { controller: b },
        ),
        (6, "Brushstrider", a, ObjectDestination::LibraryBottom),
        (7, "Brushstrider", b, ObjectDestination::LibraryBottom),
    ] {
        let row = inventory.cards.iter().find(|row| row.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        seeds.push(
            registry
                .seed(&id, CardId(n), owner, destination)
                .unwrap_or_else(|err| panic!("{name}: {err:?}")),
        );
    }
    let forest = spellwake_engine::land::BasicLandType::Forest;
    for n in [8, 9] {
        seeds.push(SeedCard {
            card: CardId(n),
            owner: a,
            destination: initial,
            program: Program::land(BTreeSet::from([forest.land_type()]), vec![]),
        });
    }
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        KEYWORD_GAMEPLAY_V3_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "crew-saddle-tests".into(),
        "qualified-crew-saddle".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![a, b],
        a,
        seeds,
        vec![
            CardId(1),
            CardId(2),
            CardId(4),
            CardId(5),
            CardId(8),
            CardId(9),
            CardId(10),
        ],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let mut state = GameState::initialize_scheduled(
        GameId(15),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::Untap,
        },
    )
    .unwrap()
    .into_state();
    let pass = |s: &GameState| {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) = s.execution().window().phase()
        else {
            panic!("expected priority")
        };
        advance(s, holder, GameCommand::Pass)
    };
    let resolve = |s: &GameState| pass(&pass(s));
    for _ in 0..24 {
        if state.turn().unwrap().position().step == Step::PrecombatMain {
            break;
        }
        state = pass(&state);
    }
    let obj = |state: &GameState, id: u64| -> ObjectId {
        state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(id))
            .unwrap()
    };
    let ship = obj(&state, 1);
    let mount = obj(&state, 2);
    let goblin = obj(&state, 4);
    let second_goblin = obj(&state, 10);
    let opponent = obj(&state, 5);
    let choose = |source, creatures| GameCommand::ActivateWithCreatures {
        source,
        ability: ActivatedAbilityId(1),
        creatures,
    };
    let attempt = |s: &GameState, cmd| {
        s.advance(GameInput {
            basis: s.basis().clone(),
            actor: a,
            command: cmd,
        })
    };
    assert!(state.execution().power_toughness(ship).unwrap().is_none());
    assert!(attempt(&state, choose(ship, vec![opponent])).is_err());
    assert!(attempt(&state, choose(mount, vec![mount])).is_err());
    assert!(attempt(&state, choose(mount, vec![goblin, goblin])).is_err());
    assert!(attempt(&state, choose(mount, vec![])).is_err());
    for n in [8, 9] {
        state = advance(
            &state,
            a,
            GameCommand::ActivateMana {
                source: obj(&state, n),
                ability: spellwake_engine::mana::ManaAbilityRef::BasicLand(forest),
            },
        );
    }
    state = advance(
        &state,
        a,
        GameCommand::CastUntargeted {
            spell: obj(&state, 3),
            payment: Some(ManaPayment {
                from_lots: Default::default(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Green,
                    amount: 2.into(),
                }],
            }),
        },
    );
    state = resolve(&state);
    let fresh = obj(&state, 3);
    // The newly cast Brushstrider can crew in response to the Saddle ability without haste.

    assert!(attempt(&state, choose(mount, vec![goblin])).is_err());
    state = advance(&state, a, choose(mount, vec![goblin, second_goblin]));
    assert_eq!(
        state.execution().window().entries()[0].power_contributors(),
        &[goblin, second_goblin]
    );
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(goblin)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert!(
        !state
            .execution()
            .current_characteristics(mount)
            .unwrap()
            .unwrap()
            .saddled
    );
    assert!(attempt(&state, choose(mount, vec![ship])).is_err());
    assert!(attempt(&state, choose(ship, vec![second_goblin])).is_err());
    state = advance(&state, a, choose(ship, vec![fresh]));
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(fresh)
            .unwrap()
            .tapped(),
        Some(true)
    );
    state = resolve(&state);
    assert!(
        state
            .execution()
            .current_characteristics(ship)
            .unwrap()
            .unwrap()
            .is_creature()
    );
    let pt = state
        .execution()
        .power_toughness(ship)
        .unwrap()
        .unwrap()
        .derived;
    assert_eq!((pt.power, pt.toughness), (4.into(), 3.into()));
    state = resolve(&state);
    assert!(
        state
            .execution()
            .current_characteristics(mount)
            .unwrap()
            .unwrap()
            .saddled
    );
    let base = state
        .execution()
        .power_toughness(mount)
        .unwrap()
        .unwrap()
        .derived;
    let mut attacked = false;
    let mut observed_bonus = false;
    for _ in 0..60 {
        if let Some(decision) = state.pending_decision() {
            let choice = match decision.frame() {
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    attacked = true;
                    DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                        attacker: mount,
                        defender: b,
                        defending_planeswalker: None,
                    }])
                }
                RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                    DecisionReply::DeclareBlockers(vec![])
                }
                other => panic!("unexpected {other:?}"),
            };
            state = advance(
                &state,
                decision.chooser(),
                GameCommand::Reply {
                    decision: decision.id(),
                    choice,
                },
            );
        } else if state.turn().unwrap().position().number > 1 {
            break;
        } else {
            if state
                .execution()
                .current_characteristics(mount)
                .unwrap()
                .unwrap()
                .has_keyword(KeywordAbility::Menace)
            {
                observed_bonus = true;
                let pt = state
                    .execution()
                    .power_toughness(mount)
                    .unwrap()
                    .unwrap()
                    .derived;
                assert_eq!(
                    (pt.power, pt.toughness),
                    (base.power.plus(&1.into()), base.toughness.plus(&2.into()))
                );
            }
            state = pass(&state)
        }
    }
    assert!(attacked && observed_bonus);
    assert!(
        !state
            .execution()
            .current_characteristics(mount)
            .unwrap()
            .unwrap()
            .has_keyword(KeywordAbility::Menace)
    );
    assert!(state.turn().unwrap().position().number > 1);
    assert!(state.execution().power_toughness(ship).unwrap().is_none());
    assert!(
        !state
            .execution()
            .current_characteristics(mount)
            .unwrap()
            .unwrap()
            .saddled
    );
}
