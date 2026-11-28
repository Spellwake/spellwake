//! Sourced ordinary banding declarations and damage assignment exceptions.
use super::cmm_gameplay::{creature, named};
use super::*;
use spellwake_engine::combat::CombatDamageAssignment;

fn at_damage(mut state: GameState) -> GameState {
    for _ in 0..16 {
        if matches!(
            state.pending_decision().map(|pending| pending.frame()),
            Some(RuleFrame::Combat(CombatDecision::DamageAssignments(_)))
        ) {
            return state;
        }
        state = pass(&state).into_state();
    }
    panic!("unresolved damage assignment expected")
}

fn attack(state: &GameState, cards: &[u64], defender: SeatId) -> Vec<AttackDeclaration> {
    cards
        .iter()
        .map(|&id| AttackDeclaration {
            attacker: physical(state, id).id(),
            defender,
            defending_planeswalker: None,
        })
        .collect()
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn banding_declared_band_blocks_flyer_and_lets_active_player_assign_blocker_damage() {
    let state = game(
        vec![A, B],
        vec![
            named(
                "Benalish Hero",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(2, A, 1, 4, &[KeywordAbility::Flying]),
            creature(3, B, 3, 4, &[]),
            creature(4, A, 1, 1, &[]),
        ],
        &[],
        Step::Untap,
    );
    let ready = combat_frame(&state, true);
    let [hero, flyer, blocker, other] = [1, 2, 3, 4].map(|id| physical(&ready, id).id());
    let RuleFrame::Combat(CombatDecision::Attackers(frame)) =
        ready.pending_decision().unwrap().frame()
    else {
        panic!()
    };
    assert_eq!(frame.banding_attackers(), &[hero].into());
    let declarations = attack(&ready, &[1, 2, 4], B);
    for bands in [
        vec![vec![hero, flyer, other]],
        vec![vec![hero, flyer], vec![hero]],
        vec![vec![flyer]],
        vec![vec![blocker, hero]],
    ] {
        let result = ready.advance(GameInput {
            basis: ready.basis().clone(),
            actor: A,
            command: GameCommand::Reply {
                decision: ready.pending_decision().unwrap().id(),
                choice: DecisionReply::DeclareBandedAttackers {
                    attackers: declarations.clone(),
                    bands,
                },
            },
        });
        assert!(result.is_err());
    }
    let attacking = reply(
        &ready,
        DecisionReply::DeclareBandedAttackers {
            attackers: attack(&ready, &[1, 2], B),
            bands: vec![vec![hero, flyer]],
        },
    )
    .into_state();
    let choosing_blocks = combat_frame(&attacking, false);
    let RuleFrame::Combat(CombatDecision::Blockers(frame)) =
        choosing_blocks.pending_decision().unwrap().frame()
    else {
        panic!()
    };
    assert!(!frame.legal_attackers(blocker).contains(&flyer));
    let blocked = reply(
        &choosing_blocks,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker,
            attacker: hero,
        }]),
    )
    .into_state();
    assert_eq!(
        blocked
            .execution()
            .combat()
            .unwrap()
            .blocker(blocker)
            .unwrap()
            .attackers(),
        &[hero, flyer].into()
    );
    let damage = at_damage(blocked);
    assert_eq!(damage.pending_decision().unwrap().chooser(), A);
    let RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) =
        damage.pending_decision().unwrap().frame()
    else {
        panic!()
    };
    let choices = frame
        .domains()
        .iter()
        .map(|domain| CombatDamageAssignment {
            source: domain.source(),
            target: DamageRecipient::Creature(if domain.source() == blocker {
                hero
            } else {
                blocker
            }),
            amount: domain.power().clone(),
        })
        .collect();
    let end = reply(&damage, DecisionReply::AssignCombatDamage(choices)).into_state();
    assert_eq!(physical(&end, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&end, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(
        end.execution().battlefield_marked_damage(blocker),
        Some(2.into())
    );
    assert_eq!(end.execution().life_total(B), Some(&20.into()));
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn banding_blocker_assigns_attacking_trample_damage_without_a_declared_band() {
    let state = game(
        vec![A, B],
        vec![
            creature(1, A, 7, 7, &[KeywordAbility::Trample]),
            named(
                "Benalish Hero",
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            creature(3, B, 1, 4, &[]),
        ],
        &[],
        Step::Untap,
    );
    let ready = combat_frame(&state, true);
    let [attacker, hero, other] = [1, 2, 3].map(|id| physical(&ready, id).id());
    let attacking = reply(
        &ready,
        DecisionReply::DeclareAttackers(attack(&ready, &[1], B)),
    )
    .into_state();
    let blocks = combat_frame(&attacking, false);
    let blocked = reply(
        &blocks,
        DecisionReply::DeclareBlockers(vec![
            BlockDeclaration {
                blocker: hero,
                attacker,
            },
            BlockDeclaration {
                blocker: other,
                attacker,
            },
        ]),
    )
    .into_state();
    let damage = at_damage(blocked);
    assert_eq!(damage.pending_decision().unwrap().chooser(), B);
    let RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) =
        damage.pending_decision().unwrap().frame()
    else {
        panic!()
    };
    let choices = frame
        .domains()
        .iter()
        .map(|domain| CombatDamageAssignment {
            source: domain.source(),
            target: DamageRecipient::Creature(if domain.source() == attacker {
                other
            } else {
                attacker
            }),
            amount: domain.power().clone(),
        })
        .collect();
    let end = reply(&damage, DecisionReply::AssignCombatDamage(choices)).into_state();
    assert_eq!(end.execution().life_total(B), Some(&20.into()));
    assert_eq!(physical(&end, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&end, 3).zone(), ObjectZone::Graveyard(B));
}

fn band_subject(
    id: u64,
    owner: SeatId,
    color: spellwake_engine::characteristics::Color,
    legendary: bool,
) -> SeedCard {
    let mut card = Program::creature(None, 2.into(), 4.into(), Default::default());
    card.names = ObjectNames::single(EnglishCardName::new(format!("Band subject {id}")).unwrap());
    card.color_indicator = Some([color].into());
    if legendary {
        card.supertypes.insert(Supertype::Legendary);
    }
    seed(
        id,
        owner,
        ObjectDestination::Battlefield { controller: owner },
        card,
    )
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn banding_five_legendary_grant_lands_allow_multiple_unqualified_leaders_only_when_all_members_match()
 {
    use spellwake_engine::characteristics::Color::*;
    use spellwake_engine::combat::BandQuality::LegendaryCreatures;
    for (land, color, other) in [
        ("Cathedral of Serra", White, Blue),
        ("Seafarer's Quay", Blue, Black),
        ("Unholy Citadel", Black, Red),
        ("Mountain Stronghold", Red, Green),
        ("Adventurers' Guildhouse", Green, White),
    ] {
        let state = game(
            vec![A, B],
            vec![
                named(land, 1, A, ObjectDestination::Battlefield { controller: A }),
                band_subject(2, A, color, true),
                band_subject(3, A, other, true),
                band_subject(4, A, other, true),
                band_subject(5, A, color, false),
                band_subject(6, B, color, true),
            ],
            &[],
            Step::Untap,
        );
        let ready = combat_frame(&state, true);
        let id = |n| physical(&ready, n).id();
        for (n, granted) in [(2, true), (3, false), (4, false), (5, false), (6, false)] {
            assert_eq!(
                ready
                    .execution()
                    .current_characteristics(id(n))
                    .unwrap()
                    .unwrap()
                    .has_keyword(KeywordAbility::BandsWithOther(LegendaryCreatures)),
                granted
            );
        }
        let band = |members: &[u64]| DecisionReply::DeclareBandedAttackers {
            attackers: attack(&ready, &[2, 3, 4, 5], B),
            bands: vec![members.iter().map(|&n| id(n)).collect()],
        };
        for invalid in [&[2, 5][..], &[3, 4]] {
            assert!(
                ready
                    .advance(GameInput {
                        actor: A,
                        basis: ready.basis().clone(),
                        command: GameCommand::Reply {
                            decision: ready.pending_decision().unwrap().id(),
                            choice: band(invalid)
                        }
                    })
                    .is_err()
            );
        }
        let accepted = reply(&ready, band(&[2, 3, 4])).into_state();
        assert_eq!(
            accepted.execution().combat().unwrap().attacking_bands()[0],
            [id(2), id(3), id(4)].into()
        );
    }
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn banding_master_of_the_hunt_makes_named_wolves_with_matching_band_quality() {
    use spellwake_engine::activated::ActivatedAbilityId;
    use spellwake_engine::combat::BandQuality::CreaturesNamedWolvesOfTheHunt;
    let state = game(
        vec![A, B],
        vec![named(
            "Master of the Hunt",
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
        )],
        &[(ManaType::Green, 4)],
        Step::PrecombatMain,
    );
    let paid = command(
        &state,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: physical(&state, 1).id(),
            ability: ActivatedAbilityId(0),
            payment: Some(spend(&[(ManaType::Green, 4)])),
        },
    )
    .into_state();
    let made = round(paid);
    let wolf = tokens(&made, A, 1)[0];
    let view = made
        .state()
        .execution()
        .current_characteristics(wolf)
        .unwrap()
        .unwrap();
    assert!(view.has_keyword(KeywordAbility::BandsWithOther(
        CreaturesNamedWolvesOfTheHunt
    )));
    assert!(made.state().execution().name_rules().has_name(
        &view.current_names,
        &EnglishCardName::new("Wolves of the Hunt".into()).unwrap()
    ));
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn banding_shelkin_brownie_removes_only_qualified_banding_and_tolaria_requires_upkeep() {
    use spellwake_engine::activated::ActivatedAbilityId;
    use spellwake_engine::combat::BandQuality::LegendaryCreatures;
    for (name, step, allowed, ordinary_survives) in [
        ("Shelkin Brownie", Step::PrecombatMain, true, true),
        ("Tolaria", Step::Upkeep, true, false),
        ("Tolaria", Step::PrecombatMain, false, true),
    ] {
        let mut subject = Program::creature(None, 2.into(), 4.into(), Default::default());
        subject.printed_keywords.extend([
            KeywordAbility::Banding,
            KeywordAbility::Flying,
            KeywordAbility::BandsWithOther(LegendaryCreatures),
        ]);
        let mut state = game(
            vec![A, B],
            vec![
                named(name, 1, A, ObjectDestination::Battlefield { controller: A }),
                seed(
                    2,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    subject,
                ),
            ],
            &[],
            Step::Untap,
        );
        // Establish control at the real turn-start cut before paying a creature tap cost.
        for _ in 0..12 {
            if state.turn().unwrap().position().step == step {
                break;
            }
            state = pass(&state).into_state();
        }
        assert_eq!(state.turn().unwrap().position().step, step);
        let target = physical(&state, 2).id();
        let action = GameCommand::ActivateAbility {
            source: physical(&state, 1).id(),
            ability: ActivatedAbilityId(0),
            target: TargetRef::Object(target),
            payment: None,
        };
        let chosen = state.advance(GameInput {
            actor: A,
            basis: state.basis().clone(),
            command: action,
        });
        if !allowed {
            assert!(chosen.is_err());
            continue;
        }
        let end = round(chosen.unwrap().into_state()).into_state();
        let view = end
            .execution()
            .current_characteristics(target)
            .unwrap()
            .unwrap();
        assert_eq!(view.has_keyword(KeywordAbility::Banding), ordinary_survives);
        assert!(view.has_keyword(KeywordAbility::Flying));
        assert!(!view.has_keyword(KeywordAbility::BandsWithOther(LegendaryCreatures)));
    }
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn banding_declared_members_remain_banded_after_land_grant_leaves_but_damage_uses_current_abilities()
 {
    use spellwake_engine::characteristics::Color::{Blue, White};
    let state = game(
        vec![A, B],
        vec![
            named(
                "Cathedral of Serra",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            band_subject(2, A, White, true),
            band_subject(3, A, Blue, true),
            creature(4, B, 3, 6, &[]),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_PERMANENT,
                    crate::keyword_creatures::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                    allow_regenerate: true,
                                },
                            },
                        ),
                    ),
                ),
            ),
        ],
        &[],
        Step::Untap,
    );
    let ready = combat_frame(&state, true);
    let [land, leader, other, blocker] = [1, 2, 3, 4].map(|n| physical(&ready, n).id());
    let attacking = reply(
        &ready,
        DecisionReply::DeclareBandedAttackers {
            attackers: attack(&ready, &[2, 3], B),
            bands: vec![vec![leader, other]],
        },
    )
    .into_state();
    let destroy = targeted(&attacking, A, 5, TargetRef::Object(land)).into_state();
    let removed = round(destroy).into_state();
    assert_eq!(
        removed.execution().combat().unwrap().attacking_bands()[0],
        [leader, other].into()
    );
    let blocks = combat_frame(&removed, false);
    let blocked = reply(
        &blocks,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker,
            attacker: leader,
        }]),
    )
    .into_state();
    assert_eq!(
        blocked
            .execution()
            .combat()
            .unwrap()
            .blocker(blocker)
            .unwrap()
            .attackers(),
        &[leader, other].into()
    );
    let damage = at_damage(blocked);
    assert_eq!(damage.pending_decision().unwrap().chooser(), B);
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn banding_errand_of_duty_creates_an_ordinary_banding_knight() {
    let state = game(
        vec![A, B],
        vec![named("Errand of Duty", 1, A, ObjectDestination::Hand)],
        &[(ManaType::White, 2)],
        Step::PrecombatMain,
    );
    let cast = cast_untargeted(&state, 1, &[(ManaType::White, 2)]).into_state();
    let made = round(cast);
    let id = tokens(&made, A, 1)[0];
    let view = made
        .state()
        .execution()
        .current_characteristics(id)
        .unwrap()
        .unwrap();
    assert!(view.has_keyword(KeywordAbility::Banding));
    assert!(
        view.current_creature_subtypes
            .contains(&current_catalogue().1.resolve_label("Knight").unwrap())
    );
    assert_eq!(
        view.current_colors,
        [spellwake_engine::characteristics::Color::White].into()
    );
}
