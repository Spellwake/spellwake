//! Synthetic native mechanisms through real GameState inputs, not card qualification.
use super::*;
use crate::{
    game::{
        GameCommand, GameContext, GameError, GameId, GameInput, GameState, NativeCandidate,
        RuleFrame,
    },
    occurrence::{RulesEvent, RulesOccurrence, RulesRecord},
    program::{CreatureTypeSlot, ResolvingPlayer},
    test_support::{creature_types, packet::AliasContext, projection},
    token::{TokenCreation, TokenDefinition, TokenName},
};
use spellwake_contracts::names::EnglishCardName;

const A: SeatId = SeatId(9);
const B: SeatId = SeatId(2);
const C: SeatId = SeatId(41);

mod applicability;
mod copied_body_mechanisms;
mod zone_replacement_mechanisms;

fn creature() -> Program {
    Program::creature(None, 2.into(), 2.into(), BTreeSet::new())
}
fn seed(id: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program,
    }
}
fn gain(amount: i64) -> Instruction {
    crate::program::Instruction::AdjustLife(Box::new(crate::toolbox::AdjustLifeSpec::gain(
        crate::toolbox::PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
        crate::program::ValueExpr::Literal(amount.into()),
    )))
}
fn sacrifice(players: AffectedPlayers, amount: RulesInteger) -> Instruction {
    Instruction::Sacrifice {
        players,
        amount: crate::program::ValueExpr::Literal(amount),
        filter: PermanentTypeFilter {
            any_types: Default::default(),
            required_types: BTreeSet::from([CardType::Creature]),
            forbidden_types: BTreeSet::new(),
            object_filter: None,
        },
    }
}
fn fixture(
    target: BodyTarget,
    body: Vec<Instruction>,
    participants: Vec<SeatId>,
    active: SeatId,
    cards: Vec<SeedCard>,
) -> GameState {
    fixture_life(target, body, participants, active, cards, 20)
}
fn fixture_life(
    target: BodyTarget,
    body: Vec<Instruction>,
    participants: Vec<SeatId>,
    active: SeatId,
    mut cards: Vec<SeedCard>,
    life: i64,
) -> GameState {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "selection-native-unit".into(),
        vocabulary.rules_revision().into(),
        "synthetic-no-catalogue".into(),
        "selection-core-unit".into(),
        "native-only".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    cards.insert(
        0,
        seed(
            1,
            active,
            ObjectDestination::Hand,
            Program::instant(ManaCost::free(), ExecutableBody::new(target, body)),
        ),
    );
    let battlefield = cards
        .iter()
        .filter(|card| matches!(card.destination, ObjectDestination::Battlefield { .. }))
        .map(|card| card.card)
        .collect();
    GameState::initialize(
        GameId(8631),
        context,
        GameSeed::with_uniform_life(
            participants,
            active,
            cards,
            battlefield,
            vec![],
            life.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            names,
        ),
    )
    .unwrap()
    .into_state()
}
fn id(state: &GameState, card: u64) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn advance(state: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    let before = state.clone();
    let input = GameInput {
        basis: state.basis().clone(),
        actor,
        command,
    };
    let first = state.advance(input.clone()).unwrap();
    assert_eq!(state, &before);
    assert_eq!(
        state.advance(input).unwrap(),
        first,
        "exact saved-input native replay"
    );
    first
}
fn cast(state: &GameState, actor: SeatId, card: u64, target: Option<TargetRef>) -> GameState {
    let command = match target {
        Some(target) => GameCommand::Cast {
            spell: id(state, card),
            target,
        },
        None => GameCommand::CastUntargeted {
            spell: id(state, card),
            payment: None,
        },
    };
    advance(state, actor, command).into_state()
}
fn pass_to_boundary(mut state: GameState) -> GameState {
    for _ in 0..state.execution().window().participants().len() {
        if state.pending_decision().is_some() {
            return state;
        }
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        else {
            panic!("actual ready priority required")
        };
        state = advance(&state, holder, GameCommand::Pass).into_state();
    }
    state
}
fn reply(state: &GameState, choice: DecisionReply) -> NativeCandidate {
    let decision = state.pending_decision().unwrap();
    advance(
        state,
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice,
        },
    )
}
fn select_frame(state: &GameState) -> &ResolutionSelectionFrame {
    let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() else {
        panic!("selection expected")
    };
    frame
}
fn order_frame(state: &GameState) -> &ResolutionGraveyardOrderFrame {
    let RuleFrame::ResolutionGraveyardOrder(frame) = state.pending_decision().unwrap().frame()
    else {
        panic!("owner ordering expected")
    };
    frame
}
fn events(candidate: &NativeCandidate) -> Vec<&RulesEvent> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| {
            if let RulesRecord::Event(event) = record {
                Some(event)
            } else {
                None
            }
        })
        .collect()
}
fn physical_pile(state: &GameState, player: SeatId) -> Vec<CardId> {
    let cards = state.execution().window().cards();
    cards
        .graveyard(player)
        .iter()
        .map(|object| cards.get(*object).unwrap().card_id().unwrap())
        .collect()
}
fn assert_rejected(state: &GameState, actor: SeatId, command: GameCommand, expected: GameError) {
    let saved = state.clone();
    assert_eq!(
        state.advance(GameInput {
            basis: state.basis().clone(),
            actor,
            command
        }),
        Err(expected)
    );
    assert_eq!(*state, saved);
}

#[test]
fn all_players_choose_in_rotated_participant_order_before_one_simultaneous_publication() {
    let cards = vec![
        seed(
            10,
            A,
            ObjectDestination::Battlefield { controller: A },
            creature(),
        ),
        seed(
            11,
            B,
            ObjectDestination::Battlefield { controller: B },
            creature(),
        ),
        seed(
            12,
            C,
            ObjectDestination::Battlefield { controller: C },
            creature(),
        ),
    ];
    let state = fixture(
        BodyTarget::None,
        vec![sacrifice(AffectedPlayers::EachPlayer, 1.into())],
        vec![A, B, C],
        B,
        cards,
    );
    let mut state = pass_to_boundary(cast(&state, B, 1, None));
    let starting_ids = [id(&state, 10), id(&state, 11), id(&state, 12)];
    for (index, (player, card)) in [(B, 11), (C, 12), (A, 10)].into_iter().enumerate() {
        assert_eq!(select_frame(&state).chooser(), player);
        let view = state.execution().resolution_selection_state().unwrap();
        let ResolutionSelectionStage::Choosing {
            requirements,
            next_player,
            accepted,
        } = view.stage
        else {
            panic!("choosing")
        };
        assert_eq!(*next_player, index);
        assert_eq!(accepted.len(), index);
        assert_eq!(
            requirements
                .iter()
                .map(|row| row.player)
                .collect::<Vec<_>>(),
            vec![B, C, A]
        );
        for object in starting_ids {
            assert_eq!(
                state
                    .execution()
                    .window()
                    .cards()
                    .get(object)
                    .unwrap()
                    .zone(),
                ObjectZone::Battlefield
            );
        }
        let input = GameInput {
            basis: state.basis().clone(),
            actor: player,
            command: GameCommand::Reply {
                decision: state.pending_decision().unwrap().id(),
                choice: DecisionReply::SelectResolution(vec![id(&state, card)]),
            },
        };
        let result = state.advance(input.clone()).unwrap();
        assert_eq!(state.advance(input).unwrap(), result);
        if index < 2 {
            assert!(result.rules_journal().records.is_empty());
        } else {
            let selected: Vec<_> = events(&result).into_iter().filter(|event| event.occurrences.iter().any(|row|
                matches!(row.value, RulesOccurrence::ZoneChange(ref change) if matches!(change.reason, ZoneReason::Sacrifice { .. })))).collect();
            assert_eq!(selected.len(), 1);
            assert_eq!(selected[0].occurrences.len(), 3);
        }
        state = result.into_state();
    }
    assert!(state.pending_decision().is_none());
    for (player, card) in [(A, 10), (B, 11), (C, 12)] {
        assert!(physical_pile(&state, player).contains(&CardId(card)));
    }
}

#[test]
fn one_owner_orders_cards_selected_by_different_controllers_above_the_old_pile() {
    let cards = vec![
        seed(
            10,
            B,
            ObjectDestination::Battlefield { controller: A },
            creature(),
        ),
        seed(
            11,
            B,
            ObjectDestination::Battlefield { controller: B },
            creature(),
        ),
        seed(12, B, ObjectDestination::Graveyard, creature()),
    ];
    let state = fixture(
        BodyTarget::None,
        vec![sacrifice(AffectedPlayers::EachPlayer, 1.into())],
        vec![A, B],
        A,
        cards,
    );
    let mut state = pass_to_boundary(cast(&state, A, 1, None));
    let first = id(&state, 10);
    let second = id(&state, 11);
    state = reply(&state, DecisionReply::SelectResolution(vec![first])).into_state();
    state = reply(&state, DecisionReply::SelectResolution(vec![second])).into_state();
    assert_eq!(order_frame(&state).chooser(), B);
    assert_eq!(
        order_frame(&state)
            .domain()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([first, second])
    );
    assert_eq!(physical_pile(&state, B), vec![CardId(12)]);
    let decision = state.pending_decision().unwrap().id();
    assert_rejected(
        &state,
        B,
        GameCommand::Reply {
            decision,
            choice: DecisionReply::OrderResolutionGraveyard(vec![first, first]),
        },
        GameError::Effect(EffectError::InvalidGraveyardPermutation),
    );
    let result = reply(
        &state,
        DecisionReply::OrderResolutionGraveyard(vec![second, first]),
    );
    let event = events(&result).into_iter().find(|event| event.occurrences.iter().any(|row| matches!(row.value, RulesOccurrence::ZoneChange(ref change) if matches!(change.reason, ZoneReason::Sacrifice { .. })))).unwrap();
    let selected_players: Vec<_> = event
        .occurrences
        .iter()
        .filter_map(|row| {
            if let RulesOccurrence::ZoneChange(change) = &row.value {
                if let ZoneReason::Sacrifice { player } = change.reason {
                    Some((change.before.object.id(), player))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();
    assert_eq!(selected_players, vec![(second, B), (first, A)]);
    state = result.into_state();
    assert_eq!(
        physical_pile(&state, B),
        vec![CardId(11), CardId(10), CardId(12)]
    );
}

#[test]
fn huge_discard_clamps_to_real_hand_and_orders_without_creating_a_failed_draw() {
    let huge =
        RulesInteger::try_from("999999999999999999999999999999999999999999".to_owned()).unwrap();
    let cards = vec![
        seed(10, B, ObjectDestination::Hand, creature()),
        seed(11, B, ObjectDestination::Hand, creature()),
    ];
    let state = fixture(
        BodyTarget::SinglePlayer,
        vec![Instruction::Discard {
            players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
            amount: crate::program::ValueExpr::Literal(huge),
        }],
        vec![A, B],
        A,
        cards,
    );
    let mut state = pass_to_boundary(cast(&state, A, 1, Some(TargetRef::Player(B))));
    let first = id(&state, 10);
    let second = id(&state, 11);
    assert_eq!(select_frame(&state).required(), 2);
    assert_rejected(
        &state,
        B,
        GameCommand::Reply {
            decision: state.pending_decision().unwrap().id(),
            choice: DecisionReply::SelectResolution(vec![first, first]),
        },
        GameError::Effect(EffectError::DuplicateSelection),
    );
    state = reply(&state, DecisionReply::SelectResolution(vec![first, second])).into_state();
    assert_eq!(order_frame(&state).chooser(), B);
    let result = reply(
        &state,
        DecisionReply::OrderResolutionGraveyard(vec![second, first]),
    );
    assert!(result.draw_journal().events.is_empty());
    assert!(result.state().execution().failed_draws.is_empty());
    assert_eq!(
        physical_pile(result.state(), B),
        vec![CardId(11), CardId(10)]
    );
    let discarded = events(&result).iter().flat_map(|event| &event.occurrences).filter(|row| matches!(row.value,
        RulesOccurrence::ZoneChange(ref change) if change.reason == ZoneReason::Discard { player: B })).count();
    assert_eq!(discarded, 2);
}

#[test]
fn rejected_later_selection_preserves_accepted_prefix_and_root_command_gates() {
    let cards = vec![
        seed(
            10,
            A,
            ObjectDestination::Battlefield { controller: A },
            creature(),
        ),
        seed(
            11,
            B,
            ObjectDestination::Battlefield { controller: B },
            creature(),
        ),
        seed(
            12,
            B,
            ObjectDestination::Battlefield { controller: B },
            creature(),
        ),
    ];
    let state = fixture(
        BodyTarget::None,
        vec![sacrifice(AffectedPlayers::EachPlayer, 1.into())],
        vec![A, B],
        A,
        cards,
    );
    let mut state = pass_to_boundary(cast(&state, A, 1, None));
    let old_basis = state.basis().clone();
    let old_decision = state.pending_decision().unwrap().id();
    let old_frame = select_frame(&state).clone();
    state = reply(
        &state,
        DecisionReply::SelectResolution(vec![id(&state, 10)]),
    )
    .into_state();
    let decision = state.pending_decision().unwrap().id();
    let live = id(&state, 11);
    assert_rejected(
        &state,
        B,
        GameCommand::Reply {
            decision,
            choice: DecisionReply::SelectResolution(vec![live, live]),
        },
        GameError::Effect(EffectError::InvalidSelectionCount),
    );
    assert_rejected(
        &state,
        B,
        GameCommand::Reply {
            decision,
            choice: DecisionReply::SelectResolution(vec![id(&state, 1)]),
        },
        GameError::Effect(EffectError::SelectionOutsideDomain),
    );
    assert_rejected(
        &state,
        A,
        GameCommand::Reply {
            decision,
            choice: DecisionReply::SelectResolution(vec![live]),
        },
        GameError::WrongChooser,
    );
    assert_rejected(
        &state,
        B,
        GameCommand::Reply {
            decision: old_decision,
            choice: DecisionReply::SelectResolution(vec![live]),
        },
        GameError::WrongDecision,
    );
    assert_rejected(
        &state,
        B,
        GameCommand::Reply {
            decision,
            choice: DecisionReply::OrderResolutionGraveyard(vec![live]),
        },
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    for command in [
        GameCommand::Pass,
        GameCommand::CastUntargeted {
            spell: live,
            payment: None,
        },
        GameCommand::PlayLand { land: live },
        GameCommand::ActivateMana {
            source: live,
            ability: crate::mana::ManaAbilityRef::Printed(crate::mana::ManaAbilityId(1)),
        },
    ] {
        assert_rejected(&state, B, command, GameError::DecisionRequired);
    }
    let before = state.clone();
    assert_eq!(
        state.advance(GameInput {
            basis: old_basis,
            actor: B,
            command: GameCommand::Reply {
                decision,
                choice: DecisionReply::SelectResolution(vec![live])
            }
        }),
        Err(GameError::StaleRevision)
    );
    assert_eq!(state, before);
    assert_eq!(
        state.execution().resume_rule(
            state.execution().revision(),
            &EffectDecision::ResolutionSelect(old_frame),
            A,
            DecisionReply::SelectResolution(vec![id(&state, 10)]),
            None
        ),
        Err(EffectError::WrongSelectionFrame)
    );
    let ResolutionSelectionStage::Choosing {
        accepted,
        next_player,
        ..
    } = state
        .execution()
        .resolution_selection_state()
        .unwrap()
        .stage
    else {
        panic!("choosing")
    };
    assert_eq!(*next_player, 1);
    assert_eq!(accepted[0].objects, vec![id(&state, 10)]);
    let projection = projection::state(&state, &AliasContext::default()).unwrap();
    assert_eq!(
        projection["pending_work"]["work"]["stage"]["accepted"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    state = reply(&state, DecisionReply::SelectResolution(vec![live])).into_state();
    assert!(state.pending_decision().is_none());
}

fn token_prefix() -> Program {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let definition = TokenDefinition::new(
        creature(),
        TokenName::Specified(EnglishCardName::new("Selection Probe".into()).unwrap()),
        &vocabulary,
    )
    .unwrap();
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens {
                creations: vec![TokenCreation::one(definition)],
            }],
        ),
    )
}

#[test]
fn same_body_bounce_excludes_hand_tokens_at_domain_and_commit_then_later_checks_cease_them() {
    for mixed in [false, true] {
        let mut cards = vec![seed(2, A, ObjectDestination::Hand, token_prefix())];
        if mixed {
            cards.push(seed(10, A, ObjectDestination::Hand, creature()));
        }
        let state = fixture(
            BodyTarget::SinglePermanent,
            vec![
                crate::program::Instruction::ZoneOp(Box::new(
                    crate::toolbox::ZoneOpSpec::move_all(
                        crate::toolbox::ObjectQuery::Targets,
                        crate::toolbox::ZoneDest::Hand,
                    ),
                )),
                Instruction::Discard {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    amount: crate::program::ValueExpr::Literal(1.into()),
                },
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
                gain(1),
            ],
            vec![A, B],
            A,
            cards,
        );
        let mut state = pass_to_boundary(cast(&state, A, 2, None));
        let token = state
            .execution()
            .window()
            .cards()
            .iter()
            .find(|object| object.is_token())
            .unwrap()
            .id();
        state = pass_to_boundary(cast(&state, A, 1, Some(TargetRef::Object(token))));
        let hand_token = state
            .execution()
            .window()
            .cards()
            .iter()
            .find(|object| object.is_token())
            .unwrap()
            .id();
        assert_ne!(hand_token, token);
        assert_eq!(
            state
                .execution()
                .window()
                .cards()
                .get(hand_token)
                .unwrap()
                .zone(),
            ObjectZone::Hand(A)
        );
        if mixed {
            let frame = select_frame(&state);
            assert_eq!(frame.required(), 1);
            assert_eq!(frame.domain(), &[id(&state, 10)]);
            let result = reply(
                &state,
                DecisionReply::SelectResolution(vec![id(&state, 10)]),
            );
            let discarded: Vec<_> = events(&result)
                .iter()
                .flat_map(|event| &event.occurrences)
                .filter_map(|row| {
                    if let RulesOccurrence::ZoneChange(change) = &row.value {
                        (change.reason == ZoneReason::Discard { player: A })
                            .then_some(change.before.object.id())
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(discarded, vec![id(&state, 10)]);
            state = result.into_state();
        }
        assert!(matches!(
            state.pending_decision().unwrap().frame(),
            RuleFrame::OptionalEffect(_)
        ));
        assert_eq!(
            state
                .execution()
                .window()
                .cards()
                .get(hand_token)
                .unwrap()
                .zone(),
            ObjectZone::Hand(A)
        );
        let window = state.execution().window();
        let before = window.clone();
        assert_eq!(
            window.record_resolution_selection_moves(
                window.revision(),
                state.execution().resolution_state().unwrap().entry(),
                vec![ResolutionSelectionMove::FromHand {
                    destination: crate::program::PublicZoneDestination::Graveyard,
                    player: A,
                    object: hand_token
                }]
            ),
            Err(StackWindowError::InvalidResolutionMove)
        );
        assert_eq!(*window, before);
        let result = reply(
            &state,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        );
        assert_eq!(result.state().execution().life_total(A), Some(&21.into()));
        assert!(
            result
                .state()
                .execution()
                .window()
                .cards()
                .get(hand_token)
                .is_none()
        );
        assert_eq!(events(&result).iter().flat_map(|event| &event.occurrences).filter(|row|
            matches!(row.value, RulesOccurrence::TokenCeased { ref object } if object.id() == hand_token)).count(), 1);
    }
}

#[test]
fn counted_amount_is_frozen_once_under_resolving_controller_for_every_affected_player() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let elf = vocabulary.resolve_label("Elf").unwrap();
    let mut elf_creature = creature();
    elf_creature.creature_subtypes.insert(elf);
    let cards = vec![
        seed(
            10,
            A,
            ObjectDestination::Battlefield { controller: A },
            elf_creature.clone(),
        ),
        seed(
            11,
            A,
            ObjectDestination::Battlefield { controller: A },
            elf_creature.clone(),
        ),
        seed(
            12,
            B,
            ObjectDestination::Battlefield { controller: B },
            elf_creature,
        ),
        seed(
            13,
            B,
            ObjectDestination::Battlefield { controller: B },
            creature(),
        ),
    ];
    let state = fixture(
        BodyTarget::None,
        vec![
            Instruction::ChooseCreatureType {
                into: CreatureTypeSlot::new(0),
                excluding: BTreeSet::new(),
            },
            Instruction::Sacrifice {
                players: AffectedPlayers::EachPlayer,
                amount: crate::program::ValueExpr::count_battlefield(
                    crate::program::ObjectFilter::any()
                        .controlled()
                        .of_chosen_slot(CreatureTypeSlot::new(0)),
                ),
                filter: PermanentTypeFilter {
                    any_types: Default::default(),
                    required_types: BTreeSet::from([CardType::Creature]),
                    forbidden_types: BTreeSet::new(),
                    object_filter: None,
                },
            },
        ],
        vec![A, B],
        A,
        cards,
    );
    let state = pass_to_boundary(cast(&state, A, 1, None));
    let mut state = reply(&state, DecisionReply::CreatureType(elf)).into_state();
    assert_eq!(select_frame(&state).required(), 2);
    state = reply(
        &state,
        DecisionReply::SelectResolution(vec![id(&state, 10), id(&state, 11)]),
    )
    .into_state();
    assert_eq!(select_frame(&state).chooser(), B);
    assert_eq!(
        select_frame(&state).required(),
        2,
        "must not reevaluate B's one Elf"
    );
    state = reply(
        &state,
        DecisionReply::SelectResolution(vec![id(&state, 12), id(&state, 13)]),
    )
    .into_state();
    state = reply(
        &state,
        DecisionReply::OrderResolutionGraveyard(vec![id(&state, 11), id(&state, 10)]),
    )
    .into_state();
    let ResolutionSelectionStage::Ordering {
        orders, next_owner, ..
    } = state
        .execution()
        .resolution_selection_state()
        .unwrap()
        .stage
    else {
        panic!("ordering")
    };
    assert_eq!(*next_owner, 1);
    assert_eq!(orders[0].owner, A);
    assert_eq!(
        orders[0].top_to_bottom,
        vec![id(&state, 11), id(&state, 10)]
    );
    assert_rejected(
        &state,
        B,
        GameCommand::Reply {
            decision: state.pending_decision().unwrap().id(),
            choice: DecisionReply::OrderResolutionGraveyard(vec![id(&state, 12), id(&state, 12)]),
        },
        GameError::Effect(EffectError::InvalidGraveyardPermutation),
    );
    state = reply(
        &state,
        DecisionReply::OrderResolutionGraveyard(vec![id(&state, 13), id(&state, 12)]),
    )
    .into_state();
    assert!(state.pending_decision().is_none());
}

#[test]
fn zero_amount_leaves_real_hand_cards_and_emits_no_discard() {
    let state = fixture(
        BodyTarget::None,
        vec![
            Instruction::Discard {
                players: AffectedPlayers::EachPlayer,
                amount: crate::program::ValueExpr::Literal(0.into()),
            },
            gain(1),
        ],
        vec![A, B],
        A,
        vec![seed(10, B, ObjectDestination::Hand, creature())],
    );
    let state = cast(&state, A, 1, None);
    let state = advance(&state, A, GameCommand::Pass).into_state();
    let result = advance(&state, B, GameCommand::Pass);
    assert!(result.state().pending_decision().is_none());
    assert_eq!(result.state().execution().life_total(A), Some(&21.into()));
    assert_eq!(
        result
            .state()
            .execution()
            .window()
            .cards()
            .get(id(result.state(), 10))
            .unwrap()
            .zone(),
        ObjectZone::Hand(B)
    );
    assert!(!events(&result).iter().flat_map(|event| &event.occurrences).any(|row|
        matches!(row.value, RulesOccurrence::ZoneChange(ref change) if matches!(change.reason, ZoneReason::Discard { .. }))));
}

#[test]
fn a_battlefield_token_is_a_real_sacrifice_without_owner_order_or_physical_pile_entry() {
    let state = fixture(
        BodyTarget::None,
        vec![
            sacrifice(AffectedPlayers::One(ResolvingPlayer::Controller), 1.into()),
            Instruction::OptionalSequence {
                chooser: ResolvingPlayer::Controller,
                instructions: 1,
            },
            gain(1),
        ],
        vec![A, B],
        A,
        vec![seed(2, A, ObjectDestination::Hand, token_prefix())],
    );
    let mut state = pass_to_boundary(cast(&state, A, 2, None));
    let token = state
        .execution()
        .window()
        .cards()
        .iter()
        .find(|object| object.is_token())
        .unwrap()
        .id();
    state = pass_to_boundary(cast(&state, A, 1, None));
    assert_eq!(select_frame(&state).domain(), &[token]);
    let result = reply(&state, DecisionReply::SelectResolution(vec![token]));
    assert!(matches!(
        result.state().pending_decision().unwrap().frame(),
        RuleFrame::OptionalEffect(_)
    ));
    assert_eq!(physical_pile(result.state(), A), vec![CardId(2)]);
    let values: Vec<_> = events(&result)
        .iter()
        .flat_map(|event| &event.occurrences)
        .collect();
    assert!(
        !values
            .iter()
            .any(|row| matches!(row.value, RulesOccurrence::TokenCeased { .. }))
    );
    let moved = values
        .iter()
        .find_map(|row| {
            if let RulesOccurrence::ZoneChange(change) = &row.value {
                (change.reason == ZoneReason::Sacrifice { player: A }).then_some(change)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(moved.before.object.id(), token);
    assert!(moved.before.object.card_id().is_none());
    assert_eq!(moved.after.object.zone(), ObjectZone::Graveyard(A));
    let departed = moved.after.object.id();
    state = result.into_state();
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(departed)
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    let result = reply(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert!(
        result
            .state()
            .execution()
            .window()
            .cards()
            .get(departed)
            .is_none()
    );
    assert_eq!(events(&result).iter().flat_map(|event| &event.occurrences).filter(|row|
        matches!(row.value, RulesOccurrence::TokenCeased { ref object } if object.id() == departed)).count(), 1);
}

#[test]
fn current_evaluated_type_filter_is_independent_of_printed_types_and_zone_control() {
    use crate::characteristics::{
        ResolvedPartSpec, ResolvedPartsSpec, SubtypeOperation, TypeChangeSpec, TypeOperation,
    };
    let mut artifact_creature = creature();
    artifact_creature.card_types.insert(CardType::Artifact);
    let prefix = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![{
                struct _Tmp {
                    parts: crate::characteristics::ResolvedPartsSpec,
                    duration: crate::characteristics::ResolvedDuration,
                }
                let _t = _Tmp {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Remove(BTreeSet::from([CardType::Artifact])),
                        subtypes: SubtypeOperation::Keep,
                    })]),
                    duration: crate::characteristics::ResolvedDuration::UntilEndOfTurn,
                };
                crate::program::Instruction::ApplyContinuous(Box::new(
                    crate::toolbox::ApplyContinuousSpec {
                        selector: Some(crate::toolbox::ObjectQuery::Targets),
                        payload: crate::toolbox::ContinuousPayload::ResolvedParts(Box::new(
                            _t.parts,
                        )),
                        duration: _t.duration,
                        condition: None,
                        one_shot: false,
                        as_emblem: None,
                        emblem_label: None,
                    },
                ))
            }],
        ),
    );
    let cards = vec![
        seed(2, A, ObjectDestination::Hand, prefix),
        seed(
            10,
            A,
            ObjectDestination::Battlefield { controller: A },
            artifact_creature.clone(),
        ),
        seed(
            11,
            A,
            ObjectDestination::Battlefield { controller: A },
            artifact_creature,
        ),
        seed(
            12,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::artifact(None, vec![]),
        ),
    ];
    let state = fixture(
        BodyTarget::None,
        vec![Instruction::Sacrifice {
            players: AffectedPlayers::EachPlayer,
            amount: crate::program::ValueExpr::Literal(1.into()),
            filter: PermanentTypeFilter {
                any_types: Default::default(),
                required_types: BTreeSet::from([CardType::Creature]),
                forbidden_types: BTreeSet::from([CardType::Artifact]),
                object_filter: None,
            },
        }],
        vec![A, B],
        A,
        cards,
    );
    let state = pass_to_boundary(cast(&state, A, 2, Some(TargetRef::Object(id(&state, 10)))));
    assert!(
        state
            .execution()
            .programs
            .get(&CardId(10))
            .unwrap()
            .card_types
            .contains(&CardType::Artifact)
    );
    assert!(
        !state
            .execution()
            .evaluated_characteristics()
            .unwrap()
            .get(id(&state, 10))
            .unwrap()
            .current_types
            .contains(&CardType::Artifact)
    );
    let state = pass_to_boundary(cast(&state, A, 1, None));
    assert_eq!(select_frame(&state).domain(), &[id(&state, 10)]);
    let decision = state.pending_decision().unwrap().id();
    assert_rejected(
        &state,
        A,
        GameCommand::Reply {
            decision,
            choice: DecisionReply::SelectResolution(vec![id(&state, 11)]),
        },
        GameError::Effect(EffectError::SelectionOutsideDomain),
    );
    let result = reply(
        &state,
        DecisionReply::SelectResolution(vec![id(&state, 10)]),
    );
    assert_eq!(
        physical_pile(result.state(), A),
        vec![CardId(1), CardId(10), CardId(2)]
    );
}

#[test]
fn resolving_wrapper_and_final_commit_reject_a_complete_invalid_batch_atomically() {
    let state = fixture(
        BodyTarget::None,
        vec![sacrifice(
            AffectedPlayers::One(ResolvingPlayer::Controller),
            1.into(),
        )],
        vec![A, B],
        A,
        vec![
            seed(
                10,
                A,
                ObjectDestination::Battlefield { controller: A },
                creature(),
            ),
            seed(11, B, ObjectDestination::Hand, creature()),
        ],
    );
    let state = pass_to_boundary(cast(&state, A, 1, None));
    let selected = id(&state, 10);
    let window = state.execution().window();
    let before = window.clone();
    let entry = select_frame(&state).entry();
    assert_eq!(
        window.record_resolution_selection_moves(
            window.revision(),
            entry,
            vec![
                ResolutionSelectionMove::Sacrifice {
                    destination: crate::program::PublicZoneDestination::Graveyard,
                    player: A,
                    object: selected
                },
                ResolutionSelectionMove::FromHand {
                    destination: crate::program::PublicZoneDestination::Graveyard,
                    player: A,
                    object: id(&state, 11)
                },
            ]
        ),
        Err(StackWindowError::InvalidResolutionMove)
    );
    assert_eq!(*window, before);
    assert_eq!(
        window.record_resolution_selection_moves(window.revision(), entry, vec![]),
        Err(StackWindowError::EmptyResolutionMoves)
    );
    assert_eq!(*window, before);
    // Privileged mechanism fault: change the physical window outside GameState
    // to prove final revalidation is independent of a frozen domain's old IDs.
    let mut changed = state.execution().clone();
    changed.window = changed
        .window
        .record_resolution_move(changed.window.revision(), selected, ObjectDestination::Hand)
        .unwrap();
    let before = changed.clone();
    assert_eq!(
        changed.resume_rule(
            changed.revision(),
            &EffectDecision::ResolutionSelect(select_frame(&state).clone()),
            A,
            DecisionReply::SelectResolution(vec![selected]),
            None
        ),
        Err(EffectError::InconsistentSelectionWork)
    );
    assert_eq!(changed, before);
}
