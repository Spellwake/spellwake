//! Artifact token and mana-trigger gameplay.
use super::*;
use spellwake_engine::program::ArtifactSubtype;

fn check_artifact_token(state: &GameState, token: ObjectId, subtype: ArtifactSubtype) {
    let view = state
        .execution()
        .current_characteristics(token)
        .unwrap()
        .unwrap();
    assert!(view.is_token);
    assert_eq!(view.current_types, [CardType::Artifact].into());
    assert_eq!(view.current_artifact_subtypes, [subtype].into());
    assert!(view.current_colors.is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_rapacious_dragon_makes_two_treasures_and_thraben_inspector_investigates() {
    use ManaType::*;
    let dragon_price = [(Colourless, 4), (Red, 1)];
    let dragon = game(
        vec![A, B],
        vec![hand("Rapacious Dragon")],
        &dragon_price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&dragon, &dragon_price);
    assert!(
        entered
            .execution()
            .current_characteristics(physical(&entered, 1).id())
            .unwrap()
            .unwrap()
            .has_keyword(Flying)
    );
    let ordering = round(entered).into_state();
    let RuleFrame::TimestampOrder(frame) = ordering.pending_decision().unwrap().frame() else {
        panic!("two simultaneous Treasures");
    };
    let made = reply(
        &ordering,
        DecisionReply::TimestampOrder(frame.members().iter().map(|member| member.member).collect()),
    );
    let treasure_ids = tokens(&made, A, 2);
    for token in &treasure_ids {
        check_artifact_token(made.state(), *token, ArtifactSubtype::Treasure);
    }
    let state = command(
        made.state(),
        A,
        GameCommand::ActivateMana {
            source: treasure_ids[0],
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let RuleFrame::ManaColor(frame) = state.pending_decision().unwrap().frame() else {
        panic!("one mana of any color");
    };
    assert_eq!(frame.colors(), &[White, Blue, Black, Red, Green].into());
    let funded = reply(&state, DecisionReply::ManaColor(Green)).into_state();
    assert_eq!(funded.execution().mana(A, Green), 1.into());
    assert!(
        funded
            .execution()
            .window()
            .cards()
            .get(treasure_ids[0])
            .is_none()
    );
    assert!(
        funded
            .execution()
            .window()
            .cards()
            .get(treasure_ids[1])
            .is_some()
    );

    let inspector = game(
        vec![A, B],
        vec![
            hand("Thraben Inspector"),
            named("Forest", 2, A, ObjectDestination::LibraryTop),
        ],
        &[(White, 1), (Colourless, 2)],
        Step::PrecombatMain,
    );
    let investigated = round(cast_named(&inspector, &[(White, 1)]));
    let clue = tokens(&investigated, A, 1)[0];
    check_artifact_token(investigated.state(), clue, ArtifactSubtype::Clue);
    let activate = |amount| GameCommand::ActivateUntargetedAbility {
        source: clue,
        ability: investigated
            .state()
            .execution()
            .base_program(clue)
            .unwrap()
            .activated[0]
            .id,
        payment: Some(spend(&[(Colourless, amount)])),
    };
    illegal(investigated.state(), A, activate(1));
    let used = command(investigated.state(), A, activate(2)).into_state();
    assert!(used.execution().window().cards().get(clue).is_none());
    assert_eq!(physical(&used, 2).zone(), ObjectZone::Library(A));
    let drew = round(used).into_state();
    assert_eq!(physical(&drew, 2).zone(), ObjectZone::Hand(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_treasure_nabber_takes_only_an_opponents_mana_artifact_until_the_next_own_turn_ends() {
    use ManaType::*;
    use spellwake_engine::land::BasicLandType;
    let price = [(Colourless, 2), (Red, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Treasure Nabber"),
            named(
                "Forest",
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named(
                "Sol Ring",
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named("Forest", 4, B, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let their_priority = pass(&entered).into_state();
    let tap = |state: &GameState, source, ability| GameCommand::ActivateMana {
        source: physical(state, source).id(),
        ability,
    };
    let land = command(
        &their_priority,
        B,
        tap(
            &their_priority,
            2,
            ManaAbilityRef::BasicLand(BasicLandType::Forest),
        ),
    )
    .into_state();
    assert!(land.execution().window().entries().is_empty());
    let ability = ManaAbilityRef::Printed(ManaAbilityId(0));
    let rock = command(&land, B, tap(&land, 3, ability)).into_state();
    assert_eq!(rock.execution().mana(B, Colourless), 2.into());
    let acquired = round(rock).into_state();
    let ring = physical(&acquired, 3);
    assert_eq!(ring.owner(), B);
    assert_eq!(ring.controller(), Some(A));
    assert_eq!(ring.tapped(), Some(true));
    let opposing_turn = next_main(acquired, 2);
    assert_eq!(physical(&opposing_turn, 3).tapped(), Some(true));
    illegal(&opposing_turn, B, tap(&opposing_turn, 3, ability));
    let next_own = next_main(opposing_turn, 3);
    assert_eq!(physical(&next_own, 3).controller(), Some(A));
    let used = command(&next_own, A, tap(&next_own, 3, ability)).into_state();
    assert!(used.execution().window().entries().is_empty());
    assert_eq!(used.execution().mana(A, Colourless), 2.into());
    let returned = next_main(used, 4);
    assert_eq!(physical(&returned, 3).controller(), Some(B));
    assert_eq!(physical(&returned, 3).tapped(), Some(false));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_smothering_tithe_offers_the_drawer_two_mana_or_creates_a_treasure() {
    use ManaType::*;
    use spellwake_engine::{effects::ResolutionPaymentReply, land::BasicLandType};
    let price = [(Colourless, 3), (White, 1)];
    let draw = seed(
        2,
        A,
        ObjectDestination::Hand,
        free_instant(
            BodyTarget::SinglePlayer,
            Instruction::DrawTargetPlayer {
                amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
            },
        ),
    );
    let initial = game(
        vec![A, B],
        vec![
            hand("Smothering Tithe"),
            draw,
            named(
                "Forest",
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named(
                "Island",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let our_draw = round(targeted(&entered, A, 2, TargetRef::Player(A)).into_state()).into_state();
    assert!(our_draw.execution().window().entries().is_empty());
    let their_draw =
        round(targeted(&entered, A, 2, TargetRef::Player(B)).into_state()).into_state();
    assert_eq!(physical(&their_draw, 101).zone(), ObjectZone::Hand(B));
    let offer = round(their_draw).into_state();
    assert_eq!(offer.pending_decision().unwrap().chooser(), B);
    let made = reply(
        &offer,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    check_artifact_token(
        made.state(),
        tokens(&made, A, 1)[0],
        ArtifactSubtype::Treasure,
    );
    let mut funded = offer;
    for (land, kind) in [(3, BasicLandType::Forest), (4, BasicLandType::Island)] {
        funded = command(
            &funded,
            B,
            GameCommand::ActivateMana {
                source: physical(&funded, land).id(),
                ability: ManaAbilityRef::BasicLand(kind),
            },
        )
        .into_state();
    }
    let pay = |mana| DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(mana)));
    illegal(&funded, B, combat_declaration(&funded, pay(&[(Blue, 1)])));
    let no_treasure = reply(&funded, pay(&[(Blue, 1), (Green, 1)]));
    assert!(tokens(&no_treasure, A, 0).is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_storm_kiln_artist_makes_treasure_on_cast_and_copy_and_counts_only_own_artifacts() {
    use ManaType::*;
    use spellwake_engine::{effects::CopyTargetChoice, life::LifeAdjustment};
    let price = [(Colourless, 3), (Red, 1)];
    let pool = named(
        "Mirrorpool",
        2,
        A,
        ObjectDestination::Battlefield { controller: A },
    );
    let copy_ability = pool
        .program
        .activated
        .iter()
        .find(|ability| matches!(ability.body.target, BodyTarget::SingleSpell(_)))
        .unwrap()
        .id;
    let initial = game(
        vec![A, B],
        vec![
            hand("Storm-Kiln Artist"),
            pool,
            named(
                "Mind Stone",
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Sol Ring",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SinglePlayer,
                    Instruction::AdjustTargetLife {
                        adjustment: LifeAdjustment::Lose(1.into()),
                    },
                ),
            ),
        ],
        &[(Colourless, 6), (Red, 1)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let power = |state: &GameState| {
        state
            .execution()
            .power_toughness(physical(state, 1).id())
            .unwrap()
            .unwrap()
            .derived
            .power
    };
    assert_eq!(power(&entered), 3.into());
    let cast = targeted(&entered, A, 5, TargetRef::Player(B)).into_state();
    let original = cast.execution().window().entries()[0].id();
    assert_eq!(cast.execution().window().entries().len(), 2);
    let first = round(cast);
    check_artifact_token(
        first.state(),
        tokens(&first, A, 1)[0],
        ArtifactSubtype::Treasure,
    );
    assert_eq!(power(first.state()), 4.into());
    let activating = command(
        first.state(),
        A,
        GameCommand::ActivateAbility {
            source: physical(first.state(), 2).id(),
            ability: copy_ability,
            target: TargetRef::Stack(original),
            payment: Some(spend(&[(Colourless, 3)])),
        },
    )
    .into_state();
    let offer = round(activating).into_state();
    let doubled = reply(&offer, DecisionReply::CopyTargets(CopyTargetChoice::Retain)).into_state();
    assert_eq!(doubled.execution().window().entries().len(), 3);
    let second = round(doubled);
    let latest = super::set_encore::new_tokens(&second);
    assert_eq!(latest.len(), 1);
    check_artifact_token(second.state(), latest[0], ArtifactSubtype::Treasure);
    assert_eq!(power(second.state()), 5.into());
    let resolved = round(round(second.into_state()).into_state()).into_state();
    assert_eq!(resolved.execution().life_total(B), Some(&18.into()));
    assert!(resolved.execution().window().entries().is_empty());
}
