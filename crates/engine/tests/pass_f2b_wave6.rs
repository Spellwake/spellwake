#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
//! Runtime integration tests for Pass F2-B Wave 6 primitives:
//! - Partial `RedirectNextDamage` (split + consume without looping, CR 614.5/614.9) and
//!   dynamic `RedirectNextDamageExpr` (creature power and `ChosenX`)
//! - Targeted damage prevention and redirection: `PreventNextTimeDamage { source_is_target: true }`,
//!   `StaticDamageRecipient::{TargetObject, TargetAny}`, and
//!   `DamageRedirectTarget::{TargetAny, SecondTargetObject}`
//! - Generalized `CreateTokenCopyExt`: `extra_keywords`, `extra_subtypes`, `extra_types`,
//!   and `TokenRecipients::EachOpponent`
//! - `SpellModes.conditional_maximum`
//! - `Instruction::BindResolutionX` feeding downstream `ValueExpr::chosen_x()` and
//!   `Instruction::EmitReflexiveTrigger`
//! - `Program.conditional_flash` (`SelfFlashPermission::Condition` and `PayExtraMana`)
//! - `Program.cannot_be_copied` blocking `Instruction::StackOp` (`StackOpSpec::copy_stack`)
//! - `TemporaryRuleKind::TemporaryZoneReplacement` with cleanup expiration
//! - `AbilityChange.suppressed_activations` (`NonMana` vs `All`) and `AbilityChange.doesnt_untap`
//! - `TemporaryRuleKind::{CantActivateAbilities, CantPlayLands, PlayerGrantKeyword}`
//! - `next_only: true` on `CantBeCountered`, `FilteredFlash`, and `NextSpellFree`
//! - `StaticRestriction::{ControllerLifeTotalCantChange, PlayersCantSearchLibraries}`
//! - `Instruction::SetDayNight`, `CombatActionKind::BecomeBlocked`,
//!   `BlockingRestriction::MustBlockAllAttackers`, `Instruction::PutResolvingSpellInLibrary`,
//!   `Instruction::PutSubjectIntoLibraryFromTop`, `SearchCount::UpToExpr`,
//!   `ValueObjectRef::LastAmassedArmy`, `EventValueKind::AdditionalCostPaid`,
//!   `ExtraActivationCost::PayEnergyExpr(ChosenX)`, and
//!   `HandDiscardKeyword::Channel` with `PermanentTargets`.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    actions::TimingError,
    activated::{
        ActivatedAbility, ActivatedAbilityId, ActivationOrigin, ExtraActivationCost,
        HandDiscardKeyword, LifeCostAmount, SourceCost,
    },
    characteristics::{
        AbilityChange, ActivationSuppression, BlockingRestriction, Color, ControllerSelector,
        DamageRedirectTarget, KeywordAbility, PermanentSelector, ResolvedDuration,
        ResolvedPartSpec, ResolvedPartsSpec, StaticAbilityId, StaticContinuousAbility,
        StaticDamageRecipient, StaticPart, StaticParts, StaticRestriction,
    },
    combat::{AttackDeclaration, BlockDeclaration, BlockStatus, CombatDecision, CombatError},
    day_night::DayNight,
    effects::{CounterKind, EffectError, LibraryTopReply, SeedCard},
    game::{
        DecisionReply, GameCommand, GameError, GameInput, GameState, NativeCandidate, RuleFrame,
    },
    land::LandType,
    mana::{
        CastMode, CastingPayment, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaCost, ManaOutput,
        ManaPayment, ManaSpend, ManaType,
    },
    object_identity::{CardId, ObjectDestination, ObjectZone},
    priority::SeatId,
    program::{
        AdditionalCastCost, AffectedPlayers, BodyTarget, CardType, CombatActionKind, Comparison,
        DamageAmountSpec, DamageRecipientsRef, DamageSourceRef, EffectSubject, EventValueKind,
        ExecutableBody, Instruction, LibraryPlacement, LibrarySearch, ModalUniqueness,
        MoveSubjectRef, ObjectFilter, ObjectStatKind, PlayerRelation, Program, ProgramKind,
        PublicZoneDestination, ResolutionCondition, ResolvingPlayer, SearchCardMatcher,
        SearchCount, SearchDestination, SearchSource, SelfFlashPermission, SpellFilter, SpellModes,
        StackCopyTarget, TemporaryRuleKind, ValueExpr, ValueObjectRef, ZoneActionPredicate,
        ZoneObjectKind, ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
        ZoneReplacementSubject,
    },
    targets::TargetRef,
    token::TokenRecipients,
    turn::{Step, TurnError},
};
use std::collections::{BTreeMap, BTreeSet};

fn validated(program: Program, vocab: &CreatureTypeVocabulary) -> Program {
    program
        .validate(vocab)
        .expect("constructed Wave 6 test program must validate");
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

fn cast_targeted(game: &GameState, actor: SeatId, id: u64, target: TargetRef) -> NativeCandidate {
    let cast = submit(
        game,
        actor,
        GameCommand::CastPaid {
            spell: object(game, id),
            target,
            payment: ManaPayment::default(),
        },
    );
    let order = if actor == P0 { [P0, P1] } else { [P1, P0] };
    pass_in_order(cast.state(), order)
}

fn opp_battlefield(id: u64, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: P1,
        destination: ObjectDestination::Battlefield { controller: P1 },
        program,
    }
}

fn opp_hand(id: u64, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: P1,
        destination: ObjectDestination::Hand,
        program,
    }
}

fn library_top(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::LibraryTop,
        program,
    }
}

fn gain_life_instant(amount: i64) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![spellwake_engine::program::Instruction::AdjustLife(
                Box::new(spellwake_engine::toolbox::AdjustLifeSpec::gain(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    ValueExpr::literal(amount),
                )),
            )],
        ),
    )
}

fn damage_any_instant(amount: i64) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SINGLE_ANY_TARGET,
            vec![spellwake_engine::program::Instruction::DealDamage(
                Box::new(spellwake_engine::toolbox::DealDamageSpec {
                    source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                    recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    amount: spellwake_engine::program::DamageAmountSpec::Expr(
                        spellwake_engine::program::ValueExpr::Literal(amount.into()),
                    ),
                    exile_if_dies: false,
                }),
            )],
        ),
    )
}

fn colorless_rock(amount: i64) -> Program {
    let mut rock = Program::artifact(None, vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, amount.into())])),
    });
    rock
}

fn colorless_payment(amount: i64) -> ManaPayment {
    ManaPayment {
        from_lots: BTreeMap::new(),
        spend: vec![ManaSpend {
            mana_type: ManaType::Colourless,
            amount: amount.into(),
        }],
    }
}

fn basic_forest() -> Program {
    Program::land(BTreeSet::from([LandType::Forest]), vec![])
}

fn advance_to_p1_upkeep_p0_priority(game: &GameState) -> NativeCandidate {
    let end = reach_end(game, [P0, P1]);
    let t2_upkeep = pass_pair(&end);
    assert_eq!(t2_upkeep.state().turn().unwrap().position().number, 2);
    assert_eq!(
        t2_upkeep.state().turn().unwrap().position().step,
        Step::Upkeep
    );
    submit(t2_upkeep.state(), P1, GameCommand::Pass)
}

#[test]
fn create_token_copy_grants_keywords_subtypes_card_types_and_each_opponent() {
    let vocab = vocabulary();
    let elf = vocab.resolve_label("Elf").unwrap();
    let zombie = vocab.resolve_label("Zombie").unwrap();
    let wizard = vocab.resolve_label("Wizard").unwrap();

    let original = validated(
        Program::creature(None, 2.into(), 2.into(), BTreeSet::from([elf])),
        &vocab,
    );
    let clone_for_opponents = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_CREATURE,
                vec![Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::create_token_copy(
                        spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                            MoveSubjectRef::TargetObject,
                        ),
                        Box::new(spellwake_engine::toolbox::TokenCopySpec {
                            count: ValueExpr::literal(1),
                            recipients: TokenRecipients::EachOpponent,
                            enters_tapped: false,
                            enters_attacking: false,
                            haste: false,
                            sacrifice_at_end_step: false,
                            exile_at_end_step: false,
                            nonlegendary: true,
                            extra_types: BTreeSet::from([CardType::Artifact]),
                            extra_subtypes: BTreeSet::from([zombie, wizard]),
                            extra_keywords: BTreeSet::from([
                                KeywordAbility::Flying,
                                KeywordAbility::Haste,
                            ]),
                            override_pt: None,
                            extra_colors: BTreeSet::new(),
                            override_colors: None,
                            strip_mana_cost: false,
                            defender_scope:
                                spellwake_engine::token::AttackingDefenderScope::AnyLegal,
                            extra_activated: Vec::new(),
                            extra_triggered: Vec::new(),
                            extra_static: Vec::new(),
                            extra_mana: Vec::new(),
                            extra_counters: Vec::new(),
                            keep_name: false,
                        }),
                    ),
                ))],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab,
        vec![card(1, original, true), card(2, clone_for_opponents, false)],
        vec![CardId(1)],
    );
    let orig_id = object(&game, 1);

    let resolved = cast_targeted(&game, P0, 2, TargetRef::Object(orig_id));
    let tokens: Vec<_> = resolved
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|obj| obj.card_id().is_none() && obj.zone() == ObjectZone::Battlefield)
        .collect();
    assert_eq!(
        tokens.len(),
        1,
        "EachOpponent in 2-player game must create exactly 1 token copy"
    );
    let token_id = tokens[0].id();
    assert_eq!(
        tokens[0].controller(),
        Some(P1),
        "TokenRecipients::EachOpponent must create the token copy under P1's control"
    );

    let view = current(resolved.state(), token_id);
    assert_eq!(
        view.current_types,
        BTreeSet::from([CardType::Creature, CardType::Artifact])
    );
    assert_eq!(
        view.current_creature_subtypes,
        BTreeSet::from([elf, zombie, wizard])
    );
    assert!(view.keywords.contains(&KeywordAbility::Flying));
    assert!(view.keywords.contains(&KeywordAbility::Haste));
    assert_eq!(view.power_toughness.unwrap().derived, pt(2, 2));
}

#[test]
fn cannot_be_copied_prevents_copy_stack_object() {
    let vocab = vocabulary();

    let mut uncopyable = gain_life_instant(4);
    uncopyable.cannot_be_copied = true;
    let uncopyable = validated(uncopyable, &vocab);

    let copyable = validated(gain_life_instant(4), &vocab);
    let copier = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::single_spell(SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::copy_stack(
                        StackCopyTarget::TargetSpell,
                        ValueExpr::literal(1),
                        false,
                    ),
                ))],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab,
        vec![
            card(1, uncopyable, false),
            card(2, copier.clone(), false),
            card(3, copyable, false),
            card(4, copier, false),
        ],
        vec![],
    );

    // Casting StackOp::copy_stack targeting a cannot_be_copied spell creates NO copy on the stack.
    let cast_a = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 1),
            payment: Some(ManaPayment::default()),
        },
    );
    let entry_a = cast_a.state().execution().window().entries()[0].id();
    let cast_copy_a = submit(
        cast_a.state(),
        P0,
        GameCommand::CastPaid {
            spell: object(cast_a.state(), 2),
            target: TargetRef::Stack(entry_a),
            payment: ManaPayment::default(),
        },
    );
    let after_copy_a = pass_pair(cast_copy_a.state());
    assert_eq!(
        after_copy_a.state().execution().window().entries().len(),
        1,
        "cannot_be_copied spell must not generate a stack copy"
    );
    let after_a = pass_pair(after_copy_a.state());
    assert_eq!(after_a.state().execution().life_total(P0), Some(&24.into()));

    // Control: StackOp::copy_stack on a normal spell creates a copy and resolves twice (+8 life).
    let cast_b = submit(
        after_a.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(after_a.state(), 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let entry_b = cast_b.state().execution().window().entries()[0].id();
    let cast_copy_b = submit(
        cast_b.state(),
        P0,
        GameCommand::CastPaid {
            spell: object(cast_b.state(), 4),
            target: TargetRef::Stack(entry_b),
            payment: ManaPayment::default(),
        },
    );
    let after_copy_b = pass_pair(cast_copy_b.state());
    assert_eq!(
        after_copy_b.state().execution().window().entries().len(),
        2,
        "normal spell must gain a stack copy"
    );
    let after_copy_res = pass_pair(after_copy_b.state());
    let after_orig_res = pass_pair(after_copy_res.state());
    assert_eq!(
        after_orig_res.state().execution().life_total(P0),
        Some(&32.into())
    );
}

#[test]
fn search_count_up_to_expr_enforces_dynamic_limit() {
    let vocab = vocabulary();

    let relic = || validated(Program::artifact(None, vec![]), &vocab);
    let bear = || {
        validated(
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            &vocab,
        )
    };
    let dynamic_tutor = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SearchLibrary {
                    filter: Box::new(LibrarySearch {
                        searcher: ResolvingPlayer::Controller,
                        source: SearchSource::OwnLibrary,
                        matcher: SearchCardMatcher::CardTypes(BTreeSet::from([CardType::Creature])),
                        mana_value: None,
                        count: SearchCount::UpToExpr(Box::new(ValueExpr::controlled_type(
                            CardType::Artifact,
                        ))),
                        pair_constraint: None,
                        destination: SearchDestination::Hand,
                        reveal: false,
                    }),
                }],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab.clone(),
        vec![
            card(1, relic(), true),
            card(2, relic(), true),
            card(3, dynamic_tutor, false),
            library_top(10, P0, bear()),
            library_top(11, P0, bear()),
            library_top(12, P0, bear()),
        ],
        vec![CardId(1), CardId(2)],
    );

    let pending = cast_untargeted(&game, 3);
    let decision = pending
        .state()
        .pending_decision()
        .expect("dynamic SearchLibrary must emit a LibraryTop decision");
    let RuleFrame::LibraryTop(frame) = decision.frame() else {
        panic!("expected LibraryTop frame");
    };
    assert!(frame.is_bounded_batch_search());
    assert_eq!(
        frame.search_batch_limit(),
        2,
        "UpToExpr must evaluate dynamically to controlled artifact count (2)"
    );
    assert_eq!(frame.eligible_results().len(), 3);

    let eligible = frame.eligible_results().to_vec();

    // Selecting 3 cards exceeds dynamic limit 2 and is rejected.
    assert_eq!(
        attempt(
            pending.state(),
            P0,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::LibraryTop(LibraryTopReply::SearchWithEntryColors {
                    selected: eligible.clone(),
                    colors: vec![],
                }),
            },
        ),
        Err(GameError::Effect(EffectError::InvalidLibraryPartition))
    );

    // Selecting 2 cards succeeds and moves both to P0's hand.
    let done = reply(
        pending.state(),
        P0,
        DecisionReply::LibraryTop(LibraryTopReply::SearchWithEntryColors {
            selected: eligible[..2].to_vec(),
            colors: vec![],
        }),
    );
    assert!(done.state().pending_decision().is_none());
    let hand_count = done
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(P0))
        .count();
    assert_eq!(hand_count, 2);
    assert_eq!(
        done.state().execution().window().cards().library(P0).len(),
        1
    );
}
