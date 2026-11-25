//! Genuine source/opening chain; Storage is a strict byte-read mock here.
//! Actual SQL/private Storage acceptance is qualified separately.
use super::*;
use spellwake_engine::{
    priority::PriorityPhase, stack_window::WindowPhase, turn::Step as TurnStep,
};
fn advance(f: &mut CheckpointFixture, viewer: Uuid, action: PlayerAction) {
    let scope = NativeScope::new(f.tip.state(), f.tip.binding(viewer).unwrap()).unwrap();
    if matches!(action, PlayerAction::PlayLand { .. }) {
        unreachable!("land uses actual issued object")
    }
    let command = PlayerCommand {
        request_id: Uuid::new_v4(),
        basis: scope.basis().clone(),
        action,
    };
    let proposal = scope.propose(&command).unwrap();
    let bytes = scope.encode_input(&proposal).unwrap();
    install(f, viewer, bytes);
}
pub(super) fn install(f: &mut CheckpointFixture, viewer: Uuid, bytes: Vec<u8>) {
    let input = NativeArtifactRef::for_bytes(
        ids().game_id,
        ids().context_id,
        NativeArtifactRole::Input,
        &bytes,
    )
    .unwrap();
    let prior_counts: Vec<_> = seats()
        .iter()
        .map(|seat| f.tip.own_turns(*seat).unwrap())
        .collect();
    let next = with_store(&bytes, &input, |store| {
        f.tip
            .append(store, input.clone(), NativeCheckpointInput::Player, viewer)
    })
    .unwrap();
    assert_eq!(
        prior_counts,
        seats()
            .iter()
            .map(|seat| f.tip.own_turns(*seat).unwrap())
            .collect::<Vec<_>>()
    );
    f.inputs.push((input, bytes));
    f.states
        .push((next.reference().clone(), next.original_bytes().to_vec()));
    f.candidates.push(next.candidate().clone());
    f.tip = next;
}
pub(super) fn replay(f: &CheckpointFixture) -> NativeCheckpoint {
    let initial = reference(&f.original);
    let mut steps = vec![];
    for (r, b) in f.states.iter().rev() {
        steps.extend(checkpoint_read_steps(r, b));
    }
    steps.extend(checkpoint_read_steps(&initial, &f.original));
    for (r, b) in &f.inputs {
        steps.extend(checkpoint_read_steps(r, b));
    }
    let mock = Mock::new(steps);
    let recovered = load_checkpoint(
        &checkpoint_store(&mock),
        &NativeCheckpointExpected::new(f.tip.reference().clone(), f.tip.basis().clone()).unwrap(),
        &expected(initial),
        source(NativeProgramPool::ResolutionSelectionNaturalMana),
        NativeRecoveryBudget::new(f.states.len(), 100_000_000).unwrap(),
    )
    .unwrap();
    mock.finish();
    recovered
}
#[test]
#[ignore = "requires explicit genuine catalogue/vocabulary; native producer mechanism, not admission"]
fn genuine_turn_starts_and_terminal_survive_exact_cold_replay() {
    let mut f = checkpoint_fixture(None);
    assert_eq!(f.tip.own_turns(seats()[0]), Some(0));
    assert_eq!(f.tip.own_turns(seats()[1]), Some(1));
    assert_eq!(f.tip.own_turns(Uuid::new_v4()), None);
    let baseline = f.tip.prepare_results().unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(baseline.original_bytes()).unwrap()["terminal"],
        Value::Null
    );
    for _ in 0..400 {
        if f.tip.own_turns(seats()[1]) == Some(3) {
            break;
        }
        assert!(f.tip.state().pending_decision().is_none());
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            f.tip.state().execution().window().phase()
        else {
            panic!("expected actual priority")
        };
        let viewer = seats()[usize::try_from(holder.0).unwrap()];
        let turn = f.tip.state().turn().unwrap();
        let active = f.tip.state().execution().window().active_player();
        if holder == active
            && turn.position().step == TurnStep::PrecombatMain
            && turn.lands_played() == &0.into()
        {
            let land = f
                .tip
                .state()
                .execution()
                .window()
                .cards()
                .iter()
                .find(|object| {
                    object.owner() == holder
                        && object.zone()
                            == spellwake_engine::object_identity::ObjectZone::Hand(holder)
                })
                .unwrap()
                .id();
            let mut scope =
                NativeScope::new(f.tip.state(), f.tip.binding(viewer).unwrap()).unwrap();
            let handle = scope.issue(NativeReference::Object(land)).unwrap();
            let proposal = scope
                .propose(&PlayerCommand {
                    request_id: Uuid::new_v4(),
                    basis: scope.basis().clone(),
                    action: PlayerAction::PlayLand { source: handle },
                })
                .unwrap();
            let bytes = scope.encode_input(&proposal).unwrap();
            install(&mut f, viewer, bytes);
        } else {
            advance(&mut f, viewer, PlayerAction::Pass {});
        }
    }
    assert_eq!(f.tip.own_turns(seats()[1]), Some(3));
    assert_eq!(f.tip.own_turns(seats()[0]), Some(2));
    let recovered = replay(&f);
    let prior_event = f.tip.prepare_event().unwrap();
    assert_eq!(
        recovered.prepare_event().unwrap().original_bytes(),
        prior_event.original_bytes()
    );
    assert!(recovered.prepare_event().unwrap().reference() == prior_event.reference());
    assert_eq!(
        recovered.prepare_results().unwrap().original_bytes(),
        f.tip.prepare_results().unwrap().original_bytes()
    );
    // Conceding during the third own turn preserves that start credit.
    advance(&mut f, seats()[1], PlayerAction::Concede {});
    let terminal = f.tip.terminal().unwrap().unwrap();
    assert_eq!(terminal.winner_seat_ids(), &[seats()[0]]);
    assert_eq!(terminal.losing_seat_ids(), &[seats()[1]]);
    assert_eq!(terminal.reason(), "native_concession");
    assert_eq!(terminal.basis(), f.tip.basis());
    assert_eq!(f.tip.own_turns(seats()[1]), Some(3));
    let restored = replay(&f);
    let terminal_event = f.tip.prepare_event().unwrap();
    assert!(terminal_event.reference() != prior_event.reference());
    assert_eq!(
        restored.prepare_event().unwrap().original_bytes(),
        terminal_event.original_bytes()
    );
    assert_eq!(
        restored.prepare_results().unwrap().original_bytes(),
        f.tip.prepare_results().unwrap().original_bytes()
    );
    assert_eq!(restored.own_turns(seats()[0]), Some(2));
    assert_eq!(restored.own_turns(seats()[1]), Some(3));
}
#[test]
#[ignore = "requires explicit genuine catalogue/vocabulary; no durable acceptance claim"]
fn actual_first_turn_concession_retains_below_threshold_counts() {
    let mut f = checkpoint_fixture(None);
    advance(&mut f, seats()[0], PlayerAction::Concede {});
    let restored = replay(&f);
    assert_eq!(restored.own_turns(seats()[0]), Some(0));
    assert_eq!(restored.own_turns(seats()[1]), Some(1));
    assert_eq!(
        restored.terminal().unwrap().unwrap().winner_seat_ids(),
        &[seats()[1]]
    );
    let evidence: Value =
        serde_json::from_slice(restored.prepare_results().unwrap().original_bytes()).unwrap();
    assert_eq!(evidence["turnStarts"][0]["ownTurns"], "0");
    assert_eq!(evidence["turnStarts"][1]["ownTurns"], "1");
}

struct SourceOnlyReplayDisplay<'a> {
    programs: &'a NativeProgramSource,
    context: spellwake_engine::game::GameContext,
}

impl NativeDisplaySource for SourceOnlyReplayDisplay<'_> {
    fn context(&self) -> &spellwake_engine::game::GameContext {
        &self.context
    }
    fn scope(&self) -> NativeDisplayScope {
        NativeDisplayScope::SourceOnly
    }
    fn object(
        &self,
        _: &spellwake_engine::object_identity::GameObject,
        definition: &spellwake_engine::program::Program,
    ) -> Result<NativeObjectDisplay, NativeProjectionError> {
        let ch = self
            .programs
            .printed_card(definition)
            .map_err(NativeProjectionError::metadata)?
            .map(|c| c.characteristics().clone())
            .ok_or(NativeProjectionError::MissingDisplay)?;
        Ok(NativeObjectDisplay {
            definition_id: None,
            printing_id: None,
            custom_card_revision_id: None,
            characteristics: ch,
        })
    }
    fn ability(
        &self,
        _: &spellwake_engine::object_identity::GameObject,
        _: &spellwake_engine::characteristics::ObjectCharacteristics,
        ability: NativeAbilityReference,
    ) -> Result<String, NativeProjectionError> {
        Ok(match ability {
            NativeAbilityReference::Mana(_) => "Mana ability",
            NativeAbilityReference::Activated(_) => "Activated ability",
            NativeAbilityReference::Triggered(_) => "Triggered ability",
            NativeAbilityReference::Continuous(_) => "Static ability",
        }
        .into())
    }
    fn stack(
        &self,
        _: &spellwake_engine::stack_window::StackEntry,
        _: Option<&spellwake_engine::program::ExecutableBody>,
        _: Option<&spellwake_engine::occurrence::ExecutionBindings>,
    ) -> Result<String, NativeProjectionError> {
        Ok("Stack entry".into())
    }
    fn token_preview(
        &self,
        _: &spellwake_engine::token::TokenEntryPreview,
    ) -> Result<String, NativeProjectionError> {
        Err(NativeProjectionError::MissingDisplay)
    }
    fn trigger(
        &self,
        _: &spellwake_engine::occurrence::WaitingInstance,
    ) -> Result<String, NativeProjectionError> {
        Ok("Triggered ability".into())
    }
    fn prevention_attempt(
        &self,
        _: &spellwake_engine::effects::DamageAllocationDomain,
    ) -> Result<String, NativeProjectionError> {
        Ok("Damage".into())
    }
}

#[test]
#[ignore = "requires explicit genuine catalogue/vocabulary; verifies durable cold-replay seat knowledge parity"]
fn seat_knowledge_and_compact_prompt_survive_exact_cold_checkpoint_replay() {
    let programs = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let opening = prepare_initial(ids(), programs, spec()).unwrap();
    let initial_bytes = opening.original_bytes().to_vec();
    let initial_ref = opening.reference().clone();
    let mut tip = NativeCheckpoint::initial(opening).unwrap();
    let display = SourceOnlyReplayDisplay {
        programs,
        context: tip.state().basis().context.clone(),
    };
    let mut state_records: Vec<(NativeArtifactRef, Vec<u8>)> =
        vec![(tip.reference().clone(), tip.original_bytes().to_vec())];
    let mut input_records: Vec<(NativeArtifactRef, Vec<u8>)> = Vec::new();

    // Starting player mulligans once, both keep, starting player bottoms 1 privately.
    let first_actor =
        seats()[usize::try_from(tip.state().pending_decision().unwrap().chooser().0).unwrap()];
    let other_actor = if first_actor == seats()[0] {
        seats()[1]
    } else {
        seats()[0]
    };

    let mut first_mulliganed = false;
    for _ in 0..8 {
        let Some(pending) = tip.state().pending_decision() else {
            break;
        };
        let viewer = seats()[usize::try_from(pending.chooser().0).unwrap()];
        let mut scope = NativeScope::new(tip.state(), tip.binding(viewer).unwrap()).unwrap();
        let meta = NativeProjectionMetadata {
            through_event_sequence: u64::try_from(input_records.len()).unwrap().into(),
            deadline_at: None,
            disclosure: NativeDisclosure::LiveParticipant,
        };
        let snap = scope.project(&meta, &display).unwrap();
        let dec = &snap.decisions[0];
        let action = match &dec.domain {
            crate::player_wire::PlayerDomain::Options { options, .. } => {
                let idx = if viewer == first_actor && !first_mulliganed {
                    first_mulliganed = true;
                    1
                } else {
                    0
                };
                PlayerAction::Choose {
                    decision: dec.decision.clone(),
                    response: PlayerChoice::Option {
                        option: options[idx].handle.clone(),
                    },
                }
            }
            crate::player_wire::PlayerDomain::Objects { objects, .. } => PlayerAction::Choose {
                decision: dec.decision.clone(),
                response: PlayerChoice::Objects {
                    objects: vec![objects[0].clone()],
                },
            },
            _ => panic!("unexpected opening domain"),
        };
        let proposal = scope
            .propose(&PlayerCommand {
                request_id: Uuid::new_v4(),
                basis: scope.basis().clone(),
                action,
            })
            .unwrap();
        let bytes = scope.encode_input(&proposal).unwrap();
        let input_ref = NativeArtifactRef::for_bytes(
            ids().game_id,
            ids().context_id,
            NativeArtifactRole::Input,
            &bytes,
        )
        .unwrap();
        let next = with_store(&bytes, &input_ref, |store| {
            tip.append(
                store,
                input_ref.clone(),
                NativeCheckpointInput::Player,
                viewer,
            )
        })
        .unwrap();
        input_records.push((input_ref, bytes));
        state_records.push((next.reference().clone(), next.original_bytes().to_vec()));
        tip = next;
    }
    assert!(first_mulliganed);

    // Cold replay from scratch through `load_checkpoint`.
    let mut steps = vec![];
    for (r, b) in state_records.iter().rev() {
        steps.extend(checkpoint_read_steps(r, b));
    }
    steps.extend(checkpoint_read_steps(&initial_ref, &initial_bytes));
    for (r, b) in &input_records {
        steps.extend(checkpoint_read_steps(r, b));
    }
    let mock = Mock::new(steps);
    let recovered = load_checkpoint(
        &checkpoint_store(&mock),
        &NativeCheckpointExpected::new(tip.reference().clone(), tip.basis().clone()).unwrap(),
        &expected(initial_ref),
        programs,
        NativeRecoveryBudget::new(state_records.len(), 100_000_000).unwrap(),
    )
    .unwrap();
    mock.finish();

    let meta = NativeProjectionMetadata {
        through_event_sequence: u64::try_from(input_records.len()).unwrap().into(),
        deadline_at: None,
        disclosure: NativeDisclosure::LiveParticipant,
    };

    for viewer in [first_actor, other_actor] {
        let mut live_scope = NativeScope::new(tip.state(), tip.binding(viewer).unwrap()).unwrap();
        let mut rec_scope =
            NativeScope::new(recovered.state(), recovered.binding(viewer).unwrap()).unwrap();
        let live_snap = live_scope.project(&meta, &display).unwrap();
        let rec_snap = rec_scope.project(&meta, &display).unwrap();
        let live_know = tip.seat_knowledge(viewer, &display, Some(&live_scope), &live_snap);
        let rec_know = recovered.seat_knowledge(viewer, &display, Some(&rec_scope), &rec_snap);
        assert!(
            live_know == rec_know,
            "cold replay must reconstruct identical seat knowledge"
        );
        if viewer == first_actor {
            assert_eq!(live_know.visible_library_cards.len(), 1);
            assert_eq!(
                live_know.visible_library_cards[0].position,
                crate::ai_runtime::VisibleLibraryPosition::Bottom
            );
            assert_eq!(
                live_know.visible_library_cards[0].source,
                crate::ai_runtime::KnowledgeSource::PrivateMulliganBottom
            );
        } else {
            assert!(
                live_know.visible_library_cards.is_empty(),
                "opponent must never see private mulligan bottom card"
            );
        }
        let (live_prompt, _) = crate::ai_runtime::compact::compact_prompt_with_knowledge(
            &live_snap,
            &crate::ai_runtime::PriorityAffordances::default(),
            Some(&live_know),
            &[],
            None,
        )
        .unwrap();
        let (rec_prompt, _) = crate::ai_runtime::compact::compact_prompt_with_knowledge(
            &rec_snap,
            &crate::ai_runtime::PriorityAffordances::default(),
            Some(&rec_know),
            &[],
            None,
        )
        .unwrap();
        let live_state: Value =
            serde_json::from_str(live_prompt.split_once("\n\nSTATE:\n").unwrap().1).unwrap();
        let rec_state: Value =
            serde_json::from_str(rec_prompt.split_once("\n\nSTATE:\n").unwrap().1).unwrap();
        assert_eq!(
            live_state.get("known_opp_hand"),
            rec_state.get("known_opp_hand")
        );
        assert_eq!(
            live_state.get("visible_library"),
            rec_state.get("visible_library")
        );
        assert_eq!(
            live_state.get("historical_reveals"),
            rec_state.get("historical_reveals")
        );
        assert_eq!(live_state.get("cards"), rec_state.get("cards"));
    }
    // Persisted checkpoint artifacts remain byte-identical.
    assert_eq!(recovered.original_bytes(), tip.original_bytes());
    assert_eq!(
        recovered.prepare_results().unwrap().original_bytes(),
        tip.prepare_results().unwrap().original_bytes()
    );
}
