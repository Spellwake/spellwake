#![forbid(unsafe_code)]
//! Behavioral integration tests for the Universal Rules Toolbox (PR 1 core foundation).

#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeVocabulary};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        AbilityChange, ControllerSelector, KeywordAbility, NumericExpression, NumericPair,
        PermanentSelector, ResolvedDuration, StaticAbilityId, StaticContinuousAbility, StaticPart,
        StaticParts,
    },
    combat::{AttackDeclaration, CombatDecision},
    damage::{PreventionRecipientSpec, PreventionSourceSpec, TurnPreventionSpec},
    effects::{CounterKind, EffectError, SeedCard},
    game::{
        DecisionReply, GameCommand, GameError, GameInput, GameState, NativeCandidate, RuleFrame,
    },
    land::LandType,
    mana::{ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    occurrence::RulesOccurrence,
    priority::SeatId,
    program::{
        AffectedPlayers, BodyTarget, CardType, CombatActionKind, ExecutableBody, Instruction,
        ObjectFilter, ObjectStatKind, PlayerRelation, PlayerTriggerAction, Program,
        ResolvingPlayer, StatAggregate, TemporaryRuleKind, TriggerEvent, TriggerGrouping,
        TriggerPlayerScope, TriggerSpec, TriggeredAbility, TriggeredAbilityId, ValueExpr,
    },
    targets::TargetRef,
    toolbox::{
        AdjustLifeSpec, ApplyContinuousSpec, BindingId, ChoiceCategory, ConditionExpr,
        ContinuousPayload, DamageSourceQuery, DamageTargetQuery, DealDamageSpec, DrawCardsSpec,
        GraveyardSemantics, LifeOp, MutateObjectSpec, MutatePlayerSpec, ObjectMutation,
        ObjectQuery, PlayerSelector, PromptChoiceReply, PromptChoiceSpec, PromptDomain, QuerySlice,
        QueryZone, ResolvedPromptDomain, SideDeck, SideDeckKind, SideDeckOpKind, SideDeckOpSpec,
        StateTrackKey, StateTrackKeySpec, StateTrackOp, StateTrackScope, TagKey, TagValue,
        ZoneController, ZoneDest, ZoneOpSpec, ZoneSelection,
    },
    turn::Step,
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
                destination: ZoneDest::exile(),
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
                attacking: false,
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
                payload: ContinuousPayload::Static(Box::new(buff_ability)),
                duration: ResolvedDuration::UntilEndOfTurn,
                condition: None,
                one_shot: false,
                as_emblem: None,
                emblem_label: None,
            })),
            // Also install a permanent authored emblem giving controlled creatures +1/+1.
            Instruction::ApplyContinuous(Box::new(ApplyContinuousSpec {
                selector: None,
                payload: ContinuousPayload::Static(Box::new(emblem_ability)),
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
                destination: ZoneDest::exile(),
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

fn hand_count(game: &GameState, seat: SeatId) -> usize {
    game.execution()
        .window()
        .cards()
        .iter()
        .filter(|card| card.zone() == ObjectZone::Hand(seat))
        .count()
}

fn library_count(game: &GameState, seat: SeatId) -> usize {
    game.execution().window().cards().library(seat).len()
}

fn advance_to_next_turn_main(mut game: GameState) -> GameState {
    let start_turn = game.turn().unwrap().position().number;
    for _ in 0..80 {
        let pos = game.turn().unwrap().position();
        if pos.number > start_turn && pos.step == Step::PrecombatMain {
            return game;
        }
        if let Some(pending) = game.pending_decision() {
            let chooser = pending.chooser();
            let choice = match pending.frame() {
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    DecisionReply::DeclareAttackers(vec![])
                }
                RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                    DecisionReply::DeclareBlockers(vec![])
                }
                other => panic!("unexpected decision while advancing turn: {other:?}"),
            };
            game = reply(&game, chooser, choice).into_state();
        } else {
            let active = game.execution().window().active_player();
            let order = if active == P0 { [P0, P1] } else { [P1, P0] };
            game = pass_in_order(&game, order).into_state();
        }
    }
    panic!("did not reach next turn PrecombatMain");
}

fn advance_to_declare_attackers(game: &GameState) -> GameState {
    let active = game.execution().window().active_player();
    let order = if active == P0 { [P0, P1] } else { [P1, P0] };
    let beg_combat = pass_in_order(game, order);
    assert_eq!(
        beg_combat.state().turn().unwrap().position().step,
        Step::BeginningCombat
    );
    let dec_atk = pass_in_order(beg_combat.state(), order);
    assert_eq!(
        dec_atk.state().turn().unwrap().position().step,
        Step::DeclareAttackers
    );
    dec_atk.into_state()
}

fn instant_noop(vocab: &CreatureTypeVocabulary) -> Program {
    let p = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::UpdateStateTrack {
                key: Box::new(StateTrackKeySpec {
                    scope: StateTrackScope::Global,
                    name: "noop".into(),
                }),
                op: StateTrackOp::AddInt(Box::new(ValueExpr::literal(1))),
            }],
        ),
    );
    validated(p, vocab)
}

fn pinger_creature(vocab: &CreatureTypeVocabulary) -> Program {
    let mut p = Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
    p.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SingleAnyTarget,
            vec![Instruction::DealDamage(Box::new(DealDamageSpec {
                source: DamageSourceQuery::EntrySource,
                recipients: DamageTargetQuery::Targets,
                amount: spellwake_engine::program::DamageAmountSpec::Expr(ValueExpr::literal(3)),
                exile_if_dies: false,
            }))],
        ),
    });
    validated(p, vocab)
}

#[test]
fn draw_cards_controller_opponent_each_player_and_exec_env_bind() {
    let vocab = vocabulary();
    let spell = sorcery(
        vec![
            Instruction::DrawCards(Box::new(DrawCardsSpec {
                players: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                amount: ValueExpr::literal(2),
                bind_drawn: Some(BindingId(0)),
            })),
            Instruction::UpdateStateTrack {
                key: Box::new(StateTrackKeySpec {
                    scope: StateTrackScope::Global,
                    name: "drawn_by_ctrl".into(),
                }),
                op: StateTrackOp::SetInt(Box::new(ValueExpr::BoundCount {
                    binding: BindingId(0),
                    filter: None,
                })),
            },
            Instruction::DrawCards(Box::new(DrawCardsSpec::simple(
                PlayerSelector::Group(AffectedPlayers::EachOpponent),
                ValueExpr::literal(1),
            ))),
            Instruction::DrawCards(Box::new(DrawCardsSpec::simple(
                PlayerSelector::Group(AffectedPlayers::EachPlayer),
                ValueExpr::literal(1),
            ))),
        ],
        &vocab,
    );

    let mut seeds = vec![in_hand(1, P0, spell)];
    for i in 0..4 {
        seeds.push(in_library(10 + i, P0, bear(2, 2, BTreeSet::new(), &vocab)));
        seeds.push(in_library(20 + i, P1, bear(2, 2, BTreeSet::new(), &vocab)));
    }
    let game = init_world(vocab, seeds);

    let resolved = cast_untargeted(&game, 1);
    let state = resolved.state();
    assert_eq!(
        state
            .execution()
            .state_tracks()
            .int_or_zero(&StateTrackKey::Global("drawn_by_ctrl".into())),
        RulesInteger::from(2)
    );
    assert_eq!(hand_count(state, P0), 3);
    assert_eq!(hand_count(state, P1), 2);
    assert_eq!(library_count(state, P0), 1);
    assert_eq!(library_count(state, P1), 2);
}

#[test]
fn mutate_object_tap_untap_toggle_and_skip_next_untap() {
    let vocab = vocabulary();
    let tap_mine = sorcery(
        vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
            ObjectQuery::zone(
                QueryZone::Battlefield,
                ObjectFilter::creature().controlled(),
            ),
            ObjectMutation::SetTapped(true),
        )))],
        &vocab,
    );
    let toggle_all = sorcery(
        vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
            ObjectQuery::zone(QueryZone::Battlefield, ObjectFilter::creature()),
            ObjectMutation::ToggleTapped,
        )))],
        &vocab,
    );
    let untap_opposing = sorcery(
        vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
            ObjectQuery::zone(QueryZone::Battlefield, ObjectFilter::creature().opposing()),
            ObjectMutation::SetTapped(false),
        )))],
        &vocab,
    );
    let freeze_opposing_flyer = sorcery(
        vec![
            Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                ObjectQuery::zone(QueryZone::Battlefield, ObjectFilter::creature().opposing()),
                ObjectMutation::SetTapped(true),
            ))),
            Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::creature()
                        .opposing()
                        .with_keyword(KeywordAbility::Flying),
                ),
                ObjectMutation::SkipNextUntap,
            ))),
        ],
        &vocab,
    );

    let mut seeds = vec![
        in_hand(10, P0, tap_mine),
        in_hand(11, P0, toggle_all),
        in_hand(12, P0, untap_opposing),
        in_hand(13, P0, freeze_opposing_flyer),
        bf(2, P0, bear(2, 2, BTreeSet::new(), &vocab)),
        bf(
            3,
            P1,
            bear(2, 2, BTreeSet::from([KeywordAbility::Flying]), &vocab),
        ),
        bf(4, P1, bear(2, 2, BTreeSet::new(), &vocab)),
    ];
    for i in 0..4 {
        seeds.push(in_library(100 + i, P0, bear(1, 1, BTreeSet::new(), &vocab)));
        seeds.push(in_library(200 + i, P1, bear(1, 1, BTreeSet::new(), &vocab)));
    }
    let game = init_world(vocab, seeds);
    let b2 = object(&game, 2);
    let b3 = object(&game, 3);
    let b4 = object(&game, 4);

    let s1 = cast_untargeted(&game, 10).into_state();
    assert_eq!(
        s1.execution().window().cards().get(b2).unwrap().tapped(),
        Some(true)
    );
    assert_eq!(
        s1.execution().window().cards().get(b3).unwrap().tapped(),
        Some(false)
    );
    assert_eq!(
        s1.execution().window().cards().get(b4).unwrap().tapped(),
        Some(false)
    );

    let s2 = cast_untargeted(&s1, 11).into_state();
    assert_eq!(
        s2.execution().window().cards().get(b2).unwrap().tapped(),
        Some(false)
    );
    assert_eq!(
        s2.execution().window().cards().get(b3).unwrap().tapped(),
        Some(true)
    );
    assert_eq!(
        s2.execution().window().cards().get(b4).unwrap().tapped(),
        Some(true)
    );

    let s3 = cast_untargeted(&s2, 12).into_state();
    assert_eq!(
        s3.execution().window().cards().get(b3).unwrap().tapped(),
        Some(false)
    );
    assert_eq!(
        s3.execution().window().cards().get(b4).unwrap().tapped(),
        Some(false)
    );

    let s4 = cast_untargeted(&s3, 13).into_state();
    assert_eq!(
        s4.execution().window().cards().get(b3).unwrap().tapped(),
        Some(true)
    );
    assert_eq!(
        s4.execution().window().cards().get(b4).unwrap().tapped(),
        Some(true)
    );

    // Turn 2 is P1's turn: b4 untaps normally, while b3 stays tapped due to SkipNextUntap.
    let turn2 = advance_to_next_turn_main(s4);
    assert_eq!(turn2.execution().window().active_player(), P1);
    assert_eq!(
        turn2.execution().window().cards().get(b3).unwrap().tapped(),
        Some(true)
    );
    assert_eq!(
        turn2.execution().window().cards().get(b4).unwrap().tapped(),
        Some(false)
    );
}

#[test]
fn mutate_object_phase_out_regenerate_goad_and_exert() {
    let vocab = vocabulary();
    let regen_and_destroy_flyer = sorcery(
        vec![
            Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::creature()
                        .controlled()
                        .with_keyword(KeywordAbility::Flying),
                ),
                ObjectMutation::Regenerate,
            ))),
            Instruction::ZoneOp(Box::new(ZoneOpSpec {
                source: ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::creature()
                        .controlled()
                        .with_keyword(KeywordAbility::Flying),
                ),
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
            })),
        ],
        &vocab,
    );
    let phase_out_opposing_flyer = sorcery(
        vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
            ObjectQuery::zone(
                QueryZone::Battlefield,
                ObjectFilter::creature()
                    .opposing()
                    .with_keyword(KeywordAbility::Flying),
            ),
            ObjectMutation::PhaseOut,
        )))],
        &vocab,
    );
    let tap_and_exert_vigilant = sorcery(
        vec![
            Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::creature()
                        .controlled()
                        .with_keyword(KeywordAbility::Vigilance),
                ),
                ObjectMutation::SetTapped(true),
            ))),
            Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::creature()
                        .controlled()
                        .with_keyword(KeywordAbility::Vigilance),
                ),
                ObjectMutation::Exert,
            ))),
        ],
        &vocab,
    );
    let goad_opposing_vanilla = sorcery(
        vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
            ObjectQuery::zone(
                QueryZone::Battlefield,
                ObjectFilter::creature()
                    .opposing()
                    .without_keyword(KeywordAbility::Flying),
            ),
            ObjectMutation::Goad {
                by: PlayerSelector::Resolving(ResolvingPlayer::Controller),
            },
        )))],
        &vocab,
    );

    let mut seeds = vec![
        in_hand(10, P0, regen_and_destroy_flyer),
        in_hand(11, P0, phase_out_opposing_flyer),
        in_hand(12, P0, tap_and_exert_vigilant),
        in_hand(13, P0, goad_opposing_vanilla),
        bf(
            2,
            P0,
            bear(2, 2, BTreeSet::from([KeywordAbility::Flying]), &vocab),
        ),
        bf(
            3,
            P0,
            bear(2, 2, BTreeSet::from([KeywordAbility::Vigilance]), &vocab),
        ),
        bf(
            4,
            P1,
            bear(2, 2, BTreeSet::from([KeywordAbility::Flying]), &vocab),
        ),
        bf(5, P1, bear(2, 2, BTreeSet::new(), &vocab)),
    ];
    for i in 0..6 {
        seeds.push(in_library(100 + i, P0, bear(1, 1, BTreeSet::new(), &vocab)));
        seeds.push(in_library(200 + i, P1, bear(1, 1, BTreeSet::new(), &vocab)));
    }
    let game = init_world(vocab, seeds);
    let b2 = object(&game, 2);
    let b3 = object(&game, 3);
    let b4 = object(&game, 4);
    let b5 = object(&game, 5);

    // 1. Regenerate saves b2 from destruction and leaves it tapped on the battlefield.
    let s1 = cast_untargeted(&game, 10).into_state();
    assert_eq!(
        s1.execution().window().cards().get(b2).unwrap().zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        s1.execution().window().cards().get(b2).unwrap().tapped(),
        Some(true)
    );
    assert!(
        s1.execution()
            .regeneration_shields()
            .get(&b2)
            .is_none_or(BTreeSet::is_empty)
    );

    // 2. PhaseOut phases out b4 under P1.
    let s2 = cast_untargeted(&s1, 11).into_state();
    assert_eq!(
        s2.execution().window().cards().phased_out_under(b4),
        Some(P1)
    );

    // 3. Exert emits KeywordActionPerformed(Exerted) and marks b3 not to untap next turn.
    let exert_cand = cast_untargeted(&s2, 12);
    assert!(all_occurrences(&exert_cand).iter().any(|occ| matches!(
        occ,
        RulesOccurrence::KeywordActionPerformed {
            player,
            action: PlayerTriggerAction::Exerted,
        } if *player == P0
    )));
    let s3 = exert_cand.into_state();

    // 4. Goad b5 so it must attack on P1's next turn.
    let s4 = cast_untargeted(&s3, 13).into_state();

    // Turn 2 (P1): b4 phased back in; goaded b5 must attack in DeclareAttackers.
    let turn2 = advance_to_next_turn_main(s4);
    assert_eq!(turn2.execution().window().active_player(), P1);
    assert_eq!(
        turn2.execution().window().cards().phased_out_under(b4),
        None
    );

    let dec_atk = advance_to_declare_attackers(&turn2);
    let decision = dec_atk.pending_decision().expect("attackers decision");
    assert_eq!(decision.chooser(), P1);
    assert!(
        attempt(
            &dec_atk,
            P1,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::DeclareAttackers(vec![]),
            },
        )
        .is_err(),
        "goaded creature must attack when able"
    );
    let after_atk = reply(
        &dec_atk,
        P1,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: b5,
            defender: P0,
            defending_planeswalker: None,
        }]),
    )
    .into_state();

    // Turn 3 (P0): b2 untaps normally, while exerted b3 stays tapped.
    let turn3 = advance_to_next_turn_main(after_atk);
    assert_eq!(turn3.execution().window().active_player(), P0);
    assert_eq!(
        turn3.execution().window().cards().get(b2).unwrap().tapped(),
        Some(false)
    );
    assert_eq!(
        turn3.execution().window().cards().get(b3).unwrap().tapped(),
        Some(true)
    );
}

#[test]
fn mutate_object_counters_add_remove_clear_double_move_and_add_one_of_each() {
    let vocab = vocabulary();
    let flyer_query = || {
        ObjectQuery::zone(
            QueryZone::Battlefield,
            ObjectFilter::creature()
                .controlled()
                .with_keyword(KeywordAbility::Flying),
        )
    };
    let vanilla_query = || {
        ObjectQuery::zone(
            QueryZone::Battlefield,
            ObjectFilter::creature()
                .controlled()
                .without_keyword(KeywordAbility::Flying),
        )
    };

    let seed_and_proliferate_one = sorcery(
        vec![
            Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                flyer_query(),
                ObjectMutation::AddCounters {
                    kind: CounterKind::PlusOne,
                    amount: ValueExpr::literal(2),
                },
            ))),
            Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                flyer_query(),
                ObjectMutation::AddCounters {
                    kind: CounterKind::Charge,
                    amount: ValueExpr::literal(1),
                },
            ))),
            Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                flyer_query(),
                ObjectMutation::AddOneOfEachCounterKind,
            ))),
        ],
        &vocab,
    );
    let double_plus_one = sorcery(
        vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
            flyer_query(),
            ObjectMutation::DoubleCounters {
                kind: Some(CounterKind::PlusOne),
            },
        )))],
        &vocab,
    );
    let remove_and_move = sorcery(
        vec![
            Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                flyer_query(),
                ObjectMutation::RemoveCounters {
                    kind: Some(CounterKind::PlusOne),
                    amount: ValueExpr::literal(2),
                },
            ))),
            Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                flyer_query(),
                ObjectMutation::MoveCounters {
                    to: vanilla_query(),
                    kind: Some(CounterKind::PlusOne),
                    amount: Some(ValueExpr::literal(3)),
                },
            ))),
        ],
        &vocab,
    );
    let clear_all = sorcery(
        vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
            flyer_query(),
            ObjectMutation::ClearCounters { kind: None },
        )))],
        &vocab,
    );

    let game = init_world(
        vocab.clone(),
        vec![
            in_hand(10, P0, seed_and_proliferate_one),
            in_hand(11, P0, double_plus_one),
            in_hand(12, P0, remove_and_move),
            in_hand(13, P0, clear_all),
            bf(
                2,
                P0,
                bear(2, 2, BTreeSet::from([KeywordAbility::Flying]), &vocab),
            ),
            bf(3, P0, bear(2, 2, BTreeSet::new(), &vocab)),
        ],
    );
    let b2 = object(&game, 2);
    let b3 = object(&game, 3);

    // Add 2 +1/+1, 1 Charge, then AddOneOfEachCounterKind -> 3 +1/+1 and 2 Charge.
    let s1 = cast_untargeted(&game, 10).into_state();
    assert_eq!(
        s1.execution().counters_of_kind(b2, CounterKind::PlusOne),
        RulesInteger::from(3)
    );
    assert_eq!(
        s1.execution().counters_of_kind(b2, CounterKind::Charge),
        RulesInteger::from(2)
    );

    // DoubleCounters(Some(PlusOne)) -> 6 +1/+1, Charge unchanged at 2.
    let s2 = cast_untargeted(&s1, 11).into_state();
    assert_eq!(
        s2.execution().counters_of_kind(b2, CounterKind::PlusOne),
        RulesInteger::from(6)
    );
    assert_eq!(
        s2.execution().counters_of_kind(b2, CounterKind::Charge),
        RulesInteger::from(2)
    );

    // Remove 2 +1/+1 (leaving 4), then Move 3 +1/+1 from b2 to b3 (leaving 1 on b2, 3 on b3).
    let s3 = cast_untargeted(&s2, 12).into_state();
    assert_eq!(
        s3.execution().counters_of_kind(b2, CounterKind::PlusOne),
        RulesInteger::from(1)
    );
    assert_eq!(
        s3.execution().counters_of_kind(b2, CounterKind::Charge),
        RulesInteger::from(2)
    );
    assert_eq!(
        s3.execution().counters_of_kind(b3, CounterKind::PlusOne),
        RulesInteger::from(3)
    );

    // ClearCounters(None) removes every counter kind from b2 without touching b3.
    let s4 = cast_untargeted(&s3, 13).into_state();
    assert_eq!(
        s4.execution().counters_of_kind(b2, CounterKind::PlusOne),
        RulesInteger::from(0)
    );
    assert_eq!(
        s4.execution().counters_of_kind(b2, CounterKind::Charge),
        RulesInteger::from(0)
    );
    assert_eq!(
        s4.execution().counters_of_kind(b3, CounterKind::PlusOne),
        RulesInteger::from(3)
    );
}

#[test]
fn mutate_object_attach_unattach_and_gain_control() {
    let vocab = vocabulary();
    let equip_prog = validated(
        Program::equipment(Some(ManaCost::free()), ManaCost::free()),
        &vocab,
    );
    let attach_spell = sorcery(
        vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
            ObjectQuery::zone(QueryZone::Battlefield, ObjectFilter::artifact()),
            ObjectMutation::Attach {
                host: ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::creature().controlled(),
                ),
            },
        )))],
        &vocab,
    );
    let unattach_spell = sorcery(
        vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
            ObjectQuery::zone(QueryZone::Battlefield, ObjectFilter::artifact()),
            ObjectMutation::Unattach,
        )))],
        &vocab,
    );
    let steal_spell = sorcery(
        vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
            ObjectQuery::zone(QueryZone::Battlefield, ObjectFilter::creature().opposing()),
            ObjectMutation::GainControl {
                controller: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                duration: ResolvedDuration::UntilEndOfTurn,
            },
        )))],
        &vocab,
    );

    let mut seeds = vec![
        in_hand(10, P0, attach_spell),
        in_hand(11, P0, unattach_spell),
        in_hand(12, P0, steal_spell),
        bf(2, P0, equip_prog),
        bf(3, P0, bear(2, 2, BTreeSet::new(), &vocab)),
        bf(4, P1, bear(3, 3, BTreeSet::new(), &vocab)),
    ];
    for i in 0..4 {
        seeds.push(in_library(100 + i, P0, bear(1, 1, BTreeSet::new(), &vocab)));
        seeds.push(in_library(200 + i, P1, bear(1, 1, BTreeSet::new(), &vocab)));
    }
    let game = init_world(vocab, seeds);
    let equip_id = object(&game, 2);
    let b3 = object(&game, 3);
    let b4 = object(&game, 4);

    assert_eq!(game.execution().attached_to(equip_id), None);
    let s1 = cast_untargeted(&game, 10).into_state();
    assert_eq!(
        s1.execution().attached_to(equip_id),
        Some(TargetRef::Object(b3))
    );

    let s2 = cast_untargeted(&s1, 11).into_state();
    assert_eq!(s2.execution().attached_to(equip_id), None);

    let s3 = cast_untargeted(&s2, 12).into_state();
    assert_eq!(current(&s3, b4).controller, Some(P0));

    // UntilEndOfTurn control expires when Turn 2 begins.
    let turn2 = advance_to_next_turn_main(s3);
    assert_eq!(current(&turn2, b4).controller, Some(P1));
}

#[test]
fn mutate_object_interactive_prompt_selection() {
    let vocab = vocabulary();
    let spell = sorcery(
        vec![Instruction::MutateObject(Box::new(
            MutateObjectSpec::with_selection(
                ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::creature().controlled(),
                ),
                ZoneSelection::Prompt {
                    chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                    min: Box::new(ValueExpr::literal(1)),
                    max: Box::new(ValueExpr::literal(2)),
                    constraint: None,
                    category: ChoiceCategory::Generic,
                    hidden: false,
                },
                ObjectMutation::SetTapped(true),
            ),
        ))],
        &vocab,
    );

    let game = init_world(
        vocab.clone(),
        vec![
            in_hand(1, P0, spell),
            bf(2, P0, bear(2, 2, BTreeSet::new(), &vocab)),
            bf(3, P0, bear(2, 2, BTreeSet::new(), &vocab)),
            bf(4, P0, bear(2, 2, BTreeSet::new(), &vocab)),
        ],
    );
    let b2 = object(&game, 2);
    let b3 = object(&game, 3);
    let b4 = object(&game, 4);

    let paused = cast_untargeted(&game, 1);
    let decision = paused
        .state()
        .pending_decision()
        .expect("interactive selection prompt");
    assert_eq!(decision.chooser(), P0);
    match decision.frame() {
        RuleFrame::PromptChoice(frame) => match &frame.domain {
            ResolvedPromptDomain::Objects {
                candidates,
                min,
                max,
                hidden,
            } => {
                assert_eq!(candidates.len(), 3);
                assert_eq!(*min, 1);
                assert_eq!(*max, 2);
                assert!(!hidden);
            }
            other => panic!("expected Objects prompt domain, got {other:?}"),
        },
        other => panic!("expected PromptChoice frame, got {other:?}"),
    }

    // Choose only b2 and b4, leaving b3 untouched.
    let after = submit(
        paused.state(),
        P0,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::PromptChoice(PromptChoiceReply::Objects(vec![b2, b4])),
        },
    );
    let state = after.state();
    assert!(state.pending_decision().is_none());
    assert_eq!(
        state.execution().window().cards().get(b2).unwrap().tapped(),
        Some(true)
    );
    assert_eq!(
        state.execution().window().cards().get(b3).unwrap().tapped(),
        Some(false)
    );
    assert_eq!(
        state.execution().window().cards().get(b4).unwrap().tapped(),
        Some(true)
    );
}

#[test]
fn apply_continuous_dynamic_boost_suppress_keywords_and_double_pt() {
    let vocab = vocabulary();
    let vigilant_query = || {
        ObjectQuery::zone(
            QueryZone::Battlefield,
            ObjectFilter::creature()
                .controlled()
                .with_keyword(KeywordAbility::Vigilance),
        )
    };
    let boost_spell = sorcery(
        vec![Instruction::ApplyContinuous(Box::new(
            ApplyContinuousSpec::dynamic_boost(
                vigilant_query(),
                ValueExpr::QueryCount(Box::new(ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::creature().controlled(),
                ))),
                ValueExpr::literal(1),
                BTreeSet::from([KeywordAbility::Trample]),
                BTreeSet::from([KeywordAbility::Flying]),
                ResolvedDuration::UntilEndOfTurn,
            ),
        ))],
        &vocab,
    );
    let double_power_spell = sorcery(
        vec![Instruction::ApplyContinuous(Box::new(
            ApplyContinuousSpec::double_pt(
                vigilant_query(),
                true,
                false,
                ResolvedDuration::UntilEndOfTurn,
            ),
        ))],
        &vocab,
    );
    let double_both_other = sorcery(
        vec![Instruction::ApplyContinuous(Box::new(
            ApplyContinuousSpec::double_pt(
                ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::creature()
                        .controlled()
                        .without_keyword(KeywordAbility::Vigilance),
                ),
                true,
                true,
                ResolvedDuration::UntilEndOfTurn,
            ),
        ))],
        &vocab,
    );

    let game = init_world(
        vocab.clone(),
        vec![
            in_hand(10, P0, boost_spell),
            in_hand(11, P0, double_power_spell),
            in_hand(12, P0, double_both_other),
            bf(
                2,
                P0,
                bear(
                    2,
                    3,
                    BTreeSet::from([KeywordAbility::Flying, KeywordAbility::Vigilance]),
                    &vocab,
                ),
            ),
            bf(3, P0, bear(2, 2, BTreeSet::new(), &vocab)),
            bf(4, P0, bear(1, 3, BTreeSet::new(), &vocab)),
        ],
    );
    let b2 = object(&game, 2);
    let b3 = object(&game, 3);
    let b4 = object(&game, 4);

    // P0 controls 3 creatures -> b2 gets +3/+1 (5/4), gains Trample, loses Flying.
    let s1 = cast_untargeted(&game, 10).into_state();
    let c2 = current(&s1, b2);
    assert_eq!(c2.power_toughness.as_ref().unwrap().derived, pt(5, 4));
    assert!(c2.has_keyword(KeywordAbility::Trample));
    assert!(!c2.has_keyword(KeywordAbility::Flying));
    assert!(c2.has_keyword(KeywordAbility::Vigilance));

    // DoublePt(power: true, toughness: false) doubles b2 from 5/4 to 10/4.
    let s2 = cast_untargeted(&s1, 11).into_state();
    assert_eq!(
        current(&s2, b2).power_toughness.as_ref().unwrap().derived,
        pt(10, 4)
    );

    // DoublePt(power: true, toughness: true) doubles b3 (2/2 -> 4/4) and b4 (1/3 -> 2/6).
    let s3 = cast_untargeted(&s2, 12).into_state();
    assert_eq!(
        current(&s3, b3).power_toughness.as_ref().unwrap().derived,
        pt(4, 4)
    );
    assert_eq!(
        current(&s3, b4).power_toughness.as_ref().unwrap().derived,
        pt(2, 6)
    );
}

#[test]
fn apply_continuous_temporary_rule_and_combat_action() {
    let vocab = vocabulary();
    let silence_opponents = sorcery(
        vec![Instruction::ApplyContinuous(Box::new(
            ApplyContinuousSpec::temporary_rule(
                TemporaryRuleKind::CantCastSpells {
                    players: AffectedPlayers::EachOpponent,
                    filter: None,
                },
                ResolvedDuration::UntilEndOfTurn,
            ),
        ))],
        &vocab,
    );
    let force_attack = sorcery(
        vec![Instruction::ApplyContinuous(Box::new(
            ApplyContinuousSpec::combat_action(
                ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::creature().controlled(),
                ),
                CombatActionKind::MustAttack,
            ),
        ))],
        &vocab,
    );

    let game = init_world(
        vocab.clone(),
        vec![
            in_hand(10, P0, silence_opponents),
            in_hand(11, P0, force_attack),
            in_hand(20, P1, instant_noop(&vocab)),
            bf(2, P0, bear(2, 2, BTreeSet::new(), &vocab)),
        ],
    );
    let b2 = object(&game, 2);

    // P0 installs CantCastSpells(EachOpponent) and can still cast own spells.
    let s1 = cast_untargeted(&game, 10).into_state();
    let s2 = cast_untargeted(&s1, 11).into_state();

    // Pass priority to P1 in PrecombatMain: P1 cannot cast instant 20.
    let p1_prio = submit(&s2, P0, GameCommand::Pass).into_state();
    assert!(
        attempt(
            &p1_prio,
            P1,
            GameCommand::CastUntargeted {
                spell: object(&p1_prio, 20),
                payment: Some(ManaPayment::default()),
            },
        )
        .is_err(),
        "TemporaryRule(CantCastSpells) must reject opponent spell cast"
    );

    // P1 passes -> BeginningCombat, then pass pair -> DeclareAttackers.
    let beg_combat = submit(&p1_prio, P1, GameCommand::Pass).into_state();
    assert_eq!(
        beg_combat.turn().unwrap().position().step,
        Step::BeginningCombat
    );
    let dec_atk = pass_pair(&beg_combat).into_state();
    assert_eq!(
        dec_atk.turn().unwrap().position().step,
        Step::DeclareAttackers
    );
    let decision = dec_atk.pending_decision().expect("attackers decision");
    assert!(
        attempt(
            &dec_atk,
            P0,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::DeclareAttackers(vec![]),
            },
        )
        .is_err(),
        "CombatAction(MustAttack) must reject empty attacker declaration"
    );
    let declared = reply(
        &dec_atk,
        P0,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: b2,
            defender: P1,
            defending_planeswalker: None,
        }]),
    );
    assert!(declared.state().pending_decision().is_none());
}

#[test]
fn apply_continuous_play_permission_and_prevent_damage() {
    let vocab = vocabulary();
    let impulse_free_cast = sorcery(
        vec![
            Instruction::ZoneOp(Box::new(ZoneOpSpec {
                source: ObjectQuery::ZoneScan {
                    zone: QueryZone::Library(Box::new(PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ))),
                    filter: Box::new(ObjectFilter::any()),
                    slice: QuerySlice::Top(Box::new(ValueExpr::literal(1))),
                },
                selection: ZoneSelection::All,
                destination: ZoneDest::exile(),
                bind_moved: Some(BindingId(0)),
                bind_snapshots: None,
                bind_unchosen: None,
                bind_succeeded: None,
                tag_moved: None,
                shuffle_library: None,
            })),
            Instruction::ApplyContinuous(Box::new(ApplyContinuousSpec::play_permission(
                ObjectQuery::Bound(BindingId(0)),
                PlayerSelector::Resolving(ResolvingPlayer::Controller),
                ResolvedDuration::UntilEndOfTurn,
                true,
                true,
                false,
            ))),
        ],
        &vocab,
    );
    let shield_controller = sorcery(
        vec![Instruction::ApplyContinuous(Box::new(
            ApplyContinuousSpec::prevent_damage(TurnPreventionSpec {
                combat_only: false,
                source: PreventionSourceSpec::Any,
                recipient: PreventionRecipientSpec::Controller,
                both_directions: false,
            }),
        ))],
        &vocab,
    );
    let expensive_creature = validated(
        Program::creature(
            Some(ManaCost::generic(5.into())),
            4.into(),
            4.into(),
            BTreeSet::new(),
        ),
        &vocab,
    );

    let game = init_world(
        vocab.clone(),
        vec![
            in_hand(10, P0, impulse_free_cast),
            in_hand(11, P0, shield_controller),
            bf(3, P0, pinger_creature(&vocab)),
            in_library(20, P0, expensive_creature),
        ],
    );
    let pinger_id = object(&game, 3);

    // 1. Exile top card (5-mana 4/4) and cast it from exile for free via PlayPermission.
    let s1 = cast_untargeted(&game, 10).into_state();
    assert_eq!(
        s1.execution()
            .window()
            .cards()
            .get(object(&s1, 20))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    let cast_from_exile = submit(
        &s1,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&s1, 20),
            payment: Some(ManaPayment::default()),
        },
    );
    let s2 = pass_pair(cast_from_exile.state()).into_state();
    assert_eq!(
        s2.execution()
            .window()
            .cards()
            .get(object(&s2, 20))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );

    // 2. PreventDamage prevents damage to P0 (Controller) but not to P1.
    let s3 = cast_untargeted(&s2, 11).into_state();
    let ping_p0 = submit(
        &s3,
        P0,
        GameCommand::ActivateAbility {
            source: pinger_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(P0),
            payment: Some(ManaPayment::default()),
        },
    );
    let s4 = pass_pair(ping_p0.state()).into_state();
    assert_eq!(s4.execution().life_total(P0), Some(&RulesInteger::from(20)));

    let ping_p1 = submit(
        &s4,
        P0,
        GameCommand::ActivateAbility {
            source: pinger_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(P1),
            payment: Some(ManaPayment::default()),
        },
    );
    let s5 = pass_pair(ping_p1.state()).into_state();
    assert_eq!(s5.execution().life_total(P1), Some(&RulesInteger::from(17)));
}

#[test]
fn apply_continuous_skip_turns_and_skip_step() {
    let vocab = vocabulary();
    let skip_spell = sorcery(
        vec![
            Instruction::ApplyContinuous(Box::new(ApplyContinuousSpec::skip_turns(
                PlayerSelector::Group(AffectedPlayers::EachOpponent),
                ValueExpr::literal(1),
            ))),
            Instruction::ApplyContinuous(Box::new(ApplyContinuousSpec::skip_step(
                PlayerSelector::Resolving(ResolvingPlayer::Controller),
                Step::Draw,
            ))),
        ],
        &vocab,
    );

    let mut seeds = vec![in_hand(1, P0, skip_spell)];
    for i in 0..6 {
        seeds.push(in_library(100 + i, P0, bear(1, 1, BTreeSet::new(), &vocab)));
        seeds.push(in_library(200 + i, P1, bear(1, 1, BTreeSet::new(), &vocab)));
    }
    let game = init_world(vocab, seeds);

    // Turn 1 (P0): cast skip_spell (hand becomes 0).
    let s1 = cast_untargeted(&game, 1).into_state();
    assert_eq!(hand_count(&s1, P0), 0);

    // Turn 2: P1's turn was skipped (active player is P0 again) and P0 skipped Draw step.
    let turn2 = advance_to_next_turn_main(s1);
    assert_eq!(turn2.turn().unwrap().position().number, 2);
    assert_eq!(turn2.execution().window().active_player(), P0);
    assert_eq!(hand_count(&turn2, P0), 0);

    // Turn 3: skip exhausted, so P1 takes Turn 3 and draws 1 card normally.
    let turn3 = advance_to_next_turn_main(turn2);
    assert_eq!(turn3.turn().unwrap().position().number, 3);
    assert_eq!(turn3.execution().window().active_player(), P1);
    assert_eq!(hand_count(&turn3, P1), 1);

    // Turn 4: SkipStep was one-shot, so P0 draws normally on Turn 4.
    let turn4 = advance_to_next_turn_main(turn3);
    assert_eq!(turn4.turn().unwrap().position().number, 4);
    assert_eq!(turn4.execution().window().active_player(), P0);
    assert_eq!(hand_count(&turn4, P0), 1);
}

#[test]
fn adjust_life_gain_lose_set_pay_drain_half_and_bind() {
    let vocab = vocabulary();
    let spell = sorcery(
        vec![
            Instruction::AdjustLife(Box::new(AdjustLifeSpec {
                players: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                op: LifeOp::Gain(ValueExpr::literal(4)),
                bind_amount: Some(BindingId(1)),
            })),
            Instruction::AdjustLife(Box::new(AdjustLifeSpec::drain(
                PlayerSelector::Group(AffectedPlayers::EachOpponent),
                ValueExpr::literal(3),
                PlayerSelector::Resolving(ResolvingPlayer::Controller),
                true,
            ))),
            Instruction::AdjustLife(Box::new(AdjustLifeSpec::lose_half(
                PlayerSelector::Group(AffectedPlayers::EachOpponent),
                true,
            ))),
        ],
        &vocab,
    );
    let game = init_world(vocab, vec![in_hand(1, P0, spell)]);
    let after = cast_untargeted(&game, 1).into_state();
    // P0: 20 + 4 + 3 = 27
    assert_eq!(
        after.execution().life_total(P0),
        Some(&RulesInteger::from(27))
    );
    // P1: 20 - 3 = 17, then lose half rounded up (9) -> 8
    assert_eq!(
        after.execution().life_total(P1),
        Some(&RulesInteger::from(8))
    );
}

#[test]
fn deal_damage_sources_targets_and_exile_if_dies() {
    let vocab = vocabulary();
    let bolt = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::DealDamage(Box::new(DealDamageSpec {
                    source: DamageSourceQuery::EntrySource,
                    recipients: DamageTargetQuery::Targets,
                    amount: spellwake_engine::program::DamageAmountSpec::Expr(ValueExpr::literal(
                        3,
                    )),
                    exile_if_dies: true,
                }))],
            ),
        ),
        &vocab,
    );
    let target_bear = bear(2, 2, BTreeSet::new(), &vocab);
    let game = init_world(vocab, vec![in_hand(1, P0, bolt), bf(2, P1, target_bear)]);
    let bear_id = object(&game, 2);
    let cast = submit(
        &game,
        P0,
        GameCommand::Cast {
            spell: object(&game, 1),
            target: TargetRef::Object(bear_id),
        },
    );
    let after = pass_pair(cast.state()).into_state();
    // 2/2 took 3 lethal damage with exile_if_dies -> exiled instead of graveyard!
    assert_eq!(
        after
            .execution()
            .window()
            .cards()
            .get(object(&after, 2))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
}

#[test]
fn mutate_player_counters_monarch_extra_turn_and_emblem() {
    let vocab = vocabulary();
    let spell = sorcery(
        vec![
            Instruction::MutatePlayer(Box::new(MutatePlayerSpec::add_counter(
                PlayerSelector::Resolving(ResolvingPlayer::Controller),
                CounterKind::Energy,
                ValueExpr::literal(3),
            ))),
            Instruction::MutatePlayer(Box::new(MutatePlayerSpec::pay_counter(
                PlayerSelector::Resolving(ResolvingPlayer::Controller),
                CounterKind::Energy,
                ValueExpr::literal(1),
            ))),
            Instruction::MutatePlayer(Box::new(MutatePlayerSpec::become_monarch(
                PlayerSelector::Resolving(ResolvingPlayer::Controller),
            ))),
            Instruction::MutatePlayer(Box::new(MutatePlayerSpec::take_extra_turn(
                PlayerSelector::Resolving(ResolvingPlayer::Controller),
            ))),
            Instruction::ApplyContinuous(Box::new(ApplyContinuousSpec::emblem(
                PlayerSelector::Resolving(ResolvingPlayer::Controller),
                spellwake_engine::emblem::EmblemDefinition::PreventAllButOne,
            ))),
        ],
        &vocab,
    );
    let mut seeds = vec![in_hand(1, P0, spell)];
    for i in 0..4 {
        seeds.push(in_library(100 + i, P0, bear(1, 1, BTreeSet::new(), &vocab)));
        seeds.push(in_library(200 + i, P1, bear(1, 1, BTreeSet::new(), &vocab)));
    }
    let game = init_world(vocab, seeds);
    let s1 = cast_untargeted(&game, 1).into_state();
    assert_eq!(
        s1.execution()
            .player_counters_of_kind(P0, CounterKind::Energy),
        RulesInteger::from(2)
    );
    assert_eq!(s1.execution().monarch(), Some(P0));
    assert_eq!(s1.execution().emblems().count(), 1);
    let turn2 = advance_to_next_turn_main(s1);
    // P0 took the extra turn!
    assert_eq!(turn2.execution().window().active_player(), P0);
}

#[test]
fn phase5_custom_player_counters_and_exile_with_counters() {
    let vocab = vocabulary();
    let ticket = CounterKind::Custom(
        spellwake_engine::effects::CustomCounterKey::from_label("ticket").expect("valid key"),
    );
    let void_counter = CounterKind::Custom(
        spellwake_engine::effects::CustomCounterKey::from_label("void").expect("valid key"),
    );
    let mut exile_spec = ZoneOpSpec::move_all(
        ObjectQuery::Targets,
        ZoneDest::Exile {
            face_down: false,
            link_to_source: false,
            link_group: None,
            delayed_return: None,
            viewer: None,
        },
    );
    exile_spec.bind_moved = Some(BindingId(1));
    let spell = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    Instruction::MutatePlayer(Box::new(MutatePlayerSpec::add_counter(
                        PlayerSelector::Resolving(ResolvingPlayer::Controller),
                        ticket,
                        ValueExpr::literal(3),
                    ))),
                    Instruction::ZoneOp(Box::new(exile_spec)),
                    Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                        ObjectQuery::Bound(BindingId(1)),
                        ObjectMutation::AddCounters {
                            kind: void_counter,
                            amount: ValueExpr::literal(2),
                        },
                    ))),
                ],
            ),
        ),
        &vocab,
    );
    let victim = bear(2, 2, BTreeSet::new(), &vocab);
    let game = init_world(vocab, vec![in_hand(1, P0, spell), bf(2, P1, victim)]);
    let cast = submit(
        &game,
        P0,
        GameCommand::Cast {
            spell: object(&game, 1),
            target: TargetRef::Object(object(&game, 2)),
        },
    );
    let after = pass_pair(cast.state()).into_state();
    assert_eq!(
        after.execution().player_counters_of_kind(P0, ticket),
        RulesInteger::from(3)
    );
    let exiled_id = object(&after, 2);
    let exiled_card = after.execution().window().cards().get(exiled_id).unwrap();
    assert_eq!(exiled_card.zone(), ObjectZone::Exile);
    assert_eq!(
        after.execution().counters_of_kind(exiled_id, void_counter),
        RulesInteger::from(2)
    );
}

#[test]
fn phase5_prevent_damage_forbidden_subtypes_and_sticker_tag() {
    let vocab = vocabulary();
    let goblin = vocab.resolve_label("Goblin").expect("Goblin in vocabulary");
    let mut gob = bear(2, 2, BTreeSet::new(), &vocab);
    gob.creature_subtypes.insert(goblin);
    let non_gob = bear(2, 2, BTreeSet::new(), &vocab);

    let preventer = sorcery(
        vec![
            Instruction::ApplyContinuous(Box::new(ApplyContinuousSpec::prevent_damage(
                TurnPreventionSpec {
                    combat_only: false,
                    source: PreventionSourceSpec::Filter(
                        spellwake_engine::damage::PreventionSourceFilter {
                            creatures_only: true,
                            forbidden_subtypes: [Some(goblin), None],
                            ..spellwake_engine::damage::PreventionSourceFilter::default()
                        },
                    ),
                    recipient: PreventionRecipientSpec::Any,
                    both_directions: false,
                },
            ))),
            Instruction::SetObjectTag {
                query: Box::new(ObjectQuery::ZoneScan {
                    zone: QueryZone::Battlefield,
                    filter: Box::new(ObjectFilter::creature().controlled()),
                    slice: QuerySlice::All,
                }),
                key: TagKey::Label("sticker".into()),
                value: Some(TagValue::Text("name-sticker".into())),
            },
        ],
        &vocab,
    );
    let ping_from_gob = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::DealDamage(Box::new(DealDamageSpec {
                    source: DamageSourceQuery::Objects(ObjectQuery::Targets),
                    recipients: DamageTargetQuery::Players(PlayerSelector::Group(
                        AffectedPlayers::EachOpponent,
                    )),
                    amount: spellwake_engine::program::DamageAmountSpec::Expr(ValueExpr::literal(
                        2,
                    )),
                    exile_if_dies: false,
                }))],
            ),
        ),
        &vocab,
    );
    let ping_from_non_gob = ping_from_gob.clone();
    let game = init_world(
        vocab,
        vec![
            in_hand(1, P0, preventer),
            in_hand(2, P0, ping_from_gob),
            in_hand(3, P0, ping_from_non_gob),
            bf(4, P0, gob),
            bf(5, P0, non_gob),
        ],
    );
    let s1 = cast_untargeted(&game, 1).into_state();
    // Both controlled creatures got the generic "sticker" tag.
    let tagged = s1.execution().object_tags().objects_with_tag(
        &TagKey::Label("sticker".into()),
        Some(&TagValue::Text("name-sticker".into())),
    );
    assert!(tagged.contains(&object(&s1, 4)));
    assert!(tagged.contains(&object(&s1, 5)));
    // Damage from non-Goblin (card 5) is prevented; damage from Goblin (card 4) is NOT prevented!
    let c_non = submit(
        &s1,
        P0,
        GameCommand::Cast {
            spell: object(&s1, 3),
            target: TargetRef::Object(object(&s1, 5)),
        },
    );
    let s2 = pass_pair(c_non.state()).into_state();
    assert_eq!(s2.execution().life_total(P1), Some(&RulesInteger::from(20)));
    let c_gob = submit(
        &s2,
        P0,
        GameCommand::Cast {
            spell: object(&s2, 2),
            target: TargetRef::Object(object(&s2, 4)),
        },
    );
    let s3 = pass_pair(c_gob.state()).into_state();
    assert_eq!(s3.execution().life_total(P1), Some(&RulesInteger::from(18)));
}

fn advance_until_end_step_resolved(mut game: GameState) -> GameState {
    for _ in 0..40 {
        let pos = game.turn().unwrap().position();
        if pos.step == Step::End
            && game.execution().window().entries().is_empty()
            && game.pending_decision().is_none()
        {
            return game;
        }
        if let Some(pending) = game.pending_decision() {
            let chooser = pending.chooser();
            let choice = match pending.frame() {
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    DecisionReply::DeclareAttackers(vec![])
                }
                RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                    DecisionReply::DeclareBlockers(vec![])
                }
                other => panic!("unexpected decision while advancing to End step: {other:?}"),
            };
            game = reply(&game, chooser, choice).into_state();
        } else {
            let active = game.execution().window().active_player();
            let order = if active == P0 { [P0, P1] } else { [P1, P0] };
            game = pass_in_order(&game, order).into_state();
        }
    }
    panic!("did not reach resolved Step::End");
}

#[test]
fn stack_op_counter_targeted_graveyard_and_exile() {
    let vocab = vocabulary();
    let gain_life = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustLife(Box::new(AdjustLifeSpec::gain(
                    PlayerSelector::Resolving(ResolvingPlayer::Controller),
                    ValueExpr::literal(4),
                )))],
            ),
        ),
        &vocab,
    );
    let counter_gy = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                        spellwake_engine::toolbox::CounterDestination::Graveyard,
                    ),
                ))],
            ),
        ),
        &vocab,
    );
    let counter_exile = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                        spellwake_engine::toolbox::CounterDestination::Exile,
                    ),
                ))],
            ),
        ),
        &vocab,
    );

    let game = init_world(
        vocab,
        vec![
            in_hand(1, P0, gain_life.clone()),
            in_hand(2, P1, counter_gy),
            in_hand(3, P0, gain_life),
            in_hand(4, P1, counter_exile),
        ],
    );

    // 1. Counter targeted spell to graveyard.
    let cast1 = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 1),
            payment: Some(ManaPayment::default()),
        },
    );
    let entry1 = cast1.state().execution().window().entries()[0].id();
    let p0_pass1 = submit(cast1.state(), P0, GameCommand::Pass);
    let cast_c1 = submit(
        p0_pass1.state(),
        P1,
        GameCommand::Cast {
            spell: object(p0_pass1.state(), 2),
            target: TargetRef::Stack(entry1),
        },
    );
    let after1 = pass_in_order(cast_c1.state(), [P1, P0]).into_state();
    assert!(after1.execution().window().entries().is_empty());
    assert_eq!(
        after1
            .execution()
            .window()
            .cards()
            .get(object(&after1, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    assert_eq!(
        after1.execution().life_total(P0),
        Some(&RulesInteger::from(20))
    );

    // 2. Counter targeted spell to exile.
    let cast2 = submit(
        &after1,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&after1, 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let entry2 = cast2.state().execution().window().entries()[0].id();
    let p0_pass2 = submit(cast2.state(), P0, GameCommand::Pass);
    let cast_c2 = submit(
        p0_pass2.state(),
        P1,
        GameCommand::Cast {
            spell: object(p0_pass2.state(), 4),
            target: TargetRef::Stack(entry2),
        },
    );
    let after2 = pass_in_order(cast_c2.state(), [P1, P0]).into_state();
    assert!(after2.execution().window().entries().is_empty());
    assert_eq!(
        after2
            .execution()
            .window()
            .cards()
            .get(object(&after2, 3))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    assert_eq!(
        after2.execution().life_total(P0),
        Some(&RulesInteger::from(20))
    );
}

#[test]
fn stack_op_counter_matching_opponents_spells() {
    let vocab = vocabulary();
    let p0_spell = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustLife(Box::new(AdjustLifeSpec::gain(
                    PlayerSelector::Resolving(ResolvingPlayer::Controller),
                    ValueExpr::literal(4),
                )))],
            ),
        ),
        &vocab,
    );
    let p1_spell = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustLife(Box::new(AdjustLifeSpec::gain(
                    PlayerSelector::Resolving(ResolvingPlayer::Controller),
                    ValueExpr::literal(3),
                )))],
            ),
        ),
        &vocab,
    );
    let sweep_opponents = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::counter_matching(
                        None, true, false, true,
                    ),
                ))],
            ),
        ),
        &vocab,
    );

    let game = init_world(
        vocab,
        vec![
            in_hand(1, P0, p0_spell),
            in_hand(2, P1, p1_spell),
            in_hand(3, P0, sweep_opponents),
        ],
    );

    let c1 = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 1),
            payment: Some(ManaPayment::default()),
        },
    );
    let p0_pass = submit(c1.state(), P0, GameCommand::Pass);
    let c2 = submit(
        p0_pass.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(p0_pass.state(), 2),
            payment: Some(ManaPayment::default()),
        },
    );
    let p1_pass = submit(c2.state(), P1, GameCommand::Pass);
    let c3 = submit(
        p1_pass.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(p1_pass.state(), 3),
            payment: Some(ManaPayment::default()),
        },
    );
    // Resolve sweep (counters P1's spell, leaves P0's spell on the stack).
    let after_sweep = pass_pair(c3.state()).into_state();
    assert_eq!(
        after_sweep
            .execution()
            .window()
            .cards()
            .get(object(&after_sweep, 2))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P1)
    );
    assert_eq!(after_sweep.execution().window().entries().len(), 1);
    // Resolve P0's surviving spell.
    let done = pass_pair(&after_sweep).into_state();
    assert!(done.execution().window().entries().is_empty());
    assert_eq!(
        done.execution().life_total(P0),
        Some(&RulesInteger::from(24))
    );
    assert_eq!(
        done.execution().life_total(P1),
        Some(&RulesInteger::from(20))
    );
}

#[test]
fn stack_op_retarget_and_copy_spell() {
    let vocab = vocabulary();
    let burn = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![Instruction::DealDamage(Box::new(DealDamageSpec {
                    source: DamageSourceQuery::EntrySource,
                    recipients: DamageTargetQuery::Targets,
                    amount: spellwake_engine::program::DamageAmountSpec::Expr(ValueExpr::literal(
                        3,
                    )),
                    exile_if_dies: false,
                }))],
            ),
        ),
        &vocab,
    );
    let retarget = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::retarget(false),
                ))],
            ),
        ),
        &vocab,
    );
    let copy_spell = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                ))],
            ),
        ),
        &vocab,
    );

    let game = init_world(
        vocab,
        vec![
            in_hand(1, P0, burn.clone()),
            in_hand(2, P1, retarget),
            in_hand(3, P0, burn),
            in_hand(4, P0, copy_spell),
        ],
    );

    // 1. Retarget P0's burn spell from P1 onto P0.
    let c_burn1 = submit(
        &game,
        P0,
        GameCommand::Cast {
            spell: object(&game, 1),
            target: TargetRef::Player(P1),
        },
    );
    let burn1_entry = c_burn1.state().execution().window().entries()[0].id();
    let p0_pass = submit(c_burn1.state(), P0, GameCommand::Pass);
    let c_retarget = submit(
        p0_pass.state(),
        P1,
        GameCommand::Cast {
            spell: object(p0_pass.state(), 2),
            target: TargetRef::Stack(burn1_entry),
        },
    );
    let retarget_prompt = pass_in_order(c_retarget.state(), [P1, P0]).into_state();
    assert!(matches!(
        retarget_prompt.pending_decision().map(|d| d.frame()),
        Some(RuleFrame::CopyTargets(_))
    ));
    let retargeted = reply(
        &retarget_prompt,
        P1,
        DecisionReply::CopyTargets(spellwake_engine::effects::CopyTargetChoice::Change(
            TargetRef::Player(P0),
        )),
    )
    .into_state();
    let after_retarget = pass_pair(&retargeted).into_state();
    assert_eq!(
        after_retarget.execution().life_total(P0),
        Some(&RulesInteger::from(17))
    );
    assert_eq!(
        after_retarget.execution().life_total(P1),
        Some(&RulesInteger::from(20))
    );

    // 2. Copy P0's burn spell targeting P1, keeping target P1 for both copy and original.
    let c_burn2 = submit(
        &after_retarget,
        P0,
        GameCommand::Cast {
            spell: object(&after_retarget, 3),
            target: TargetRef::Player(P1),
        },
    );
    let burn2_entry = c_burn2.state().execution().window().entries()[0].id();
    let c_copy = submit(
        c_burn2.state(),
        P0,
        GameCommand::Cast {
            spell: object(c_burn2.state(), 4),
            target: TargetRef::Stack(burn2_entry),
        },
    );
    let copy_prompt = pass_pair(c_copy.state()).into_state();
    assert!(matches!(
        copy_prompt.pending_decision().map(|d| d.frame()),
        Some(RuleFrame::CopyTargets(_))
    ));
    let copied = reply(
        &copy_prompt,
        P0,
        DecisionReply::CopyTargets(spellwake_engine::effects::CopyTargetChoice::Retain),
    )
    .into_state();
    // Resolve the copy, then the original burn spell.
    let after_copy = pass_pair(&copied).into_state();
    let after_orig = pass_pair(&after_copy).into_state();
    assert!(after_orig.execution().window().entries().is_empty());
    assert_eq!(
        after_orig.execution().life_total(P1),
        Some(&RulesInteger::from(14))
    );
}

#[test]
fn schedule_delayed_step_trigger_with_created_tokens() {
    let vocab = vocabulary();
    let saproling = vocab
        .resolve_label("Saproling")
        .expect("Saproling in vocab");
    let token_def = spellwake_engine::token::TokenDefinition::new(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::from([saproling])),
        spellwake_engine::token::TokenName::Default {
            ordered_subtypes: vec![spellwake_engine::token::TokenSubtype::Creature(saproling)],
        },
        &vocab,
    )
    .expect("saproling token definition");

    for (use_exile, label) in [(true, "exile"), (false, "sacrifice")] {
        let delayed_dest = if use_exile {
            ZoneDest::exile()
        } else {
            ZoneDest::Graveyard {
                semantics: GraveyardSemantics::Sacrifice,
            }
        };
        let make_and_schedule = sorcery(
            vec![
                Instruction::CreateTokens {
                    creations: vec![spellwake_engine::token::TokenCreation {
                        definition: token_def.clone(),
                        amount: ValueExpr::literal(1),
                        base_size: None,
                        enters_tapped: false,
                        recipients: spellwake_engine::token::TokenRecipients::Controller,
                        attacking_defender: None,
                    }],
                },
                Instruction::ScheduleDelayedStepTrigger {
                    step: Step::End,
                    own_step_only: false,
                    body: Box::new(ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::ZoneOp(Box::new(ZoneOpSpec::move_all(
                            ObjectQuery::CreatedTokens,
                            delayed_dest,
                        )))],
                    )),
                },
            ],
            &vocab,
        );
        let bystander = bear(2, 2, BTreeSet::new(), &vocab);
        let game = init_world(
            vocab.clone(),
            vec![in_hand(1, P0, make_and_schedule), bf(2, P0, bystander)],
        );
        let after_cast = cast_untargeted(&game, 1).into_state();
        let bf_before: usize = after_cast
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|c| c.zone() == ObjectZone::Battlefield)
            .count();
        assert_eq!(
            bf_before, 2,
            "{label}: bear and created token must both be on battlefield"
        );

        let at_end = advance_until_end_step_resolved(after_cast);
        let bf_after: usize = at_end
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|c| c.zone() == ObjectZone::Battlefield)
            .count();
        assert_eq!(
            bf_after, 1,
            "{label}: delayed End-step trigger must remove only the created token"
        );
        assert_eq!(
            at_end
                .execution()
                .window()
                .cards()
                .get(object(&at_end, 2))
                .unwrap()
                .zone(),
            ObjectZone::Battlefield,
            "{label}: non-token bear must stay on battlefield"
        );
    }
}

#[test]
fn phase9_mutate_player_mill_and_add_mana_compose() {
    let vocab = vocabulary();
    let mut green_bundle = std::collections::BTreeMap::new();
    green_bundle.insert(spellwake_engine::mana::ManaType::Green, 2.into());
    let spell = sorcery(
        vec![
            Instruction::MutatePlayer(Box::new(MutatePlayerSpec::mill(
                ResolvingPlayer::Controller,
                ValueExpr::literal(1),
            ))),
            Instruction::MutatePlayer(Box::new(MutatePlayerSpec::add_mana(
                ResolvingPlayer::Controller,
                spellwake_engine::program::EffectManaYield::Fixed(green_bundle),
            ))),
        ],
        &vocab,
    );
    let filler = bear(1, 1, BTreeSet::new(), &vocab);
    let game = init_world(
        vocab,
        vec![
            in_hand(1, P0, spell),
            in_library(2, P0, filler.clone()),
            in_library(3, P0, filler.clone()),
            in_library(4, P0, filler),
        ],
    );
    let after = cast_untargeted(&game, 1).into_state();
    let gy_count = after
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Graveyard(P0))
        .count();
    assert_eq!(gy_count, 2, "spell plus 1 milled card must be in graveyard");
    assert_eq!(
        after
            .execution()
            .mana(P0, spellwake_engine::mana::ManaType::Green),
        RulesInteger::from(2),
        "controller must have 2 green mana from MutatePlayer::AddMana"
    );
}

#[test]
fn phase9_apply_continuous_damage_multiplier_doubles_from_power_damage() {
    let vocab = vocabulary();
    let doubler = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::ApplyContinuous(Box::new(
                    ApplyContinuousSpec::damage_multiplier(2.into()),
                ))],
            ),
        ),
        &vocab,
    );
    let mut hitter = bear(2, 2, BTreeSet::new(), &vocab);
    hitter.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::DealDamage(Box::new(
                DealDamageSpec::from_power(
                    spellwake_engine::program::PowerDamageSource::AbilitySource,
                ),
            ))],
        ),
    });
    let hitter = validated(hitter, &vocab);
    let big = bear(4, 4, BTreeSet::new(), &vocab);
    let game = init_world(
        vocab,
        vec![in_hand(1, P0, doubler), bf(2, P0, hitter), bf(3, P1, big)],
    );
    let target = TargetRef::Object(object(&game, 3));
    let after_doubler = pass_pair(
        submit(
            &game,
            P0,
            GameCommand::Cast {
                spell: object(&game, 1),
                target,
            },
        )
        .state(),
    )
    .into_state();
    let after_hit = pass_pair(
        submit(
            &after_doubler,
            P0,
            GameCommand::ActivateAbility {
                source: object(&after_doubler, 2),
                ability: ActivatedAbilityId(1),
                target,
                payment: Some(ManaPayment::default()),
            },
        )
        .state(),
    )
    .into_state();
    assert_eq!(
        after_hit
            .execution()
            .window()
            .cards()
            .get(object(&after_hit, 3))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P1),
        "2 power damage doubled by ApplyContinuous(DamageMultiplier(2)) must be lethal to a 4/4"
    );
}

#[test]
fn phase9_mutate_object_solve_case_and_advance_class_level() {
    let vocab = vocabulary();
    let ability = ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::MutateObject(Box::new(MutateObjectSpec::solve_case())),
                Instruction::MutateObject(Box::new(MutateObjectSpec::advance_class_level(2))),
            ],
        ),
    };
    let perm = validated(Program::artifact(None, vec![ability]), &vocab);
    let game = init_world(vocab, vec![bf(1, P0, perm)]);
    let source = object(&game, 1);
    let activated = submit(
        &game,
        P0,
        GameCommand::ActivateUntargetedAbility {
            source,
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let resolved = pass_pair(activated.state());
    let occs = all_occurrences(&resolved);
    assert!(
        occs.iter()
            .any(|occ| matches!(occ, RulesOccurrence::CaseSolved { object } if *object == source)),
        "MutateObject::SolveCase must emit CaseSolved"
    );
    assert!(
        occs.iter().any(|occ| matches!(
            occ,
            RulesOccurrence::ClassLevelReached { object, level: 2 } if *object == source
        )),
        "MutateObject::AdvanceClassLevel(2) must emit ClassLevelReached(2)"
    );
}
