use super::*;
use spellwake_engine::{
    combat::AttackDeclaration,
    game::{DecisionReply, GameInput, GameState},
    turn::{Step, TurnStart},
};
#[test]
#[ignore = "requires retained source-qualified vocabulary"]
fn ninjutsu_replays_the_returned_attacker_and_only_reveals_the_current_hand_source() {
    let mut ninja = Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
    ninja.activated.push(ActivatedAbility::ninjutsu(
        ActivatedAbilityId(1),
        ManaCost::free(),
    ));
    let fighter = Program::creature(None, 2.into(), 3.into(), BTreeSet::new())
        .with_printed_keywords([spellwake_engine::characteristics::KeywordAbility::Haste]);
    let fixture = seed(vec![
        card(1, A, ObjectDestination::Hand, ninja),
        card(
            2,
            A,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        ),
        card(
            3,
            A,
            ObjectDestination::Battlefield { controller: A },
            fighter,
        ),
    ]);
    let game = GameState::initialize_scheduled(
        GameId(90),
        context(&fixture.creature_vocabulary),
        fixture,
        TurnStart {
            number: 1,
            step: Step::DeclareAttackers,
        },
    )
    .unwrap()
    .into_state();
    let d = game.pending_decision().unwrap();
    let attacker = object(&game, 3);
    let game = game
        .advance(GameInput {
            basis: game.basis().clone(),
            actor: d.chooser(),
            command: GameCommand::Reply {
                decision: d.id(),
                choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                    attacker,
                    defender: B,
                    defending_planeswalker: None,
                }]),
            },
        })
        .unwrap()
        .into_state();
    let game = bridged(&game, viewer_a(), |_| PlayerAction::Pass {});
    let game = bridged(&game, viewer_b(), |_| PlayerAction::Pass {});
    let source = object(&game, 1);
    let game = bridged(&game, viewer_a(), |scope| PlayerAction::Ninjutsu {
        source: scope.issue(NativeReference::Object(source)).unwrap(),
        ability: scope
            .issue(NativeReference::ActivatedAbility {
                source,
                ability: ActivatedAbilityId(1),
            })
            .unwrap(),
        attacker: scope.issue(NativeReference::Object(attacker)).unwrap(),
        payment: None,
    });
    let before_resolution = scope(&game, viewer_b())
        .project(&metadata(), &Display::default())
        .unwrap();
    let hand = before_resolution
        .zones
        .iter()
        .find(|z| z.zone_type() == PlayerZoneKind::Hand && z.owner_seat_id() == Some(viewer_a()))
        .unwrap();
    assert!(
        matches!(hand,PlayerZone::Mixed{objects,hidden_count,..} if objects.len()==1 && hidden_count.as_rules_integer()==&2.into())
    );
    let game = bridged(&game, viewer_a(), |_| PlayerAction::Pass {});
    let game = bridged(&game, viewer_b(), |_| PlayerAction::Pass {});
    let after = scope(&game, viewer_b())
        .project(&metadata(), &Display::default())
        .unwrap();
    assert!(after.zones.iter().any(|z|matches!(z,PlayerZone::Hidden{zone_type:PlayerZoneKind::Hand,owner_seat_id:Some(owner),hidden_count,..} if *owner==viewer_a() && hidden_count.as_rules_integer()==&2.into())));
}
