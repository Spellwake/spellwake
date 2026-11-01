//! Public native target-gate scenarios, not complete Magic card execution.
use spellwake_engine::{
    TargetId,
    card_identity::{CardDestination, CardId, CardObjectId, CardObjects, CardZone},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackWindow, StackWindowError, WindowPhase},
    targets::{
        ResolutionPermission, TargetCheck, TargetChoice, TargetClause, TargetClauseId, TargetError,
        TargetLegality, TargetRef,
    },
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const C: SeatId = SeatId(2);
struct Setup {
    window: StackWindow,
    spell: CardObjectId,
    first: CardObjectId,
    second: CardObjectId,
    response: CardObjectId,
}
fn setup() -> Setup {
    let spell = CardObjects::new(0)
        .seed_card(0, CardId(1), A, CardDestination::Hand)
        .unwrap();
    let first = spell
        .next
        .seed_card(
            1,
            CardId(2),
            B,
            CardDestination::Battlefield { controller: B },
        )
        .unwrap();
    let second = first
        .next
        .seed_card(
            2,
            CardId(3),
            C,
            CardDestination::Battlefield { controller: C },
        )
        .unwrap();
    let response = second
        .next
        .seed_card(3, CardId(4), B, CardDestination::Hand)
        .unwrap();
    Setup {
        window: StackWindow::new(vec![A, B, C], A, response.next).unwrap(),
        spell: spell.after.id(),
        first: first.after.id(),
        second: second.after.id(),
        response: response.after.id(),
    }
}
fn clause(id: u32, min: usize, max: usize) -> TargetClause {
    TargetClause {
        id: TargetClauseId(id),
        minimum: min,
        maximum: max,
    }
}
fn choice(id: u32, clause: u32, target: TargetRef) -> TargetChoice {
    TargetChoice {
        id: TargetId(id),
        clause: TargetClauseId(clause),
        target,
    }
}
fn check(id: u32, legal: bool) -> TargetCheck {
    TargetCheck {
        target: TargetId(id),
        legal,
    }
}
fn ready(w: StackWindow) -> StackWindow {
    w.finish_checks(w.revision()).unwrap()
}
fn passes(mut w: StackWindow, players: &[SeatId]) -> StackWindow {
    for player in players {
        if matches!(
            w.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ) {
            w = ready(w);
        }
        w = w.pass(w.revision(), *player).unwrap();
    }
    w
}
fn targeted(s: &Setup, clauses: Vec<TargetClause>, choices: Vec<TargetChoice>) -> StackWindow {
    let w = ready(s.window.clone());
    w.record_targeted_card_spell(w.revision(), A, s.spell, clauses, choices)
        .unwrap()
}
fn response(s: &Setup, w: StackWindow) -> StackWindow {
    let w = ready(passes(w, &[A]));
    let w = w.record_card_spell(w.revision(), B, s.response).unwrap();
    passes(w, &[B, C, A])
}
fn finish_response(w: StackWindow) -> StackWindow {
    let id = w.resolving().unwrap().id();
    let w = w
        .finish_resolution(w.revision(), id, Some(CardDestination::Graveyard))
        .unwrap();
    passes(w, &[A, B, C])
}
fn checking_id(w: &StackWindow) -> spellwake_engine::stack_window::StackEntryId {
    w.checking_targets().unwrap().id()
}

#[test]
fn a_departed_and_returned_card_stays_illegal_even_if_additional_checks_say_legal() {
    let s = setup();
    let w = targeted(
        &s,
        vec![clause(1, 1, 1)],
        vec![choice(1, 1, TargetRef::Card(s.first))],
    );
    let w = response(&s, w);
    let w = w
        .record_resolution_move(w.revision(), s.first, CardDestination::Exile)
        .unwrap();
    let exiled = w.cards().current_identity(CardId(2)).unwrap();
    let w = w
        .record_resolution_move(
            w.revision(),
            exiled,
            CardDestination::Battlefield { controller: B },
        )
        .unwrap();
    let returned = w.cards().current_identity(CardId(2)).unwrap();
    let w = finish_response(w);
    let entry = checking_id(&w);
    let saved = w.clone();
    assert_eq!(
        w.record_resolution_move(w.revision(), s.second, CardDestination::Graveyard),
        Err(StackWindowError::NotResolving)
    );
    assert_eq!(w, saved);
    let w = w
        .check_resolution_targets(w.revision(), entry, vec![check(1, true)])
        .unwrap();
    assert_eq!(w.phase(), WindowPhase::AllTargetsIllegal(entry));
    assert_eq!(
        w.target_assessment().unwrap().results()[0].legality,
        TargetLegality::ObjectGone
    );
    assert_eq!(
        w.finish_resolution(w.revision(), entry, Some(CardDestination::Graveyard)),
        Err(StackWindowError::NotResolving)
    );
    assert_eq!(
        w.record_resolution_move(w.revision(), s.second, CardDestination::Graveyard),
        Err(StackWindowError::NotResolving)
    );
    let w = w
        .finish_unresolved(w.revision(), entry, Some(CardDestination::Graveyard))
        .unwrap();
    assert_eq!(
        w.cards().get(returned).unwrap().zone(),
        CardZone::Battlefield
    );
    assert_eq!(
        w.cards()
            .get(w.cards().current_identity(CardId(1)).unwrap())
            .unwrap()
            .zone(),
        CardZone::Graveyard(A)
    );
    assert_eq!(
        w.phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: A })
    );
}
#[test]
fn one_remaining_target_resolves_even_when_casting_required_two_targets() {
    let s = setup();
    let w = targeted(
        &s,
        vec![clause(1, 2, 2)],
        vec![
            choice(1, 1, TargetRef::Card(s.first)),
            choice(2, 1, TargetRef::Card(s.second)),
        ],
    );
    let w = response(&s, w);
    let w = w
        .record_resolution_move(w.revision(), s.second, CardDestination::Exile)
        .unwrap();
    let w = finish_response(w);
    let entry = checking_id(&w);
    let w = w
        .check_resolution_targets(w.revision(), entry, vec![check(2, true), check(1, true)])
        .unwrap();
    assert_eq!(w.phase(), WindowPhase::Resolving(entry));
    let report = w.target_assessment().unwrap();
    assert_eq!(report.permission(), ResolutionPermission::SomeTargetsLegal);
    assert_eq!(report.results().len(), 2);
    assert_eq!(report.results()[0].choice.id, TargetId(1));
    assert_eq!(report.results()[1].choice.id, TargetId(2));
    assert_eq!(report.results()[1].legality, TargetLegality::ObjectGone);
    let saved = w.clone();
    assert_eq!(
        w.record_targeted_resolution_move(w.revision(), TargetId(2), CardDestination::Graveyard),
        Err(StackWindowError::IllegalTarget)
    );
    assert_eq!(w, saved);
    let w = w
        .record_targeted_resolution_move(w.revision(), TargetId(1), CardDestination::Graveyard)
        .unwrap();
    assert_eq!(
        w.cards()
            .get(w.cards().current_identity(CardId(2)).unwrap())
            .unwrap()
            .zone(),
        CardZone::Graveyard(B)
    );
    assert_eq!(
        w.cards()
            .get(w.cards().current_identity(CardId(3)).unwrap())
            .unwrap()
            .zone(),
        CardZone::Exile
    );
    let w = w
        .finish_resolution(w.revision(), entry, Some(CardDestination::Graveyard))
        .unwrap();
    assert!(w.entries().is_empty());
}
#[test]
fn the_same_object_in_two_target_clauses_is_assessed_per_occurrence() {
    let s = setup();
    let w = targeted(
        &s,
        vec![clause(1, 1, 1), clause(2, 1, 1)],
        vec![
            choice(1, 1, TargetRef::Card(s.first)),
            choice(2, 2, TargetRef::Card(s.first)),
        ],
    );
    let w = passes(w, &[A, B, C]);
    let entry = checking_id(&w);
    let w = w
        .check_resolution_targets(w.revision(), entry, vec![check(1, false), check(2, true)])
        .unwrap();
    assert_eq!(
        w.target_assessment().unwrap().results()[0].legality,
        TargetLegality::AdditionalRulesIllegal
    );
    assert_eq!(
        w.target_assessment().unwrap().results()[1].legality,
        TargetLegality::Legal
    );
    assert_eq!(
        w.record_targeted_resolution_move(w.revision(), TargetId(1), CardDestination::Graveyard),
        Err(StackWindowError::IllegalTarget)
    );
    let w = w
        .record_targeted_resolution_move(w.revision(), TargetId(2), CardDestination::Graveyard)
        .unwrap();
    assert!(w.cards().get(s.first).is_none());
}
#[test]
fn selecting_zero_optional_targets_does_not_fail_resolution() {
    let s = setup();
    let w = targeted(&s, vec![clause(1, 0, 3)], vec![]);
    let w = passes(w, &[A, B, C]);
    assert!(matches!(w.phase(), WindowPhase::Resolving(_)));
    assert_eq!(
        w.target_assessment().unwrap().permission(),
        ResolutionPermission::NoTargets
    );
    assert_eq!(
        w.check_resolution_targets(w.revision(), w.resolving().unwrap().id(), vec![]),
        Err(StackWindowError::TargetChecksNotPending)
    );
}
#[test]
fn malformed_selections_leave_the_source_card_and_stack_unchanged() {
    let s = setup();
    let w = ready(s.window);
    let saved = w.clone();
    let valid = choice(1, 1, TargetRef::Card(s.first));
    let cases = vec![
        (vec![clause(1, 2, 1)], vec![], TargetError::InvalidClause),
        (
            vec![clause(1, 0, 1), clause(1, 0, 1)],
            vec![],
            TargetError::DuplicateClause,
        ),
        (vec![clause(1, 1, 1)], vec![], TargetError::TargetCount),
        (
            vec![clause(2, 0, 1)],
            vec![valid],
            TargetError::UnknownClause,
        ),
        (
            vec![clause(1, 1, 2)],
            vec![valid, valid],
            TargetError::DuplicateKey,
        ),
        (
            vec![clause(1, 1, 2)],
            vec![valid, choice(2, 1, TargetRef::Card(s.first))],
            TargetError::RepeatedTarget,
        ),
        (
            vec![clause(1, 1, 1)],
            vec![choice(1, 1, TargetRef::Player(SeatId(99)))],
            TargetError::MissingTarget,
        ),
    ];
    for (clauses, choices, error) in cases {
        assert_eq!(
            w.record_targeted_card_spell(w.revision(), A, s.spell, clauses, choices),
            Err(StackWindowError::Targets(error))
        );
        assert_eq!(w, saved);
    }
    assert_eq!(w.cards().get(s.spell).unwrap().zone(), CardZone::Hand(A));
    assert!(w.entries().is_empty());
}
#[test]
fn resolution_checks_must_cover_exactly_the_bound_keys_once_and_cannot_be_replayed() {
    let s = setup();
    let w = targeted(
        &s,
        vec![clause(1, 1, 1)],
        vec![choice(1, 1, TargetRef::Card(s.first))],
    );
    let w = passes(w, &[A, B, C]);
    let entry = checking_id(&w);
    let saved = w.clone();
    for (checks, error) in [
        (vec![], TargetError::MissingCheck),
        (
            vec![check(1, true), check(1, false)],
            TargetError::DuplicateCheck,
        ),
        (vec![check(999, true)], TargetError::UnknownCheck),
    ] {
        assert_eq!(
            w.check_resolution_targets(w.revision(), entry, checks),
            Err(StackWindowError::Targets(error))
        );
        assert_eq!(w, saved);
    }
    assert_eq!(
        w.check_resolution_targets(w.revision() - 1, entry, vec![check(1, true)]),
        Err(StackWindowError::StaleRevision)
    );
    let w = w
        .check_resolution_targets(w.revision(), entry, vec![check(1, true)])
        .unwrap();
    assert_eq!(
        w.check_resolution_targets(w.revision(), entry, vec![check(1, false)]),
        Err(StackWindowError::TargetChecksNotPending)
    );
    assert_eq!(
        w.finish_unresolved(w.revision(), entry, Some(CardDestination::Graveyard)),
        Err(StackWindowError::TargetsNotRejected)
    );
}
#[test]
fn targeted_abilities_with_all_illegal_targets_leave_the_source_untouched() {
    let s = setup();
    let source = s.window.cards().get(s.first).unwrap().clone();
    let w = ready(s.window);
    let w = w
        .record_targeted_activation(
            w.revision(),
            A,
            source,
            vec![clause(1, 1, 1)],
            vec![choice(1, 1, TargetRef::Player(B))],
        )
        .unwrap();
    let w = passes(w, &[A, B, C]);
    let entry = checking_id(&w);
    let w = w
        .check_resolution_targets(w.revision(), entry, vec![check(1, false)])
        .unwrap();
    assert_eq!(w.phase(), WindowPhase::AllTargetsIllegal(entry));
    assert_eq!(
        w.finish_unresolved(w.revision(), entry, Some(CardDestination::Graveyard)),
        Err(StackWindowError::UnexpectedSpellExit)
    );
    let w = w.finish_unresolved(w.revision(), entry, None).unwrap();
    assert!(w.entries().is_empty());
    assert_eq!(
        w.cards().get(s.first).unwrap().zone(),
        CardZone::Battlefield
    );
}
#[test]
fn stack_card_references_have_one_representation_and_a_removed_entry_becomes_illegal() {
    let s = setup();
    let w = ready(s.window);
    let w = w.record_card_spell(w.revision(), A, s.spell).unwrap();
    let lower = w.entries()[0].id();
    let spell_object = match *w.entries()[0].kind() {
        spellwake_engine::stack_window::StackKind::CardSpell(id) => id,
        _ => panic!("card expected"),
    };
    let source = w.cards().get(s.first).unwrap().clone();
    let w = ready(w);
    assert_eq!(
        w.record_targeted_activation(
            w.revision(),
            A,
            source.clone(),
            vec![clause(1, 1, 1)],
            vec![choice(1, 1, TargetRef::Card(spell_object))]
        ),
        Err(StackWindowError::Targets(TargetError::UseStackReference))
    );
    let w = w
        .record_targeted_activation(
            w.revision(),
            A,
            source,
            vec![clause(1, 1, 1)],
            vec![choice(1, 1, TargetRef::Stack(lower))],
        )
        .unwrap();
    let w = ready(passes(w, &[A]));
    let w = w.record_card_spell(w.revision(), B, s.response).unwrap();
    let w = passes(w, &[B, C, A]);
    let response = w.resolving().unwrap().id();
    let w = w
        .record_resolution_move(w.revision(), spell_object, CardDestination::Graveyard)
        .unwrap();
    let w = w
        .finish_resolution(w.revision(), response, Some(CardDestination::Graveyard))
        .unwrap();
    let w = passes(w, &[A, B, C]);
    let entry = checking_id(&w);
    let w = w
        .check_resolution_targets(w.revision(), entry, vec![check(1, true)])
        .unwrap();
    assert_eq!(
        w.target_assessment().unwrap().results()[0].legality,
        TargetLegality::StackEntryGone
    );
    assert_eq!(w.phase(), WindowPhase::AllTargetsIllegal(entry));
}
