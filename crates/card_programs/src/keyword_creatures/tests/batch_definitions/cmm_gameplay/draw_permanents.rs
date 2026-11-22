//! Paid draw abilities with permanent, name, subtype and controller restrictions.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_azami_can_tap_itself_immediately_or_another_untapped_controlled_wizard_to_draw() {
    use spellwake_engine::activated::ActivationOrigin;
    let price = [(ManaType::Colourless, 2), (ManaType::Blue, 3)];
    let wizard = current_catalogue().1.resolve_label("Wizard").unwrap();
    let body = |n, seat| {
        seed(
            n,
            seat,
            ObjectDestination::Battlefield { controller: seat },
            Program::creature(None, 2.into(), 2.into(), [wizard].into()),
        )
    };
    let state = game(
        vec![A, B],
        vec![
            hand("Azami, Lady of Scrolls"),
            body(2, A),
            body(3, B),
            creature(4, A, 2, 2, &[]),
            named("Island", 5, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&state, &price);
    let tap = |s: &GameState, n| GameCommand::ActivateWithRestrictedMana {
        source: physical(s, 1).id(),
        ability: ActivatedAbilityId(0),
        granting_effect: None,
        origin: ActivationOrigin::Contributors(vec![physical(s, n).id()]),
        chosen_x: None,
        payment: None,
        restricted_mana: vec![],
    };
    for n in [3, 4] {
        illegal(&entered, A, tap(&entered, n));
    }
    let mut state = entered;
    for (n, drawn) in [(1, 5), (2, 100)] {
        let paid = command(&state, A, tap(&state, n)).into_state();
        assert_eq!(physical(&paid, n).tapped(), Some(true));
        illegal(&paid, A, tap(&paid, n));
        state = round(paid).into_state();
        assert_eq!(physical(&state, drawn).zone(), ObjectZone::Hand(A));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_endless_atlas_needs_three_controlled_lands_with_the_same_name() {
    let cost = [(ManaType::Colourless, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Endless Atlas"),
            named(
                "Island",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Island",
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Forest",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Island",
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named("Island", 6, A, ObjectDestination::Hand),
        ],
        &[(ManaType::Colourless, 4)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &cost);
    let activate = |s: &GameState| GameCommand::ActivateUntargetedAbility {
        source: physical(s, 1).id(),
        ability: ActivatedAbilityId(0),
        payment: Some(spend(&cost)),
    };
    illegal(&entered, A, activate(&entered));
    let played = command(
        &entered,
        A,
        GameCommand::PlayLand {
            land: physical(&entered, 6).id(),
        },
    )
    .into_state();
    let stacked = command(&played, A, activate(&played)).into_state();
    assert_eq!(physical(&stacked, 1).tapped(), Some(true));
    assert_eq!(stacked.execution().mana(A, ManaType::Colourless), 0.into());
    let done = round(stacked);
    assert_eq!(physical(done.state(), 100).zone(), ObjectZone::Hand(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_bonders_ornament_chooses_mana_or_draws_once_for_each_player_with_its_name() {
    use ManaType::*;
    let price = [(Colourless, 3)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Bonder's Ornament"),
            named(
                "Bonder's Ornament",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Bonder's Ornament",
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
        ],
        &[(Colourless, 7)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let mana = command(
        &entered,
        A,
        GameCommand::ActivateMana {
            source: physical(&entered, 1).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    for color in [White, Blue, Black, Red, Green] {
        let done = reply(&mana, DecisionReply::ManaColor(color));
        assert_eq!(done.state().execution().mana(A, color), 1.into());
        assert_eq!(physical(done.state(), 1).tapped(), Some(true));
    }
    let stack = command(
        &entered,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: physical(&entered, 1).id(),
            ability: ActivatedAbilityId(1),
            payment: Some(spend(&[(Colourless, 4)])),
        },
    )
    .into_state();
    assert_eq!(stack.execution().mana(A, Colourless), 0.into());
    let drawn = round(stack);
    assert_eq!(drawn.draw_journal().events.len(), 2);
    for seat in [A, B] {
        assert_eq!(
            physical(drawn.state(), 100 + u64::from(seat.0)).zone(),
            ObjectZone::Hand(seat)
        );
    }
    assert_eq!(physical(drawn.state(), 102).zone(), ObjectZone::Library(C));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_geier_reach_sanitarium_produces_colorless_or_draws_then_discards_for_each_player() {
    let state = game(
        vec![A, B, C],
        vec![
            hand("Geier Reach Sanitarium"),
            named("Island", 2, B, ObjectDestination::Hand),
        ],
        &[(ManaType::Colourless, 2)],
        Step::PrecombatMain,
    );
    let played = command(
        &state,
        A,
        GameCommand::PlayLand {
            land: physical(&state, 1).id(),
        },
    )
    .into_state();
    let mana = command(
        &played,
        A,
        GameCommand::ActivateMana {
            source: physical(&played, 1).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    );
    assert_eq!(
        mana.state().execution().mana(A, ManaType::Colourless),
        3.into()
    );
    let stack = command(
        &played,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: physical(&played, 1).id(),
            ability: ActivatedAbilityId(0),
            payment: Some(spend(&[(ManaType::Colourless, 2)])),
        },
    )
    .into_state();
    assert_eq!(physical(&stack, 1).tapped(), Some(true));
    let mut s = round(stack).into_state();
    while let Some(decision) = s.pending_decision() {
        let RuleFrame::ResolutionSelect(frame) = decision.frame() else {
            panic!("expected discard decision");
        };
        let n = match frame.chooser() {
            A => 100,
            B => 2,
            C => 102,
            _ => unreachable!(),
        };
        s = reply(
            &s,
            DecisionReply::SelectResolution(vec![physical(&s, n).id()]),
        )
        .into_state();
    }
    for (n, seat) in [(100, A), (2, B), (102, C)] {
        assert_eq!(physical(&s, n).zone(), ObjectZone::Graveyard(seat));
    }
    assert_eq!(physical(&s, 101).zone(), ObjectZone::Hand(B));
}
