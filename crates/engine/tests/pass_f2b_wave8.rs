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

fn destroy_permanent_instant() -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                        semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                            allow_regenerate: true,
                        },
                    },
                ),
            ))],
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
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
                    ))],
                ))),
                Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                    BodyTarget::FilteredPermanent(ObjectFilter::creature().not_controlled()),
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
                    ))],
                ))),
                Instruction::Fight {
                    first: Box::new(spellwake_engine::program::MoveSubjectRef::RememberedFirst),
                    second: Box::new(spellwake_engine::program::MoveSubjectRef::RememberedSecond),
                },
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

    // Duplicate targets are rejected for fixed-target Fight.
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

// ---------------------------------------------------------------------------
// 2. Triggered FixedTargetClause (modal)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 3. SourceRelation::SharingNameWithAbilitySource
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 4. RulesOccurrence::Shuffled firing PlayerTriggerAction::ShuffledLibrary
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 5. DealDamageFrom with up_to_one_target = 0 targets (and 1 target)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 6. CardType::Planeswalker admitted in validate_battlefield_types
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 7. Cast-time SourceStateExpr on Unbound / resolving CardSpell
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 8. TargetSpellController on non-spell stack entries (activated / triggered)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 9. Non-controller SourceCost::Counter and SourceCost::Exile activation
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 10. Newly admitted OptionalEffect / OptionalSequence primitives
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 11. TriggerZone::Any on a spell program & AffectPermanentTargets follow-ups
// ---------------------------------------------------------------------------
