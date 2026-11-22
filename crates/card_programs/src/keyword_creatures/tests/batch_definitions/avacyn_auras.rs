//! Printed Avacyn Restored Auras combining an attachment with existing rules.
use super::{cmm_gameplay::named, *};
use spellwake_engine::mana::ManaAbilityRef;

fn enchant(state: &GameState, id: u64, host: ObjectId, costs: &[(ManaType, i64)]) -> GameState {
    round(
        command(
            state,
            A,
            GameCommand::CastWithResources {
                spell: physical(state, id).id(),
                target: Some(TargetRef::Object(host)),
                payment: CastingPayment {
                    mana: Some(spend(costs)),
                    ..Default::default()
                },
            },
        )
        .into_state(),
    )
    .into_state()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn spirit_away_changes_control_and_grants_flying_and_stats_until_the_aura_leaves() {
    let cards = vec![
        named("Spirit Away", 1, A, ObjectDestination::Hand),
        named(
            "Grizzly Bears",
            2,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
        named("Naturalize", 3, A, ObjectDestination::Hand),
    ];
    let initial = game(
        vec![A, B],
        cards,
        &[
            (ManaType::Colourless, 6),
            (ManaType::Blue, 2),
            (ManaType::Green, 1),
        ],
        Step::PrecombatMain,
    );
    let host = physical(&initial, 2).id();
    let owned = enchant(
        &initial,
        1,
        host,
        &[(ManaType::Colourless, 5), (ManaType::Blue, 2)],
    );
    let aura = physical(&owned, 1).id();
    let current = |game: &GameState| {
        game.execution()
            .current_characteristics(host)
            .unwrap()
            .unwrap()
    };
    let stolen = current(&owned);
    assert_eq!(stolen.controller, Some(A));
    assert!(stolen.has_keyword(KeywordAbility::Flying));
    assert_eq!(
        owned.execution().creature(host).unwrap().unwrap().power,
        4.into()
    );
    let destroyed = enchant(
        &owned,
        3,
        aura,
        &[(ManaType::Colourless, 1), (ManaType::Green, 1)],
    );
    assert_eq!(physical(&destroyed, 1).zone(), ObjectZone::Graveyard(A));
    let returned = current(&destroyed);
    assert_eq!(returned.controller, Some(B));
    assert!(!returned.has_keyword(KeywordAbility::Flying));
    assert_eq!(
        destroyed.execution().creature(host).unwrap().unwrap().power,
        2.into()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn abundant_growth_draws_and_grants_mana_to_its_enchanted_land() {
    let cards = vec![
        named("Abundant Growth", 1, A, ObjectDestination::Hand),
        named(
            "Forest",
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named("Mountain", 3, A, ObjectDestination::LibraryBottom),
        named(
            "Island",
            4,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
    ];
    let price = [(ManaType::Green, 1)];
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let forest = physical(&initial, 2).id();
    let attached = enchant(&initial, 1, forest, &price);
    let drawn = round(attached).into_state();
    assert_eq!(physical(&drawn, 3).zone(), ObjectZone::Hand(A));
    let key = *drawn
        .execution()
        .current_characteristics(forest)
        .unwrap()
        .unwrap()
        .granted_any_color_mana
        .first()
        .unwrap();
    let activated = command(
        &drawn,
        A,
        GameCommand::ActivateMana {
            source: forest,
            ability: ManaAbilityRef::GrantedAnyColor(key),
        },
    )
    .into_state();
    let produced = reply(&activated, DecisionReply::ManaColor(ManaType::Blue)).into_state();
    assert_eq!(produced.execution().mana(A, ManaType::Blue), 1.into());
    assert_eq!(physical(&produced, 2).tapped(), Some(true));
    let other = physical(&produced, 4).id();
    assert!(
        produced
            .execution()
            .current_characteristics(other)
            .unwrap()
            .unwrap()
            .granted_any_color_mana
            .is_empty()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cloudshift_and_acrobatic_maneuver_apply_the_printed_return_controller() {
    for (spell_name, price, controller, draws) in [
        ("Cloudshift", vec![(ManaType::White, 1)], A, false),
        (
            "Acrobatic Maneuver",
            vec![(ManaType::Colourless, 2), (ManaType::White, 1)],
            B,
            true,
        ),
    ] {
        let cards = vec![
            named(spell_name, 1, A, ObjectDestination::Hand),
            named(
                "Grizzly Bears",
                2,
                B,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Forest", 3, A, ObjectDestination::LibraryBottom),
        ];
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let before = physical(&initial, 2).id();
        let cast = command(
            &initial,
            A,
            GameCommand::CastWithResources {
                spell: physical(&initial, 1).id(),
                target: None,
                payment: CastingPayment {
                    mana: Some(spend(&price)),
                    permanent_targets: vec![TargetRef::Object(before)],
                    ..Default::default()
                },
            },
        )
        .into_state();
        let resolved = round(cast);
        let after = physical(resolved.state(), 2);
        assert_ne!(after.id(), before);
        assert_eq!(after.zone(), ObjectZone::Battlefield);
        assert_eq!(after.owner(), B);
        assert_eq!(after.controller(), Some(controller));
        assert_eq!(
            physical(resolved.state(), 3).zone() == ObjectZone::Hand(A),
            draws
        );
    }
}
