//! Uninterrupted World history (CR 704.5k), distinct from ordering timestamps.
use super::*;
use crate::program::Supertype;

fn world_program(is_world: bool) -> Program {
    let mut program = Program::artifact(None, vec![]);
    if is_world {
        program.supertypes.insert(Supertype::World);
    }
    program
}
fn bf() -> ObjectDestination {
    ObjectDestination::Battlefield { controller: P0 }
}
fn cohorts(f: &Fixture, groups: &[&[&str]]) -> Vec<Vec<CardId>> {
    groups
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|alias| {
                    f.cards
                        .get(f.object(alias))
                        .unwrap()
                        .card_id()
                        .expect("physical fixture card")
                })
                .collect()
        })
        .collect()
}
fn initialize(f: &mut Fixture, groups: &[&[&str]]) {
    let current = evaluate(f.world()).unwrap();
    let groups = cohorts(f, groups);
    f.state
        .initialize_world_tenures(&f.cards, &current, &groups)
        .unwrap();
}
fn tenure(f: &Fixture, alias: &str) -> Option<WorldTenure> {
    f.state
        .world_tenures()
        .find_map(|(id, tenure)| (id == f.object(alias)).then_some(tenure))
}
fn set_world(f: &mut Fixture, alias: &str, value: bool) {
    // Explicit unit fixture preparation of the next evaluated cut, not a public
    // definition-update operation. Actual publisher trajectories are separate.
    let card = f
        .cards
        .get(f.object(alias))
        .unwrap()
        .card_id()
        .expect("physical fixture card");
    let program = f.programs.get_mut(&card).unwrap();
    if value {
        program.supertypes.insert(Supertype::World);
    } else {
        program.supertypes.remove(&Supertype::World);
    }
}
fn reconcile(f: &mut Fixture) -> Vec<WorldTenureChange> {
    let current = evaluate(f.world()).unwrap();
    f.state.reconcile_world_tenures(&current).unwrap()
}

#[test]
fn world_initial_cohorts_override_timestamp_order_and_tied_latest_removes_every_world() {
    let mut f = Fixture::new(
        vocabulary(),
        vec![
            ("old", bf(), world_program(true)),
            ("new-a", bf(), world_program(true)),
            ("new-b", bf(), world_program(true)),
        ],
        &["new-b", "new-a", "old"],
    );
    initialize(&mut f, &[&["old"], &["new-a", "new-b"]]);
    assert!(
        f.state.object_timestamp(f.object("old")) > f.state.object_timestamp(f.object("new-a"))
    );
    assert!(tenure(&f, "old") < tenure(&f, "new-a"));
    assert_eq!(tenure(&f, "new-a"), tenure(&f, "new-b"));
    let current = evaluate(f.world()).unwrap();
    assert_eq!(
        f.state.world_eliminations(&current).unwrap(),
        f.affected(&["old", "new-a", "new-b"])
    );
}

#[test]
fn world_sole_latest_survives_across_controllers_without_partitioning() {
    let mut f = Fixture::new(
        vocabulary(),
        vec![
            ("old", bf(), world_program(true)),
            (
                "new",
                ObjectDestination::Battlefield {
                    controller: SeatId(1),
                },
                world_program(true),
            ),
        ],
        &["new", "old"],
    );
    initialize(&mut f, &[&["old"], &["new"]]);
    assert_eq!(
        f.state
            .world_eliminations(&evaluate(f.world()).unwrap())
            .unwrap(),
        f.affected(&["old"])
    );
}

#[test]
fn world_initial_cohorts_reject_incomplete_duplicate_empty_foreign_and_nonbattlefield_members_atomically()
 {
    let f = Fixture::new(
        vocabulary(),
        vec![
            ("world", bf(), world_program(true)),
            ("ordinary", bf(), world_program(false)),
            ("hand", ObjectDestination::Hand, world_program(true)),
        ],
        &["world", "ordinary"],
    );
    let current = evaluate(f.world()).unwrap();
    let w = f
        .cards
        .get(f.object("world"))
        .unwrap()
        .card_id()
        .expect("physical fixture card");
    let ordinary = f
        .cards
        .get(f.object("ordinary"))
        .unwrap()
        .card_id()
        .expect("physical fixture card");
    let hand = f
        .cards
        .get(f.object("hand"))
        .unwrap()
        .card_id()
        .expect("physical fixture card");
    for groups in [
        vec![],
        vec![vec![]],
        vec![vec![w, w]],
        vec![vec![w], vec![w]],
        vec![vec![w, ordinary]],
        vec![vec![w, hand]],
        vec![vec![w, CardId(999)]],
    ] {
        let mut state = f.state.clone();
        let before = state.clone();
        assert_eq!(
            state.initialize_world_tenures(&f.cards, &current, &groups),
            Err(CharacteristicsError::InvalidInitialWorldCohorts)
        );
        assert_eq!(state, before);
    }
}

#[test]
fn world_same_cut_gainers_share_one_cohort_and_continuous_members_keep_their_age() {
    let mut f = Fixture::new(
        vocabulary(),
        vec![
            ("old", bf(), world_program(true)),
            ("a", bf(), world_program(false)),
            ("b", bf(), world_program(false)),
        ],
        &["b", "a", "old"],
    );
    initialize(&mut f, &[&["old"]]);
    let old = tenure(&f, "old").unwrap();
    set_world(&mut f, "a", true);
    set_world(&mut f, "b", true);
    let before = f.clone();
    let current = evaluate(f.world()).unwrap();
    assert_eq!(current, evaluate(f.world()).unwrap());
    assert_eq!(f, before);
    let changes = reconcile(&mut f);
    assert_eq!(changes.len(), 2);
    assert_eq!(tenure(&f, "old"), Some(old));
    assert_eq!(tenure(&f, "a"), tenure(&f, "b"));
    assert!(tenure(&f, "a").unwrap() > old);
    assert_eq!(f.state.next_world_tenure_sequence(), 3);
    let before = f.clone();
    assert!(reconcile(&mut f).is_empty());
    assert_eq!(f, before);
}

#[test]
fn world_loss_and_regain_renew_tenure_while_overflow_preserves_departures_and_gains() {
    let mut f = Fixture::new(
        vocabulary(),
        vec![
            ("a", bf(), world_program(true)),
            ("b", bf(), world_program(false)),
        ],
        &["a", "b"],
    );
    initialize(&mut f, &[&["a"]]);
    let old = tenure(&f, "a").unwrap();
    set_world(&mut f, "a", false);
    assert_eq!(
        reconcile(&mut f),
        vec![WorldTenureChange::Lost {
            object: f.object("a"),
            tenure: old
        }]
    );
    set_world(&mut f, "a", true);
    reconcile(&mut f);
    assert!(tenure(&f, "a").unwrap() > old);
    set_world(&mut f, "a", false);
    set_world(&mut f, "b", true);
    f.state.qualification_set_next_world_tenure(u64::MAX);
    let before = f.state.clone();
    let current = evaluate(f.world()).unwrap();
    assert_eq!(
        f.state.reconcile_world_tenures(&current),
        Err(CharacteristicsError::WorldTenureExhausted)
    );
    assert_eq!(f.state, before);
}

#[test]
fn world_history_mismatch_is_not_hidden_by_the_singleton_early_return() {
    let mut f = Fixture::new(vocabulary(), vec![("a", bf(), world_program(true))], &["a"]);
    assert_eq!(
        f.state.world_eliminations(&evaluate(f.world()).unwrap()),
        Err(CharacteristicInvariantError::MissingWorldTenure(
            f.object("a")
        ))
    );
    initialize(&mut f, &[&["a"]]);
    set_world(&mut f, "a", false);
    assert_eq!(
        f.state.world_eliminations(&evaluate(f.world()).unwrap()),
        Err(CharacteristicInvariantError::UnexpectedWorldTenure(
            f.object("a")
        ))
    );
    reconcile(&mut f);
    assert!(
        f.state
            .world_eliminations(&evaluate(f.world()).unwrap())
            .unwrap()
            .is_empty()
    );
}
