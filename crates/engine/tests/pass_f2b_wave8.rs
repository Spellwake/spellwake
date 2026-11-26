#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
//! Runtime and validation integration tests for Pass F2-B Wave 8A engine additions:
//! - Triggered `FixedTargetClause` (non-modal fight/destroy, target validation, and fizzle)
//! - Triggered `FixedTargetClause` inside `TriggeredModes` (modal choice, targeting, and
//!   prefix/suffix rejection)
//! - `SourceRelation::SharingNameWithAbilitySource` across trigger matching (`occurrence/matching`)
//!   and battlefield/target filtering (`effects`)
//! - `RulesOccurrence::Shuffled` firing `PlayerTriggerAction::ShuffledLibrary` from library search
//!   and graveyard-to-library shuffles
//! - `Instruction::DealDamageFrom` with `ExecutableBody::with_up_to_one_target()` (0 and 1 targets)
//! - `CardType::Planeswalker` admitted in `validate_battlefield_types` (static and resolved type
//!   operations, while keeping `Instant`/`Sorcery` rejected)
//! - Cast-time `SourceStateExpr` (`ManaSpentToCast`, `CastFromGraveyard`, `CastFromExile`,
//!   `Bargained`) on `BodyContext::Unbound` resolving spells (`StackKind::CardSpell`)
//! - `ResolvingPlayer::TargetSpellController` on non-spell stack entries (activated and triggered
//!   abilities on the stack)
//! - Non-controller `SourceCost::Counter` and `SourceCost::Exile` activations under
//!   `ActivationRestriction::OnlyOpponents` and `ActivationRestriction::AnyPlayer`
//! - Newly admitted `OptionalEffect` / `OptionalSequence` primitives (`UntapUpTo`,
//!   `DoubleCountersOnTargets`, `RegenerateSource`, and internal `OptionalSequence` jump bounds)
//! - `TriggerZone::Any` on spell programs and `Instruction::AffectPermanentTargets` companion
//!   follow-up validation and execution.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::{
    creature_types::CreatureTypeVocabulary,
    names::{EnglishCardName, ObjectNames},
};
use spellwake_engine::{
    activated::{
        ActivatedAbility, ActivatedAbilityId, ActivationRestriction, CounterCostDelta, SourceCost,
    },
    characteristics::{
        ControllerSelector, PermanentSelector, ResolvedDuration, ResolvedPartSpec,
        ResolvedPartsSpec, StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
        StaticTypeChange, SubtypeOperation, TypeOperation,
    },
    effects::{CounterKind, EffectError, LibraryTopReply, OptionalEffectChoice, SeedCard},
    game::{
        DecisionReply, GameCommand, GameError, GameInput, GameState, NativeCandidate, RuleFrame,
    },
    land::LandType,
    mana::{
        CastMode, CastingPayment, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaCost, ManaOutput,
        ManaPayment, ManaSpend, ManaType,
    },
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    occurrence::{EntryCompletion, RulesOccurrence},
    priority::SeatId,
    program::{
        AffectedPlayers, BodyTarget, CardType, Comparison, DamageAmountSpec, DamageRecipientsRef,
        DamageSourceRef, DefinitionError, EffectSubject, EntryCounterAbility, ExecutableBody,
        GraveyardSelfCast, Instruction, LibrarySearch, ModalUniqueness, ObjectFilter,
        PermanentTargetAction, PlayerRelation, PlayerStatExpr, PlayerTriggerAction, Program,
        ResolutionCondition, ResolvingPlayer, SearchCardMatcher, SourceStateExpr, SpellFilter,
        Supertype, TriggerGrouping, TriggerSpec, TriggerZone, TriggeredAbility, TriggeredAbilityId,
        TriggeredModes, ValueExpr,
    },
    targets::TargetRef,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU8,
};

fn validated(program: Program, vocab: &CreatureTypeVocabulary) -> Program {
    program
        .validate(vocab)
        .expect("constructed Wave 8A test program must validate");
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

fn graveyard_of(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Graveyard,
        program,
    }
}

fn exile_of(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Exile,
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

fn named(mut program: Program, name: &str) -> Program {
    program.names = ObjectNames::single(EnglishCardName::new(name.into()).unwrap());
    program
}

fn vanilla_creature(power: i64, toughness: i64) -> Program {
    Program::creature(
        Some(ManaCost::free()),
        power.into(),
        toughness.into(),
        BTreeSet::new(),
    )
}

fn gain_life_instant(amount: i64) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(amount),
            }],
        ),
    )
}

fn destroy_permanent_instant() -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::DestroyTarget],
        ),
    )
}

fn basic_forest() -> Program {
    let mut land = Program::land(
        BTreeSet::from([LandType::Forest]),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
        }],
    );
    land.supertypes.insert(Supertype::Basic);
    land
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

fn zone_of(game: &GameState, id: u64) -> ObjectZone {
    let obj = object(game, id);
    game.execution()
        .window()
        .cards()
        .get(obj)
        .expect("seeded card must exist")
        .zone()
}

fn is_tapped(game: &GameState, id: ObjectId) -> Option<bool> {
    game.execution()
        .window()
        .cards()
        .get(id)
        .and_then(|obj| obj.tapped())
}

// ---------------------------------------------------------------------------
// 1. Triggered FixedTargetClause (non-modal): fight and destroy + fizzle
// ---------------------------------------------------------------------------

#[test]
fn triggered_fixed_target_clause_fight_resolves_and_handles_removed_target() {
    let vocab = vocabulary();

    let mut ragebeast = vanilla_creature(4, 4);
    ragebeast.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::self_enters(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                    BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled()),
                    vec![Instruction::RememberPermanentTarget],
                ))),
                Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                    BodyTarget::FilteredPermanent(ObjectFilter::creature().not_controlled()),
                    vec![Instruction::RememberPermanentTarget],
                ))),
                Instruction::FightFixedTargets,
            ],
        ),
    });
    let ragebeast = validated(ragebeast, &vocab);
    let destroy = validated(destroy_permanent_instant(), &vocab);

    let game = world(
        vocab.clone(),
        vec![
            card(1, ragebeast.clone(), false),
            opp_battlefield(2, validated(vanilla_creature(2, 2), &vocab)),
            opp_battlefield(3, validated(vanilla_creature(5, 5), &vocab)),
            card(4, destroy, false),
            card(5, ragebeast, false),
        ],
        vec![CardId(2), CardId(3)],
    );

    let opp_bear = object(&game, 2);
    let opp_titan = object(&game, 3);

    // Part A: Cast ragebeast #1; ETB trigger prompts for two fixed-clause targets.
    let entered = cast_untargeted(&game, 1);
    let beast_1 = object(entered.state(), 1);
    let decision = entered
        .state()
        .pending_decision()
        .expect("ETB FixedTargetClause trigger must prompt for targets");
    assert_eq!(decision.chooser(), P0);
    let RuleFrame::TriggerTarget(frame) = decision.frame() else {
        panic!("expected TriggerTarget frame, got {:?}", decision.frame());
    };
    assert_eq!(frame.permanent_target_bounds(), Some((2, 2)));

    // Swapped target order violates clause 0 (controlled) / clause 1 (not_controlled).
    assert_eq!(
        attempt(
            entered.state(),
            P0,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::TriggerPermanentTargets(vec![
                    TargetRef::Object(opp_bear),
                    TargetRef::Object(beast_1),
                ]),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget))
    );

    // Duplicate targets are rejected for FightFixedTargets.
    assert_eq!(
        attempt(
            entered.state(),
            P0,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::TriggerPermanentTargets(vec![
                    TargetRef::Object(beast_1),
                    TargetRef::Object(beast_1),
                ]),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget))
    );

    // Legal target order places the trigger on the stack; resolving executes the fight.
    let placed = reply(
        entered.state(),
        P0,
        DecisionReply::TriggerPermanentTargets(vec![
            TargetRef::Object(beast_1),
            TargetRef::Object(opp_bear),
        ]),
    );
    let fought = pass_pair(placed.state());
    assert_eq!(
        zone_of(fought.state(), 2),
        ObjectZone::Graveyard(P1),
        "2/2 opposing bear must die from fighting 4/4 ragebeast"
    );
    assert_eq!(zone_of(fought.state(), 1), ObjectZone::Battlefield);
    assert_eq!(
        fought
            .state()
            .execution()
            .creature(beast_1)
            .unwrap()
            .unwrap()
            .damage,
        2.into(),
        "4/4 ragebeast must take 2 damage from fighting the 2/2 bear"
    );

    // Part B: Cast ragebeast #2 targeting itself and opp_titan (5/5), then destroy opp_titan
    // in response so the fight does not happen and ragebeast #2 survives unharmed.
    let entered_2 = cast_untargeted(fought.state(), 5);
    let beast_2 = object(entered_2.state(), 5);
    let placed_2 = reply(
        entered_2.state(),
        P0,
        DecisionReply::TriggerPermanentTargets(vec![
            TargetRef::Object(beast_2),
            TargetRef::Object(opp_titan),
        ]),
    );
    let after_destroy = cast_targeted(placed_2.state(), P0, 4, TargetRef::Object(opp_titan));
    assert_eq!(zone_of(after_destroy.state(), 3), ObjectZone::Graveyard(P1));

    let after_trigger = pass_pair(after_destroy.state());
    assert_eq!(zone_of(after_trigger.state(), 5), ObjectZone::Battlefield);
    assert_eq!(
        after_trigger
            .state()
            .execution()
            .creature(beast_2)
            .unwrap()
            .unwrap()
            .damage,
        0.into(),
        "fight must deal 0 damage when one fighter leaves before resolution"
    );
}

#[test]
fn triggered_fixed_target_clause_destroy_resolves_and_fizzles_when_all_targets_leave() {
    let vocab = vocabulary();

    let mut demolisher = Program::enchantment(Some(ManaCost::free()));
    demolisher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::self_enters(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                    BodyTarget::FilteredPermanent(ObjectFilter::artifact()),
                    vec![Instruction::DestroyTarget],
                ))),
                Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                    BodyTarget::FilteredPermanent(ObjectFilter::creature().not_controlled()),
                    vec![Instruction::DestroyTarget],
                ))),
                Instruction::DestroyFixedTargets,
            ],
        ),
    });
    let demolisher = validated(demolisher, &vocab);
    let destroy = validated(destroy_permanent_instant(), &vocab);

    let game = world(
        vocab.clone(),
        vec![
            card(1, demolisher.clone(), false),
            opp_battlefield(2, validated(Program::artifact(None, vec![]), &vocab)),
            opp_battlefield(3, validated(vanilla_creature(2, 2), &vocab)),
            card(4, demolisher, false),
            opp_battlefield(5, validated(Program::artifact(None, vec![]), &vocab)),
            opp_battlefield(6, validated(vanilla_creature(3, 3), &vocab)),
            card(7, destroy.clone(), false),
            card(8, destroy, false),
        ],
        vec![CardId(2), CardId(3), CardId(5), CardId(6)],
    );

    // Subcase A: Legal targets are selected and both are destroyed on resolution.
    let entered_a = cast_untargeted(&game, 1);
    let art_1 = object(entered_a.state(), 2);
    let creat_1 = object(entered_a.state(), 3);
    let placed_a = reply(
        entered_a.state(),
        P0,
        DecisionReply::TriggerPermanentTargets(vec![
            TargetRef::Object(art_1),
            TargetRef::Object(creat_1),
        ]),
    );
    let trigger_a = placed_a
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .expect("trigger must be on stack")
        .id();
    let paused_a = pass_pair(placed_a.state());
    let resolved_a = reply(
        paused_a.state(),
        P1,
        DecisionReply::OrderResolutionGraveyard(vec![art_1, creat_1]),
    );
    assert_completion(&resolved_a, trigger_a, EntryCompletion::Resolved);
    assert_eq!(zone_of(resolved_a.state(), 2), ObjectZone::Graveyard(P1));
    assert_eq!(zone_of(resolved_a.state(), 3), ObjectZone::Graveyard(P1));

    // Subcase B: Both targets leave the battlefield before resolution -> trigger fizzles.
    let entered_b = cast_untargeted(resolved_a.state(), 4);
    let art_2 = object(entered_b.state(), 5);
    let creat_2 = object(entered_b.state(), 6);
    let placed_b = reply(
        entered_b.state(),
        P0,
        DecisionReply::TriggerPermanentTargets(vec![
            TargetRef::Object(art_2),
            TargetRef::Object(creat_2),
        ]),
    );
    let trigger_b = placed_b
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .expect("trigger must be on stack")
        .id();
    let killed_art = cast_targeted(placed_b.state(), P0, 7, TargetRef::Object(art_2));
    let killed_both = cast_targeted(killed_art.state(), P0, 8, TargetRef::Object(creat_2));
    let fizzled = pass_pair(killed_both.state());
    assert_completion(&fizzled, trigger_b, EntryCompletion::AllTargetsIllegal);
}

// ---------------------------------------------------------------------------
// 2. Triggered FixedTargetClause (modal)
// ---------------------------------------------------------------------------

#[test]
fn triggered_modal_fixed_target_clause_chooses_mode_targets_and_executes() {
    let vocab = vocabulary();

    let fixed_destroy_mode = ExecutableBody::new(
        BodyTarget::None,
        vec![
            Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                BodyTarget::FilteredPermanent(ObjectFilter::artifact()),
                vec![Instruction::DestroyTarget],
            ))),
            Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                BodyTarget::FilteredPermanent(ObjectFilter::creature().not_controlled()),
                vec![Instruction::DestroyTarget],
            ))),
            Instruction::DestroyFixedTargets,
        ],
    );
    let life_mode = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::GainControllerLife {
            amount: ValueExpr::literal(4),
        }],
    );

    // Validation: FixedTargetClause inside prefix or suffix is rejected.
    for in_prefix in [true, false] {
        let mut invalid = Program::enchantment(Some(ManaCost::free()));
        let mut modes = TriggeredModes {
            minimum: 1,
            maximum: 1,
            distinct_targets: false,
            allow_repeat: false,
            uniqueness: ModalUniqueness::None,
            prefix: None,
            suffix: None,
            options: vec![life_mode.clone(), life_mode.clone()],
        };
        if in_prefix {
            modes.prefix = Some(Box::new(fixed_destroy_mode.clone()));
        } else {
            modes.suffix = Some(Box::new(fixed_destroy_mode.clone()));
        }
        invalid.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: TriggerSpec::self_enters(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::with_triggered_modes(modes),
        });
        assert_eq!(
            invalid.validate(&vocab),
            Err(DefinitionError::InvalidInstruction)
        );
    }

    let mut modal_etb = Program::enchantment(Some(ManaCost::free()));
    modal_etb.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::self_enters(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::with_triggered_modes(TriggeredModes {
            minimum: 1,
            maximum: 1,
            distinct_targets: false,
            allow_repeat: false,
            uniqueness: ModalUniqueness::None,
            prefix: None,
            suffix: None,
            options: vec![fixed_destroy_mode, life_mode],
        }),
    });
    let modal_etb = validated(modal_etb, &vocab);

    let game = world(
        vocab.clone(),
        vec![
            card(1, modal_etb.clone(), false),
            opp_battlefield(2, validated(Program::artifact(None, vec![]), &vocab)),
            opp_battlefield(3, validated(vanilla_creature(2, 2), &vocab)),
            card(4, modal_etb, false),
        ],
        vec![CardId(2), CardId(3)],
    );

    let opp_art = object(&game, 2);
    let opp_creat = object(&game, 3);

    // Subcase A: Mode 0 (FixedTargetClause x2) and Mode 1 are both offered; choose Mode 0.
    let entered_1 = cast_untargeted(&game, 1);
    let decision_1 = entered_1
        .state()
        .pending_decision()
        .expect("modal ETB trigger must prompt for mode and target choices");
    let RuleFrame::TriggerTarget(frame_1) = decision_1.frame() else {
        panic!("expected TriggerTarget frame, got {:?}", decision_1.frame());
    };
    let modal_1 = frame_1.modal().expect("expected modal trigger options");
    assert_eq!(modal_1.modes.len(), 2);
    assert_eq!(modal_1.modes[0].index, 0);
    assert_eq!(modal_1.modes[0].multiple, Some((2, 2)));
    assert!(modal_1.modes[0].target_required);
    assert!(
        modal_1.modes[0]
            .targets
            .contains(&TargetRef::Object(opp_art))
    );
    assert!(
        modal_1.modes[0]
            .targets
            .contains(&TargetRef::Object(opp_creat))
    );
    assert_eq!(modal_1.modes[1].index, 1);
    assert!(!modal_1.modes[1].target_required);

    // Wrong target ordering for Mode 0 is rejected.
    assert_eq!(
        attempt(
            entered_1.state(),
            P0,
            GameCommand::Reply {
                decision: decision_1.id(),
                choice: DecisionReply::TriggerModes(vec![CastMode {
                    mode: 0,
                    target: None,
                    targets: vec![TargetRef::Object(opp_creat), TargetRef::Object(opp_art)],
                }]),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget))
    );

    // Choosing Mode 0 with legal targets destroys both targets on resolution.
    let chosen_0 = reply(
        entered_1.state(),
        P0,
        DecisionReply::TriggerModes(vec![CastMode {
            mode: 0,
            target: None,
            targets: vec![TargetRef::Object(opp_art), TargetRef::Object(opp_creat)],
        }]),
    );
    let paused_0 = pass_pair(chosen_0.state());
    let resolved_0 = reply(
        paused_0.state(),
        P1,
        DecisionReply::OrderResolutionGraveyard(vec![opp_art, opp_creat]),
    );
    assert_eq!(zone_of(resolved_0.state(), 2), ObjectZone::Graveyard(P1));
    assert_eq!(zone_of(resolved_0.state(), 3), ObjectZone::Graveyard(P1));
    assert_eq!(
        resolved_0.state().execution().life_total(P0),
        Some(&20.into())
    );

    // Subcase B: With no legal artifact/creature targets left, only Mode 1 is offered.
    let entered_2 = cast_untargeted(resolved_0.state(), 4);
    let decision_2 = entered_2
        .state()
        .pending_decision()
        .expect("second modal ETB trigger must prompt for remaining legal mode");
    let RuleFrame::TriggerTarget(frame_2) = decision_2.frame() else {
        panic!("expected TriggerTarget frame, got {:?}", decision_2.frame());
    };
    let modal_2 = frame_2.modal().expect("expected modal trigger options");
    assert_eq!(modal_2.modes.len(), 1);
    assert_eq!(modal_2.modes[0].index, 1);

    let chosen_1 = reply(
        entered_2.state(),
        P0,
        DecisionReply::TriggerModes(vec![CastMode {
            mode: 1,
            target: None,
            targets: vec![],
        }]),
    );
    let resolved_1 = pass_pair(chosen_1.state());
    assert_eq!(
        resolved_1.state().execution().life_total(P0),
        Some(&24.into())
    );
}

// ---------------------------------------------------------------------------
// 3. SourceRelation::SharingNameWithAbilitySource
// ---------------------------------------------------------------------------

#[test]
fn sharing_name_with_ability_source_matches_triggers_and_filters() {
    let vocab = vocabulary();

    let mut apostle = named(vanilla_creature(1, 1), "Shadowborn Apostle");
    // Trigger matching (occurrence/matching.rs): fires only when a creature sharing this
    // source's exact name enters the battlefield.
    apostle.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::enters(ObjectFilter::creature().sharing_name_with_source()),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(3),
            }],
        ),
    });
    // Targeted filter matching (effects.rs): can target only creatures sharing source name.
    apostle.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::FilteredPermanent(ObjectFilter::creature().sharing_name_with_source()),
            vec![effect(
                vec![ResolvedPartSpec::ModifyPT(pt(1, 1))],
                ResolvedDuration::UntilEndOfTurn,
            )],
        ),
    });
    // Group battlefield filter matching (effects.rs matches_object_clause_with_source):
    // buffs every creature sharing this source's name, leaving other creatures unchanged.
    apostle.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(2),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(
                    ObjectFilter::creature().sharing_name_with_source(),
                )),
                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt(2, 2))]),
                duration: ResolvedDuration::UntilEndOfTurn,
            }],
        ),
    });
    let apostle = validated(apostle, &vocab);

    let sibling = validated(named(vanilla_creature(1, 1), "Shadowborn Apostle"), &vocab);
    let bears = validated(named(vanilla_creature(2, 2), "Grizzly Bears"), &vocab);

    let game = world(
        vocab,
        vec![
            card(1, apostle, true),
            card(2, sibling, false),
            card(3, bears, false),
        ],
        vec![CardId(1)],
    );

    let source_id = object(&game, 1);

    // Negative trigger match: casting a differently named creature does NOT trigger.
    let after_bears = cast_untargeted(&game, 3);
    assert!(
        after_bears
            .state()
            .execution()
            .window()
            .entries()
            .is_empty()
    );
    assert_eq!(
        after_bears.state().execution().life_total(P0),
        Some(&20.into())
    );
    let bears_id = object(after_bears.state(), 3);

    // Positive trigger match: casting a creature with the exact same name DOES trigger.
    let after_sibling = cast_untargeted(after_bears.state(), 2);
    assert_eq!(
        after_sibling.state().execution().window().entries().len(),
        1
    );
    let sibling_id = object(after_sibling.state(), 2);
    let after_trigger = pass_pair(after_sibling.state());
    assert_eq!(
        after_trigger.state().execution().life_total(P0),
        Some(&23.into()),
        "ETB trigger with SharingNameWithAbilitySource must fire for same-named creature"
    );

    // Negative targeted filter match: targeting Grizzly Bears is rejected.
    assert_eq!(
        attempt(
            after_trigger.state(),
            P0,
            GameCommand::ActivateAbility {
                source: source_id,
                ability: ActivatedAbilityId(1),
                target: TargetRef::Object(bears_id),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget))
    );

    // Positive targeted filter match: targeting the same-named sibling succeeds (+1/+1 -> 2/2).
    let act_targeted = submit(
        after_trigger.state(),
        P0,
        GameCommand::ActivateAbility {
            source: source_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(sibling_id),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_targeted = pass_pair(act_targeted.state());
    assert_eq!(
        current(after_targeted.state(), sibling_id)
            .power_toughness
            .map(|value| value.derived),
        Some(pt(2, 2))
    );

    // Group filter match: ChangeGroup with SharingNameWithAbilitySource buffs both
    // Shadowborn Apostles (+2/+2) while leaving Grizzly Bears untouched at 2/2.
    let act_group = submit(
        after_targeted.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: source_id,
            ability: ActivatedAbilityId(2),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_group = pass_pair(act_group.state());
    assert_eq!(
        current(after_group.state(), source_id)
            .power_toughness
            .map(|value| value.derived),
        Some(pt(3, 3))
    );
    assert_eq!(
        current(after_group.state(), sibling_id)
            .power_toughness
            .map(|value| value.derived),
        Some(pt(4, 4))
    );
    assert_eq!(
        current(after_group.state(), bears_id)
            .power_toughness
            .map(|value| value.derived),
        Some(pt(2, 2)),
        "differently named creature must not match SharingNameWithAbilitySource group filter"
    );
}

// ---------------------------------------------------------------------------
// 4. RulesOccurrence::Shuffled firing PlayerTriggerAction::ShuffledLibrary
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 5. DealDamageFrom with up_to_one_target = 0 targets (and 1 target)
// ---------------------------------------------------------------------------

#[test]
fn deal_damage_from_with_up_to_one_target_resolves_zero_and_one_targets() {
    let vocab = vocabulary();

    let deal_to_target = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::DealDamageFrom {
                    source: Box::new(DamageSourceRef::AbilitySource),
                    recipients: Box::new(DamageRecipientsRef::Target),
                    amount: Box::new(DamageAmountSpec::Expr(ValueExpr::literal(3))),
                }],
            )
            .with_up_to_one_target(),
        ),
        &vocab,
    );

    let target_hits_controller = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::DealDamageFrom {
                    source: Box::new(DamageSourceRef::TargetObject),
                    recipients: Box::new(DamageRecipientsRef::TargetController),
                    amount: Box::new(DamageAmountSpec::SourcePower),
                }],
            )
            .with_up_to_one_target(),
        ),
        &vocab,
    );

    let game = world(
        vocab.clone(),
        vec![
            card(1, deal_to_target.clone(), false),
            card(2, target_hits_controller, false),
            card(3, deal_to_target, false),
            opp_battlefield(4, validated(vanilla_creature(4, 4), &vocab)),
        ],
        vec![CardId(4)],
    );

    let opp_beast = object(&game, 4);

    // 0 targets for DealDamageFrom(AbilitySource -> Target): resolves cleanly, 0 damage.
    let cast_zero_a = submit(
        &game,
        P0,
        GameCommand::CastWithResources {
            spell: object(&game, 1),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let entry_zero_a = cast_zero_a
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let res_zero_a = pass_pair(cast_zero_a.state());
    assert_completion(&res_zero_a, entry_zero_a, EntryCompletion::Resolved);
    assert_eq!(
        res_zero_a
            .state()
            .execution()
            .creature(opp_beast)
            .unwrap()
            .unwrap()
            .damage,
        0.into()
    );

    // 0 targets for DealDamageFrom(TargetObject -> TargetController): resolves cleanly, 0 damage.
    let cast_zero_b = submit(
        res_zero_a.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(res_zero_a.state(), 2),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let entry_zero_b = cast_zero_b
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let res_zero_b = pass_pair(cast_zero_b.state());
    assert_completion(&res_zero_b, entry_zero_b, EntryCompletion::Resolved);
    assert_eq!(
        res_zero_b.state().execution().life_total(P1),
        Some(&20.into())
    );

    // 1 target for DealDamageFrom(AbilitySource -> Target): deals 3 damage to opp_beast.
    let cast_one = submit(
        res_zero_b.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(res_zero_b.state(), 3),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![TargetRef::Object(opp_beast)],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let res_one = pass_pair(cast_one.state());
    assert_eq!(
        res_one
            .state()
            .execution()
            .creature(opp_beast)
            .unwrap()
            .unwrap()
            .damage,
        3.into(),
        "DealDamageFrom with 1 chosen target must deal 3 damage"
    );
}

// ---------------------------------------------------------------------------
// 6. CardType::Planeswalker admitted in validate_battlefield_types
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 7. Cast-time SourceStateExpr on Unbound / resolving CardSpell
// ---------------------------------------------------------------------------

#[test]
fn cast_time_source_state_expr_validates_and_evaluates_on_resolving_spell() {
    let vocab = vocabulary();

    // All four cast-time SourceStateExpr variants validate on an unbound spell program.
    for expr in [
        SourceStateExpr::ManaSpentToCast,
        SourceStateExpr::CastFromGraveyard,
        SourceStateExpr::CastFromExile,
        SourceStateExpr::Bargained,
    ] {
        let probe = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::SourceState(expr),
                }],
            ),
        );
        assert!(
            probe.validate(&vocab).is_ok(),
            "cast-time SourceStateExpr::{expr:?} must validate on unbound spell"
        );
    }

    // Non-cast-time SourceStateExpr remains rejected on an unbound spell program.
    let invalid_spell = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::SourceState(SourceStateExpr::EnteredThisTurn),
            }],
        ),
    );
    assert_eq!(
        invalid_spell.validate(&vocab),
        Err(DefinitionError::InvalidInstruction)
    );

    // Spell A: gains life equal to SourceStateExpr::ManaSpentToCast (costs {3}).
    let mana_spent_spell = validated(
        Program::sorcery(
            ManaCost {
                generic: 3.into(),
                required: BTreeMap::new(),
            },
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::SourceState(SourceStateExpr::ManaSpentToCast),
                }],
            ),
        ),
        &vocab,
    );

    // Spell B: castable from hand, graveyard, or exile; gains 4 if cast from graveyard and
    // 7 if cast from exile.
    let mut zone_origin_spell = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::GainControllerLife {
                    amount: ValueExpr::SourceState(SourceStateExpr::CastFromGraveyard).scaled(4),
                },
                Instruction::GainControllerLife {
                    amount: ValueExpr::SourceState(SourceStateExpr::CastFromExile).scaled(7),
                },
            ],
        ),
    );
    zone_origin_spell.graveyard_self_cast = Some(GraveyardSelfCast::GraveyardOrExile);
    let zone_origin_spell = validated(zone_origin_spell, &vocab);

    let game = world(
        vocab.clone(),
        vec![
            card(1, validated(colorless_rock(3), &vocab), true),
            card(2, mana_spent_spell, false),
            card(3, zone_origin_spell.clone(), false),
            graveyard_of(4, P0, zone_origin_spell.clone()),
            exile_of(5, P0, zone_origin_spell),
        ],
        vec![CardId(1)],
    );

    // Tap rock for {3}, cast Spell A paying {3} -> ManaSpentToCast evaluates to 3 (20 -> 23).
    let with_mana = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source: object(&game, 1),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let cast_a = submit(
        with_mana.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(with_mana.state(), 2),
            payment: Some(colorless_payment(3)),
        },
    );
    let after_a = pass_pair(cast_a.state());
    assert_eq!(after_a.state().execution().life_total(P0), Some(&23.into()));

    // Cast Spell B from hand -> CastFromGraveyard = 0, CastFromExile = 0 -> life stays 23.
    let after_hand = cast_untargeted(after_a.state(), 3);
    assert_eq!(
        after_hand.state().execution().life_total(P0),
        Some(&23.into())
    );

    // Cast Spell B from graveyard -> CastFromGraveyard = 1 (+4) -> life becomes 27.
    let after_gy = cast_untargeted(after_hand.state(), 4);
    assert_eq!(
        after_gy.state().execution().life_total(P0),
        Some(&27.into())
    );

    // Cast Spell B from exile -> CastFromExile = 1 (+7) -> life becomes 34.
    let after_exile = cast_untargeted(after_gy.state(), 5);
    assert_eq!(
        after_exile.state().execution().life_total(P0),
        Some(&34.into())
    );
}

// ---------------------------------------------------------------------------
// 8. TargetSpellController on non-spell stack entries (activated / triggered)
// ---------------------------------------------------------------------------

#[test]
fn target_spell_controller_resolves_for_activated_and_triggered_stack_entries() {
    let vocab = vocabulary();

    // Validation: TargetSpellController validates with both SingleStackEntry and SingleStackObject.
    let stack_object_probe = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleStackObject(SpellFilter::any()),
            vec![Instruction::AdjustPlayersLife {
                players: AffectedPlayers::One(ResolvingPlayer::TargetSpellController),
                gain: false,
                amount: ValueExpr::literal(2),
            }],
        ),
    );
    assert!(stack_object_probe.validate(&vocab).is_ok());

    let punish_stack_controller = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleStackEntry,
                vec![Instruction::AdjustPlayersLife {
                    players: AffectedPlayers::One(ResolvingPlayer::TargetSpellController),
                    gain: false,
                    amount: ValueExpr::literal(4),
                }],
            ),
        ),
        &vocab,
    );

    let opp_artifact = validated(
        Program::artifact(
            None,
            vec![ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::literal(1),
                    }],
                ),
            }],
        ),
        &vocab,
    );

    let mut opp_watcher = Program::enchantment(None);
    opp_watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::enters(ObjectFilter::creature()),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(1),
            }],
        ),
    });
    let opp_watcher = validated(opp_watcher, &vocab);

    let game = world(
        vocab.clone(),
        vec![
            card(1, punish_stack_controller.clone(), false),
            card(2, punish_stack_controller, false),
            card(3, validated(vanilla_creature(2, 2), &vocab), false),
            opp_battlefield(4, opp_artifact),
            opp_battlefield(5, opp_watcher),
        ],
        vec![CardId(4), CardId(5)],
    );

    // Subcase A: P1 activates an artifact ability on the stack; P0 targets that activated
    // ability with SingleStackEntry -> TargetSpellController resolves to P1 (-4 life).
    let p1_prio = submit(&game, P0, GameCommand::Pass).into_state();
    let p1_act = submit(
        &p1_prio,
        P1,
        GameCommand::ActivateUntargetedAbility {
            source: object(&p1_prio, 4),
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let act_entry = p1_act
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .expect("activated ability must be on stack")
        .id();
    let p0_resp = submit(p1_act.state(), P1, GameCommand::Pass).into_state();
    let after_punish_act = cast_targeted(&p0_resp, P0, 1, TargetRef::Stack(act_entry));
    assert_eq!(
        after_punish_act.state().execution().life_total(P0),
        Some(&20.into())
    );
    assert_eq!(
        after_punish_act.state().execution().life_total(P1),
        Some(&16.into())
    );
    let after_act_res = pass_pair(after_punish_act.state());
    assert_eq!(
        after_act_res.state().execution().life_total(P1),
        Some(&17.into())
    );

    // Subcase B: P0 casts a creature, firing P1's ETB triggered ability onto the stack; P0
    // targets that triggered ability -> TargetSpellController resolves to P1 (-4 life).
    let after_creature = cast_untargeted(after_act_res.state(), 3);
    let trig_entry = after_creature
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .expect("opponent ETB triggered ability must be on stack")
        .id();
    let after_punish_trig =
        cast_targeted(after_creature.state(), P0, 2, TargetRef::Stack(trig_entry));
    assert_eq!(
        after_punish_trig.state().execution().life_total(P0),
        Some(&20.into())
    );
    assert_eq!(
        after_punish_trig.state().execution().life_total(P1),
        Some(&13.into())
    );
    let after_trig_res = pass_pair(after_punish_trig.state());
    assert_eq!(
        after_trig_res.state().execution().life_total(P1),
        Some(&14.into())
    );
}

// ---------------------------------------------------------------------------
// 9. Non-controller SourceCost::Counter and SourceCost::Exile activation
// ---------------------------------------------------------------------------

#[test]
fn non_controller_can_activate_counter_and_exile_source_cost_abilities() {
    let vocab = vocabulary();

    let one = NonZeroU8::new(1).unwrap();

    // Negative validation: OnlyOpponents with tap_source: true on SourceCost::Counter is rejected.
    let mut bad_tap_counter = Program::artifact(
        Some(ManaCost::free()),
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::Counter {
                kind: CounterKind::PlusOne,
                delta: CounterCostDelta::Remove(one),
                tap_source: true,
                sacrifice_source: false,
            },
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(1),
                }],
            ),
        }],
    );
    bad_tap_counter
        .activation_restrictions
        .insert(ActivatedAbilityId(1), ActivationRestriction::OnlyOpponents);
    assert_eq!(
        bad_tap_counter.validate(&vocab),
        Err(DefinitionError::InvalidInstruction)
    );

    // Permanent 1 (controlled by P0): enters with 2 +1/+1 counters; OnlyOpponents may remove
    // a +1/+1 counter to gain 3 life.
    let mut counter_relic = Program::artifact(
        Some(ManaCost::free()),
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::Counter {
                kind: CounterKind::PlusOne,
                delta: CounterCostDelta::Remove(one),
                tap_source: false,
                sacrifice_source: false,
            },
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(3),
                }],
            ),
        }],
    );
    counter_relic
        .entry_counter_abilities
        .push(EntryCounterAbility::FixedPlusOne(2.into()));
    counter_relic
        .activation_restrictions
        .insert(ActivatedAbilityId(1), ActivationRestriction::OnlyOpponents);
    let counter_relic = validated(counter_relic, &vocab);

    // Permanent 2 (controlled by P0): AnyPlayer may exile this permanent to gain 2 life.
    let mut exile_relic = Program::artifact(
        Some(ManaCost::free()),
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::Exile {
                tap_source: false,
                from_graveyard: false,
            },
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(2),
                }],
            ),
        }],
    );
    exile_relic
        .activation_restrictions
        .insert(ActivatedAbilityId(1), ActivationRestriction::AnyPlayer);
    let exile_relic = validated(exile_relic, &vocab);

    let game = world(
        vocab,
        vec![card(1, counter_relic, false), card(2, exile_relic, true)],
        vec![CardId(2)],
    );

    // Cast counter_relic so it enters with 2 +1/+1 counters.
    let entered = cast_untargeted(&game, 1);
    let relic_1 = object(entered.state(), 1);
    let relic_2 = object(entered.state(), 2);
    assert_eq!(
        entered
            .state()
            .execution()
            .power_toughness_counters(relic_1)
            .unwrap()
            .plus_one,
        2.into()
    );

    // Controller P0 cannot activate an OnlyOpponents ability.
    assert_eq!(
        attempt(
            entered.state(),
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: relic_1,
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::ActivationConditionNotMet))
    );

    // Pass priority to opponent P1; P1 activates counter_relic's remove-counter ability.
    let p1_prio = submit(entered.state(), P0, GameCommand::Pass).into_state();
    let act_counter = submit(
        &p1_prio,
        P1,
        GameCommand::ActivateUntargetedAbility {
            source: relic_1,
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    assert_eq!(
        act_counter
            .state()
            .execution()
            .power_toughness_counters(relic_1)
            .unwrap()
            .plus_one,
        1.into(),
        "non-controller activation must pay SourceCost::Counter by removing 1 counter"
    );
    let res_counter = pass_in_order(act_counter.state(), [P1, P0]);
    assert_eq!(
        res_counter.state().execution().life_total(P1),
        Some(&23.into())
    );
    assert_eq!(
        res_counter.state().execution().life_total(P0),
        Some(&20.into())
    );

    // Pass priority to opponent P1 again; P1 activates exile_relic's SourceCost::Exile ability.
    let p1_prio_2 = submit(res_counter.state(), P0, GameCommand::Pass).into_state();
    let act_exile = submit(
        &p1_prio_2,
        P1,
        GameCommand::ActivateUntargetedAbility {
            source: relic_2,
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    assert_eq!(
        zone_of(act_exile.state(), 2),
        ObjectZone::Exile,
        "non-controller activation must exile P0's permanent as SourceCost::Exile"
    );
    let res_exile = pass_in_order(act_exile.state(), [P1, P0]);
    assert_eq!(
        res_exile.state().execution().life_total(P1),
        Some(&25.into())
    );
    assert_eq!(
        res_exile.state().execution().life_total(P0),
        Some(&20.into())
    );
}

// ---------------------------------------------------------------------------
// 10. Newly admitted OptionalEffect / OptionalSequence primitives
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 11. TriggerZone::Any on a spell program & AffectPermanentTargets follow-ups
// ---------------------------------------------------------------------------
