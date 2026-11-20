//! Source9/original20 cases through GameState::advance, independently of Forge.
//!
//! The initial calendar, control history, library filler and floating mana are
//! disclosed fixture premises. These are privileged native observations, never
//! seat-safe views, old capture-epoch data, ordinary host or full-card qualification.
#[path = "support/selection_native_observations.rs"]
mod native_observations;
#[path = "support/resolution_selection_source.rs"]
mod source_input;

use serde_json::{Value, json};
use source_input::{
    CastSpec, Fixture, P1, P2, P3, Source, cost, digest, payment, required_path, seat, source,
    strings, text,
};
use spellwake_engine::{
    draw::GameOutcome,
    effects::{EffectError, ResolutionSelectionKind, ResolutionSelectionStage},
    game::{
        DecisionId, DecisionReply, GameCommand, GameError, GameInput, GameState, NativeCandidate,
        RuleFrame,
    },
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaType},
    object_identity::{CardId, ObjectId, ObjectZone},
    occurrence::{
        EventCause, RulesEvent, RulesOccurrence, RulesRecord, TriggerInstanceId, ZoneReason,
    },
    priority::{PriorityPhase, SeatId},
    program::{Instruction, ProgramKind},
    stack_window::{StackEntryId, WindowPhase},
    turn::Step,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::Write as _,
};

#[derive(Debug, Clone)]
struct Call {
    input: GameInput,
    result: Result<NativeCandidate, GameError>,
}
/// A test records original typed inputs. Replay never derives an input from
/// native expected output, a Forge response, a changed basis or a repaired reply.
struct Run<'a> {
    source: &'a Source,
    fixture: Fixture,
    initial: GameState,
    state: GameState,
    calls: Vec<Call>,
    tokens: BTreeMap<String, ObjectId>,
    entries: BTreeMap<String, StackEntryId>,
    decisions: BTreeMap<String, DecisionId>,
    obligations: BTreeMap<String, &'static str>,
}
impl<'a> Run<'a> {
    fn new(source: &'a Source, fixture: Fixture) -> Self {
        let initial = fixture.initialized.state().clone();
        assert_eq!(initial.turn().unwrap().position().step, Step::PrecombatMain);
        Self {
            source,
            fixture,
            state: initial.clone(),
            initial,
            calls: vec![],
            tokens: BTreeMap::new(),
            entries: BTreeMap::new(),
            decisions: BTreeMap::new(),
            obligations: BTreeMap::new(),
        }
    }
    fn object(&self, alias: &str) -> ObjectId {
        self.tokens
            .get(alias)
            .copied()
            .unwrap_or_else(|| self.fixture.object(&self.state, alias))
    }
    fn input(&self, actor: SeatId, command: GameCommand) -> GameInput {
        GameInput {
            basis: self.state.basis().clone(),
            actor,
            command,
        }
    }
    fn call(&mut self, input: GameInput) -> &Result<NativeCandidate, GameError> {
        let before = self.state.clone();
        let result = self.state.advance(input.clone());
        assert_eq!(
            self.state, before,
            "every rejected or prepared root call leaves the accepted source exactly unchanged"
        );
        assert_eq!(
            self.state.advance(input.clone()),
            result,
            "same original typed input, full exact Result equality before adoption"
        );
        if let Ok(candidate) = &result {
            self.state = candidate.state().clone();
        }
        self.calls.push(Call { input, result });
        &self.calls.last().unwrap().result
    }
    fn ok(&mut self, actor: SeatId, command: GameCommand) -> &NativeCandidate {
        let input = self.input(actor, command);
        self.call(input)
            .as_ref()
            .expect("source-qualified native call")
    }
    fn reply(&mut self, choice: DecisionReply) -> &NativeCandidate {
        let decision = self
            .state
            .pending_decision()
            .expect("actual pending decision");
        self.ok(
            decision.chooser(),
            GameCommand::Reply {
                decision: decision.id(),
                choice,
            },
        )
    }
    fn reject(&mut self, label: &str, input: GameInput, expected: GameError) {
        let before = self.state.clone();
        assert_eq!(self.call(input), &Err(expected), "{label}");
        assert_eq!(
            self.state, before,
            "{label}: exact latest accepted source, including earlier choices/orders"
        );
        assert!(
            self.obligations
                .insert(
                    label.to_owned(),
                    "EXECUTED_NATIVE_TYPED_ERR_EXACT_SOURCE_UNCHANGED"
                )
                .is_none()
        );
    }
    fn cast(&mut self, spell: &CastSpec) -> StackEntryId {
        let expected_cost = cost(&self.source.program(&spell.oracle));
        let old = self.object(&spell.alias);
        let command = match spell.target {
            Some(target) => GameCommand::CastPaid {
                spell: old,
                target: spellwake_engine::targets::TargetRef::Player(target),
                payment: payment(&expected_cost),
            },
            None => GameCommand::CastUntargeted {
                spell: old,
                payment: Some(payment(&expected_cost)),
            },
        };
        let result = self.ok(spell.caster, command);
        let paid = result
            .mana_payment()
            .expect("real native source-cost payment receipt");
        assert_eq!(paid.payer, spell.caster);
        assert_eq!(paid.cost, expected_cost);
        assert_eq!(paid.spend, payment(&expected_cost).spend);
        let casts: Vec<_> = events(result)
            .flat_map(|e| &e.occurrences)
            .filter_map(|o| match &o.value {
                RulesOccurrence::CastCompleted(c) => Some(c),
                _ => None,
            })
            .collect();
        let [cast] = casts.as_slice() else {
            panic!("one actual completed paid physical cast")
        };
        assert_eq!(cast.entry(), paid.source);
        assert_ne!(cast.spell().object.id(), old);
        let entry = cast.entry();
        self.entries.insert(spell.alias.clone(), entry);
        entry
    }
    /// Exactly one real priority round, stopping at the executor's next boundary.
    /// Calling this only with a nonempty stack avoids advancing to an unrelated step.
    fn pass_round(&mut self) {
        let count = self.state.execution().window().participants().len();
        assert!(!self.state.execution().window().entries().is_empty());
        for _ in 0..count {
            assert!(self.state.pending_decision().is_none());
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                self.state.execution().window().phase()
            else {
                panic!("actual priority holder required")
            };
            self.ok(holder, GameCommand::Pass);
        }
    }
    fn all_events(&self) -> impl Iterator<Item = &RulesEvent> {
        events(&self.fixture.initialized).chain(
            self.calls
                .iter()
                .filter_map(|c| c.result.as_ref().ok())
                .flat_map(events),
        )
    }
    fn replay(&self) {
        let mut actual = self.initial.clone();
        for (index, call) in self.calls.iter().enumerate() {
            let saved = actual.clone();
            let result = actual.advance(call.input.clone());
            assert_eq!(actual, saved, "replay source {index}");
            assert_eq!(
                result, call.result,
                "full native Result/candidate/facts/state at saved original input {index}"
            );
            if let Ok(candidate) = result {
                actual = candidate.into_state();
            }
        }
        assert_eq!(
            actual, self.state,
            "full final native state, all participants and hidden state retained"
        );
    }
    fn finish(&self, name: &str, source_case: &str, extra: Value) -> Result<(), String> {
        self.replay();
        native_observations::write_if_requested(self, name, source_case)?;
        let mut debug = String::new();
        writeln!(
            &mut debug,
            "INITIALIZATION_NATIVE_CANDIDATE\n{:#?}",
            self.fixture.initialized
        )
        .unwrap();
        for (index, call) in self.calls.iter().enumerate() {
            writeln!(&mut debug, "\nSAVED_ORIGINAL_TYPED_INPUT_{index}\n{:#?}\nACTUAL_FULL_NATIVE_RESULT_{index}\n{:#?}", call.input, call.result).unwrap();
        }
        writeln!(
            &mut debug,
            "\nACTUAL_FINAL_FULL_NATIVE_STATE\n{:#?}",
            self.state
        )
        .unwrap();
        let dir = required_path("SPELLWAKE_SELECTION_CASE_EVIDENCE_OUTPUT")?;
        if !dir.is_dir() {
            return Err("explicit fresh native case evidence directory must already exist".into());
        }
        let report = json!({
            "kind": "SOURCE9_TYPED_NATIVE_CASE_NOT_ORDINARY_OR_FULL_CARD_QUALIFICATION",
            "case": source_case, "execution_name": name,
            "matrix": if matches!(source_case, "SD09" | "SD10") { "reviewed-raise-a2a01290799de7d0fcec2f2914883ed547b26863c34c8579794be04a921a35f8" } else { "original20-c2a23c4d5a65cd39dd9c95c73e3c8e52881c9f461833a78373a9d8abca3778dd" },
            "typed_root_calls": self.calls.len(), "accepted_calls": self.calls.iter().filter(|c| c.result.is_ok()).count(),
            "rejected_calls": self.calls.iter().filter(|c| c.result.is_err()).count(),
            "full_native_result_and_state_saved_typed_input_replay": "PASS_IN_MEMORY_NO_REPLY_REPAIR",
            "retained_native_debug": { "file": format!("{name}.native-debug.txt"), "sha256": digest(debug.as_bytes()), "bytes": debug.len(), "codec": "complete Rust Debug evidence; not a durable private action/replay grammar" },
            "authority_obligations": self.obligations, "actual_consumed_decisions": format!("{:?}", self.decisions), "details": extra,
            "limits": ["Prepared precombat main turn2 with cost-specific seeded floating mana; no natural Black/White/Green land funding", "Seeded prior control and already-present creatures are disclosed, no invented historical source", "Unspecified twenty-card libraries are explicitly source-Island fixture fillers; no draw or library effect occurs", "Privileged GameState only; no seat-safe privacy or ordinary capture/reader/host proof", "No Forge comparison performed by this test", "Registry9 bindings remain Partial; zero fully qualified cards"]
        });
        for (path, bytes) in [
            (
                dir.join(format!("{name}.native-debug.txt")),
                debug.into_bytes(),
            ),
            (
                dir.join(format!("{name}.json")),
                serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
            ),
        ] {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
fn events(candidate: &NativeCandidate) -> impl Iterator<Item = &RulesEvent> {
    candidate.rules_journal().records.iter().filter_map(|r| {
        if let RulesRecord::Event(e) = r {
            Some(e)
        } else {
            None
        }
    })
}
fn current_priority(state: &GameState, expected: SeatId) {
    assert_eq!(
        state.execution().window().phase(),
        WindowPhase::Priority(PriorityPhase::Ready { holder: expected })
    );
}

#[derive(Debug, Clone)]
struct Choice {
    player: SeatId,
    domain: Vec<String>,
    selected: Vec<String>,
}
fn planned_choices(case: &Value) -> Vec<Choice> {
    let id = text(&case["id"]);
    let request = &case["requested_choice_or_mutation"];
    let discard = matches!(
        id,
        "SD11" | "SD12" | "SD13" | "SD14" | "SD15" | "SD16" | "SD17"
    );
    let field = if discard {
        "hand_instance_ids"
    } else {
        "eligible_instance_ids"
    };
    let domains = &request[field];
    let labels = if domains.is_array() {
        vec!["P2".to_owned()]
    } else {
        strings(&case["initial_players_and_zones"]["turn"]["order"])
    };
    labels
        .into_iter()
        .map(|label| {
            let domain = strings(if domains.is_array() {
                domains
            } else {
                &domains[&label]
            });
            let count = if discard {
                if matches!(id, "SD15" | "SD16" | "SD17") {
                    3
                } else {
                    2
                }
            } else {
                1
            };
            let selected = domain.iter().take(count).cloned().collect();
            Choice {
                player: seat(&label),
                domain,
                selected,
            }
        })
        .collect()
}
/// Source-selected choices are fixed before any runtime output is examined.
/// The owner's chosen permutation is also fixed from these actual input aliases.
fn resolve_choices(
    run: &mut Run<'_>,
    before_resolution: &GameState,
    entry: StackEntryId,
    kind: ResolutionSelectionKind,
    choices: Vec<Choice>,
) {
    // This actual source state was retained after the payable cast and before
    // the real pass round. A zero-domain effect can finish entirely in that round.
    let before_cards = before_resolution.execution().window().cards().clone();
    let mut chosen = Vec::new();
    let mut physical: BTreeMap<SeatId, Vec<(ObjectId, CardId)>> = BTreeMap::new();
    let mut old_piles = BTreeMap::new();
    for &owner in run.state.execution().window().participants() {
        old_piles.insert(owner, physical_pile(before_resolution, owner));
    }
    for choice in &choices {
        for alias in &choice.selected {
            let object = run.object(alias);
            let current = before_cards.get(object).unwrap();
            chosen.push((choice.player, object));
            if let Some(card) = current.card_id() {
                physical
                    .entry(current.owner())
                    .or_default()
                    .push((object, card));
            }
        }
    }
    for (index, choice) in choices.iter().enumerate() {
        if choice.selected.is_empty() {
            continue;
        }
        let decision = run
            .state
            .pending_decision()
            .expect("positive effective source amount prompts even a forced subset");
        let RuleFrame::ResolutionSelect(frame) = decision.frame() else {
            panic!("source selection frame")
        };
        assert_eq!(frame.entry(), entry);
        assert_eq!(frame.instruction(), 0);
        assert_eq!(frame.operation(), kind);
        assert_eq!(frame.chooser(), choice.player);
        assert_eq!(frame.required(), choice.selected.len());
        let expected_domain: BTreeSet<_> = choice
            .domain
            .iter()
            .map(|alias| run.object(alias))
            .collect();
        assert_eq!(
            frame.domain().iter().copied().collect::<BTreeSet<_>>(),
            expected_domain
        );
        assert_eq!(frame.domain().len(), choice.domain.len());
        let work = run.state.execution().resolution_selection_state().unwrap();
        let ResolutionSelectionStage::Choosing {
            requirements,
            next_player,
            accepted,
        } = work.stage
        else {
            panic!("choosing owns existing cursor")
        };
        assert_eq!(requirements.len(), choices.len());
        assert_eq!(*next_player, index);
        assert_eq!(accepted.len(), index);
        for (row, expected) in requirements.iter().zip(&choices) {
            assert_eq!(row.player, expected.player);
            assert_eq!(row.required, expected.selected.len());
            assert_eq!(
                row.domain.iter().copied().collect::<BTreeSet<_>>(),
                expected
                    .domain
                    .iter()
                    .map(|alias| run.object(alias))
                    .collect()
            );
        }
        if index > 0 && text(&run.fixture.case["id"]) != "SD19" {
            let decision = run.state.pending_decision().unwrap().id();
            let malformed = run.input(
                choice.player,
                GameCommand::Reply {
                    decision,
                    choice: DecisionReply::SelectResolution(vec![]),
                },
            );
            let label = format!(
                "MECHANISM-{}-LATER-CHOOSER-{:?}-PREFIX",
                text(&run.fixture.case["id"]),
                choice.player
            );
            run.reject(
                &label,
                malformed,
                GameError::Effect(EffectError::InvalidSelectionCount),
            );
        }
        let selected = choice
            .selected
            .iter()
            .map(|alias| run.object(alias))
            .collect();
        run.reply(DecisionReply::SelectResolution(selected));
        if run.state.pending_decision().is_some() {
            assert_eq!(
                run.state.execution().window().cards(),
                &before_cards,
                "all choose and owner-order barrier before any physical movement"
            );
            assert!(
                run.calls
                    .last()
                    .unwrap()
                    .result
                    .as_ref()
                    .unwrap()
                    .rules_journal()
                    .records
                    .is_empty()
            );
        }
    }
    let participants = strings(&run.fixture.case["initial_players_and_zones"]["turn"]["order"]);
    let mut desired = BTreeMap::new();
    for owner in participants.iter().map(|p| seat(p)) {
        let Some(group) = physical.get(&owner) else {
            continue;
        };
        let submitted: Vec<_> = group.iter().rev().map(|(id, _)| *id).collect();
        let cards: Vec<_> = group.iter().rev().map(|(_, card)| *card).collect();
        desired.insert(owner, cards);
        if group.len() < 2 {
            continue;
        }
        let RuleFrame::ResolutionGraveyardOrder(frame) = run
            .state
            .pending_decision()
            .expect("physical multi-card owner order")
            .frame()
        else {
            panic!("owner-order frame")
        };
        assert_eq!(frame.entry(), entry);
        assert_eq!(frame.instruction(), 0);
        assert_eq!(frame.chooser(), owner);
        assert_eq!(
            frame.domain().iter().copied().collect::<BTreeSet<_>>(),
            group.iter().map(|(id, _)| *id).collect()
        );
        assert_eq!(run.state.execution().window().cards(), &before_cards);
        // A real later-order malformed reply proves the latest accepted earlier
        // owner's prefix survives; this is an extra mechanism probe, not a new SD row.
        if text(&run.fixture.case["id"]) == "SD15" && owner == P2 {
            let decision = run.state.pending_decision().unwrap().id();
            let input = run.input(
                owner,
                GameCommand::Reply {
                    decision,
                    choice: DecisionReply::OrderResolutionGraveyard(vec![]),
                },
            );
            run.reject(
                "MECHANISM-SD15-LATER-OWNER-PREFIX",
                input,
                GameError::Effect(EffectError::InvalidGraveyardPermutation),
            );
        }
        run.reply(DecisionReply::OrderResolutionGraveyard(submitted));
        if run.state.pending_decision().is_some() {
            assert_eq!(run.state.execution().window().cards(), &before_cards);
            assert!(
                run.calls
                    .last()
                    .unwrap()
                    .result
                    .as_ref()
                    .unwrap()
                    .rules_journal()
                    .records
                    .is_empty()
            );
        }
    }
    assert!(
        run.state.pending_decision().is_none(),
        "all source selections/orders complete"
    );
    let moving_spell_cards: BTreeSet<_> = run
        .entries
        .iter()
        .filter(|(_, e)| **e == entry)
        .filter_map(|(alias, _)| run.fixture.cards.get(alias).copied())
        .collect();
    for object in before_cards.iter() {
        if chosen.iter().any(|(_, id)| *id == object.id())
            || object
                .card_id()
                .is_some_and(|c| moving_spell_cards.contains(&c))
        {
            continue;
        }
        assert_eq!(
            run.state.execution().window().cards().get(object.id()),
            Some(object),
            "unchosen source object unchanged across complete selected operation and normal checks"
        );
    }
    // Zero-count effects can complete inside the real pass call that entered
    // resolution. Inspect every retained actual call, scoped to this exact native
    // entry/instruction, rather than an empty suffix after that pass.
    let selected_events: Vec<_> = run
        .calls
        .iter()
        .filter_map(|c| c.result.as_ref().ok())
        .flat_map(events)
        .filter(|event| {
            event.cause
                == EventCause::Instruction {
                    entry,
                    instruction: 0,
                }
        })
        .filter(|event| {
            event.occurrences.iter().any(|o| match &o.value {
                RulesOccurrence::ZoneChange(change) => matches!(
                    (kind, change.reason),
                    (
                        ResolutionSelectionKind::Sacrifice,
                        ZoneReason::Sacrifice { .. }
                    ) | (ResolutionSelectionKind::Discard, ZoneReason::Discard { .. })
                ),
                _ => false,
            })
        })
        .collect();
    if chosen.is_empty() {
        assert!(selected_events.is_empty());
    } else {
        let [event] = selected_events.as_slice() else {
            panic!("one atomic source-selected operation event")
        };
        assert_eq!(
            event.cause,
            EventCause::Instruction {
                entry,
                instruction: 0
            }
        );
        let movements: Vec<_> = event
            .occurrences
            .iter()
            .filter_map(|o| {
                if let RulesOccurrence::ZoneChange(change) = &o.value {
                    Some(change)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(movements.len(), chosen.len());
        let mut publication = Vec::new();
        for owner in participants.iter().map(|p| seat(p)) {
            if let Some(group) = physical.get(&owner) {
                publication.extend(group.iter().rev().map(|(object, _)| *object));
            }
            let mut tokens: Vec<_> = chosen
                .iter()
                .map(|(_, object)| *object)
                .filter(|object| {
                    let value = before_cards.get(*object).unwrap();
                    value.owner() == owner && value.card_id().is_none()
                })
                .collect();
            tokens.sort();
            publication.extend(tokens);
        }
        assert_eq!(
            movements
                .iter()
                .map(|movement| movement.before.object.id())
                .collect::<Vec<_>>(),
            publication,
            "logical owner-APNAP publication is distinct from reverse top-insertion application order"
        );
        assert_eq!(
            movements
                .iter()
                .map(|m| m.before.object.id())
                .collect::<BTreeSet<_>>(),
            chosen.iter().map(|(_, id)| *id).collect()
        );
        for (player, object) in &chosen {
            let movement = movements
                .iter()
                .find(|m| m.before.object.id() == *object)
                .unwrap();
            assert_eq!(
                movement.reason,
                match kind {
                    ResolutionSelectionKind::Sacrifice => ZoneReason::Sacrifice { player: *player },
                    ResolutionSelectionKind::Discard => ZoneReason::Discard { player: *player },
                }
            );
            assert_eq!(
                movement.before.object.zone(),
                match kind {
                    ResolutionSelectionKind::Sacrifice => ObjectZone::Battlefield,
                    ResolutionSelectionKind::Discard => ObjectZone::Hand(*player),
                }
            );
            assert_eq!(
                movement.after.object.zone(),
                ObjectZone::Graveyard(movement.before.object.owner())
            );
            assert_ne!(movement.before.object.id(), movement.after.object.id());
            if kind == ResolutionSelectionKind::Discard {
                assert!(movement.before.object.card_id().is_some());
            }
        }
    }
    for owner in participants.iter().map(|p| seat(p)) {
        let mut expected = desired.remove(&owner).unwrap_or_default();
        expected.extend(old_piles.remove(&owner).unwrap());
        // A completed instant/sorcery exits after the simultaneous selected batch.
        for (alias, actual_entry) in &run.entries {
            if *actual_entry != entry {
                continue;
            }
            let Some(card) = run.fixture.cards.get(alias) else {
                continue;
            };
            let object = run
                .state
                .execution()
                .window()
                .cards()
                .current_identity(*card)
                .unwrap();
            let current = run.state.execution().window().cards().get(object).unwrap();
            if current.owner() == owner && current.zone() == ObjectZone::Graveyard(owner) {
                expected.insert(0, *card);
            }
        }
        assert_eq!(
            physical_pile(&run.state, owner),
            expected,
            "owner permutation above unchanged old graveyard; later spell exit on top"
        );
    }
}
fn physical_pile(state: &GameState, owner: SeatId) -> Vec<CardId> {
    let cards = state.execution().window().cards();
    cards
        .graveyard(owner)
        .iter()
        .map(|id| {
            cards
                .get(*id)
                .unwrap()
                .card_id()
                .expect("no token physical card in ordered graveyard")
        })
        .collect()
}
fn raise_prefix(run: &mut Run<'_>, spell: &CastSpec) {
    let entry = run.cast(spell);
    let start = run.calls.len();
    run.pass_round();
    let ProgramKind::Spell(raise) = run.source.program(source_input::RAISE).kind else {
        panic!("source Raise spell")
    };
    let [Instruction::CreateTokens { creations }] = raise.body.instructions.as_slice() else {
        panic!("source Raise creation instruction")
    };
    let expected_token = creations[0]
        .definition
        .materialize(&run.source.vocabulary)
        .unwrap();
    // CR 613.7m: choose actual relative timestamps before any incarnation is
    // allocated. These issued members are preview coordinates, not token IDs.
    let pending = run
        .state
        .pending_decision()
        .expect("real simultaneous token timestamp order");
    let RuleFrame::TimestampOrder(frame) = pending.frame() else {
        panic!("actual Raise timestamp frame")
    };
    assert_eq!(frame.entry(), entry);
    assert_eq!(frame.instruction(), 0);
    assert_eq!(frame.chooser(), P1);
    assert_eq!(frame.members().len(), 2);
    assert_eq!(
        run.state
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token())
            .count(),
        0,
        "no invented token incarnation before actual timestamp acceptance"
    );
    for member in frame.members() {
        assert_eq!(member.creator, P1);
        assert_eq!(member.definition, expected_token);
    }
    let order = frame.members().iter().map(|member| member.member).collect();
    run.reply(DecisionReply::TimestampOrder(order));
    assert!(run.state.pending_decision().is_none());
    let mut births = Vec::new();
    for event in run.calls[start..]
        .iter()
        .filter_map(|c| c.result.as_ref().ok())
        .flat_map(events)
    {
        for occurrence in &event.occurrences {
            if let RulesOccurrence::TokenCreated { creator, after } = &occurrence.value {
                assert_eq!(*creator, P1);
                assert_eq!(
                    event.cause,
                    EventCause::Instruction {
                        entry,
                        instruction: 0
                    }
                );
                assert!(after.object.is_token());
                assert_eq!(after.object.card_id(), None);
                assert_eq!(after.object.zone(), ObjectZone::Battlefield);
                assert_eq!(after.object.controller(), Some(P1));
                assert_eq!(
                    run.state.execution().base_program(after.object.id()),
                    Some(&expected_token),
                    "complete real source token definition, not a generic creature substitute"
                );
                births.push(after.object.id());
            }
        }
    }
    assert_eq!(
        births.len(),
        2,
        "two actual Raise token births, bound from event receipts"
    );
    assert_ne!(births[0], births[1]);
    for (alias, object) in ["P1-BF-SOLDIER-1", "P1-BF-SOLDIER-2"]
        .into_iter()
        .zip(births)
    {
        assert!(run.tokens.insert(alias.into(), object).is_none());
    }
    assert_eq!(
        physical_pile(&run.state, P1),
        vec![run.fixture.cards["P1-GY-RAISE-1"]]
    );
    current_priority(&run.state, P1);
}
fn fleshbag_prefix(run: &mut Run<'_>, spell: &CastSpec) -> (StackEntryId, TriggerInstanceId) {
    let entry = run.cast(spell);
    let start = run.calls.len();
    run.pass_round();
    let rows: Vec<_> = run.calls[start..]
        .iter()
        .filter_map(|c| c.result.as_ref().ok())
        .flat_map(|c| c.rules_journal().records.iter())
        .collect();
    let discoveries: Vec<_> = rows
        .iter()
        .filter_map(|r| {
            if let RulesRecord::TriggerDiscovered(w) = r {
                Some(w.as_ref())
            } else {
                None
            }
        })
        .collect();
    let placements: Vec<_> = rows
        .iter()
        .filter_map(|r| {
            if let RulesRecord::TriggerPlaced(p) = r {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    let [waiting] = discoveries.as_slice() else {
        panic!("actual Fleshbag ETB discovery")
    };
    let [placed] = placements.as_slice() else {
        panic!("actual Fleshbag placement")
    };
    let permanent = run.calls[start..]
        .iter()
        .filter_map(|c| c.result.as_ref().ok())
        .find_map(NativeCandidate::permanent_entry)
        .unwrap();
    assert_eq!(permanent.entry, entry);
    assert_eq!(permanent.before.zone(), ObjectZone::Stack);
    assert_eq!(permanent.after.id(), run.object("P1-BF-FLESHBAG-1"));
    assert_eq!(permanent.after.zone(), ObjectZone::Battlefield);
    assert_eq!(waiting.source.object.id(), permanent.after.id());
    assert_eq!(waiting.controller, P1);
    assert_eq!(waiting.causes.len(), 1);
    let member = &waiting.causes.members()[0];
    assert_eq!(member.related_player, Some(P1));
    let event = run.calls[start..]
        .iter()
        .filter_map(|c| c.result.as_ref().ok())
        .flat_map(events)
        .find(|event| event.id == waiting.causes.event())
        .unwrap();
    let occurrence = event
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id == member.occurrence)
        .unwrap();
    let RulesOccurrence::ZoneChange(change) = &occurrence.value else {
        panic!("actual source permanent entry caused ETB")
    };
    assert_eq!(change.reason, ZoneReason::PermanentResolution);
    assert_eq!(change.before.object, permanent.before);
    assert_eq!(change.after.object, permanent.after);
    assert_eq!(
        waiting.body,
        run.source.program(source_input::FLESHBAG).triggered[0].body
    );
    assert_eq!(placed.instance, waiting.id);
    assert_eq!(placed.controller, P1);
    let binding = run
        .state
        .execution()
        .execution_bindings(placed.entry)
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.instance, waiting.id);
    assert_eq!(binding.source, waiting.source);
    assert_eq!(binding.causes, waiting.causes);
    let answer = (placed.entry, waiting.id);
    run.entries.insert("FLESHBAG-ETB".into(), answer.0);
    current_priority(&run.state, P1);
    answer
}

struct HistoricalAuthority {
    decision: DecisionId,
    object: ObjectId,
    frame: RuleFrame,
}
/// A separately named source9 history derivative. It never changes the original
/// matrix or claims its extra spell/graveyard row was part of the original case.
fn history_prefix(run: &mut Run<'_>, id: &str) -> HistoricalAuthority {
    let spell = run.fixture.spells[0].clone();
    assert_eq!(spell.alias, "COMPANION-HISTORY-SPELL");
    let entry = run.cast(&spell);
    let before_resolution = run.state.clone();
    run.pass_round();
    let pending = run.state.pending_decision().unwrap();
    let decision = pending.id();
    let frame = pending.frame().clone();
    let (stale, domain, selected, kind) = if id == "SD04" {
        let stale = "P2-GY-BEARS-STALE";
        (
            stale,
            vec!["P2-BF-BEARS-1", "P2-BF-FLESHBAG-1", stale],
            vec![stale],
            ResolutionSelectionKind::Sacrifice,
        )
    } else {
        let stale = "P2-GY-ISLAND-STALE";
        (
            stale,
            vec![
                "P2-HAND-BEARS-1",
                "P2-HAND-ISLAND-1",
                "P2-HAND-ALARM-1",
                stale,
                "COMPANION-EXTRA-ISLAND",
            ],
            vec![stale, "COMPANION-EXTRA-ISLAND"],
            ResolutionSelectionKind::Discard,
        )
    };
    let object = run.object(stale);
    resolve_choices(
        run,
        &before_resolution,
        entry,
        kind,
        vec![Choice {
            player: P2,
            domain: domain.into_iter().map(str::to_owned).collect(),
            selected: selected.into_iter().map(str::to_owned).collect(),
        }],
    );
    assert_ne!(
        object,
        run.object(stale),
        "genuine changed incarnation, derived from actual source9 movement"
    );
    assert!(run.state.execution().window().cards().get(object).is_none());
    current_priority(&run.state, P1);
    run.decisions
        .insert("CONSUMED-HISTORICAL-DECISION".into(), decision);
    HistoricalAuthority {
        decision,
        object,
        frame,
    }
}
fn foreign_authority(
    source: &Source,
    suffix: &str,
) -> Result<(spellwake_engine::game::GameBasis, DecisionId), String> {
    let mut run = Run::new(source, Fixture::build(source, "SD11"));
    let spell = run.fixture.spells[0].clone();
    run.cast(&spell);
    run.pass_round();
    let cards = ["P2-HAND-BEARS-1", "P2-HAND-ISLAND-1"]
        .iter()
        .map(|a| run.object(a))
        .collect();
    run.reply(DecisionReply::SelectResolution(cards));
    let decision = run
        .state
        .pending_decision()
        .expect("actual foreign owner-order decision")
        .id();
    let basis = run.state.basis().clone();
    run.finish(&format!("{suffix}-foreign-authority-helper"), "SD11", json!({ "purpose": "foreign actual basis and independently different pending decision only; not completed original SD11 execution", "ordinary_capture": "not run" }))?;
    Ok((basis, decision))
}
fn malformed_source_case(run: &mut Run<'_>, id: &str) -> Result<(), String> {
    let (foreign_basis, foreign_decision) =
        foreign_authority(run.source, &id.to_ascii_lowercase())?;
    let pending = run.state.pending_decision().unwrap();
    let decision = pending.id();
    assert_ne!(
        foreign_decision, decision,
        "independently observed other-choice identifier"
    );
    let choose = |ids| GameCommand::Reply {
        decision,
        choice: DecisionReply::SelectResolution(ids),
    };
    let p2_bear = run.object(if id == "SD04" {
        "P2-BF-BEARS-1"
    } else {
        "P2-HAND-BEARS-1"
    });
    let valid = if id == "SD04" {
        vec![p2_bear]
    } else {
        vec![p2_bear, run.object("P2-HAND-ISLAND-1")]
    };
    let wrong_zone = if id == "SD04" {
        vec![run.object("P1-HAND-EDICT-1")]
    } else {
        vec![p2_bear, run.object("P1-HAND-EDICT-1")]
    };
    let stale_row = if id == "SD04" {
        vec![run.object("P2-GY-BEARS-STALE")]
    } else {
        vec![p2_bear, run.object("P2-GY-ISLAND-STALE")]
    };
    let mut attempts = vec![(
        1,
        run.input(P1, choose(valid.clone())),
        GameError::WrongChooser,
    )];
    if id == "SD04" {
        attempts.extend([
            (
                2,
                run.input(P2, choose(vec![run.object("P2-BF-ISLAND-1")])),
                GameError::Effect(EffectError::SelectionOutsideDomain),
            ),
            (
                3,
                run.input(P2, choose(vec![run.object("P1-BF-BEARS-1")])),
                GameError::Effect(EffectError::SelectionOutsideDomain),
            ),
            (
                4,
                run.input(P2, choose(vec![])),
                GameError::Effect(EffectError::InvalidSelectionCount),
            ),
            (
                5,
                run.input(P2, choose(vec![p2_bear, p2_bear])),
                GameError::Effect(EffectError::InvalidSelectionCount),
            ),
            (
                6,
                run.input(P2, choose(vec![p2_bear, run.object("P2-BF-FLESHBAG-1")])),
                GameError::Effect(EffectError::InvalidSelectionCount),
            ),
            (
                7,
                run.input(P2, choose(wrong_zone)),
                GameError::Effect(EffectError::SelectionOutsideDomain),
            ),
            (
                8,
                run.input(P2, choose(stale_row)),
                GameError::Effect(EffectError::SelectionOutsideDomain),
            ),
            (
                9,
                run.input(
                    P1,
                    GameCommand::ActivateMana {
                        source: run.object("P1-BF-ISLAND-1"),
                        ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
                    },
                ),
                GameError::DecisionRequired,
            ),
            (
                10,
                run.input(
                    P1,
                    GameCommand::Cast {
                        spell: run.object("P1-HAND-EDICT-1"),
                        target: spellwake_engine::targets::TargetRef::Player(P2),
                    },
                ),
                GameError::DecisionRequired,
            ),
        ]);
    } else {
        attempts.extend([
            (
                2,
                run.input(P2, choose(vec![p2_bear])),
                GameError::Effect(EffectError::InvalidSelectionCount),
            ),
            (
                3,
                run.input(
                    P2,
                    choose(vec![
                        p2_bear,
                        run.object("P2-HAND-ISLAND-1"),
                        run.object("P2-HAND-ALARM-1"),
                    ]),
                ),
                GameError::Effect(EffectError::InvalidSelectionCount),
            ),
            (
                4,
                run.input(P2, choose(vec![p2_bear, p2_bear])),
                GameError::Effect(EffectError::DuplicateSelection),
            ),
            (
                5,
                run.input(P2, choose(wrong_zone)),
                GameError::Effect(EffectError::SelectionOutsideDomain),
            ),
            (
                6,
                run.input(P2, choose(stale_row)),
                GameError::Effect(EffectError::SelectionOutsideDomain),
            ),
        ]);
    }
    for (number, input, expected) in attempts {
        run.reject(&format!("{id}-V{number}"), input, expected);
    }
    let offset = if id == "SD04" { 11 } else { 7 };
    let mut stale = run.input(P2, choose(valid.clone()));
    stale.basis = run.initial.basis().clone();
    run.reject(&format!("{id}-V{offset}"), stale, GameError::StaleRevision);
    let mut foreign = run.input(P2, choose(valid.clone()));
    foreign.basis = foreign_basis;
    run.reject(
        &format!("{id}-V{}", offset + 1),
        foreign,
        GameError::WrongGame,
    );
    // The exact original prefix has no earlier same-game choice. Do not label
    // a public fabricated DecisionId as a genuine consumed earlier decision.
    run.obligations.insert(
        format!("{id}-V{}", offset + 2),
        "PENDING_EXACT_ORIGINAL_HISTORY; separately named companion executes real consumed D_old",
    );
    let wrong = run.input(
        P2,
        GameCommand::Reply {
            decision: foreign_decision,
            choice: DecisionReply::SelectResolution(valid),
        },
    );
    run.reject(
        &format!("{id}-V{}", offset + 3),
        wrong,
        GameError::WrongDecision,
    );
    let wrong_kind = run.input(
        P2,
        GameCommand::Reply {
            decision,
            choice: DecisionReply::OrderResolutionGraveyard(vec![]),
        },
    );
    run.reject(
        &format!("MECHANISM-{id}-WRONG-REPLY-KIND"),
        wrong_kind,
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    Ok(())
}
fn assert_final_source_facts(run: &Run<'_>, id: &str) {
    let seats = strings(&run.fixture.case["initial_players_and_zones"]["turn"]["order"]);
    assert!(run.state.execution().window().entries().is_empty());
    assert_eq!(
        run.state
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|c| c.card_id().is_some())
            .count(),
        run.initial
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|c| c.card_id().is_some())
            .count(),
        "no physical card invented or deleted"
    );
    assert!(run.state.pending_decision().is_none());
    assert_eq!(run.state.execution().waiting_triggers().count(), 0);
    for label in &seats {
        let player = seat(label);
        let starting = run.fixture.case["initial_players_and_zones"][label]["life"]
            .as_i64()
            .unwrap();
        let expected = starting - i64::from(matches!(id, "SD05" | "SD06") && player == P2);
        assert_eq!(
            run.state.execution().life_total(player),
            Some(&expected.into())
        );
        for mana_type in ManaType::ALL {
            assert_eq!(
                run.state.execution().mana(player, mana_type),
                0.into(),
                "all explicitly funded source costs paid; rejected mana attempts create nothing"
            );
        }
        assert_eq!(
            run.state.execution().window().cards().library(player),
            run.initial.execution().window().cards().library(player),
            "no invented draw/library mutation"
        );
    }
    assert!(
        run.calls
            .iter()
            .filter_map(|c| c.result.as_ref().ok())
            .all(|c| c.draw_journal().events.is_empty())
    );
    assert!(
        !run.all_events()
            .flat_map(|e| &e.occurrences)
            .any(|o| matches!(o.value, RulesOccurrence::FailedDraw { .. }))
    );
    if id == "SD06" {
        assert_eq!(run.state.outcome(), Some(GameOutcome::Winner(P1)));
        assert_eq!(run.state.execution().life_total(P2), Some(&0.into()));
    } else {
        assert_eq!(run.state.outcome(), None);
        current_priority(&run.state, P1);
    }
    if matches!(id, "SD05" | "SD06") {
        let changes: Vec<_> = run
            .all_events()
            .flat_map(|e| &e.occurrences)
            .filter_map(|o| {
                if let RulesOccurrence::LifeChanged(change) = &o.value {
                    Some(change)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].player, P2);
        assert_eq!(changes[0].amount, 1.into());
        if id == "SD06" {
            let events: Vec<_> = run.all_events().collect();
            let sacrifice = events.iter().position(|e| e.occurrences.iter().any(|o| matches!(&o.value, RulesOccurrence::ZoneChange(c) if matches!(c.reason, ZoneReason::Sacrifice { .. })))).unwrap();
            let loss = events
                .iter()
                .position(|e| {
                    e.occurrences
                        .iter()
                        .any(|o| matches!(o.value, RulesOccurrence::LifeChanged(_)))
                })
                .unwrap();
            assert!(
                sacrifice < loss,
                "real sacrifice instruction precedes Geth life tail and later checks"
            );
        }
    }
    if matches!(id, "SD11" | "SD14") {
        assert_eq!(
            run.state
                .execution()
                .window()
                .cards()
                .get(run.object("P2-HAND-ALARM-1"))
                .unwrap()
                .zone(),
            ObjectZone::Hand(P2)
        );
    }
    if matches!(id, "SD09" | "SD10") {
        let first = run.tokens["P1-BF-SOLDIER-1"];
        let second = run.tokens["P1-BF-SOLDIER-2"];
        let cards = run.state.execution().window().cards();
        assert_eq!(cards.get(second).unwrap().zone(), ObjectZone::Battlefield);
        assert!(cards.get(second).unwrap().is_token());
        if id == "SD09" {
            assert_eq!(cards.get(first).unwrap().zone(), ObjectZone::Battlefield);
        } else {
            assert!(cards.get(first).is_none());
            let events: Vec<_> = run.all_events().collect();
            let moved = events.iter().position(|e| e.occurrences.iter().any(|o| matches!(&o.value, RulesOccurrence::ZoneChange(c) if c.before.object.id() == first && matches!(c.reason, ZoneReason::Sacrifice { player: P1 })))).unwrap();
            let moved_event = events[moved];
            let moved_after = moved_event
                .occurrences
                .iter()
                .find_map(|o| match &o.value {
                    RulesOccurrence::ZoneChange(change) if change.before.object.id() == first => {
                        Some(change.after.object.clone())
                    }
                    _ => None,
                })
                .unwrap();
            let exit = events.iter().position(|e| e.occurrences.iter().any(|o| matches!(&o.value, RulesOccurrence::ZoneChange(c) if c.before.object.card_id() == Some(run.fixture.cards["CASE-SPELL"]) && c.reason == ZoneReason::ResolutionExit))).unwrap();
            let ceased = events.iter().position(|e| e.occurrences.iter().any(|o| matches!(&o.value, RulesOccurrence::TokenCeased { object } if object == &moved_after))).unwrap();
            assert!(
                moved < exit && exit < ceased,
                "actual token move, actual spell exit, then distinct ordinary cessation boundary"
            );
        }
    }
    if id == "SD17" {
        assert_eq!(run.state.execution().window().participants(), &[P3, P1, P2]);
        for player in [P1, P2, P3] {
            assert_eq!(
                run.state
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .filter(|c| c.zone() == ObjectZone::Hand(player))
                    .count(),
                0
            );
        }
    }
}

fn run_original_case(id: &str) -> Result<(), String> {
    let source = source()?;
    let fixture = Fixture::build(source, id);
    let mut run = Run::new(source, fixture);
    let spells = run.fixture.spells.clone();
    if matches!(id, "SD18" | "SD19") {
        let (trigger, instance) = fleshbag_prefix(&mut run, &spells[0]);
        if id == "SD19" {
            // Actual P1 priority after the ETB placement; P2 receives priority
            // by one actual pass, then casts the real response above that entry.
            run.ok(P1, GameCommand::Pass);
            current_priority(&run.state, P2);
            let response = run.cast(&spells[1]);
            assert_eq!(
                run.state
                    .execution()
                    .window()
                    .entries()
                    .iter()
                    .map(|e| e.id())
                    .collect::<Vec<_>>(),
                vec![trigger, response]
            );
            let before_response = run.state.clone();
            run.pass_round();
            resolve_choices(
                &mut run,
                &before_response,
                response,
                ResolutionSelectionKind::Sacrifice,
                vec![Choice {
                    player: P1,
                    domain: vec!["P1-BF-FLESHBAG-1".into()],
                    selected: vec!["P1-BF-FLESHBAG-1".into()],
                }],
            );
            current_priority(&run.state, P1);
            assert_eq!(
                run.state
                    .execution()
                    .window()
                    .cards()
                    .get(run.object("P1-BF-FLESHBAG-1"))
                    .unwrap()
                    .zone(),
                ObjectZone::Graveyard(P1)
            );
            let binding = run
                .state
                .execution()
                .execution_bindings(trigger)
                .unwrap()
                .trigger
                .as_ref()
                .unwrap();
            assert_eq!(binding.instance, instance);
            assert_eq!(
                binding.source.object.zone(),
                ObjectZone::Battlefield,
                "captured actual ETB appearance survives source departure"
            );
            assert_ne!(binding.source.object.id(), run.object("P1-BF-FLESHBAG-1"));
            assert_eq!(
                run.state.execution().stack_body(trigger),
                Some(&source.program(source_input::FLESHBAG).triggered[0].body)
            );
            let before_trigger = run.state.clone();
            run.pass_round();
            resolve_choices(
                &mut run,
                &before_trigger,
                trigger,
                ResolutionSelectionKind::Sacrifice,
                vec![
                    Choice {
                        player: P1,
                        domain: vec![],
                        selected: vec![],
                    },
                    Choice {
                        player: P2,
                        domain: vec!["P2-BF-BEARS-1".into()],
                        selected: vec!["P2-BF-BEARS-1".into()],
                    },
                ],
            );
        } else {
            let before_trigger = run.state.clone();
            run.pass_round();
            let choices = planned_choices(&run.fixture.case);
            resolve_choices(
                &mut run,
                &before_trigger,
                trigger,
                ResolutionSelectionKind::Sacrifice,
                choices,
            );
        }
    } else {
        let main = if matches!(id, "SD09" | "SD10") {
            raise_prefix(&mut run, &spells[0]);
            &spells[1]
        } else {
            &spells[0]
        };
        let entry = run.cast(main);
        let before_resolution = run.state.clone();
        run.pass_round();
        if matches!(id, "SD04" | "SD14") {
            malformed_source_case(&mut run, id)?;
        }
        let kind = if matches!(
            id,
            "SD11" | "SD12" | "SD13" | "SD14" | "SD15" | "SD16" | "SD17"
        ) {
            ResolutionSelectionKind::Discard
        } else {
            ResolutionSelectionKind::Sacrifice
        };
        let choices = planned_choices(&run.fixture.case);
        resolve_choices(&mut run, &before_resolution, entry, kind, choices);
    }
    assert_final_source_facts(&run, id);
    run.finish(&id.to_ascii_lowercase(), id, json!({
        "source_case_title": run.fixture.case["title"],
        "native_execution": "PASS_SCOPED_ASSERTIONS_WITH_EXPLICIT_PENDING_AUTHORITY_AND_PRIVACY_LIMITS",
        "reference_disposition": if matches!(id, "SD04" | "SD14") { "NativeOnly" } else { "native execution only; independent Forge comparison not run here" },
        "stale_named_row": if matches!(id, "SD04" | "SD14") { "Original pre-seeded current graveyard ObjectId: wrong-zone rejection executed. Genuine obsolete receipt is covered only by separate source9 history companion." } else { "not applicable" },
        "three_seat": if id == "SD17" { "actual three participants in cyclic rotation, all native state/results/inputs retained and replayed; ordinary capture/readers/replay and seat-safe/privacy/host remain pending" } else { "not applicable" },
        "old_capture_epoch": "never used",
    }))
}
fn run_history_companion(id: &str) -> Result<(), String> {
    let source = source()?;
    let mut run = Run::new(source, Fixture::build_with_history(source, id, true));
    let history = history_prefix(&mut run, id);
    let main = run.fixture.spells[1].clone();
    let entry = run.cast(&main);
    let before_resolution = run.state.clone();
    run.pass_round();
    let pending = run.state.pending_decision().unwrap();
    assert_ne!(pending.id(), history.decision);
    assert_ne!(pending.frame(), &history.frame);
    let decision = pending.id();
    let valid: Vec<_> = if id == "SD04" {
        vec![run.object("P2-BF-BEARS-1")]
    } else {
        vec![
            run.object("P2-HAND-BEARS-1"),
            run.object("P2-HAND-ISLAND-1"),
        ]
    };
    let old_decision = run.input(
        P2,
        GameCommand::Reply {
            decision: history.decision,
            choice: DecisionReply::SelectResolution(valid),
        },
    );
    run.reject(
        &format!("COMPANION-{id}-GENUINE-CONSUMED-D-OLD"),
        old_decision,
        GameError::WrongDecision,
    );
    let stale_objects = if id == "SD04" {
        vec![history.object]
    } else {
        vec![run.object("P2-HAND-BEARS-1"), history.object]
    };
    let stale_object = run.input(
        P2,
        GameCommand::Reply {
            decision,
            choice: DecisionReply::SelectResolution(stale_objects),
        },
    );
    run.reject(
        &format!("COMPANION-{id}-GENUINE-OBSOLETE-OBJECT"),
        stale_object,
        GameError::Effect(EffectError::SelectionOutsideDomain),
    );
    let choices = planned_choices(&run.fixture.case);
    resolve_choices(
        &mut run,
        &before_resolution,
        entry,
        if id == "SD04" {
            ResolutionSelectionKind::Sacrifice
        } else {
            ResolutionSelectionKind::Discard
        },
        choices,
    );
    assert_final_source_facts(&run, id);
    run.finish(&format!("{}-real-history-companion", id.to_ascii_lowercase()), id, json!({
        "scope": "separately named source9 authority/history derivative, not an original20 row and not Forge equivalent",
        "differences": if id == "SD04" { "Initial STALE Grizzly is Battlefield, then actual extra paid Edict sacrifices it. Extra physical Edict remains in P1 graveyard; explicit extra cost funded. Original eligible creatures/hand remain untouched." } else { "Initial STALE Island is Hand, plus one additional genuine source Island. Extra paid Mind Rot discards those two through real selection/order. Extra Island and Mind Rot stay in graveyards; original three hand cards remain untouched; explicit extra cost funded." },
        "actual_consumed_decision": format!("{:?}", history.decision), "actual_old_frame": format!("{:?}", history.frame), "actual_obsolete_object": format!("{:?}", history.object),
        "qualification_credit": "only the separately disclosed actual prefix; no retroactive original-row/card/privacy qualification",
    }))
}
macro_rules! source_case {
    ($name:ident, $id:literal) => {
        #[test]
        #[ignore = "requires exact full catalogue, original9 manifest, original20/reviewed Raise matrices, saved CR, genuine vocabulary and fresh explicit evidence directory"]
        fn $name() -> Result<(), String> { run_original_case($id) }
    };
}
source_case!(sd01_edict_one_creature, "SD01");
source_case!(sd02_edict_no_creatures, "SD02");
source_case!(sd03_edict_stolen_owner_graveyard, "SD03");
source_case!(sd04_edict_native_authority_rejections_and_retry, "SD04");
source_case!(sd05_geth_zero_sacrifice_life_tail, "SD05");
source_case!(sd06_geth_sacrifice_before_life_loss_and_sba_loss, "SD06");
source_case!(sd07_blood_each_player_atomic_barrier, "SD07");
source_case!(sd08_blood_multiple_and_zero_domains, "SD08");
source_case!(sd09_reviewed_physical_raise_stolen_owner_order, "SD09");
source_case!(
    sd10_reviewed_physical_raise_token_sacrifice_and_later_cessation,
    "SD10"
);
source_case!(sd11_mind_rot_exact_hidden_choice_and_owner_order, "SD11");
source_case!(sd12_mind_rot_short_hand, "SD12");
source_case!(sd13_mind_rot_empty_hand_library_no_failed_draw, "SD13");
source_case!(sd14_mind_rot_native_authority_rejections_and_retry, "SD14");
source_case!(sd15_skeins_apnap_choose_and_owner_order_barriers, "SD15");
source_case!(sd16_skeins_short_and_empty_hands, "SD16");
source_case!(
    sd17_skeins_genuine_three_seat_native_replay_privacy_still_pending,
    "SD17"
);
source_case!(sd18_fleshbag_actual_cast_etb_self_sacrifice, "SD18");
source_case!(
    sd19_fleshbag_actual_edict_response_captured_departed_source,
    "SD19"
);

#[test]
#[ignore = "requires the same genuine source inputs; separately named SD04 history derivative, never original20 credit"]
fn companion_sd04_genuine_consumed_decision_and_obsolete_object() -> Result<(), String> {
    run_history_companion("SD04")
}
#[test]
#[ignore = "requires the same genuine source inputs; separately named SD14 history derivative, never original20 credit"]
fn companion_sd14_genuine_consumed_decision_and_obsolete_object() -> Result<(), String> {
    run_history_companion("SD14")
}

// SD20 deliberately has no passing Rust test. The unchanged SourceGap is kept
// in the exact pinned twenty-case ledger; no qualified replacement/prohibition
// source or corresponding semantics were supplied by the original nine cards.
