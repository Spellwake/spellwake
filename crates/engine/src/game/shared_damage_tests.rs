//! Root adjudication of a trusted simultaneous primitive, not ordinary 1v1 combat.
use super::*;
#[test]
fn n04_simultaneous_noncombat_player_losses_are_a_draw_after_shared_root_checks() {
    let (execution, progress) = crate::effects::n04_player_batch_fixture();
    assert_eq!(execution.life_total(SeatId(41)), Some(&2.into()));
    assert_eq!(execution.life_total(SeatId(3)), Some(&3.into()));
    let mut state = GameState {
        basis: GameBasis {
            game: GameId(1804),
            context: GameContext::new(
                "r".into(),
                "cr".into(),
                "c".into(),
                "p".into(),
                "trusted-primitive".into(),
                crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr").id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            epoch: 0,
            revision: 0,
        },
        execution: Box::new(execution),
        search_reveal: None,
        cascade_reveals: Vec::new(),
        face_down_reveals: Vec::new(),
        library_reveals: Vec::new(),
        pending: None,
        counter_replacement: None,
        next_decision: 1,
        opening: None,
        random: None,
        attack_mana: None,
        conclusion: None,
        turn: None,
    };
    let mut facts = TransitionFacts::default();
    state.accept_rule_progress(progress, &mut facts).unwrap();
    assert!(state.execution.state_based_losses().is_empty());
    assert_eq!(state.execution.life_total(SeatId(41)), Some(&0.into()));
    assert_eq!(state.execution.life_total(SeatId(3)), Some(&0.into()));
    assert!(matches!(
        state.execution.window().phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
    ));
    let state = state.drive(&mut facts).unwrap();
    assert_eq!(state.outcome(), Some(GameOutcome::Draw));
    assert_eq!(
        state.execution.state_based_losses(),
        &BTreeSet::from([SeatId(41), SeatId(3)])
    );
    assert!(!matches!(
        state.execution.window().phase(),
        WindowPhase::Priority(PriorityPhase::Ready { .. })
    ));
    assert_eq!(facts.damage_journal.batches.len(), 1);
    assert_eq!(facts.damage_journal.attempts.len(), 2);
    assert_eq!(facts.damage_journal.applied.len(), 2);
    assert_eq!(
        facts.damage_journal.batches[0].kind,
        crate::damage::DamageKind::Noncombat
    );
}
