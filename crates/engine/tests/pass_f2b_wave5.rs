#![allow(dead_code, unused_imports)]
//! Runtime integration tests for Pass F2-B Wave 5 primitives:
//! - `StaticRestriction::IgnoreOpponentsHexproof` (Hexproof and `HexproofFrom`)
//! - `StaticRestriction::ActivateAbilitiesAsThoughHaste`
//! - `StaticRestriction::PreventMatchingNonManaActivations` vs `PreventMatchingActivations`
//! - `StaticRestriction::PreventGraveyardTargeting`
//! - `StaticRestriction::CannotBeSacrificed` (non-creature permanents, effects and costs)
//! - `StaticRestriction::CannotBeUntapped` (untap step and untap effects)
//! - `StaticRestriction::CantBeTargetedBy` (auras-only and color masks)
//! - `StaticRestriction::ControllerOneSpellPerTurn` and `OpponentsCantCastDuringCombat`
//! - `StaticDrawRule::MaxHandSize` and `NoMaximumHandSize` override
//! - `CastPermissionRule::{AllPlayersRevealLibraryTop, RevealHands,
//!   AllPlayersAdditionalLandPlays, AllSpellsCannotBeCounteredFiltered, LookAtSourceExiledCards}`
//! - `SubtypeFilter::Planeswalker` and `PermanentSelector.planeswalker_subtype`
//! - `CastEntryKind::NotCast`
//! - `Instruction::ExertAbilitySource` (`PlayerTriggerAction::Exerted` and
//!   `StatusTriggerKind::Exerted`)
//! - `EventValueKind::TriggeringSpellManaSpent`, `PlayerStatExpr::MaxPlayerLifeLostThisTurn`,
//!   and `PlayerStatExpr::OtherSpellsCastThisTurn`.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    attachment::EnchantRestriction,
    characteristics::{
        AbilityChange, CastPermissionRule, Color, ControllerSelector, HexproofQuality,
        KeywordAbility, PermanentSelector, StaticAbilityId, StaticContinuousAbility,
        StaticDrawRule, StaticPart, StaticParts, StaticRestriction,
    },
    checks::CheckDecision,
    effects::{EffectError, SeedCard},
    game::{
        DecisionReply, GameCommand, GameError, GameInput, GameState, NativeCandidate, RuleFrame,
    },
    land::LandType,
    mana::{
        CastResource, CastingPayment, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaCost,
        ManaOutput, ManaPayment, ManaSpend, ManaType,
    },
    object_identity::{CardId, ObjectDestination, ObjectZone},
    occurrence::RulesOccurrence,
    priority::SeatId,
    program::{
        AffectedPlayers, BodyTarget, CardType, CastEntryKind, ColorMask, Comparison,
        EventValueKind, ExecutableBody, Instruction, ObjectFilter, PermanentDefinition,
        PermanentTypeFilter, PlaneswalkerSubtype, PlayerRelation, PlayerStatExpr,
        PlayerTriggerAction, Program, ProgramKind, ResolutionCondition, ResolvingPlayer,
        SpellFilter, StatusTriggerKind, TriggerCondition, TriggerEvent, TriggerFromZone,
        TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId, ValueExpr,
    },
    targets::TargetRef,
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients},
    turn::{Step, TurnError},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU8,
};

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

fn graveyard_card(id: u64, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: P0,
        destination: ObjectDestination::Graveyard,
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

fn colored_destroy_instant(color: Color) -> Program {
    let mut prog = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SINGLE_CREATURE,
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
    );
    prog.color_indicator = Some(BTreeSet::from([color]));
    prog
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

fn planeswalker(subtype: PlaneswalkerSubtype, loyalty: i64) -> Program {
    let mut prog = Program::new(
        BTreeSet::from([CardType::Planeswalker]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: None,
            casting_cost: Some(ManaCost::free()),
        }),
    );
    prog.printed_loyalty = Some(loyalty.into());
    prog.planeswalker_subtypes.insert(subtype);
    prog
}

fn basic_forest() -> Program {
    Program::land(BTreeSet::from([LandType::Forest]), vec![])
}

#[test]
fn ignore_opponents_hexproof_bypasses_hexproof_and_hexproof_from() {
    let vocab = vocabulary();

    let mut hexproof_bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    hexproof_bear
        .printed_keywords
        .insert(KeywordAbility::Hexproof);

    let mut hexproof_from_red = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    hexproof_from_red
        .printed_keywords
        .insert(KeywordAbility::HexproofFrom(HexproofQuality::Color(
            Color::Red,
        )));

    let mut spotlight = Program::artifact(Some(ManaCost::free()), vec![]);
    spotlight.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::IgnoreOpponentsHexproof(Box::new(PermanentSelector {
            controller: ControllerSelector::Opponents,
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        })),
    });

    let game = world(
        vocab,
        vec![
            opp_battlefield(1, hexproof_bear),
            opp_battlefield(2, hexproof_from_red),
            card(3, colored_destroy_instant(Color::Red), false),
            card(4, colored_destroy_instant(Color::Red), false),
            card(5, spotlight, false),
        ],
        vec![CardId(1), CardId(2)],
    );

    let hex_id = object(&game, 1);
    let hex_red_id = object(&game, 2);

    // Without IgnoreOpponentsHexproof, neither opponent creature can be targeted.
    assert_eq!(
        attempt(
            &game,
            P0,
            GameCommand::CastPaid {
                spell: object(&game, 3),
                target: TargetRef::Object(hex_id),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget))
    );
    assert_eq!(
        attempt(
            &game,
            P0,
            GameCommand::CastPaid {
                spell: object(&game, 3),
                target: TargetRef::Object(hex_red_id),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget))
    );

    // Once Spotlight is on the battlefield, P0 can target and destroy both creatures.
    let with_spotlight = cast_untargeted(&game, 5);
    let after_first = cast_targeted(with_spotlight.state(), P0, 3, TargetRef::Object(hex_id));
    assert!(
        after_first
            .state()
            .execution()
            .window()
            .cards()
            .get(hex_id)
            .is_none_or(|c| c.zone() != ObjectZone::Battlefield),
        "opponent Hexproof creature should be destroyed once IgnoreOpponentsHexproof is active"
    );

    let after_second = cast_targeted(after_first.state(), P0, 4, TargetRef::Object(hex_red_id));
    assert!(
        after_second
            .state()
            .execution()
            .window()
            .cards()
            .get(hex_red_id)
            .is_none_or(|c| c.zone() != ObjectZone::Battlefield),
        "opponent HexproofFrom(Red) creature should be destroyed once IgnoreOpponentsHexproof is active"
    );
}

#[test]
fn prevent_graveyard_targeting_rejects_graveyard_card_and_creature_targets() {
    let vocab = vocabulary();

    let bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let exile_any_gy = spell(
        2,
        BodyTarget::ANY_GRAVEYARD_CARD,
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                spellwake_engine::toolbox::ZoneDest::Exile {
                    face_down: false,
                    viewer: None,
                    link_to_source: matches!(
                        None,
                        Some(spellwake_engine::program::DelayedReturnTiming::UntilSourceLeaves)
                    ),
                    delayed_return: None,
                    link_group: None,
                },
            ),
        ))],
    );
    let exile_gy_creature = spell(
        3,
        BodyTarget::ANY_GRAVEYARD_CREATURE,
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                spellwake_engine::toolbox::ZoneDest::Exile {
                    face_down: false,
                    viewer: None,
                    link_to_source: matches!(
                        None,
                        Some(spellwake_engine::program::DelayedReturnTiming::UntilSourceLeaves)
                    ),
                    delayed_return: None,
                    link_group: None,
                },
            ),
        ))],
    );

    let mut ground_seal = Program::enchantment(Some(ManaCost::free()));
    ground_seal.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::PreventGraveyardTargeting,
    });

    let game = world(
        vocab,
        vec![
            graveyard_card(1, bear),
            exile_any_gy,
            exile_gy_creature,
            card(4, ground_seal, false),
        ],
        vec![],
    );

    let gy_bear_id = object(&game, 1);

    // Baseline: without PreventGraveyardTargeting, both graveyard-targeting spells resolve cleanly.
    let exiled_a = cast_targeted(&game, P0, 2, TargetRef::Object(gy_bear_id));
    assert_eq!(
        exiled_a
            .state()
            .execution()
            .window()
            .cards()
            .get(object(exiled_a.state(), 1))
            .map(|c| c.zone()),
        Some(ObjectZone::Exile)
    );
    let exiled_b = cast_targeted(&game, P0, 3, TargetRef::Object(gy_bear_id));
    assert_eq!(
        exiled_b
            .state()
            .execution()
            .window()
            .cards()
            .get(object(exiled_b.state(), 1))
            .map(|c| c.zone()),
        Some(ObjectZone::Exile)
    );

    // Cast Ground Seal: both AnyGraveyardCard and AnyGraveyardCreature targets are now rejected.
    let sealed = cast_untargeted(&game, 4);
    assert_eq!(
        attempt(
            sealed.state(),
            P0,
            GameCommand::CastPaid {
                spell: object(sealed.state(), 2),
                target: TargetRef::Object(gy_bear_id),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget))
    );
    assert_eq!(
        attempt(
            sealed.state(),
            P0,
            GameCommand::CastPaid {
                spell: object(sealed.state(), 3),
                target: TargetRef::Object(gy_bear_id),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget))
    );
}

#[test]
fn cannot_be_sacrificed_protects_noncreature_permanent_from_effect_and_cost() {
    let vocab = vocabulary();

    let relic = Program::artifact(None, vec![]);
    let bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());

    let mut ward = Program::enchantment(None);
    ward.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::CannotBeSacrificed(Box::new(PermanentSelector {
            required_types: BTreeSet::from([CardType::Artifact]),
            ..PermanentSelector::default()
        })),
    });

    let force_sac_artifact = spell(
        4,
        BodyTarget::None,
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
                AffectedPlayers::One(ResolvingPlayer::Controller),
                ValueExpr::literal(1),
                PermanentTypeFilter {
                    any_types: BTreeSet::from([CardType::Artifact]),
                    required_types: BTreeSet::new(),
                    forbidden_types: BTreeSet::new(),
                    object_filter: None,
                },
            ),
        ))],
    );

    let mut sac_cost_spell = gain_life_instant(3);
    sac_cost_spell.sacrifice_creature_cast_cost = true;
    sac_cost_spell.sacrifice_cast_cost_filter = Some(ObjectFilter::artifact());

    let game = world(
        vocab,
        vec![
            card(1, relic, true),
            card(2, bear, true),
            card(3, ward, true),
            force_sac_artifact,
            card(5, sac_cost_spell, false),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );

    let relic_id = object(&game, 1);
    let bear_id = object(&game, 2);

    let evaluated = game.execution().evaluated_characteristics().unwrap();
    assert!(evaluated.sacrifice_prohibited(relic_id));
    assert!(!evaluated.sacrifice_prohibited(bear_id));

    // Sacrifice effect resolves with no eligible artifact to sacrifice; relic stays on battlefield.
    let after_effect = cast_untargeted(&game, 4);
    assert!(after_effect.state().pending_decision().is_none());
    assert_eq!(
        after_effect
            .state()
            .execution()
            .window()
            .cards()
            .get(relic_id)
            .map(|c| c.zone()),
        Some(ObjectZone::Battlefield)
    );

    // Paying a sacrifice casting cost with the protected artifact is rejected.
    assert_eq!(
        attempt(
            &game,
            P0,
            GameCommand::CastWithResources {
                spell: object(&game, 5),
                target: None,
                payment: CastingPayment {
                    mana: Some(ManaPayment::default()),
                    objects: vec![CastResource::SacrificeForSpell(relic_id)],
                    ..CastingPayment::default()
                },
            },
        ),
        Err(GameError::Effect(EffectError::InvalidCastResource))
    );
}

#[test]
fn cant_be_targeted_by_enforces_aura_only_and_color_mask_restrictions() {
    let vocab = vocabulary();

    let bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let creature_selector = PermanentSelector {
        required_types: BTreeSet::from([CardType::Creature]),
        ..PermanentSelector::default()
    };

    // Case A: auras_only = true blocks Aura spells, permits non-Aura spells.
    let mut anti_aura = Program::enchantment(None);
    anti_aura.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::CantBeTargetedBy {
            selector: Box::new(creature_selector.clone()),
            auras_only: true,
            colors: ColorMask::EMPTY,
        },
    });
    let aura = Program::aura(Some(ManaCost::free()), EnchantRestriction::Creature);
    let destroy = spell(
        4,
        BodyTarget::SINGLE_CREATURE,
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
    );

    let game_aura = world(
        vocab.clone(),
        vec![
            card(1, bear.clone(), true),
            card(2, anti_aura, true),
            card(3, aura, false),
            destroy,
        ],
        vec![CardId(1), CardId(2)],
    );
    let bear_id_a = object(&game_aura, 1);

    assert_eq!(
        attempt(
            &game_aura,
            P0,
            GameCommand::CastPaid {
                spell: object(&game_aura, 3),
                target: TargetRef::Object(bear_id_a),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget))
    );
    let destroyed_a = cast_targeted(&game_aura, P0, 4, TargetRef::Object(bear_id_a));
    assert!(
        destroyed_a
            .state()
            .execution()
            .window()
            .cards()
            .get(bear_id_a)
            .is_none_or(|c| c.zone() != ObjectZone::Battlefield),
        "non-Aura spell must still legally target and destroy the creature"
    );

    // Case B: colors = ColorMask::WHITE blocks white spells, permits red spells.
    let mut anti_white = Program::enchantment(None);
    anti_white.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::CantBeTargetedBy {
            selector: Box::new(creature_selector),
            auras_only: false,
            colors: ColorMask::WHITE,
        },
    });

    let game_color = world(
        vocab,
        vec![
            card(1, bear, true),
            card(2, anti_white, true),
            card(3, colored_destroy_instant(Color::White), false),
            card(4, colored_destroy_instant(Color::Red), false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let bear_id_b = object(&game_color, 1);

    assert_eq!(
        attempt(
            &game_color,
            P0,
            GameCommand::CastPaid {
                spell: object(&game_color, 3),
                target: TargetRef::Object(bear_id_b),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget))
    );
    let destroyed_b = cast_targeted(&game_color, P0, 4, TargetRef::Object(bear_id_b));
    assert!(
        destroyed_b
            .state()
            .execution()
            .window()
            .cards()
            .get(bear_id_b)
            .is_none_or(|c| c.zone() != ObjectZone::Battlefield),
        "red spell must still legally target and destroy the creature"
    );
}

#[test]
fn controller_one_spell_per_turn_and_opponents_cant_cast_during_combat() {
    let vocab = vocabulary();

    // Part A: ControllerOneSpellPerTurn caps controller at 1 spell while opponent can cast 2.
    let mut moderation = Program::enchantment(None);
    moderation.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::ControllerOneSpellPerTurn,
    });
    let game_one = world(
        vocab.clone(),
        vec![
            card(1, moderation, true),
            card(2, gain_life_instant(1), false),
            card(3, gain_life_instant(1), false),
            opp_hand(4, gain_life_instant(1)),
            opp_hand(5, gain_life_instant(1)),
        ],
        vec![CardId(1)],
    );

    let after_p0_first = cast_untargeted(&game_one, 2);
    assert_eq!(
        after_p0_first.state().execution().life_total(P0),
        Some(&21.into())
    );

    assert_eq!(
        attempt(
            after_p0_first.state(),
            P0,
            GameCommand::CastUntargeted {
                spell: object(after_p0_first.state(), 3),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::SpellCastLimitReached))
    );

    // Opponent casts 2 spells in the same turn without hitting ControllerOneSpellPerTurn.
    let p1_prio1 = submit(after_p0_first.state(), P0, GameCommand::Pass);
    let p1_cast1 = submit(
        p1_prio1.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(p1_prio1.state(), 4),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_p1_first = pass_in_order(p1_cast1.state(), [P1, P0]);
    let p1_prio2 = submit(after_p1_first.state(), P0, GameCommand::Pass);
    let p1_cast2 = submit(
        p1_prio2.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(p1_prio2.state(), 5),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_p1_second = pass_in_order(p1_cast2.state(), [P1, P0]);
    assert_eq!(
        after_p1_second.state().execution().life_total(P1),
        Some(&22.into())
    );

    // Part B: OpponentsCantCastDuringCombat blocks opponent in DeclareAttackers while controller can cast.
    let mut marisi = Program::enchantment(None);
    marisi.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::OpponentsCantCastDuringCombat,
    });
    let game_combat = world(
        vocab,
        vec![
            card(1, marisi, true),
            card(2, gain_life_instant(1), false),
            opp_hand(3, gain_life_instant(1)),
        ],
        vec![CardId(1)],
    );

    let beg_combat = pass_pair(&game_combat);
    let dec_attackers = pass_pair(beg_combat.state());
    assert_eq!(
        dec_attackers.state().turn().unwrap().position().step,
        Step::DeclareAttackers
    );

    // Controller can still cast an instant during DeclareAttackers.
    let p0_combat_cast = cast_untargeted(dec_attackers.state(), 2);
    assert_eq!(
        p0_combat_cast.state().execution().life_total(P0),
        Some(&21.into())
    );

    // Pass priority to opponent in DeclareAttackers; opponent's instant cast is prohibited.
    let p1_combat = submit(p0_combat_cast.state(), P0, GameCommand::Pass);
    assert_eq!(
        p1_combat.state().turn().unwrap().position().step,
        Step::DeclareAttackers
    );
    assert_eq!(
        attempt(
            p1_combat.state(),
            P1,
            GameCommand::CastUntargeted {
                spell: object(p1_combat.state(), 3),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::OpponentActionProhibited))
    );
}

#[test]
fn max_hand_size_enforces_cleanup_discard_and_is_overridden_by_no_max_hand_size() {
    let vocab = vocabulary();

    let mut recycle = Program::enchantment(None);
    recycle.continuous = Some(StaticContinuousAbility::DrawRule {
        id: StaticAbilityId(1),
        rule: StaticDrawRule::MaxHandSize(2),
    });

    let mut reliquary = Program::artifact(None, vec![]);
    reliquary.continuous = Some(StaticContinuousAbility::DrawRule {
        id: StaticAbilityId(1),
        rule: StaticDrawRule::NoMaximumHandSize,
    });

    let hand_cards = || -> Vec<SeedCard> {
        (1..=5)
            .map(|id| card(id, gain_life_instant(1), false))
            .collect()
    };

    // Case A: 5 cards in hand with MaxHandSize(2) -> cleanup requires discarding 3 cards down to 2.
    let mut seeds_a = hand_cards();
    seeds_a.push(card(6, recycle.clone(), true));
    let game_a = world(vocab.clone(), seeds_a, vec![CardId(6)]);

    let end_a = reach_end(&game_a, [P0, P1]);
    let cleanup_pending = pass_pair(&end_a);
    let pending = cleanup_pending
        .state()
        .pending_decision()
        .expect("cleanup discard decision expected");
    let RuleFrame::Checks(CheckDecision::CleanupDiscard(frame)) = pending.frame() else {
        panic!("expected CleanupDiscard frame");
    };
    assert_eq!(frame.chooser(), P0);
    assert_eq!(frame.required(), 3);
    assert_eq!(frame.domain().len(), 5);

    let discarded = reply(
        cleanup_pending.state(),
        P0,
        DecisionReply::Discard(frame.domain()[..3].to_vec()),
    );
    assert!(discarded.state().pending_decision().is_none());
    assert_eq!(discarded.state().turn().unwrap().position().number, 2);
    assert_eq!(
        discarded.state().turn().unwrap().position().step,
        Step::Upkeep
    );
    let remaining_hand = discarded
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(P0))
        .count();
    assert_eq!(remaining_hand, 2);

    // Case B: NoMaximumHandSize overrides MaxHandSize(2) -> no discard required at cleanup.
    let mut seeds_b = hand_cards();
    seeds_b.push(card(6, recycle, true));
    seeds_b.push(card(7, reliquary, true));
    let game_b = world(vocab, seeds_b, vec![CardId(6), CardId(7)]);

    let end_b = reach_end(&game_b, [P0, P1]);
    let cleanup_done = pass_pair(&end_b);
    assert!(cleanup_done.state().pending_decision().is_none());
    assert_eq!(cleanup_done.state().turn().unwrap().position().number, 2);
    assert_eq!(
        cleanup_done.state().turn().unwrap().position().step,
        Step::Upkeep
    );
    let still_in_hand = cleanup_done
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(P0))
        .count();
    assert_eq!(still_in_hand, 5);
}

#[test]
fn cast_permission_reveal_library_top_and_hands() {
    let vocab = vocabulary();

    let bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());

    // Part A: AllPlayersRevealLibraryTop reveals both players' library tops to both viewers.
    let mut lantern = Program::artifact(Some(ManaCost::free()), vec![]);
    lantern.continuous = Some(StaticContinuousAbility::CastPermission {
        id: StaticAbilityId(1),
        permission: CastPermissionRule::AllPlayersRevealLibraryTop,
    });

    let game_top = world(
        vocab.clone(),
        vec![
            library_top(1, P0, bear.clone()),
            library_top(2, P1, bear),
            card(3, lantern, false),
        ],
        vec![],
    );
    let eval_before = game_top.execution().evaluated_characteristics().unwrap();
    for owner in [P0, P1] {
        for viewer in [P0, P1] {
            assert!(
                !game_top
                    .execution()
                    .library_top_access(&eval_before, owner, viewer)
                    .visible,
                "library top should be hidden before AllPlayersRevealLibraryTop enters"
            );
        }
    }

    let with_lantern = cast_untargeted(&game_top, 3);
    let eval_after = with_lantern
        .state()
        .execution()
        .evaluated_characteristics()
        .unwrap();
    for owner in [P0, P1] {
        for viewer in [P0, P1] {
            assert!(
                with_lantern
                    .state()
                    .execution()
                    .library_top_access(&eval_after, owner, viewer)
                    .visible,
                "AllPlayersRevealLibraryTop must make every player's library top visible to every viewer"
            );
        }
    }

    // Part B: RevealHands { opponents_only: true } vs { opponents_only: false }.
    let mut telepathy = Program::enchantment(None);
    telepathy.continuous = Some(StaticContinuousAbility::CastPermission {
        id: StaticAbilityId(1),
        permission: CastPermissionRule::RevealHands {
            opponents_only: true,
        },
    });
    let game_opp_hands = world(
        vocab.clone(),
        vec![card(1, telepathy, true)],
        vec![CardId(1)],
    );
    let eval_opp = game_opp_hands
        .execution()
        .evaluated_characteristics()
        .unwrap();
    assert!(
        game_opp_hands
            .execution()
            .hand_revealed_to(&eval_opp, P1, P0)
    );
    assert!(
        !game_opp_hands
            .execution()
            .hand_revealed_to(&eval_opp, P0, P1)
    );
    assert!(
        game_opp_hands
            .execution()
            .hand_revealed_to(&eval_opp, P0, P0)
    );
    assert!(
        game_opp_hands
            .execution()
            .hand_revealed_to(&eval_opp, P1, P1)
    );

    let mut revelation = Program::enchantment(None);
    revelation.continuous = Some(StaticContinuousAbility::CastPermission {
        id: StaticAbilityId(1),
        permission: CastPermissionRule::RevealHands {
            opponents_only: false,
        },
    });
    let game_all_hands = world(vocab, vec![card(1, revelation, true)], vec![CardId(1)]);
    let eval_all = game_all_hands
        .execution()
        .evaluated_characteristics()
        .unwrap();
    assert!(
        game_all_hands
            .execution()
            .hand_revealed_to(&eval_all, P1, P0)
    );
    assert!(
        game_all_hands
            .execution()
            .hand_revealed_to(&eval_all, P0, P1)
    );
    assert!(
        game_all_hands
            .execution()
            .hand_revealed_to(&eval_all, P0, P0)
    );
    assert!(
        game_all_hands
            .execution()
            .hand_revealed_to(&eval_all, P1, P1)
    );
}

#[test]
fn cast_permission_all_players_additional_land_plays_in_additional_abilities() {
    let vocab = vocabulary();

    // Place AllPlayersAdditionalLandPlays in additional_battlefield_abilities to verify it is
    // honored outside the primary continuous slot.
    let mut rites = Program::enchantment(None);
    rites
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(2),
            permission: CastPermissionRule::AllPlayersAdditionalLandPlays(
                NonZeroU8::new(1).unwrap(),
            ),
        });

    let game = world(
        vocab.clone(),
        vec![
            card(1, rites, true),
            card(2, basic_forest(), false),
            card(3, basic_forest(), false),
            card(4, basic_forest(), false),
            opp_hand(5, basic_forest()),
            opp_hand(6, basic_forest()),
            opp_hand(7, basic_forest()),
            library(8, P1, &vocab),
        ],
        vec![CardId(1)],
    );

    // Turn 1 (P0): allowed 2 land plays; 3rd fails LandLimitReached.
    let p0_land1 = submit(
        &game,
        P0,
        GameCommand::PlayLand {
            land: object(&game, 2),
        },
    );
    let p0_land2 = submit(
        p0_land1.state(),
        P0,
        GameCommand::PlayLand {
            land: object(p0_land1.state(), 3),
        },
    );
    assert_eq!(
        attempt(
            p0_land2.state(),
            P0,
            GameCommand::PlayLand {
                land: object(p0_land2.state(), 4),
            },
        ),
        Err(GameError::Turn(TurnError::LandLimitReached))
    );

    // Advance to Turn 2 PrecombatMain (P1 active).
    let t1_end = reach_end(p0_land2.state(), [P0, P1]);
    let t2_upkeep = pass_pair(&t1_end);
    let t2_draw = pass_in_order(t2_upkeep.state(), [P1, P0]);
    let t2_main = pass_in_order(t2_draw.state(), [P1, P0]);
    assert_eq!(
        t2_main.state().turn().unwrap().position().step,
        Step::PrecombatMain
    );

    // Turn 2 (P1): also allowed 2 land plays; 3rd fails LandLimitReached.
    let p1_land1 = submit(
        t2_main.state(),
        P1,
        GameCommand::PlayLand {
            land: object(t2_main.state(), 5),
        },
    );
    let p1_land2 = submit(
        p1_land1.state(),
        P1,
        GameCommand::PlayLand {
            land: object(p1_land1.state(), 6),
        },
    );
    assert_eq!(
        attempt(
            p1_land2.state(),
            P1,
            GameCommand::PlayLand {
                land: object(p1_land2.state(), 7),
            },
        ),
        Err(GameError::Turn(TurnError::LandLimitReached))
    );
}

#[test]
fn planeswalker_subtype_filter_and_permanent_selector_match_jace_and_reject_chandra() {
    let vocab = vocabulary();

    let jace = planeswalker(PlaneswalkerSubtype::Jace, 4);
    let chandra = planeswalker(PlaneswalkerSubtype::Chandra, 4);

    // PermanentSelector.planeswalker_subtype grants Indestructible only to Jace.
    let mut jace_boon = Program::enchantment(None);
    jace_boon.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            planeswalker_subtype: Some(PlaneswalkerSubtype::Jace),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
            [KeywordAbility::Indestructible],
        ))]),
    });

    // ObjectFilter with SubtypeFilter::Planeswalker(Jace) targets only Jace.
    let exile_jace = spell(
        4,
        BodyTarget::filtered_permanent(
            ObjectFilter::planeswalker().with_planeswalker_subtype(PlaneswalkerSubtype::Jace),
        ),
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                spellwake_engine::toolbox::ZoneDest::exile(),
            ),
        ))],
    );

    let initial = world(
        vocab,
        vec![
            card(1, jace, false),
            card(2, chandra, false),
            card(3, jace_boon, true),
            exile_jace,
        ],
        vec![CardId(3)],
    );
    let after_jace = cast_untargeted(&initial, 1);
    let after_chandra = cast_untargeted(after_jace.state(), 2);
    let game = after_chandra.state();

    let jace_id = object(game, 1);
    let chandra_id = object(game, 2);

    assert!(
        current(game, jace_id)
            .keywords
            .contains(&KeywordAbility::Indestructible),
        "PermanentSelector with planeswalker_subtype Jace must match Jace"
    );
    assert!(
        !current(game, chandra_id)
            .keywords
            .contains(&KeywordAbility::Indestructible),
        "PermanentSelector with planeswalker_subtype Jace must reject Chandra"
    );

    // ObjectFilter with SubtypeFilter::Planeswalker(Jace) rejects Chandra and accepts Jace.
    assert_eq!(
        attempt(
            game,
            P0,
            GameCommand::CastPaid {
                spell: object(game, 4),
                target: TargetRef::Object(chandra_id),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget))
    );
    let exiled = cast_targeted(game, P0, 4, TargetRef::Object(jace_id));
    assert_eq!(
        exiled
            .state()
            .execution()
            .window()
            .cards()
            .get(object(exiled.state(), 1))
            .map(|c| c.zone()),
        Some(ObjectZone::Exile)
    );
}
