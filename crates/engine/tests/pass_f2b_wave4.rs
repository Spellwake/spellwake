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
        SearchDestination, SearchSource, SubjectDestination, TriggerEvent, TriggerGrouping,
        TriggerPlayerScope, TriggerSpec, TriggeredAbility, TriggeredAbilityId, ValueExpr,
        ValueObjectRef,
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
fn while_ability_source_on_battlefield_persists_across_control_change_and_expires_on_exit() {
    let vocab = vocabulary();
    // Source permanent: {0}: Target creature gets +3/+3 for as long as this permanent remains on the battlefield.
    let mut anchor = Program::artifact(None, vec![]);
    anchor.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::CreateContinuousEffect {
                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt(3, 3))]),
                duration: ResolvedDuration::WhileAbilitySourceOnBattlefield,
            }],
        ),
    });

    let bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    // Opponent steals anchor permanently.
    let steal = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::CreateContinuousEffect {
                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                    ControlRecipient::ResolvingController,
                )]),
                duration: ResolvedDuration::NoStatedDuration,
            }],
        ),
    );
    let destroy = spell(
        4,
        BodyTarget::SinglePermanent,
        vec![Instruction::DestroyTarget],
    );

    let game = world(
        vocab,
        vec![
            card(1, anchor, true),
            card(2, bear, true),
            opp_hand(3, steal),
            destroy,
        ],
        vec![CardId(1), CardId(2)],
    );

    let anchor_id = object(&game, 1);
    let bear_id = object(&game, 2);

    // Activate buff on bear -> 5/5.
    let act = submit(
        &game,
        P0,
        GameCommand::ActivateAbility {
            source: anchor_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(bear_id),
            payment: Some(ManaPayment::default()),
        },
    );
    let buffed = pass_pair(act.state());
    let buffed_pt = current(buffed.state(), bear_id)
        .power_toughness
        .expect("bear pt");
    assert_eq!(buffed_pt.derived, pt(5, 5));

    // Pass priority to P1 so P1 steals anchor; effect must STILL persist because anchor is still on battlefield!
    let p1_prio = submit(buffed.state(), P0, GameCommand::Pass);
    let stolen = cast_targeted(p1_prio.state(), P1, 3, TargetRef::Object(anchor_id));
    assert_eq!(current(stolen.state(), anchor_id).controller, Some(P1));
    let still_buffed = current(stolen.state(), bear_id)
        .power_toughness
        .expect("bear pt after steal");
    assert_eq!(still_buffed.derived, pt(5, 5));

    // Destroy anchor -> effect expires immediately (back to 2/2).
    let destroyed = cast_targeted(stolen.state(), P0, 4, TargetRef::Object(anchor_id));
    let after_exit = current(destroyed.state(), bear_id)
        .power_toughness
        .expect("bear pt after exit");
    assert_eq!(after_exit.derived, pt(2, 2));
}

#[test]
fn while_ability_source_tapped_expires_permanently_on_untap_cr_611_2b() {
    let vocab = vocabulary();
    // Source creature: {T}: Gain control of target creature for as long as you control this creature
    // and this creature remains tapped.
    let mut rubinia = Program::creature(None, 2.into(), 3.into(), BTreeSet::new());
    rubinia.printed_keywords.insert(KeywordAbility::Haste);
    rubinia.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::CreateContinuousEffect {
                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                    ControlRecipient::ResolvingController,
                )]),
                duration: ResolvedDuration::WhileControllingTappedAbilitySource,
            }],
        ),
    });
    // Separate ability: {0}: Tap this creature (to test re-tapping after expiration).
    rubinia.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(2),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(BodyTarget::None, vec![Instruction::TapAbilitySource]),
    });

    let opp_bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let untap_spell = spell(
        3,
        BodyTarget::SinglePermanent,
        vec![Instruction::AffectPermanentTargets {
            minimum: 1,
            maximum: 1,
            action: spellwake_engine::program::PermanentTargetAction::Untap,
        }],
    );

    let game = world(
        vocab,
        vec![
            card(1, rubinia, true),
            opp_battlefield(2, opp_bear),
            untap_spell,
        ],
        vec![CardId(1), CardId(2)],
    );

    let rubinia_id = object(&game, 1);
    let opp_bear_id = object(&game, 2);

    // Tap Rubinia to steal P1's bear -> P0 controls bear while Rubinia is tapped.
    let act = submit(
        &game,
        P0,
        GameCommand::ActivateAbility {
            source: rubinia_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(opp_bear_id),
            payment: Some(ManaPayment::default()),
        },
    );
    let stolen = pass_pair(act.state());
    assert_eq!(current(stolen.state(), opp_bear_id).controller, Some(P0));

    // Untap Rubinia -> duration ends immediately and control returns to P1.
    let untap_cast = submit(
        stolen.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(stolen.state(), 3),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![TargetRef::Object(rubinia_id)],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let untapped = pass_pair(untap_cast.state());
    assert_eq!(current(untapped.state(), opp_bear_id).controller, Some(P1));

    // Re-tapping Rubinia must NOT restore the expired effect (CR 611.2b).
    let retap = submit(
        untapped.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: rubinia_id,
            ability: ActivatedAbilityId(2),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_retap = pass_pair(retap.state());
    assert_eq!(
        after_retap
            .state()
            .execution()
            .window()
            .cards()
            .get(rubinia_id)
            .and_then(|c| c.tapped()),
        Some(true)
    );
    assert_eq!(
        current(after_retap.state(), opp_bear_id).controller,
        Some(P1)
    );
}

#[test]
fn static_source_conditions_evaluate_counters_equipped_enchanted_host_and_exact_count() {
    let vocab = vocabulary();
    let warrior = vocab.resolve_label("Warrior").unwrap();

    // Creature with 3 gated self-buffs:
    // - Static 1: +2/+0 if it has at least 2 counters of any kind (`SourceAnyCounterAtLeast(2)`)
    // - Static 2: +0/+2 if it is equipped (`SourceIsEquipped`)
    // - Static 3: +1/+1 if you control exactly 1 creature (`ControlsExactCount`)
    let mut hero = Program::creature(None, 1.into(), 1.into(), BTreeSet::from([warrior]));
    let self_selector = PermanentSelector {
        controller: ControllerSelector::SourceItself,
        ..PermanentSelector::default()
    };
    hero.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: self_selector.clone(),
        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(2.into()),
            toughness: NumericExpression::Literal(0.into()),
        })]),
    });
    hero.static_conditions.insert(
        StaticAbilityId(1),
        StaticSourceCondition::SourceAnyCounterAtLeast(2),
    );
    hero.additional_battlefield_abilities
        .push(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(2),
            selector: self_selector.clone(),
            parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(0.into()),
                toughness: NumericExpression::Literal(2.into()),
            })]),
        });
    hero.static_conditions
        .insert(StaticAbilityId(2), StaticSourceCondition::SourceIsEquipped);
    hero.additional_battlefield_abilities
        .push(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(3),
            selector: self_selector.clone(),
            parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(1.into()),
                toughness: NumericExpression::Literal(1.into()),
            })]),
        });
    hero.static_conditions.insert(
        StaticAbilityId(3),
        StaticSourceCondition::ControlsExactCount {
            selector: Box::new(PermanentSelector {
                controller: ControllerSelector::SourceController,
                required_types: BTreeSet::from([CardType::Creature]),
                ..PermanentSelector::default()
            }),
            count: 1,
        },
    );
    hero.additional_battlefield_abilities
        .push(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(4),
            selector: self_selector,
            parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(0.into()),
                toughness: NumericExpression::Literal(3.into()),
            })]),
        });
    hero.static_conditions
        .insert(StaticAbilityId(4), StaticSourceCondition::SourceIsEnchanted);

    // Aura to trigger SourceIsEnchanted (+0/+3).
    let aura = Program::new(
        BTreeSet::from([CardType::Enchantment]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: Some(AttachmentKind::Aura(EnchantRestriction::Creature)),
            printed_pt: None,
            casting_cost: Some(ManaCost::free()),
        }),
    );

    // Equipment giving +3/+0 only if attached host is a Warrior (`AttachedHostMatches`).
    let mut blade = Program::new(
        BTreeSet::from([CardType::Artifact]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: Some(AttachmentKind::Equipment),
            printed_pt: None,
            casting_cost: Some(ManaCost::free()),
        }),
    );
    blade.artifact_subtypes.insert(ArtifactSubtype::Equipment);
    blade.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::Attach(AttachAbility::Equip)],
        ),
    });
    blade.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(10),
        selector: PermanentSelector {
            attached_to_source: true,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(3.into()),
            toughness: NumericExpression::Literal(0.into()),
        })]),
    });
    blade.static_conditions.insert(
        StaticAbilityId(10),
        StaticSourceCondition::AttachedHostMatches(Box::new(PermanentSelector {
            creature_subtype: Some(warrior),
            ..PermanentSelector::default()
        })),
    );

    // Spell putting 1 +1/+1 counter and 1 charge counter on target creature.
    let add_counters = spell(
        3,
        BodyTarget::SingleCreature,
        vec![
            Instruction::PutNamedCounterOnTarget {
                kind: CounterKind::PlusOne,
                amount: ValueExpr::literal(1),
            },
            Instruction::PutNamedCounterOnTarget {
                kind: CounterKind::Charge,
                amount: ValueExpr::literal(1),
            },
        ],
    );
    // Second creature in hand to break ControlsExactCount(1).
    let ally = Program::creature(Some(ManaCost::free()), 1.into(), 1.into(), BTreeSet::new());

    let game = world(
        vocab,
        vec![
            card(1, hero, true),
            card(2, blade, true),
            add_counters,
            card(4, ally, false),
            card(5, aura, false),
        ],
        vec![CardId(1), CardId(2)],
    );

    let hero_id = object(&game, 1);
    let blade_id = object(&game, 2);

    // Initially: base 1/1 + ControlsExactCount(1) (+1/+1) = 2/2.
    assert_eq!(
        current(&game, hero_id).power_toughness.unwrap().derived,
        pt(2, 2)
    );

    // Equip blade: triggers SourceIsEquipped (+0/+2) and AttachedHostMatches(Warrior) (+3/+0) -> 5/4.
    let eq = submit(
        &game,
        P0,
        GameCommand::ActivateAbility {
            source: blade_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(hero_id),
            payment: Some(ManaPayment::default()),
        },
    );
    let equipped = pass_pair(eq.state());
    assert_eq!(
        current(equipped.state(), hero_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(5, 4)
    );

    // Add 1 +1/+1 counter and 1 charge counter (2 total counters) -> +1/+1 from counter + +2/+0 from SourceAnyCounterAtLeast(2) = 8/5.
    let countered = cast_targeted(equipped.state(), P0, 3, TargetRef::Object(hero_id));
    assert_eq!(
        current(countered.state(), hero_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(8, 5)
    );

    // Cast second creature -> ControlsExactCount(1) turns OFF (-1/-1) -> 7/4.
    let two_creatures = cast_untargeted(countered.state(), 4);
    assert_eq!(
        current(two_creatures.state(), hero_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(7, 4)
    );

    // Cast aura enchanting hero -> SourceIsEnchanted turns ON (+0/+3) -> 7/7.
    let enchanted = cast_targeted(two_creatures.state(), P0, 5, TargetRef::Object(hero_id));
    assert_eq!(
        current(enchanted.state(), hero_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(7, 7)
    );
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
fn set_subject_tapped_handles_created_tokens_trigger_subject_and_last_moved() {
    let vocab = vocabulary();
    let tok_def = TokenDefinition::new(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &vocab,
    )
    .expect("token def");

    // 1. Create untapped token, then tap CreatedTokens via SetSubjectTapped.
    let make_and_tap = spell(
        1,
        BodyTarget::None,
        vec![
            Instruction::CreateTokens {
                creations: vec![TokenCreation {
                    enters_tapped: false,
                    definition: tok_def,
                    amount: ValueExpr::literal(1),
                    base_size: None,
                    recipients: TokenRecipients::Controller,
                }],
            },
            Instruction::SetSubjectTapped {
                subject: Box::new(EffectSubject::CreatedTokens),
                tapped: true,
            },
        ],
    );

    // 2. Reanimate from graveyard, then tap LastMoved via SetSubjectTapped.
    let reanimate_and_tap = spell(
        2,
        BodyTarget::SingleOwnGraveyardCard,
        vec![
            Instruction::MoveSubject {
                subject: MoveSubjectRef::TargetObject,
                destination: SubjectDestination::Battlefield {
                    tapped: false,
                    attacking: false,
                    transformed: false,
                    under_owner_control: false,
                    with_counter: None,
                    attach_to_self: false,
                },
            },
            Instruction::SetSubjectTapped {
                subject: Box::new(EffectSubject::LastMoved),
                tapped: true,
            },
        ],
    );

    // 3. ETB watcher that taps TriggerSubject whenever another creature enters.
    let mut watcher = Program::enchantment(None);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::enters(ObjectFilter::creature().other_than_ability_source()),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::SetSubjectTapped {
                subject: Box::new(EffectSubject::TriggerSubject),
                tapped: true,
            }],
        ),
    });

    let bear = Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());

    let game = world(
        vocab,
        vec![
            make_and_tap,
            reanimate_and_tap,
            graveyard_card(3, bear.clone()),
            card(4, watcher, true),
            card(5, bear, false),
        ],
        vec![CardId(4)],
    );

    // Test 1: CreatedTokens tapped.
    let after_tok = cast_untargeted(&game, 1);
    let token_tapped = after_tok
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .find(|c| c.zone() == ObjectZone::Battlefield && c.card_id().is_none())
        .and_then(|c| c.tapped());
    assert_eq!(token_tapped, Some(true));

    // Test 2: LastMoved tapped (on original game).
    let gy_bear = object(&game, 3);
    let after_reanim = cast_targeted(&game, P0, 2, TargetRef::Object(gy_bear));
    let reanim_tapped = after_reanim
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .find(|c| c.card_id() == Some(CardId(3)) && c.zone() == ObjectZone::Battlefield)
        .and_then(|c| c.tapped());
    assert_eq!(reanim_tapped, Some(true));

    // Test 3: TriggerSubject tapped when card 5 enters.
    let after_cast = cast_untargeted(&game, 5);
    let after_trig = pass_pair(after_cast.state());
    let entrant_tapped = after_trig
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .find(|c| c.card_id() == Some(CardId(5)) && c.zone() == ObjectZone::Battlefield)
        .and_then(|c| c.tapped());
    assert_eq!(entrant_tapped, Some(true));
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
            vec![Instruction::ShuffleTargetIntoLibrary],
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
