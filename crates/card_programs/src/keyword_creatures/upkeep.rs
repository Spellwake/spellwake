//! Exact fixed-mana echo and cumulative upkeep source labels.
use spellwake_engine::program::{TriggeredAbility, TriggeredAbilityId, UpkeepKeyword};
pub(super) fn trigger(label: &str, id: TriggeredAbilityId) -> Option<TriggeredAbility> {
    let (keyword, price) = label
        .strip_prefix("echo ")
        .map(|price| (UpkeepKeyword::Echo, price))
        .or_else(|| {
            label
                .strip_prefix("cumulative upkeep ")
                .map(|price| (UpkeepKeyword::CumulativeUpkeep, price))
        })?;
    Some(keyword.ability(id, super::fixed_cost(&price.to_ascii_uppercase())?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OracleId, TEACHING_SOURCE_SHA256, keyword_creatures::KEYWORD_GAMEPLAY_V3_PROFILE};
    use spellwake_engine::{
        combat::{CombatDecision, CombatRules},
        effects::{ResolutionPaymentReply, SeedCard},
        game::{DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, RuleFrame},
        mana::{
            ManaAbility, ManaAbilityId, ManaAbilityRef, ManaCost, ManaOutput, ManaPayment,
            ManaSpend, ManaType,
        },
        object_identity::{CardId, ObjectDestination, ObjectZone},
        priority::{PriorityPhase, SeatId},
        program::Program,
        setup::GameSeed,
        stack_window::WindowPhase,
        turn::{Step, TurnStart},
    };

    #[test]
    fn upkeep_accepts_only_complete_fixed_mana_costs() {
        let id = TriggeredAbilityId(7);
        assert_eq!(
            trigger("echo {1}{g}", id),
            Some(UpkeepKeyword::Echo.ability(
                id,
                ManaCost {
                    generic: 1.into(),
                    required: [(ManaType::Green, 1.into())].into()
                }
            ))
        );
        for text in [
            "echo pay 1 life",
            "cumulative upkeep {u/w}",
            "cumulative upkeep {u} or {b}",
            "echo",
            "cumulative upkeep",
        ] {
            assert!(trigger(text, id).is_none(), "{text}");
        }
    }

    #[test]
    #[ignore = "requires pinned Oracle and creature vocabulary inputs"]
    fn goblin_patrol_and_illusionary_wall_resolve_their_actual_upkeep_costs() {
        let (_, vocab, registry) = crate::keyword_creatures::tests::extended_catalogue();
        let (a, b) = (SeatId(0), SeatId(1));
        let mut cards = vec![];
        for (n, name, player, zone) in [
            (
                1,
                "Goblin Patrol",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                2,
                "Illusionary Wall",
                b,
                ObjectDestination::Battlefield { controller: b },
            ),
            (3, "Goblin Patrol", a, ObjectDestination::LibraryBottom),
            (4, "Illusionary Wall", b, ObjectDestination::LibraryBottom),
        ] {
            let row = registry
                .coverage
                .cards
                .iter()
                .find(|r| r.name == name)
                .unwrap();
            cards.push(
                registry
                    .seed(
                        &OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap(),
                        CardId(n),
                        player,
                        zone,
                    )
                    .unwrap(),
            );
        }
        cards.push(SeedCard {
            card: CardId(5),
            owner: a,
            destination: ObjectDestination::Battlefield { controller: a },
            program: Program::land(
                Default::default(),
                vec![ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed([(ManaType::Red, 1.into())].into()),
                }],
            ),
        });
        let names = std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context = GameContext::new(
            KEYWORD_GAMEPLAY_V3_PROFILE.into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "keyword-gameplay-registry14".into(),
            "named-upkeeps".into(),
            vocab.id(),
            names.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(1), CardId(2), CardId(5)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            names,
        );
        let mut state = GameState::initialize_scheduled(
            GameId(78),
            context,
            seed,
            TurnStart {
                number: 1,
                step: Step::Untap,
            },
        )
        .unwrap()
        .into_state();
        let id = |s: &GameState, n| {
            s.execution()
                .window()
                .cards()
                .current_identity(CardId(n))
                .unwrap()
        };
        let apply = |s: &GameState, actor, command| {
            s.advance(GameInput {
                basis: s.basis().clone(),
                actor,
                command,
            })
            .unwrap()
            .into_state()
        };
        let mut payments = 0;
        for _ in 0..100 {
            if let Some(p) = state.pending_decision() {
                let chooser = p.chooser();
                let decision = p.id();
                let choice = match p.frame() {
                    RuleFrame::ResolutionPayment(frame) => {
                        if payments == 0 {
                            assert_eq!((chooser, frame.cost()), (a, &ManaCost::one(ManaType::Red)));
                            state = apply(
                                &state,
                                a,
                                GameCommand::ActivateMana {
                                    source: id(&state, 5),
                                    ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
                                },
                            );
                            let updated = state.pending_decision().unwrap();
                            state = apply(
                                &state,
                                a,
                                GameCommand::Reply {
                                    decision: updated.id(),
                                    choice: DecisionReply::ResolutionPayment(
                                        ResolutionPaymentReply::Pay(ManaPayment {
                                            from_lots: Default::default(),
                                            spend: vec![ManaSpend {
                                                mana_type: ManaType::Red,
                                                amount: 1.into(),
                                            }],
                                        }),
                                    ),
                                },
                            );
                            payments += 1;
                            continue;
                        }
                        assert_eq!((chooser, frame.cost()), (b, &ManaCost::one(ManaType::Blue)));
                        assert_eq!(state.execution().age_counters()[&id(&state, 2)], 1.into());
                        payments += 1;
                        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline)
                    }
                    RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                        DecisionReply::DeclareAttackers(vec![])
                    }
                    other => panic!("unexpected decision {other:?}"),
                };
                state = apply(&state, chooser, GameCommand::Reply { decision, choice });
                if payments == 2 {
                    break;
                }
            } else if let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                state.execution().window().phase()
            {
                state = apply(&state, holder, GameCommand::Pass);
            } else {
                panic!("missing priority");
            }
        }
        assert_eq!(payments, 2);
        let lookup = |n| {
            state
                .execution()
                .window()
                .cards()
                .get(id(&state, n))
                .unwrap()
                .zone()
        };
        assert_eq!(lookup(1), ObjectZone::Battlefield);
        assert_eq!(lookup(2), ObjectZone::Graveyard(b));
    }
}
