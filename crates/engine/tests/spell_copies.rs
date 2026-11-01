//! Restricted spell-copy behavior, including boundaries hidden by settled reference snapshots.
use spellwake_engine::{
    card_identity::{CardDestination, CardId, CardObjectId, CardZone},
    effects::{CopyTargetChoice, EffectError, EffectWindow, Instruction, Program, SeedCard},
    mana::{FloatingMana, ManaCost, ManaType},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackWindowError, WindowPhase},
    targets::TargetRef,
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn instant(
    id: u64,
    owner: SeatId,
    mana_type: ManaType,
    amount: i64,
    instruction: Instruction,
) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: CardDestination::Hand,
        program: Program::Instant {
            cost: ManaCost::specific(mana_type, amount.into()),
            instructions: vec![instruction],
        },
    }
}
fn setup() -> EffectWindow {
    let mut cards = vec![
        instant(
            1,
            A,
            ManaType::Red,
            1,
            Instruction::Damage { amount: 3.into() },
        ),
        instant(2, B, ManaType::Blue, 2, Instruction::CopySpell),
        instant(3, A, ManaType::Blue, 2, Instruction::CounterSpell),
        instant(4, A, ManaType::Blue, 1, Instruction::ReturnToOwnersHand),
    ];
    for (id, owner) in [(5, B), (6, A)] {
        cards.push(SeedCard {
            card: CardId(id),
            owner,
            destination: CardDestination::Battlefield { controller: owner },
            program: Program::VanillaCreature {
                power: 2.into(),
                toughness: 2.into(),
            },
        });
    }
    ready(
        EffectWindow::new(
            vec![A, B],
            A,
            cards,
            vec![
                FloatingMana {
                    player: A,
                    mana_type: ManaType::Red,
                    amount: 1.into(),
                },
                FloatingMana {
                    player: A,
                    mana_type: ManaType::Blue,
                    amount: 3.into(),
                },
                FloatingMana {
                    player: B,
                    mana_type: ManaType::Blue,
                    amount: 2.into(),
                },
            ],
        )
        .unwrap(),
    )
}
fn object(s: &EffectWindow, id: u64) -> CardObjectId {
    s.window().cards().current_identity(CardId(id)).unwrap()
}
fn ready(s: EffectWindow) -> EffectWindow {
    s.settle_checks(s.revision()).unwrap()
}
fn holding(mut s: EffectWindow, actor: SeatId) -> EffectWindow {
    for _ in 0..3 {
        if matches!(
            s.window().phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ) {
            s = ready(s);
        }
        match s.window().phase() {
            WindowPhase::Priority(PriorityPhase::Ready { holder }) if holder == actor => return s,
            WindowPhase::Priority(PriorityPhase::Ready { holder }) => {
                s = s.pass(s.revision(), holder).unwrap()
            }
            _ => panic!("unexpected phase"),
        }
    }
    panic!("no holder")
}
fn pending(mut s: EffectWindow) -> EffectWindow {
    for _ in 0..3 {
        if matches!(
            s.window().phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ) {
            s = ready(s);
        }
        match s.window().phase() {
            WindowPhase::Priority(PriorityPhase::Ready { holder }) => {
                s = s.pass(s.revision(), holder).unwrap()
            }
            WindowPhase::CheckingTargets(_) => return s,
            _ => panic!("unexpected phase"),
        }
    }
    panic!("not resolving")
}
fn cast(s: EffectWindow, actor: SeatId, id: u64, target: TargetRef) -> EffectWindow {
    let s = holding(s, actor);
    s.cast(s.revision(), actor, object(&s, id), target).unwrap()
}
fn resolve(s: EffectWindow) -> EffectWindow {
    let s = pending(s);
    s.resolve_top(s.revision()).unwrap()
}
fn stacked() -> EffectWindow {
    let s = setup();
    let target = object(&s, 5);
    let s = cast(s, A, 1, TargetRef::Card(target));
    let bolt = s.window().entries()[0].id();
    cast(s, B, 2, TargetRef::Stack(bolt))
}
fn copied() -> EffectWindow {
    let s = pending(stacked());
    let target = object(&s, 6);
    let s = s
        .resolve_top_with_copy_choice(
            s.revision(),
            B,
            CopyTargetChoice::Change(TargetRef::Card(target)),
        )
        .unwrap();
    ready(s)
}
fn copy_id(s: &EffectWindow) -> StackEntryId {
    *s.window().spell_copies().keys().next().unwrap()
}
#[test]
fn copy_has_independent_identity_owner_payload_and_no_physical_card_or_mana_charge() {
    let s = copied();
    let copy = copy_id(&s);
    assert_ne!(copy, s.window().entries()[0].id());
    assert_eq!(s.window().cards().iter().count(), 6);
    assert_eq!(s.window().spell_copies()[&copy].owner, B);
    assert_eq!(s.window().spell_copies()[&copy].controller, Some(B));
    assert_eq!(s.mana(B, ManaType::Blue), 0.into());
    assert_eq!(s.mana(B, ManaType::Red), 0.into());
    let s = resolve(s);
    assert_eq!(s.creature(object(&s, 6)).unwrap().damage, 3.into());
    assert_eq!(
        s.window().spell_copies()[&copy].zone,
        CardZone::Graveyard(B)
    );
    assert_eq!(s.window().spell_copies()[&copy].controller, None);
    assert_eq!(s.window().entries().len(), 1);
    assert_eq!(
        s.window().finish_checks(s.window().revision()),
        Err(StackWindowError::CopyChecksRequired)
    );
    let s = ready(s);
    assert!(s.window().spell_copies().is_empty());
    assert_eq!(
        s.window().cards().get(object(&s, 6)).unwrap().zone(),
        CardZone::Graveyard(A)
    );
    let s = ready(resolve(s));
    assert_eq!(
        s.window().cards().get(object(&s, 5)).unwrap().zone(),
        CardZone::Graveyard(B)
    );
}
#[test]
fn countering_original_leaves_copied_payload_executable() {
    let s = copied();
    let original = s.window().entries()[0].id();
    let copy = copy_id(&s);
    let s = ready(resolve(cast(s, A, 3, TargetRef::Stack(original))));
    assert_eq!(s.window().entries()[0].id(), copy);
    let s = ready(resolve(s));
    assert!(s.window().entries().is_empty());
    assert_eq!(
        s.window().cards().get(object(&s, 5)).unwrap().zone(),
        CardZone::Battlefield
    );
    assert_eq!(
        s.window().cards().get(object(&s, 6)).unwrap().zone(),
        CardZone::Graveyard(A)
    );
}
#[test]
fn countered_copy_exits_before_sba_without_removing_or_refunding_original() {
    let s = copied();
    let copy = copy_id(&s);
    let s = resolve(cast(s, A, 3, TargetRef::Stack(copy)));
    assert_eq!(
        s.window().spell_copies()[&copy].zone,
        CardZone::Graveyard(B)
    );
    assert_eq!(s.window().entries().len(), 1);
    assert_eq!(s.mana(A, ManaType::Red), 0.into());
    assert_eq!(s.mana(B, ManaType::Blue), 0.into());
    let s = ready(s);
    assert!(s.window().spell_copies().is_empty());
    let s = ready(resolve(s));
    assert_eq!(
        s.window().cards().get(object(&s, 5)).unwrap().zone(),
        CardZone::Graveyard(B)
    );
    assert_eq!(
        s.window().cards().get(object(&s, 6)).unwrap().zone(),
        CardZone::Battlefield
    );
}
fn missing() -> (EffectWindow, CardObjectId) {
    let s = stacked();
    let old = object(&s, 5);
    let s = ready(resolve(cast(s, A, 4, TargetRef::Card(old))));
    (pending(s), old)
}
#[test]
fn copy_may_retain_already_illegal_target_but_never_follows_returned_card() {
    let (s, old) = missing();
    assert_ne!(old, object(&s, 5));
    let s = ready(
        s.resolve_top_with_copy_choice(s.revision(), B, CopyTargetChoice::Retain)
            .unwrap(),
    );
    let copy = copy_id(&s);
    assert_eq!(
        s.window().entries().last().unwrap().targets().targets()[0]
            .choice()
            .target,
        TargetRef::Card(old)
    );
    let s = resolve(s);
    assert_eq!(
        s.window().spell_copies()[&copy].zone,
        CardZone::Graveyard(B)
    );
    let s = ready(resolve(ready(s)));
    assert!(s.window().entries().is_empty());
    assert_eq!(
        s.window().cards().get(object(&s, 5)).unwrap().zone(),
        CardZone::Hand(B)
    );
    assert_eq!(s.creature(object(&s, 6)).unwrap().damage, 0.into());
}
#[test]
fn illegal_new_target_wrong_chooser_stale_revision_and_missing_choice_are_atomic() {
    let (s, old) = missing();
    let saved = s.clone();
    assert_eq!(
        s.resolve_top(s.revision()),
        Err(EffectError::CopyChoiceRequired)
    );
    assert_eq!(
        s.resolve_top_with_copy_choice(s.revision(), A, CopyTargetChoice::Retain),
        Err(EffectError::WrongChooser)
    );
    for target in [
        TargetRef::Card(old),
        TargetRef::Card(object(&s, 5)),
        TargetRef::Player(A),
    ] {
        assert_eq!(
            s.resolve_top_with_copy_choice(s.revision(), B, CopyTargetChoice::Change(target)),
            Err(EffectError::IllegalTarget)
        );
    }
    assert_eq!(
        s.resolve_top_with_copy_choice(0, B, CopyTargetChoice::Retain),
        Err(EffectError::Window(StackWindowError::StaleRevision))
    );
    assert_eq!(s, saved);
    let target = object(&s, 6);
    let s = ready(
        s.resolve_top_with_copy_choice(
            s.revision(),
            B,
            CopyTargetChoice::Change(TargetRef::Card(target)),
        )
        .unwrap(),
    );
    let s = ready(resolve(s));
    assert_eq!(
        s.window().cards().get(object(&s, 6)).unwrap().zone(),
        CardZone::Graveyard(A)
    );
}
#[test]
fn target_departing_after_copy_creation_invalidates_both_spells() {
    let s = pending(stacked());
    let s = ready(
        s.resolve_top_with_copy_choice(s.revision(), B, CopyTargetChoice::Retain)
            .unwrap(),
    );
    let old = object(&s, 5);
    let s = ready(resolve(cast(s, A, 4, TargetRef::Card(old))));
    let s = ready(resolve(s));
    let s = ready(resolve(s));
    assert!(s.window().entries().is_empty());
    assert_eq!(
        s.window().cards().get(object(&s, 5)).unwrap().zone(),
        CardZone::Hand(B)
    );
    assert_eq!(s.creature(object(&s, 6)).unwrap().damage, 0.into());
}
#[test]
fn copy_decision_is_not_accepted_for_an_ordinary_damage_spell() {
    let s = setup();
    let target = object(&s, 5);
    let s = pending(cast(s, A, 1, TargetRef::Card(target)));
    let saved = s.clone();
    assert_eq!(
        s.resolve_top_with_copy_choice(s.revision(), A, CopyTargetChoice::Retain),
        Err(EffectError::UnexpectedCopyChoice)
    );
    assert_eq!(s, saved);
}

#[test]
fn departed_copy_source_target_does_not_request_or_create_a_copy() {
    let s = stacked();
    let bolt = s.window().entries()[0].id();
    let s = ready(resolve(cast(s, A, 3, TargetRef::Stack(bolt))));
    let s = ready(resolve(s));
    assert!(s.window().entries().is_empty());
    assert!(s.window().spell_copies().is_empty());
    assert_eq!(s.creature(object(&s, 5)).unwrap().damage, 0.into());
}
