//! Actual completed participant historical reveal, not a portable replay archive.
use super::*;
use crate::player_wire::{PlayerObjectIdentity, PlayerZone};

pub(super) fn run(
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    reservation: &Reservation,
    actor: &crate::actor::RequestActor,
) -> usize {
    let NativeHumanAccessOutcome::Granted(access) = repository
        .human_access(actor, reservation.game_id, reservation.context_id)
        .unwrap()
    else {
        panic!("actual retained participant required")
    };
    assert!(access.completed_disclosure().is_some());
    assert!(!reservation.seat_ids.contains(&reservation.profile_id));
    assert!(
        repository
            .project_completed_history(
                store,
                programs,
                &access,
                reservation.profile_id,
                NativeRecoveryBudget::new(10_000, 100_000_000).unwrap()
            )
            .is_err()
    );
    let ordinary = repository
        .project_human_history(
            store,
            programs,
            &access,
            NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert!(
        ordinary.observations()[0]
            .projection()
            .snapshot()
            .zones
            .iter()
            .any(|zone| matches!(zone, PlayerZone::Hidden { .. }))
    );
    let mut observed = 0;
    for perspective in &reservation.seat_ids {
        let history = repository
            .project_completed_history(
                store,
                programs,
                &access,
                *perspective,
                NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
            )
            .unwrap()
            .unwrap();
        assert!(history.disclosure() == NativeDisclosure::CompletedParticipant);
        assert!(history.recovered().checkpoint().state().outcome().is_some());
        let baseline = &history.observations()[0];
        let snapshot = baseline.projection().snapshot();
        assert_eq!(snapshot.basis.revision.get(), 0);
        assert_eq!(snapshot.viewer_seat_id, *perspective);
        assert_eq!(snapshot.visible_objects.len(), 40);
        assert!(
            snapshot
                .zones
                .iter()
                .all(|zone| matches!(zone, PlayerZone::Visible { .. }))
        );
        assert!(
            snapshot
                .visible_objects
                .iter()
                .all(|object| matches!(object.identity, PlayerObjectIdentity::Known { .. }))
        );
        let vocabulary = history
            .recovered()
            .checkpoint()
            .state()
            .execution()
            .creature_vocabulary();
        let facts = baseline.projection().facts(vocabulary).unwrap();
        assert!(facts.authorized_full_disclosure);
        snapshot.validate(&facts).unwrap();

        // Actual earlier State has no outcome. The ordinary loader must not gain
        // authority merely because SQL retained a completed-disclosure artifact.
        let initial = load_initial(
            store,
            &history.recovered().source().initial_expected().unwrap(),
            programs,
        )
        .unwrap();
        assert!(initial.candidate().state().outcome().is_none());
        let display = NativeImmutableDisplay::new(programs, initial.provenance().unwrap()).unwrap();
        let expected = NativeViewExpected::new(
            baseline.view_reference().clone(),
            snapshot.basis.clone(),
            *perspective,
            NativeDisclosure::CompletedParticipant,
            snapshot.through_event_sequence.clone(),
        )
        .unwrap();
        assert!(matches!(
            load_view(
                store,
                &expected,
                initial.candidate().state(),
                initial.binding(*perspective).unwrap(),
                &display
            ),
            Err(NativeViewError::Projection(
                NativeProjectionError::NotCompleted
            ))
        ));

        let retry = repository
            .project_completed_history(
                store,
                programs,
                &access,
                *perspective,
                NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(history.observations().len(), retry.observations().len());
        for (original, retry) in history.observations().iter().zip(retry.observations()) {
            assert_eq!(original.receipt_id(), retry.receipt_id());
            assert!(original.projection().snapshot() == retry.projection().snapshot());
        }
        let mut budget = NativeRecoveryBudget::new(100, 4_000_000).unwrap();
        let scan = repository
            .read_human_archive(&history, &Amount::from(0), None, 100, &mut budget)
            .unwrap();
        assert_eq!(scan.item_count(), 1);
        assert!(!scan.has_more());
        let cursor = repository
            .issue_human_history_cursor(&scan, &mut budget)
            .unwrap();
        assert!(
            repository
                .read_verified_history_cursor(&history, &cursor.cursor())
                .unwrap()
                .unwrap()
                .cursor()
                == cursor.cursor()
        );
        assert!(
            repository
                .read_verified_history_cursor(&ordinary, &cursor.cursor())
                .unwrap()
                .is_none()
        );
        assert!(
            repository
                .read_human_history_cursor(&access, &cursor.cursor())
                .unwrap()
                .is_none()
        );
        observed += history.observations().len();
    }
    observed
}
