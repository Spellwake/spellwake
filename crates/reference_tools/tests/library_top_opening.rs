//! Genuine source4, natural opening, paid ordinary play and full native Result replay.
use spellwake_engine::{
    checks::CheckDecision,
    draw::{DrawEvent, GameOutcome},
    effects::{LibraryChoiceKind, LibraryTopReply},
    game::{
        DecisionReply, GameBasis, GameCommand, GameError, GameId, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaPayment, ManaSpend, ManaType},
    object_identity::{ObjectId, ObjectZone},
    opening::{MulliganChoice, OpeningDecision},
    priority::{PriorityPhase, SeatId},
    shuffle::EntropyTape,
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::Step,
};
use spellwake_reference_tools::{
    creature_vocabulary::load_vocabulary,
    ordinary_game::{
        fixture::{A, B},
        library_top::{LibraryTopSource, TeachingDeck},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
const ISLAND: &str = "b2c6aa39-2d2a-459c-a555-fb48ba993373";
const SCOUR: &str = "83101ba8-a569-4827-8c53-9ca0dfcd59a7";
fn input(key: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("explicit genuine source input required: {key}"))
}
fn load_source() -> LibraryTopSource {
    LibraryTopSource::load(
        &input("SPELLWAKE_CATALOGUE_METADATA"),
        &input("SPELLWAKE_CATALOGUE_INPUT"),
        load_vocabulary(&input("SPELLWAKE_CREATURE_VOCABULARY_INPUT")).unwrap(),
    )
    .unwrap()
}
// Deterministic test words are supplied to the native entropy API. This is not
// evidence of cryptographic randomness or a second shuffle implementation.
fn words(count: usize) -> Vec<u64> {
    let mut seed = 0x0ddc_0ffe_e123_4567_u64;
    (0..count)
        .map(|_| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            seed
        })
        .collect()
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Player(GameInput),
    Entropy { basis: GameBasis, words: Vec<u64> },
}
impl Call {
    fn apply(&self, state: &GameState) -> Result<NativeCandidate, GameError> {
        match self {
            Self::Player(input) => state.advance(input.clone()),
            Self::Entropy { basis, words } => state.supply_entropy(basis, words.clone()),
        }
    }
}
struct Run {
    state: GameState,
    calls: Vec<(Call, Result<NativeCandidate, GameError>)>,
    cast: BTreeMap<String, usize>,
    choices: usize,
    payments: usize,
    productions: usize,
    lands: usize,
    failures: usize,
}
impl Run {
    fn call(&mut self, call: Call) -> Result<NativeCandidate, GameError> {
        let before = self.state.clone();
        let result = call.apply(&self.state);
        assert_eq!(
            self.state, before,
            "native candidate must not mutate its predecessor"
        );
        if let Ok(candidate) = &result {
            self.payments += usize::from(candidate.mana_payment().is_some());
            self.productions += usize::from(candidate.mana_production().is_some());
            self.lands += usize::from(candidate.land_play().is_some());
            self.state = candidate.state().clone();
        } else {
            self.failures += 1;
        }
        self.calls.push((call, result.clone()));
        result
    }
    fn command(
        &mut self,
        actor: SeatId,
        command: GameCommand,
    ) -> Result<NativeCandidate, GameError> {
        self.call(Call::Player(GameInput {
            basis: self.state.basis().clone(),
            actor,
            command,
        }))
    }
    fn reply(
        &mut self,
        actor: SeatId,
        choice: DecisionReply,
    ) -> Result<NativeCandidate, GameError> {
        self.command(
            actor,
            GameCommand::Reply {
                decision: self.state.pending_decision().unwrap().id(),
                choice,
            },
        )
    }
    fn definition(
        &self,
        id: ObjectId,
        aliases: &BTreeMap<
            spellwake_engine::object_identity::CardId,
            spellwake_reference_tools::ordinary_game::fixture::Alias,
        >,
    ) -> String {
        let card = self
            .state
            .execution()
            .window()
            .cards()
            .get(id)
            .unwrap()
            .card_id()
            .unwrap();
        aliases[&card].definition_ref.clone()
    }
}
#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary; completes and fully replays a real native game"]
fn authenticated_four_sources_complete_a_natural_game_with_typed_entropy_and_full_result_replay() {
    let source = load_source();
    let deck = TeachingDeck {
        opt: 4,
        consider: 4,
        thought_scour: 4,
    };
    let entropy = words(236);
    // Exactly118 initial native shuffle words; the later mulligan receives a
    // distinct actual host-only append, recorded among the typed calls.
    let opening = source
        .opening([deck; 2], EntropyTape::new(entropy[..118].to_vec()))
        .unwrap();
    assert_eq!(opening.definitions().len(), 4);
    assert_eq!(opening.setup().seed.cards.len(), 120);
    assert!(opening.setup().seed.mana.is_empty());
    assert!(opening.setup().seed.initial_battlefield_order.is_empty());
    assert!(opening.setup().seed.cards.iter().all(|c| matches!(
        c.destination,
        spellwake_engine::object_identity::ObjectDestination::LibraryBottom
    )));
    let initialized = opening.initialize(GameId(88501)).unwrap();
    assert_eq!(initialized.shuffle_journal().len(), 2);
    assert_eq!(initialized.draw_journal().events.len(), 14);
    assert_eq!(initialized.state().entropy().unwrap().position(), 118);
    assert!(initialized.state().turn().is_none());
    assert!(initialized.state().opening().is_some());
    let mut run = Run {
        state: initialized.state().clone(),
        calls: vec![],
        cast: BTreeMap::new(),
        choices: 0,
        payments: 0,
        productions: 0,
        lands: 0,
        failures: 0,
    };
    assert!(run.command(A, GameCommand::Pass).is_err());
    assert!(
        run.call(Call::Entropy {
            basis: run.state.basis().clone(),
            words: vec![]
        })
        .is_err()
    );
    run.call(Call::Entropy {
        basis: run.state.basis().clone(),
        words: entropy[118..].to_vec(),
    })
    .unwrap();
    let aliases = opening.aliases();
    let mut invalid_partition_checked = false;
    let mut wrong_chooser_checked = false;
    let mut unpaid_checked = false;
    let mut order_checked = false;
    for _ in 0..8000 {
        if run.state.outcome().is_some() {
            break;
        }
        if let Some(decision) = run.state.pending_decision().cloned() {
            let choice = match decision.frame() {
                RuleFrame::Opening(OpeningDecision::Declare { taken, .. }) => {
                    DecisionReply::Mulligan(if decision.chooser() == A && *taken == 0 {
                        MulliganChoice::Mulligan
                    } else {
                        MulliganChoice::Keep
                    })
                }
                RuleFrame::Opening(OpeningDecision::Bottom { count, hand, .. }) => {
                    let mut ordered = hand.clone();
                    ordered.sort_by_key(|id| run.definition(*id, aliases) != ISLAND);
                    DecisionReply::Bottom(ordered.into_iter().take(*count).collect())
                }
                RuleFrame::LibraryTop(frame) => {
                    run.choices += 1;
                    let domain = frame.domain_top_first().to_vec();
                    let remove = run.choices.is_multiple_of(2);
                    let (top, rest) = if remove {
                        (vec![], domain.clone())
                    } else {
                        (domain.clone(), vec![])
                    };
                    let make = |top, rest| {
                        DecisionReply::LibraryTop(match frame.kind() {
                            LibraryChoiceKind::Scry => LibraryTopReply::Scry {
                                top_first: top,
                                bottom_top_first: rest,
                            },
                            LibraryChoiceKind::Surveil => LibraryTopReply::Surveil {
                                top_first: top,
                                to_graveyard: rest,
                            },
                        })
                    };
                    if !invalid_partition_checked {
                        assert!(
                            run.reply(decision.chooser(), make(domain.clone(), domain.clone()))
                                .is_err()
                        );
                        invalid_partition_checked = true;
                    }
                    if !wrong_chooser_checked {
                        assert_eq!(
                            run.reply(
                                if decision.chooser() == A { B } else { A },
                                make(top.clone(), rest.clone())
                            ),
                            Err(GameError::WrongChooser)
                        );
                        wrong_chooser_checked = true;
                    }
                    make(top, rest)
                }
                RuleFrame::ResolutionGraveyardOrder(frame) => {
                    let domain = frame.domain().to_vec();
                    if !order_checked && domain.len() > 1 {
                        assert!(
                            run.reply(
                                decision.chooser(),
                                DecisionReply::OrderResolutionGraveyard(vec![
                                    domain[0];
                                    domain.len()
                                ])
                            )
                            .is_err()
                        );
                        order_checked = true;
                    }
                    DecisionReply::OrderResolutionGraveyard(domain.into_iter().rev().collect())
                }
                RuleFrame::Checks(CheckDecision::CleanupDiscard(frame)) => {
                    let mut domain = frame.domain().to_vec();
                    domain.sort_by_key(|id| run.definition(*id, aliases) != ISLAND);
                    DecisionReply::Discard(domain.into_iter().take(frame.required()).collect())
                }
                RuleFrame::Checks(CheckDecision::GraveyardOrder(frame)) => {
                    DecisionReply::OrderGraveyard(frame.domain().to_vec())
                }
                other => panic!("unsupported actual source4 frame {other:?}"),
            };
            run.reply(decision.chooser(), choice).unwrap();
            continue;
        }
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            run.state.execution().window().phase()
        else {
            panic!("unexpected root stop {:?}", run.state.pending_turn_work())
        };
        let active = run.state.execution().window().turn_owner();
        let turn = run.state.turn().unwrap();
        let main = matches!(
            turn.position().step,
            Step::PrecombatMain | Step::PostcombatMain
        );
        let empty = run.state.execution().window().entries().is_empty();
        if holder == active && main && empty {
            let cards = run.state.execution().window().cards();
            let hand = cards
                .iter()
                .filter(|c| c.zone() == ObjectZone::Hand(holder))
                .map(|c| c.id())
                .collect::<Vec<_>>();
            if turn.lands_played() == &0.into()
                && let Some(land) = hand
                    .iter()
                    .find(|id| run.definition(**id, aliases) == ISLAND)
                    .copied()
            {
                run.command(holder, GameCommand::PlayLand { land }).unwrap();
                continue;
            }
            if let Some(spell) = hand
                .iter()
                .find(|id| run.definition(**id, aliases) != ISLAND)
                .copied()
            {
                let oracle = run.definition(spell, aliases);
                let cast = |payment| {
                    if oracle == SCOUR {
                        GameCommand::CastPaid {
                            spell,
                            target: TargetRef::Player(if holder == A { B } else { A }),
                            payment,
                        }
                    } else {
                        GameCommand::CastUntargeted {
                            spell,
                            payment: Some(payment),
                        }
                    }
                };
                if !unpaid_checked {
                    assert!(run.command(holder, cast(ManaPayment::default())).is_err());
                    unpaid_checked = true;
                }
                if run.state.execution().mana(holder, ManaType::Blue) == 0.into() {
                    let island = run
                        .state
                        .execution()
                        .window()
                        .cards()
                        .iter()
                        .find(|c| {
                            c.zone() == ObjectZone::Battlefield
                                && c.controller() == Some(holder)
                                && c.tapped() == Some(false)
                        })
                        .map(|c| c.id());
                    if let Some(island) = island {
                        run.command(
                            holder,
                            GameCommand::ActivateMana {
                                source: island,
                                ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
                            },
                        )
                        .unwrap();
                        continue;
                    }
                } else {
                    run.command(
                        holder,
                        cast(ManaPayment {
                            spend: vec![ManaSpend {
                                mana_type: ManaType::Blue,
                                amount: 1.into(),
                            }],
                        }),
                    )
                    .unwrap();
                    *run.cast.entry(oracle).or_default() += 1;
                    continue;
                }
            }
        }
        run.command(holder, GameCommand::Pass).unwrap();
    }
    assert!(matches!(run.state.outcome(), Some(GameOutcome::Winner(_))));
    assert_eq!(
        run.cast.keys().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "4c9bcba6-87b5-4fb3-97ee-6fe5b739337d".to_string(),
            "713332c1-5bd8-400f-bfff-c1ca0697a043".to_string(),
            SCOUR.to_string()
        ])
    );
    assert!(run.choices >= 2 && run.lands >= 2 && run.productions > 0);
    assert_eq!(run.payments, run.cast.values().sum::<usize>());
    assert_eq!(run.payments, run.productions);
    assert!(invalid_partition_checked && wrong_chooser_checked && unpaid_checked && order_checked);
    assert!(
        run.calls
            .iter()
            .filter_map(|(_, r)| r.as_ref().ok())
            .flat_map(|c| &c.draw_journal().events)
            .any(|event| matches!(event, DrawEvent::EmptyLibrary { .. }))
    );
    assert_eq!(run.state.execution().window().cards().iter().count(), 120);
    assert!(
        run.command(A, GameCommand::Pass).is_err(),
        "terminal game rejects further play"
    );
    // Reopen genuine bytes and rebuild the entire initial native source and input,
    // rather than starting replay at the post-opening state.
    let reopened = load_source();
    let replay_opening = reopened
        .opening([deck; 2], EntropyTape::new(entropy[..118].to_vec()))
        .unwrap();
    assert_eq!(opening, replay_opening);
    assert_eq!(replay_opening.initialize(GameId(88501)), Ok(initialized));
    let mut replay = replay_opening
        .initialize(GameId(88501))
        .unwrap()
        .into_state();
    for (input, expected) in &run.calls {
        let before = replay.clone();
        let actual = input.apply(&replay);
        assert_eq!(
            &actual, expected,
            "full original native Result differs for {input:?}"
        );
        assert_eq!(replay, before);
        if let Ok(next) = actual {
            replay = next.into_state();
        }
    }
    assert_eq!(run.state, replay);
    println!(
        "source4 natural game: calls={}, rejected={}, casts={:?}, payments={}, productions={}, lands={}, choices={}, outcome={:?}, entropy_position={}",
        run.calls.len(),
        run.failures,
        run.cast,
        run.payments,
        run.productions,
        run.lands,
        run.choices,
        run.state.outcome(),
        run.state.entropy().unwrap().position()
    );
}
