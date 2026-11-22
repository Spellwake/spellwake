//! Actual owned Auth/SQL/Storage result and canonical transition acceptance.
//! Clock intent is derived; feature metrics/cards/timeout/replay remain unqualified.
use super::*;
use crate::player_wire::{PlayerAction, PlayerChoice, PlayerDomain};
use spellwake_engine::{
    priority::PriorityPhase, stack_window::WindowPhase, turn::Step as TurnStep,
};
type Bank = (Uuid, Amount, Amount, bool, String);
fn banks(source: &NativeGameSource) -> Vec<Bank> {
    source
        .clocks()
        .map(|(seat, remaining, revision, paused, at)| {
            (
                seat,
                remaining.clone(),
                revision.clone(),
                paused,
                at.to_owned(),
            )
        })
        .collect()
}
fn assert_accounted(prior: &[Bank], current: &NativeGameSource) {
    assert_eq!(prior.len(), current.clocks().len());
    for (seat, remaining, revision, _, at) in current.clocks() {
        let old = prior.iter().find(|old| old.0 == seat).unwrap();
        assert_eq!(revision, &old.2.checked_add(&Amount::from(1)).unwrap());
        let expected = if old.3 {
            old.1.clone()
        } else {
            let from = crate::source_protocol::parse_utc_microsecond(&old.4)
                .unwrap()
                .timestamp_millis();
            let to = crate::source_protocol::parse_utc_microsecond(at)
                .unwrap()
                .timestamp_millis();
            let elapsed = u64::try_from(to.checked_sub(from).unwrap()).unwrap();
            old.1.checked_sub(&Amount::from(elapsed)).unwrap()
        };
        assert_eq!(remaining, &expected);
    }
}
pub(super) fn run(
    reservation: &Reservation,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
) {
    let target: u64 = std::env::var("SPELLWAKE_NATIVE_RESULT_TURNS")
        .unwrap()
        .parse()
        .unwrap();
    assert!(matches!(target, 0 | 3));
    let repository = NativeSourceRepository::from_env().unwrap();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let auth = crate::actor::ActorClient::new(
        crate::actor::ActorConfig::new(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap(),
            &format!("{}/auth/v1", base.trim_end_matches('/')),
        )
        .unwrap(),
    )
    .unwrap();
    let tokens = [
        std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap(),
        std::env::var("SPELLWAKE_NATIVE_SOURCE_PEER_BEARER").unwrap(),
    ];
    let actors = [
        auth.resolve(&tokens[0]).unwrap(),
        auth.resolve(&tokens[1]).unwrap(),
    ];
    let command_http = http::HttpProbe::owned(
        &base,
        &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
        "fixture:source-reader",
    );
    let read_only_http = http::HttpProbe::new(
        &base,
        &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
    );
    let unsupported = std::env::var("SPELLWAKE_NATIVE_UNSUPPORTED_WORK").is_ok_and(|v| v == "1");
    let rule_errors = std::env::var("SPELLWAKE_NATIVE_RULE_ERRORS").is_ok_and(|v| v == "1");
    let no_contest = std::env::var("SPELLWAKE_NATIVE_NO_CONTEST").is_ok_and(|v| v == "1");
    let mut terminal_accepted = false;
    let mut completed_history_cuts = 0;
    let mut history_page_events = 0;
    let mut replay_frames = 0;
    let mut active_replay_frames = 0;
    let mut publication_budget = serde_json::Value::Null;
    let mut prior_banks: Option<Vec<Bank>> = None;
    for _ in 0..400 {
        // Each step really cold-recovers the accepted canonical chain. This is
        // intentionally a recovery test, not a performance claim or cache design.
        let recovered = repository
            .recover_with_history(
                reservation.game_id,
                store,
                programs,
                NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            recovered.history_evidence(),
            NativeHistoryEvidence::TransitionV1
        );
        let checkpoint = recovered.checkpoint();
        for (seat, _, _, _, _) in recovered.source().clocks() {
            assert_eq!(
                recovered.source().clock_policy(seat),
                Some(reservation.bank_policy())
            );
        }
        if let Some(prior) = prior_banks.as_ref() {
            assert_accounted(prior, recovered.source());
        }
        prior_banks = Some(banks(recovered.source()));
        let waiting_owner = if let Some(decision) = checkpoint.state().pending_decision() {
            decision.chooser()
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                checkpoint.state().execution().window().phase()
            else {
                panic!("actual supported wait required");
            };
            assert!(checkpoint.state().pending_turn_work().is_none());
            assert!(checkpoint.state().execution().pending_work_kind().is_none());
            holder
        };
        let waiting_seat = reservation.seat_ids[usize::try_from(waiting_owner.0).unwrap()];
        for (seat, remaining, _, paused, _) in recovered.source().clocks() {
            match reservation.structure {
                crate::rooms::configuration::MatchStructure::Bo1 => {
                    assert!(paused);
                    assert!(remaining.is_zero());
                }
                crate::rooms::configuration::MatchStructure::Bo3 => {
                    assert_eq!(paused, seat != waiting_seat);
                    assert!(!remaining.is_zero());
                }
            }
        }
        let concede = !rule_errors
            && !unsupported
            && checkpoint.own_turns(reservation.seat_ids[0]).unwrap() >= target;
        if concede && active_replay_frames == 0 {
            // The game is still in progress here: own-seat live replay only.
            active_replay_frames = super::active_replay::run(
                &read_only_http,
                &repository,
                store,
                programs,
                reservation,
                &tokens,
                &actors,
            );
        }
        if concede && no_contest {
            // End the game operationally instead of by native concession.
            let metrics =
                super::no_contest::run(&repository, store, programs, reservation, &tokens, &actors);
            let mut metrics = metrics;
            metrics["activeReplayFramesChecked"] = json!(active_replay_frames);
            println!("OWNED_NATIVE_SOURCE_READER_OK {metrics}");
            return;
        }
        let native_actor = if concede {
            SeatId(0)
        } else if let Some(decision) = checkpoint.state().pending_decision() {
            decision.chooser()
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                checkpoint.state().execution().window().phase()
            else {
                panic!("actual priority required")
            };
            holder
        };
        let index = usize::try_from(native_actor.0).unwrap();
        let actor = &actors[index];
        let NativeHumanAccessOutcome::Granted(access) = repository
            .human_access(actor, reservation.game_id, reservation.context_id)
            .unwrap()
        else {
            panic!("actual participant required")
        };
        if checkpoint.basis().revision.get() == 0 {
            assert!(
                repository
                    .project_completed_history(
                        store,
                        programs,
                        &access,
                        access.viewer_seat_id(),
                        NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
                    )
                    .is_err()
            );
        }
        let display =
            NativeImmutableDisplay::new(programs, checkpoint.provenance().unwrap()).unwrap();
        let metadata = NativeProjectionMetadata {
            through_event_sequence: recovered.source().last_event_sequence().clone(),
            deadline_at: None,
            disclosure: NativeDisclosure::LiveParticipant,
        };
        let issued = repository
            .issue_human_view(store, &access, &recovered, &metadata, &display)
            .unwrap();
        let snapshot = issued.view().snapshot();
        if rule_errors && checkpoint.state().pending_decision().is_none() {
            super::rule_errors::run(
                &repository,
                (&command_http, &read_only_http),
                (actor, &tokens[index]),
                &access,
                &recovered,
                &issued,
            );
            return;
        }
        let action = if concede {
            PlayerAction::Concede {}
        } else if checkpoint.state().pending_decision().is_some() {
            let decision = snapshot.decisions.first().unwrap();
            let response = match &decision.domain {
                PlayerDomain::Options { options, .. } => PlayerChoice::Option {
                    option: options
                        .iter()
                        .find(|option| {
                            option.label
                                == if unsupported && checkpoint.basis().revision.get() == 0 {
                                    "Mulligan"
                                } else {
                                    "Keep"
                                }
                        })
                        .expect("actual opening keep")
                        .handle
                        .clone(),
                },
                PlayerDomain::Attackers {
                    choices, required, ..
                } => {
                    assert!(
                        choices.is_empty() && required.is_empty(),
                        "land-only fixture has no attackers"
                    );
                    PlayerChoice::Attackers {
                        assignments: vec![],
                    }
                }
                PlayerDomain::Blockers {
                    choices,
                    required,
                    minimum_if_blocked,
                    ..
                } => {
                    assert!(
                        choices.is_empty() && required.is_empty() && minimum_if_blocked.is_empty(),
                        "land-only fixture has no blockers"
                    );
                    PlayerChoice::Blockers {
                        assignments: vec![],
                    }
                }
                PlayerDomain::CombatDamage { sources } => {
                    assert!(
                        sources.is_empty(),
                        "land-only fixture has no combat damage sources"
                    );
                    PlayerChoice::CombatDamage {
                        assignments: vec![],
                    }
                }
                _ => panic!("unexpected fixture decision domain"),
            };
            PlayerAction::Choose {
                decision: decision.decision.clone(),
                response,
            }
        } else if !unsupported
            && checkpoint.state().execution().window().active_player() == native_actor
            && checkpoint.state().turn().unwrap().position().step == TurnStep::PrecombatMain
            && checkpoint.state().turn().unwrap().lands_played() == &0.into()
        {
            let source = snapshot
                .zones
                .iter()
                .find(|zone| {
                    zone.zone_type() == crate::player_wire::PlayerZoneKind::Hand
                        && zone.owner_seat_id() == Some(access.viewer_seat_id())
                })
                .unwrap()
                .objects()
                .first()
                .unwrap()
                .clone();
            PlayerAction::PlayLand { source }
        } else {
            PlayerAction::Pass {}
        };
        let original = PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: checkpoint.basis().clone(),
            action,
        };
        let prepared = match issued.view().scope().propose(&original) {
            Ok(proposal) => proposal,
            Err(error) => {
                if unsupported && let Some(kind) = super::unsupported::error_kind(&error) {
                    super::unsupported::refuse(
                        &repository,
                        &command_http,
                        actor,
                        &tokens[index],
                        &original,
                        kind,
                    );
                    return;
                }
                panic!("unexpected native proposal failure: {error}");
            }
        };
        let bytes = issued.view().scope().encode_input(&prepared).unwrap();
        let input = store
            .retain(
                reservation.game_id,
                reservation.context_id,
                NativeArtifactRole::Input,
                &bytes,
            )
            .unwrap();
        let next = checkpoint
            .append(
                store,
                input,
                NativeCheckpointInput::Player,
                access.viewer_seat_id(),
            )
            .unwrap();
        next.retain(store).unwrap();
        let facts = next.prepare_results().unwrap().retain(store).unwrap();
        let event = next.prepare_event().unwrap().retain(store).unwrap();
        let produced = recovered.prepare_candidate(&next).unwrap();
        let body: serde_json::Value = serde_json::from_str(produced.original_json()).unwrap();
        assert_eq!(body["factsRef"], json!(facts));
        assert_eq!(body["events"], json!([event.as_str()]));
        if concede {
            // Use the SAME original request for rejected frames. A mistaken RPC
            // rejection receipt would poison the subsequent valid original, so
            // accepted commit/retry below also checks these were rejected early.
            let wrong_ref = store
                .retain(
                    reservation.game_id,
                    reservation.context_id,
                    NativeArtifactRole::Facts,
                    br#"{"fixture":"not-native-results"}"#,
                )
                .unwrap();
            let mut wrong_bytes = body.clone();
            wrong_bytes["factsRef"] = json!(wrong_ref.as_str());
            wrong_bytes["terminal"]["winnersRef"] = json!(wrong_ref.as_str());
            let mut wrong_count = body.clone();
            wrong_count["participation"][0]["ownTurns"] = json!((target + 1).to_string());
            let mut wrong_winner = body.clone();
            wrong_winner["terminal"]["winnerSeatIds"] = json!([reservation.seat_ids[0]]);
            let mut missing_recipient = body.clone();
            missing_recipient["participation"]
                .as_array_mut()
                .unwrap()
                .pop();
            for invalid in [wrong_bytes, wrong_count, wrong_winner, missing_recipient] {
                let candidate = crate::admission_ports::HostCandidate::from_bytes(
                    &serde_json::to_vec(&invalid).unwrap(),
                )
                .unwrap();
                assert!(
                    repository
                        .commit_human(
                            store,
                            NativeHumanCommit {
                                actor,
                                access: &access,
                                recovered: &recovered,
                                issued: &issued,
                                original: &original,
                                next: &next,
                                candidate: &candidate
                            }
                        )
                        .is_err()
                );
            }
            let mut stale = original.clone();
            stale.basis.revision = stale.basis.revision.get().checked_add(1).unwrap().into();
            let candidate = crate::admission_ports::HostCandidate::from_bytes(
                &serde_json::to_vec(&body).unwrap(),
            )
            .unwrap();
            assert!(
                repository
                    .commit_human(
                        store,
                        NativeHumanCommit {
                            actor,
                            access: &access,
                            recovered: &recovered,
                            issued: &issued,
                            original: &stale,
                            next: &next,
                            candidate: &candidate
                        }
                    )
                    .is_err()
            );
        }
        let candidate = produced;
        let commit = || {
            repository
                .commit_human(
                    store,
                    NativeHumanCommit {
                        actor,
                        access: &access,
                        recovered: &recovered,
                        issued: &issued,
                        original: &original,
                        next: &next,
                        candidate: &candidate,
                    },
                )
                .unwrap()
        };
        let response = command_http.command(Some(&tokens[index]), &original, 202);
        let ack: crate::player_wire::PlayerCommandAck =
            serde_json::from_value(response["data"].clone()).unwrap();
        ack.validate(&original, access.viewer_seat_id()).unwrap();
        assert_eq!(ack.basis, *next.basis());
        if concede {
            let retry = match commit() {
                ApiResponse::Success { data } => data,
                _ => panic!("exact terminal retry must accept"),
            };
            assert_eq!(
                serde_json::to_value(&ack).unwrap(),
                serde_json::to_value(&retry).unwrap()
            );
            assert_eq!(
                read_only_http.command(Some(&tokens[index]), &original, 202),
                response
            );
            let inactive = PlayerCommand {
                request_id: Uuid::new_v4(),
                basis: next.basis().clone(),
                action: PlayerAction::Pass {},
            };
            let stale = read_only_http.command(Some(&tokens[index]), &inactive, 409);
            assert_eq!(stale["data"], json!({"basis":"stale"}));
            assert_eq!(
                read_only_http.command(Some(&tokens[index]), &inactive, 409),
                stale
            );
            assert_eq!(next.own_turns(reservation.seat_ids[0]), Some(target));
            let cold = repository
                .recover_with_history(
                    reservation.game_id,
                    store,
                    programs,
                    NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
                )
                .unwrap()
                .unwrap();
            assert_eq!(cold.history_evidence(), NativeHistoryEvidence::TransitionV1);
            assert_accounted(prior_banks.as_ref().unwrap(), cold.source());
            assert!(cold.source().clocks().all(|(_, _, _, paused, _)| paused));
            assert_eq!(
                cold.checkpoint()
                    .prepare_results()
                    .unwrap()
                    .original_bytes(),
                next.prepare_results().unwrap().original_bytes()
            );
            assert_eq!(
                cold.checkpoint()
                    .terminal()
                    .unwrap()
                    .unwrap()
                    .winner_seat_ids(),
                &[reservation.seat_ids[1]]
            );
            fs::write(std::env::var_os("SPELLWAKE_NATIVE_SOURCE_OUTPUT").unwrap(), serde_json::to_vec(&json!({"contextId":reservation.context_id,"ownTurns":reservation.seat_ids.iter().map(|seat|next.own_turns(*seat).unwrap().to_string()).collect::<Vec<_>>(),"basis":next.basis(),"factsRef":facts,"requestId":original.request_id})).unwrap()).unwrap();
            completed_history_cuts = super::completed_history::run(
                &repository,
                store,
                programs,
                reservation,
                &actors[0],
            );
            (replay_frames, publication_budget) = super::replay_frames::run(
                &read_only_http,
                &repository,
                store,
                programs,
                reservation,
                &tokens,
                &actors,
            );
            history_page_events = super::history_pages::run(
                &read_only_http,
                &repository,
                store,
                programs,
                reservation,
                &tokens,
                &actors,
            );
            terminal_accepted = true;
            break;
        }
    }
    assert!(terminal_accepted);
    assert!(completed_history_cuts > 0);
    assert!(history_page_events > 0);
    assert!(replay_frames > 0);
    assert!(active_replay_frames > 0);
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":"results","actualCompletedFullHistoricalViews":true,"completedHistoricalCutCount":completed_history_cuts,"actualHistoryPagesHttp":true,"historyPageVisibleEvents":history_page_events,"actualPortableReplayHttp":true,"replayFramesChecked":replay_frames,"activeReplayFramesChecked":active_replay_frames,"publicationBudget":publication_budget,"actualNativeTransitionEvidence":true,"targetOwnTurns":target,"actualTerminalCommitAndRetry":true,"actualTerminalCommandHttp":true,"actualTerminalStaleHttp":true,"actualPlayerPathHttp":true,"exactColdResultEvidence":true,"actualSelectedClockPolicyObserved":true,"actualClockIntentDerived":true,"actualClockAccountingChecked":true,"actualWaitingActorChecked":true,"falseResultsRejectedBeforeCommit":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}
