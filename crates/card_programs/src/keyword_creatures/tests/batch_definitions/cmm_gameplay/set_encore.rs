//! Printed and granted encore with multiplayer combat and end-step choices.
use super::set_slivers::sheet;
use super::*;
use spellwake_engine::activated::SourceCost;

pub(super) fn new_tokens(candidate: &NativeCandidate) -> Vec<ObjectId> {
    events(candidate)
        .flat_map(|event| &event.occurrences)
        .filter_map(|occurrence| match &occurrence.value {
            RulesOccurrence::TokenCreated { creator, after } => {
                assert_eq!(*creator, A);
                assert_eq!(after.object.controller(), Some(A));
                Some(after.object.id())
            }
            _ => None,
        })
        .collect()
}

fn encore_resolves(activation: GameState) -> (GameState, [ObjectId; 2]) {
    let ordering = round(activation).into_state();
    let RuleFrame::TimestampOrder(frame) = ordering.pending_decision().unwrap().frame() else {
        panic!("two simultaneous encore tokens");
    };
    let made = reply(
        &ordering,
        DecisionReply::TimestampOrder(frame.members().iter().map(|member| member.member).collect()),
    );
    let tokens: [ObjectId; 2] = new_tokens(&made)
        .try_into()
        .expect("one token per opponent");
    for token in tokens {
        let view = made
            .state()
            .execution()
            .current_characteristics(token)
            .unwrap()
            .unwrap();
        assert!(view.has_keyword(Haste));
    }
    (made.into_state(), tokens)
}

fn encore_attack(main: &GameState, tokens: [ObjectId; 2]) -> GameState {
    let decision = combat_frame(main, true);
    illegal(
        &decision,
        A,
        combat_declaration(&decision, DecisionReply::DeclareAttackers(vec![])),
    );
    let mut valid = [[B, C], [C, B]].into_iter().filter_map(|players| {
        let declarations = tokens
            .into_iter()
            .zip(players)
            .map(|(attacker, defender)| AttackDeclaration {
                attacker,
                defender,
                defending_planeswalker: None,
            })
            .collect();
        decision
            .advance(GameInput {
                actor: A,
                basis: decision.basis().clone(),
                command: combat_declaration(
                    &decision,
                    DecisionReply::DeclareAttackers(declarations),
                ),
            })
            .ok()
    });
    let attacked = valid.next().expect("assigned opponents can be attacked");
    assert!(valid.next().is_none(), "opponent requirements are distinct");
    attacked.into_state()
}

fn encore_end(attacked: GameState) -> GameState {
    let end = exile_hand::until(attacked, |state| {
        state.turn().unwrap().position().step == Step::End
            && (state.pending_decision().is_some()
                || !state.execution().window().entries().is_empty())
    });
    round(end).into_state()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sliver_gravemother_disables_its_legend_rule_and_gives_a_sliver_encore_at_mana_value() {
    use ManaType::*;
    let price = [(White, 1), (Blue, 1), (Black, 1), (Red, 1), (Green, 1)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Sliver Gravemother"),
            named("Sliver Gravemother", 2, A, ObjectDestination::Hand),
            named("Diffusion Sliver", 3, A, ObjectDestination::Graveyard),
            named("Elvish Mystic", 4, A, ObjectDestination::Graveyard),
        ],
        &[
            (White, 2),
            (Blue, 2),
            (Black, 2),
            (Red, 2),
            (Green, 2),
            (Colourless, 2),
        ],
        Step::PrecombatMain,
    );
    let first = cast_named(&initial, &price);
    let both = round(cast_untargeted(&first, 2, &price).into_state()).into_state();
    for id in [1, 2] {
        assert_eq!(physical(&both, id).zone(), ObjectZone::Battlefield);
    }
    let graveyard_sliver = physical(&both, 3).id();
    let grants = sheet(&both, 3);
    assert_eq!(grants.granted_activated.len(), 2);
    assert!(sheet(&both, 4).granted_activated.is_empty());
    let (&effect, _) = grants.granted_activated.first_key_value().unwrap();
    let activate = |amount| GameCommand::ActivateGranted {
        source: graveyard_sliver,
        effect,
        target: None,
        payment: Some(spend(&[(Colourless, amount)])),
    };
    illegal(&both, A, activate(1));
    let paid = command(&both, A, activate(2)).into_state();
    assert_eq!(physical(&paid, 3).zone(), ObjectZone::Exile);
    assert_eq!(paid.execution().mana(A, Colourless), 0.into());
    let (formed, copies) = encore_resolves(paid);
    for copy in copies {
        let view = formed
            .execution()
            .current_characteristics(copy)
            .unwrap()
            .unwrap();
        assert_eq!(
            view.current_names.iter().next().unwrap().as_str(),
            "Diffusion Sliver"
        );
    }
    let attacked = encore_attack(&formed, copies);
    let ended = encore_end(attacked);
    assert!(
        copies
            .into_iter()
            .all(|id| ended.execution().window().cards().get(id).is_none())
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_gravemother_and_impulsive_pilferer_pay_printed_encore_and_sacrifice_their_copies() {
    use ManaType::*;
    for (name, cost, used_first) in [
        ("Sliver Gravemother", vec![(Colourless, 5)], false),
        ("Impulsive Pilferer", vec![(Colourless, 3), (Red, 1)], true),
    ] {
        let original = named(
            name,
            1,
            A,
            if used_first {
                ObjectDestination::Hand
            } else {
                ObjectDestination::Graveyard
            },
        );
        let ability = original
            .program
            .activated
            .iter()
            .find(|ability| {
                ability.source_cost
                    == SourceCost::Exile {
                        tap_source: false,
                        from_graveyard: true,
                    }
            })
            .unwrap()
            .id;
        let mut budget = cost.clone();
        if used_first {
            budget[1].1 += 1;
        }
        let initial = game(
            vec![A, B, C],
            vec![original, remove_spell(2, A)],
            &budget,
            Step::PrecombatMain,
        );
        let initial = if used_first {
            let entered = cast_named(&initial, &[(Red, 1)]);
            let killed = round(
                targeted(
                    &entered,
                    A,
                    2,
                    TargetRef::Object(physical(&entered, 1).id()),
                )
                .into_state(),
            )
            .into_state();
            let treasure = round(killed);
            assert_eq!(new_tokens(&treasure).len(), 1);
            treasure.into_state()
        } else {
            initial
        };
        let announce = GameCommand::ActivateUntargetedAbility {
            source: physical(&initial, 1).id(),
            ability,
            payment: Some(spend(&cost)),
        };
        let outside_main = exile_hand::until(initial.clone(), |state| {
            state.turn().unwrap().position().step == Step::BeginningCombat
        });
        assert!(matches!(
            outside_main.advance(GameInput {
                actor: A,
                basis: outside_main.basis().clone(),
                command: announce.clone(),
            }),
            Err(GameError::Effect(EffectError::Timing(
                TimingError::MainPhaseRequired
            )))
        ));
        let activated = command(&initial, A, announce).into_state();
        assert_eq!(physical(&activated, 1).zone(), ObjectZone::Exile);
        let (formed, copies) = encore_resolves(activated);
        for copy in copies {
            let view = formed
                .execution()
                .current_characteristics(copy)
                .unwrap()
                .unwrap();
            assert_eq!(view.current_names.iter().next().unwrap().as_str(), name);
        }
        let attacked = encore_attack(&formed, copies);
        let mut ended = encore_end(attacked);
        assert!(
            copies
                .into_iter()
                .all(|id| ended.execution().window().cards().get(id).is_none())
        );
        if used_first {
            if let Some(RuleFrame::Triggers(frame)) =
                ended.pending_decision().map(|decision| decision.frame())
            {
                ended = reply(
                    &ended,
                    DecisionReply::OrderTriggers(frame.instances().to_vec()),
                )
                .into_state();
            }
            assert_eq!(ended.execution().window().entries().len(), 2);
            for _ in 0..2 {
                let result = round(ended);
                assert_eq!(new_tokens(&result).len(), 1);
                ended = result.into_state();
            }
        }
        assert!(ended.execution().window().entries().is_empty());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_phyrexian_triniform_dies_into_three_golems_and_its_encore_copies_each_die_into_three_more() {
    use ManaType::Colourless;
    let body = hand("Phyrexian Triniform");
    let encore = body
        .program
        .activated
        .iter()
        .find(|ability| {
            ability.source_cost
                == SourceCost::Exile {
                    tap_source: false,
                    from_graveyard: true,
                }
        })
        .unwrap()
        .id;
    let start = game(
        vec![A, B, C],
        vec![body, remove_spell(2, A)],
        &[(Colourless, 21)],
        Step::PrecombatMain,
    );
    let field = round(cast_untargeted(&start, 1, &[(Colourless, 9)]).into_state()).into_state();
    let original = physical(&field, 1).id();
    let died = round(targeted(&field, A, 2, TargetRef::Object(original)).into_state()).into_state();
    let offspring = |stack: GameState| {
        let order = round(stack).into_state();
        let RuleFrame::TimestampOrder(frame) = order.pending_decision().unwrap().frame() else {
            panic!("order three created Golems")
        };
        let made = reply(
            &order,
            DecisionReply::TimestampOrder(
                frame.members().iter().map(|member| member.member).collect(),
            ),
        );
        let ids = new_tokens(&made);
        assert_eq!(ids.len(), 3);
        let vocab = current_catalogue().1;
        for id in ids {
            let current = made
                .state()
                .execution()
                .current_characteristics(id)
                .unwrap()
                .unwrap();
            assert_eq!(
                current.current_types,
                [CardType::Artifact, CardType::Creature].into()
            );
            assert!(current.current_colors.is_empty());
            assert_eq!(
                current.current_creature_subtypes,
                [
                    vocab.resolve_label("Phyrexian").unwrap(),
                    vocab.resolve_label("Golem").unwrap()
                ]
                .into()
            );
            assert_eq!(
                current.power_toughness.unwrap().derived,
                PowerToughness {
                    power: 3.into(),
                    toughness: 3.into()
                }
            );
        }
        made.into_state()
    };
    let first = offspring(died);
    let activate = |s: &GameState| GameCommand::ActivateUntargetedAbility {
        source: physical(s, 1).id(),
        ability: encore,
        payment: Some(spend(&[(Colourless, 12)])),
    };
    let combat = exile_hand::until(first.clone(), |s| {
        s.turn().unwrap().position().step == Step::BeginningCombat
    });
    illegal(&combat, A, activate(&combat));
    let paid = command(&first, A, activate(&first)).into_state();
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Exile);
    let (raised, copies) = encore_resolves(paid);
    for id in copies {
        let copy = raised
            .execution()
            .current_characteristics(id)
            .unwrap()
            .unwrap();
        assert_eq!(
            copy.current_names.iter().next().unwrap().as_str(),
            "Phyrexian Triniform"
        );
        assert_eq!(
            copy.power_toughness.unwrap().derived,
            PowerToughness {
                power: 9.into(),
                toughness: 9.into()
            }
        );
    }
    let mut end = encore_end(encore_attack(&raised, copies));
    if let Some(RuleFrame::Triggers(order)) = end.pending_decision().map(|p| p.frame()) {
        end = reply(
            &end,
            DecisionReply::OrderTriggers(order.instances().to_vec()),
        )
        .into_state();
    }
    assert_eq!(end.execution().window().entries().len(), 2);
    let done = offspring(offspring(end));
    assert!(done.execution().window().entries().is_empty());
    assert_eq!(
        done.execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
            .count(),
        9
    );
}
