use super::*;
use spellwake_engine::{
    mana::{CastResource, CastingPayment},
    occurrence::{EventCause, RulesOccurrence, RulesRecord, ZoneReason},
    program::ConditionalCastCost,
};

fn escape_game(step: Step, hand: bool, permanent: bool) -> GameState {
    let mut cards = seeds();
    let id = if permanent { 1 } else { 10 };
    let card = cards.iter_mut().find(|c| c.card == CardId(id)).unwrap();
    card.destination = if hand {
        ObjectDestination::Hand
    } else {
        ObjectDestination::Graveyard
    };
    card.program.conditional_cost = Some(ConditionalCastCost::Escape {
        mana: ManaCost::one(ManaType::Green),
        exile: 2,
        entry_counters: if permanent { 3 } else { 0 },
    });
    for (id, owner) in [(51, A), (52, A), (53, B), (54, A)] {
        let mut card = artifact(id, owner, None);
        card.destination = ObjectDestination::Graveyard;
        cards.push(card);
    }
    setup(Some(step), cards)
}

fn request(game: &GameState, id: u64, objects: &[u64]) -> GameCommand {
    GameCommand::CastWithResources {
        spell: object(game, id),
        target: (id == 10).then(|| TargetRef::Object(object(game, 90))),
        payment: CastingPayment {
            use_alternative: true,
            objects: objects
                .iter()
                .map(|&id| CastResource::Escape(object(game, id)))
                .collect(),
            ..Default::default()
        },
    }
}

#[test]
fn escape_requires_exact_distinct_other_cards_and_keeps_exiles_when_countered() {
    let game = escape_game(Step::Upkeep, false, false);
    for invalid in [
        &[][..],
        &[51],
        &[51, 52, 54],
        &[51, 51],
        &[10, 52],
        &[51, 53],
        &[51, 8],
    ] {
        reject(
            &game,
            A,
            request(&game, 10, invalid),
            EffectError::InvalidCastResource,
        );
    }
    let mut invalid = request(&game, 10, &[51, 52]);
    if let GameCommand::CastWithResources { payment, .. } = &mut invalid {
        payment.use_alternative = false;
    }
    reject(&game, A, invalid, EffectError::InvalidCastResource);
    let mut unpaid = request(&game, 10, &[51, 52]);
    if let GameCommand::CastWithResources { payment, .. } = &mut unpaid {
        payment.mana = Some(ManaPayment {
            from_lots: Default::default(),
            spend: vec![],
        });
    }
    reject(
        &game,
        A,
        unpaid,
        EffectError::InvalidManaPayment(ManaPaymentError::MissingRequired),
    );
    let hand = escape_game(Step::Upkeep, true, false);
    reject(
        &hand,
        A,
        request(&hand, 10, &[51, 52]),
        EffectError::InvalidCastResource,
    );
    let cast = accept(&game, A, request(&game, 10, &[51, 52]));
    assert_eq!(
        cast.mana_payment().unwrap().cost,
        ManaCost::one(ManaType::Green)
    );
    let event = cast
        .rules_journal()
        .records
        .iter()
        .find_map(|r| match r {
            RulesRecord::Event(e) if matches!(e.cause, EventCause::EscapePayment { .. }) => Some(e),
            _ => None,
        })
        .unwrap();
    assert_eq!(event.occurrences.len(), 2);
    for row in &event.occurrences {
        assert!(
            matches!(&row.value,RulesOccurrence::ZoneChange(z) if z.reason==ZoneReason::EscapeCost)
        );
    }
    for counter in [false, true] {
        let next = if counter {
            let ready = pass(cast.state()).into_state();
            let entry = ready.execution().window().entries()[0].id();
            let response = targeted(&ready, B, 11, TargetRef::Stack(entry));
            resolve(response.state()).into_state()
        } else {
            resolve(cast.state()).into_state()
        };
        assert_eq!(physical(&next, 10).zone(), ObjectZone::Graveyard(A));
        for id in [51, 52] {
            assert_eq!(physical(&next, id).zone(), ObjectZone::Exile);
        }
    }
}

#[test]
fn escape_creature_obeys_timing_and_enters_with_the_printed_counters_only_when_it_escaped() {
    let upkeep = escape_game(Step::Upkeep, false, true);
    reject(
        &upkeep,
        A,
        request(&upkeep, 1, &[51, 52]),
        EffectError::Timing(TimingError::MainPhaseRequired),
    );
    for hand in [true, false] {
        let game = escape_game(Step::PrecombatMain, hand, true);
        let command = if hand {
            untargeted(&game, 1, Some(ordinary_payment()))
        } else {
            request(&game, 1, &[51, 52])
        };
        let cast = accept(&game, A, command);
        let done = resolve(cast.state()).into_state();
        assert_eq!(physical(&done, 1).zone(), ObjectZone::Battlefield);
        assert_eq!(
            done.execution()
                .power_toughness_counters(object(&done, 1))
                .unwrap()
                .plus_one,
            if hand { 0 } else { 3 }.into()
        );
    }
}
