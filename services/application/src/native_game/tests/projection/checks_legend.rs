//! Public Legend originals across actual precommit multiplayer departures.
use super::checks_order::{app, departure, project};
use super::chooser_replacement::{bound, four_seat_root};
use super::*;
use spellwake_contracts::names::{EnglishCardName, ObjectNames};
use spellwake_engine::{effects::SeedCard, program::Supertype};
const C: SeatId = SeatId(29);
const D: SeatId = SeatId(77);
fn legendary(id: u64, owner: SeatId, controller: SeatId, name: &str) -> SeedCard {
    let mut program = Program::artifact(None, vec![]);
    program.supertypes.insert(Supertype::Legendary);
    program.names = ObjectNames::new(vec![EnglishCardName::new(name.into()).unwrap()]).unwrap();
    card(
        id,
        owner,
        ObjectDestination::Battlefield { controller },
        program,
    )
}
fn prepare(owner: SeatId) -> GameState {
    four_seat_root(vec![
        legendary(1, A, A, "Alpha"),
        legendary(2, owner, A, "Alpha"),
        legendary(3, C, C, "Gamma"),
        legendary(4, C, C, "Gamma"),
    ])
}
fn choose(source: &GameState, wanted: u64) -> GameState {
    let actor = source.pending_decision().unwrap().chooser();
    let viewer = app(source, actor);
    let mut scope = NativeScope::new(source, bound(source, viewer)).unwrap();
    let projected = project(&mut scope);
    let descriptor = &projected.snapshot().decisions[0];
    assert!(descriptor.kind == DecisionKind::KeepLegend);
    assert!(
        matches!(crate::native_game::clocks::waiting(source,&scope.binding).unwrap(),crate::native_game::clocks::Waiting::Decision(seat) if seat==viewer)
    );
    let PlayerDomain::Legend { conflicts } = &descriptor.domain else {
        panic!("Legend")
    };
    let wanted = object(source, wanted);
    let (conflict,keep)=conflicts.iter().find_map(|conflict|conflict.objects.iter().find(|handle|matches!(scope.reference(handle),Ok(NativeReference::Object(id)) if id==wanted)).map(|keep|(conflict.conflict.clone(),keep.clone()))).unwrap();
    let original = command(
        &scope,
        PlayerAction::Choose {
            decision: descriptor.decision.clone(),
            response: PlayerChoice::Legend { conflict, keep },
        },
    );
    let roster = projected
        .snapshot()
        .players
        .iter()
        .map(|seat| seat.seat_id)
        .collect::<Vec<_>>();
    descriptor
        .validate_response(&original, viewer, &roster, &[])
        .unwrap();
    let prepared = scope.propose(&original).unwrap();
    input_codec::verify(&scope, &prepared);
    let bytes = scope.encode_input(&prepared).unwrap();
    let foreign=source.execution().window().cards().iter().enumerate().find(|(_,object)|
        !conflicts.iter().flat_map(|conflict| &conflict.objects).any(|handle| matches!(scope.reference(handle),Ok(NativeReference::Object(id)) if id==object.id()))
    );
    if let Some((index, _)) = foreign {
        let mut record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let row = record["transition"]["references"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["location"]["kind"] == "object")
            .unwrap();
        row["location"]["index"] = serde_json::Value::String(index.to_string());
        assert!(
            restore_input(
                source,
                scope.binding.clone(),
                &serde_json::to_vec(&record).unwrap()
            )
            .is_err()
        );
    }
    if conflicts.len() > 1 {
        let mut record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let row = record["transition"]["references"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["location"]["kind"] == "legend")
            .unwrap();
        let index = row["location"]["index"]
            .as_str()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        row["location"]["index"] =
            serde_json::Value::String(((index + 1) % conflicts.len()).to_string());
        assert!(
            restore_input(
                source,
                scope.binding.clone(),
                &serde_json::to_vec(&record).unwrap()
            )
            .is_err()
        );
    }
    assert!(
        restore_input(
            prepared.candidate().state(),
            bound(prepared.candidate().state(), viewer),
            &bytes
        )
        .is_err()
    );
    let observer = if actor == D { A } else { D };
    let mut other = NativeScope::new(source, bound(source, app(source, observer))).unwrap();
    assert!(project(&mut other).snapshot().decisions.is_empty());
    assert!(other.propose(&original).is_err());
    prepared.candidate().state().clone()
}
#[test]
#[ignore = "requires genuine vocabulary; real Legend departures and exact public input replay"]
fn public_legend_originals_keep_accepted_cohort_skip_gone_chooser_and_refresh_unanswered_domain() {
    for mode in 0..4 {
        let mut source = if mode == 3 {
            four_seat_root(vec![
                legendary(1, B, A, "Alpha"),
                legendary(2, B, A, "Alpha"),
                legendary(5, A, A, "Beta"),
                legendary(6, A, A, "Beta"),
                legendary(3, C, C, "Gamma"),
                legendary(4, C, C, "Gamma"),
            ])
        } else {
            prepare(if mode == 2 { B } else { A })
        };
        if mode == 0 || mode == 3 {
            source = choose(&source, if mode == 3 { 5 } else { 1 });
            source = departure(&source, B).into_state();
            source = departure(&source, D).into_state();
        } else {
            source = departure(&source, if mode == 1 { A } else { B }).into_state();
        }
        assert_eq!(source.pending_decision().unwrap().chooser(), C);
        source = choose(&source, 3);
        assert!(source.pending_decision().is_none());
        assert_eq!(
            source
                .execution()
                .window()
                .cards()
                .get(object(&source, 3))
                .unwrap()
                .zone(),
            ObjectZone::Battlefield
        );
        assert_eq!(
            source
                .execution()
                .window()
                .cards()
                .get(object(&source, 4))
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(C)
        );
        if mode == 0 {
            assert_eq!(
                source
                    .execution()
                    .window()
                    .cards()
                    .get(object(&source, 2))
                    .unwrap()
                    .zone(),
                ObjectZone::Graveyard(A)
            );
        }
        let mut scope = NativeScope::new(&source, bound(&source, app(&source, C))).unwrap();
        assert!(project(&mut scope).snapshot().decisions.is_empty());
    }
}
