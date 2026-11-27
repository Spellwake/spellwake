//! Timing, independence, retargeting and additional payments on real stack entries.
use super::*;
use spellwake_engine::{
    effects::CopyTargetChoice,
    life::LifeAdjustment,
    mana::CastingPayment,
    program::{AdditionalCastCost, CastCopyKeyword, TriggeredAbility, TriggeredAbilityId},
};
fn fixture(kind: CastCopyKeyword) -> GameState {
    let free = ManaCost::free();
    let mut source = spell(
        1,
        A,
        spellwake_engine::program::Instruction::AdjustLife(Box::new(
            spellwake_engine::toolbox::AdjustLifeSpec::lose(
                spellwake_engine::toolbox::PlayerSelector::Resolving(
                    spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                ),
                spellwake_engine::program::ValueExpr::Literal(2.into()),
            ),
        )),
        free.clone(),
        BodyTarget::SINGLE_PLAYER,
    );
    source
        .program
        .triggered
        .push(TriggeredAbility::spell_copies(TriggeredAbilityId(4), kind));
    if let CastCopyKeyword::Replicate(_) = kind {
        source
            .program
            .additional_cast_costs
            .push(AdditionalCastCost::Replicate(ManaCost::one(ManaType::Blue)));
    }
    let mut cards = vec![
        source,
        creature(
            2,
            B,
            ObjectDestination::Battlefield { controller: B },
            None,
            6,
        ),
    ];
    for (n, seat) in [(3, A), (4, B)] {
        cards.push(spell(
            n,
            seat,
            spellwake_engine::program::Instruction::AdjustLife(Box::new(
                spellwake_engine::toolbox::AdjustLifeSpec::gain(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
            )),
            free.clone(),
            BodyTarget::None,
        ));
    }
    cards.push(spell(
        5,
        B,
        Instruction::StackOp(Box::new(
            spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                spellwake_engine::toolbox::CounterDestination::Graveyard,
            ),
        )),
        free.clone(),
        BodyTarget::single_spell(spellwake_engine::program::SpellFilter::any()),
    ));
    cards.push(spell(
        6,
        B,
        Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(
            spellwake_engine::toolbox::ObjectQuery::Targets,
            spellwake_engine::toolbox::ZoneDest::Graveyard {
                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                    allow_regenerate: true,
                },
            },
        ))),
        free,
        BodyTarget::SINGLE_CREATURE,
    ));
    initialize(
        Some(Step::PostcombatMain),
        cards,
        vec![FloatingMana {
            player: A,
            mana_type: ManaType::Blue,
            amount: 2.into(),
        }],
    )
    .into_state()
}
fn answer(g: &GameState, choice: CopyTargetChoice) -> GameState {
    let p = g.pending_decision().unwrap();
    assert!(matches!(p.frame(), RuleFrame::CopyTargets(_)));
    accept(
        g,
        p.chooser(),
        GameCommand::Reply {
            decision: p.id(),
            choice: DecisionReply::CopyTargets(choice),
        },
    )
    .into_state()
}
fn copies(g: &GameState) -> usize {
    g.execution()
        .window()
        .entries()
        .iter()
        .filter(|s| matches!(s.kind(), StackKind::SpellCopy { .. }))
        .count()
}
#[test]
fn storm_uses_cast_time_count_with_both_players_and_last_known_spell() {
    let mut g = fixture(CastCopyKeyword::Storm);
    g = cast(&g, A, 3, None).into_state();
    g = pass(&g).into_state();
    g = cast(&g, B, 4, None).into_state();
    g = resolve(&g).into_state();
    g = resolve(&g).into_state();
    g = targeted(&g, A, 1, TargetRef::Player(B)).into_state();
    let original = g.execution().window().entries()[0].id();
    g = pass(&g).into_state();
    g = targeted(&g, B, 5, TargetRef::Stack(original)).into_state();
    g = resolve(&g).into_state();
    assert_eq!(
        g.execution()
            .window()
            .cards()
            .get(object(&g, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    g = resolve(&g).into_state();
    let p = g.pending_decision().unwrap();
    assert!(
        g.advance(input(
            &g,
            A,
            GameCommand::Reply {
                decision: p.id(),
                choice: DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Object(
                    object(&g, 2)
                )))
            }
        ))
        .is_err()
    );
    g = answer(&g, CopyTargetChoice::Retain);
    assert_eq!(copies(&g), 0);
    g = answer(&g, CopyTargetChoice::Change(TargetRef::Player(A)));
    assert_eq!(copies(&g), 2);
    let entries = g.execution().window().entries();
    assert_eq!(
        entries[0].targets().targets()[0].choice().target,
        TargetRef::Player(B)
    );
    assert_eq!(
        entries[1].targets().targets()[0].choice().target,
        TargetRef::Player(A)
    );
    assert_eq!(g.execution().cast_turn_history().unwrap().spells, 4.into());
    g = resolve(&g).into_state();
    g = resolve(&g).into_state();
    assert_eq!(g.execution().life_total(A), Some(&19.into()));
    assert_eq!(g.execution().life_total(B), Some(&19.into()));
}
#[test]
fn gravestorm_counts_a_permanent_dying_after_cast_before_resolution() {
    let mut g = fixture(CastCopyKeyword::Gravestorm);
    g = targeted(&g, A, 1, TargetRef::Player(B)).into_state();
    assert_eq!(
        g.execution().cast_turn_history().unwrap().grave_permanents,
        0.into()
    );
    g = pass(&g).into_state();
    g = targeted(&g, B, 6, TargetRef::Object(object(&g, 2))).into_state();
    g = resolve(&g).into_state();
    assert_eq!(
        g.execution().cast_turn_history().unwrap().grave_permanents,
        1.into()
    );
    g = resolve(&g).into_state();
    g = answer(&g, CopyTargetChoice::Retain);
    assert_eq!(copies(&g), 1);
    g = resolve(&g).into_state();
    g = resolve(&g).into_state();
    assert_eq!(g.execution().life_total(B), Some(&16.into()));
}
#[test]
fn replicate_only_triggers_for_paid_cost_and_copies_do_not_pay_again() {
    let initial = fixture(CastCopyKeyword::Replicate(0));
    let free = targeted(&initial, A, 1, TargetRef::Player(B)).into_state();
    assert_eq!(free.execution().window().entries().len(), 1);
    let pay = CastingPayment {
        additional: [(0, 2.into())].into(),
        mana: Some(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Blue,
                amount: 2.into(),
            }],
        }),
        ..Default::default()
    };
    let mut g = accept(
        &initial,
        A,
        GameCommand::CastWithResources {
            spell: object(&initial, 1),
            target: Some(TargetRef::Player(B)),
            payment: pay,
        },
    )
    .into_state();
    let original = &g.execution().window().entries()[0];
    assert_eq!(original.replicate_choices().get(&0), Some(&2.into()));
    assert!(original.kicker_choices().is_empty());
    assert_eq!(g.execution().mana(A, ManaType::Blue), 0.into());
    g = resolve(&g).into_state();
    g = answer(&g, CopyTargetChoice::Retain);
    g = answer(&g, CopyTargetChoice::Retain);
    assert_eq!(copies(&g), 2);
    assert!(
        g.execution()
            .window()
            .entries()
            .iter()
            .all(|e| e.replicate_choices().get(&0) == Some(&2.into()))
    );
    for _ in 0..3 {
        g = resolve(&g).into_state();
    }
    assert_eq!(g.execution().life_total(B), Some(&14.into()));
    assert_eq!(g.execution().cast_turn_history().unwrap().spells, 1.into());
}
