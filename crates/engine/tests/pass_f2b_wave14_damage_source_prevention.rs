#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
//! Wave 14 interactive damage-source selection (CR 609.7a/b), spell-source identity across
//! stack-to-battlefield resolution (CR 400.7c), generalized source/recipient turn-prevention
//! scopes, visibility hygiene, and atomic invalid-reply rejection.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;

use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{Color, KeywordAbility},
    combat::AttackDeclaration,
    damage::{
        DamageSourceChoiceFilter, DamageSourceId, PreventionRecipientSpec, PreventionSourceFilter,
        PreventionSourceSpec, TurnPreventionSpec,
    },
    effects::{CopyTargetChoice, SeedCard},
    game::{DecisionReply, GameCommand, GameInput, GameState, NativeCandidate, RuleFrame},
    mana::{CastingPayment, ManaCost, ManaPayment, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    program::{BodyTarget, ColorMask, ExecutableBody, Instruction, Program, SpellFilter},
    targets::TargetRef,
};
use std::collections::BTreeSet;
use trajectories::*;

fn colored_pinger(color: Option<Color>, keywords: BTreeSet<KeywordAbility>) -> Program {
    let mut p = Program::creature(Some(ManaCost::free()), 2.into(), 4.into(), BTreeSet::new());
    p.printed_keywords = keywords;
    if let Some(c) = color {
        p.color_indicator = Some(BTreeSet::from([c]));
    }
    p.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SINGLE_ANY_TARGET,
            vec![spellwake_engine::program::Instruction::DealDamage(
                Box::new(spellwake_engine::toolbox::DealDamageSpec {
                    source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                    recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    amount: spellwake_engine::program::DamageAmountSpec::Expr(
                        spellwake_engine::program::ValueExpr::Literal(2.into()),
                    ),
                    exile_if_dies: false,
                }),
            )],
        ),
    });
    p
}

fn ping(game: &GameState, source: u64, target: TargetRef) -> NativeCandidate {
    let act = submit(
        game,
        P0,
        GameCommand::ActivateAbility {
            source: object(game, source),
            ability: ActivatedAbilityId(1),
            target,
            payment: Some(ManaPayment::default()),
        },
    );
    pass_pair(act.state())
}

#[test]
fn chosen_damage_source_prevents_selected_permanent_respects_visibility_and_rejects_invalid_atomically()
 {
    let hidden_hand_card = colored_pinger(Some(Color::Red), BTreeSet::new());

    let mut game = world(
        vocabulary(),
        vec![
            card(1, colored_pinger(Some(Color::Red), BTreeSet::new()), true),
            card(2, colored_pinger(Some(Color::Green), BTreeSet::new()), true),
            SeedCard {
                card: CardId(3),
                owner: P1,
                destination: ObjectDestination::Hand,
                program: hidden_hand_card,
            },
            spell(
                10,
                BodyTarget::None,
                vec![Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec::prevent_damage(
                        TurnPreventionSpec {
                            combat_only: false,
                            source: PreventionSourceSpec::ChosenSource(
                                DamageSourceChoiceFilter::Any,
                            ),
                            recipient: PreventionRecipientSpec::Controller,
                            both_directions: false,
                        },
                    ),
                ))],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    // Card 3 is in P1's hidden hand; cards 1 and 2 are on the battlefield.
    let hidden_c3 = object(&game, 3);
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(hidden_c3)
            .unwrap()
            .zone(),
        ObjectZone::Hand(P1)
    );

    let cast = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 10),
            payment: Some(ManaPayment::default()),
        },
    );
    let pending = pass_pair(cast.state());
    let dec = pending
        .state()
        .pending_decision()
        .expect("DamageSourceChoice decision expected");
    let RuleFrame::DamageSourceChoice(frame) = dec.frame() else {
        panic!("expected DamageSourceChoice frame, got {:?}", dec.frame());
    };
    let c1_id = object(pending.state(), 1);
    let c2_id = object(pending.state(), 2);
    assert!(frame.sources().contains(&DamageSourceId::Object(c1_id)));
    assert!(frame.sources().contains(&DamageSourceId::Object(c2_id)));
    // Visibility invariant: hidden hand card MUST NEVER be offered in generic damage-source list.
    assert!(!frame.sources().contains(&DamageSourceId::Object(hidden_c3)));

    // Atomic invalid-reply rejections: wrong actor, wrong decision kind, out-of-domain hidden object, stale basis.
    let basis_before = pending.state().basis().clone();
    let dec_id = dec.id();
    let err_actor = pending.state().advance(GameInput {
        basis: basis_before.clone(),
        actor: P1,
        command: GameCommand::Reply {
            decision: dec_id,
            choice: DecisionReply::ChooseDamageSource(DamageSourceId::Object(c1_id)),
        },
    });
    assert!(err_actor.is_err());
    let err_kind = pending.state().advance(GameInput {
        basis: basis_before.clone(),
        actor: P0,
        command: GameCommand::Reply {
            decision: dec_id,
            choice: DecisionReply::OptionIndex(0),
        },
    });
    assert!(err_kind.is_err());
    let err_hidden = pending.state().advance(GameInput {
        basis: basis_before.clone(),
        actor: P0,
        command: GameCommand::Reply {
            decision: dec_id,
            choice: DecisionReply::ChooseDamageSource(DamageSourceId::Object(hidden_c3)),
        },
    });
    assert!(err_hidden.is_err());
    let mut stale_basis = basis_before.clone();
    stale_basis.revision += 99;
    let err_stale = pending.state().advance(GameInput {
        basis: stale_basis,
        actor: P0,
        command: GameCommand::Reply {
            decision: dec_id,
            choice: DecisionReply::ChooseDamageSource(DamageSourceId::Object(c1_id)),
        },
    });
    assert!(err_stale.is_err());
    assert_eq!(pending.state().basis().revision, basis_before.revision);

    // Legal reply: choose c1.
    let resolved = reply(
        pending.state(),
        P0,
        DecisionReply::ChooseDamageSource(DamageSourceId::Object(c1_id)),
    );
    assert!(resolved.state().pending_decision().is_none());

    // c1 -> P0 (controller) is prevented.
    let after_c1_p0 = ping(resolved.state(), 1, TargetRef::Player(P0));
    assert_eq!(
        after_c1_p0.state().execution().life_total(P0),
        Some(&20.into())
    );

    // c1 -> P1 (opponent) is NOT prevented because recipient scope is Controller ("to you").
    let after_c1_p1 = ping(after_c1_p0.state(), 1, TargetRef::Player(P1));
    assert_eq!(
        after_c1_p1.state().execution().life_total(P1),
        Some(&18.into())
    );

    // c2 -> P0 is NOT prevented because only c1 was chosen.
    let after_c2_p0 = ping(after_c1_p1.state(), 2, TargetRef::Player(P0));
    assert_eq!(
        after_c2_p0.state().execution().life_total(P0),
        Some(&18.into())
    );
    game = after_c2_p0.into_state();
    assert_eq!(game.execution().life_total(P0), Some(&18.into()));
}

#[test]
fn chosen_color_and_filtered_source_recipient_scopes_match_exact_subsets() {
    // 1. ChosenColor + Target recipient (Avacyn, Guardian Angel): choose Red -> only red damage to target prevented.
    let game = world(
        vocabulary(),
        vec![
            card(1, colored_pinger(Some(Color::Red), BTreeSet::new()), true),
            card(2, colored_pinger(Some(Color::Green), BTreeSet::new()), true),
            spell(
                10,
                BodyTarget::SINGLE_ANY_TARGET,
                vec![Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec::prevent_damage(
                        TurnPreventionSpec {
                            combat_only: false,
                            source: PreventionSourceSpec::ChosenColor,
                            recipient: PreventionRecipientSpec::Target,
                            both_directions: false,
                        },
                    ),
                ))],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let cast = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 10),
            target: TargetRef::Player(P1),
            payment: ManaPayment::default(),
        },
    );
    let pending = pass_pair(cast.state());
    assert!(matches!(
        pending.state().pending_decision().unwrap().frame(),
        RuleFrame::ResolutionManaColor(_)
    ));
    let chosen_red = reply(pending.state(), P0, DecisionReply::ManaColor(ManaType::Red));
    let red_to_p1 = ping(chosen_red.state(), 1, TargetRef::Player(P1));
    assert_eq!(
        red_to_p1.state().execution().life_total(P1),
        Some(&20.into())
    );
    let green_to_p1 = ping(red_to_p1.state(), 2, TargetRef::Player(P1));
    assert_eq!(
        green_to_p1.state().execution().life_total(P1),
        Some(&18.into())
    );
    let red_to_p0 = ping(green_to_p1.state(), 1, TargetRef::Player(P0));
    assert_eq!(
        red_to_p0.state().execution().life_total(P0),
        Some(&18.into())
    );

    // 2. Color-filtered ChosenSource (Burrenton Forge-Tender): empty domain no-ops cleanly.
    let no_red = world(
        vocabulary(),
        vec![
            card(1, colored_pinger(Some(Color::Green), BTreeSet::new()), true),
            spell(
                10,
                BodyTarget::None,
                vec![Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec::prevent_damage(
                        TurnPreventionSpec {
                            combat_only: false,
                            source: PreventionSourceSpec::ChosenSource(
                                DamageSourceChoiceFilter::Colors(ColorMask::RED),
                            ),
                            recipient: PreventionRecipientSpec::Any,
                            both_directions: false,
                        },
                    ),
                ))],
            ),
        ],
        vec![CardId(1)],
    );
    let cast_empty = submit(
        &no_red,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&no_red, 10),
            payment: Some(ManaPayment::default()),
        },
    );
    let resolved_empty = pass_pair(cast_empty.state());
    assert!(resolved_empty.state().pending_decision().is_none());

    // 3. Surge of Salvation filter (black/red sources -> controlled creatures only).
    let surge_world = world(
        vocabulary(),
        vec![
            card(1, colored_pinger(Some(Color::Red), BTreeSet::new()), true),
            card(2, colored_pinger(Some(Color::Green), BTreeSet::new()), true),
            spell(
                10,
                BodyTarget::None,
                vec![Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec::prevent_damage(
                        TurnPreventionSpec {
                            combat_only: false,
                            source: PreventionSourceSpec::Filter(PreventionSourceFilter {
                                any_colors: ColorMask::BLACK.union(ColorMask::RED),
                                ..Default::default()
                            }),
                            recipient: PreventionRecipientSpec::ControlledCreatures,
                            both_directions: false,
                        },
                    ),
                ))],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let cast_surge = submit(
        &surge_world,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&surge_world, 10),
            payment: Some(ManaPayment::default()),
        },
    );
    let surged = pass_pair(cast_surge.state());
    // Red -> controlled creature c2: prevented (0 damage marked on c2).
    let red_to_c2 = ping(
        surged.state(),
        1,
        TargetRef::Object(object(surged.state(), 2)),
    );
    assert_eq!(
        red_to_c2
            .state()
            .execution()
            .battlefield_marked_damage(object(red_to_c2.state(), 2))
            .unwrap_or_else(|| 0.into()),
        0.into()
    );
    // Red -> P0 (player, not controlled creature): NOT prevented.
    let red_to_p0 = ping(red_to_c2.state(), 1, TargetRef::Player(P0));
    assert_eq!(
        red_to_p0.state().execution().life_total(P0),
        Some(&18.into())
    );
    // Green -> controlled creature c2: NOT prevented (2 damage marked on c2).
    let green_to_c2 = ping(
        red_to_p0.state(),
        2,
        TargetRef::Object(object(red_to_p0.state(), 2)),
    );
    assert_eq!(
        green_to_c2
            .state()
            .execution()
            .battlefield_marked_damage(object(green_to_c2.state(), 2)),
        Some(2.into())
    );

    // 4. Al-abara's Carpet filter: attacking creatures without flying -> Controller.
    let carpet_world = world(
        vocabulary(),
        vec![
            card(1, colored_pinger(None, BTreeSet::new()), true),
            card(
                2,
                colored_pinger(None, BTreeSet::from([KeywordAbility::Flying])),
                true,
            ),
            SeedCard {
                card: CardId(10),
                owner: P1,
                destination: ObjectDestination::Hand,
                program: Program::instant(
                    ManaCost::free(),
                    ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::ApplyContinuous(Box::new(
                            spellwake_engine::toolbox::ApplyContinuousSpec::prevent_damage(
                                TurnPreventionSpec {
                                    combat_only: false,
                                    source: PreventionSourceSpec::Filter(PreventionSourceFilter {
                                        creatures_only: true,
                                        attacking_only: true,
                                        forbidden_keyword: Some(KeywordAbility::Flying),
                                        ..Default::default()
                                    }),
                                    recipient: PreventionRecipientSpec::Controller,
                                    both_directions: false,
                                },
                            ),
                        ))],
                    ),
                ),
            },
        ],
        vec![CardId(1), CardId(2)],
    );
    let begin = pass_pair(&carpet_world);
    let attack = pass_pair(begin.state());
    let declared = reply(
        attack.state(),
        P0,
        DecisionReply::DeclareAttackers(vec![
            AttackDeclaration {
                attacker: object(attack.state(), 1),
                defender: P1,
                defending_planeswalker: None,
            },
            AttackDeclaration {
                attacker: object(attack.state(), 2),
                defender: P1,
                defending_planeswalker: None,
            },
        ]),
    );
    let p0_pass = submit(declared.state(), P0, GameCommand::Pass);
    let cast_carpet = submit(
        p0_pass.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(p0_pass.state(), 10),
            payment: Some(ManaPayment::default()),
        },
    );
    let p1_pass = submit(cast_carpet.state(), P1, GameCommand::Pass);
    let carpet_done = submit(p1_pass.state(), P0, GameCommand::Pass);
    let blockers = pass_pair(carpet_done.state());
    let damage_step = if blockers.state().pending_decision().is_some() {
        let unblocked = reply(blockers.state(), P1, DecisionReply::DeclareBlockers(vec![]));
        pass_pair(unblocked.state())
    } else {
        pass_pair(blockers.state())
    };
    let after_combat = pass_pair(damage_step.state());
    // Non-flying attacker c1 (2 power) prevented; flying attacker c2 (2 power) deals 2 to P1!
    assert_eq!(
        after_combat.state().execution().life_total(P1),
        Some(&18.into())
    );
}

#[test]
fn multi_target_permanent_prevention_prevents_announced_targets_only() {
    let soul_parry = card(
        10,
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_CREATURE,
                vec![Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec::prevent_damage(
                        TurnPreventionSpec {
                            combat_only: false,
                            source: PreventionSourceSpec::AllPermanentTargets,
                            recipient: PreventionRecipientSpec::Any,
                            both_directions: false,
                        },
                    ),
                ))],
            )
            .with_permanent_target_bounds(1, 2),
        ),
        false,
    );
    let game = world(
        vocabulary(),
        vec![
            card(1, colored_pinger(None, BTreeSet::new()), true),
            card(2, colored_pinger(None, BTreeSet::new()), true),
            card(3, colored_pinger(None, BTreeSet::new()), true),
            soul_parry,
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let cast = submit(
        &game,
        P0,
        GameCommand::CastWithResources {
            spell: object(&game, 10),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![
                    TargetRef::Object(object(&game, 1)),
                    TargetRef::Object(object(&game, 2)),
                ],
                ..CastingPayment::default()
            },
        },
    );
    let resolved = pass_pair(cast.state());
    let p1 = ping(resolved.state(), 1, TargetRef::Player(P1));
    assert_eq!(p1.state().execution().life_total(P1), Some(&20.into()));
    let p2 = ping(p1.state(), 2, TargetRef::Player(P1));
    assert_eq!(p2.state().execution().life_total(P1), Some(&20.into()));
    let p3 = ping(p2.state(), 3, TargetRef::Player(P1));
    assert_eq!(p3.state().execution().life_total(P1), Some(&18.into()));
}

#[test]
fn recipient_only_prevent_all_damage_scopes_protect_exact_recipients() {
    let game = world(
        vocabulary(),
        vec![
            card(1, colored_pinger(None, BTreeSet::new()), true),
            card(2, colored_pinger(None, BTreeSet::new()), true),
            spell(
                10,
                BodyTarget::None,
                vec![Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec::prevent_damage(
                        TurnPreventionSpec {
                            combat_only: false,
                            source: PreventionSourceSpec::Any,
                            recipient: PreventionRecipientSpec::AllCreatures,
                            both_directions: false,
                        },
                    ),
                ))],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let cast = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 10),
            payment: Some(ManaPayment::default()),
        },
    );
    let resolved = pass_pair(cast.state());
    // Ping creature c2 -> prevented under AllCreatures (`Blinding Fog` / `Forfend`).
    let ping_c2 = ping(
        resolved.state(),
        1,
        TargetRef::Object(object(resolved.state(), 2)),
    );
    assert_eq!(
        ping_c2
            .state()
            .execution()
            .battlefield_marked_damage(object(ping_c2.state(), 2))
            .unwrap_or_else(|| 0.into()),
        0.into()
    );
    // Ping player P1 -> NOT prevented under AllCreatures.
    let ping_p1 = ping(ping_c2.state(), 1, TargetRef::Player(P1));
    assert_eq!(ping_p1.state().execution().life_total(P1), Some(&18.into()));
}
