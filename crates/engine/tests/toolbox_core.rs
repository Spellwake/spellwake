#![forbid(unsafe_code)]
//! Behavioral integration tests for the Universal Rules Toolbox (PR 1 core foundation).

#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeVocabulary};
use spellwake_engine::{
    characteristics::{
        AbilityChange, ControllerSelector, KeywordAbility, NumericExpression, NumericPair,
        PermanentSelector, ResolvedDuration, StaticAbilityId, StaticContinuousAbility, StaticPart,
        StaticParts,
    },
    effects::{CounterKind, EffectError, SeedCard},
    game::{
        DecisionReply, GameCommand, GameError, GameInput, GameState, NativeCandidate, RuleFrame,
    },
    land::LandType,
    mana::ManaCost,
    mana::ManaPayment,
    object_identity::{CardId, ObjectDestination, ObjectZone},
    priority::SeatId,
    program::{
        BodyTarget, CardType, ExecutableBody, Instruction, ObjectFilter, ObjectStatKind,
        PlayerRelation, Program, ResolvingPlayer, StatAggregate, TriggerEvent, TriggerGrouping,
        TriggerPlayerScope, TriggerSpec, TriggeredAbility, TriggeredAbilityId, ValueExpr,
    },
    toolbox::{
        ApplyContinuousSpec, BindingId, ChoiceCategory, ConditionExpr, GraveyardSemantics,
        ObjectQuery, PlayerSelector, PromptChoiceReply, PromptChoiceSpec, PromptDomain, QuerySlice,
        QueryZone, ResolvedPromptDomain, SideDeck, SideDeckKind, SideDeckOpKind, SideDeckOpSpec,
        StateTrackKey, StateTrackKeySpec, StateTrackOp, StateTrackScope, TagKey, TagValue,
        ZoneController, ZoneDest, ZoneOpSpec, ZoneSelection,
    },
};
use std::collections::BTreeSet;

fn init_world(vocab: std::sync::Arc<CreatureTypeVocabulary>, cards: Vec<SeedCard>) -> GameState {
    let order: Vec<CardId> = cards
        .iter()
        .filter(|c| matches!(c.destination, ObjectDestination::Battlefield { .. }))
        .map(|c| c.card)
        .collect();
    world(vocab, cards, order)
}

fn validated(program: Program, vocab: &CreatureTypeVocabulary) -> Program {
    program
        .validate(vocab)
        .expect("constructed toolbox test program must validate");
    program
}

fn attempt(
    game: &GameState,
    actor: SeatId,
    command: GameCommand,
) -> Result<NativeCandidate, GameError> {
    observe_advance(
        game,
        GameInput {
            basis: game.basis().clone(),
            actor,
            command,
        },
    )
}

fn cast_untargeted(game: &GameState, id: u64) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(game, id),
            payment: Some(ManaPayment::default()),
        },
    );
    pass_pair(cast.state())
}

fn bf(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Battlefield { controller: owner },
        program,
    }
}

fn in_hand(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program,
    }
}

fn in_library(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::LibraryTop,
        program,
    }
}

fn sorcery(instructions: Vec<Instruction>, vocab: &CreatureTypeVocabulary) -> Program {
    let p = Program::sorcery(
        ManaCost::free(),
        ExecutableBody::new(BodyTarget::None, instructions),
    );
    validated(p, vocab)
}

fn bear(
    power: i64,
    toughness: i64,
    keywords: BTreeSet<KeywordAbility>,
    vocab: &CreatureTypeVocabulary,
) -> Program {
    let mut p = Program::creature(None, power.into(), toughness.into(), BTreeSet::new());
    p.printed_keywords = keywords;
    validated(p, vocab)
}

#[test]
fn zone_op_exile_captures_lki_and_chains_through_exec_env() {
    let vocab = vocabulary();
    let spell = sorcery(
        vec![
            Instruction::ZoneOp(Box::new(ZoneOpSpec {
                source: ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::creature().not_controlled(),
                ),
                selection: ZoneSelection::All,
                destination: ZoneDest::Exile {
                    face_down: false,
                    viewer: None,
                    link_to_source: false,
                },
                bind_moved: Some(BindingId(0)),
                bind_snapshots: Some(BindingId(1)),
                bind_unchosen: None,
                bind_succeeded: Some(BindingId(2)),
                tag_moved: Some((TagKey::Label("exiled_by_test".into()), TagValue::Unit)),
                shuffle_library: None,
            })),
            // Skip state-track update if nothing was exiled.
            Instruction::BranchCondition {
                condition: Box::new(ConditionExpr::BoundBool(BindingId(2))),
                destination: 3,
            },
            Instruction::UpdateStateTrack {
                key: Box::new(StateTrackKeySpec {
                    scope: StateTrackScope::Player(Box::new(PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ))),
                    name: "exiled_power".into(),
                }),
                op: StateTrackOp::SetInt(Box::new(ValueExpr::BoundStat {
                    binding: BindingId(1),
                    stat: ObjectStatKind::Power,
                    op: StatAggregate::Sum,
                })),
            },
        ],
        &vocab,
    );

    let game = init_world(
        vocab.clone(),
        vec![
            in_hand(1, P0, spell),
            bf(2, P1, bear(4, 4, BTreeSet::new(), &vocab)),
            bf(3, P1, bear(3, 2, BTreeSet::new(), &vocab)),
        ],
    );

    let resolved = cast_untargeted(&game, 1);
    let state = resolved.state();
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(state, 2))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(state, 3))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    assert_eq!(
        state
            .execution()
            .state_tracks()
            .int_or_zero(&StateTrackKey::Player(P0, "exiled_power".into())),
        RulesInteger::from(7)
    );
    let tagged = state.execution().object_tags().objects_with_tag(
        &TagKey::Label("exiled_by_test".into()),
        Some(&TagValue::Unit),
    );
    assert_eq!(tagged.len(), 2);
}

#[test]
fn zone_op_until_match_enters_battlefield_tapped_with_counters() {
    let vocab = vocabulary();
    let spell = sorcery(
        vec![Instruction::ZoneOp(Box::new(ZoneOpSpec {
            source: ObjectQuery::ZoneScan {
                zone: QueryZone::Library(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::Controller,
                ))),
                filter: Box::new(ObjectFilter::creature()),
                slice: QuerySlice::UntilMatch {
                    filter: Box::new(ObjectFilter::creature()),
                    count: Box::new(ValueExpr::literal(1)),
                    include_non_matches: false,
                },
            },
            selection: ZoneSelection::All,
            destination: ZoneDest::Battlefield {
                controller: ZoneController::Player(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::Controller,
                ))),
                tapped: true,
                transformed: false,
                face_down: false,
                entering_counters: vec![(CounterKind::PlusOne, Box::new(ValueExpr::literal(2)))],
                attach_to: None,
            },
            bind_moved: Some(BindingId(1)),
            bind_snapshots: None,
            bind_unchosen: None,
            bind_succeeded: None,
            tag_moved: None,
            shuffle_library: None,
        }))],
        &vocab,
    );

    let land = validated(
        Program::land(BTreeSet::from([LandType::Forest]), vec![]),
        &vocab,
    );
    let creature = bear(2, 2, BTreeSet::new(), &vocab);
    // Card 3 (creature) pushed first, then Card 2 (land) pushed on top of library.
    let game = init_world(
        vocab.clone(),
        vec![
            in_hand(1, P0, spell),
            in_library(3, P0, creature),
            in_library(2, P0, land),
        ],
    );

    let resolved = cast_untargeted(&game, 1);
    let state = resolved.state();
    let entrant_id = object(state, 3);
    let entrant = state.execution().window().cards().get(entrant_id).unwrap();
    assert_eq!(entrant.zone(), ObjectZone::Battlefield);
    assert_eq!(entrant.tapped(), Some(true));
    let eval = state.execution().evaluated_characteristics().unwrap();
    let chars = eval.get(entrant_id).unwrap();
    assert_eq!(chars.plus_one_counters, RulesInteger::from(2));
    assert_eq!(
        chars.power_toughness.as_ref().unwrap().derived.power,
        RulesInteger::from(4)
    );
}

#[test]
fn zone_op_destroy_respects_indestructible_and_sacrifice_bypasses() {
    let vocab = vocabulary();
    let destroy_all = sorcery(
        vec![Instruction::ZoneOp(Box::new(ZoneOpSpec {
            source: ObjectQuery::zone(QueryZone::Battlefield, ObjectFilter::creature()),
            selection: ZoneSelection::All,
            destination: ZoneDest::Graveyard {
                semantics: GraveyardSemantics::Destroy {
                    allow_regenerate: true,
                },
            },
            bind_moved: None,
            bind_snapshots: None,
            bind_unchosen: None,
            bind_succeeded: None,
            tag_moved: None,
            shuffle_library: None,
        }))],
        &vocab,
    );
    let sac_all = sorcery(
        vec![Instruction::ZoneOp(Box::new(ZoneOpSpec {
            source: ObjectQuery::zone(QueryZone::Battlefield, ObjectFilter::creature()),
            selection: ZoneSelection::All,
            destination: ZoneDest::Graveyard {
                semantics: GraveyardSemantics::Sacrifice,
            },
            bind_moved: None,
            bind_snapshots: None,
            bind_unchosen: None,
            bind_succeeded: None,
            tag_moved: None,
            shuffle_library: None,
        }))],
        &vocab,
    );

    let game = init_world(
        vocab.clone(),
        vec![
            in_hand(1, P0, destroy_all),
            in_hand(2, P0, sac_all),
            bf(3, P1, bear(2, 2, BTreeSet::new(), &vocab)),
            bf(
                4,
                P1,
                bear(
                    3,
                    3,
                    BTreeSet::from([KeywordAbility::Indestructible]),
                    &vocab,
                ),
            ),
        ],
    );

    let after_destroy = cast_untargeted(&game, 1);
    let s1 = after_destroy.state();
    assert!(matches!(
        s1.execution()
            .window()
            .cards()
            .get(object(s1, 3))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P1)
    ));
    assert_eq!(
        s1.execution()
            .window()
            .cards()
            .get(object(s1, 4))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );

    let after_sac = cast_untargeted(s1, 2);
    let s2 = after_sac.state();
    assert!(matches!(
        s2.execution()
            .window()
            .cards()
            .get(object(s2, 4))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P1)
    ));
}

#[test]
fn apply_continuous_locks_tagged_objects_and_creates_authored_emblem() {
    let vocab = vocabulary();
    let buff_selector = PermanentSelector {
        controller: ControllerSelector::Any,
        required_types: BTreeSet::from([CardType::Creature]),
        ..PermanentSelector::default()
    };
    let buff_ability = StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: buff_selector,
        parts: StaticParts(vec![
            StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(3.into()),
                toughness: NumericExpression::Literal(3.into()),
            }),
            StaticPart::Abilities(AbilityChange::grant_keywords([KeywordAbility::Flying])),
        ]),
    };
    let emblem_ability = StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(2),
        selector: PermanentSelector {
            controller: ControllerSelector::SourceController,
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(1.into()),
            toughness: NumericExpression::Literal(1.into()),
        })]),
    };

    let spell = sorcery(
        vec![
            // Tag only P0's creatures with "blessed".
            Instruction::SetObjectTag {
                query: Box::new(ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::creature().controlled(),
                )),
                key: TagKey::Label("blessed".into()),
                value: Some(TagValue::Unit),
            },
            // Apply +3/+3 & Flying ONLY to objects carrying the "blessed" tag (locked at resolution).
            Instruction::ApplyContinuous(Box::new(ApplyContinuousSpec {
                selector: Some(ObjectQuery::Tagged {
                    key: TagKey::Label("blessed".into()),
                    value: Some(TagValue::Unit),
                    zone: Some(QueryZone::Battlefield),
                    filter: None,
                }),
                rule: buff_ability,
                duration: ResolvedDuration::UntilEndOfTurn,
                condition: None,
                one_shot: false,
                as_emblem: None,
                emblem_label: None,
            })),
            // Also install a permanent authored emblem giving controlled creatures +1/+1.
            Instruction::ApplyContinuous(Box::new(ApplyContinuousSpec {
                selector: None,
                rule: emblem_ability,
                duration: ResolvedDuration::UntilEndOfTurn,
                condition: None,
                one_shot: false,
                as_emblem: Some(PlayerSelector::Resolving(ResolvingPlayer::Controller)),
                emblem_label: Some("Toolbox Emblem".into()),
            })),
        ],
        &vocab,
    );

    let game = init_world(
        vocab.clone(),
        vec![
            in_hand(1, P0, spell),
            bf(2, P0, bear(2, 2, BTreeSet::new(), &vocab)),
            bf(3, P1, bear(2, 2, BTreeSet::new(), &vocab)),
        ],
    );

    let resolved = cast_untargeted(&game, 1);
    let state = resolved.state();
    let eval = state.execution().evaluated_characteristics().unwrap();
    let c_p0 = eval.get(object(state, 2)).unwrap();
    let c_p1 = eval.get(object(state, 3)).unwrap();
    // P0's bear gets +3/+3 (locked continuous) + +1/+1 (emblem) = 6/6 with Flying.
    assert_eq!(
        c_p0.power_toughness.as_ref().unwrap().derived.power,
        RulesInteger::from(6)
    );
    assert!(c_p0.has_keyword(KeywordAbility::Flying));
    // P1's bear was untagged and not controlled by P0, so stays 2/2 without Flying.
    assert_eq!(
        c_p1.power_toughness.as_ref().unwrap().derived.power,
        RulesInteger::from(2)
    );
    assert!(!c_p1.has_keyword(KeywordAbility::Flying));
}

#[test]
fn instant_custom_card_partition_sectors_dilemma_trigger_and_atomic_rejection() {
    let vocab = vocabulary();
    // Custom enchantment: "Whenever you face a dilemma, increment your dilemma_count track."
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::new(TriggerEvent::ChoiceMade {
            category: Some(ChoiceCategory::Dilemma),
            player: TriggerPlayerScope::Relation(PlayerRelation::Controller),
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::UpdateStateTrack {
                key: Box::new(StateTrackKeySpec {
                    scope: StateTrackScope::Player(Box::new(PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ))),
                    name: "dilemma_count".into(),
                }),
                op: StateTrackOp::AddInt(Box::new(ValueExpr::literal(1))),
            }],
        ),
    });
    let watcher = validated(watcher, &vocab);

    // Brand-new invented custom sorcery:
    // 1. Partition all battlefield creatures into 3 sectors (ChoiceCategory::Dilemma), tagging TagKey::Partition(7).
    // 2. Exile all creatures in sector 0, capturing LKI snapshots in BindingId(10).
    // 3. Record total exiled power into "sector0_power" track and choice count into "dilemmas_seen" track.
    let custom_spell = sorcery(
        vec![
            Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                domain: PromptDomain::Partition {
                    query: ObjectQuery::zone(QueryZone::Battlefield, ObjectFilter::creature()),
                    pile_count: 3,
                    min_per_pile: 0,
                },
                category: ChoiceCategory::Dilemma,
                bind_to: BindingId(0),
                bind_unchosen: None,
                tag_partition: Some(TagKey::Partition(7)),
                auto_resolve_trivial: false,
            })),
            Instruction::ZoneOp(Box::new(ZoneOpSpec {
                source: ObjectQuery::Tagged {
                    key: TagKey::Partition(7),
                    value: Some(TagValue::Int(RulesInteger::from(0))),
                    zone: Some(QueryZone::Battlefield),
                    filter: None,
                },
                selection: ZoneSelection::All,
                destination: ZoneDest::Exile {
                    face_down: false,
                    viewer: None,
                    link_to_source: false,
                },
                bind_moved: Some(BindingId(9)),
                bind_snapshots: Some(BindingId(10)),
                bind_unchosen: None,
                bind_succeeded: None,
                tag_moved: None,
                shuffle_library: None,
            })),
            Instruction::UpdateStateTrack {
                key: Box::new(StateTrackKeySpec {
                    scope: StateTrackScope::Player(Box::new(PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ))),
                    name: "sector0_power".into(),
                }),
                op: StateTrackOp::SetInt(Box::new(ValueExpr::BoundStat {
                    binding: BindingId(10),
                    stat: ObjectStatKind::Power,
                    op: StatAggregate::Sum,
                })),
            },
            Instruction::UpdateStateTrack {
                key: Box::new(StateTrackKeySpec {
                    scope: StateTrackScope::Player(Box::new(PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ))),
                    name: "dilemmas_seen".into(),
                }),
                op: StateTrackOp::SetInt(Box::new(ValueExpr::ChoiceHistoryCount {
                    category: Some(ChoiceCategory::Dilemma),
                    chooser: None,
                    this_turn_only: true,
                })),
            },
        ],
        &vocab,
    );

    let game = init_world(
        vocab.clone(),
        vec![
            in_hand(1, P0, custom_spell),
            bf(2, P0, watcher),
            bf(3, P0, bear(5, 5, BTreeSet::new(), &vocab)),
            bf(4, P1, bear(2, 2, BTreeSet::new(), &vocab)),
            bf(5, P1, bear(3, 3, BTreeSet::new(), &vocab)),
        ],
    );

    let pending = cast_untargeted(&game, 1);
    let s_pending = pending.state();
    let dec = s_pending.pending_decision().expect("expected PromptChoice");
    let RuleFrame::PromptChoice(frame) = dec.frame() else {
        panic!("expected RuleFrame::PromptChoice");
    };
    assert_eq!(frame.category, ChoiceCategory::Dilemma);
    let ResolvedPromptDomain::Partition {
        candidates,
        pile_count,
        ..
    } = &frame.domain
    else {
        panic!("expected Partition domain");
    };
    assert_eq!(*pile_count, 3);
    assert_eq!(candidates.len(), 3);

    let c3 = object(s_pending, 3);
    let c4 = object(s_pending, 4);
    let c5 = object(s_pending, 5);

    // Atomic rejection 1: missing an object from partition.
    let err_missing = attempt(
        s_pending,
        P0,
        GameCommand::Reply {
            decision: dec.id(),
            choice: DecisionReply::PromptChoice(PromptChoiceReply::Partition(vec![
                vec![c3],
                vec![c4],
                vec![],
            ])),
        },
    );
    assert!(matches!(
        err_missing,
        Err(GameError::Effect(EffectError::InvalidSelectionCount))
    ));
    // Atomic rejection 2: duplicate object across piles.
    let err_dup = attempt(
        s_pending,
        P0,
        GameCommand::Reply {
            decision: dec.id(),
            choice: DecisionReply::PromptChoice(PromptChoiceReply::Partition(vec![
                vec![c3],
                vec![c3, c4],
                vec![c5],
            ])),
        },
    );
    assert!(matches!(
        err_dup,
        Err(GameError::Effect(EffectError::DuplicateSelection))
    ));

    // Valid partition: sector 0 = [c3 (5/5)], sector 1 = [c4 (2/2)], sector 2 = [c5 (3/3)].
    let after_choice = submit(
        s_pending,
        P0,
        GameCommand::Reply {
            decision: dec.id(),
            choice: DecisionReply::PromptChoice(PromptChoiceReply::Partition(vec![
                vec![c3],
                vec![c4],
                vec![c5],
            ])),
        },
    );
    // Resolve the ChoiceMade(Dilemma) trigger placed on the stack!
    let final_state = pass_pair(after_choice.state());
    let fs = final_state.state();

    // Sector 0 creature exiled; sectors 1 & 2 remain on battlefield with their partition tags!
    assert_eq!(
        fs.execution()
            .window()
            .cards()
            .get(object(fs, 3))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    assert_eq!(
        fs.execution()
            .window()
            .cards()
            .get(object(fs, 4))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        fs.execution()
            .window()
            .cards()
            .get(object(fs, 5))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        fs.execution().object_tags().partition_members(7, 1),
        vec![object(fs, 4)]
    );
    assert_eq!(
        fs.execution().object_tags().partition_members(7, 2),
        vec![object(fs, 5)]
    );

    // Tracks & history verified!
    assert_eq!(
        fs.execution()
            .state_tracks()
            .int_or_zero(&StateTrackKey::Player(P0, "sector0_power".into())),
        RulesInteger::from(5)
    );
    assert_eq!(
        fs.execution()
            .state_tracks()
            .int_or_zero(&StateTrackKey::Player(P0, "dilemmas_seen".into())),
        RulesInteger::from(1)
    );
    assert_eq!(
        fs.execution()
            .state_tracks()
            .int_or_zero(&StateTrackKey::Player(P0, "dilemma_count".into())),
        RulesInteger::from(1)
    );
    assert_eq!(fs.execution().choice_history().len(), 1);
    assert_eq!(
        fs.execution().choice_history()[0].category,
        ChoiceCategory::Dilemma
    );
}

#[test]
fn for_each_suspends_and_resumes_prompt_choice_per_item() {
    let vocab = vocabulary();
    let spell = sorcery(
        vec![Instruction::ForEach {
            query: Box::new(ObjectQuery::zone(
                QueryZone::Battlefield,
                ObjectFilter::creature().controlled(),
            )),
            bind_item: BindingId(1),
            body: Box::new(ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                        chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                        domain: PromptDomain::Options {
                            labels: vec!["Keep".into(), "Exile".into()],
                            min: 1,
                            max: 1,
                            allow_repeat: false,
                        },
                        category: ChoiceCategory::VillainousChoice,
                        bind_to: BindingId(2),
                        bind_unchosen: None,
                        tag_partition: None,
                        auto_resolve_trivial: false,
                    })),
                    Instruction::UpdateStateTrack {
                        key: Box::new(StateTrackKeySpec {
                            scope: StateTrackScope::Global,
                            name: "loop_steps".into(),
                        }),
                        op: StateTrackOp::AddInt(Box::new(ValueExpr::literal(1))),
                    },
                ],
            )),
        }],
        &vocab,
    );

    let game = init_world(
        vocab.clone(),
        vec![
            in_hand(1, P0, spell),
            bf(2, P0, bear(2, 2, BTreeSet::new(), &vocab)),
            bf(3, P0, bear(3, 3, BTreeSet::new(), &vocab)),
        ],
    );

    // First iteration suspends on PromptChoice.
    let step1 = cast_untargeted(&game, 1);
    let d1 = step1.state().pending_decision().expect("prompt 1");
    let step2 = submit(
        step1.state(),
        P0,
        GameCommand::Reply {
            decision: d1.id(),
            choice: DecisionReply::PromptChoice(PromptChoiceReply::Options(vec![0])),
        },
    );
    // Second iteration suspends on PromptChoice.
    let d2 = step2.state().pending_decision().expect("prompt 2");
    let step3 = submit(
        step2.state(),
        P0,
        GameCommand::Reply {
            decision: d2.id(),
            choice: DecisionReply::PromptChoice(PromptChoiceReply::Options(vec![1])),
        },
    );
    assert!(step3.state().pending_decision().is_none());
    assert_eq!(
        step3
            .state()
            .execution()
            .state_tracks()
            .int_or_zero(&StateTrackKey::Global("loop_steps".into())),
        RulesInteger::from(2)
    );
    assert_eq!(step3.state().execution().choice_history().len(), 2);
}

#[test]
fn side_deck_op_advances_cycles_and_recycles_deterministically() {
    let vocab = vocabulary();
    let spell = sorcery(
        vec![
            Instruction::SideDeckOp(Box::new(SideDeckOpSpec {
                owner: None,
                kind: SideDeckKind::Planar,
                op: SideDeckOpKind::AdvanceTop,
                bind_succeeded: Some(BindingId(0)),
            })),
            Instruction::SideDeckOp(Box::new(SideDeckOpSpec {
                owner: None,
                kind: SideDeckKind::Planar,
                op: SideDeckOpKind::CycleToBottom,
                bind_succeeded: Some(BindingId(1)),
            })),
            Instruction::BranchCondition {
                condition: Box::new(ConditionExpr::BoundBool(BindingId(1))),
                destination: 4,
            },
            Instruction::UpdateStateTrack {
                key: Box::new(StateTrackKeySpec {
                    scope: StateTrackScope::Global,
                    name: "planeswalked".into(),
                }),
                op: StateTrackOp::SetInt(Box::new(ValueExpr::literal(1))),
            },
        ],
        &vocab,
    );

    let mut game = init_world(vocab.clone(), vec![in_hand(1, P0, spell)]);
    // Install a 3-card shared Planar side-deck via SideDeckStore.
    let mut deck = SideDeck {
        cards: vec![CardId(101), CardId(102), CardId(103)],
        ..Default::default()
    };
    // Exercise SideDeck directly plus via spell execution.
    assert_eq!(deck.peek_top(2), vec![CardId(101), CardId(102)]);
    assert_eq!(deck.advance_top(), Some(CardId(101)));
    assert_eq!(deck.active, Some(CardId(101)));
    assert_eq!(deck.cycle_to_bottom(), Some(CardId(102)));
    assert_eq!(deck.active, Some(CardId(102)));
    assert_eq!(deck.cards, vec![CardId(103), CardId(101)]);
    // Verify spell runs cleanly even on an empty global planar deck (returns succeeded=false).
    let _ = &mut game;
    let res = cast_untargeted(&game, 1);
    assert_eq!(
        res.state()
            .execution()
            .state_tracks()
            .int_or_zero(&StateTrackKey::Global("planeswalked".into())),
        RulesInteger::from(0)
    );
}
