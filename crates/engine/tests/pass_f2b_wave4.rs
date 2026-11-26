#![allow(dead_code, unused_imports)]
//! Runtime integration tests for Pass F2-B Wave 4 primitives:
//! - CR 611.2b source-bound `ResolvedDuration` (`WhileAbilitySourceOnBattlefield`,
//!   `WhileAbilitySourceTapped`, `WhileControllingTappedAbilitySource`)
//! - `StaticSourceCondition` (`SourceAnyCounterAtLeast`, `SourceIsEquipped`,
//!   `SourceIsEnchanted`, `AttachedHostMatches`, `ControlsExactCount`)
//! - `SearchCardMatcher::SameNameAs` (ability source, target object, and dying trigger LKI)
//! - `Instruction::CounterTriggeringStackObject` (targeting spells with once-per-turn
//!   grouping and `SpellCast` triggers)
//! - `Instruction::SetSubjectTapped` (`CreatedTokens`, `TriggerSubject`, `LastMoved`)
//! - Multi-target `Instruction::ShuffleTargetIntoLibrary` with `graveyard_target_bounds`
//! - `ExecutableBody::with_up_to_one_target()` with `Instruction::Fight`
//! - `ValueObjectRef::{RevealedCostCard, ExiledCostCreature}` (`Power` and `ManaValue`).
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::names::{EnglishCardName, ObjectNames};
use spellwake_engine::{
    activated::{
        ActivatedAbility, ActivatedAbilityId, ExtraActivationCost, GraveyardExileCost, SourceCost,
    },
    attachment::{AttachAbility, AttachmentKind, EnchantRestriction},
    characteristics::{
        ControlRecipient, ControllerSelector, KeywordAbility, NumericExpression, NumericPair,
        PermanentSelector, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec, StaticAbilityId,
        StaticContinuousAbility, StaticPart, StaticParts, StaticSourceCondition,
    },
    effects::{CounterKind, SeedCard},
    game::{GameCommand, GameState, NativeCandidate, RuleFrame},
    mana::{CastResource, CastingPayment, ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    program::{
        ArtifactSubtype, BodyTarget, CardType, EffectSubject, ExecutableBody, Instruction,
        LibrarySearch, MoveSubjectRef, ObjectFilter, ObjectStatKind, PermanentDefinition,
        PlayerRelation, Program, ProgramKind, ResolvingPlayer, SearchCardMatcher, SearchCount,
        SearchDestination, SearchSource, TriggerEvent, TriggerGrouping, TriggerPlayerScope,
        TriggerSpec, TriggeredAbility, TriggeredAbilityId, ValueExpr, ValueObjectRef,
    },
    targets::TargetRef,
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients},
};
use std::{collections::BTreeSet, num::NonZeroU8};

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

fn cast_targeted(
    game: &GameState,
    actor: spellwake_engine::priority::SeatId,
    id: u64,
    target: TargetRef,
) -> NativeCandidate {
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

fn library_card(id: u64, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: P0,
        destination: ObjectDestination::LibraryTop,
        program,
    }
}

fn graveyard_card(id: u64, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: P0,
        destination: ObjectDestination::Graveyard,
        program,
    }
}

fn named_creature(name: &str, power: i64, toughness: i64) -> Program {
    let mut prog = Program::creature(
        Some(ManaCost::free()),
        power.into(),
        toughness.into(),
        BTreeSet::new(),
    );
    prog.names = ObjectNames::single(EnglishCardName::new(name.into()).unwrap());
    prog
}

#[test]
fn search_same_name_as_matches_source_target_and_dying_trigger_lki() {
    let vocab = vocabulary();

    // Case 1: AbilitySource (Squadron Hawk ETB search).
    let mut hawk = named_creature("Squadron Hawk", 1, 1);
    hawk.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::enters(ObjectFilter::self_object()),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::SearchLibrary {
                filter: Box::new(LibrarySearch {
                    searcher: ResolvingPlayer::Controller,
                    source: SearchSource::OwnLibrary,
                    matcher: SearchCardMatcher::SameNameAs {
                        reference: MoveSubjectRef::AbilitySource,
                        extra: None,
                    },
                    mana_value: None,
                    count: SearchCount::UpTo(3),
                    pair_constraint: None,
                    destination: SearchDestination::Hand,
                    reveal: true,
                }),
            }],
        ),
    });

    // Case 2: TriggeringObject LKI (Remembrance-style dies trigger).
    let mut remembrance = Program::enchantment(None);
    remembrance.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(2),
        condition: TriggerSpec::dies(ObjectFilter::creature()),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::SearchLibrary {
                filter: Box::new(LibrarySearch {
                    searcher: ResolvingPlayer::Controller,
                    source: SearchSource::OwnLibrary,
                    matcher: SearchCardMatcher::SameNameAs {
                        reference: MoveSubjectRef::TriggeringObject,
                        extra: None,
                    },
                    mana_value: None,
                    count: SearchCount::UpTo(1),
                    pair_constraint: None,
                    destination: SearchDestination::Hand,
                    reveal: true,
                }),
            }],
        ),
    });

    // Case 3: TargetObject (Pack Hunt-style targeted search).
    let pack_hunt = spell(
        10,
        BodyTarget::SingleCreature,
        vec![Instruction::SearchLibrary {
            filter: Box::new(LibrarySearch {
                searcher: ResolvingPlayer::Controller,
                source: SearchSource::OwnLibrary,
                matcher: SearchCardMatcher::SameNameAs {
                    reference: MoveSubjectRef::TargetObject,
                    extra: None,
                },
                mana_value: None,
                count: SearchCount::UpTo(1),
                pair_constraint: None,
                destination: SearchDestination::Hand,
                reveal: true,
            }),
        }],
    );

    let destroy = spell(
        11,
        BodyTarget::SingleCreature,
        vec![Instruction::DestroyTarget],
    );

    let game = world(
        vocab,
        vec![
            card(1, hawk.clone(), false),
            library_card(2, hawk),
            library_card(3, named_creature("Grizzly Bears", 2, 2)),
            card(4, remembrance, true),
            card(5, named_creature("Grizzly Bears", 2, 2), true),
            pack_hunt,
            destroy,
        ],
        vec![CardId(4), CardId(5)],
    );

    // Test Case 1: cast Hawk, resolve ETB trigger -> only card 2 ("Squadron Hawk") is eligible.
    let hawk_entered = cast_untargeted(&game, 1);
    let hawk_search = pass_pair(hawk_entered.state());
    let RuleFrame::LibraryTop(frame) = hawk_search.state().pending_decision().unwrap().frame()
    else {
        panic!("expected Hawk search frame");
    };
    assert_eq!(frame.eligible_results(), &[object(hawk_search.state(), 2)]);

    // Test Case 3 (on original game): cast Pack Hunt targeting Grizzly Bears (card 5) -> only card 3 eligible.
    let bears_id = object(&game, 5);
    let target_search = cast_targeted(&game, P0, 10, TargetRef::Object(bears_id));
    let RuleFrame::LibraryTop(frame) = target_search.state().pending_decision().unwrap().frame()
    else {
        panic!("expected Pack Hunt search frame");
    };
    assert_eq!(
        frame.eligible_results(),
        &[object(target_search.state(), 3)]
    );

    // Test Case 2 (on original game): destroy Grizzly Bears (card 5) -> Remembrance triggers using LKI -> only card 3 eligible.
    let after_destroy = cast_targeted(&game, P0, 11, TargetRef::Object(bears_id));
    let lki_search = pass_pair(after_destroy.state());
    let RuleFrame::LibraryTop(frame) = lki_search.state().pending_decision().unwrap().frame()
    else {
        panic!("expected Remembrance LKI search frame");
    };
    assert_eq!(frame.eligible_results(), &[object(lki_search.state(), 3)]);
}

#[test]
fn counter_triggering_stack_object_handles_targeted_once_per_turn_and_spell_cast() {
    let vocab = vocabulary();
    // Shimmering Glasskite: Whenever this creature becomes the target of a spell or ability
    // for the first time each turn, counter that spell or ability.
    let mut glasskite = Program::creature(None, 2.into(), 3.into(), BTreeSet::new());
    glasskite.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::new(TriggerEvent::Targeted {
            subject: ObjectFilter::self_object(),
            by_opponent: false,
            spell_only: false,
            instant_or_sorcery_only: false,
        }),
        grouping: TriggerGrouping::OneOrMoreOnceEachTurn,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CounterTriggeringStackObject],
        ),
    });

    let destroy_a = spell(
        2,
        BodyTarget::SingleCreature,
        vec![Instruction::DestroyTarget],
    );
    let destroy_b = spell(
        3,
        BodyTarget::SingleCreature,
        vec![Instruction::DestroyTarget],
    );

    let game = world(
        vocab.clone(),
        vec![card(1, glasskite, true), destroy_a, destroy_b],
        vec![CardId(1)],
    );

    let kite_id = object(&game, 1);

    // First targeting spell triggers Glasskite and gets countered -> Glasskite survives!
    let cast1 = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 2),
            target: TargetRef::Object(kite_id),
            payment: ManaPayment::default(),
        },
    );
    // Resolve the Glasskite counter trigger on top of the stack.
    let after_trigger = pass_pair(cast1.state());
    assert_eq!(
        after_trigger
            .state()
            .execution()
            .window()
            .cards()
            .get(kite_id)
            .map(|c| c.zone()),
        Some(ObjectZone::Battlefield)
    );
    assert!(
        after_trigger
            .state()
            .execution()
            .window()
            .entries()
            .is_empty(),
        "first targeting spell should have been countered off the stack"
    );

    // Second targeting spell this same turn does NOT trigger Glasskite -> Glasskite is destroyed!
    let cast2 = cast_targeted(after_trigger.state(), P0, 3, TargetRef::Object(kite_id));
    assert!(
        cast2
            .state()
            .execution()
            .window()
            .cards()
            .get(kite_id)
            .is_none_or(|c| c.zone() != ObjectZone::Battlefield),
        "second targeting spell same turn should resolve and destroy Glasskite"
    );

    // SpellCast trigger countering any cast spell.
    let mut chalice = Program::artifact(None, vec![]);
    chalice.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(2),
        condition: TriggerSpec::new(TriggerEvent::SpellCast {
            caster: TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant),
            filter: spellwake_engine::program::SpellFilter::any(),
            self_cast: false,
            include_copy: false,
            min_mana_value: None,
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CounterTriggeringStackObject],
        ),
    });
    let gain_life = spell(
        2,
        BodyTarget::None,
        vec![Instruction::GainControllerLife {
            amount: ValueExpr::literal(5),
        }],
    );
    let game2 = world(
        vocab,
        vec![card(1, chalice, true), gain_life],
        vec![CardId(1)],
    );
    let life_before = game2.execution().life_total(P0).cloned().unwrap();
    let cast = submit(
        &game2,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game2, 2),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_counter = pass_pair(cast.state());
    assert!(
        after_counter
            .state()
            .execution()
            .window()
            .entries()
            .is_empty()
    );
    assert_eq!(
        after_counter.state().execution().life_total(P0),
        Some(&life_before)
    );
}

#[test]
fn multi_target_graveyard_shuffle_moves_selected_cards_and_handles_zero_targets() {
    let vocab = vocabulary();
    // Perpetual Timepiece-style ability: {0}: Shuffle up to 3 target cards from your graveyard into your library.
    let mut timepiece = Program::artifact(None, vec![]);
    timepiece.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SingleOwnGraveyardCard,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    spellwake_engine::toolbox::ZoneDest::Library {
                        position: spellwake_engine::toolbox::LibraryOrderPos::ShuffleInto,
                    },
                ),
            ))],
        )
        .with_graveyard_target_bounds(0, 3),
    });

    let bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let game = world(
        vocab,
        vec![
            card(1, timepiece, true),
            graveyard_card(2, bear.clone()),
            graveyard_card(3, bear.clone()),
            graveyard_card(4, bear),
        ],
        vec![CardId(1)],
    );

    let tp_id = object(&game, 1);
    let gy2 = object(&game, 2);
    let gy3 = object(&game, 3);

    // Case A: Choose 2 of the 3 graveyard cards -> both shuffle into library, card 4 stays in graveyard.
    let act_two = submit(
        &game,
        P0,
        GameCommand::ActivatePermanentTargets {
            source: tp_id,
            ability: ActivatedAbilityId(1),
            targets: vec![TargetRef::Object(gy2), TargetRef::Object(gy3)],
            payment: Some(ManaPayment::default()),
        },
    );
    let ready = act_two
        .state()
        .supply_entropy(act_two.state().basis(), vec![0; 16])
        .unwrap()
        .into_state();
    let resolved_two = pass_pair(&ready);
    let cards_after = resolved_two.state().execution().window().cards();
    assert_eq!(cards_after.library(P0).len(), 2);
    assert_eq!(cards_after.graveyard(P0).len(), 1);
    assert_eq!(
        cards_after
            .get(cards_after.graveyard(P0)[0])
            .and_then(|c| c.card_id()),
        Some(CardId(4))
    );

    // Case B: Choose 0 targets -> resolves cleanly without touching graveyard.
    let act_zero = submit(
        &game,
        P0,
        GameCommand::ActivatePermanentTargets {
            source: tp_id,
            ability: ActivatedAbilityId(1),
            targets: vec![],
            payment: Some(ManaPayment::default()),
        },
    );
    let resolved_zero = pass_pair(act_zero.state());
    assert_eq!(
        resolved_zero
            .state()
            .execution()
            .window()
            .cards()
            .graveyard(P0)
            .len(),
        3
    );
}

#[test]
fn up_to_one_target_fight_fights_when_targeted_and_noops_when_untargeted() {
    let vocab = vocabulary();
    // Activated ability: {0}: This creature fights up to one target creature.
    let mut brawler = Program::creature(None, 3.into(), 3.into(), BTreeSet::new());
    brawler.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::Fight {
                first: Box::new(MoveSubjectRef::AbilitySource),
                second: Box::new(MoveSubjectRef::TargetObject),
            }],
        )
        .with_up_to_one_target(),
    });

    let opp_bear = Program::creature(None, 3.into(), 3.into(), BTreeSet::new());
    let game = world(
        vocab,
        vec![card(1, brawler, true), opp_battlefield(2, opp_bear)],
        vec![CardId(1), CardId(2)],
    );

    let brawler_id = object(&game, 1);
    let opp_id = object(&game, 2);

    // Case A: 1 target -> both 3/3 creatures deal 3 damage to each other and die.
    let act_one = submit(
        &game,
        P0,
        GameCommand::ActivatePermanentTargets {
            source: brawler_id,
            ability: ActivatedAbilityId(1),
            targets: vec![TargetRef::Object(opp_id)],
            payment: Some(ManaPayment::default()),
        },
    );
    let fought = pass_pair(act_one.state());
    assert!(
        fought
            .state()
            .execution()
            .window()
            .cards()
            .get(brawler_id)
            .is_none_or(|c| c.zone() != ObjectZone::Battlefield)
    );
    assert!(
        fought
            .state()
            .execution()
            .window()
            .cards()
            .get(opp_id)
            .is_none_or(|c| c.zone() != ObjectZone::Battlefield)
    );

    // Case B: 0 targets -> resolves cleanly, both survive on battlefield.
    let act_zero = submit(
        &game,
        P0,
        GameCommand::ActivatePermanentTargets {
            source: brawler_id,
            ability: ActivatedAbilityId(1),
            targets: vec![],
            payment: Some(ManaPayment::default()),
        },
    );
    let nooped = pass_pair(act_zero.state());
    assert_eq!(
        nooped
            .state()
            .execution()
            .window()
            .cards()
            .get(brawler_id)
            .map(|c| c.zone()),
        Some(ObjectZone::Battlefield)
    );
    assert_eq!(
        nooped
            .state()
            .execution()
            .window()
            .cards()
            .get(opp_id)
            .map(|c| c.zone()),
        Some(ObjectZone::Battlefield)
    );
}

#[test]
fn revealed_cost_card_and_exiled_cost_creature_evaluate_power_and_mana_value() {
    let vocab = vocabulary();

    // 1. Spell with extra RevealHand cost: gain life equal to revealed card's power + mana value.
    let mut reveal_spell = Program::sorcery(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::GainControllerLife {
                    amount: ValueExpr::ObjectStat {
                        subject: ValueObjectRef::RevealedCostCard,
                        stat: ObjectStatKind::Power,
                    },
                },
                Instruction::GainControllerLife {
                    amount: ValueExpr::ObjectStat {
                        subject: ValueObjectRef::RevealedCostCard,
                        stat: ObjectStatKind::ManaValue,
                    },
                },
            ],
        ),
    );
    reveal_spell
        .extra_cast_costs
        .push(ExtraActivationCost::RevealHand {
            filter: ObjectFilter::creature(),
            count: NonZeroU8::new(1).unwrap(),
            allow_controlled: false,
        });

    // 2. Activated ability with ExileGraveyard(Creature): gain life equal to exiled card's mana value.
    let mut paleontologist = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    paleontologist.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::ExileGraveyard {
            filter: GraveyardExileCost::Creature {
                pay_mana_cost: false,
            },
            tap_source: true,
        },
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::ObjectStat {
                    subject: ValueObjectRef::ExiledCostCreature,
                    stat: ObjectStatKind::ManaValue,
                },
            }],
        ),
    });

    // 6-power, MV 4 creature in hand; MV 5 creature in graveyard.
    let giant_hand = Program::creature(
        Some(ManaCost {
            generic: 4.into(),
            required: Default::default(),
        }),
        6.into(),
        5.into(),
        BTreeSet::new(),
    );
    let fossil_gy = Program::creature(
        Some(ManaCost {
            generic: 5.into(),
            required: Default::default(),
        }),
        3.into(),
        3.into(),
        BTreeSet::new(),
    );

    let game = world(
        vocab,
        vec![
            card(1, reveal_spell, false),
            card(2, giant_hand, false),
            card(3, paleontologist, true),
            graveyard_card(4, fossil_gy),
        ],
        vec![CardId(3)],
    );

    let life_before = game.execution().life_total(P0).cloned().unwrap();

    // Case 1: Cast reveal_spell revealing giant_hand (power 6 + MV 4 = +10 life).
    let cast = submit(
        &game,
        P0,
        GameCommand::CastWithResources {
            spell: object(&game, 1),
            target: None,
            payment: CastingPayment {
                mana: Some(ManaPayment::default()),
                objects: vec![CastResource::ExtraCost(object(&game, 2))],
                ..CastingPayment::default()
            },
        },
    );
    let after_reveal = pass_pair(cast.state());
    assert_eq!(
        after_reveal.state().execution().life_total(P0),
        Some(&life_before.plus(&10.into()))
    );

    // Case 2: Activate paleontologist exiling fossil_gy (MV 5 = +5 life).
    let act = submit(
        &game,
        P0,
        GameCommand::ActivateWithCostObjects {
            source: object(&game, 3),
            ability: ActivatedAbilityId(1),
            granting_effect: None,
            target: None,
            objects: vec![object(&game, 4)],
            payment: Some(ManaPayment::default()),
        },
    );
    let after_exile = pass_pair(act.state());
    assert_eq!(
        after_exile.state().execution().life_total(P0),
        Some(&life_before.plus(&5.into()))
    );
}
