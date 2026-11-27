//! A discarded card gets a separate, counterable cast opportunity in exile.
use super::*;
use spellwake_engine::{
    effects::{
        ExileCastAbility, OptionalEffectChoice, ZoneReplacementReply, ZoneReplacementRequest,
    },
    mana::{CastManaActivation, CastingPayment, ManaAbilityRef},
    program::{AffectedPlayers, ResolvingPlayer},
};
fn setup_discard(target: bool, competing: bool) -> GameState {
    let mut cards = seeds();
    cards.retain(|c| matches!(c.card.0, 9 | 10 | 13) || (competing && c.card.0 == 90));
    let origin = cards.iter_mut().find(|c| c.card == CardId(13)).unwrap();
    origin.program = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::discard(
                    AffectedPlayers::One(ResolvingPlayer::Controller),
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
            ))],
        ),
    );
    let source = cards.iter_mut().find(|c| c.card == CardId(10)).unwrap();
    if !target {
        source.program = Program::creature(
            Some(ManaCost::generic(8.into())),
            2.into(),
            3.into(),
            BTreeSet::new(),
        );
    }
    source.program.madness = Some(ManaCost::one(ManaType::Green));
    if competing {
        use spellwake_engine::program::*;
        let other = cards.iter_mut().find(|c| c.card == CardId(90)).unwrap();
        other
            .program
            .zone_replacements
            .push(ZoneReplacementAbility {
                id: ZoneReplacementAbilityId(7),
                actions: ZoneActionPredicate::OneOf([ZoneActionKind::EffectDiscard].into()),
                objects: ZoneObjectKind::Card,
                subject: ZoneReplacementSubject::AnyObject,
                destination: PublicZoneDestination::Graveyard,
                replace_with: PublicZoneDestination::Exile,
                choice: ZoneReplacementChoice::Mandatory,
            });
    }
    initialize(Some(Step::PostcombatMain), cards, vec![]).into_state()
}
fn answer(g: &GameState, choice: DecisionReply) -> GameState {
    let p = g.pending_decision().unwrap();
    accept(
        g,
        p.chooser(),
        GameCommand::Reply {
            decision: p.id(),
            choice,
        },
    )
    .into_state()
}
fn discard(g: &GameState) -> GameState {
    let casted = cast(g, A, 13, None);
    let start = resolve(casted.state()).into_state();
    if let Some(p) = start.pending_decision()
        && matches!(p.frame(), RuleFrame::ResolutionSelect(_))
    {
        let id = object(&start, 10);
        answer(&start, DecisionReply::SelectResolution(vec![id]))
    } else {
        start
    }
}
#[test]
fn madness_cast_can_tap_a_color_choice_land_during_payment_and_rejects_a_bad_payment_atomically() {
    let g = setup_discard(false, false);
    let exiled = discard(&g);
    assert_eq!(physical(&exiled, 10).zone(), ObjectZone::Exile);
    assert_eq!(exiled.execution().window().entries().len(), 1);
    let choice = resolve(&exiled).into_state();
    let p = choice.pending_decision().unwrap();
    let RuleFrame::OptionalEffect(frame) = p.frame() else {
        panic!("madness cast offer")
    };
    let offer = frame.exile_cast().unwrap();
    assert_eq!(offer.ability, ExileCastAbility::Madness);
    assert_eq!(offer.madness_price, Some(ManaCost::one(ManaType::Green)));
    let pay = |color| DecisionReply::MadnessCast {
        target: None,
        payment: CastingPayment {
            mana: Some(payment(&[(ManaType::Green, 1)])),
            ..Default::default()
        },
        mana_abilities: vec![CastManaActivation {
            recipient: None,
            source: object(&choice, 9),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
            payment: None,
            bundle: None,
            color: Some(color),
            cost_objects: Vec::new(),
            restricted_mana: Vec::new(),
        }],
    };
    let wrong = GameCommand::Reply {
        decision: p.id(),
        choice: pay(ManaType::Blue),
    };
    assert!(choice.advance(input(&choice, A, wrong)).is_err());
    assert_eq!(physical(&choice, 9).tapped(), Some(false));
    let casted = answer(&choice, pay(ManaType::Green));
    assert_eq!(physical(&casted, 10).zone(), ObjectZone::Stack);
    assert_eq!(physical(&casted, 9).tapped(), Some(true));
    assert_eq!(casted.execution().mana(A, ManaType::Green), 0.into());
    assert_eq!(
        physical(&resolve(&casted).into_state(), 10).zone(),
        ObjectZone::Battlefield
    );
    let declined = answer(
        &choice,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(physical(&declined, 10).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&declined, 9).tapped(), Some(false));
}
#[test]
fn madness_when_no_target_exists_moves_to_graveyard_as_the_trigger_resolves() {
    let g = setup_discard(true, false);
    let exiled = discard(&g);
    assert_eq!(physical(&exiled, 10).zone(), ObjectZone::Exile);
    let done = resolve(&exiled).into_state();
    assert_eq!(physical(&done, 10).zone(), ObjectZone::Graveyard(A));
    assert!(done.pending_decision().is_none());
}
#[test]
fn only_the_madness_replacement_can_create_its_cast_trigger() {
    let g = setup_discard(false, true);
    let choice = discard(&g);
    let RuleFrame::ZoneReplacement(frame) = choice.pending_decision().unwrap().frame() else {
        panic!("two applicable discard replacements")
    };
    let ZoneReplacementRequest::Select { applications } = frame.request() else {
        panic!("select effect")
    };
    assert_eq!(applications.len(), 2);
    for &op in applications {
        let outcome = answer(
            &choice,
            DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(op)),
        );
        assert_eq!(physical(&outcome, 10).zone(), ObjectZone::Exile);
        assert_eq!(
            outcome.execution().window().entries().len(),
            usize::from(op.effect.madness)
        );
    }
}

#[test]
fn cycling_a_madness_card_preserves_the_cycling_discard_and_stacks_both_abilities() {
    use spellwake_engine::occurrence::{RulesOccurrence, RulesRecord, ZoneReason};
    let mut cards = seeds();
    cards.retain(|c| matches!(c.card.0, 1 | 98 | 99));
    let card = cards.iter_mut().find(|c| c.card == CardId(1)).unwrap();
    card.program.madness = Some(ManaCost::free());
    let ability = ActivatedAbilityId(4);
    card.program
        .activated
        .push(ActivatedAbility::cycling(ability, ManaCost::free()));
    let game = setup(Some(Step::PrecombatMain), cards);
    let cycled = accept(
        &game,
        A,
        GameCommand::Cycle {
            source: object(&game, 1),
            ability,
            payment: None,
        },
    );
    assert!(cycled.rules_journal().records.iter().any(|row|matches!(row,RulesRecord::Event(e) if e.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::ZoneChange(c) if c.madness && c.reason==ZoneReason::CycleDiscard{player:A})))));
    let cycled = cycled.into_state();
    assert_eq!(cycled.execution().window().entries().len(), 2);
    assert_eq!(physical(&cycled, 1).zone(), ObjectZone::Exile);
    let choice = resolve(&cycled).into_state();
    let declined = answer(
        &choice,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(physical(&declined, 1).zone(), ObjectZone::Graveyard(A));
    let drawn = resolve(&declined).into_state();
    assert_eq!(physical(&drawn, 98).zone(), ObjectZone::Hand(A));
}

#[test]
fn cleanup_discard_can_create_a_madness_trigger_before_the_extra_cleanup() {
    use spellwake_engine::checks::CheckDecision;
    let mut cards = Vec::new();
    for n in 1..=8 {
        cards.push(creature(
            n,
            A,
            ObjectDestination::Hand,
            Some(ManaCost::free()),
            1,
        ));
    }
    cards[0].program.madness = Some(ManaCost::free());
    let game = initialize(Some(Step::End), cards, vec![]).into_state();
    let end = resolve(&game).into_state();
    let p = end.pending_decision().expect("cleanup hand-size decision");
    assert!(matches!(
        p.frame(),
        RuleFrame::Checks(CheckDecision::CleanupSelect(_))
    ));
    let discarded = answer(&end, DecisionReply::Discard(vec![object(&end, 1)]));
    assert_eq!(physical(&discarded, 1).zone(), ObjectZone::Exile);
    let offer = resolve(&discarded).into_state();
    let casted = answer(
        &offer,
        DecisionReply::MadnessCast {
            target: None,
            payment: CastingPayment::default(),
            mana_abilities: vec![],
        },
    );
    assert_eq!(physical(&casted, 1).zone(), ObjectZone::Stack);
    assert_eq!(
        physical(&resolve(&casted).into_state(), 1).zone(),
        ObjectZone::Battlefield
    );
}
