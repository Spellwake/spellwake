//! Source-bound mutate spells through actual casting, priority, merging and departure.
use super::cmm_gameplay::{creature, illegal, named};
use super::*;

fn announce(
    state: &GameState,
    spell: u64,
    target: u64,
    payment: &[(ManaType, i64)],
) -> GameCommand {
    GameCommand::CastWithResources {
        spell: physical(state, spell).id(),
        target: Some(TargetRef::Object(physical(state, target).id())),
        payment: CastingPayment {
            mana: Some(spend(payment)),
            use_alternative: true,
            ..Default::default()
        },
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_mutate_dreamtail_heron_chooses_either_order_retains_host_and_draws_only_after_a_merge() {
    use ManaType::*;
    use spellwake_engine::characteristics::KeywordAbility::{Flying, Vigilance};
    let price = [(Colourless, 3), (Blue, 1)];
    let human_type = current_catalogue().1.resolve_label("Human").unwrap();
    let human = Program::creature(None, 2.into(), 2.into(), [human_type].into());
    let start = game(
        vec![A, B],
        vec![
            named("Dreamtail Heron", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 3, &[Vigilance]),
            creature(3, B, 2, 3, &[]),
            seed(
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
                human,
            ),
            seed(
                5,
                B,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::ReturnToOwnersHand),
            ),
            seed(
                6,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::ReturnToOwnersHand),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    for illegal_target in [3, 4] {
        illegal(&start, A, announce(&start, 1, illegal_target, &price));
    }
    illegal(
        &start,
        A,
        announce(&start, 1, 2, &[(Colourless, 2), (Blue, 1)]),
    );
    let host = physical(&start, 2).id();
    let cast = command(&start, A, announce(&start, 1, 2, &price)).into_state();
    let choosing = round(cast.clone()).into_state();
    let RuleFrame::Mutation(frame) = choosing.pending_decision().unwrap().frame() else {
        panic!("expected placement while resolving the spell")
    };
    assert_eq!(frame.host(), host);
    assert_eq!(frame.spell(), physical(&choosing, 1).id());
    for on_top in [true, false] {
        let merged = reply(&choosing, DecisionReply::MutateOnTop(on_top)).into_state();
        assert_eq!(physical(&merged, 1).id(), host);
        assert_eq!(physical(&merged, 2).id(), host);
        assert_eq!(physical(&merged, 1).times_mutated(), 1);
        let current = merged
            .execution()
            .current_characteristics(host)
            .unwrap()
            .unwrap();
        assert!(current.has_keyword(Flying) && current.has_keyword(Vigilance));
        let pt = current.power_toughness.unwrap().derived;
        let expected = if on_top {
            (3.into(), 4.into())
        } else {
            (2.into(), 3.into())
        };
        assert_eq!((pt.power, pt.toughness), expected);
        assert_eq!(physical(&merged, 100).zone(), ObjectZone::Library(A));
        let drawn = round(merged).into_state();
        assert_eq!(physical(&drawn, 100).zone(), ObjectZone::Hand(A));
        let returned =
            round(targeted(&drawn, A, 6, TargetRef::Object(host)).into_state()).into_state();
        assert_ne!(physical(&returned, 1).id(), physical(&returned, 2).id());
        assert_eq!(physical(&returned, 1).zone(), ObjectZone::Hand(A));
        assert_eq!(physical(&returned, 2).zone(), ObjectZone::Hand(A));
    }
    let b = pass(&cast).into_state();
    let bounce = targeted(&b, B, 5, TargetRef::Object(host));
    let gone = round(bounce.into_state()).into_state();
    let resolved = round(gone).into_state();
    assert_eq!(physical(&resolved, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&resolved, 1).times_mutated(), 0);
    assert_eq!(physical(&resolved, 2).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&resolved, 100).zone(), ObjectZone::Library(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_mutate_vulpikeet_puts_a_counter_on_the_same_host_and_grants_flying() {
    let price = [(ManaType::Colourless, 2), (ManaType::White, 1)];
    let start = game(
        vec![A, B],
        vec![
            named("Vulpikeet", 1, A, ObjectDestination::Hand),
            creature(2, A, 1, 4, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let choosing =
        round(command(&start, A, announce(&start, 1, 2, &price)).into_state()).into_state();
    let merged = reply(&choosing, DecisionReply::MutateOnTop(false)).into_state();
    let host = physical(&merged, 2).id();
    let done = round(merged).into_state();
    assert_eq!(
        done.execution()
            .power_toughness_counters(host)
            .unwrap()
            .plus_one,
        1.into()
    );
    let row = done
        .execution()
        .current_characteristics(host)
        .unwrap()
        .unwrap();
    assert!(row.has_keyword(KeywordAbility::Flying));
    let pt = row.power_toughness.unwrap().derived;
    assert_eq!((pt.power, pt.toughness), (2.into(), 5.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_mutate_keeps_an_earlier_entry_copy_and_recomputes_the_top_face_for_either_order() {
    use ManaType::*;
    use spellwake_engine::effects::ZoneReplacementReply;
    let state = game(
        vec![A, B],
        vec![
            named("Body Double", 1, A, ObjectDestination::Hand),
            named("Elvish Mystic", 2, B, ObjectDestination::Graveyard),
            named("Vulpikeet", 3, A, ObjectDestination::Hand),
        ],
        &[(Colourless, 6), (Blue, 1), (White, 1)],
        Step::PrecombatMain,
    );
    let choice =
        round(cast_untargeted(&state, 1, &[(Colourless, 4), (Blue, 1)]).into_state()).into_state();
    let host = reply(
        &choice,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::EntryCopy(Some(
            physical(&choice, 2).id(),
        ))),
    )
    .into_state();
    let id = physical(&host, 1).id();
    let placement = round(
        command(
            &host,
            A,
            announce(&host, 3, 1, &[(Colourless, 2), (White, 1)]),
        )
        .into_state(),
    )
    .into_state();
    for (on_top, name, size) in [
        (true, "Vulpikeet", (3, 4)),
        (false, "Elvish Mystic", (2, 2)),
    ] {
        let joined = reply(&placement, DecisionReply::MutateOnTop(on_top)).into_state();
        let done = round(joined).into_state();
        let current = done
            .execution()
            .current_characteristics(id)
            .unwrap()
            .unwrap();
        assert!(
            done.execution()
                .display_program(id)
                .unwrap()
                .names
                .iter()
                .any(|n| n.as_str() == name)
        );
        assert!(current.current_names.iter().any(|n| n.as_str() == name));
        assert!(current.has_keyword(KeywordAbility::Flying));
        assert_eq!(current.mana_abilities().len(), 1);
        let pt = current.power_toughness.unwrap().derived;
        assert_eq!((pt.power, pt.toughness), (size.0.into(), size.1.into()));
        assert_eq!(physical(&done, 2).zone(), ObjectZone::Graveyard(B));
    }
}
