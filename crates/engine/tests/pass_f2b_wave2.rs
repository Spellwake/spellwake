#![allow(dead_code, unused_imports)]
//! Runtime integration tests for Pass F2-B Wave 2 primitives:
//! - CR 701.60 `CounterKind::Suspect` (Menace + CannotBlock)
//! - `StatAggregate::Min`
//! - `Instruction::CounterSpellToLibrary { top }`
//! - `StaticRestriction` (`ControllerHasShroud`, `ControllerCantLoseOpponentsCantWin`,
//!   `ControllerCantLoseForZeroLife`, `ControllerCantWinOpponentsCantLose`,
//!   `CantPlayLands`, `PreventCounters` on permanents and players)
//! - `StaticDamageRule::PreventIncomingRemovePlusOneIfCounter` and
//!   `StaticDamageOp::{PreventAtMost, PreventHalf}`
//! - `StaticModifierRule::{BonusTokenOnCreate, OthersEnterWithCountersExpr}`
//! - `StaticSourceCondition::EntryOptionChosen`, `TriggerCondition::SourceEntryOption`,
//!   and `MatchObjectRef::LastMovedToGraveyard`.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    characteristics::{
        BlockingRestriction, ControllerSelector, KeywordAbility, NumericExpression, NumericPair,
        PermanentSelector, StaticAbilityId, StaticContinuousAbility, StaticDamageKind,
        StaticDamageOp, StaticDamageRecipient, StaticDamageRule, StaticModifierRule, StaticPart,
        StaticParts, StaticRestriction, StaticSourceCondition,
    },
    effects::{CounterKind, SeedCard},
    game::{DecisionReply, GameCommand, GameInput, GameState, NativeCandidate, RuleFrame},
    land::{EntryChoice, EntryTap},
    life::LifeAdjustment,
    mana::{ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    program::{
        AffectedPlayers, BodyTarget, CardType, Comparison, DamageAmountSpec, DamageRecipientsRef,
        DamageSourceRef, EffectSubject, EntryCounterAbility, ExecutableBody, Instruction,
        MatchObjectRef, ObjectFilter, ObjectStatKind, PermanentDefinition, PlayerRelation, Program,
        ProgramKind, ResolutionCondition, ResolvingPlayer, SpellFilter, StatAggregate,
        TriggerCondition, TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
        ValueExpr, ValueZone,
    },
    targets::TargetRef,
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients, TokenSubtype},
};
use std::collections::BTreeSet;

fn cast_untargeted(game: &GameState, id: u64) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(game, id),
            payment: Some(ManaPayment::default()),
        },
    );
    pass_pair(cast.state())
}

fn cast_targeted(
    game: &GameState,
    actor: spellwake_engine::priority::SeatId,
    id: u64,
    target: TargetRef,
) -> NativeCandidate {
    let cast = submit(
        game,
        actor,
        GameCommand::CastPaid {
            spell: object(game, id),
            target,
            payment: ManaPayment::default(),
        },
    );
    let order = if actor == P0 { [P0, P1] } else { [P1, P0] };
    pass_in_order(cast.state(), order)
}

#[test]
fn bonus_token_on_create_and_others_enter_with_counters_expr() {
    let vocab = vocabulary();
    let squirrel = vocab.resolve_label("Squirrel").unwrap();
    let saproling = vocab.resolve_label("Saproling").unwrap();

    let squirrel_tok = TokenDefinition::new(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::from([squirrel])),
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(squirrel)],
        },
        &vocab,
    )
    .unwrap();
    let saproling_tok = TokenDefinition::new(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::from([saproling])),
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(saproling)],
        },
        &vocab,
    )
    .unwrap();

    // Chatterfang style (per_created: true) + Peregrin style (per_created: false).
    let mut chatterfang = Program::creature(None, 3.into(), 3.into(), BTreeSet::new());
    chatterfang.continuous = Some(StaticContinuousAbility::Modifier {
        id: StaticAbilityId(1),
        rule: StaticModifierRule::BonusTokenOnCreate {
            controller: PlayerRelation::Controller,
            token: Box::new(squirrel_tok.clone()),
            count: 1,
            per_created: true,
        },
    });
    let mut peregrin = Program::creature(None, 2.into(), 3.into(), BTreeSet::new());
    peregrin.continuous = Some(StaticContinuousAbility::Modifier {
        id: StaticAbilityId(2),
        rule: StaticModifierRule::BonusTokenOnCreate {
            controller: PlayerRelation::Controller,
            token: Box::new(squirrel_tok),
            count: 1,
            per_created: false,
        },
    });
    // OthersEnterWithCountersExpr: other creatures enter with +1/+1 counters equal to creatures P0 controls.
    let mut mentor = Program::creature(None, 1.into(), 1.into(), BTreeSet::new());
    mentor.continuous = Some(StaticContinuousAbility::Modifier {
        id: StaticAbilityId(3),
        rule: StaticModifierRule::OthersEnterWithCountersExpr {
            selector: Box::new(PermanentSelector {
                controller: ControllerSelector::SourceController,
                required_types: BTreeSet::from([CardType::Creature]),
                exclude_source: true,
                ..PermanentSelector::default()
            }),
            kind: CounterKind::PlusOne,
            amount: Box::new(ValueExpr::Count {
                zone: ValueZone::Battlefield,
                filter: Box::new(ObjectFilter::creature().controlled()),
                controller_scope: None,
            }),
        },
    });

    let game = world(
        vocab,
        vec![
            card(1, chatterfang, true),
            card(2, peregrin, true),
            spell(
                3,
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation {
                        enters_tapped: false,
                        definition: saproling_tok,
                        amount: ValueExpr::literal(2),
                        base_size: None,
                        recipients: TokenRecipients::Controller,
                    }],
                }],
            ),
            card(4, mentor, true),
            card(
                5,
                Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new()),
                false,
            ),
        ],
        vec![CardId(1), CardId(2), CardId(4)],
    );

    // Creating 2 Saprolings with Chatterfang (+2 Squirrels) and Peregrin (+1 Squirrel) => 5 tokens!
    let mut after_tokens = cast_untargeted(&game, 3);
    if let Some(RuleFrame::TimestampOrder(order)) =
        after_tokens.state().pending_decision().map(|d| d.frame())
    {
        let members = order.members().iter().map(|m| m.member).collect();
        after_tokens = reply(
            after_tokens.state(),
            P0,
            DecisionReply::TimestampOrder(members),
        );
    }
    let token_count = after_tokens
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|o| o.zone() == ObjectZone::Battlefield && o.card_id().is_none())
        .count();
    assert_eq!(token_count, 5);

    // Cast bear (card 5): P0 currently controls 3 nontoken + 5 token = 8 creatures, so mentor gives 8 +1/+1 counters -> 10/10!
    let after_bear = cast_untargeted(after_tokens.state(), 5);
    let bear_pt = current(after_bear.state(), object(after_bear.state(), 5))
        .power_toughness
        .unwrap();
    assert_eq!(bear_pt.derived.power, 10.into());
    assert_eq!(bear_pt.derived.toughness, 10.into());
}
