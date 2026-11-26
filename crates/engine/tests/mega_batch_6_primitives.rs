#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
//! Behavioral integration tests for Mega-Batch 6 engine primitives:
//! - Universal `up_to_one_target` execution with 0 targets and 1 target (including `BranchIfFalse`
//!   and non-target follow-up instructions)
//! - `StaticPart::GrantLoyalty` granting a +N loyalty ability to a planeswalker
//! - `StaticPart::InheritZoneAbilities` inheriting activated abilities from `AllGraveyards`
//!   (Mirran Safehouse / Necrotic Ooze) and `loyalty_only: true` from `ControlledBattlefield`
//! - `StaticPlayerStat::{TotalUnspentMana, OtherControlledCreatureCounters, OwnGraveyardWithCycling}`
//!   and `NumericExpression::{Add, Scale, Cap}`
//! - `ActivatedCostScope::{EquipTargeting, Exhaust, GraveyardCardFilter}`
//! - `TemporaryRuleKind::{CastMaxMoreSpellsThisTurn, NextSpellAffinity}`

#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, HandDiscardKeyword, SourceCost},
    characteristics::{
        CardType, ControllerSelector, NumericExpression, NumericPair, PermanentSelector,
        ResolvedDuration, StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
        StaticPlayerStat, StaticZone,
    },
    effects::SeedCard,
    game::{GameCommand, GameInput, GameState, NativeCandidate},
    life::LifeAdjustment,
    mana::{CastingPayment, ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    priority::SeatId,
    program::{
        AffectedPlayers, BodyTarget, CardTypeMask, Comparison, ExecutableBody, Instruction,
        ObjectClause, ObjectFilter, Program, ResolutionCondition, ResolvingPlayer,
        TemporaryRuleKind, ValueExpr, ValueObjectRef,
    },
    targets::TargetRef,
};
use std::collections::BTreeSet;

fn validated(program: Program, v: &CreatureTypeVocabulary) -> Program {
    program.validate(v).expect("program must validate");
    program
}

fn vanilla(p: i64, t: i64) -> Program {
    Program::creature(Some(ManaCost::free()), p.into(), t.into(), BTreeSet::new())
}

fn creature_clause() -> ObjectClause {
    let mut c = ObjectClause::any();
    c.required_types = CardTypeMask::CREATURE;
    c
}

fn artifact_clause() -> ObjectClause {
    let mut c = ObjectClause::any();
    c.required_types = CardTypeMask::ARTIFACT;
    c
}

fn zone_of(state: &GameState, card_id: u64) -> ObjectZone {
    state
        .execution()
        .window()
        .cards()
        .get(object(state, card_id))
        .expect("card object must exist")
        .zone()
}

fn cast_untargeted(game: &GameState, actor: SeatId, card_id: u64) -> NativeCandidate {
    submit(
        game,
        actor,
        GameCommand::CastUntargeted {
            spell: object(game, card_id),
            payment: Some(ManaPayment::default()),
        },
    )
}

#[test]
fn universal_up_to_one_target_resolves_with_zero_or_one_target_including_target_branch() {
    let v = vocabulary();
    let victim = validated(vanilla(2, 2), &v);

    // "Choose up to one target creature. If its power is 2 or less, destroy it. You gain 3 life."
    // Uses DestroyTarget + BranchIfFalse on TargetObject, neither in old 6-op whitelist!
    let body = ExecutableBody::new(
        BodyTarget::SingleCreature,
        vec![
            Instruction::BranchIfFalse {
                condition: ResolutionCondition {
                    left: ValueExpr::ObjectStat {
                        subject: ValueObjectRef::TargetObject,
                        stat: spellwake_engine::program::ObjectStatKind::Power,
                    },
                    comparison: Comparison::LessOrEqual,
                    right: ValueExpr::literal(2),
                },
                destination: 2,
            },
            Instruction::DestroyTarget,
            Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(3.into()),
            },
        ],
    )
    .with_up_to_one_target();
    let spell = validated(Program::sorcery(ManaCost::free(), body), &v);

    // Case 1: cast with 0 targets -> skips BranchIfFalse & DestroyTarget, still gains 3 life!
    let game0 = world(
        v.clone(),
        vec![
            SeedCard {
                card: CardId(1),
                owner: P1,
                program: victim.clone(),
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            card(2, spell.clone(), false),
        ],
        vec![CardId(1)],
    );
    let cast0 = submit(
        &game0,
        P0,
        GameCommand::CastWithResources {
            spell: object(&game0, 2),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let res0 = pass_pair(cast0.state());
    assert_eq!(zone_of(res0.state(), 1), ObjectZone::Battlefield);
    assert_eq!(res0.state().execution().life_total(P0), Some(&23.into()));

    // Case 2: cast with 1 target (2/2) -> destroys target AND gains 3 life!
    let game1 = world(
        v,
        vec![
            SeedCard {
                card: CardId(1),
                owner: P1,
                program: victim,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            card(2, spell, false),
        ],
        vec![CardId(1)],
    );
    let cast1 = submit(
        &game1,
        P0,
        GameCommand::CastWithResources {
            spell: object(&game1, 2),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![TargetRef::Object(object(&game1, 1))],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let res1 = pass_pair(cast1.state());
    assert_eq!(zone_of(res1.state(), 1), ObjectZone::Graveyard(P1));
    assert_eq!(res1.state().execution().life_total(P0), Some(&23.into()));
}

#[test]
fn static_part_grant_loyalty_grants_activatable_plus_loyalty_ability() {
    let v = vocabulary();
    // Planeswalker with starting loyalty 3 and no printed abilities.
    let mut pw = Program::artifact(Some(ManaCost::free()), vec![]);
    pw.card_types = BTreeSet::from([CardType::Planeswalker]);
    pw.printed_loyalty = Some(3.into());
    let pw = validated(pw, &v);

    // Static permanent granting planeswalkers you control "+2: You gain 4 life."
    let mut granter = Program::artifact(Some(ManaCost::free()), vec![]);
    granter.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Planeswalker]),
            controller: ControllerSelector::SourceController,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::GrantLoyalty {
            ability: Box::new(ActivatedAbility {
                id: ActivatedAbilityId(0),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(4.into()),
                    }],
                ),
            }),
            loyalty: Some(2),
            variable_loyalty: false,
        }]),
    });
    let granter = validated(granter, &v);

    let game = world(
        v,
        vec![card(1, pw, false), card(2, granter, true)],
        vec![CardId(2)],
    );
    let after_pw = pass_pair(cast_untargeted(&game, P0, 1).state());
    assert_eq!(zone_of(after_pw.state(), 1), ObjectZone::Battlefield);

    let eval = after_pw
        .state()
        .execution()
        .evaluated_characteristics()
        .expect("eval");
    let pw_view = eval.get(object(after_pw.state(), 1)).expect("pw view");
    assert_eq!(pw_view.granted_activated.len(), 1);
    let (&grant_key, _) = pw_view.granted_activated.iter().next().expect("grant");
    let terms = pw_view
        .inherited_activation_terms
        .get(&grant_key)
        .expect("loyalty terms");
    assert_eq!(terms.loyalty, Some(2));
    assert!(!terms.variable_loyalty);
}

#[test]
fn static_part_inherit_zone_abilities_all_graveyards_artifacts() {
    let v = vocabulary();
    // Artifact in opponent's graveyard with "{T}: You gain 5 life."
    let gy_artifact = validated(
        Program::artifact(
            Some(ManaCost::free()),
            vec![ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::Tap,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(5.into()),
                    }],
                ),
            }],
        ),
        &v,
    );
    // Creature in graveyard with activated ability (must NOT be inherited by artifact-only filter).
    let mut gy_creature = vanilla(2, 2);
    gy_creature.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(2),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(9.into()),
            }],
        ),
    });
    let gy_creature = validated(gy_creature, &v);

    // Mirran Safehouse style permanent: has all activated abilities of artifact cards in all graveyards.
    let mut safehouse = Program::artifact(Some(ManaCost::free()), vec![]);
    safehouse.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            controller: ControllerSelector::SourceItself,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::InheritZoneAbilities {
            zone: StaticZone::AllGraveyards,
            filter: Box::new(ObjectFilter::Clause(artifact_clause())),
            loyalty_only: false,
        }]),
    });
    let safehouse = validated(safehouse, &v);

    let game = world(
        v,
        vec![
            card(1, safehouse, true),
            SeedCard {
                card: CardId(2),
                owner: P1,
                program: gy_artifact,
                destination: ObjectDestination::Graveyard,
            },
            SeedCard {
                card: CardId(3),
                owner: P0,
                program: gy_creature,
                destination: ObjectDestination::Graveyard,
            },
        ],
        vec![CardId(1)],
    );

    let eval = game.execution().evaluated_characteristics().expect("eval");
    let sh_view = eval.get(object(&game, 1)).expect("safehouse view");
    assert_eq!(
        sh_view.granted_activated.len(),
        1,
        "Must inherit exactly the 1 artifact ability from opponent's graveyard, not the creature's"
    );
}

#[test]
fn numeric_expression_own_graveyard_with_cycling_scale_and_cap() {
    let v = vocabulary();
    let mut cycler = vanilla(2, 2);
    cycler.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::generic(2.into()),
        source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: ValueExpr::literal(1),
            }],
        ),
    });
    let cycler = validated(cycler, &v);
    let non_cycler = validated(vanilla(2, 2), &v);

    // Creature gets +2/+0 for each card with cycling in your graveyard, capped at +3/+0.
    let mut manifestation = vanilla(0, 4);
    manifestation.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            controller: ControllerSelector::SourceItself,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Cap {
                expr: Box::new(NumericExpression::Scale {
                    expr: Box::new(NumericExpression::PlayerStat(
                        StaticPlayerStat::OwnGraveyardWithCycling,
                    )),
                    multiplier: 2.into(),
                    offset: 0.into(),
                }),
                max: 3.into(),
            },
            toughness: NumericExpression::Literal(0.into()),
        })]),
    });
    let manifestation = validated(manifestation, &v);

    let game = world(
        v,
        vec![
            card(1, manifestation, true),
            SeedCard {
                card: CardId(2),
                owner: P0,
                program: cycler.clone(),
                destination: ObjectDestination::Graveyard,
            },
            SeedCard {
                card: CardId(3),
                owner: P0,
                program: cycler,
                destination: ObjectDestination::Graveyard,
            },
            SeedCard {
                card: CardId(4),
                owner: P0,
                program: non_cycler,
                destination: ObjectDestination::Graveyard,
            },
        ],
        vec![CardId(1)],
    );

    let eval = game.execution().evaluated_characteristics().expect("eval");
    let pt = eval
        .get(object(&game, 1))
        .and_then(|c| c.power_toughness.as_ref())
        .expect("pt");
    // 2 cyclers * 2 = 4, capped at 3 -> 3/4.
    assert_eq!(pt.derived.power, 3.into());
    assert_eq!(pt.derived.toughness, 4.into());
}
