use super::*;
use spellwake_engine::{
    mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput, ManaType},
    program::{TriggeredAbility, TriggeredAbilityId},
};

#[test]
#[ignore = "requires retained source-qualified vocabulary"]
fn cascade_reveals_are_public_but_only_the_controller_can_cast_during_the_trigger() {
    let price = ManaCost::one(ManaType::Green);
    let mut source = Program::instant(
        price,
        ExecutableBody::new(BodyTarget::None, vec![Instruction::Jump { destination: 1 }]),
    );
    source
        .triggered
        .push(TriggeredAbility::cascade(TriggeredAbilityId(1)));
    let land = Program::land(
        Default::default(),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed([(ManaType::Green, 1.into())].into()),
        }],
    );
    let state = main_state(vec![
        card(1, A, ObjectDestination::Hand, source),
        card(
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
            land.clone(),
        ),
        card(3, A, ObjectDestination::LibraryBottom, land),
        card(
            4,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(Some(ManaCost::free()), vec![]),
        ),
    ]);
    let mana = bridged(&state, viewer_a(), |view| {
        let source = object(&state, 2);
        PlayerAction::ActivateMana {
            source: view.issue(NativeReference::Object(source)).unwrap(),
            ability: view
                .issue(NativeReference::ManaAbility {
                    source,
                    ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
                })
                .unwrap(),
        }
    });
    let cast = bridged(&mana, viewer_a(), |view| PlayerAction::CastUntargeted {
        source: view
            .issue(NativeReference::Object(object(&mana, 1)))
            .unwrap(),
        payment: None,
    });
    let passed = bridged(&cast, viewer_a(), |_| PlayerAction::Pass {});
    let choice = bridged(&passed, viewer_b(), |_| PlayerAction::Pass {});
    for viewer in [viewer_a(), viewer_b()] {
        let snap = scope(&choice, viewer)
            .project(&metadata(), &Display::default())
            .unwrap();
        assert_eq!(
            snap.players
                .iter()
                .map(|seat| seat.cascade_reveals.len())
                .sum::<usize>(),
            2
        );
        assert_eq!(snap.decisions.len(), usize::from(viewer == viewer_a()));
    }
    let second = bridged(&choice, viewer_a(), |view| {
        let snap = view.project(&metadata(), &Display::default()).unwrap();
        let frame = &snap.decisions[0];
        let PlayerDomain::CascadeCast {
            allows_x: _,
            spell,
            targets,
            target_required,
        } = &frame.domain
        else {
            panic!("cascade permission")
        };
        assert!(
            spell
                == &view
                    .issue(NativeReference::Object(object(&choice, 4)))
                    .unwrap()
        );
        assert!(!*target_required && targets.is_empty());
        PlayerAction::Choose {
            decision: frame.decision.clone(),
            response: PlayerChoice::CascadeCast {
                face: None,
                adventure: false,
                chosen_x: None,
                target: None,
                payment: None,
                additional: vec![],
                resources: vec![],
            },
        }
    });
    assert_eq!(second.execution().window().entries().len(), 2);
    assert_eq!(
        second.execution().window().cards().library(A),
        &[object(&second, 3)]
    );
    let passed = bridged(&second, viewer_a(), |_| PlayerAction::Pass {});
    let done = bridged(&passed, viewer_b(), |_| PlayerAction::Pass {});
    assert_eq!(
        done.execution()
            .window()
            .cards()
            .get(object(&done, 4))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
}
