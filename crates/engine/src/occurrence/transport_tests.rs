//! Exact envelope mapping over a real mana transition and its current append.
use super::*;
use crate::object_identity::CardId;
use crate::{
    combat::CombatRules,
    effects::{EffectWindow, ManaActivationResult, SeedCard},
    mana::{ManaAbility, ManaOutput},
    object_identity::ObjectDestination,
    program::Program,
    setup::GameSeed,
};

#[test]
fn mapping_the_actual_mana_result_preserves_exact_state_and_nonempty_append() {
    let a = SeatId(0);
    let state = EffectWindow::new(GameSeed::with_uniform_life(
        vec![a, SeatId(1)],
        a,
        vec![SeedCard {
            card: CardId(1),
            owner: a,
            destination: ObjectDestination::Battlefield { controller: a },
            program: Program::land(
                std::collections::BTreeSet::new(),
                vec![ManaAbility {
                    id: ManaAbilityId(1),
                    output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
                }],
            ),
        }],
        vec![CardId(1)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        crate::test_support::creature_types::synthetic_vocabulary(),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    ))
    .unwrap();
    let state = state.settle_checks(state.revision()).unwrap();
    let source = state.window().cards().current_identity(CardId(1)).unwrap();
    let (state, production, append) = state
        .activate_mana(
            state.revision(),
            a,
            source,
            crate::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        )
        .unwrap()
        .into_parts();
    assert!(!append.records.is_empty());
    let before_state = state.clone();
    let before_production = production.clone();
    let before_append = append.clone();
    let mapped = RulesTransition::new(state, production, append)
        .map_result(|production| ManaActivationResult::Complete { production });
    let (state, result, append) = mapped.into_parts();
    assert_eq!(state, before_state);
    assert_eq!(
        result,
        ManaActivationResult::Complete {
            production: before_production
        }
    );
    assert_eq!(append, before_append);
    assert_eq!(state.mana(a, ManaType::Green), 1.into());
    assert_eq!(
        state.window().cards().get(source).unwrap().tapped(),
        Some(true)
    );
}
