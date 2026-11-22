//! Authored response fixtures that send chosen sacrificed cards directly to exile.
use super::*;
use spellwake_engine::{
    game::RuleFrame,
    mana::ManaCost,
    program::{
        AffectedPlayers, PermanentTypeFilter, PublicZoneDestination, ZoneActionKind,
        ZoneActionPredicate, ZoneObjectKind, ZoneReplacementAbility, ZoneReplacementAbilityId,
        ZoneReplacementChoice, ZoneReplacementSubject,
    },
};

pub(super) fn exile_response(spell: u64, replacement: u64) -> [SeedCard; 2] {
    let mut enchantment = Program::enchantment(None);
    enchantment.zone_replacements.push(ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(0),
        actions: ZoneActionPredicate::OneOf([ZoneActionKind::EffectSacrifice].into()),
        objects: ZoneObjectKind::Card,
        subject: ZoneReplacementSubject::AnyObject,
        destination: PublicZoneDestination::Graveyard,
        replace_with: PublicZoneDestination::Exile,
        choice: ZoneReplacementChoice::Mandatory,
    });
    [
        SeedCard {
            card: CardId(replacement),
            owner: B,
            destination: ObjectDestination::Battlefield { controller: B },
            program: enchantment,
        },
        SeedCard {
            card: CardId(spell),
            owner: B,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::Sacrifice {
                        players: AffectedPlayers::EachPlayer,
                        amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                        filter: PermanentTypeFilter {
                            any_types: Default::default(),
                            required_types: BTreeSet::new(),
                            forbidden_types: BTreeSet::new(),
                        },
                    }],
                ),
            ),
        },
    ]
}

pub(super) fn resolve_exile_response(
    state: &GameState,
    spell: u64,
    sacrifices_in_order: &[(SeatId, u64)],
) -> GameState {
    let passed = advance(state, A, GameCommand::Pass);
    let response = advance(
        &passed,
        B,
        GameCommand::CastUntargeted {
            spell: object(&passed, spell),
            payment: None,
        },
    );
    let mut current = resolve_top(&response).into_state();
    for &(chooser, card) in sacrifices_in_order {
        let target = object(&current, card);
        let decision = current.pending_decision().unwrap();
        let RuleFrame::ResolutionSelect(frame) = decision.frame() else {
            panic!("expected sacrifice selection, got {decision:?}");
        };
        assert_eq!(frame.required(), 1);
        assert!(frame.domain().contains(&target));
        current = reply(
            &current,
            chooser,
            DecisionReply::SelectResolution(vec![target]),
        );
    }
    assert!(current.pending_decision().is_none());
    for &(_, card) in sacrifices_in_order {
        assert_eq!(physical(&current, card).zone(), ObjectZone::Exile);
        assert_ne!(object(&current, card), object(state, card));
    }
    current
}
