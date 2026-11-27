use super::*;
use spellwake_engine::{
    combat::AttackDeclaration,
    game::DecisionReply,
    mana::{CastResource, CastingPayment, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput},
    program::{AffectedPlayers, BodyTarget, ExecutableBody, Instruction, ResolvingPlayer},
    targets::TargetRef,
};

fn object(game: &GameState, n: u64) -> spellwake_engine::object_identity::ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(n))
        .unwrap()
}

fn price(color: ManaType, generic: i64) -> CastingPayment {
    let mut spend = vec![ManaSpend {
        mana_type: color,
        amount: 1.into(),
    }];
    if generic > 0 {
        spend.push(ManaSpend {
            mana_type: ManaType::Colourless,
            amount: generic.into(),
        });
    }
    CastingPayment {
        use_alternative: true,
        mana: Some(ManaPayment {
            from_lots: Default::default(),
            spend,
        }),
        ..Default::default()
    }
}

fn setup(name: &str, step: Step, subtype: &str) -> GameState {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "sixth-costs".into(),
        name.into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let row = inventory.cards.iter().find(|r| r.name == name).unwrap();
    let oracle = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
    let mut attacker = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    attacker.printed_keywords.insert(KeywordAbility::Haste);
    attacker
        .creature_subtypes
        .insert(vocabulary.resolve_label(subtype).unwrap());
    let mut battery = Program::artifact(None, vec![]);
    battery.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(
            [
                (ManaType::Black, 1.into()),
                (ManaType::Blue, 1.into()),
                (ManaType::White, 1.into()),
                (ManaType::Red, 1.into()),
                (ManaType::Colourless, 3.into()),
            ]
            .into(),
        ),
    });
    let discard = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::discard(
                    AffectedPlayers::One(ResolvingPlayer::Controller),
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
            ))],
        ),
    );
    let mut cards = vec![
        registry
            .seed(&oracle, CardId(1), a, ObjectDestination::Hand)
            .unwrap(),
        SeedCard {
            card: CardId(2),
            owner: b,
            destination: ObjectDestination::Battlefield { controller: a },
            program: attacker.clone(),
        },
        SeedCard {
            card: CardId(3),
            owner: a,
            destination: ObjectDestination::Battlefield { controller: a },
            program: battery,
        },
        SeedCard {
            card: CardId(4),
            owner: a,
            destination: ObjectDestination::Hand,
            program: discard,
        },
    ];
    for n in 5..15 {
        cards.push(SeedCard {
            card: CardId(n),
            owner: if n % 2 == 0 { a } else { b },
            destination: ObjectDestination::LibraryBottom,
            program: attacker.clone(),
        });
    }
    GameState::initialize_scheduled(
        GameId(16),
        context,
        GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(2), CardId(3)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            names,
        ),
        TurnStart { number: 1, step },
    )
    .unwrap()
    .into_state()
}

fn charge(game: &GameState) -> GameState {
    advance(
        game,
        SeatId(0),
        GameCommand::ActivateMana {
            source: object(game, 3),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    )
}

fn cast(game: &GameState, mut payment: CastingPayment, name: &str) -> GameCommand {
    if name == "Amazing Spider-Girl" {
        payment
            .objects
            .push(CastResource::WebSlinging(object(game, 2)));
    }
    GameCommand::CastWithResources {
        spell: object(game, 1),
        target: (name == "Morsel Theft").then_some(TargetRef::Player(SeatId(1))),
        payment,
    }
}

fn illegal(game: &GameState, command: GameCommand) {
    assert!(
        game.advance(GameInput {
            actor: SeatId(0),
            basis: game.basis().clone(),
            command
        })
        .is_err()
    );
}

#[test]
#[ignore = "uses the dated source catalogue"]
fn morsel_theft_eagle_vision_and_amazing_spider_girl_use_this_turns_combat() {
    let (a, b) = (SeatId(0), SeatId(1));
    for (name, subtype, color, generic) in [
        ("Morsel Theft", "Rogue", ManaType::Black, 1),
        ("Eagle Vision", "Assassin", ManaType::Blue, 1),
        ("Amazing Spider-Girl", "Goblin", ManaType::White, 2),
    ] {
        let early = charge(&setup(name, Step::PrecombatMain, subtype));
        illegal(&early, cast(&early, price(color, generic), name));
        for typ in [subtype, "Elf"] {
            let start = setup(name, Step::DeclareAttackers, typ);
            let decision = start.pending_decision().unwrap();
            let attack = advance(
                &start,
                a,
                GameCommand::Reply {
                    decision: decision.id(),
                    choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                        attacker: object(&start, 2),
                        defender: b,
                        defending_planeswalker: None,
                    }]),
                },
            );
            let main = exile_hand::until(attack, |s| {
                s.turn().unwrap().position().step == Step::PostcombatMain
            });
            assert_eq!(main.execution().life_total(b), Some(&18.into()));
            let ready = charge(&main);
            let cmd = cast(&ready, price(color, generic), name);
            if typ == "Elf" && name != "Amazing Spider-Girl" {
                illegal(&ready, cmd);
                continue;
            }
            let stacked = advance(&ready, a, cmd);
            assert!(
                stacked
                    .execution()
                    .window()
                    .entries()
                    .last()
                    .unwrap()
                    .paid_conditional_cost()
            );
            let done = exile_hand::until(stacked, |s| s.execution().window().entries().is_empty());
            match name {
                "Morsel Theft" => {
                    assert_eq!(done.execution().life_total(b), Some(&15.into()));
                    assert_eq!(done.execution().life_total(a), Some(&23.into()));
                    assert_eq!(done.execution().window().cards().library(a).len(), 4);
                }
                "Eagle Vision" => assert_eq!(done.execution().window().cards().library(a).len(), 2),
                _ => assert_eq!(
                    done.execution()
                        .window()
                        .cards()
                        .get(object(&done, 2))
                        .unwrap()
                        .zone(),
                    ObjectZone::Hand(b)
                ),
            }
        }
    }
}

#[test]
#[ignore = "uses the dated source catalogue"]
fn spider_islanders_mayhem_requires_its_discard_during_the_current_turn() {
    let a = SeatId(0);
    let name = "Spider-Islanders";
    let main = setup(name, Step::PrecombatMain, "Goblin");
    let ready = charge(&main);
    illegal(&ready, cast(&ready, price(ManaType::Red, 1), name));
    let discard = advance(
        &main,
        a,
        GameCommand::CastUntargeted {
            spell: object(&main, 4),
            payment: None,
        },
    );
    let grave = exile_hand::until(discard, |s| {
        s.execution()
            .window()
            .cards()
            .get(object(s, 1))
            .unwrap()
            .zone()
            == ObjectZone::Graveyard(a)
            && s.execution().window().entries().is_empty()
    });
    let ready = charge(&grave);
    let spell = advance(&ready, a, cast(&ready, price(ManaType::Red, 1), name));
    let done = exile_hand::until(spell, |s| s.execution().window().entries().is_empty());
    assert_eq!(
        done.execution()
            .window()
            .cards()
            .get(object(&done, 1))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    let later = exile_hand::until(grave, |s| {
        let position = s.turn().unwrap().position();
        position.number == 3 && position.step == Step::PrecombatMain
    });
    let later = charge(&later);
    illegal(&later, cast(&later, price(ManaType::Red, 1), name));
}
