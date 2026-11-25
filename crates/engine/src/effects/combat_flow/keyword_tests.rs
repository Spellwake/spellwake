//! Attack/block restrictions through the turn action, pending frame, and reply.
use super::*;
use crate::{
    characteristics::Color,
    land::LandType,
    program::{CardType, Supertype},
};

fn combat_creature(
    id: u64,
    seat: SeatId,
    keywords: &[KeywordAbility],
    power: i64,
    colors: &[Color],
    artifact: bool,
) -> SeedCard {
    let mut card = creature(id, seat);
    card.program = Program::creature(None, power.into(), 10.into(), BTreeSet::new())
        .with_printed_keywords(keywords.iter().copied());
    if !colors.is_empty() {
        card.program.color_indicator = Some(colors.iter().copied().collect());
    }
    if artifact {
        card.program.card_types.insert(CardType::Artifact);
    }
    card
}

fn defender_step(cards: Vec<SeedCard>, attacks: &[u64]) -> (EffectWindow, TurnScheduler) {
    let (mut state, turn) = prepared(cards, Step::DeclareBlockers);
    let attackers = attacks
        .iter()
        .map(|id| {
            let object = state.window.cards().get(object(&state, *id)).unwrap();
            (Combatant::from_object(object).unwrap(), B)
        })
        .collect();
    state.combat = Some(CombatState::new(attackers).unwrap());
    (state, turn)
}

fn blocker_frame(state: &EffectWindow, turn: &TurnScheduler) -> (EffectWindow, BlockersDecision) {
    let (pending, result, _) = state
        .begin_declare_blockers(state.revision(), turn)
        .unwrap()
        .into_parts();
    let RuleResult::Pending {
        frame: EffectDecision::Combat(CombatDecision::Blockers(frame)),
        ..
    } = result
    else {
        panic!("expected a blocker decision")
    };
    (pending, frame)
}

#[test]
fn evasion_is_specific_to_each_attacker_blocker_pair() {
    use KeywordAbility::*;
    let cards = vec![
        combat_creature(1, A, &[Flying], 2, &[], false),
        combat_creature(2, A, &[Shadow], 2, &[], false),
        combat_creature(3, A, &[Horsemanship], 2, &[], false),
        combat_creature(4, A, &[Fear], 2, &[], false),
        combat_creature(5, A, &[Intimidate], 2, &[Color::Red, Color::Green], false),
        combat_creature(6, A, &[Skulk], 2, &[], false),
        combat_creature(7, A, &[Flying, Shadow], 2, &[], false),
        combat_creature(8, A, &[Intimidate], 2, &[], false),
        combat_creature(10, B, &[Reach], 2, &[Color::Black], false),
        combat_creature(11, B, &[Flying, Shadow], 2, &[], false),
        combat_creature(12, B, &[Horsemanship], 3, &[Color::Green], false),
        combat_creature(13, B, &[], 1, &[], true),
        combat_creature(14, B, &[Flying], 2, &[], false),
    ];
    let (state, turn) = defender_step(cards, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let (pending, frame) = blocker_frame(&state, &turn);
    for (blocker, targets) in [
        (10, vec![1, 4, 6]),
        (11, vec![2, 7]),
        (12, vec![3, 5]),
        (13, vec![4, 5, 6, 8]),
        (14, vec![1, 6]),
    ] {
        assert_eq!(
            frame.legal_attackers(object(&pending, blocker)),
            targets
                .iter()
                .map(|id| object(&pending, *id))
                .collect::<Vec<_>>(),
            "blocker {blocker}"
        );
    }
    let result = pending.resume_combat(
        pending.revision(),
        &CombatDecision::Blockers(frame),
        B,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker: object(&pending, 12),
            attacker: object(&pending, 1),
        }]),
        &turn,
    );
    assert!(matches!(
        result,
        Err(EffectError::Combat(CombatError::IllegalBlockTarget))
    ));
}

#[test]
fn menace_checks_the_whole_declaration_before_committing_any_blocker() {
    use KeywordAbility::Menace;
    let (state, turn) = defender_step(
        vec![
            combat_creature(1, A, &[Menace], 2, &[], false),
            creature(2, A),
            creature(3, B),
            creature(4, B),
        ],
        &[1, 2],
    );
    let (pending, domain) = blocker_frame(&state, &turn);
    assert_eq!(
        domain.group_minimums().collect::<Vec<_>>(),
        vec![(object(&pending, 1), 2)]
    );
    let frame = CombatDecision::Blockers(domain);
    let block = |id, target| BlockDeclaration {
        blocker: object(&pending, id),
        attacker: object(&pending, target),
    };
    let submit = |rows| {
        pending.resume_combat(
            pending.revision(),
            &frame,
            B,
            DecisionReply::DeclareBlockers(rows),
            &turn,
        )
    };
    let before = pending.clone();
    assert!(matches!(
        submit(vec![block(3, 1), block(4, 2)]),
        Err(EffectError::Combat(CombatError::IllegalBlockTarget))
    ));
    assert_eq!(pending, before);
    for rows in [vec![], vec![block(3, 1), block(4, 1)], vec![block(4, 2)]] {
        assert!(matches!(
            submit(rows).unwrap().into_parts().1,
            RuleResult::Complete { .. }
        ));
    }
}

#[test]
fn lone_reachable_blocker_cannot_offer_a_menace_block() {
    use KeywordAbility::{Flying, Menace, Reach};
    let (state, turn) = defender_step(
        vec![
            combat_creature(1, A, &[Menace, Flying], 2, &[], false),
            combat_creature(2, B, &[Reach], 2, &[], false),
            creature(3, B),
        ],
        &[1],
    );
    let (_, result, _) = state
        .begin_declare_blockers(state.revision(), &turn)
        .unwrap()
        .into_parts();
    assert!(
        matches!(result, RuleResult::Complete { declarations, .. } if matches!(&declarations[..], [CombatDeclarationRecord::Blockers { player: B,origin: DeclarationOrigin::ForcedEmpty, blockers }] if blockers.is_empty()))
    );
}

#[test]
fn defender_cannot_attack_even_with_haste() {
    let (state, turn) = prepared(
        vec![
            combat_creature(
                1,
                A,
                &[KeywordAbility::Defender, KeywordAbility::Haste],
                2,
                &[],
                false,
            ),
            creature(2, A),
        ],
        Step::DeclareAttackers,
    );
    let (pending, frame) = attacker_pending(&state, &turn);
    let CombatDecision::Attackers(domain) = &frame else {
        unreachable!()
    };
    assert_eq!(domain.eligible_attackers(), &[object(&pending, 2)]);
    assert!(matches!(
        pending.resume_combat(
            pending.revision(),
            &frame,
            A,
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: object(&pending, 1),
                defender: B,
                defending_planeswalker: None,
            }]),
            &turn
        ),
        Err(EffectError::Combat(CombatError::IllegalAttacker))
    ));
}

#[test]
fn landwalk_requires_a_matching_land_controlled_by_the_defending_player() {
    use KeywordAbility::*;
    for (keyword, subtype, supertype) in [
        (Plainswalk, Some(LandType::Plains), None),
        (Islandwalk, Some(LandType::Island), None),
        (Swampwalk, Some(LandType::Swamp), None),
        (Mountainwalk, Some(LandType::Mountain), None),
        (Forestwalk, Some(LandType::Forest), None),
        (Desertwalk, Some(LandType::Desert), None),
        (LegendaryLandwalk, None, Some(Supertype::Legendary)),
        (NonbasicLandwalk, None, None),
        (SnowLandwalk, None, Some(Supertype::Snow)),
        (
            SnowForestwalk,
            Some(LandType::Forest),
            Some(Supertype::Snow),
        ),
        (SnowSwampwalk, Some(LandType::Swamp), Some(Supertype::Snow)),
        (ArtifactLandwalk, None, None),
    ] {
        for owner in [C, B] {
            let mut land = creature(4, owner);
            land.program = Program::land(subtype.into_iter().collect(), vec![]);
            land.program.supertypes.extend(supertype);
            if keyword == ArtifactLandwalk {
                land.program.card_types.insert(CardType::Artifact);
            }
            let mut irrelevant = creature(5, B);
            irrelevant.program = Program::land(BTreeSet::new(), vec![]);
            irrelevant.program.supertypes.insert(Supertype::Basic);
            let (state, turn) = defender_step(
                vec![
                    combat_creature(1, A, &[keyword], 2, &[], false),
                    creature(2, A),
                    creature(3, B),
                    land,
                    irrelevant,
                ],
                &[1, 2],
            );
            let (pending, frame) = blocker_frame(&state, &turn);
            let targets = frame.legal_attackers(object(&pending, 3));
            assert_eq!(
                targets.contains(&object(&pending, 1)),
                owner != B,
                "{keyword:?} vs {owner:?}"
            );
            assert!(targets.contains(&object(&pending, 2)));
        }
    }
}

#[test]
fn snow_forestwalk_requires_both_qualities_on_a_single_land() {
    let mut forest = creature(3, B);
    forest.program = Program::land(BTreeSet::from([LandType::Forest]), vec![]);
    let mut snow = creature(4, B);
    snow.program = Program::land(BTreeSet::from([LandType::Swamp]), vec![]);
    snow.program.supertypes.insert(Supertype::Snow);
    let (state, turn) = defender_step(
        vec![
            combat_creature(1, A, &[KeywordAbility::SnowForestwalk], 2, &[], false),
            creature(2, B),
            forest,
            snow,
        ],
        &[1],
    );
    let (pending, frame) = blocker_frame(&state, &turn);
    assert_eq!(
        frame.legal_attackers(object(&pending, 2)),
        &[object(&pending, 1)]
    );
}

#[test]
fn block_legality_reads_the_current_layer_six_removal_and_grant() {
    use crate::characteristics::{
        AbilityChange, AbilityRemoval, ControllerSelector, PermanentSelector, StaticAbilityId,
        StaticContinuousAbility, StaticPart, StaticParts,
    };
    use KeywordAbility::{Flying, Reach, Shadow};
    let mut lord = creature(4, B);
    lord.program = Program::artifact(None, vec![]);
    lord.program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            controller: ControllerSelector::SourceController,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange {
            suppressed_keywords: BTreeSet::new(),
            removal: Some(AbilityRemoval::All),
            granted_exalted: 0,
            granted_toxic: None,
            grants_unblockable: false,
            granted_triggers: vec![],
            granted_combat_keywords: vec![],
            granted_keywords: BTreeSet::from([Reach]),
            suppressed_activations: None,
            doesnt_untap: false,
        })]),
    });
    let (state, turn) = defender_step(
        vec![
            combat_creature(1, A, &[Flying], 2, &[], false),
            combat_creature(2, A, &[Shadow], 2, &[], false),
            combat_creature(3, B, &[Shadow], 2, &[], false),
            lord,
        ],
        &[1, 2],
    );
    let (pending, frame) = blocker_frame(&state, &turn);
    assert_eq!(
        frame.legal_attackers(object(&pending, 3)),
        &[object(&pending, 1)]
    );
}
