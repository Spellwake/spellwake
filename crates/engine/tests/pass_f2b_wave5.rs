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
        EventValueKind, ExecutableBody, Instruction, MoveSubjectRef, ObjectFilter,
        PermanentDefinition, PermanentTargetAction, PermanentTypeFilter, PlaneswalkerSubtype,
        PlayerRelation, PlayerStatExpr, PlayerTriggerAction, Program, ProgramKind,
        ResolutionCondition, ResolvingPlayer, SpellFilter, StatusTriggerKind, SubjectDestination,
        TriggerCondition, TriggerEvent, TriggerFromZone, TriggerGrouping, TriggerSpec,
        TriggeredAbility, TriggeredAbilityId, ValueExpr,
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
        ExecutableBody::new(BodyTarget::SingleCreature, vec![Instruction::DestroyTarget]),
    );
    prog.color_indicator = Some(BTreeSet::from([color]));
    prog
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
fn activate_abilities_as_though_haste_bypasses_summoning_sickness_tap_restriction() {
    let vocab = vocabulary();

    let mut tapper = Program::creature(Some(ManaCost::free()), 1.into(), 1.into(), BTreeSet::new());
    tapper.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(2),
            }],
        ),
    });

    let mut elixir = Program::artifact(Some(ManaCost::free()), vec![]);
    elixir.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::ActivateAbilitiesAsThoughHaste(Box::new(PermanentSelector {
            controller: ControllerSelector::SourceController,
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        })),
    });

    let game = world(
        vocab,
        vec![card(1, tapper, false), card(2, elixir, false)],
        vec![],
    );

    // Cast creature so it enters this turn with summoning sickness.
    let entered = cast_untargeted(&game, 1);
    let tapper_id = object(entered.state(), 1);
    assert_eq!(
        entered
            .state()
            .execution()
            .window()
            .cards()
            .get(tapper_id)
            .and_then(|c| c.has_continuous_control_since_turn_start()),
        Some(false)
    );

    // Activating {T} ability while summoning sick is rejected.
    assert_eq!(
        attempt(
            entered.state(),
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: tapper_id,
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::CreatureTapRestricted))
    );

    // Cast Elixir granting ActivateAbilitiesAsThoughHaste; activation now succeeds.
    let with_elixir = cast_untargeted(entered.state(), 2);
    let life_before = with_elixir
        .state()
        .execution()
        .life_total(P0)
        .cloned()
        .unwrap();
    let act = submit(
        with_elixir.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: tapper_id,
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let resolved = pass_pair(act.state());
    assert_eq!(
        resolved
            .state()
            .execution()
            .window()
            .cards()
            .get(tapper_id)
            .and_then(|c| c.tapped()),
        Some(true)
    );
    assert_eq!(
        resolved.state().execution().life_total(P0),
        Some(&life_before.plus(&2.into()))
    );
}

#[test]
fn prevent_matching_non_mana_vs_all_activations_distinguishes_mana_abilities() {
    let vocab = vocabulary();

    let mut rock = Program::artifact(
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
    );
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
    });

    let artifact_selector = PermanentSelector {
        required_types: BTreeSet::from([CardType::Artifact]),
        ..PermanentSelector::default()
    };

    // Case A: PreventMatchingNonManaActivations blocks non-mana ability but allows mana ability.
    let mut matrix = Program::enchantment(None);
    matrix.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::PreventMatchingNonManaActivations(Box::new(
            artifact_selector.clone(),
        )),
    });
    let game_non_mana = world(
        vocab.clone(),
        vec![card(1, rock.clone(), true), card(2, matrix, true)],
        vec![CardId(1), CardId(2)],
    );
    let rock_id_a = object(&game_non_mana, 1);

    assert_eq!(
        attempt(
            &game_non_mana,
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: rock_id_a,
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::ActivationConditionNotMet))
    );
    let mana_ok = submit(
        &game_non_mana,
        P0,
        GameCommand::ActivateMana {
            source: rock_id_a,
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(
        mana_ok.state().execution().mana(P0, ManaType::Green),
        1.into()
    );

    // Case B: PreventMatchingActivations blocks BOTH non-mana and mana abilities.
    let mut ouphe_lock = Program::enchantment(None);
    ouphe_lock.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::PreventMatchingActivations(Box::new(artifact_selector)),
    });
    let game_all = world(
        vocab,
        vec![card(1, rock, true), card(2, ouphe_lock, true)],
        vec![CardId(1), CardId(2)],
    );
    let rock_id_b = object(&game_all, 1);

    assert_eq!(
        attempt(
            &game_all,
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: rock_id_b,
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::ActivationConditionNotMet))
    );
    assert_eq!(
        attempt(
            &game_all,
            P0,
            GameCommand::ActivateMana {
                source: rock_id_b,
                ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
            },
        ),
        Err(GameError::Effect(
            EffectError::ManaActivationConditionNotMet
        ))
    );
}

#[test]
fn prevent_graveyard_targeting_rejects_graveyard_card_and_creature_targets() {
    let vocab = vocabulary();

    let bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let exile_any_gy = spell(
        2,
        BodyTarget::AnyGraveyardCard,
        vec![Instruction::MoveSubject {
            subject: MoveSubjectRef::TargetObject,
            destination: SubjectDestination::Exile {
                delayed_return: None,
            },
        }],
    );
    let exile_gy_creature = spell(
        3,
        BodyTarget::AnyGraveyardCreature,
        vec![Instruction::MoveSubject {
            subject: MoveSubjectRef::TargetObject,
            destination: SubjectDestination::Exile {
                delayed_return: None,
            },
        }],
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
        vec![Instruction::Sacrifice {
            players: AffectedPlayers::One(ResolvingPlayer::Controller),
            amount: ValueExpr::literal(1),
            filter: PermanentTypeFilter {
                any_types: BTreeSet::from([CardType::Artifact]),
                required_types: BTreeSet::new(),
                forbidden_types: BTreeSet::new(),
                object_filter: None,
            },
        }],
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
fn cannot_be_untapped_blocks_untap_step_and_untap_effect() {
    let vocab = vocabulary();

    let tap_ability = || ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(1),
            }],
        ),
    };

    let mut frozen_artifact = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    frozen_artifact.card_types.insert(CardType::Artifact);
    frozen_artifact.activated.push(tap_ability());

    let mut normal_bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    normal_bear.activated.push(tap_ability());

    let mut frost_lock = Program::enchantment(None);
    frost_lock.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::CannotBeUntapped(Box::new(PermanentSelector {
            required_types: BTreeSet::from([CardType::Artifact]),
            ..PermanentSelector::default()
        })),
    });

    let untap_spell = spell(
        4,
        BodyTarget::SinglePermanent,
        vec![Instruction::AffectPermanentTargets {
            minimum: 1,
            maximum: 1,
            action: PermanentTargetAction::Untap,
        }],
    );

    let game = world(
        vocab.clone(),
        vec![
            card(1, frozen_artifact, true),
            card(2, normal_bear, true),
            card(3, frost_lock, true),
            untap_spell,
            library(5, P0, &vocab),
            library(6, P1, &vocab),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );

    let frozen_id = object(&game, 1);
    let bear_id = object(&game, 2);

    // Tap both creatures.
    let tap1 = submit(
        &game,
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: frozen_id,
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_tap1 = pass_pair(tap1.state());
    let tap2 = submit(
        after_tap1.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: bear_id,
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let both_tapped = pass_pair(tap2.state());
    assert_eq!(
        both_tapped
            .state()
            .execution()
            .window()
            .cards()
            .get(frozen_id)
            .and_then(|c| c.tapped()),
        Some(true)
    );
    assert_eq!(
        both_tapped
            .state()
            .execution()
            .window()
            .cards()
            .get(bear_id)
            .and_then(|c| c.tapped()),
        Some(true)
    );

    // Attempting to untap the locked permanent via an untap effect leaves it tapped.
    let untap_cast = submit(
        both_tapped.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(both_tapped.state(), 4),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![TargetRef::Object(frozen_id)],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let after_untap_spell = pass_pair(untap_cast.state());
    assert_eq!(
        after_untap_spell
            .state()
            .execution()
            .window()
            .cards()
            .get(frozen_id)
            .and_then(|c| c.tapped()),
        Some(true),
        "permanent with CannotBeUntapped must not untap via untap_for_effect_with_stun"
    );

    // Advance through Turn 1 cleanup -> Turn 2 (P1) -> Turn 3 Upkeep (after P0's untap step).
    let t1_end = reach_end(after_untap_spell.state(), [P0, P1]);
    let t2_upkeep = pass_pair(&t1_end);
    let t2_draw = pass_in_order(t2_upkeep.state(), [P1, P0]);
    let t2_main = pass_in_order(t2_draw.state(), [P1, P0]);
    let t2_end = reach_end(t2_main.state(), [P1, P0]);
    let t3_upkeep = pass_in_order(&t2_end, [P1, P0]);
    assert_eq!(t3_upkeep.state().turn().unwrap().position().number, 3);
    assert_eq!(
        t3_upkeep.state().turn().unwrap().position().step,
        Step::Upkeep
    );

    assert_eq!(
        t3_upkeep
            .state()
            .execution()
            .window()
            .cards()
            .get(bear_id)
            .and_then(|c| c.tapped()),
        Some(false),
        "unrestricted tapped creature must untap during controller's untap step"
    );
    assert_eq!(
        t3_upkeep
            .state()
            .execution()
            .window()
            .cards()
            .get(frozen_id)
            .and_then(|c| c.tapped()),
        Some(true),
        "permanent with CannotBeUntapped must remain tapped after controller's untap step"
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
        BodyTarget::SingleCreature,
        vec![Instruction::DestroyTarget],
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
fn cast_permission_all_spells_cannot_be_countered_and_look_at_source_exiled() {
    let vocab = vocabulary();

    // Part A: AllSpellsCannotBeCounteredFiltered protects opponent's matching creature spell
    // while non-matching instant spells can still be countered.
    let mut herald = Program::enchantment(None);
    herald.continuous = Some(StaticContinuousAbility::CastPermission {
        id: StaticAbilityId(1),
        permission: CastPermissionRule::AllSpellsCannotBeCounteredFiltered(Box::new(
            SpellFilter::creature(),
        )),
    });

    let mut flash_bear =
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
    flash_bear.printed_keywords.insert(KeywordAbility::Flash);

    let counterspell = |id: u64| {
        spell(
            id,
            BodyTarget::SingleSpell(SpellFilter::any()),
            vec![Instruction::CounterSpell],
        )
    };

    let game_counter = world(
        vocab.clone(),
        vec![
            card(1, herald, true),
            opp_hand(2, flash_bear),
            opp_hand(3, gain_life_instant(5)),
            counterspell(4),
            counterspell(5),
        ],
        vec![CardId(1)],
    );

    // P1 casts creature spell with Flash; P0 attempts to counter it -> counter resolves with no effect.
    let p1_prio = submit(&game_counter, P0, GameCommand::Pass);
    let p1_cast_creature = submit(
        p1_prio.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(p1_prio.state(), 2),
            payment: Some(ManaPayment::default()),
        },
    );
    let p0_resp = submit(p1_cast_creature.state(), P1, GameCommand::Pass);
    let creature_entry = p0_resp.state().execution().window().entries()[0].id();
    let p0_counter = submit(
        p0_resp.state(),
        P0,
        GameCommand::CastPaid {
            spell: object(p0_resp.state(), 4),
            target: TargetRef::Stack(creature_entry),
            payment: ManaPayment::default(),
        },
    );
    let after_counter = pass_pair(p0_counter.state());
    assert_eq!(
        after_counter.state().execution().window().entries().len(),
        1,
        "opponent's matching creature spell must remain on the stack after counterspell resolves"
    );
    let after_creature = pass_pair(after_counter.state());
    assert_eq!(
        after_creature
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_creature.state(), 2))
            .map(|c| (c.zone(), c.controller())),
        Some((ObjectZone::Battlefield, Some(P1)))
    );

    // Opponent's non-creature instant is NOT protected and gets countered.
    let p1_life_before = after_creature
        .state()
        .execution()
        .life_total(P1)
        .cloned()
        .unwrap();
    let p1_prio2 = submit(after_creature.state(), P0, GameCommand::Pass);
    let p1_cast_inst = submit(
        p1_prio2.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(p1_prio2.state(), 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let p0_resp2 = submit(p1_cast_inst.state(), P1, GameCommand::Pass);
    let inst_entry = p0_resp2.state().execution().window().entries()[0].id();
    let p0_counter2 = submit(
        p0_resp2.state(),
        P0,
        GameCommand::CastPaid {
            spell: object(p0_resp2.state(), 5),
            target: TargetRef::Stack(inst_entry),
            payment: ManaPayment::default(),
        },
    );
    let after_counter2 = pass_pair(p0_counter2.state());
    assert!(
        after_counter2
            .state()
            .execution()
            .window()
            .entries()
            .is_empty(),
        "non-creature instant should have been countered"
    );
    assert_eq!(
        after_counter2.state().execution().life_total(P1),
        Some(&p1_life_before)
    );

    // Part B: LookAtSourceExiledCards grants viewer permission without cast permission.
    let mut vault = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::ExileTargetUntilSourceLeaves],
            ),
        }],
    );
    vault.continuous = Some(StaticContinuousAbility::CastPermission {
        id: StaticAbilityId(1),
        permission: CastPermissionRule::LookAtSourceExiledCards,
    });
    let opp_bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());

    let game_exile = world(
        vocab,
        vec![card(10, vault, true), opp_battlefield(11, opp_bear)],
        vec![CardId(10), CardId(11)],
    );
    let vault_id = object(&game_exile, 10);
    let opp_bear_id = object(&game_exile, 11);

    let act = submit(
        &game_exile,
        P0,
        GameCommand::ActivateAbility {
            source: vault_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(opp_bear_id),
            payment: Some(ManaPayment::default()),
        },
    );
    let exiled = pass_pair(act.state());
    let exiled_id = object(exiled.state(), 11);
    assert_eq!(
        exiled
            .state()
            .execution()
            .window()
            .cards()
            .get(exiled_id)
            .map(|c| c.zone()),
        Some(ObjectZone::Exile)
    );
    assert_eq!(
        exiled.state().execution().face_down_exile_viewer(exiled_id),
        Some(P0)
    );
    assert!(
        !exiled
            .state()
            .execution()
            .may_cast_from_face_down_exile(exiled_id, P0),
        "LookAtSourceExiledCards must not grant permission to cast the exiled card"
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
        BodyTarget::FilteredPermanent(
            ObjectFilter::planeswalker().with_planeswalker_subtype(PlaneswalkerSubtype::Jace),
        ),
        vec![Instruction::ExileTarget],
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

#[test]
fn cast_entry_kind_not_cast_triggers_on_token_and_reanimate_but_not_cast() {
    let vocab = vocabulary();

    let mut watcher = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::new(TriggerEvent::Enters {
            subject: ObjectFilter::creature().other_than_ability_source(),
            from_zone: TriggerFromZone::Any,
            cast_origin: Some(CastEntryKind::NotCast),
            attached_to_creature: false,
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(3),
            }],
        ),
    });

    let castable_bear =
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
    let tok_def = TokenDefinition::new(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &vocab,
    )
    .expect("token def");
    let make_token = spell(
        3,
        BodyTarget::None,
        vec![Instruction::CreateTokens {
            creations: vec![TokenCreation {
                enters_tapped: false,
                definition: tok_def,
                amount: ValueExpr::literal(1),
                base_size: None,
                recipients: TokenRecipients::Controller,
            }],
        }],
    );
    let reanimate = spell(
        5,
        BodyTarget::SingleOwnGraveyardCard,
        vec![Instruction::MoveSubject {
            subject: MoveSubjectRef::TargetObject,
            destination: SubjectDestination::Battlefield {
                tapped: false,
                attacking: false,
                transformed: false,
                under_owner_control: false,
                with_counter: None,
                attach_to_self: false,
            },
        }],
    );

    let game = world(
        vocab,
        vec![
            card(1, watcher, true),
            card(2, castable_bear.clone(), false),
            make_token,
            graveyard_card(4, castable_bear),
            reanimate,
        ],
        vec![CardId(1)],
    );

    // 1. Casting a creature normally must NOT fire the CastEntryKind::NotCast trigger.
    let after_cast = cast_untargeted(&game, 2);
    assert!(
        after_cast.state().execution().window().entries().is_empty(),
        "normally cast creature must not trigger CastEntryKind::NotCast"
    );
    assert_eq!(
        after_cast.state().execution().life_total(P0),
        Some(&20.into())
    );

    // 2. Creating a creature token DOES fire the trigger (+3 life).
    let after_token = cast_untargeted(after_cast.state(), 3);
    assert_eq!(after_token.state().execution().window().entries().len(), 1);
    let after_token_trig = pass_pair(after_token.state());
    assert_eq!(
        after_token_trig.state().execution().life_total(P0),
        Some(&23.into())
    );

    // 3. Reanimating a creature from the graveyard without casting DOES fire the trigger (+3 life).
    let gy_bear_id = object(after_token_trig.state(), 4);
    let after_reanim = cast_targeted(
        after_token_trig.state(),
        P0,
        5,
        TargetRef::Object(gy_bear_id),
    );
    assert_eq!(after_reanim.state().execution().window().entries().len(), 1);
    let after_reanim_trig = pass_pair(after_reanim.state());
    assert_eq!(
        after_reanim_trig.state().execution().life_total(P0),
        Some(&26.into())
    );
}

#[test]
fn exert_ability_source_emits_exerted_occurrence_and_fires_status_and_player_triggers() {
    let vocab = vocabulary();

    let exert_ability = || ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(BodyTarget::None, vec![Instruction::ExertAbilitySource]),
    };

    // Case A: Instruction::ExertAbilitySource emits PlayerTriggerAction::Exerted occurrence
    // and fires a self StatusTriggerKind::Exerted trigger.
    let mut exerter = Program::creature(None, 3.into(), 3.into(), BTreeSet::new());
    exerter.activated.push(exert_ability());
    exerter.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::status_changed(
            ObjectFilter::self_object(),
            StatusTriggerKind::Exerted,
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(4),
            }],
        ),
    });

    let game_a = world(vocab.clone(), vec![card(1, exerter, true)], vec![CardId(1)]);
    let exerter_id = object(&game_a, 1);
    let act_a = submit(
        &game_a,
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: exerter_id,
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let resolved_a = pass_pair(act_a.state());
    assert!(
        all_occurrences(&resolved_a).iter().any(|occ| matches!(
            occ,
            RulesOccurrence::KeywordActionPerformed {
                player,
                action: PlayerTriggerAction::Exerted,
            } if *player == P0
        )),
        "ExertAbilitySource must emit KeywordActionPerformed(Exerted)"
    );
    assert_eq!(resolved_a.state().execution().window().entries().len(), 1);
    let after_status_trig = pass_pair(resolved_a.state());
    assert_eq!(
        after_status_trig.state().execution().life_total(P0),
        Some(&24.into())
    );

    // Case B: Instruction::ExertAbilitySource fires a PlayerAction { action: Exerted } watcher.
    let mut plain_exerter = Program::creature(None, 3.into(), 3.into(), BTreeSet::new());
    plain_exerter.activated.push(exert_ability());
    let mut watcher = Program::enchantment(None);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(2),
        condition: TriggerSpec::player_action(
            PlayerRelation::Controller,
            PlayerTriggerAction::Exerted,
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(5),
            }],
        ),
    });

    let game_b = world(
        vocab,
        vec![card(1, plain_exerter, true), card(2, watcher, true)],
        vec![CardId(1), CardId(2)],
    );
    let act_b = submit(
        &game_b,
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: object(&game_b, 1),
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let resolved_b = pass_pair(act_b.state());
    assert_eq!(resolved_b.state().execution().window().entries().len(), 1);
    let after_player_trig = pass_pair(resolved_b.state());
    assert_eq!(
        after_player_trig.state().execution().life_total(P0),
        Some(&25.into())
    );
}

#[test]
fn triggering_spell_mana_spent_gates_spell_cast_intervening_if() {
    let vocab = vocabulary();

    // Watcher: Whenever you cast a spell, if at least 4 mana was spent to cast it,
    // gain life equal to the mana spent to cast that spell.
    let mut watcher = Program::enchantment(None);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::spell_cast(PlayerRelation::Controller, SpellFilter::any()).only_if(
            TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::EventValue(EventValueKind::TriggeringSpellManaSpent),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(4),
            })),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::EventValue(EventValueKind::TriggeringSpellManaSpent),
            }],
        ),
    });

    let mut mana_rock = Program::artifact(None, vec![]);
    mana_rock.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 6.into())])),
    });

    let cheap_spell = Program::instant(
        ManaCost::generic(2.into()),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(1),
            }],
        ),
    );
    let expensive_spell = Program::instant(
        ManaCost::generic(4.into()),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(1),
            }],
        ),
    );

    let game = world(
        vocab,
        vec![
            card(1, watcher, true),
            card(2, mana_rock, true),
            card(3, cheap_spell, false),
            card(4, expensive_spell, false),
        ],
        vec![CardId(1), CardId(2)],
    );

    // Produce 6 colorless mana.
    let funded = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source: object(&game, 2),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );

    // Spend 2 mana -> intervening-if (>= 4) fails, trigger does NOT fire.
    let cast_two = submit(
        funded.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(funded.state(), 3),
            payment: Some(ManaPayment {
                from_lots: BTreeMap::new(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Colourless,
                    amount: 2.into(),
                }],
            }),
        },
    );
    assert_eq!(
        cast_two.state().execution().window().entries().len(),
        1,
        "2-mana spell must not trigger >= 4 TriggeringSpellManaSpent intervening-if"
    );
    let after_two = pass_pair(cast_two.state());
    assert_eq!(
        after_two.state().execution().life_total(P0),
        Some(&21.into())
    );

    // Spend 4 mana -> intervening-if (>= 4) passes, trigger fires and gains 4 life.
    let cast_four = submit(
        after_two.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(after_two.state(), 4),
            payment: Some(ManaPayment {
                from_lots: BTreeMap::new(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Colourless,
                    amount: 4.into(),
                }],
            }),
        },
    );
    assert_eq!(
        cast_four.state().execution().window().entries().len(),
        2,
        "4-mana spell must trigger >= 4 TriggeringSpellManaSpent intervening-if"
    );
    let after_trigger = pass_pair(cast_four.state());
    assert_eq!(
        after_trigger.state().execution().life_total(P0),
        Some(&25.into())
    );
    let after_four = pass_pair(after_trigger.state());
    assert_eq!(
        after_four.state().execution().life_total(P0),
        Some(&26.into())
    );
}

#[test]
fn max_player_life_lost_and_other_spells_cast_this_turn_evaluate_accurately() {
    let vocab = vocabulary();

    let reader = Program::artifact(
        None,
        vec![
            ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::PlayerStat(PlayerStatExpr::MaxPlayerLifeLostThisTurn),
                    }],
                ),
            },
            ActivatedAbility {
                id: ActivatedAbilityId(2),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::PlayerStat(PlayerStatExpr::OtherSpellsCastThisTurn),
                    }],
                ),
            },
        ],
    );

    let hurt_self = spell(
        2,
        BodyTarget::None,
        vec![Instruction::AdjustPlayersLife {
            players: AffectedPlayers::One(ResolvingPlayer::Controller),
            gain: false,
            amount: ValueExpr::literal(2),
        }],
    );
    let hurt_opp = spell(
        3,
        BodyTarget::None,
        vec![Instruction::AdjustPlayersLife {
            players: AffectedPlayers::EachOpponent,
            gain: false,
            amount: ValueExpr::literal(5),
        }],
    );
    let count_other_spells = spell(
        4,
        BodyTarget::None,
        vec![Instruction::GainControllerLife {
            amount: ValueExpr::PlayerStat(PlayerStatExpr::OtherSpellsCastThisTurn),
        }],
    );

    let game = world(
        vocab,
        vec![
            card(1, reader, true),
            hurt_self,
            hurt_opp,
            count_other_spells,
        ],
        vec![CardId(1)],
    );
    let reader_id = object(&game, 1);

    // Before any life loss: MaxPlayerLifeLostThisTurn evaluates to 0.
    let check_zero = submit(
        &game,
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: reader_id,
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_zero = pass_pair(check_zero.state());
    assert_eq!(
        after_zero.state().execution().life_total(P0),
        Some(&20.into())
    );

    // P0 loses 2 life (to 18), P1 loses 5 life (to 15) -> MaxPlayerLifeLostThisTurn = 5.
    let after_hurt_self = cast_untargeted(after_zero.state(), 2);
    assert_eq!(
        after_hurt_self.state().execution().life_total(P0),
        Some(&18.into())
    );
    let after_hurt_opp = cast_untargeted(after_hurt_self.state(), 3);
    assert_eq!(
        after_hurt_opp.state().execution().life_total(P1),
        Some(&15.into())
    );

    let check_max = submit(
        after_hurt_opp.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: reader_id,
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_max = pass_pair(check_max.state());
    assert_eq!(
        after_max.state().execution().life_total(P0),
        Some(&23.into()),
        "MaxPlayerLifeLostThisTurn must return max(2, 5) = 5"
    );

    // Cast 3rd spell reading OtherSpellsCastThisTurn: excludes itself on stack -> 3 - 1 = 2.
    let after_spell_count = cast_untargeted(after_max.state(), 4);
    assert_eq!(
        after_spell_count.state().execution().life_total(P0),
        Some(&25.into()),
        "OtherSpellsCastThisTurn inside a resolving spell must exclude itself (2 other spells)"
    );

    // Activate ability reading OtherSpellsCastThisTurn: not a CardSpell on stack -> all 3 spells.
    let check_ability_count = submit(
        after_spell_count.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: reader_id,
            ability: ActivatedAbilityId(2),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_ability_count = pass_pair(check_ability_count.state());
    assert_eq!(
        after_ability_count.state().execution().life_total(P0),
        Some(&28.into()),
        "OtherSpellsCastThisTurn inside an activated ability must count all 3 spells cast this turn"
    );
}
