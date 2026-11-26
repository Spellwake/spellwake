#![forbid(unsafe_code)]
//! Behavioral integration tests for Mega-Batch 3 engine primitives:
//! - `Instruction::CounterMatchingStackObjects` (opponents-only spell sweep & ability sweep)
//! - `Instruction::ReturnLinkedExile` (returns exiled card linked to source permanent)
//! - `Instruction::TransferAllCountersFromTriggeringLki` (transfers +1/+1 and keyword counters on death LKI)
//! - `SpellCostModifier::AlternativeForMatching { pay_life_mv: true, each_player }`
//! - `CastPermissionRule::Flash` with `cast_zone` enforcement
//! - `Instruction::TransformSubject`
//! - `Instruction::SetPlayersLife` (normal resolution + inline LoseGame replacement)
//! - `ActivatedCostScope::Loyalty` & `ActivatedCostScope::Targeting`
//! - `ImpulsePlayDuration::UntilNextEndStep` & `UntilNextUpkeep`

#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        KeywordAbility, SpellCostModifier, StaticAbilityId, StaticContinuousAbility,
    },
    effects::{CounterKind, DecisionReply, SeedCard},
    game::{GameCommand, GameState, NativeCandidate},
    life::LifeAdjustment,
    mana::{CastingPayment, ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    priority::SeatId,
    program::{
        BodyTarget, DiesTriggerMode, EffectSubject, ExecutableBody, Instruction, ObjectFilter,
        Program, SpellFilter, SubjectDestination, TriggerEvent, TriggerGrouping, TriggerSpec,
        TriggeredAbility, TriggeredAbilityId, ValueExpr,
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
fn counter_matching_stack_objects_counters_opponent_spells_and_keeps_own_spells() {
    let v = vocabulary();
    let own_bear = validated(vanilla(2, 2), &v);
    let opp_instant = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(5.into()),
                }],
            ),
        ),
        &v,
    );
    let sweep = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CounterMatchingStackObjects {
                    filter: None,
                    include_spells: true,
                    include_abilities: false,
                    opponents_only: true,
                }],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            card(1, own_bear, false),
            SeedCard {
                card: CardId(2),
                owner: P1,
                program: opp_instant,
                destination: ObjectDestination::Hand,
            },
            card(3, sweep, false),
        ],
        vec![],
    );

    // P0 casts own_bear; passes priority to P1
    let s1 = cast_untargeted(&game, P0, 1);
    let s2 = submit(s1.state(), P0, GameCommand::Pass);
    // P1 responds with instant (+5 life); passes priority to P0
    let s3 = cast_untargeted(s2.state(), P1, 2);
    let s4 = submit(s3.state(), P1, GameCommand::Pass);
    // P0 responds with opponents-only spell sweep
    let s5 = cast_untargeted(s4.state(), P0, 3);
    // Resolve sweep (P0 pass -> P1 pass)
    let after_sweep = pass_pair(s5.state());
    assert_eq!(
        zone_of(after_sweep.state(), 2),
        ObjectZone::Graveyard(P1),
        "Opponents-only CounterMatchingStackObjects must counter opponent spell to graveyard"
    );
    assert_eq!(
        after_sweep.state().execution().life_total(P1),
        Some(&20.into()),
        "Countered opponent instant must not resolve its life gain"
    );
    assert_eq!(
        zone_of(after_sweep.state(), 1),
        ObjectZone::Stack,
        "Opponents-only CounterMatchingStackObjects must NOT counter controller's own spell"
    );
    // Resolve own_bear
    let resolved = pass_pair(after_sweep.state());
    assert_eq!(
        zone_of(resolved.state(), 1),
        ObjectZone::Battlefield,
        "Controller's own spell must still resolve onto battlefield"
    );
}

#[test]
fn return_linked_exile_returns_cards_exiled_with_source() {
    let v = vocabulary();
    let mut wave = vanilla(2, 2);
    wave.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::ExileTargetUntilSourceLeaves],
        ),
    });
    wave.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(2),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::ReturnLinkedExile {
                destination: SubjectDestination::Battlefield {
                    tapped: false,
                    attacking: false,
                    transformed: false,
                    under_owner_control: true,
                    with_counter: None,
                    attach_to_self: false,
                },
                only_first: false,
            }],
        ),
    });
    let wave = validated(wave, &v);
    let target_bear = validated(vanilla(3, 3), &v);

    let game = world(
        v,
        vec![
            card(1, wave, true),
            SeedCard {
                card: CardId(2),
                owner: P1,
                program: target_bear,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
        ],
        vec![CardId(1), CardId(2)],
    );

    // Activate ability 1 to exile P1's creature linked to card 1
    let act1 = submit(
        &game,
        P0,
        GameCommand::ActivateAbility {
            source: object(&game, 1),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(object(&game, 2)),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_exile = pass_pair(act1.state());
    assert_eq!(zone_of(after_exile.state(), 2), ObjectZone::Exile);

    // Activate ability 2 to return linked exile to battlefield
    let act2 = submit(
        after_exile.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: object(after_exile.state(), 1),
            ability: ActivatedAbilityId(2),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_return = pass_pair(act2.state());
    assert_eq!(
        zone_of(after_return.state(), 2),
        ObjectZone::Battlefield,
        "ReturnLinkedExile must return exiled card linked to source permanent"
    );
}

#[test]
fn transfer_all_counters_from_triggering_lki_copies_pt_and_keyword_counters() {
    let v = vocabulary();
    let mut donor = vanilla(1, 1);
    donor.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::new(TriggerEvent::Dies {
            subject: ObjectFilter::self_object(),
            mode: DiesTriggerMode::Creature,
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::TransferAllCountersFromTriggeringLki {
                to: Box::new(EffectSubject::TargetObject),
            }],
        ),
    });
    let donor = validated(donor, &v);
    let recipient = validated(vanilla(1, 1), &v);
    let buff = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    Instruction::PutCountersOnSubject {
                        subject: Box::new(EffectSubject::TargetObject),
                        kind: CounterKind::PlusOne,
                        amount: ValueExpr::literal(2),
                    },
                    Instruction::PutCountersOnSubject {
                        subject: Box::new(EffectSubject::TargetObject),
                        kind: CounterKind::Flying,
                        amount: ValueExpr::literal(1),
                    },
                ],
            ),
        ),
        &v,
    );
    let kill = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(BodyTarget::SingleCreature, vec![Instruction::DestroyTarget]),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            card(1, donor, true),
            card(2, recipient, true),
            card(3, buff, false),
            card(4, kill, false),
        ],
        vec![CardId(1), CardId(2)],
    );

    // Put two +1/+1 counters and a flying counter on donor (card 1)
    let c1 = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 3),
            target: TargetRef::Object(object(&game, 1)),
            payment: ManaPayment::default(),
        },
    );
    let buffed = pass_pair(c1.state());

    // Destroy donor (card 1) -> Dies trigger prompts target; choose recipient (card 2)
    let c2 = submit(
        buffed.state(),
        P0,
        GameCommand::CastPaid {
            spell: object(buffed.state(), 4),
            target: TargetRef::Object(object(buffed.state(), 1)),
            payment: ManaPayment::default(),
        },
    );
    let killed = pass_pair(c2.state());
    let pending = killed
        .state()
        .pending_decision()
        .expect("dies trigger must ask for target");
    let chosen = submit(
        killed.state(),
        P0,
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::TriggerTarget(TargetRef::Object(object(killed.state(), 2))),
        },
    );
    let resolved = pass_pair(chosen.state());

    let ev = resolved
        .state()
        .execution()
        .evaluated_characteristics()
        .expect("evaluate characteristics");
    let rec_view = ev
        .get(object(resolved.state(), 2))
        .expect("recipient on battlefield");
    let pt = rec_view.power_toughness.as_ref().expect("creature P/T");
    assert_eq!(
        (&pt.derived.power, &pt.derived.toughness),
        (&3.into(), &3.into()),
        "TransferAllCountersFromTriggeringLki must transfer +1/+1 counters (1/1 -> 3/3)"
    );
    assert!(
        rec_view.has_keyword(KeywordAbility::Flying),
        "TransferAllCountersFromTriggeringLki must also transfer keyword counters (Flying)"
    );
}

#[test]
fn alternative_for_matching_pay_life_mv_casts_spell_by_paying_life() {
    let v = vocabulary();
    let mut citadel = vanilla(0, 4);
    citadel.continuous = Some(StaticContinuousAbility::CostModifier {
        id: StaticAbilityId(1),
        modifier: SpellCostModifier::AlternativeForMatching {
            filter: SpellFilter::any(),
            mana: ManaCost::free(),
            once_each_turn: false,
            own_turn_only: false,
            pay_life_mv: true,
            each_player: false,
        },
    });
    let citadel = validated(citadel, &v);
    let four_drop = validated(
        Program::creature(
            Some(ManaCost::generic(4.into())),
            4.into(),
            4.into(),
            BTreeSet::new(),
        ),
        &v,
    );

    let game = world(
        v,
        vec![card(1, citadel, true), card(2, four_drop, false)],
        vec![CardId(1)],
    );

    let payment = CastingPayment {
        use_alternative: false,
        alternative_cost_source: Some(object(&game, 1)),
        ..CastingPayment::default()
    };
    let cast = submit(
        &game,
        P0,
        GameCommand::CastWithResources {
            spell: object(&game, 2),
            target: None,
            payment,
        },
    );
    let resolved = pass_pair(cast.state());
    assert_eq!(
        zone_of(resolved.state(), 2),
        ObjectZone::Battlefield,
        "AlternativeForMatching(pay_life_mv: true) must allow casting 4-drop without mana"
    );
    assert_eq!(
        resolved.state().execution().life_total(P0),
        Some(&16.into()),
        "AlternativeForMatching(pay_life_mv: true) must deduct life equal to mana value (20 -> 16)"
    );
}

fn try_submit(
    game: &GameState,
    actor: SeatId,
    command: GameCommand,
) -> Result<NativeCandidate, spellwake_engine::game::GameError> {
    game.advance(spellwake_engine::game::GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    })
}

fn advance_to_step(
    mut game: GameState,
    target_player: SeatId,
    target_step: spellwake_engine::turn::Step,
) -> GameState {
    for _ in 0..120 {
        if let Some(turn) = game.turn()
            && game.execution().window().active_player() == target_player
            && turn.position().step == target_step
            && game.execution().window().entries().is_empty()
        {
            return game;
        }
        let active = game.execution().window().active_player();
        let other = if active == P0 { P1 } else { P0 };
        if let Ok(next) = try_submit(&game, active, GameCommand::Pass) {
            game = next.into_state();
        } else if let Ok(next) = try_submit(&game, other, GameCommand::Pass) {
            game = next.into_state();
        } else if let Some(d) = game.pending_decision() {
            let choice = if let Ok(next) = try_submit(
                &game,
                d.chooser(),
                GameCommand::Reply {
                    decision: d.id(),
                    choice: DecisionReply::DeclareAttackers(Vec::new()),
                },
            ) {
                game = next.into_state();
                continue;
            } else {
                DecisionReply::DeclareBlockers(Vec::new())
            };
            game = submit(
                &game,
                d.chooser(),
                GameCommand::Reply {
                    decision: d.id(),
                    choice,
                },
            )
            .into_state();
        } else {
            panic!(
                "stuck advancing step at active={:?} step={:?}",
                active,
                game.turn().map(|t| t.position().step)
            );
        }
    }
    panic!("failed to advance to {:?} {:?}", target_player, target_step);
}

#[test]
fn flash_cast_permission_enforces_cast_zone_filter() {
    let v = vocabulary();
    let mut elsha = vanilla(3, 3);
    elsha.continuous = Some(StaticContinuousAbility::CastPermission {
        id: StaticAbilityId(1),
        permission: spellwake_engine::characteristics::CastPermissionRule::PlayLibraryTopMatching {
            lands_allowed: false,
            filter: Box::new(ObjectFilter::any()),
        },
    });
    elsha
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(2),
            permission: spellwake_engine::characteristics::CastPermissionRule::Flash {
                any_player: false,
                matching: SpellFilter {
                    cast_zone: spellwake_engine::program::SpellCastZoneFilter::OwnLibrary,
                    ..SpellFilter::any()
                },
            },
        });
    let elsha = validated(elsha, &v);
    let lib_bear = validated(vanilla(2, 2), &v);
    let hand_bear = validated(vanilla(2, 2), &v);
    let instant_stub = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(1),
                }],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            card(1, elsha, true),
            SeedCard {
                card: CardId(2),
                owner: P0,
                program: lib_bear,
                destination: ObjectDestination::LibraryTop,
            },
            card(3, hand_bear, false),
            card(4, instant_stub, false),
        ],
        vec![CardId(1)],
    );
    // Put an instant on the stack so sorcery-speed casts are illegal right now.
    let s1 = cast_untargeted(&game, P0, 4);

    // Negative: CardId 3 is in hand, NOT OwnLibrary, so it MUST NOT get flash!
    let hand_err = try_submit(
        s1.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(s1.state(), 3),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        hand_err.is_err(),
        "Flash with cast_zone: OwnLibrary must NOT grant flash to spells in hand"
    );

    // Positive: CardId 2 is on top of own library -> MUST be castable at instant speed!
    let lib_cast = cast_untargeted(s1.state(), P0, 2);
    let after_lib = pass_pair(lib_cast.state());
    assert_eq!(
        zone_of(after_lib.state(), 2),
        ObjectZone::Battlefield,
        "Flash with cast_zone: OwnLibrary must grant flash to top-of-library spells"
    );
}

#[test]
fn alternative_for_matching_each_player_allows_opponent_free_cast() {
    let v = vocabulary();
    let mut aluren = vanilla(0, 4);
    aluren.continuous = Some(StaticContinuousAbility::CostModifier {
        id: StaticAbilityId(1),
        modifier: SpellCostModifier::AlternativeForMatching {
            filter: SpellFilter::any(),
            mana: ManaCost::free(),
            once_each_turn: false,
            own_turn_only: false,
            pay_life_mv: false,
            each_player: true,
        },
    });
    let aluren = validated(aluren, &v);
    let opp_creature = validated(
        Program::creature(
            Some(ManaCost::generic(3.into())),
            3.into(),
            3.into(),
            BTreeSet::new(),
        ),
        &v,
    );
    let filler = validated(vanilla(1, 1), &v);

    let game = world(
        v,
        vec![
            card(1, aluren, true),
            SeedCard {
                card: CardId(2),
                owner: P1,
                program: opp_creature,
                destination: ObjectDestination::Hand,
            },
            SeedCard {
                card: CardId(3),
                owner: P1,
                program: filler.clone(),
                destination: ObjectDestination::LibraryTop,
            },
            SeedCard {
                card: CardId(4),
                owner: P0,
                program: filler,
                destination: ObjectDestination::LibraryTop,
            },
        ],
        vec![CardId(1)],
    );
    let game = advance_to_step(game, P1, spellwake_engine::turn::Step::PrecombatMain);
    let payment = CastingPayment {
        use_alternative: false,
        alternative_cost_source: Some(object(&game, 1)),
        ..CastingPayment::default()
    };
    let cast = submit(
        &game,
        P1,
        GameCommand::CastWithResources {
            spell: object(&game, 2),
            target: None,
            payment,
        },
    );
    let p1_pass = submit(cast.state(), P1, GameCommand::Pass);
    let resolved = submit(p1_pass.state(), P0, GameCommand::Pass);
    assert_eq!(
        zone_of(resolved.state(), 2),
        ObjectZone::Battlefield,
        "AlternativeForMatching(each_player: true) must allow opponent to cast for free"
    );
}

#[test]
fn transform_subject_transforms_targeted_dfc_permanent() {
    let v = vocabulary();
    let back_face = Program::creature(None, 5.into(), 5.into(), BTreeSet::new());
    let mut dfc = vanilla(1, 1);
    dfc.transform_back = Some(Box::new(back_face));
    let dfc = validated(dfc, &v);

    let moonmist = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::TransformSubject {
                    subject: Box::new(EffectSubject::TargetObject),
                }],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![card(1, dfc, true), card(2, moonmist, false)],
        vec![CardId(1)],
    );
    let dfc_id = object(&game, 1);
    assert!(
        !game
            .execution()
            .window()
            .cards()
            .get(dfc_id)
            .expect("dfc")
            .transformed(),
        "DFC starts on front face"
    );
    let cast = submit(
        &game,
        P0,
        GameCommand::Cast {
            spell: object(&game, 2),
            target: TargetRef::Object(dfc_id),
        },
    );
    let resolved = pass_pair(cast.state());
    assert!(
        resolved
            .state()
            .execution()
            .window()
            .cards()
            .get(dfc_id)
            .expect("dfc")
            .transformed(),
        "Instruction::TransformSubject must transform the targeted DFC permanent"
    );
}

#[test]
fn set_players_life_sets_dynamic_total_and_works_in_lose_game_replacement() {
    let v = vocabulary();
    let mut archangel = vanilla(5, 5);
    archangel.continuous = Some(StaticContinuousAbility::EventReplacement {
        id: StaticAbilityId(1),
        event: spellwake_engine::characteristics::ReplaceableEvent::LoseGame {
            player: spellwake_engine::program::AffectedPlayers::One(
                spellwake_engine::program::ResolvingPlayer::Controller,
            ),
        },
        condition: None,
        action: spellwake_engine::characteristics::StaticReplacementAction::ReplaceWithBody {
            optional: false,
            body: Box::new(ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::MoveSubject {
                        subject: spellwake_engine::program::MoveSubjectRef::AbilitySource,
                        destination: SubjectDestination::Exile {
                            delayed_return: None,
                        },
                    },
                    Instruction::SetPlayersLife {
                        players: spellwake_engine::program::AffectedPlayers::One(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        amount: ValueExpr::PlayerStat(
                            spellwake_engine::program::PlayerStatExpr::ControllerStartingLife,
                        ),
                    },
                ],
            )),
        },
    });
    let archangel = validated(archangel, &v);

    let set_to_5 = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SetPlayersLife {
                    players: spellwake_engine::program::AffectedPlayers::One(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    amount: ValueExpr::literal(5),
                }],
            ),
        ),
        &v,
    );
    let lethal = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::LoseGame(
                    spellwake_engine::program::AffectedPlayers::One(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                )],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            card(1, archangel, true),
            card(2, set_to_5, false),
            card(3, lethal, false),
        ],
        vec![CardId(1)],
    );
    let c1 = cast_untargeted(&game, P0, 2);
    let after_set = pass_pair(c1.state());
    assert_eq!(
        after_set.state().execution().life_total(P0),
        Some(&5.into()),
        "SetPlayersLife must set controller's life total to 5"
    );

    let c2 = cast_untargeted(after_set.state(), P0, 3);
    let after_replace = pass_pair(c2.state());
    assert_eq!(
        zone_of(after_replace.state(), 1),
        ObjectZone::Exile,
        "LoseGame replacement must exile the archangel"
    );
    assert_eq!(
        after_replace.state().execution().life_total(P0),
        Some(&20.into()),
        "SetPlayersLife(ControllerStartingLife) inside LoseGame replacement must reset life to 20"
    );
}

#[test]
fn activated_cost_scope_targeting_taxes_only_matching_targets() {
    let v = vocabulary();
    let mut kopala = vanilla(2, 2);
    kopala.continuous = Some(StaticContinuousAbility::Modifier {
        id: StaticAbilityId(1),
        rule: spellwake_engine::characteristics::StaticModifierRule::ActivatedAbilityCost {
            scope: spellwake_engine::characteristics::ActivatedCostScope::Targeting(Box::new(
                ObjectFilter::creature().controlled(),
            )),
            controller: spellwake_engine::program::PlayerRelation::Opponent,
            delta: 2,
            less: false,
            scale: None,
            minimum_total_mana: 0,
        },
    });
    let kopala = validated(kopala, &v);

    let mut pinger = vanilla(1, 1);
    pinger.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(BodyTarget::SingleCreature, vec![Instruction::GoadTarget]),
    });
    let pinger = validated(pinger, &v);

    let game = world(
        v,
        vec![
            card(1, kopala, true),
            SeedCard {
                card: CardId(2),
                owner: P1,
                program: pinger,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
        ],
        vec![CardId(1), CardId(2)],
    );
    let g = submit(&game, P0, GameCommand::Pass).into_state();

    // Negative: targeting P0's Kopala (creature P0 controls) MUST fail with 0 mana due to {2} tax!
    let taxed = try_submit(
        &g,
        P1,
        GameCommand::ActivateAbility {
            source: object(&g, 2),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(object(&g, 1)),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        taxed.is_err(),
        "ActivatedCostScope::Targeting must tax opponent abilities targeting a matching creature"
    );

    // Positive: targeting P1's own pinger MUST succeed for {0}!
    let untaxed = try_submit(
        &g,
        P1,
        GameCommand::ActivateAbility {
            source: object(&g, 2),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(object(&g, 2)),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        untaxed.is_ok(),
        "ActivatedCostScope::Targeting must NOT tax abilities targeting non-matching permanents"
    );
}

#[test]
fn impulse_play_duration_until_next_end_step_and_until_next_upkeep_expire_on_step() {
    let v = vocabulary();
    let exile_until_end = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ExileTopPlay {
                    count: ValueExpr::literal(1),
                    library_owner: spellwake_engine::program::AffectedPlayers::One(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    duration: spellwake_engine::program::ImpulsePlayDuration::UntilNextEndStep,
                    cast_only: false,
                    free: false,
                    any_color: false,
                    face_down: false,
                    grant_play: true,
                }],
            ),
        ),
        &v,
    );
    let exiled_bear = validated(vanilla(2, 2), &v);
    let filler = validated(vanilla(1, 1), &v);

    let game = world(
        v,
        vec![
            card(1, exile_until_end, false),
            SeedCard {
                card: CardId(2),
                owner: P0,
                program: exiled_bear,
                destination: ObjectDestination::LibraryTop,
            },
            SeedCard {
                card: CardId(3),
                owner: P1,
                program: filler,
                destination: ObjectDestination::LibraryTop,
            },
        ],
        vec![],
    );
    // Cast exile_until_end during P0's precombat main -> CardId 2 exiled with UntilNextEndStep(P0)
    let c1 = cast_untargeted(&game, P0, 1);
    let after_exile = pass_pair(c1.state()).into_state();
    assert_eq!(zone_of(&after_exile, 2), ObjectZone::Exile);

    // Positive: playable during P0's precombat main (before P0's next end step)
    let playable_before = try_submit(
        &after_exile,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&after_exile, 2),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        playable_before.is_ok(),
        "UntilNextEndStep grant must be playable before grantee's next end step"
    );

    // Negative: advance to P0's End step -> permission expires upon entering Step::End!
    let at_end = advance_to_step(after_exile, P0, spellwake_engine::turn::Step::End);
    let expired = try_submit(
        &at_end,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&at_end, 2),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        expired.is_err(),
        "UntilNextEndStep grant must expire when grantee's next end step begins"
    );
}

#[test]
fn impulse_play_duration_until_next_upkeep_survives_end_step_and_expires_on_upkeep() {
    let v = vocabulary();
    let elkin_bottle = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ExileTopPlay {
                    count: ValueExpr::literal(1),
                    library_owner: spellwake_engine::program::AffectedPlayers::One(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    duration: spellwake_engine::program::ImpulsePlayDuration::UntilNextUpkeep,
                    cast_only: false,
                    free: false,
                    any_color: false,
                    face_down: false,
                    grant_play: true,
                }],
            ),
        ),
        &v,
    );
    let exiled_instant = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(3),
                }],
            ),
        ),
        &v,
    );
    let filler = validated(vanilla(1, 1), &v);

    let game = world(
        v,
        vec![
            card(1, elkin_bottle, false),
            SeedCard {
                card: CardId(4),
                owner: P0,
                program: filler.clone(),
                destination: ObjectDestination::LibraryTop,
            },
            SeedCard {
                card: CardId(2),
                owner: P0,
                program: exiled_instant,
                destination: ObjectDestination::LibraryTop,
            },
            SeedCard {
                card: CardId(3),
                owner: P1,
                program: filler,
                destination: ObjectDestination::LibraryTop,
            },
        ],
        vec![],
    );
    let c1 = cast_untargeted(&game, P0, 1);
    let after_exile = pass_pair(c1.state()).into_state();
    assert_eq!(zone_of(&after_exile, 2), ObjectZone::Exile);

    // Positive: still playable on P1's main phase (after P0's end step!)
    let on_p1_turn = advance_to_step(after_exile, P1, spellwake_engine::turn::Step::PrecombatMain);
    let on_p1_p0_prio = submit(&on_p1_turn, P1, GameCommand::Pass).into_state();
    let playable_on_p1 = try_submit(
        &on_p1_p0_prio,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&on_p1_p0_prio, 2),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        playable_on_p1.is_ok(),
        "UntilNextUpkeep grant must survive past P0's end step into P1's turn"
    );

    // Negative: advance to P0's next Upkeep -> expires upon entering Step::Upkeep!
    let at_p0_upkeep = advance_to_step(on_p1_p0_prio, P0, spellwake_engine::turn::Step::Upkeep);
    let expired = try_submit(
        &at_p0_upkeep,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&at_p0_upkeep, 2),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        expired.is_err(),
        "UntilNextUpkeep grant must expire when grantee's next upkeep begins"
    );
}
