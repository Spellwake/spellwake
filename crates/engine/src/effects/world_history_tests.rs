//! Captured actual World history, later source exit and a real draw overlay.
use super::*;
use crate::{
    characteristics::{
        ResolvedDuration, ResolvedPartSpec, SubtypeOperation, TypeChangeSpec, TypeOperation,
    },
    program::Supertype,
    test_support::{characteristic_trajectories as tr, packet::AliasContext, projection},
};
use serde_json::json;

#[test]
fn world_history_snapshot_and_privileged_receipts_survive_later_loss_exit_and_draw_overlay() {
    let operation = |supertypes| {
        struct _Tmp {
            parts: crate::characteristics::ResolvedPartsSpec,
            duration: crate::characteristics::ResolvedDuration,
        }
        let _t = _Tmp {
            parts: crate::characteristics::ResolvedPartsSpec(vec![ResolvedPartSpec::Type(
                TypeChangeSpec {
                    land_subtypes: SubtypeOperation::Keep,
                    supertypes,
                    types: TypeOperation::Keep,
                    subtypes: SubtypeOperation::Keep,
                },
            )]),
            duration: ResolvedDuration::NoStatedDuration,
        };
        crate::program::Instruction::ApplyContinuous(Box::new(
            crate::toolbox::ApplyContinuousSpec {
                selector: Some(crate::toolbox::ObjectQuery::Targets),
                payload: crate::toolbox::ContinuousPayload::ResolvedParts(Box::new(_t.parts)),
                duration: _t.duration,
                condition: None,
                one_shot: false,
                as_emblem: None,
                emblem_label: None,
            },
        ))
    };
    let mut library = tr::card(4, Program::artifact(None, vec![]), false);
    library.destination = ObjectDestination::LibraryTop;
    let game = tr::world(
        tr::vocabulary(),
        vec![
            tr::card(1, Program::artifact(None, vec![]), true),
            tr::spell(
                2,
                BodyTarget::SinglePermanent,
                vec![operation(TypeOperation::Add(BTreeSet::from([
                    Supertype::World,
                ])))],
            ),
            tr::spell(
                3,
                BodyTarget::SinglePermanent,
                vec![
                    operation(TypeOperation::Remove(BTreeSet::from([Supertype::World]))),
                    crate::program::Instruction::ZoneOp(Box::new(
                        crate::toolbox::ZoneOpSpec::move_all(
                            crate::toolbox::ObjectQuery::Targets,
                            crate::toolbox::ZoneDest::Hand,
                        ),
                    )),
                ],
            ),
            library,
        ],
        vec![CardId(1)],
    );
    let target = tr::object(&game, 1);
    let (gained, _) = tr::resolve(&game, 2, target);
    let snapshot = gained
        .state()
        .execution()
        .rules_snapshot(gained.state().turn())
        .unwrap();
    let view = projection::candidate(&gained, Some(&game), &AliasContext::default()).unwrap();
    assert_eq!(
        view["state"]["world_tenures"],
        json!([{"object":format!("{target:?}"),"tenure":"1"}])
    );
    assert_eq!(view["state"]["resources"]["next_world_tenure"], "2");
    let projected_change = view["append"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|r| r["occurrences"].as_array().into_iter().flatten())
        .map(|r| &r["value"])
        .find(|v| v["tag"] == "WorldTenureChanged")
        .unwrap();
    assert_eq!(
        projected_change["change"],
        json!({"tag":"Gained","object":format!("{target:?}"),"tenure":"1"})
    );
    let (departed, _) = tr::resolve(gained.state(), 3, target);
    assert_eq!(departed.state().execution().world_tenures().count(), 0);
    assert!(
        departed
            .state()
            .execution()
            .window()
            .cards()
            .get(target)
            .is_none()
    );
    let old_view = snapshot.view();
    let old = old_view.characteristics.world_tenures().collect::<Vec<_>>();
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].0, target);
    assert_eq!(old[0].1.sequence(), 1);
    let old_library = tr::object(gained.state(), 4);
    let actual_draw = gained
        .state()
        .execution()
        .window()
        .cards()
        .draw_cards(
            gained.state().execution().window().cards().revision(),
            tr::P0,
            1.into(),
        )
        .unwrap();
    let overlay = snapshot.view_with_cards(&actual_draw.next).unwrap();
    assert_eq!(
        overlay.characteristics.world_tenures().collect::<Vec<_>>(),
        old
    );
    assert_eq!(overlay.characteristics.next_world_tenure_sequence(), 2);
    assert!(
        overlay
            .appearance(target)
            .unwrap()
            .current_supertypes
            .contains(&Supertype::World)
    );
    assert!(overlay.appearance(old_library).is_none());
    let current = gained.state().execution().clone();
    let first = current.evaluated_characteristics().unwrap();
    assert_eq!(first, current.evaluated_characteristics().unwrap());
    assert_eq!(&current, gained.state().execution());
}
