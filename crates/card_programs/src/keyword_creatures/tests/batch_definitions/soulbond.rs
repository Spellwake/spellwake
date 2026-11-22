//! Actual Oracle cards using both soulbond triggers and continuous pair rewards.
use super::{cmm_gameplay::named, *};
use spellwake_engine::characteristics::KeywordAbility::{DoubleStrike, Flying};

fn choose_partner(mut state: GameState) -> GameState {
    for _ in 0..10 {
        state = match state.pending_decision().map(|pending| pending.frame()) {
            Some(RuleFrame::ResolutionSelect(frame))
                if frame.operation() == ResolutionSelectionKind::Soulbond =>
            {
                return state;
            }
            Some(RuleFrame::Triggers(frame)) => reply(
                &state,
                DecisionReply::OrderTriggers(frame.instances().to_vec()),
            )
            .into_state(),
            None => pass(&state).into_state(),
            other => panic!("unexpected decision {other:?}"),
        };
    }
    panic!("no soulbond choice");
}

fn candidates(game: &GameState) -> &[ObjectId] {
    let RuleFrame::ResolutionSelect(frame) = game.pending_decision().unwrap().frame() else {
        panic!("expected soulbond choice");
    };
    assert_eq!(frame.operation(), ResolutionSelectionKind::Soulbond);
    assert_eq!(frame.minimum(), 0);
    assert_eq!(frame.required(), 1);
    frame.domain()
}

fn keyword(game: &GameState, card: u64, ability: KeywordAbility) -> bool {
    game.execution()
        .current_characteristics(physical(game, card).id())
        .unwrap()
        .unwrap()
        .has_keyword(ability)
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn wingcrafter_can_choose_an_existing_creature_or_decline_without_targeting() {
    let cards = vec![
        named("Wingcrafter", 1, A, ObjectDestination::Hand),
        named(
            "Grizzly Bears",
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named(
            "Grizzly Bears",
            3,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
    ];
    let price = [(ManaType::Blue, 1)];
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let choose = choose_partner(cast_untargeted(&initial, 1, &price).into_state());
    let partner = physical(&choose, 2).id();
    assert_eq!(candidates(&choose), [partner]);
    let declined = reply(&choose, DecisionReply::SelectResolution(vec![])).into_state();
    assert!(!keyword(&declined, 1, Flying));
    assert!(!keyword(&declined, 2, Flying));
    let paired = reply(&choose, DecisionReply::SelectResolution(vec![partner]));
    assert!(keyword(paired.state(), 1, Flying));
    assert!(keyword(paired.state(), 2, Flying));
    assert!(!keyword(paired.state(), 3, Flying));
    assert!(!events(&paired).any(|event| {
        event
            .occurrences
            .iter()
            .any(|row| matches!(row.value, RulesOccurrence::PermanentTargeted { .. }))
    }));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn double_soulbond_pair_shares_both_keywords_and_a_partner_leaving_clears_the_grants() {
    let cards = vec![
        named(
            "Wingcrafter",
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named("Silverblade Paladin", 2, A, ObjectDestination::Hand),
        named("Unsummon", 3, A, ObjectDestination::Hand),
    ];
    let price = [(ManaType::Colourless, 1), (ManaType::White, 2)];
    let initial = game(
        vec![A, B],
        cards,
        &[
            (ManaType::Colourless, 1),
            (ManaType::White, 2),
            (ManaType::Blue, 1),
        ],
        Step::PrecombatMain,
    );
    let choose = choose_partner(cast_untargeted(&initial, 2, &price).into_state());
    assert_eq!(candidates(&choose).len(), 1);
    let paired = reply(
        &choose,
        DecisionReply::SelectResolution(candidates(&choose).to_vec()),
    )
    .into_state();
    let idle = exile_hand::until(paired, |state| {
        state.pending_decision().is_none() && state.execution().window().entries().is_empty()
    });
    for card in [1, 2] {
        assert!(keyword(&idle, card, Flying));
        assert!(keyword(&idle, card, DoubleStrike));
    }
    let paladin = physical(&idle, 2).id();
    let bounced = round(
        command(
            &idle,
            A,
            GameCommand::CastWithResources {
                spell: physical(&idle, 3).id(),
                target: Some(TargetRef::Object(paladin)),
                payment: CastingPayment {
                    mana: Some(spend(&[(ManaType::Blue, 1)])),
                    ..Default::default()
                },
            },
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(physical(&bounced, 2).zone(), ObjectZone::Hand(A));
    assert_ne!(physical(&bounced, 2).id(), paladin);
    assert!(!keyword(&bounced, 1, Flying));
    assert!(!keyword(&bounced, 1, DoubleStrike));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn druid_familiar_pairs_only_with_the_new_entrant_and_both_get_two_two() {
    let cards = vec![
        named(
            "Druid's Familiar",
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named(
            "Grizzly Bears",
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named("Grizzly Bears", 3, A, ObjectDestination::Hand),
    ];
    let price = [(ManaType::Colourless, 1), (ManaType::Green, 1)];
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let choose = choose_partner(cast_untargeted(&initial, 3, &price).into_state());
    let entrant = physical(&choose, 3).id();
    assert_eq!(candidates(&choose), [entrant]);
    let paired = reply(&choose, DecisionReply::SelectResolution(vec![entrant])).into_state();
    for (card, size) in [(1, 4), (2, 2), (3, 4)] {
        let view = paired
            .execution()
            .creature(physical(&paired, card).id())
            .unwrap()
            .unwrap();
        assert_eq!(view.power, size.into());
        assert_eq!(view.toughness, size.into());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn a_phased_out_partner_breaks_soulbond_and_phasing_in_does_not_pair_it_again() {
    let mut cards = vec![
        named("Nearheath Pilgrim", 1, A, ObjectDestination::Hand),
        named(
            "Teferi's Drake",
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
    ];
    for (owner, offset) in [(A, 20), (B, 30)] {
        cards.extend(
            (offset..offset + 5)
                .map(|id| named("Forest", id, owner, ObjectDestination::LibraryBottom)),
        );
    }
    let price = [(ManaType::Colourless, 1), (ManaType::White, 1)];
    let state = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let choose = choose_partner(cast_untargeted(&state, 1, &price).into_state());
    let drake = physical(&choose, 2).id();
    let paired = reply(&choose, DecisionReply::SelectResolution(vec![drake])).into_state();
    for id in [1, 2] {
        assert!(keyword(&paired, id, KeywordAbility::Lifelink));
    }
    let phased = exile_hand::until(paired, |state| {
        state.execution().window().cards().phased_out_under(drake) == Some(A)
    });
    assert!(!keyword(&phased, 1, KeywordAbility::Lifelink));
    let returned = exile_hand::until(phased, |state| {
        state.turn().is_some_and(|turn| {
            turn.position().number == 5 && turn.position().step == Step::PrecombatMain
        })
    });
    assert_eq!(physical(&returned, 2).id(), drake);
    for id in [1, 2] {
        assert!(!keyword(&returned, id, KeywordAbility::Lifelink));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn printed_soulbond_rewards_apply_to_each_creature_in_the_pair() {
    use KeywordAbility::{
        Deathtouch, DoubleStrike, FirstStrike, Flying, Haste, Hexproof, Lifelink, Reach, Trample,
        Vigilance,
    };
    use ManaType::{Blue, Colourless, Green, Red, White};

    for (name, generic, colored, count, grant) in [
        ("Wingcrafter", 0, Blue, 1, Flying),
        ("Spectral Gateguards", 4, White, 1, Vigilance),
        ("Nearheath Pilgrim", 1, White, 1, Lifelink),
        ("Hanweir Lancer", 2, Red, 1, FirstStrike),
        ("Lightning Mauler", 1, Red, 1, Haste),
        ("Geist Trappers", 4, Green, 1, Reach),
        ("Elgaud Shieldmate", 3, Blue, 1, Hexproof),
        ("Pathbreaker Wurm", 4, Green, 2, Trample),
        ("Nightshade Peddler", 1, Green, 1, Deathtouch),
        ("Silverblade Paladin", 1, White, 2, DoubleStrike),
    ] {
        let price = [(Colourless, generic), (colored, count)]
            .into_iter()
            .filter(|(_, amount)| *amount > 0)
            .collect::<Vec<_>>();
        let cards = vec![
            named(name, 1, A, ObjectDestination::Hand),
            named(
                "Grizzly Bears",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ];
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let choose = choose_partner(cast_untargeted(&initial, 1, &price).into_state());
        let chosen = physical(&choose, 2).id();
        assert_eq!(candidates(&choose), [chosen], "{name}");
        let paired = reply(&choose, DecisionReply::SelectResolution(vec![chosen])).into_state();
        assert!(keyword(&paired, 1, grant), "{name}");
        assert!(keyword(&paired, 2, grant), "{name}");
    }
}
