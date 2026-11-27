//! Optional entry effects, stack targets and ownership of linked created tokens.
use super::*;

fn simple_cast(s: &GameState, who: SeatId, id: u64) -> GameState {
    round(
        command(
            s,
            who,
            GameCommand::CastUntargeted {
                spell: physical(s, id).id(),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state()
}
fn artifact_creature(id: u64, owner: SeatId, p: i64, t: i64) -> SeedCard {
    let mut card = Program::creature(
        Some(ManaCost::free()),
        p.into(),
        t.into(),
        Default::default(),
    );
    card.card_types.insert(CardType::Artifact);
    seed(id, owner, ObjectDestination::Hand, card)
}
fn created_token(result: &NativeCandidate, owner: SeatId) -> ObjectId {
    let added = events(result)
        .flat_map(|ev| ev.occurrences.iter())
        .filter_map(|row| match &row.value {
            RulesOccurrence::TokenCreated { creator, after } if *creator == owner => {
                Some(after.object.id())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [id] = added.as_slice() else {
        panic!("expected one created token, got {added:?}");
    };
    *id
}
fn live_tokens(state: &GameState, owner: SeatId) -> Vec<ObjectId> {
    state
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| {
            c.is_token() && c.zone() == ObjectZone::Battlefield && c.controller() == Some(owner)
        })
        .map(|c| c.id())
        .collect()
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_faerie_artisans_copies_only_opposing_nontoken_creatures_and_keeps_only_its_own_most_recent_artifact_copy()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (Blue, 1)];
    let donor = seed(
        2,
        B,
        ObjectDestination::Hand,
        Program::creature(
            Some(ManaCost::free()),
            2.into(),
            3.into(),
            Default::default(),
        ),
    );
    let mut token_source = named("Dragon Fodder", 7, B, ObjectDestination::Hand);
    let ProgramKind::Spell(body) = &token_source.program.kind else {
        panic!("token spell");
    };
    token_source.program = Program::instant(ManaCost::free(), body.body.clone());
    let mut unrelated = token_source.clone();
    unrelated.card = CardId(8);
    unrelated.owner = A;
    let cards = vec![
        named("Faerie Artisans", 1, A, ObjectDestination::Hand),
        donor,
        artifact_creature(3, C, 4, 3),
        artifact_creature(4, A, 2, 4),
        super::super::super::cmm_gameplay::remove_spell(5, A),
        token_source,
        unrelated,
    ];
    let before = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let initial = cast_card(&before, 1, &price);
    assert!(sheet(&initial, physical(&initial, 1).id()).has_keyword(KeywordAbility::Flying));
    let own = cast_card(&initial, 4, &[]);
    assert!(own.execution().window().entries().is_empty());
    let batch =
        super::super::equipment::order_tokens(round(cast_untargeted(&own, 8, &[]).into_state()))
            .into_state();
    let keep = live_tokens(&batch, A);
    assert_eq!(keep.len(), 2);
    let opponent = main_on(batch, 2);
    let tokens_in = super::super::equipment::order_tokens(round(
        command(
            &opponent,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&opponent, 7).id(),
                payment: None,
            },
        )
        .into_state(),
    ))
    .into_state();
    assert!(tokens_in.execution().window().entries().is_empty());
    assert_eq!(live_tokens(&tokens_in, A), keep);
    let entered = simple_cast(&tokens_in, B, 2);
    assert_eq!(entered.execution().window().entries().len(), 1);
    let active = pass(&pass(&entered).into_state()).into_state();
    let dead = round(targeted(&active, A, 5, marker(&active, 2)).into_state()).into_state();
    assert_eq!(physical(&dead, 2).zone(), ObjectZone::Graveyard(B));
    let copy = round(dead);
    let first = created_token(&copy, A);
    let field = copy.into_state();
    assert_eq!(
        sheet(&field, first).current_types,
        [CardType::Artifact, CardType::Creature].into()
    );
    stats(&field, first, 2, 3);
    let next = main_on(field, 3);
    let other = simple_cast(&next, C, 3);
    let new = round(other);
    let token = created_token(&new, A);
    let end = new.into_state();
    assert!(end.execution().window().cards().get(first).is_none());
    stats(&end, token, 4, 3);
    assert_eq!(
        sheet(&end, token).current_types,
        [CardType::Artifact, CardType::Creature].into()
    );
    for id in keep {
        assert!(end.execution().window().cards().get(id).is_some());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_brinelin_bounces_a_legal_nonland_on_entry_or_its_controllers_six_mana_value_cast_and_is_a_partner_commander()
 {
    use ManaType::*;
    let price = [(Colourless, 6), (Blue, 2)];
    let cards = vec![
        named("Brinelin, the Moon Kraken", 1, A, ObjectDestination::Hand),
        creature(2, B, 2, 2, &[]),
        named("Island", 3, B, battlefield(B)),
        seed(
            4,
            A,
            ObjectDestination::Hand,
            Program::artifact(Some(ManaCost::generic(6.into())), vec![]),
        ),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            Program::artifact(Some(ManaCost::generic(5.into())), vec![]),
        ),
        named("Sol Ring", 6, B, battlefield(B)),
    ];
    let initial = game(
        vec![A, B],
        cards,
        &[(Colourless, 12), (Blue, 2)],
        Step::PrecombatMain,
    );
    let entry = cast_card(&initial, 1, &price);
    super::super::declarations::invalid(&entry, A, DecisionReply::TriggerTarget(marker(&entry, 3)));
    let triggered = reply(&entry, DecisionReply::TriggerTarget(marker(&entry, 6))).into_state();
    let may = round(triggered).into_state();
    let no = super::super::equipment::optional(&may, false);
    assert_eq!(physical(&no, 6).zone(), ObjectZone::Battlefield);
    let yes = super::super::equipment::optional(&may, true);
    assert_eq!(physical(&yes, 6).zone(), ObjectZone::Hand(B));
    let low = cast_untargeted(&no, 5, &[(Colourless, 5)]).into_state();
    assert!(low.pending_decision().is_none());
    assert_eq!(low.execution().window().entries().len(), 1);
    let six = cast_untargeted(&no, 4, &[(Colourless, 6)]).into_state();
    let triggered = reply(&six, DecisionReply::TriggerTarget(marker(&six, 2))).into_state();
    let may = round(triggered).into_state();
    let took = super::super::equipment::optional(&may, true);
    assert_eq!(physical(&took, 2).zone(), ObjectZone::Hand(B));
    assert_eq!(physical(&took, 4).zone(), ObjectZone::Stack);
    let inv = current_catalogue().0;
    let id = |label: &str| {
        OracleId::try_from(
            inv.cards
                .iter()
                .find(|c| c.name == label)
                .unwrap()
                .oracle_id
                .clone()
                .unwrap(),
        )
        .unwrap()
    };
    let leaders = [
        id("Brinelin, the Moon Kraken"),
        id("Keleth, Sunmane Familiar"),
    ];
    let deck = crate::commander::prepare_deck(
        inv,
        &spellwake_contracts::names::NameRules::literal_names(),
        &leaders,
        &vec![id("Wastes"); 98],
        &Default::default(),
    )
    .unwrap();
    assert_eq!(deck.commanders(), &leaders);
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_mirrorpool_enters_tapped_produces_colorless_and_sacrifices_to_copy_an_owned_spell_or_controlled_creature()
 {
    use ManaType::*;
    use spellwake_engine::effects::CopyTargetChoice;
    let land = named("Mirrorpool", 1, A, ObjectDestination::Hand);
    assert_eq!(land.program.activated.len(), 2);
    let spell_ability = land
        .program
        .activated
        .iter()
        .find(|ab| {
            matches!(
                ab.body.target,
                BodyTarget::Spell {
                    include_abilities: false,
                    ..
                }
            )
        })
        .unwrap()
        .id;
    let creature_ability = land
        .program
        .activated
        .iter()
        .find(|ab| ab.id != spell_ability)
        .unwrap()
        .id;
    let cards = vec![
        land,
        creature(2, A, 2, 6, &[KeywordAbility::Reach]),
        creature(3, B, 2, 6, &[]),
        seed(
            4,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_CREATURE,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(2),
                    false,
                ),
            ),
        ),
        seed(
            5,
            B,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_CREATURE,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(1),
                    false,
                ),
            ),
        ),
        super::super::loyalty::mana_source(6, &[(Colourless, 5)]),
    ];
    let before = game(vec![A, B], cards, &[], Step::PrecombatMain);
    let played = command(
        &before,
        A,
        GameCommand::PlayLand {
            land: physical(&before, 1).id(),
        },
    )
    .into_state();
    let source = physical(&played, 1).id();
    let tap = GameCommand::ActivateMana {
        source,
        ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
    };
    assert_eq!(physical(&played, 1).tapped(), Some(true));
    illegal(&played, A, tap.clone());
    let field = main_on(played, 3);
    let tapped = command(&field, A, tap).into_state();
    assert_eq!(tapped.execution().mana(A, Colourless), 1.into());
    let state = command(
        &field,
        A,
        GameCommand::ActivateMana {
            source: physical(&field, 6).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let copy = |ab, t, n| GameCommand::ActivateAbility {
        source,
        ability: ab,
        target: t,
        payment: Some(spend(&[(Colourless, n)])),
    };
    illegal(&state, A, copy(creature_ability, marker(&state, 3), 5));
    let sacrificed = command(&state, A, copy(creature_ability, marker(&state, 2), 5)).into_state();
    assert_eq!(physical(&sacrificed, 1).zone(), ObjectZone::Graveyard(A));
    let cloned = round(sacrificed);
    let token = tokens(&cloned, A, 1)[0];
    stats(cloned.state(), token, 2, 6);
    assert!(sheet(cloned.state(), token).has_keyword(KeywordAbility::Reach));
    let opposing = pass(&state).into_state();
    let enemy = targeted(&opposing, B, 5, marker(&opposing, 2)).into_state();
    let mine = pass(&enemy).into_state();
    let foreign = TargetRef::Stack(mine.execution().window().entries().last().unwrap().id());
    illegal(&mine, A, copy(spell_ability, foreign, 3));
    let chosen = targeted(&state, A, 4, marker(&state, 2)).into_state();
    let original = chosen.execution().window().entries().last().unwrap().id();
    let activated = command(
        &chosen,
        A,
        copy(spell_ability, TargetRef::Stack(original), 3),
    )
    .into_state();
    assert_eq!(physical(&activated, 1).zone(), ObjectZone::Graveyard(A));
    let offer = round(activated).into_state();
    for change in [false, true] {
        let replacement = if change {
            CopyTargetChoice::Change(marker(&offer, 3))
        } else {
            CopyTargetChoice::Retain
        };
        let doubled = reply(&offer, DecisionReply::CopyTargets(replacement)).into_state();
        let after = round(round(doubled).into_state()).into_state();
        for (n, damage) in [
            (2, if change { 2 } else { 4 }),
            (3, if change { 2 } else { 0 }),
        ] {
            assert_eq!(
                after
                    .execution()
                    .battlefield_marked_damage(physical(&after, n).id()),
                Some(damage.into())
            );
        }
    }
}
