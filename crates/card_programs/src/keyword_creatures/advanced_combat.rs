//! Whole-label compilation for the third gameplay-profile combat triggers.
use spellwake_contracts::RulesInteger;
use spellwake_engine::program::{
    CombatTriggerKeyword as Keyword, TriggeredAbility, TriggeredAbilityId,
};

pub(super) fn trigger(label: &str, id: TriggeredAbilityId) -> Option<TriggeredAbility> {
    let keyword = match label {
        "dethrone" => Keyword::Dethrone,
        "melee" => Keyword::Melee,
        "training" => Keyword::Training,
        _ => {
            let (word, operand) = label.split_once(' ')?;
            let amount = RulesInteger::try_from(operand.to_owned()).ok()?;
            if amount < 0.into() {
                return None;
            }
            match word {
                "renown" => Keyword::Renown(amount),
                "afflict" => Keyword::Afflict(amount),
                _ => return None,
            }
        }
    };
    Some(TriggeredAbility::combat_keyword(id, keyword))
}

#[cfg(test)]
mod tests {
    use crate::{OracleId, TEACHING_SOURCE_SHA256, keyword_creatures::KEYWORD_GAMEPLAY_V3_PROFILE};
    use spellwake_engine::{
        combat::{AttackDeclaration, BlockDeclaration, CombatDecision, CombatRules},
        game::{DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, RuleFrame},
        object_identity::{CardId, ObjectDestination},
        priority::{PriorityPhase, SeatId},
        setup::GameSeed,
        stack_window::WindowPhase,
        turn::{Step, TurnStart},
    };
    #[test]
    #[ignore = "requires pinned Oracle and creature vocabulary inputs"]
    fn five_named_combat_keywords_resolve_in_one_three_player_combat() {
        let (_, vocab, registry) = crate::keyword_creatures::tests::extended_catalogue();
        let (a, b, c) = (SeatId(0), SeatId(1), SeatId(2));
        let mut cards = Vec::new();
        for (n, name, seat, destination) in [
            (
                1,
                "Khenra Eternal",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                2,
                "Enraged Revolutionary",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                3,
                "Gryff Rider",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                4,
                "Wings of the Guard",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                5,
                "Rhox Maulers",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                6,
                "Khenra Eternal",
                b,
                ObjectDestination::Battlefield { controller: b },
            ),
            (7, "Rhox Maulers", a, ObjectDestination::LibraryBottom),
            (8, "Rhox Maulers", b, ObjectDestination::LibraryBottom),
            (9, "Rhox Maulers", c, ObjectDestination::LibraryBottom),
        ] {
            let row = registry
                .coverage
                .cards
                .iter()
                .find(|r| r.name == name)
                .unwrap();
            let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
            cards.push(registry.seed(&id, CardId(n), seat, destination).unwrap());
        }
        let names = std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context = GameContext::new(
            KEYWORD_GAMEPLAY_V3_PROFILE.into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "keyword-gameplay-registry14".into(),
            "five-combat-keywords".into(),
            vocab.id(),
            names.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![a, b, c],
            a,
            cards,
            (1..=6).map(CardId).collect(),
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            names,
        );
        let mut state = GameState::initialize_scheduled(
            GameId(77),
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
        let pass = |s: &GameState| {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                s.execution().window().phase()
            else {
                panic!("priority")
            };
            apply(s, holder, GameCommand::Pass)
        };
        while state.pending_decision().is_none() {
            state = pass(&state);
        }
        let reply = |s: &GameState, choice| {
            let p = s.pending_decision().unwrap();
            apply(
                s,
                p.chooser(),
                GameCommand::Reply {
                    decision: p.id(),
                    choice,
                },
            )
        };
        state = reply(
            &state,
            DecisionReply::DeclareAttackers(
                (1..=5)
                    .map(|n| AttackDeclaration {
                        attacker: id(&state, n),
                        defender: if n == 4 { c } else { b },
                        defending_planeswalker: None,
                    })
                    .collect(),
            ),
        );
        let RuleFrame::Triggers(order) = state.pending_decision().unwrap().frame() else {
            panic!("three concurrent attack triggers")
        };
        assert_eq!(order.instances().len(), 3);
        state = reply(
            &state,
            DecisionReply::OrderTriggers(order.instances().to_vec()),
        );
        while !state.execution().window().entries().is_empty() {
            state = pass(&state);
        }
        for n in [2, 3] {
            assert_eq!(
                state
                    .execution()
                    .power_toughness_counters(id(&state, n))
                    .unwrap()
                    .plus_one,
                1.into()
            );
        }
        assert_eq!(
            state
                .execution()
                .creature(id(&state, 4))
                .unwrap()
                .unwrap()
                .power,
            3.into()
        );
        while state.pending_decision().is_none() {
            state = pass(&state);
        }
        assert!(matches!(
            state.pending_decision().unwrap().frame(),
            RuleFrame::Combat(CombatDecision::Blockers(_))
        ));
        state = reply(
            &state,
            DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                blocker: id(&state, 6),
                attacker: id(&state, 1),
            }]),
        );
        assert_eq!(state.execution().window().entries().len(), 1);
        while !state.execution().window().entries().is_empty() {
            state = pass(&state);
        }
        assert_eq!(state.execution().life_total(b), Some(&19.into()));
        while state.execution().window().entries().is_empty() {
            state = pass(&state);
        }
        assert_eq!(state.execution().life_total(b), Some(&9.into()));
        assert_eq!(state.execution().life_total(c), Some(&17.into()));
        assert!(state.execution().renowned_objects().is_empty());
        while !state.execution().window().entries().is_empty() {
            state = pass(&state);
        }
        assert!(
            state
                .execution()
                .renowned_objects()
                .contains(&id(&state, 5))
        );
        assert_eq!(
            state
                .execution()
                .power_toughness_counters(id(&state, 5))
                .unwrap()
                .plus_one,
            2.into()
        );
    }
}
