//! Source-bound graveyard, upkeep and counter-trigger interactions.
use super::*;
use spellwake_engine::{
    characteristics::KeywordAbility::Flying,
    effects::{CounterKind, OptionalEffectChoice},
};

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_chainer_returns_an_owned_or_opposing_creature_as_a_black_nightmare_and_exiles_every_nightmare_when_he_leaves()
 {
    let vocab = current_catalogue().1;
    let nightmare = vocab.resolve_label("Nightmare").unwrap();
    let elf = vocab.resolve_label("Elf").unwrap();
    let mut elf_card = Program::creature(fixed_cost("{G}"), 2.into(), 3.into(), [elf].into());
    elf_card.printed_keywords.insert(Flying);
    let born = Program::creature(None, 1.into(), 2.into(), [nightmare].into());
    let cards = vec![
        named("Chainer, Dementia Master", 1, A, ObjectDestination::Hand),
        seed(2, A, ObjectDestination::Graveyard, elf_card.clone()),
        seed(3, B, ObjectDestination::Graveyard, elf_card),
        seed(4, B, ObjectDestination::Battlefield { controller: B }, born),
        seed(
            5,
            A,
            ObjectDestination::Graveyard,
            Program::artifact(None, vec![]),
        ),
        creature(6, B, 1, 1, &[]),
        remove_spell(7, A),
    ];
    let start = game(
        vec![A, B],
        cards,
        &[(Colourless, 3), (Black, 5)],
        Step::PrecombatMain,
    );
    let entered =
        round(cast_untargeted(&start, 1, &[(Colourless, 3), (Black, 2)]).into_state()).into_state();
    assert_size(&entered, physical(&entered, 4).id(), 2, 3);
    let bring = |n| GameCommand::ActivateAbility {
        source: physical(&entered, 1).id(),
        ability: ActivatedAbilityId(0),
        target: TargetRef::Object(physical(&entered, n).id()),
        payment: Some(spend(&[(Black, 3)])),
    };
    illegal(&entered, A, bring(5));
    for (n, owner) in [(2, A), (3, B)] {
        let paid = command(&entered, A, bring(n)).into_state();
        assert_eq!(paid.execution().life_total(A), Some(&17.into()));
        assert_eq!(paid.execution().mana(A, Black), 0.into());
        let body = round(paid).into_state();
        let id = physical(&body, n).id();
        let ret = sheet(&body, id);
        assert_eq!(physical(&body, n).controller(), Some(A));
        assert_eq!(physical(&body, n).owner(), owner);
        assert_eq!(ret.current_colors, [Color::Black].into());
        assert_eq!(ret.current_creature_subtypes, [elf, nightmare].into());
        assert!(ret.has_keyword(Flying));
        assert_size(&body, id, 3, 4);
        let left =
            round(targeted(&body, A, 7, TargetRef::Object(physical(&body, 1).id())).into_state())
                .into_state();
        assert_size(&left, id, 2, 3);
        let done = round(left).into_state();
        for removed in [n, 4] {
            assert_eq!(physical(&done, removed).zone(), ObjectZone::Exile);
        }
        assert_eq!(physical(&done, 6).zone(), ObjectZone::Battlefield);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_priest_of_the_blood_rite_creates_a_black_flying_demon_and_only_its_controller_loses_life_on_their_upkeep()
 {
    let cost = [(Colourless, 3), (Black, 2)];
    let mut cards = draw_pile();
    cards.push(named(
        "Priest of the Blood Rite",
        1,
        A,
        ObjectDestination::Hand,
    ));
    let start = game(vec![A, B], cards, &cost, Step::PrecombatMain);
    let field = round(cast_untargeted(&start, 1, &cost).into_state()).into_state();
    let (made, children) = ordered_tokens(field, 1);
    let id = children[0];
    let demon = sheet(&made, id);
    assert!(demon.has_keyword(Flying));
    assert_eq!(demon.current_colors, [Color::Black].into());
    assert_eq!(
        demon.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Demon").unwrap()].into()
    );
    assert_size(&made, id, 5, 5);
    let their = exile_hand::until(made, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert_eq!(their.execution().life_total(A), Some(&20.into()));
    let own = exile_hand::until(their, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert_eq!(own.execution().life_total(A), Some(&18.into()));
    assert_eq!(own.execution().life_total(B), Some(&20.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_bloodchief_ascension_counts_each_end_step_and_at_three_quests_offers_a_linked_loss_and_gain_for_opponents_graveyard_cards()
 {
    let damage = ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SinglePlayer,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(2),
                    false,
                ),
            ],
        ),
    };
    let mut cards = draw_pile();
    cards.extend([
        named("Bloodchief Ascension", 1, A, ObjectDestination::Hand),
        seed(
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::artifact(None, vec![damage]),
        ),
        remove_spell(3, A),
        creature(4, B, 1, 1, &[]),
        seed(
            5,
            B,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ),
        ),
    ]);
    let start = game(vec![A, B], cards, &[(Black, 1)], Step::PrecombatMain);
    let field = round(cast_untargeted(&start, 1, &[(Black, 1)]).into_state()).into_state();
    let source = physical(&field, 1).id();
    let before =
        round(targeted(&field, A, 3, TargetRef::Object(physical(&field, 4).id())).into_state())
            .into_state();
    assert!(before.execution().window().entries().is_empty());
    let none = exile_hand::until(field.clone(), |s| {
        s.turn().unwrap().position().step == Step::End
    });
    assert!(none.execution().window().entries().is_empty());
    let mut state = field;
    for turn in 1..=3 {
        let start = exile_hand::until(state, |s| {
            s.turn().unwrap().position().number == turn
                && s.turn().unwrap().position().step == Step::PrecombatMain
        });
        let ready = if turn == 2 {
            pass(&start).into_state()
        } else {
            start
        };
        let hurt = round(
            command(
                &ready,
                A,
                GameCommand::ActivateAbility {
                    source: physical(&ready, 2).id(),
                    ability: ActivatedAbilityId(0),
                    target: TargetRef::Player(B),
                    payment: None,
                },
            )
            .into_state(),
        )
        .into_state();
        let end = exile_hand::until(hurt, |s| s.turn().unwrap().position().step == Step::End);
        let offer = round(end).into_state();
        let skipped = reply(
            &offer,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        )
        .into_state();
        assert_eq!(
            skipped
                .execution()
                .counters_of_kind(source, CounterKind::Quest),
            ((turn - 1) as i64).into()
        );
        state = reply(
            &offer,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        )
        .into_state();
        assert_eq!(
            state
                .execution()
                .counters_of_kind(source, CounterKind::Quest),
            (turn as i64).into()
        );
    }
    let died =
        round(targeted(&state, A, 3, TargetRef::Object(physical(&state, 4).id())).into_state())
            .into_state();
    let other = pass(&state).into_state();
    let spell = round(
        command(
            &other,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&other, 5).id(),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state();
    for (state, b_before) in [(died, 14), (spell, 15)] {
        assert_eq!(state.execution().window().entries().len(), 1);
        let ask = round(state).into_state();
        for (choice, change) in [
            (OptionalEffectChoice::Take, 2),
            (OptionalEffectChoice::Decline, 0),
        ] {
            let done = reply(&ask, DecisionReply::OptionalEffect(choice)).into_state();
            assert_eq!(
                done.execution().life_total(B),
                Some(&(b_before - change).into())
            );
            assert_eq!(done.execution().life_total(A), Some(&(20 + change).into()));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_the_scarab_god_exiles_a_creature_from_either_graveyard_copies_it_as_a_black_zombie_counts_only_allied_zombies_and_returns_after_dying()
 {
    use spellwake_engine::effects::LibraryTopReply;
    let vocab = current_catalogue().1;
    let zombie = vocab.resolve_label("Zombie").unwrap();
    let elf = vocab.resolve_label("Elf").unwrap();
    let corpse = |artifact| {
        let mut body = Program::creature(fixed_cost("{G}"), 2.into(), 3.into(), [elf].into());
        body.printed_keywords.insert(Flying);
        if artifact {
            body.card_types.insert(CardType::Artifact);
        }
        body
    };
    let undead = |n, owner| {
        seed(
            n,
            owner,
            ObjectDestination::Battlefield { controller: owner },
            Program::creature(None, 1.into(), 1.into(), [zombie].into()),
        )
    };
    let mut cards = draw_pile();
    cards.extend([
        named("The Scarab God", 1, A, ObjectDestination::Hand),
        seed(2, A, ObjectDestination::Graveyard, corpse(false)),
        seed(3, B, ObjectDestination::Graveyard, corpse(true)),
        undead(4, A),
        undead(5, B),
        creature(6, A, 2, 2, &[]),
        seed(
            7,
            A,
            ObjectDestination::Graveyard,
            Program::artifact(None, vec![]),
        ),
        remove_spell(8, A),
    ]);
    let price = [(Colourless, 3), (Blue, 1), (Black, 1)];
    let start = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 5), (Blue, 2), (Black, 2)],
        Step::PrecombatMain,
    );
    let field = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    let raise = |n| GameCommand::ActivateAbility {
        source: physical(&field, 1).id(),
        ability: ActivatedAbilityId(0),
        target: TargetRef::Object(physical(&field, n).id()),
        payment: Some(spend(&[(Colourless, 2), (Blue, 1), (Black, 1)])),
    };
    illegal(&field, A, raise(7));
    for (n, artifact) in [(2, false), (3, true)] {
        let (made, ids) = ordered_tokens(command(&field, A, raise(n)).into_state(), 1);
        let id = ids[0];
        let card = sheet(&made, id);
        assert_eq!(physical(&made, n).zone(), ObjectZone::Exile);
        assert_eq!(card.current_colors, [Color::Black].into());
        assert_eq!(card.current_creature_subtypes, [zombie].into());
        assert_eq!(card.has_type(CardType::Artifact), artifact);
        assert!(card.has_keyword(Flying));
        assert_size(&made, id, 4, 4);
        let turn = exile_hand::until(made, |s| {
            s.turn().unwrap().position().number == 4
                && matches!(
                    s.pending_decision().map(|p| p.frame()),
                    Some(RuleFrame::LibraryTop(_))
                )
        });
        for seat in [B, C] {
            assert_eq!(turn.execution().life_total(seat), Some(&18.into()));
        }
        assert_eq!(turn.execution().life_total(A), Some(&20.into()));
        let RuleFrame::LibraryTop(frame) = turn.pending_decision().unwrap().frame() else {
            unreachable!()
        };
        let visible = frame.domain_top_first();
        assert_eq!(visible.len(), 2);
        let arranged = reply(
            &turn,
            DecisionReply::LibraryTop(LibraryTopReply::Scry {
                top_first: vec![visible[1]],
                bottom_top_first: vec![visible[0]],
            }),
        )
        .into_state();
        assert_eq!(
            arranged.execution().window().cards().library(A).first(),
            Some(&visible[1])
        );
        assert_eq!(
            arranged.execution().window().cards().library(A).last(),
            Some(&visible[0])
        );
    }
    let dead =
        round(targeted(&field, A, 8, TargetRef::Object(physical(&field, 1).id())).into_state())
            .into_state();
    assert_eq!(physical(&dead, 1).zone(), ObjectZone::Graveyard(A));
    let delayed = round(dead).into_state();
    let end = exile_hand::until(delayed, |s| s.turn().unwrap().position().step == Step::End);
    assert_eq!(physical(&end, 1).zone(), ObjectZone::Graveyard(A));
    let hand = round(end).into_state();
    assert_eq!(physical(&hand, 1).zone(), ObjectZone::Hand(A));
}
