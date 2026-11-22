use super::{
    cmm_gameplay::{creature, named},
    *,
};

fn battlefield() -> ObjectDestination {
    ObjectDestination::Battlefield { controller: A }
}

fn declaration(cards: Vec<SeedCard>, attacker: u64) -> (GameState, ObjectId) {
    let initial = game(vec![A, B], cards, &[], Step::Untap);
    let choose = combat_frame(&initial, true);
    let attacker = physical(&choose, attacker).id();
    let attacked = reply(
        &choose,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    (combat_frame(&attacked, false), attacker)
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_combat_compares_blockers_with_the_live_power_of_the_restriction_source() {
    for (name, source_power, attacker) in [
        ("Champion of Lambholt", 1, 2),
        ("Wandering Wolf", 2, 1),
        ("Howlgeist", 4, 1),
    ] {
        let (state, attacker) = declaration(
            vec![
                named(name, 1, A, battlefield()),
                creature(2, A, 7, 7, &[]),
                creature(3, B, source_power - 1, 5, &[]),
                creature(4, B, source_power, 5, &[]),
            ],
            attacker,
        );
        let RuleFrame::Combat(CombatDecision::Blockers(frame)) =
            state.pending_decision().unwrap().frame()
        else {
            panic!("expected a blocking declaration");
        };
        assert_eq!(
            frame.eligible_blockers(),
            &[physical(&state, 4).id()],
            "{name}"
        );
        assert_eq!(frame.legal_attackers(physical(&state, 4).id()), &[attacker]);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_combat_distinguishes_flying_from_reach_in_both_directions() {
    let flying = [KeywordAbility::Flying];
    let reach = [KeywordAbility::Reach];
    let (state, _) = declaration(
        vec![
            named("Bower Passage", 1, A, battlefield()),
            creature(2, A, 4, 3, &flying),
            creature(3, B, 2, 4, &flying),
            creature(4, B, 2, 4, &reach),
        ],
        2,
    );
    let RuleFrame::Combat(CombatDecision::Blockers(frame)) =
        state.pending_decision().unwrap().frame()
    else {
        panic!("expected a blocking declaration");
    };
    assert_eq!(frame.eligible_blockers(), &[physical(&state, 4).id()]);

    for flyer in [false, true] {
        let (state, _) = declaration(
            vec![
                creature(1, A, 1, 1, if flyer { &flying } else { &[] }),
                named(
                    "Gloomwidow",
                    2,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                ),
                named(
                    "Scrapskin Drake",
                    3,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                ),
                creature(4, B, 1, 1, &reach),
            ],
            1,
        );
        let RuleFrame::Combat(CombatDecision::Blockers(frame)) =
            state.pending_decision().unwrap().frame()
        else {
            panic!("expected a blocking declaration");
        };
        let expected: Vec<_> = (if flyer { 2..=4 } else { 4..=4 })
            .map(|number| physical(&state, number).id())
            .collect();
        assert_eq!(
            frame.eligible_blockers(),
            expected,
            "flying attacker: {flyer}"
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_combat_tapped_and_only_creature_modifiers_follow_live_state() {
    let state = game(
        vec![A, B],
        vec![
            named("Builder's Blessing", 1, A, battlefield()),
            named("Homicidal Seclusion", 2, A, battlefield()),
            creature(3, A, 2, 2, &[]),
            creature(4, B, 1, 1, &[]),
        ],
        &[],
        Step::Untap,
    );
    let host = physical(&state, 3).id();
    let initial = state.execution().creature(host).unwrap().unwrap();
    assert_eq!(initial.power, 5.into());
    assert_eq!(initial.toughness, 5.into());
    let choose = combat_frame(&state, true);
    let attacked = reply(
        &choose,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: host,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let updated = attacked.execution().creature(host).unwrap().unwrap();
    assert_eq!(updated.power, 5.into());
    assert_eq!(updated.toughness, 3.into());
    assert!(
        attacked
            .execution()
            .current_characteristics(host)
            .unwrap()
            .unwrap()
            .has_keyword(KeywordAbility::Lifelink)
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_combat_exquisite_blood_uses_the_exact_opponent_life_loss() {
    let state = game(
        vec![A, B],
        vec![
            named("Exquisite Blood", 1, A, battlefield()),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SinglePlayer,
                    Instruction::AdjustTargetLife {
                        adjustment: spellwake_engine::life::LifeAdjustment::Lose(3.into()),
                    },
                ),
            ),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SinglePlayer,
                    Instruction::AdjustTargetLife {
                        adjustment: spellwake_engine::life::LifeAdjustment::Lose(2.into()),
                    },
                ),
            ),
        ],
        &[],
        Step::PrecombatMain,
    );
    let mut state = state;
    for id in [2, 3] {
        state = round(targeted(&state, A, id, TargetRef::Player(B)).into_state()).into_state();
        assert_eq!(state.execution().window().entries().len(), 1);
        state = round(state).into_state();
    }
    assert_eq!(state.execution().life_total(B), Some(&15.into()));
    assert_eq!(state.execution().life_total(A), Some(&25.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_combat_misthollow_griffin_casts_from_its_owners_face_up_exile() {
    let price = [(ManaType::Colourless, 2), (ManaType::Blue, 2)];
    let initial = game(
        vec![A, B],
        vec![
            named("Misthollow Griffin", 1, A, ObjectDestination::Exile),
            named("Misthollow Griffin", 2, B, ObjectDestination::Exile),
        ],
        &price,
        Step::PrecombatMain,
    );
    assert!(
        initial
            .execution()
            .may_play_from_exile(physical(&initial, 1).id(), A)
    );
    assert!(
        !initial
            .execution()
            .may_play_from_exile(physical(&initial, 2).id(), A)
    );
    let state = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    assert_eq!(physical(&state, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&state, 2).zone(), ObjectZone::Exile);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_combat_revenge_requires_every_legal_blocker_to_block_the_target() {
    let price = [(ManaType::Colourless, 4), (ManaType::Green, 2)];
    let initial = game(
        vec![A, B],
        vec![
            named("Revenge of the Hunted", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 2, &[KeywordAbility::Haste]),
            creature(3, A, 1, 1, &[KeywordAbility::Haste]),
            creature(4, B, 1, 1, &[]),
            creature(5, B, 1, 1, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let enriched = physical(&initial, 2).id();
    let other = physical(&initial, 3).id();
    let cast = command(
        &initial,
        A,
        GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: Some(TargetRef::Object(enriched)),
            payment: CastingPayment {
                mana: Some(spend(&price)),
                ..Default::default()
            },
        },
    );
    let resolved = round(cast.into_state()).into_state();
    assert_eq!(
        resolved
            .execution()
            .creature(enriched)
            .unwrap()
            .unwrap()
            .power,
        8.into()
    );
    assert!(
        resolved
            .execution()
            .current_characteristics(enriched)
            .unwrap()
            .unwrap()
            .has_keyword(KeywordAbility::Trample)
    );

    let choose = combat_frame(&resolved, true);
    let attacked = reply(
        &choose,
        DecisionReply::DeclareAttackers(
            [enriched, other]
                .into_iter()
                .map(|attacker| AttackDeclaration {
                    attacker,
                    defender: B,
                    defending_planeswalker: None,
                })
                .collect(),
        ),
    );
    let blocking = combat_frame(attacked.state(), false);
    let blockers = [physical(&blocking, 4).id(), physical(&blocking, 5).id()];
    let choice = blocking.pending_decision().unwrap();
    assert!(
        blocking
            .advance(GameInput {
                basis: blocking.basis().clone(),
                actor: B,
                command: GameCommand::Reply {
                    decision: choice.id(),
                    choice: DecisionReply::DeclareBlockers(vec![
                        BlockDeclaration {
                            blocker: blockers[0],
                            attacker: enriched
                        },
                        BlockDeclaration {
                            blocker: blockers[1],
                            attacker: other
                        },
                    ]),
                },
            })
            .is_err()
    );
    reply(
        &blocking,
        DecisionReply::DeclareBlockers(
            blockers
                .into_iter()
                .map(|blocker| BlockDeclaration {
                    blocker,
                    attacker: enriched,
                })
                .collect(),
        ),
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_combat_gloom_surgeon_exiles_even_when_damage_cannot_be_prevented() {
    for unpreventable in [false, true] {
        let attacker = if unpreventable {
            named("Malignus", 1, A, battlefield())
        } else {
            creature(1, A, 3, 3, &[])
        };
        let cards = vec![
            attacker,
            named(
                "Gloom Surgeon",
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            seed(
                3,
                B,
                ObjectDestination::LibraryTop,
                Program::artifact(None, vec![]),
            ),
        ];
        let (blocking, attacker) = declaration(cards, 1);
        let surgeon = physical(&blocking, 2).id();
        let blocked = reply(
            &blocking,
            DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                blocker: surgeon,
                attacker,
            }]),
        );
        let result = round(blocked.into_state());
        assert_eq!(physical(result.state(), 3).zone(), ObjectZone::Exile);
        assert_eq!(
            physical(result.state(), 2).zone(),
            if unpreventable {
                ObjectZone::Graveyard(B)
            } else {
                ObjectZone::Battlefield
            }
        );
        assert_eq!(
            result
                .damage_journal()
                .applied
                .iter()
                .any(|dealt| dealt.target == DamageRecipient::Creature(surgeon)),
            unpreventable
        );
    }
}
