//! A single melded permanent has two physical cards and one battlefield arrival.
use super::{
    cmm_gameplay::{creature, named},
    *,
};
use spellwake_contracts::names::{EnglishCardName, ObjectNames};
use spellwake_engine::characteristics::KeywordAbility::{Haste, Menace};

fn field(player: SeatId) -> ObjectDestination {
    ObjectDestination::Battlefield { controller: player }
}
fn choice(state: GameState) -> GameState {
    exile_hand::until(state, |s| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::ResolutionSelect(_))
        )
    })
}
fn view(s: &GameState, n: u64) -> spellwake_engine::characteristics::ObjectCharacteristics {
    s.execution()
        .current_characteristics(physical(s, n).id())
        .unwrap()
        .unwrap()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_meld_graf_rats_chooses_one_genuine_partner_enters_once_and_both_cards_return_to_hand() {
    let init = game(
        vec![A, B],
        vec![
            named("Graf Rats", 1, A, field(A)),
            named("Midnight Scavengers", 2, A, field(A)),
            named("Midnight Scavengers", 3, A, field(A)),
            named("Midnight Scavengers", 4, B, field(A)),
            creature(5, A, 1, 2, &[]),
            seed(
                6,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_CREATURE,
                    crate::keyword_creatures::toolbox_emit::return_to_owners_hand(),
                ),
            ),
        ],
        &[],
        Step::PrecombatMain,
    );
    let selected = choice(init);
    let RuleFrame::ResolutionSelect(f) = selected.pending_decision().unwrap().frame() else {
        unreachable!()
    };
    assert_eq!((f.minimum(), f.required()), (1, 1));
    assert_eq!(
        f.domain(),
        [physical(&selected, 2).id(), physical(&selected, 3).id()]
    );
    let arrival = reply(
        &selected,
        DecisionReply::SelectResolution(vec![physical(&selected, 3).id()]),
    );
    let joined = physical(arrival.state(), 1);
    assert_eq!(joined.id(), physical(arrival.state(), 3).id());
    assert_eq!(joined.component_origins().count(), 2);
    let enters=events(&arrival).flat_map(|e|&e.occurrences).filter(|o|matches!(&o.value,RulesOccurrence::ZoneChange(z) if z.after.object.id()==joined.id() && z.after.object.zone()==ObjectZone::Battlefield)).count();
    assert_eq!(enters, 1);
    let s = exile_hand::until(arrival.into_state(), |s| {
        s.pending_decision().is_none() && s.execution().window().entries().is_empty()
    });
    let host = view(&s, 1);
    assert_eq!(
        host.current_names.iter().next().unwrap().as_str(),
        "Chittering Host"
    );
    assert_eq!(host.defined.mana_value, 7.into());
    assert!(host.has_keyword(Haste) && host.has_keyword(Menace));
    assert_eq!(
        host.power_toughness.unwrap().derived,
        PowerToughness {
            power: 5.into(),
            toughness: 6.into()
        }
    );
    let ally = view(&s, 5);
    assert_eq!(
        ally.power_toughness.as_ref().unwrap().derived.power,
        2.into()
    );
    assert!(ally.has_keyword(Menace));
    let returned = round(targeted(&s, A, 6, TargetRef::Object(physical(&s, 1).id())).into_state())
        .into_state();
    for (n, name) in [(1, "Graf Rats"), (3, "Midnight Scavengers")] {
        assert_eq!(physical(&returned, n).zone(), ObjectZone::Hand(A));
        assert_eq!(
            view(&returned, n)
                .current_names
                .iter()
                .next()
                .unwrap()
                .as_str(),
            name
        );
    }
    assert_ne!(physical(&returned, 1).id(), physical(&returned, 3).id());
    assert_eq!(physical(&returned, 2).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_meld_a_named_impostor_is_exiled_but_cannot_form_the_joined_face() {
    let mut impostor = creature(2, A, 2, 2, &[]);
    impostor.program.names =
        ObjectNames::single(EnglishCardName::new("Midnight Scavengers".to_owned()).unwrap());
    let selected = choice(game(
        vec![A, B],
        vec![named("Graf Rats", 1, A, field(A)), impostor],
        &[],
        Step::PrecombatMain,
    ));
    let result = reply(
        &selected,
        DecisionReply::SelectResolution(vec![physical(&selected, 2).id()]),
    )
    .into_state();
    assert_eq!(physical(&result, 1).zone(), ObjectZone::Exile);
    assert_eq!(physical(&result, 2).zone(), ObjectZone::Exile);
    assert_ne!(physical(&result, 1).id(), physical(&result, 2).id());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_meld_midnight_scavengers_targets_a_small_creature_then_may_return_it_to_hand() {
    use super::cmm_gameplay::illegal;
    use spellwake_engine::effects::OptionalEffectChoice;
    let price = [(ManaType::Colourless, 4), (ManaType::Black, 1)];
    let init = game(
        vec![A, B],
        vec![
            named("Midnight Scavengers", 1, A, ObjectDestination::Hand),
            named("Graf Rats", 2, A, ObjectDestination::Graveyard),
            named("Midnight Scavengers", 3, A, ObjectDestination::Graveyard),
            named("Graf Rats", 4, B, ObjectDestination::Graveyard),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&init, 1, &price).into_state()).into_state();
    let pick = |n| DecisionReply::TriggerTarget(TargetRef::Object(physical(&entered, n).id()));
    for n in [3, 4] {
        illegal(
            &entered,
            A,
            GameCommand::Reply {
                decision: entered.pending_decision().unwrap().id(),
                choice: pick(n),
            },
        );
    }
    let offer = round(reply(&entered, pick(2)).into_state()).into_state();
    let declined = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(
        physical(declined.state(), 2).zone(),
        ObjectZone::Graveyard(A)
    );
    let returned = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(physical(returned.state(), 2).zone(), ObjectZone::Hand(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_meld_preflights_every_dated_pair_and_validates_its_compilable_components() {
    let (inventory, vocabulary) = super::super::dated_sources();
    let mut sets = BTreeMap::new();
    for row in inventory.cards.iter().filter(|row| row.layout == "meld") {
        let family = inventory
            .meld_set(row.oracle_id.as_deref().unwrap())
            .unwrap();
        let compiled = crate::keyword_creatures::meld::compile(row, inventory, &vocabulary, true);
        if let Some(program) = &compiled {
            program
                .validate(&vocabulary)
                .unwrap_or_else(|error| panic!("{}: {error:?}", row.name));
        }
        let mut leaf = row.clone();
        leaf.layout = "normal".to_owned();
        let text = crate::keyword_creatures::without_reminder(
            leaf.faces[0].oracle_text.as_deref().unwrap(),
        )
        .unwrap();
        let residual = text
            .lines()
            .filter(|line| !line.contains("then meld them into"))
            .collect::<Vec<_>>();
        leaf.faces[0].oracle_text = Some(residual.join("\n"));
        if row.name == family.result {
            leaf.faces[0].mana_cost = Some("{0}".into());
            assert!(compiled.is_none(), "a joint back cannot be seeded alone");
        }
        let body = crate::keyword_creatures::compile(
            &leaf,
            &vocabulary,
            crate::keyword_creatures::Profile::Current,
        )
        .and_then(|program| {
            program
                .validate(&vocabulary)
                .map_err(|_| "invalid-standalone-body")
        });
        println!(
            "meld source | {} | {} | component executable={} | residual body={:?}",
            family.result,
            row.name,
            compiled.is_some(),
            body
        );
        sets.entry(family.result.clone())
            .or_insert_with(Vec::new)
            .push((row.name.clone(), compiled.is_some()));
    }
    assert_eq!(sets.len(), 7);
    assert!(sets.values().all(|rows| rows.len() == 3));
    assert!(
        sets["Chittering Host"]
            .iter()
            .filter(|(name, _)| name != "Chittering Host")
            .all(|(_, admitted)| *admitted)
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_meld_ghostly_flicker_tracks_both_cards_that_the_melded_permanent_becomes() {
    use spellwake_engine::{land::BasicLandType, mana::ManaAbilityRef};
    let mut cards = vec![
        named("Graf Rats", 1, A, field(A)),
        named("Midnight Scavengers", 2, A, field(A)),
        named("Ghostly Flicker", 3, A, ObjectDestination::Hand),
    ];
    cards.extend((4..=6).map(|n| named("Island", n, A, field(A))));
    let choose = choice(game(vec![A, B], cards, &[], Step::PrecombatMain));
    let joined = reply(
        &choose,
        DecisionReply::SelectResolution(vec![physical(&choose, 2).id()]),
    )
    .into_state();
    let mut ready = exile_hand::until(joined, |s| {
        s.pending_decision().is_none() && s.execution().window().entries().is_empty()
    });
    for n in 4..=6 {
        ready = command(
            &ready,
            A,
            GameCommand::ActivateMana {
                source: physical(&ready, n).id(),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
            },
        )
        .into_state();
    }
    let original = physical(&ready, 1).id();
    let stacked = command(
        &ready,
        A,
        GameCommand::CastWithResources {
            spell: physical(&ready, 3).id(),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![
                    TargetRef::Object(original),
                    TargetRef::Object(physical(&ready, 6).id()),
                ],
                mana: Some(spend(&[(ManaType::Blue, 3)])),
                ..Default::default()
            },
        },
    )
    .into_state();
    let exile = round(stacked).into_state();
    let Some(RuleFrame::LinkedReturnOrder(order)) = exile.pending_decision().map(|p| p.frame())
    else {
        panic!("three returning permanents require one timestamp order")
    };
    assert_eq!(order.objects().len(), 3);
    for n in [1, 2, 6] {
        assert_eq!(physical(&exile, n).zone(), ObjectZone::Exile);
    }
    let result = reply(
        &exile,
        DecisionReply::LinkedReturnOrder(order.objects().to_vec()),
    )
    .into_state();
    for (n, name) in [(1, "Graf Rats"), (2, "Midnight Scavengers"), (6, "Island")] {
        assert_eq!(physical(&result, n).zone(), ObjectZone::Battlefield);
        assert_ne!(physical(&result, n).id(), original);
        assert_eq!(
            view(&result, n)
                .current_names
                .iter()
                .next()
                .unwrap()
                .as_str(),
            name
        );
    }
    assert_ne!(physical(&result, 1).id(), physical(&result, 2).id());
}
