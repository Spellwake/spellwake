//! Consumed runtime metadata and exact legacy numeric/error boundaries.
use super::*;
use crate::ordinary_game::{
    fixture::FixtureInitialization,
    mandatory::MandatoryCase,
    optional::{OptionalCase, OptionalSource},
};
use spellwake_engine::game::GameState;
use std::{path::PathBuf, sync::Arc};

fn row<K>(mana_type: K, amount: &str) -> PaymentItem<K> {
    PaymentItem {
        mana_type,
        amount: amount.into(),
    }
}
#[test]
fn runtime_maps_old_keep_metadata_and_plan_indices_without_serde_runtime() {
    let input = inputs().unwrap();
    for case in input.cases {
        let case = case.map_id(CaseId::Mandatory);
        let expected = case.clone();
        let runtime = FinitePolicy::normalize_recipe(case);
        match (&expected.initialization, &runtime.initialization) {
            (
                Initialization::Opening { keep_choices, .. },
                RuntimeInitialization::Opening {
                    keep_choices: actual,
                },
            ) => assert_eq!(keep_choices, actual),
            (Initialization::Scheduled { .. }, RuntimeInitialization::Prepared) => {}
            _ => panic!("runtime initializer kind changed"),
        }
        assert_eq!(
            runtime
                .decision_plans
                .iter()
                .map(|plan| plan.legacy().expect("old plan stays in the legacy wrapper"))
                .collect::<Vec<_>>(),
            expected.decision_plans.iter().collect::<Vec<_>>()
        );
        assert_eq!(runtime.inventory, expected.inventory);
        assert_eq!(runtime.stop, expected.stop);
        assert_eq!(runtime.budget, expected.budget);
        assert_eq!(runtime.reached_probe_limit, expected.reached_probe_limit);
    }
}

#[test]
fn cast_and_quote_loops_keep_distinct_old_error_order_and_native_kind_support() {
    let cases = [
        (
            vec![row(Mana::Blue, "1"), row(Mana::Blue, "bad")],
            "duplicate mandatory mana spend type",
        ),
        (
            vec![row(Mana::Blue, "020")],
            "invalid mandatory integer: expected a canonical decimal integer string",
        ),
        (vec![row(Mana::Blue, "0")], "nonpositive mandatory spend"),
        (vec![row(Mana::Blue, "-1")], "nonpositive mandatory spend"),
    ];
    for (payment, expected) in cases {
        let native = payment
            .iter()
            .map(|r| row(FinitePolicy::mana(r.mana_type), &r.amount))
            .collect::<Payment<ManaType>>();
        assert_eq!(FinitePolicy::payment(&payment).unwrap_err(), expected);
        assert_eq!(FinitePolicy::payment(&native).unwrap_err(), expected);
    }
    let old = vec![row(Mana::Blue, "1"), row(Mana::White, "2")];
    let native = vec![row(ManaType::Blue, "1"), row(ManaType::White, "2")];
    assert_eq!(FinitePolicy::payment(&old), FinitePolicy::payment(&native));
    assert_eq!(
        FinitePolicy::payment(&vec![row(ManaType::Colourless, "2")])
            .unwrap()
            .spend[0]
            .mana_type,
        ManaType::Colourless
    );
    // Quote/funding validation intentionally parses before duplicate detection.
    let invalid = vec![row(Mana::Blue, "1"), row(Mana::Blue, "bad")];
    assert_eq!(
        FinitePolicy::amounts(&invalid, true).unwrap_err(),
        "invalid mandatory integer: expected a canonical decimal integer string"
    );
    assert!(FinitePolicy::amounts(&vec![row(Mana::Blue, "0")], true).is_ok());
    assert_eq!(
        FinitePolicy::amounts(&vec![row(Mana::Blue, "0")], false).unwrap_err(),
        "duplicate or invalid finite mana amount"
    );
}

#[test]
fn normalization_keeps_invalid_cast_amount_as_unparsed_lexeme() {
    let mut recipe = inputs()
        .unwrap()
        .cases
        .into_iter()
        .find(|row| row.id == MandatoryCase::T04OneCard)
        .unwrap()
        .map_id(CaseId::Mandatory);
    let instruction = recipe
        .instructions
        .iter_mut()
        .find(|row| {
            matches!(
                row,
                Instruction::CastUntargeted { .. } | Instruction::CastTargeted { .. }
            )
        })
        .unwrap();
    let (Instruction::CastUntargeted { payment, .. } | Instruction::CastTargeted { payment, .. }) =
        instruction
    else {
        unreachable!()
    };
    payment[0].amount = "020".into();
    let runtime = FinitePolicy::normalize_recipe(recipe);
    let instruction = runtime
        .instructions
        .iter()
        .find(|row| {
            matches!(
                row,
                Instruction::CastUntargeted { .. } | Instruction::CastTargeted { .. }
            )
        })
        .unwrap();
    let (Instruction::CastUntargeted { payment, .. } | Instruction::CastTargeted { payment, .. }) =
        instruction
    else {
        unreachable!()
    };
    assert_eq!(payment[0].amount, "020");
    assert_eq!(
        FinitePolicy::payment(payment).unwrap_err(),
        "invalid mandatory integer: expected a canonical decimal integer string"
    );
}

fn required(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("explicit {name} required"))
}
fn initialize(fixture: &Fixture) -> GameState {
    match fixture.initialization {
        FixtureInitialization::Opening => GameState::initialize_opening(
            fixture.game,
            fixture.context.clone(),
            fixture.setup.clone(),
        ),
        FixtureInitialization::Scheduled(start) => GameState::initialize_scheduled(
            fixture.game,
            fixture.context.clone(),
            fixture.setup.seed.clone(),
            start,
        ),
    }
    .unwrap()
    .into_state()
}
fn drive(policy: &mut FinitePolicy, state: &mut GameState) {
    for _ in 0..4096 {
        if state.outcome().is_some() || policy.completed_boundary(state).unwrap().is_some() {
            return;
        }
        let (input, _, intent) = policy.prepare(state).unwrap();
        let candidate = state.advance(input.clone()).unwrap();
        policy.accept(&input, &candidate, intent).unwrap();
        *state = candidate.into_state();
    }
    panic!("bounded old finite driver did not finish");
}
#[test]
#[ignore = "requires genuine complete source19/metadata/vocabulary; actual opening metadata and entropy guards only"]
fn genuine_opening_keep_and_unused_entropy_guards_remain_active() {
    let vocab = crate::creature_types::selected_vocabulary().unwrap();
    let source = crate::ordinary_game::mandatory::MandatorySource::load(
        &required("SPELLWAKE_CATALOGUE_METADATA"),
        &required("SPELLWAKE_CATALOGUE_INPUT"),
        &vocab,
    )
    .unwrap();
    let fixture = source
        .fixture(MandatoryCase::Mt01, Arc::clone(&vocab))
        .unwrap();
    let mut policy = FinitePolicy::new(&fixture).unwrap();
    let initial = initialize(&fixture);
    let RuntimeInitialization::Opening { keep_choices } = &mut policy.recipe.initialization else {
        panic!()
    };
    keep_choices[0].role = Role::P1;
    assert_eq!(
        policy.prepare(&initial).unwrap_err(),
        "actual starter-first opening frame mismatch"
    );
    let mut fixture = fixture;
    fixture.setup.entropy = fixture.setup.entropy.appended(vec![0]).unwrap();
    let mut policy = FinitePolicy::new(&fixture).unwrap();
    let mut state = initialize(&fixture);
    drive(&mut policy, &mut state);
    assert_eq!(state.entropy().unwrap().remaining(), 1);
    assert_eq!(
        policy.finish(&state).unwrap_err(),
        "mandatory unused opening entropy"
    );
}
#[test]
#[ignore = "requires genuine complete source19/metadata/vocabulary; actual optional constructor-to-issue numeric timing only"]
fn genuine_optional_bad_cast_is_accepted_by_normalization_then_refused_at_issue() {
    let vocab = crate::creature_types::selected_vocabulary().unwrap();
    let source = OptionalSource::load(
        &required("SPELLWAKE_CATALOGUE_METADATA"),
        &required("SPELLWAKE_CATALOGUE_INPUT"),
        &vocab,
    )
    .unwrap();
    let fixture = source.fixture(OptionalCase::R11, vocab).unwrap();
    let mut recipe = crate::ordinary_game::optional::recipe(OptionalCase::R11).unwrap();
    let instruction=recipe.instructions.iter_mut().find(|row|matches!(row,Instruction::CastUntargeted{payment,..}|Instruction::CastTargeted{payment,..} if !payment.is_empty())).unwrap();
    let (Instruction::CastUntargeted { payment, .. } | Instruction::CastTargeted { payment, .. }) =
        instruction
    else {
        unreachable!()
    };
    payment[0].amount = "020".into();
    let mut policy = FinitePolicy::from_optional(&fixture, recipe)
        .expect("old constructor must not parse cast spend early");
    let mut state = initialize(&fixture);
    let mut accepted = 0;
    for _ in 0..512 {
        let before = state.clone();
        let prior = format!("{policy:?}");
        match policy.prepare(&state) {
            Err(error) => {
                assert_eq!(
                    error,
                    "invalid mandatory integer: expected a canonical decimal integer string"
                );
                assert_eq!(state, before);
                assert_eq!(format!("{policy:?}"), prior);
                assert!(accepted > 0);
                return;
            }
            Ok((input, _, intent)) => {
                let candidate = state.advance(input.clone()).unwrap();
                policy.accept(&input, &candidate, intent).unwrap();
                state = candidate.into_state();
                accepted += 1;
            }
        }
    }
    panic!("actual optional case did not reach malformed cast issue");
}
