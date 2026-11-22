//! Named cases from the dated catalogue use land play, priority, costs and player choices.
use super::*;
use spellwake_engine::{
    effects::LibraryTopReply,
    game::{DecisionReply, RuleFrame},
    land::BasicLandType,
    mana::{ManaAbilityId, ManaAbilityRef},
    object_identity::ObjectId,
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn seed(name: &str, n: u64, owner: SeatId, destination: ObjectDestination) -> SeedCard {
    let (inventory, _, registry) = current_catalogue();
    let row = inventory
        .cards
        .iter()
        .find(|r| r.required_by_base_formats && r.name == name)
        .unwrap();
    let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
    registry.seed(&id, CardId(n), owner, destination).unwrap()
}
fn game(name: &str, extra: &[(&str, SeatId)], seats: usize, life: i64) -> GameState {
    game_with_library(name, extra, seats, life, &["Forest"])
}
fn game_with_library(
    name: &str,
    extra: &[(&str, SeatId)],
    seats: usize,
    life: i64,
    library: &[&str],
) -> GameState {
    let (_, vocabulary, registry) = current_catalogue();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        format!(
            "keyword-gameplay-registry{}",
            registry.coverage.registry_version
        ),
        "nonbasic-land-play".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let mut cards = vec![
        seed(name, 1, A, ObjectDestination::Hand),
        seed("Forest", 3, B, ObjectDestination::LibraryBottom),
    ];
    for (index, card) in library.iter().enumerate() {
        let number = if index == 0 { 2 } else { 100 + index as u64 };
        cards.push(seed(card, number, A, ObjectDestination::LibraryBottom));
    }
    let mut order = vec![];
    for (index, (other, controller)) in extra.iter().enumerate() {
        let n = 10 + index as u64;
        cards.push(seed(
            other,
            n,
            *controller,
            ObjectDestination::Battlefield {
                controller: *controller,
            },
        ));
        order.push(CardId(n));
    }
    let mana = vec![FloatingMana {
        player: A,
        mana_type: ManaType::Colourless,
        amount: 4.into(),
    }];
    let seats = (0..seats).map(|n| SeatId(n.try_into().unwrap())).collect();
    let initial = GameSeed::with_uniform_life(
        seats,
        A,
        cards,
        order,
        mana,
        life.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    GameState::initialize_scheduled(
        GameId(190),
        context,
        initial,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state()
}
fn id(s: &GameState, n: u64) -> ObjectId {
    s.execution()
        .window()
        .cards()
        .current_identity(CardId(n))
        .unwrap()
}
fn play(s: &GameState) -> GameState {
    advance(s, A, GameCommand::PlayLand { land: id(s, 1) })
}
fn tapped(s: &GameState) -> bool {
    s.execution()
        .window()
        .cards()
        .get(id(s, 1))
        .unwrap()
        .tapped()
        .unwrap()
}
fn reject(s: &GameState, c: GameCommand) {
    assert!(
        s.advance(GameInput {
            basis: s.basis().clone(),
            actor: A,
            command: c
        })
        .is_err()
    )
}
fn resolve(s: &GameState) -> GameState {
    advance(&advance(s, A, GameCommand::Pass), B, GameCommand::Pass)
}
fn production(s: &GameState, ability: ManaAbilityRef) -> GameCommand {
    GameCommand::ActivateMana {
        source: id(s, 1),
        ability,
    }
}
fn reply(s: &GameState, choice: DecisionReply) -> GameCommand {
    GameCommand::Reply {
        decision: s.pending_decision().unwrap().id(),
        choice,
    }
}
fn payment(amount: i64) -> Option<ManaPayment> {
    Some(ManaPayment {
        from_lots: Default::default(),
        spend: vec![ManaSpend {
            mana_type: ManaType::Colourless,
            amount: amount.into(),
        }],
    })
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn named_nonbasic_lands_check_preentry_controller_types_life_and_opponents() {
    let forest = ("Forest", A);
    let island = ("Island", A);
    let tundra = ("Tundra", A);
    let other = ("Forest", B);
    for (land, prior, seats, life, expected) in [
        ("Rockfall Vale", vec![forest, other], 2, 20, true),
        ("Rockfall Vale", vec![forest, island], 2, 20, false),
        ("Blackcleave Cliffs", vec![forest, forest], 2, 20, false),
        (
            "Blackcleave Cliffs",
            vec![forest, forest, forest],
            2,
            20,
            true,
        ),
        ("Smoldering Marsh", vec![tundra, forest], 2, 20, true),
        ("Smoldering Marsh", vec![forest, forest], 2, 20, false),
        ("Sunpetal Grove", vec![tundra], 2, 20, false),
        ("Sunpetal Grove", vec![island, other], 2, 20, true),
        ("Sea of Clouds", vec![], 2, 20, true),
        ("Sea of Clouds", vec![], 3, 20, false),
        ("Abandoned Campground", vec![], 2, 14, true),
        ("Abandoned Campground", vec![], 2, 13, false),
        ("Turbulent Fen", vec![other; 7], 2, 20, true),
        ("Turbulent Fen", vec![other; 8], 2, 20, false),
    ] {
        let s = play(&game(land, &prior, seats, life));
        assert_eq!(
            tapped(&s),
            expected,
            "{land} with {} preexisting lands",
            prior.len()
        );
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn named_nonbasic_mana_uses_only_the_printed_or_intrinsic_colors_and_tap_cost() {
    let fast = play(&game("Blackcleave Cliffs", &[], 2, 20));
    let ability = ManaAbilityRef::Printed(ManaAbilityId(0));
    let choice = advance(&fast, A, production(&fast, ability));
    let RuleFrame::ManaColor(f) = choice.pending_decision().unwrap().frame() else {
        panic!("mana color")
    };
    assert_eq!(f.colors(), &[ManaType::Black, ManaType::Red].into());
    reject(
        &choice,
        reply(&choice, DecisionReply::ManaColor(ManaType::Blue)),
    );
    for color in [ManaType::Black, ManaType::Red] {
        let result = advance(&choice, A, reply(&choice, DecisionReply::ManaColor(color)));
        assert_eq!(result.execution().mana(A, color), 1.into());
        assert!(tapped(&result));
        reject(&result, production(&result, ability));
    }
    let original = play(&game("Tundra", &[], 2, 20));
    for intrinsic in [BasicLandType::Island, BasicLandType::Plains] {
        let result = advance(
            &original,
            A,
            production(&original, ManaAbilityRef::BasicLand(intrinsic)),
        );
        assert_eq!(result.execution().mana(A, intrinsic.mana_type()), 1.into());
    }
    reject(
        &original,
        production(&original, ManaAbilityRef::BasicLand(BasicLandType::Forest)),
    );
    let bridge = play(&game("Silverbluff Bridge", &[], 2, 20));
    let sheet = bridge
        .execution()
        .current_characteristics(id(&bridge, 1))
        .unwrap()
        .unwrap();
    assert!(
        sheet.has_type(CardType::Artifact)
            && sheet.keywords.contains(&KeywordAbility::Indestructible)
    );
    assert!(tapped(&bridge));
    reject(&bridge, production(&bridge, ability));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shattered_sanctum_uses_only_its_controllers_other_lands_and_printed_colors() {
    let ability = ManaAbilityRef::Printed(ManaAbilityId(0));
    for (lands, enters_tapped) in [
        (vec![], true),
        (vec![("Forest", A), ("Forest", B), ("Island", B)], true),
        (vec![("Tundra", A), ("Rockfall Vale", A)], false),
    ] {
        let game = game("Shattered Sanctum", &lands, 2, 20);
        let play = game
            .advance(GameInput {
                basis: game.basis().clone(),
                actor: A,
                command: GameCommand::PlayLand { land: id(&game, 1) },
            })
            .unwrap();
        let land_play = play.land_play().unwrap();
        assert_eq!(land_play.player, A);
        assert_eq!(land_play.before.zone(), ObjectZone::Hand(A));
        assert_eq!(land_play.after.zone(), ObjectZone::Battlefield);
        assert_eq!(land_play.after.id(), id(play.state(), 1));
        let mut state = play.into_state();
        assert!(state.execution().window().entries().is_empty());
        assert_eq!(tapped(&state), enters_tapped, "{lands:?}");
        if enters_tapped {
            reject(&state, production(&state, ability));
            continue;
        }

        for color in [ManaType::White, ManaType::Black] {
            assert!(!tapped(&state));
            let before = ManaType::ALL.map(|kind| state.execution().mana(A, kind));
            let choice = advance(&state, A, production(&state, ability));
            let decision = choice.pending_decision().unwrap();
            assert_eq!(decision.chooser(), A);
            let RuleFrame::ManaColor(frame) = decision.frame() else {
                panic!("expected Sanctum's printed mana color selection")
            };
            assert_eq!(frame.colors(), &[ManaType::White, ManaType::Black].into());
            assert_eq!(
                ManaType::ALL.map(|kind| choice.execution().mana(A, kind)),
                before
            );
            reject(
                &choice,
                reply(&choice, DecisionReply::ManaColor(ManaType::Blue)),
            );
            state = advance(&choice, A, reply(&choice, DecisionReply::ManaColor(color)));
            for (kind, initial) in ManaType::ALL.into_iter().zip(before) {
                assert_eq!(
                    state.execution().mana(A, kind),
                    initial.plus(&i64::from(kind == color).into()),
                    "{color:?} choice, {kind:?} pool"
                );
                assert_eq!(state.execution().mana(B, kind), 0.into());
            }
            assert!(tapped(&state));
            reject(&state, production(&state, ability));
            if color == ManaType::White {
                state = exile_hand::until(state, |game| {
                    let position = game.turn().unwrap().position();
                    position.number == 3 && position.step == Step::PrecombatMain
                });
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn named_nonbasic_land_triggers_and_cycling_payments_reach_the_correct_zones() {
    let refuge = play(&game("Kazandu Refuge", &[], 2, 20));
    assert!(tapped(&refuge));
    assert_eq!(refuge.execution().life_total(A), Some(&20.into()));
    assert_eq!(refuge.execution().window().entries().len(), 1);
    let gained = resolve(&refuge);
    assert_eq!(gained.execution().life_total(A), Some(&21.into()));
    let sewer = resolve(&play(&game("Undercity Sewers", &[], 2, 20)));
    let RuleFrame::LibraryTop(frame) = sewer.pending_decision().unwrap().frame() else {
        panic!("surveil")
    };
    let top = frame.domain_top_first().to_vec();
    assert_eq!(top, [id(&sewer, 2)]);
    let surveilled = advance(
        &sewer,
        A,
        reply(
            &sewer,
            DecisionReply::LibraryTop(LibraryTopReply::Surveil {
                top_first: vec![],
                to_graveyard: top,
            }),
        ),
    );
    assert_eq!(
        surveilled
            .execution()
            .window()
            .cards()
            .get(id(&surveilled, 2))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );

    let triome = game("Ketria Triome", &[], 2, 20);
    let activate = |price| GameCommand::Cycle {
        source: id(&triome, 1),
        ability: ActivatedAbilityId(0),
        payment: payment(price),
    };
    reject(&triome, activate(2));
    let paid = advance(&triome, A, activate(3));
    assert_eq!(paid.execution().mana(A, ManaType::Colourless), 1.into());
    assert_eq!(
        paid.execution()
            .window()
            .cards()
            .get(id(&paid, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    let drawn = resolve(&paid);
    assert_eq!(
        drawn
            .execution()
            .window()
            .cards()
            .get(id(&drawn, 2))
            .unwrap()
            .zone(),
        ObjectZone::Hand(A)
    );
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn nonbasic_land_admission_excludes_unrepresented_optional_costs_and_restrictions() {
    let (inventory, _, _) = current_catalogue();
    for name in [
        "Stomping Ground",
        "Port Town",
        "Shivan Reef",
        "Sungrass Prairie",
        "Misty Rainforest",
        "Theorix Annex",
        "Selesnya Sanctuary",
    ] {
        let row = inventory
            .cards
            .iter()
            .find(|r| r.name == name && r.required_by_base_formats)
            .unwrap();
        assert!(
            basic_lands::compile(row, Profile::Current).is_none(),
            "{name}"
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn paid_source_boosts_stack_and_expire_and_a_campus_can_pay_to_scry() {
    let mut dragon = game(
        "Forest",
        &[("Shivan Dragon", A), ("Mountain", A), ("Mountain", A)],
        2,
        20,
    );
    let source = id(&dragon, 10);
    let activate = || GameCommand::ActivateUntargetedAbility {
        source,
        ability: ActivatedAbilityId(0),
        payment: Some(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Red,
                amount: 1.into(),
            }],
        }),
    };
    reject(&dragon, activate());
    for (mountain, total) in [(11, 6), (12, 7)] {
        dragon = advance(
            &dragon,
            A,
            GameCommand::ActivateMana {
                source: id(&dragon, mountain),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Mountain),
            },
        );
        dragon = advance(&dragon, A, activate());
        assert_eq!(dragon.execution().mana(A, ManaType::Red), 0.into());
        assert_eq!(
            dragon
                .execution()
                .power_toughness(source)
                .unwrap()
                .unwrap()
                .derived
                .power,
            (total - 1).into()
        );
        dragon = resolve(&dragon);
        assert_eq!(
            dragon
                .execution()
                .power_toughness(source)
                .unwrap()
                .unwrap()
                .derived
                .power,
            total.into()
        );
    }
    for _ in 0..32 {
        if dragon.turn().unwrap().position().number > 1 {
            break;
        }
        if let Some(frame) = dragon.pending_decision() {
            dragon = advance(
                &dragon,
                frame.chooser(),
                reply(&dragon, DecisionReply::DeclareAttackers(vec![])),
            );
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                dragon.execution().window().phase()
            else {
                panic!("priority")
            };
            dragon = advance(&dragon, holder, GameCommand::Pass);
        }
    }
    assert_eq!(dragon.turn().unwrap().position().number, 2);
    assert_eq!(
        dragon
            .execution()
            .power_toughness(source)
            .unwrap()
            .unwrap()
            .derived
            .power,
        5.into()
    );

    let campus = game("Forest", &[("Quandrix Campus", A)], 2, 20);
    let activate = |amount| GameCommand::ActivateUntargetedAbility {
        source: id(&campus, 10),
        ability: ActivatedAbilityId(0),
        payment: payment(amount),
    };
    reject(&campus, activate(3));
    let activated = advance(&campus, A, activate(4));
    assert_eq!(
        activated.execution().mana(A, ManaType::Colourless),
        0.into()
    );
    let scry = resolve(&activated);
    let RuleFrame::LibraryTop(frame) = scry.pending_decision().unwrap().frame() else {
        panic!("scry")
    };
    assert_eq!(frame.domain_top_first(), [id(&scry, 2)]);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn sol_ring_and_llanowar_elves_pay_their_prices_and_produce_printed_mana() {
    let mut ring = game("Sol Ring", &[], 2, 20);
    ring = advance(
        &ring,
        A,
        GameCommand::CastUntargeted {
            spell: id(&ring, 1),
            payment: payment(1),
        },
    );
    ring = resolve(&ring);
    let printed = ManaAbilityRef::Printed(ManaAbilityId(0));
    assert_eq!(ring.execution().mana(A, ManaType::Colourless), 3.into());
    ring = advance(&ring, A, production(&ring, printed));
    assert!(tapped(&ring));
    assert_eq!(ring.execution().mana(A, ManaType::Colourless), 5.into());
    reject(&ring, production(&ring, printed));

    let mut elves = game("Llanowar Elves", &[("Forest", A)], 2, 20);
    elves = advance(
        &elves,
        A,
        GameCommand::ActivateMana {
            source: id(&elves, 10),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
        },
    );
    elves = advance(
        &elves,
        A,
        GameCommand::CastUntargeted {
            spell: id(&elves, 1),
            payment: Some(ManaPayment {
                from_lots: Default::default(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Green,
                    amount: 1.into(),
                }],
            }),
        },
    );
    elves = resolve(&elves);
    assert_eq!(elves.execution().mana(A, ManaType::Green), 0.into());
    reject(&elves, production(&elves, printed));
    let ready = exile_hand::until(elves, |s| {
        s.turn().is_some_and(|turn| {
            turn.position().number == 3 && turn.position().step == Step::PrecombatMain
        })
    });
    let source = id(&ready, 1);
    let activation = GameCommand::ActivateMana {
        source,
        ability: printed,
    };
    let result = advance(&ready, A, activation.clone());
    assert_eq!(result.execution().mana(A, ManaType::Green), 1.into());
    reject(&result, activation);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_three_color_lands_enter_tapped_and_produce_each_printed_color() {
    use ManaType::{Black, Blue, Green, Red, White};
    let cases = [
        (
            "Frontier Bivouac",
            "e4cf6c2f-0f1e-4980-9ef9-e4eabcae42a9",
            "{G}, {U}, or {R}",
            [Green, Blue, Red],
        ),
        (
            "Jungle Shrine",
            "2e69537c-c898-4e13-a72d-ce3957a90304",
            "{R}, {G}, or {W}",
            [Red, Green, White],
        ),
        (
            "Mystic Monastery",
            "834b8f71-9a45-42ae-9e99-e749fa6fb45e",
            "{U}, {R}, or {W}",
            [Blue, Red, White],
        ),
        (
            "Nomad Outpost",
            "4619de7e-3d6e-4c6b-8e6e-e24db324839d",
            "{R}, {W}, or {B}",
            [Red, White, Black],
        ),
        (
            "Opulent Palace",
            "f9e7e855-1e3b-42d3-91b0-64ba8b5b8982",
            "{B}, {G}, or {U}",
            [Black, Green, Blue],
        ),
        (
            "Sandsteppe Citadel",
            "544dbabd-cbfc-40da-a5ba-2fea9cddb453",
            "{W}, {B}, or {G}",
            [White, Black, Green],
        ),
        (
            "Savage Lands",
            "a3292406-3f49-42d6-a547-e43dd5797f84",
            "{B}, {R}, or {G}",
            [Black, Red, Green],
        ),
        (
            "Seaside Citadel",
            "2ae77795-6a80-498b-bf69-6fd612f601e4",
            "{G}, {W}, or {U}",
            [Green, White, Blue],
        ),
    ];
    let ability = ManaAbilityRef::Printed(ManaAbilityId(0));
    for (name, oracle, text, colors) in cases {
        let (inventory, _, _) = current_catalogue();
        let row = inventory
            .cards
            .iter()
            .find(|row| row.oracle_id.as_deref() == Some(oracle))
            .unwrap();
        assert_eq!(row.name, name);
        assert_eq!(row.faces.len(), 1);
        assert_eq!(row.faces[0].type_line.as_deref(), Some("Land"));
        assert_eq!(
            row.faces[0].oracle_text.as_deref(),
            Some(format!("This land enters tapped.\n{{T}}: Add {text}.").as_str())
        );
        let before = game(name, &[], 2, 20);
        let transition = before
            .advance(GameInput {
                basis: before.basis().clone(),
                actor: A,
                command: GameCommand::PlayLand {
                    land: id(&before, 1),
                },
            })
            .unwrap();
        let receipt = transition.land_play().unwrap();
        assert_eq!(receipt.before.zone(), ObjectZone::Hand(A));
        assert_eq!(receipt.after.zone(), ObjectZone::Battlefield);
        let entered = transition.into_state();
        assert!(tapped(&entered), "{name}");
        reject(&entered, production(&entered, ability));
        let ready = exile_hand::until(entered, |state| {
            state.turn().is_some_and(|turn| {
                turn.position().number == 3 && turn.position().step == Step::PrecombatMain
            })
        });
        assert!(!tapped(&ready), "{name}");
        let initial = ManaType::ALL.map(|kind| ready.execution().mana(A, kind));
        let choosing = advance(&ready, A, production(&ready, ability));
        let decision = choosing.pending_decision().unwrap();
        assert_eq!(decision.chooser(), A);
        let RuleFrame::ManaColor(frame) = decision.frame() else {
            panic!("expected mana color")
        };
        assert_eq!(frame.colors(), &colors.into(), "{name}");
        assert_eq!(
            ManaType::ALL.map(|kind| choosing.execution().mana(A, kind)),
            initial
        );
        let illegal = ManaType::ALL
            .into_iter()
            .find(|kind| !colors.contains(kind))
            .unwrap();
        reject(
            &choosing,
            reply(&choosing, DecisionReply::ManaColor(illegal)),
        );
        for color in colors {
            let result = advance(
                &choosing,
                A,
                reply(&choosing, DecisionReply::ManaColor(color)),
            );
            assert!(tapped(&result));
            for (kind, amount) in ManaType::ALL.into_iter().zip(&initial) {
                assert_eq!(
                    result.execution().mana(A, kind),
                    amount.plus(&i64::from(kind == color).into()),
                    "{name}: {color:?}, {kind:?}"
                );
                assert_eq!(result.execution().mana(B, kind), 0.into());
            }
            reject(&result, production(&result, ability));
        }
    }
}

mod cmm_lands;

mod cmm_hybrid_mana;
mod cmm_paid_mana;
mod cmm_urza_mana;
mod commander_mana_empty;
