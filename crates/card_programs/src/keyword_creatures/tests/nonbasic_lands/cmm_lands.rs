//! Commander Masters lands played through their dated registry definitions.
use super::*;

fn next_main(state: GameState) -> GameState {
    exile_hand::until(state, |state| {
        let p = state.turn().unwrap().position();
        p.number == 3 && p.step == Step::PrecombatMain
    })
}

fn check_output(state: &GameState, result: &GameState, color: ManaType, ability: ManaAbilityRef) {
    for kind in ManaType::ALL {
        let previous = state.execution().mana(A, kind);
        assert_eq!(
            result.execution().mana(A, kind),
            previous.plus(&i64::from(color == kind).into())
        );
        assert_eq!(
            result.execution().mana(B, kind),
            state.execution().mana(B, kind)
        );
    }
    assert!(tapped(result));
    reject(result, production(result, ability));
}

fn printed_colors(state: &GameState, colors: [ManaType; 2]) {
    let printed = ManaAbilityRef::Printed(ManaAbilityId(0));
    let choosing = advance(state, A, production(state, printed));
    let RuleFrame::ManaColor(frame) = choosing.pending_decision().unwrap().frame() else {
        panic!("expected printed mana choice")
    };
    assert_eq!(frame.colors(), &colors.into());
    for kind in ManaType::ALL {
        let command = reply(&choosing, DecisionReply::ManaColor(kind));
        if colors.contains(&kind) {
            check_output(state, &advance(&choosing, A, command), kind, printed);
        } else {
            reject(&choosing, command);
        }
    }
}

fn intrinsic_colors(state: &GameState, types: [BasicLandType; 2]) {
    for kind in BasicLandType::ALL {
        let ability = ManaAbilityRef::BasicLand(kind);
        if types.contains(&kind) {
            let result = advance(state, A, production(state, ability));
            check_output(state, &result, kind.mana_type(), ability);
        } else {
            reject(state, production(state, ability));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_land_games_count_only_controlled_basic_lands_and_produce_intrinsic_colors() {
    use BasicLandType::*;
    for (name, types) in [
        ("Canopy Vista", [Forest, Plains]),
        ("Cinder Glade", [Mountain, Forest]),
        ("Prairie Stream", [Plains, Island]),
        ("Smoldering Marsh", [Swamp, Mountain]),
        ("Sunken Hollow", [Island, Swamp]),
    ] {
        let blocked = play(&game(
            name,
            &[("Forest", A), ("Tundra", A), ("Island", B)],
            2,
            20,
        ));
        assert!(tapped(&blocked), "{name}");
        reject(
            &blocked,
            production(&blocked, ManaAbilityRef::BasicLand(types[0])),
        );
        let ready = play(&game(name, &[("Forest", A), ("Island", A)], 2, 20));
        assert!(!tapped(&ready), "{name}");
        intrinsic_colors(&ready, types);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_land_games_count_opponents_and_offer_only_the_two_printed_colors() {
    use ManaType::*;
    for (name, colors) in [
        ("Rejuvenating Springs", [Green, Blue]),
        ("Spectator Seating", [Red, White]),
        ("Training Center", [Blue, Red]),
        ("Undergrowth Stadium", [Black, Green]),
        ("Vault of Champions", [White, Black]),
    ] {
        let blocked = play(&game(name, &[], 2, 20));
        assert!(tapped(&blocked), "{name}");
        reject(
            &blocked,
            production(&blocked, ManaAbilityRef::Printed(ManaAbilityId(0))),
        );
        let ready = play(&game(name, &[], 3, 20));
        assert!(!tapped(&ready), "{name}");
        printed_colors(&ready, colors);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_land_games_cycle_for_two_or_play_tapped_then_use_either_intrinsic_color() {
    use BasicLandType::*;
    for (name, types) in [
        ("Irrigated Farmland", [Plains, Island]),
        ("Scattered Groves", [Forest, Plains]),
        ("Sheltered Thicket", [Mountain, Forest]),
    ] {
        let initial = game(name, &[], 2, 20);
        let cycle = |price| GameCommand::Cycle {
            source: id(&initial, 1),
            ability: ActivatedAbilityId(0),
            payment: payment(price),
        };
        reject(&initial, cycle(1));
        let paid = advance(&initial, A, cycle(2));
        assert_eq!(paid.execution().mana(A, ManaType::Colourless), 2.into());
        assert_eq!(
            paid.execution()
                .window()
                .cards()
                .get(id(&paid, 1))
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(A)
        );
        assert_eq!(
            paid.execution().window().cards().library(A),
            &[id(&paid, 2)]
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
        let entered = play(&initial);
        assert!(tapped(&entered));
        reject(
            &entered,
            production(&entered, ManaAbilityRef::BasicLand(types[0])),
        );
        intrinsic_colors(&next_main(entered), types);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_land_games_temple_scry_choices_empty_library_and_printed_mana() {
    use ManaType::*;
    for (name, colors) in [
        ("Temple of Enlightenment", [White, Blue]),
        ("Temple of Epiphany", [Blue, Red]),
        ("Temple of Malady", [Black, Green]),
        ("Temple of Plenty", [Green, White]),
        ("Temple of Silence", [White, Black]),
        ("Temple of Triumph", [Red, White]),
    ] {
        let entered = play(&game_with_library(name, &[], 2, 20, &["Forest", "Island"]));
        assert!(tapped(&entered));
        reject(
            &entered,
            production(&entered, ManaAbilityRef::Printed(ManaAbilityId(0))),
        );
        assert_eq!(entered.execution().window().entries().len(), 1);
        let choice = resolve(&entered);
        let d = choice.pending_decision().unwrap();
        assert_eq!(d.chooser(), A);
        let RuleFrame::LibraryTop(frame) = d.frame() else {
            panic!("expected scry")
        };
        let library = choice.execution().window().cards().library(A).to_vec();
        assert_eq!(frame.domain_top_first(), &library[..1]);
        for keep in [true, false] {
            let (top_first, bottom_top_first) = if keep {
                (vec![library[0]], vec![])
            } else {
                (vec![], vec![library[0]])
            };
            let command = reply(
                &choice,
                DecisionReply::LibraryTop(LibraryTopReply::Scry {
                    top_first,
                    bottom_top_first,
                }),
            );
            assert!(
                choice
                    .advance(GameInput {
                        basis: choice.basis().clone(),
                        actor: B,
                        command: command.clone()
                    })
                    .is_err()
            );
            let result = advance(&choice, A, command);
            let expected = if keep {
                library.clone()
            } else {
                vec![library[1], library[0]]
            };
            assert_eq!(
                result.execution().window().cards().library(A),
                expected.as_slice()
            );
            printed_colors(&next_main(result), colors);
        }
        let empty = resolve(&play(&game_with_library(name, &[], 2, 20, &[])));
        assert!(empty.pending_decision().is_none());
        assert!(empty.execution().window().cards().library(A).is_empty());
        assert!(empty.execution().window().entries().is_empty());
    }
}
