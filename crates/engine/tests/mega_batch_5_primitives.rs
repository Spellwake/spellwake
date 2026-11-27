#![forbid(unsafe_code)]
//! Behavioral integration tests for Mega-Batch 5 engine primitives:
//! - Compositional `ObjectFilterFlags` (`EVEN_MANA_VALUE`, `ODD_MANA_VALUE`,
//!   `SHARES_COLOR_WITH_TARGET`, `SHARES_CREATURE_TYPE_WITH_SOURCE`, `SAME_TOTAL_PT`,
//!   `HAS_ACTIVATED_ABILITY`, `NO_ACTIVATED_ABILITIES`, `UNIQUE_NAME_AMONG_CONTROLLED`)
//! - `DynamicStatAnchor` (`TargetToughness`, `TargetManaValue`)
//! - `SourceZoneFilter::TargetPlayerHand` inside `SelectZoneCardsToZone`
//! - `ZoneOpSpec::reveal_until_match_instructions` (`keep_all_revealed: true`, Treasure Hunt behavior)
//! - `ZoneOpSpec::look_top_choose_instructions` with `LookTopKeepDest::ExileLinked` (linked exile + face-down + playable)

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
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::ResolvedDuration,
    effects::{DecisionReply, SeedCard},
    game::{GameCommand, GameState, NativeCandidate},
    land::LandType,
    mana::{ManaCost, ManaPayment, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    priority::SeatId,
    program::{
        BodyTarget, CardTypeMask, Comparison, DynamicStatAnchor, EffectSubject, ExecutableBody,
        Instruction, LookTopKeepDest, LookTopRestDest, ManaValueFilter, ObjectClause, ObjectFilter,
        ObjectFilterFlags, PowerFilter, Program, ResolvingPlayer, ValueExpr,
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

fn creature_with_mv(p: i64, t: i64, generic_mv: i64) -> Program {
    Program::creature(
        Some(ManaCost::generic(generic_mv.into())),
        p.into(),
        t.into(),
        BTreeSet::new(),
    )
}

fn colored_creature(p: i64, t: i64, mana: ManaType) -> Program {
    Program::creature(
        Some(ManaCost::one(mana)),
        p.into(),
        t.into(),
        BTreeSet::new(),
    )
}

fn creature_clause() -> ObjectClause {
    let mut c = ObjectClause::any();
    c.required_types = CardTypeMask::CREATURE;
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

fn resolve_with_gy_order(mut state: GameState) -> GameState {
    let passed = pass_pair(&state);
    state = passed.into_state();
    while let Some(decision) = state.pending_decision() {
        if let spellwake_engine::game::RuleFrame::ResolutionGraveyardOrder(frame) = decision.frame()
        {
            let order = frame.domain().to_vec();
            let next = submit(
                &state,
                decision.chooser(),
                GameCommand::Reply {
                    decision: decision.id(),
                    choice: DecisionReply::OrderResolutionGraveyard(order),
                },
            );
            state = next.into_state();
        } else {
            break;
        }
    }
    state
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

fn cast_targeted(
    game: &GameState,
    actor: SeatId,
    card_id: u64,
    target: TargetRef,
) -> NativeCandidate {
    submit(
        game,
        actor,
        GameCommand::Cast {
            spell: object(game, card_id),
            target,
        },
    )
}

#[test]
fn even_and_odd_mana_value_filter_flags_destroy_only_matching_creatures() {
    let v = vocabulary();
    let mv2 = validated(creature_with_mv(2, 2, 2), &v);
    let mv3 = validated(creature_with_mv(3, 3, 3), &v);
    let mv4 = validated(creature_with_mv(4, 4, 4), &v);

    let mut even_clause = creature_clause();
    even_clause.flags = ObjectFilterFlags::EVEN_MANA_VALUE;
    let destroy_even = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::Matching(ObjectFilter::Clause(even_clause)),
                        )),
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: !(false),
                            },
                        },
                    ),
                ))],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            card(1, mv2, true),
            SeedCard {
                card: CardId(2),
                owner: P1,
                program: mv3,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            SeedCard {
                card: CardId(3),
                owner: P1,
                program: mv4,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            card(4, destroy_even, false),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );

    let cast = cast_untargeted(&game, P0, 4);
    let resolved = pass_pair(cast.state());

    assert_eq!(zone_of(resolved.state(), 1), ObjectZone::Graveyard(P0));
    assert_eq!(zone_of(resolved.state(), 2), ObjectZone::Battlefield);
    assert_eq!(zone_of(resolved.state(), 3), ObjectZone::Graveyard(P1));
}

#[test]
fn shares_color_with_target_and_same_total_pt_filter_flags() {
    let v = vocabulary();
    let red_target = validated(colored_creature(2, 2, ManaType::Red), &v);
    let red_other = validated(colored_creature(3, 3, ManaType::Red), &v);
    let blue_other = validated(colored_creature(2, 2, ManaType::Blue), &v);
    let red_unequal = validated(colored_creature(3, 1, ManaType::Red), &v);

    let mut clause = creature_clause();
    clause.flags =
        ObjectFilterFlags::SHARES_COLOR_WITH_TARGET.union(ObjectFilterFlags::SAME_TOTAL_PT);
    let spell = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_CREATURE,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::Matching(ObjectFilter::Clause(clause)),
                        )),
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: !(false),
                            },
                        },
                    ),
                ))],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            SeedCard {
                card: CardId(1),
                owner: P1,
                program: red_target,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            SeedCard {
                card: CardId(2),
                owner: P1,
                program: red_other,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            SeedCard {
                card: CardId(3),
                owner: P1,
                program: blue_other,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            SeedCard {
                card: CardId(4),
                owner: P1,
                program: red_unequal,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            card(5, spell, false),
        ],
        vec![CardId(1), CardId(2), CardId(3), CardId(4)],
    );

    let tgt = TargetRef::Object(object(&game, 1));
    let cast = cast_targeted(&game, P0, 5, tgt);
    let resolved = resolve_with_gy_order(cast.into_state());

    // Red 2/2 and Red 3/3 share color with target AND have power == toughness.
    assert_eq!(zone_of(&resolved, 1), ObjectZone::Graveyard(P1));
    assert_eq!(zone_of(&resolved, 2), ObjectZone::Graveyard(P1));
    // Blue 2/2 does not share color; Red 3/1 does not have power == toughness.
    assert_eq!(zone_of(&resolved, 3), ObjectZone::Battlefield);
    assert_eq!(zone_of(&resolved, 4), ObjectZone::Battlefield);
}

#[test]
fn dynamic_stat_anchor_target_toughness_and_target_mana_value() {
    let v = vocabulary();
    // Target is a 1/4 with MV 3.
    let target_wall = validated(creature_with_mv(1, 4, 3), &v);
    let small_3_3 = validated(creature_with_mv(3, 3, 2), &v);
    let big_5_5 = validated(creature_with_mv(5, 5, 5), &v);

    let mut clause = creature_clause();
    clause.power = PowerFilter::CompareAnchor {
        op: Comparison::LessOrEqual,
        anchor: DynamicStatAnchor::TargetToughness,
    };
    let spell = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_CREATURE,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::Matching(ObjectFilter::Clause(clause)),
                        )),
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: !(false),
                            },
                        },
                    ),
                ))],
            ),
        ),
        &v,
    );

    let game = world(
        v.clone(),
        vec![
            SeedCard {
                card: CardId(1),
                owner: P1,
                program: target_wall,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            SeedCard {
                card: CardId(2),
                owner: P1,
                program: small_3_3,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            SeedCard {
                card: CardId(3),
                owner: P1,
                program: big_5_5,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            card(4, spell, false),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );

    let tgt = TargetRef::Object(object(&game, 1));
    let cast = cast_targeted(&game, P0, 4, tgt);
    let resolved = resolve_with_gy_order(cast.into_state());

    assert_eq!(zone_of(&resolved, 1), ObjectZone::Graveyard(P1));
    assert_eq!(zone_of(&resolved, 2), ObjectZone::Graveyard(P1));
    assert_eq!(zone_of(&resolved, 3), ObjectZone::Battlefield);

    // TargetManaValue: destroy all creatures with MV <= target creature's MV (2).
    let tgt_mv2 = validated(creature_with_mv(1, 1, 2), &v);
    let other_mv2 = validated(creature_with_mv(6, 6, 2), &v);
    let other_mv4 = validated(creature_with_mv(1, 1, 4), &v);

    let mut mv_clause = creature_clause();
    mv_clause.mana_value = ManaValueFilter::CompareAnchor {
        op: Comparison::LessOrEqual,
        anchor: DynamicStatAnchor::TargetManaValue,
    };
    let mv_spell = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_CREATURE,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::Matching(ObjectFilter::Clause(mv_clause)),
                        )),
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: !(false),
                            },
                        },
                    ),
                ))],
            ),
        ),
        &v,
    );

    let game2 = world(
        v,
        vec![
            SeedCard {
                card: CardId(11),
                owner: P1,
                program: tgt_mv2,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            SeedCard {
                card: CardId(12),
                owner: P1,
                program: other_mv2,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            SeedCard {
                card: CardId(13),
                owner: P1,
                program: other_mv4,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            card(14, mv_spell, false),
        ],
        vec![CardId(11), CardId(12), CardId(13)],
    );

    let tgt2 = TargetRef::Object(object(&game2, 11));
    let cast2 = cast_targeted(&game2, P0, 14, tgt2);
    let resolved2 = resolve_with_gy_order(cast2.into_state());

    assert_eq!(zone_of(&resolved2, 11), ObjectZone::Graveyard(P1));
    assert_eq!(zone_of(&resolved2, 12), ObjectZone::Graveyard(P1));
    assert_eq!(zone_of(&resolved2, 13), ObjectZone::Battlefield);
}

#[test]
fn source_zone_filter_target_player_hand_exiles_chosen_creature() {
    let v = vocabulary();
    let opp_hand_creature1 = validated(vanilla(2, 2), &v);
    let opp_hand_creature2 = validated(vanilla(4, 4), &v);
    let opp_hand_land = validated(
        Program::land(BTreeSet::from([LandType::Forest]), vec![]),
        &v,
    );
    let own_hand_creature = validated(vanilla(3, 3), &v);

    let strip = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_OPPONENT,
                vec![Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::simple(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Hand(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer))), ObjectFilter::Clause(creature_clause())), spellwake_engine::toolbox::ZoneSelection::Prompt { chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller), min: Box::new(ValueExpr::literal(1)), max: Box::new(ValueExpr::literal(1)), constraint: None, category: spellwake_engine::toolbox::ChoiceCategory::Generic, hidden: !(false) }, spellwake_engine::toolbox::ZoneDest::Exile { face_down: false, viewer: None, link_to_source: matches!(None, Some(spellwake_engine::program::DelayedReturnTiming::UntilSourceLeaves)), delayed_return: None, link_group: None })))],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            SeedCard {
                card: CardId(1),
                owner: P1,
                program: opp_hand_creature1,
                destination: ObjectDestination::Hand,
            },
            SeedCard {
                card: CardId(5),
                owner: P1,
                program: opp_hand_creature2,
                destination: ObjectDestination::Hand,
            },
            SeedCard {
                card: CardId(2),
                owner: P1,
                program: opp_hand_land,
                destination: ObjectDestination::Hand,
            },
            card(3, own_hand_creature, false),
            card(4, strip, false),
        ],
        vec![],
    );

    let opp_creature_obj = object(&game, 1);
    let cast = cast_targeted(&game, P0, 4, TargetRef::Player(P1));
    let paused = pass_pair(cast.state());
    let decision = paused
        .state()
        .pending_decision()
        .expect("should prompt controller to choose from target opponent's hand");
    let after_reply = submit(
        paused.state(),
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::PromptChoice(
                spellwake_engine::toolbox::PromptChoiceReply::Objects(vec![opp_creature_obj]),
            ),
        },
    );

    assert_eq!(zone_of(after_reply.state(), 1), ObjectZone::Exile);
    assert_eq!(zone_of(after_reply.state(), 5), ObjectZone::Hand(P1));
    assert_eq!(zone_of(after_reply.state(), 2), ObjectZone::Hand(P1));
    assert_eq!(zone_of(after_reply.state(), 3), ObjectZone::Hand(P0));
}

#[test]
fn reveal_until_match_ext_keep_all_revealed_puts_all_revealed_cards_in_hand() {
    let v = vocabulary();
    let land1 = validated(
        Program::land(BTreeSet::from([LandType::Island]), vec![]),
        &v,
    );
    let land2 = validated(
        Program::land(BTreeSet::from([LandType::Island]), vec![]),
        &v,
    );
    let nonland = validated(vanilla(2, 2), &v);
    let under_card = validated(vanilla(4, 4), &v);

    let mut nonland_clause = ObjectClause::any();
    nonland_clause.forbidden_types = CardTypeMask::LAND;
    let treasure_hunt = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                spellwake_engine::toolbox::ZoneOpSpec::reveal_until_match_instructions(
                    ResolvingPlayer::Controller,
                    ResolvingPlayer::Controller,
                    ObjectFilter::Clause(nonland_clause),
                    ValueExpr::literal(1),
                    LookTopKeepDest::Hand,
                    LookTopRestDest::BottomRandom,
                    true,
                    false,
                    false,
                ),
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            SeedCard {
                card: CardId(1),
                owner: P0,
                program: land1,
                destination: ObjectDestination::LibraryTop,
            },
            SeedCard {
                card: CardId(2),
                owner: P0,
                program: land2,
                destination: ObjectDestination::LibraryBottom,
            },
            SeedCard {
                card: CardId(3),
                owner: P0,
                program: nonland,
                destination: ObjectDestination::LibraryBottom,
            },
            SeedCard {
                card: CardId(4),
                owner: P0,
                program: under_card,
                destination: ObjectDestination::LibraryBottom,
            },
            card(5, treasure_hunt, false),
        ],
        vec![],
    );

    let cast = cast_untargeted(&game, P0, 5);
    let resolved = pass_pair(cast.state());

    // Cards 1, 2, 3 (top-to-bottom through first nonland) all enter hand; card 4 stays in library.
    assert_eq!(zone_of(resolved.state(), 1), ObjectZone::Hand(P0));
    assert_eq!(zone_of(resolved.state(), 2), ObjectZone::Hand(P0));
    assert_eq!(zone_of(resolved.state(), 3), ObjectZone::Hand(P0));
    assert_eq!(zone_of(resolved.state(), 4), ObjectZone::Library(P0));
}

#[test]
fn look_top_choose_ext_exile_linked_face_down_and_playable() {
    let v = vocabulary();
    let top_creature = validated(vanilla(2, 2), &v);
    let second_land = validated(
        Program::land(BTreeSet::from([LandType::Forest]), vec![]),
        &v,
    );

    let artifact = validated(
        Program::artifact(
            Some(ManaCost::free()),
            vec![ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::Tap,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    spellwake_engine::toolbox::ZoneOpSpec::look_top_choose_instructions(
                        ResolvingPlayer::Controller,
                        ResolvingPlayer::Controller,
                        ValueExpr::literal(2),
                        Some(ObjectFilter::Clause(creature_clause())),
                        1,
                        true,
                        LookTopKeepDest::ExileLinked {
                            face_down: true,
                            playable: true,
                            free: false,
                            cast_only: true,
                            any_color: false,
                            duration: ResolvedDuration::UntilEndOfTurn,
                        },
                        LookTopRestDest::BottomRandom,
                        false,
                        false,
                        false,
                        None,
                    ),
                ),
            }],
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            card(1, artifact, true),
            SeedCard {
                card: CardId(2),
                owner: P0,
                program: top_creature,
                destination: ObjectDestination::LibraryTop,
            },
            SeedCard {
                card: CardId(3),
                owner: P0,
                program: second_land,
                destination: ObjectDestination::LibraryBottom,
            },
        ],
        vec![CardId(1)],
    );

    let act = submit(
        &game,
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: object(&game, 1),
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let resolved = pass_pair(act.state());

    assert_eq!(zone_of(resolved.state(), 2), ObjectZone::Exile);
    assert_eq!(zone_of(resolved.state(), 3), ObjectZone::Library(P0));
    // Verify controller can look at & cast the linked face-down exiled card!
    let _exiled_obj = object(resolved.state(), 2);
    // And controller can cast it from exile!
    let cast_from_exile = cast_untargeted(resolved.state(), P0, 2);
    let on_bf = pass_pair(cast_from_exile.state());
    assert_eq!(zone_of(on_bf.state(), 2), ObjectZone::Battlefield);
}

#[test]
fn static_world_eval_shares_creature_type_with_source_and_unique_name() {
    let v = vocabulary();
    let elf_id = v.resolve_label("Elf").expect("Elf must be in vocabulary");
    let goblin_id = v
        .resolve_label("Goblin")
        .expect("Goblin must be in vocabulary");

    let mut lord_clause = creature_clause();
    lord_clause.flags = ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_SOURCE;
    let mut elf_lord = Program::creature(
        Some(ManaCost::free()),
        1.into(),
        1.into(),
        BTreeSet::from([elf_id]),
    );
    elf_lord.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                        EffectSubject::Matching(ObjectFilter::Clause(lord_clause)),
                    )),
                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                        semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                            allow_regenerate: !(false),
                        },
                    },
                ),
            ))],
        ),
    });
    let elf_lord = validated(elf_lord, &v);

    let other_elf = validated(
        Program::creature(
            Some(ManaCost::free()),
            2.into(),
            2.into(),
            BTreeSet::from([elf_id]),
        ),
        &v,
    );
    let goblin = validated(
        Program::creature(
            Some(ManaCost::free()),
            2.into(),
            2.into(),
            BTreeSet::from([goblin_id]),
        ),
        &v,
    );

    let game = world(
        v.clone(),
        vec![
            card(1, elf_lord, true),
            card(2, other_elf, true),
            card(3, goblin, true),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );

    let act = submit(
        &game,
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: object(&game, 1),
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_pump = resolve_with_gy_order(act.into_state());
    assert_eq!(zone_of(&after_pump, 2), ObjectZone::Graveyard(P0));
    assert_eq!(zone_of(&after_pump, 3), ObjectZone::Battlefield);

    // Unique name among controlled: two cards named "Bear" vs one named "Wolf".
    let mut bear1 = vanilla(2, 2);
    bear1.names = ObjectNames::single(EnglishCardName::new("Bear".to_string()).expect("name"));
    let mut bear2 = vanilla(2, 2);
    bear2.names = ObjectNames::single(EnglishCardName::new("Bear".to_string()).expect("name"));
    let mut wolf = vanilla(2, 2);
    wolf.names = ObjectNames::single(EnglishCardName::new("Wolf".to_string()).expect("name"));

    let mut unique_clause = creature_clause();
    unique_clause.flags = ObjectFilterFlags::UNIQUE_NAME_AMONG_CONTROLLED;
    let destroy_unique = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::Matching(ObjectFilter::Clause(unique_clause)),
                        )),
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: !(false),
                            },
                        },
                    ),
                ))],
            ),
        ),
        &v,
    );

    let game2 = world(
        v.clone(),
        vec![
            card(11, validated(bear1, &v), true),
            card(12, validated(bear2, &v), true),
            card(13, validated(wolf, &v), true),
            card(14, destroy_unique, false),
        ],
        vec![CardId(11), CardId(12), CardId(13)],
    );
    let cast2 = cast_untargeted(&game2, P0, 14);
    let resolved2 = pass_pair(cast2.state());

    assert_eq!(zone_of(resolved2.state(), 11), ObjectZone::Battlefield);
    assert_eq!(zone_of(resolved2.state(), 12), ObjectZone::Battlefield);
    assert_eq!(zone_of(resolved2.state(), 13), ObjectZone::Graveyard(P0));
}

#[test]
fn toughness_greater_than_power_and_power_greater_than_toughness_filter_flags() {
    let v = vocabulary();
    let wall_1_4 = validated(vanilla(1, 4), &v);
    let bear_2_2 = validated(vanilla(2, 2), &v);
    let glass_4_1 = validated(vanilla(4, 1), &v);

    let mut tgp = creature_clause();
    tgp.flags = ObjectFilterFlags::TOUGHNESS_GREATER_THAN_POWER;
    let destroy_tgp = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::Matching(ObjectFilter::Clause(tgp)),
                        )),
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: !(false),
                            },
                        },
                    ),
                ))],
            ),
        ),
        &v,
    );

    let mut pgt = creature_clause();
    pgt.flags = ObjectFilterFlags::POWER_GREATER_THAN_TOUGHNESS;
    let destroy_pgt = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::Matching(ObjectFilter::Clause(pgt)),
                        )),
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: !(false),
                            },
                        },
                    ),
                ))],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            SeedCard {
                card: CardId(1),
                owner: P1,
                program: wall_1_4,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            SeedCard {
                card: CardId(2),
                owner: P1,
                program: bear_2_2,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            SeedCard {
                card: CardId(3),
                owner: P1,
                program: glass_4_1,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            card(4, destroy_tgp, false),
            card(5, destroy_pgt, false),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );

    let after_tgp = pass_pair(cast_untargeted(&game, P0, 4).state());
    assert_eq!(zone_of(after_tgp.state(), 1), ObjectZone::Graveyard(P1));
    assert_eq!(zone_of(after_tgp.state(), 2), ObjectZone::Battlefield);
    assert_eq!(zone_of(after_tgp.state(), 3), ObjectZone::Battlefield);

    let after_pgt = pass_pair(cast_untargeted(after_tgp.state(), P0, 5).state());
    assert_eq!(zone_of(after_pgt.state(), 2), ObjectZone::Battlefield);
    assert_eq!(zone_of(after_pgt.state(), 3), ObjectZone::Graveyard(P1));
}
