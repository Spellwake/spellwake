//! Complete printed keyword labels with a dedicated triggered ability instance.
use spellwake_contracts::RulesInteger;
use spellwake_engine::program::{CombatTriggerKeyword, TriggeredAbility, TriggeredAbilityId};

pub(super) fn combat_trigger(label: &str, id: TriggeredAbilityId) -> Option<TriggeredAbility> {
    Some(TriggeredAbility::combat_keyword(id, keyword_label(label)?))
}

fn keyword_label(label: &str) -> Option<CombatTriggerKeyword> {
    let keyword = match label {
        "prowess" => CombatTriggerKeyword::Prowess,
        "battle cry" => CombatTriggerKeyword::BattleCry,
        "flanking" => CombatTriggerKeyword::Flanking,
        _ => {
            let (name, value) = label.split_once(' ')?;
            let amount = RulesInteger::try_from(value.to_owned()).ok()?;
            if amount < 0.into() {
                return None;
            }
            match name {
                "bushido" => CombatTriggerKeyword::Bushido(amount),
                "poisonous" => CombatTriggerKeyword::Poisonous(amount),
                "frenzy" => CombatTriggerKeyword::Frenzy(amount),
                "rampage" => CombatTriggerKeyword::Rampage(amount),
                _ => return None,
            }
        }
    };
    Some(keyword)
}

/// Whole-text Sliver grants affect Slivers controlled by any player, including the source.
pub(super) fn sliver_grant(
    label: &str,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Option<spellwake_engine::characteristics::StaticContinuousAbility> {
    use spellwake_engine::{
        characteristics::{
            AbilityChange, PermanentSelector, StaticAbilityId, StaticContinuousAbility, StaticPart,
            StaticParts,
        },
        program::CardType,
    };
    let keyword = keyword_label(
        label
            .strip_prefix("all sliver creatures have ")?
            .strip_suffix('.')?,
    )?;
    Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(0),
        selector: PermanentSelector {
            required_types: [CardType::Creature].into(),
            creature_subtype: Some(vocabulary.resolve_label("Sliver")?),
            ..Default::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange {
            granted_combat_keywords: vec![keyword],
            ..Default::default()
        })]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Registry, keyword_creatures::vocabulary};
    use spellwake_engine::program::CombatTriggerKeyword::*;
    use std::path::PathBuf;

    #[test]
    fn each_trigger_label_consumes_its_full_canonical_operand() {
        for (word, k) in [
            ("prowess", Prowess),
            ("battle cry", BattleCry),
            ("bushido 2", Bushido(2.into())),
            ("flanking", Flanking),
            ("frenzy 1", Frenzy(1.into())),
            ("poisonous 2", Poisonous(2.into())),
            ("rampage 13", Rampage(13.into())),
        ] {
            assert_eq!(
                combat_trigger(word, TriggeredAbilityId(9)),
                Some(TriggeredAbility::combat_keyword(TriggeredAbilityId(9), k))
            );
        }
        for word in [
            "prowess twice",
            "flanking 1",
            "rampage",
            "bushido 01",
            "bushido 2.",
            "frenzy -1",
            "frenzy 1 and haste",
            "suspend 2",
        ] {
            assert_eq!(combat_trigger(word, TriggeredAbilityId(0)), None);
        }
    }
    #[test]
    #[ignore = "requires the saved catalogue and creature vocabulary"]
    fn pinned_sources_bind_to_distinct_printed_or_granted_abilities() {
        let input = |key| PathBuf::from(std::env::var_os(key).unwrap());
        let vocab =
            vocabulary::load_vocabulary(&input("SPELLWAKE_CREATURE_VOCABULARY_INPUT")).unwrap();
        let inventory = spellwake_content::snapshot::load(
            &input("SPELLWAKE_CATALOGUE_METADATA"),
            &input("SPELLWAKE_CATALOGUE_INPUT"),
        )
        .unwrap();
        let registry = Registry::from_keyword_gameplay_inventory(&inventory, &vocab).unwrap();
        for (name, k) in [
            ("Bloodfire Expert", Prowess),
            ("Accorder Paladin", BattleCry),
            ("Devoted Retainer", Bushido(1.into())),
            ("Benalish Cavalry", Flanking),
            ("Aerathi Berserker", Rampage(3.into())),
        ] {
            let binding = registry
                .coverage
                .cards
                .iter()
                .find(|card| card.name == name)
                .unwrap();
            let id = crate::OracleId::try_from(binding.oracle_id.clone().unwrap()).unwrap();
            let body = &registry.bindings[&id].instruction;
            assert_eq!(
                body.triggered,
                vec![TriggeredAbility::combat_keyword(TriggeredAbilityId(0), k)],
                "{name}"
            );
        }
        for (name, k) in [
            ("Frenzy Sliver", Frenzy(1.into())),
            ("Sidewinder Sliver", Flanking),
            ("Virulent Sliver", Poisonous(1.into())),
        ] {
            let binding = registry
                .coverage
                .cards
                .iter()
                .find(|card| card.name == name)
                .unwrap();
            let id = crate::OracleId::try_from(binding.oracle_id.clone().unwrap()).unwrap();
            let body = &registry.bindings[&id].instruction;
            assert!(body.triggered.is_empty(), "{name}");
            let Some(spellwake_engine::characteristics::StaticContinuousAbility::Battlefield {
                selector,
                parts,
                ..
            }) = &body.continuous
            else {
                panic!("expected {name} to grant an ability")
            };
            assert_eq!(selector.creature_subtype, vocab.resolve_label("Sliver"));
            let [spellwake_engine::characteristics::StaticPart::Abilities(change)] =
                parts.0.as_slice()
            else {
                panic!("expected keyword grant")
            };
            assert_eq!(change.granted_combat_keywords, vec![k], "{name}");
        }
        use spellwake_engine::characteristics::KeywordAbility::{
            Deathtouch, Flying, Infect, Wither,
        };
        for (name, keywords, toxic) in [
            ("Sickle Ripper", vec![Wither], None),
            ("Plague Stinger", vec![Flying, Infect], None),
            ("Branchblight Stalker", vec![], Some(2.into())),
            ("Bilious Skulldweller", vec![Deathtouch], Some(1.into())),
        ] {
            let row = registry
                .coverage
                .cards
                .iter()
                .find(|card| card.name == name)
                .unwrap();
            let id = crate::OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
            let program = &registry.bindings[&id].instruction;
            assert_eq!(
                program.printed_keywords,
                keywords.into_iter().collect(),
                "{name}"
            );
            assert_eq!(program.printed_toxic, toxic, "{name}");
        }
        named_combat(&registry, vocab);
    }

    fn named_combat(
        registry: &Registry,
        vocabulary: std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    ) {
        use spellwake_engine::{
            combat::{AttackDeclaration, BlockDeclaration, CombatDecision, CombatRules},
            game::{
                DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, RuleFrame,
            },
            object_identity::{CardId, ObjectDestination},
            priority::{PriorityPhase, SeatId},
            setup::GameSeed,
            stack_window::WindowPhase,
            turn::{Step, TurnStart},
        };
        let (a, b) = (SeatId(0), SeatId(1));
        let mut cards = Vec::new();
        for (n, name, seat, destination) in [
            (
                1,
                "Accorder Paladin",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                2,
                "Frenzy Sliver",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                3,
                "Sidewinder Sliver",
                b,
                ObjectDestination::Battlefield { controller: b },
            ),
            (4, "Bloodfire Expert", a, ObjectDestination::LibraryBottom),
            (5, "Bloodfire Expert", b, ObjectDestination::LibraryBottom),
            (
                6,
                "Virulent Sliver",
                b,
                ObjectDestination::Battlefield { controller: b },
            ),
        ] {
            let row = registry
                .coverage
                .cards
                .iter()
                .find(|card| card.name == name)
                .unwrap();
            let id = crate::OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
            cards.push(registry.seed(&id, CardId(n), seat, destination).unwrap());
        }
        let names = std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context = GameContext::new(
            crate::KEYWORD_GAMEPLAY_PROFILE.into(),
            vocabulary.rules_revision().to_owned(),
            crate::TEACHING_SOURCE_SHA256.into(),
            "keyword-gameplay-registry13".into(),
            "battle-cry-and-frenzy".into(),
            vocabulary.id(),
            names.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(1), CardId(2), CardId(3), CardId(6)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            names,
        );
        let mut state = GameState::initialize_scheduled(
            GameId(14),
            context,
            seed,
            TurnStart {
                number: 1,
                step: Step::Untap,
            },
        )
        .unwrap()
        .into_state();
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
                panic!("expected priority")
            };
            apply(s, holder, GameCommand::Pass)
        };
        for _ in 0..32 {
            if state.pending_decision().is_some() {
                break;
            }
            state = pass(&state);
        }
        let id = |n| {
            state
                .execution()
                .window()
                .cards()
                .current_identity(CardId(n))
                .unwrap()
        };
        let (paladin, sliver, sidewinder) = (id(1), id(2), id(3));
        let pending = state.pending_decision().unwrap();
        assert!(matches!(
            pending.frame(),
            RuleFrame::Combat(CombatDecision::Attackers(_))
        ));
        state = apply(
            &state,
            a,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::DeclareAttackers(vec![
                    AttackDeclaration {
                        attacker: paladin,
                        defender: b,
                        defending_planeswalker: None,
                    },
                    AttackDeclaration {
                        attacker: sliver,
                        defender: b,
                        defending_planeswalker: None,
                    },
                ]),
            },
        );
        assert_eq!(state.execution().window().entries().len(), 1);
        state = pass(&pass(&state));
        assert_eq!(
            state.execution().creature(sliver).unwrap().unwrap().power,
            2.into()
        );
        state = pass(&pass(&state));
        let pending = state.pending_decision().unwrap();
        state = apply(
            &state,
            b,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                    blocker: sidewinder,
                    attacker: paladin,
                }]),
            },
        );
        assert_eq!(state.execution().window().entries().len(), 1);
        state = pass(&pass(&state));
        assert_eq!(
            state.execution().creature(sliver).unwrap().unwrap().power,
            3.into()
        );
        assert_eq!(
            state.execution().creature(paladin).unwrap().unwrap().power,
            3.into()
        );
        state = pass(&pass(&state));
        assert_eq!(state.execution().life_total(b), Some(&17.into()));
        assert_eq!(state.execution().poison_counters(b), 0.into());
        assert_eq!(state.execution().window().entries().len(), 1);
        state = pass(&pass(&state));
        assert_eq!(state.execution().poison_counters(b), 1.into());
    }
}
