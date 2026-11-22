//! Source-bound phasing at the normal turn and land-play entry points.

use super::{
    cmm_gameplay::{creature, named},
    *,
};
use spellwake_engine::{
    activated::ActivatedAbilityId,
    characteristics::KeywordAbility::{DoubleStrike, Flying, Islandwalk, Phasing},
    mana::{ManaAbilityId, ManaAbilityRef},
};

fn main_phase(state: GameState, number: u64) -> GameState {
    exile_hand::until(state, |state| {
        let turn = state.turn().unwrap().position();
        turn.number == number && turn.step == Step::PrecombatMain
    })
}

fn library() -> Vec<SeedCard> {
    [(A, 20), (B, 30)]
        .into_iter()
        .flat_map(|(owner, first)| {
            (first..first + 5)
                .map(move |id| named("Island", id, owner, ObjectDestination::LibraryBottom))
        })
        .collect()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn printed_phasing_creatures_alternate_visibility_and_keep_their_incarnation() {
    for (name, generic, other_ability) in [
        ("Breezekeeper", 3, Some(Flying)),
        ("Merfolk Raiders", 1, Some(Islandwalk)),
        ("Sandbar Crocodile", 4, None),
        ("Teferi's Drake", 2, Some(Flying)),
        ("Tolarian Drake", 2, Some(Flying)),
    ] {
        let mut cards = library();
        cards.push(named(name, 1, A, ObjectDestination::Hand));
        let price = [(ManaType::Colourless, generic), (ManaType::Blue, 1)];
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let entered = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
        let id = physical(&entered, 1).id();
        let sheet = entered
            .execution()
            .current_characteristics(id)
            .unwrap()
            .unwrap();
        assert!(sheet.has_keyword(Phasing), "{name}");
        if let Some(ability) = other_ability {
            assert!(sheet.has_keyword(ability), "{name}");
        }
        let first_away = main_phase(entered, 3);
        let other_player = main_phase(first_away.clone(), 4);
        for state in [&first_away, &other_player] {
            let cards = state.execution().window().cards();
            assert!(cards.get(id).is_none(), "{name}");
            assert_eq!(cards.current_identity(CardId(1)), Some(id), "{name}");
            assert_eq!(cards.phased_out_under(id), Some(A), "{name}");
        }
        let back = main_phase(other_player, 5);
        assert_eq!(physical(&back, 1).id(), id, "{name}");
        assert_eq!(physical(&back, 1).zone(), ObjectZone::Battlefield, "{name}");
        assert_eq!(physical(&back, 1).tapped(), Some(false), "{name}");
        let away_again = main_phase(back, 7);
        assert_eq!(
            away_again.execution().window().cards().phased_out_under(id),
            Some(A)
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn teferis_isle_phases_out_as_an_equipped_drake_phases_in_then_adds_two_blue() {
    let mut cards = library();
    cards.extend([
        named("Teferi's Drake", 1, A, ObjectDestination::Hand),
        named("Teferi's Isle", 2, A, ObjectDestination::Hand),
        named(
            "Fireshrieker",
            3,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
    ]);
    let price = [(ManaType::Colourless, 2), (ManaType::Blue, 1)];
    let initial = game(
        vec![A, B],
        cards,
        &[(ManaType::Colourless, 4), (ManaType::Blue, 1)],
        Step::PrecombatMain,
    );
    let cast = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    let drake = physical(&cast, 1).id();
    let equipment = physical(&cast, 3).id();
    let attach = command(
        &cast,
        A,
        GameCommand::ActivateAbility {
            source: equipment,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(drake),
            payment: Some(spend(&[(ManaType::Colourless, 2)])),
        },
    );
    let equipped = round(attach.into_state()).into_state();
    assert_eq!(
        equipped.execution().attached_to(equipment),
        Some(TargetRef::Object(drake))
    );
    let disappeared = main_phase(equipped, 3);
    for object in [drake, equipment] {
        assert_eq!(
            disappeared
                .execution()
                .window()
                .cards()
                .phased_out_under(object),
            Some(A)
        );
    }
    let land_play = command(
        &disappeared,
        A,
        GameCommand::PlayLand {
            land: physical(&disappeared, 2).id(),
        },
    )
    .into_state();
    let isle = physical(&land_play, 2).id();
    assert_eq!(physical(&land_play, 2).tapped(), Some(true));
    let tap = GameCommand::ActivateMana {
        source: isle,
        ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
    };
    assert!(
        land_play
            .advance(GameInput {
                basis: land_play.basis().clone(),
                actor: A,
                command: tap.clone()
            })
            .is_err()
    );
    let switched = main_phase(land_play, 5);
    assert_eq!(
        switched.execution().window().cards().phased_out_under(isle),
        Some(A)
    );
    assert_eq!(physical(&switched, 1).id(), drake);
    assert_eq!(physical(&switched, 3).id(), equipment);
    assert_eq!(
        switched.execution().attached_to(equipment),
        Some(TargetRef::Object(drake))
    );
    assert!(
        switched
            .execution()
            .current_characteristics(drake)
            .unwrap()
            .unwrap()
            .has_keyword(DoubleStrike)
    );
    let available = main_phase(switched, 7);
    assert_eq!(physical(&available, 2).id(), isle);
    assert_eq!(physical(&available, 2).tapped(), Some(false));
    assert_eq!(
        available
            .execution()
            .window()
            .cards()
            .phased_out_under(drake),
        Some(A)
    );
    let tapped = command(&available, A, tap).into_state();
    assert_eq!(tapped.execution().mana(A, ManaType::Blue), 2.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn teferis_imp_discards_when_phasing_out_and_draws_when_phasing_in() {
    let mut cards = library();
    cards.extend([
        named("Teferi's Imp", 1, A, ObjectDestination::Hand),
        named("Forest", 2, A, ObjectDestination::Hand),
    ]);
    let price = [(ManaType::Colourless, 2), (ManaType::Blue, 1)];
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let cast = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    let imp = physical(&cast, 1).id();
    assert!(
        cast.execution()
            .current_characteristics(imp)
            .unwrap()
            .unwrap()
            .has_keyword(Flying)
    );
    let away = main_phase(cast, 3);
    assert_eq!(
        away.execution().window().cards().phased_out_under(imp),
        Some(A)
    );
    assert_eq!(physical(&away, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&away, 20).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&away, 21).zone(), ObjectZone::Library(A));
    let back = main_phase(away, 5);
    assert_eq!(physical(&back, 1).id(), imp);
    assert_eq!(physical(&back, 21).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&back, 22).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&back, 23).zone(), ObjectZone::Library(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shimmering_efreet_phases_out_a_target_that_returns_on_its_controllers_turn() {
    let mut cards = library();
    cards.extend([
        named("Shimmering Efreet", 1, A, ObjectDestination::Hand),
        creature(2, B, 2, 2, &[]),
        named(
            "Forest",
            3,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
    ]);
    let price = [(ManaType::Colourless, 2), (ManaType::Blue, 1)];
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let cast = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    let cohort = main_phase(cast, 3);
    let choosing = exile_hand::until(cohort, |state| {
        matches!(
            state.pending_decision().map(|decision| decision.frame()),
            Some(RuleFrame::TriggerTarget(_))
        )
    });
    assert_eq!(choosing.turn().unwrap().position().step, Step::Upkeep);
    let target = physical(&choosing, 2).id();
    let land = physical(&choosing, 3).id();
    let decision = choosing.pending_decision().unwrap();
    assert!(
        choosing
            .advance(GameInput {
                basis: choosing.basis().clone(),
                actor: A,
                command: GameCommand::Reply {
                    decision: decision.id(),
                    choice: DecisionReply::TriggerTarget(TargetRef::Object(land)),
                },
            })
            .is_err()
    );
    let stacked = reply(
        &choosing,
        DecisionReply::TriggerTarget(TargetRef::Object(target)),
    );
    let phased = round(stacked.into_state()).into_state();
    assert_eq!(
        phased.execution().window().cards().phased_out_under(target),
        Some(B)
    );
    assert_eq!(physical(&phased, 3).id(), land);
    assert_eq!(physical(&main_phase(phased, 6), 2).id(), target);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn taniwha_phases_out_only_its_controllers_lands_during_its_upkeep() {
    let mut cards = library();
    cards.push(named("Taniwha", 1, A, ObjectDestination::Hand));
    for (id, owner) in [(2, A), (3, A), (4, B)] {
        cards.push(named(
            "Island",
            id,
            owner,
            ObjectDestination::Battlefield { controller: owner },
        ));
    }
    let price = [(ManaType::Colourless, 3), (ManaType::Blue, 2)];
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let cast = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    let serpent = physical(&cast, 1).id();
    let lands = [physical(&cast, 2).id(), physical(&cast, 3).id()];
    let before = main_phase(cast, 3);
    assert_eq!(
        before
            .execution()
            .window()
            .cards()
            .phased_out_under(serpent),
        Some(A)
    );
    for (card, id) in [2, 3].into_iter().zip(lands) {
        assert_eq!(physical(&before, card).id(), id);
    }
    let resolved = main_phase(before, 5);
    assert_eq!(physical(&resolved, 1).id(), serpent);
    assert!(
        resolved
            .execution()
            .current_characteristics(serpent)
            .unwrap()
            .unwrap()
            .has_keyword(KeywordAbility::Trample)
    );
    for land in lands {
        assert_eq!(
            resolved.execution().window().cards().phased_out_under(land),
            Some(A)
        );
    }
    assert_eq!(physical(&resolved, 4).zone(), ObjectZone::Battlefield);
    let returned = main_phase(resolved, 7);
    for (card, id) in [2, 3].into_iter().zip(lands) {
        assert_eq!(physical(&returned, card).id(), id);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn warping_wurm_can_pay_to_remain_and_grows_after_phasing_back_in() {
    use spellwake_engine::effects::ResolutionPaymentReply;
    use spellwake_engine::land::BasicLandType::{Forest, Island};

    let mut cards = library();
    cards.push(named(
        "Warping Wurm",
        1,
        A,
        ObjectDestination::Battlefield { controller: A },
    ));
    for (id, land) in [(2, "Forest"), (3, "Forest"), (4, "Island"), (5, "Island")] {
        cards.push(named(
            land,
            id,
            A,
            ObjectDestination::Battlefield { controller: A },
        ));
    }
    let state = game(vec![A, B], cards, &[], Step::PrecombatMain);
    let wurm = physical(&state, 1).id();
    let phased = main_phase(state, 3);
    assert_eq!(
        phased.execution().window().cards().phased_out_under(wurm),
        Some(A)
    );
    let mut state = exile_hand::until(phased, |game| {
        game.turn()
            .is_some_and(|turn| turn.position().step == Step::Upkeep && turn.position().number == 5)
    });
    for _ in 0..6 {
        state = match state.pending_decision().map(|decision| decision.frame()) {
            Some(RuleFrame::ResolutionPayment(_)) => break,
            Some(RuleFrame::Triggers(frame)) => reply(
                &state,
                DecisionReply::OrderTriggers(frame.instances().to_vec()),
            )
            .into_state(),
            None => pass(&state).into_state(),
            other => panic!("unexpected decision {other:?}"),
        };
    }
    assert!(matches!(
        state.pending_decision().map(|p| p.frame()),
        Some(RuleFrame::ResolutionPayment(_))
    ));
    let declined = reply(
        &state,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    )
    .into_state();
    assert_eq!(
        declined.execution().window().cards().phased_out_under(wurm),
        Some(A)
    );

    let mut paid = state;
    for (id, kind) in [(2, Forest), (3, Forest), (4, Island), (5, Island)] {
        paid = command(
            &paid,
            A,
            GameCommand::ActivateMana {
                source: physical(&paid, id).id(),
                ability: ManaAbilityRef::BasicLand(kind),
            },
        )
        .into_state();
    }
    let paid = reply(
        &paid,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(&[
            (ManaType::Green, 2),
            (ManaType::Blue, 2),
        ]))),
    )
    .into_state();
    let reached_main = main_phase(paid, 5);
    assert_eq!(physical(&reached_main, 1).id(), wurm);
    let creature = reached_main.execution().creature(wurm).unwrap().unwrap();
    assert_eq!(creature.power, 2.into());
    assert_eq!(creature.toughness, 2.into());
}
