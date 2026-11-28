use super::{cmm_gameplay::named, *};
use spellwake_engine::effects::{DrawReplacementChoice, DrawReplacementDomain, ExileCastAbility};

fn draw_two(
    name: &str,
    price: &[(ManaType, i64)],
    first: bool,
    creatures: Vec<SeedCard>,
) -> GameState {
    let ordinary = seed(
        3,
        A,
        ObjectDestination::LibraryBottom,
        Program::artifact(None, vec![]),
    );
    let miracle = named(name, 2, A, ObjectDestination::LibraryBottom);
    let mut cards = vec![seed(
        1,
        A,
        ObjectDestination::Hand,
        free_instant(
            BodyTarget::None,
            crate::keyword_creatures::toolbox_emit::draw_controller(
                spellwake_engine::program::ValueExpr::Literal(2.into()),
            ),
        ),
    )];
    cards.extend(if first {
        [miracle, ordinary]
    } else {
        [ordinary, miracle]
    });
    cards.extend(creatures);
    let game = game(vec![A, B], cards, price, Step::Upkeep);
    round(cast_untargeted(&game, 1, &[]).into_state()).into_state()
}

fn reveal(game: &GameState, yes: bool) -> GameState {
    let RuleFrame::DrawReplacement(frame) = game.pending_decision().unwrap().frame() else {
        panic!("expected Miracle reveal");
    };
    assert_eq!(frame.chooser, A);
    assert_eq!(
        frame.domain,
        DrawReplacementDomain::Miracle {
            card: physical(game, 2).id()
        }
    );
    assert!(game.face_down_reveals().next().is_none());
    reply(
        game,
        DecisionReply::DrawReplacement(DrawReplacementChoice::Miracle(yes)),
    )
    .into_state()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_miracles_reveal_only_the_first_card_and_cast_at_resolution() {
    let cost = [(ManaType::Red, 1)];
    let pending = draw_two("Thunderous Wrath", &cost, true, vec![]);
    let declined = reveal(&pending, false);
    assert_eq!(physical(&declined, 2).zone(), ObjectZone::Hand(A));
    assert!(declined.execution().window().entries().is_empty());
    assert!(declined.face_down_reveals().next().is_none());

    let revealed = reveal(&pending, true);
    assert_eq!(physical(&revealed, 2).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&revealed, 3).zone(), ObjectZone::Hand(A));
    assert_eq!(revealed.face_down_reveals().count(), 1);
    let offered = round(revealed).into_state();
    let RuleFrame::OptionalEffect(frame) = offered.pending_decision().unwrap().frame() else {
        panic!("expected Miracle cast");
    };
    let cast = frame.exile_cast().unwrap();
    assert_eq!(cast.ability, ExileCastAbility::Miracle);
    assert_eq!(cast.spell, physical(&offered, 2).id());
    assert_eq!(offered.face_down_reveals().count(), 1);
    let skipped = reply(
        &offered,
        DecisionReply::OptionalEffect(spellwake_engine::effects::OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(physical(&skipped, 2).zone(), ObjectZone::Hand(A));
    assert!(skipped.face_down_reveals().next().is_none());

    let placed = reply(
        &offered,
        DecisionReply::ImpulseCast {
            target: Some(TargetRef::Player(B)),
            payment: CastingPayment {
                mana: Some(spend(&cost)),
                ..Default::default()
            },
            mana_abilities: vec![],
        },
    )
    .into_state();
    let end = round(placed).into_state();
    assert_eq!(end.execution().life_total(B), Some(&15.into()));
    assert_eq!(physical(&end, 2).zone(), ObjectZone::Graveyard(A));

    let later = draw_two("Thunderous Wrath", &cost, false, vec![]);
    assert!(later.pending_decision().is_none());
    assert_eq!(physical(&later, 2).zone(), ObjectZone::Hand(A));
    assert!(later.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_miracles_use_one_x_symbol_for_entreat_and_keep_printed_mana_value() {
    let price = [(ManaType::White, 2), (ManaType::Colourless, 2)];
    let pending = draw_two("Entreat the Angels", &price, true, vec![]);
    let offered = round(reveal(&pending, true)).into_state();
    let RuleFrame::OptionalEffect(frame) = offered.pending_decision().unwrap().frame() else {
        panic!("expected Miracle cast");
    };
    assert!(frame.exile_cast().unwrap().allows_x);
    let placed = reply(
        &offered,
        DecisionReply::ImpulseCast {
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&price)),
                chosen_x: Some(2.into()),
                ..Default::default()
            },
            mana_abilities: vec![],
        },
    );
    let cast_value = events(&placed)
        .flat_map(|event| &event.occurrences)
        .find_map(|row| match &row.value {
            RulesOccurrence::CastCompleted(cast) => Some(cast.mana_value()),
            _ => None,
        });
    assert_eq!(cast_value, Some(&7.into()));
    let placed = placed.into_state();
    let mut end = round(placed).into_state();
    if let Some(RuleFrame::TimestampOrder(frame)) =
        end.pending_decision().map(|decision| decision.frame())
    {
        end = reply(
            &end,
            DecisionReply::TimestampOrder(
                frame.members().iter().map(|member| member.member).collect(),
            ),
        )
        .into_state();
    }
    let flyers = end
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|object| {
            object.is_token()
                && object.controller() == Some(A)
                && end
                    .execution()
                    .current_characteristics(object.id())
                    .unwrap()
                    .unwrap()
                    .has_keyword(KeywordAbility::Flying)
        })
        .count();
    assert_eq!(flyers, 2);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_miracles_blessings_accepts_the_same_target_division_at_the_alternate_price() {
    let price = [(ManaType::Green, 1)];
    let pending = draw_two(
        "Blessings of Nature",
        &price,
        true,
        vec![
            super::cmm_gameplay::creature(4, A, 1, 2, &[]),
            super::cmm_gameplay::creature(5, B, 2, 2, &[]),
        ],
    );
    let offered = round(reveal(&pending, true)).into_state();
    let RuleFrame::OptionalEffect(frame) = offered.pending_decision().unwrap().frame() else {
        panic!("expected a Miracle cast offer");
    };
    let offer = frame.exile_cast().unwrap();
    let targets = offer.permanent_targets.as_ref().unwrap();
    assert_eq!(targets.division_total, Some(4));
    let first = physical(&offered, 4).id();
    let second = physical(&offered, 5).id();
    let cast = reply(
        &offered,
        DecisionReply::ImpulseCast {
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&price)),
                permanent_targets: vec![TargetRef::Object(first), TargetRef::Object(second)],
                target_division: vec![1, 3],
                ..Default::default()
            },
            mana_abilities: vec![],
        },
    );
    let done = round(cast.into_state()).into_state();
    let kind = spellwake_engine::effects::CounterKind::PlusOne;
    assert_eq!(done.execution().counters_of_kind(first, kind), 1.into());
    assert_eq!(done.execution().counters_of_kind(second, kind), 3.into());
}
