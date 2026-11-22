//! The spell keeps the original object as its only target after a bounce and recast.
use super::*;
use spellwake_engine::occurrence::EventCause;

#[test]
#[ignore = "requires pinned Oracle and creature vocabulary inputs"]
fn brute_force_loses_its_only_target() {
    check_target_departure("Brute Force", &[(ManaType::Red, 1)]);
}

#[test]
#[ignore = "requires pinned Oracle and creature vocabulary inputs"]
fn explosive_shot_loses_its_only_target() {
    check_target_departure(
        "Explosive Shot",
        &[(ManaType::Red, 1), (ManaType::White, 1)],
    );
}

fn check_target_departure(spell: &str, cost: &[(ManaType, i64)]) {
    let mut mana = vec![
        (B, ManaType::Blue, 1),
        (A, ManaType::Green, 1),
        (A, ManaType::Colourless, 1),
    ];
    mana.extend(cost.iter().map(|&(color, amount)| (A, color, amount)));
    let state = game(
        vec![
            seed(spell, 1, A, ObjectDestination::Hand),
            seed("Unsummon", 2, B, ObjectDestination::Hand),
            seed(
                "Ashcoat Bear",
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            seed(
                "Grizzly Bears",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
        ],
        &mana,
    );
    let original = object(&state, 3);
    let alternate = object(&state, 4);
    // The other creature is legal, but the engine must never substitute it at resolution.
    advance(&state, A, targeted_spell(&state, 1, alternate, cost));
    let cast = advance(&state, A, targeted_spell(&state, 1, original, cost));
    let [entry] = cast.execution().window().entries() else {
        panic!("expected one spell")
    };
    let expected_id = entry.id();
    assert_eq!(
        entry.targets().targets()[0].choice().target,
        TargetRef::Object(original)
    );
    let priority_to_b = advance(&cast, A, GameCommand::Pass);
    let response = advance(
        &priority_to_b,
        B,
        targeted_spell(&priority_to_b, 2, original, &[(ManaType::Blue, 1)]),
    );
    let bounced = resolve_top(&response).into_state();
    assert_eq!(physical(&bounced, 3).zone(), ObjectZone::Hand(A));
    let recast = advance(
        &bounced,
        A,
        GameCommand::CastUntargeted {
            spell: object(&bounced, 3),
            payment: Some(payment(&[(ManaType::Green, 1), (ManaType::Colourless, 1)])),
        },
    );
    let before = resolve_top(&recast).into_state();
    let replacement = object(&before, 3);
    assert_ne!(replacement, original);
    assert_eq!(physical(&before, 3).zone(), ObjectZone::Battlefield);
    let finished = resolve_top(&before);
    let completion = EventCause::ResolutionCompleted {
        entry: expected_id,
        completion: EntryCompletion::AllTargetsIllegal,
    };
    assert!(finished.rules_journal().records.iter().any(|record| {
        matches!(record, RulesRecord::Event(event) if event.cause == completion)
    }));
    let after = finished.state();
    assert!(after.execution().window().entries().is_empty());
    assert_eq!(physical(after, 1).zone(), ObjectZone::Graveyard(A));
    for target in [alternate, replacement] {
        assert_eq!(
            after.execution().current_characteristics(target).unwrap(),
            before.execution().current_characteristics(target).unwrap()
        );
        assert_eq!(
            after.execution().battlefield_marked_damage(target),
            Some(0.into())
        );
    }
}

#[test]
#[ignore = "requires pinned Oracle and creature vocabulary inputs"]
fn explosive_shot_does_not_resolve_after_its_only_target_is_exiled() {
    use super::sacrifice_exile::{exile_response, resolve_exile_response};
    use ObjectDestination::{Battlefield, Hand};
    let cost = [(ManaType::Red, 1), (ManaType::White, 1)];
    let targets = [
        seed("Explosive Shot", 1, A, Hand),
        seed("Grizzly Bears", 2, B, Battlefield { controller: B }),
        seed("Gray Ogre", 3, B, Battlefield { controller: B }),
    ];
    let state = game(
        targets.into_iter().chain(exile_response(4, 5)).collect(),
        &[(A, ManaType::Red, 1), (A, ManaType::White, 1)],
    );
    let original = object(&state, 2);
    let cast = advance(&state, A, targeted_spell(&state, 1, original, &cost));
    let entry = cast.execution().window().entries()[0].id();
    let exiled = resolve_exile_response(&cast, 4, &[(B, 2)]);
    let finished = resolve_top(&exiled);
    let completion = EventCause::ResolutionCompleted {
        entry,
        completion: EntryCompletion::AllTargetsIllegal,
    };
    assert!(finished.rules_journal().records.iter().any(|record| {
        matches!(record, RulesRecord::Event(event) if event.cause == completion)
    }));
    let result = finished.state();
    assert!(result.execution().window().entries().is_empty());
    assert_eq!(physical(result, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(result, 2).zone(), ObjectZone::Exile);
    assert_eq!(
        result
            .execution()
            .battlefield_marked_damage(object(result, 3)),
        Some(0.into())
    );
}
