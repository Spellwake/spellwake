//! Cast and placement through the public wire and retained native input codec.
use super::*;

#[test]
#[ignore = "requires retained source-qualified vocabulary"]
fn mutating_cast_replays_top_or_bottom_and_projects_the_result_to_both_seats() {
    use spellwake_engine::{characteristics::KeywordAbility, program::ConditionalCastCost};
    let mut incoming =
        Program::creature(Some(ManaCost::free()), 4.into(), 5.into(), BTreeSet::new());
    incoming.conditional_cost = Some(ConditionalCastCost::Mutate(ManaCost::free()));
    incoming.printed_keywords.insert(KeywordAbility::Flying);
    let mut host = Program::creature(None, 2.into(), 3.into(), BTreeSet::new());
    host.printed_keywords.insert(KeywordAbility::Vigilance);
    let state = main_state(vec![
        card(1, A, ObjectDestination::Hand, incoming),
        card(2, A, ObjectDestination::Battlefield { controller: A }, host),
    ]);
    let cast = bridged(&state, viewer_a(), |v| PlayerAction::CastWithResources {
        face: None,
        adventure: false,
        hybrid_choices: vec![],
        fixed_targets: vec![],
        chosen_x: None,
        permanent_targets: vec![],
        target_division: vec![],
        modes: vec![],
        source: v.issue(NativeReference::Object(object(&state, 1))).unwrap(),
        target: Some(PlayerTarget::Object {
            handle: v.issue(NativeReference::Object(object(&state, 2))).unwrap(),
        }),
        use_alternative: true,
        payment: None,
        additional: vec![],
        resources: vec![],
    });
    let passed = bridged(&cast, viewer_a(), |_| PlayerAction::Pass {});
    let placement = bridged(&passed, viewer_b(), |_| PlayerAction::Pass {});
    let opponent = scope(&placement, viewer_b())
        .project(&metadata(), &Display::default())
        .unwrap();
    assert!(opponent.decisions.is_empty());
    for (selection, power) in [(1, 4), (2, 2)] {
        let merged = bridged(&placement, viewer_a(), |v| {
            let view = v.project(&metadata(), &Display::default()).unwrap();
            let choice = &view.decisions[0];
            assert!(
                matches!(&choice.domain, PlayerDomain::Number {minimum, maximum,..} if *minimum == 1.into() && *maximum == 2.into())
            );
            PlayerAction::Choose {
                decision: choice.decision.clone(),
                response: PlayerChoice::Number {
                    value: selection.into(),
                },
            }
        });
        assert_eq!(object(&merged, 1), object(&state, 2));
        assert_eq!(object(&merged, 2), object(&state, 2));
        for viewer in [viewer_a(), viewer_b()] {
            let seen = scope(&merged, viewer)
                .project(&metadata(), &Display::default())
                .unwrap();
            let joined: Vec<_> = seen
                .visible_objects
                .iter()
                .filter(|c| c.times_mutated.is_some())
                .collect();
            assert_eq!(joined.len(), 1);
            let public = joined[0].public_properties.as_ref().unwrap();
            assert_eq!(public.power, Some(power.into()));
            assert!(public.keywords.contains(&PlayerKeyword::Flying));
            assert!(public.keywords.contains(&PlayerKeyword::Vigilance));
        }
    }
}
