//! Restricted spell-copy behavior, including boundaries hidden by settled reference snapshots.
use spellwake_engine::combat::CombatRules;
use spellwake_engine::program::{BodyTarget, ExecutableBody, SpellTargetFilter};
use spellwake_engine::program::{CardType, Supertype};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    effects::{CopyTargetChoice, EffectError, EffectWindow, Instruction, Program, SeedCard},
    mana::{FloatingMana, ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackWindowError, WindowPhase},
    targets::TargetRef,
};
use std::collections::BTreeSet;
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn instant(
    id: u64,
    owner: SeatId,
    mana_type: ManaType,
    amount: i64,
    instruction: Instruction,
    body_target: BodyTarget,
) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::specific(mana_type, amount.into()),
            ExecutableBody::new(body_target, vec![instruction]),
        ),
    }
}
fn setup() -> EffectWindow {
    setup_with_cards(vec![])
}
fn setup_with_cards(extra: Vec<SeedCard>) -> EffectWindow {
    setup_with_traits(extra, BTreeSet::new())
}
fn setup_with_traits(extra: Vec<SeedCard>, supertypes: BTreeSet<Supertype>) -> EffectWindow {
    setup_with_card_types(
        extra,
        supertypes,
        BTreeSet::from([CardType::Instant]),
        BTreeSet::new(),
    )
}
fn setup_with_card_types(
    extra: Vec<SeedCard>,
    supertypes: BTreeSet<Supertype>,
    card_types: BTreeSet<CardType>,
    subtypes: BTreeSet<spellwake_contracts::creature_types::CreatureTypeId>,
) -> EffectWindow {
    let mut cards = vec![
        instant(
            1,
            A,
            ManaType::Red,
            1,
            Instruction::Damage { amount: 3.into() },
            BodyTarget::SingleCreature,
        ),
        instant(
            2,
            B,
            ManaType::Blue,
            2,
            Instruction::CopySpell,
            BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
        ),
        instant(
            3,
            A,
            ManaType::Blue,
            2,
            Instruction::CounterSpell,
            BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
        ),
        instant(
            4,
            A,
            ManaType::Blue,
            1,
            Instruction::ReturnToOwnersHand,
            BodyTarget::SingleCreature,
        ),
    ];
    cards.extend(extra);
    cards[0].program.supertypes = supertypes;
    cards[0].program.card_types = card_types;
    cards[0].program.creature_subtypes = subtypes;
    cards[0].program.names = spellwake_contracts::names::ObjectNames::single(
        spellwake_contracts::names::EnglishCardName::new("Lightning Bolt".into()).unwrap(),
    );
    for (id, owner) in [(5, B), (6, A)] {
        cards.push(SeedCard {
            card: CardId(id),
            owner,
            destination: ObjectDestination::Battlefield { controller: owner },
            program: Program::creature(None, 2.into(), 2.into(), std::collections::BTreeSet::new()),
        });
    }
    ready(
        EffectWindow::new({
            // Fixture premise: seeded BF timestamps follow this authored card sequence.
            let initial_battlefield_order = cards
                .iter()
                .filter(|card| {
                    matches!(
                        card.destination,
                        spellwake_engine::object_identity::ObjectDestination::Battlefield { .. }
                    )
                })
                .map(|card| card.card)
                .collect();
            GameSeed::with_uniform_life(
                vec![A, B],
                A,
                cards,
                initial_battlefield_order,
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
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                test_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        })
        .unwrap(),
    )
}
fn object(s: &EffectWindow, id: u64) -> ObjectId {
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
    stacked_with_traits(BTreeSet::new())
}
fn stacked_with_traits(supertypes: BTreeSet<Supertype>) -> EffectWindow {
    let s = setup_with_traits(vec![], supertypes);
    let target = object(&s, 5);
    let s = cast(s, A, 1, TargetRef::Object(target));
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
            CopyTargetChoice::Change(TargetRef::Object(target)),
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
    assert_eq!(s.creature(object(&s, 6)).unwrap().unwrap().damage, 3.into());
    assert_eq!(
        s.window().spell_copies()[&copy].zone,
        ObjectZone::Graveyard(B)
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
        ObjectZone::Graveyard(A)
    );
    let s = ready(resolve(s));
    assert_eq!(
        s.window().cards().get(object(&s, 5)).unwrap().zone(),
        ObjectZone::Graveyard(B)
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
        ObjectZone::Battlefield
    );
    assert_eq!(
        s.window().cards().get(object(&s, 6)).unwrap().zone(),
        ObjectZone::Graveyard(A)
    );
}
#[test]
fn countered_copy_exits_before_sba_without_removing_or_refunding_original() {
    let s = copied();
    let copy = copy_id(&s);
    let s = resolve(cast(s, A, 3, TargetRef::Stack(copy)));
    assert_eq!(
        s.window().spell_copies()[&copy].zone,
        ObjectZone::Graveyard(B)
    );
    assert_eq!(s.window().entries().len(), 1);
    assert_eq!(s.mana(A, ManaType::Red), 0.into());
    assert_eq!(s.mana(B, ManaType::Blue), 0.into());
    let s = ready(s);
    assert!(s.window().spell_copies().is_empty());
    let s = ready(resolve(s));
    assert_eq!(
        s.window().cards().get(object(&s, 5)).unwrap().zone(),
        ObjectZone::Graveyard(B)
    );
    assert_eq!(
        s.window().cards().get(object(&s, 6)).unwrap().zone(),
        ObjectZone::Battlefield
    );
}
fn missing() -> (EffectWindow, ObjectId) {
    let s = stacked();
    let old = object(&s, 5);
    let s = ready(resolve(cast(s, A, 4, TargetRef::Object(old))));
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
        TargetRef::Object(old)
    );
    let s = resolve(s);
    assert_eq!(
        s.window().spell_copies()[&copy].zone,
        ObjectZone::Graveyard(B)
    );
    let s = ready(resolve(ready(s)));
    assert!(s.window().entries().is_empty());
    assert_eq!(
        s.window().cards().get(object(&s, 5)).unwrap().zone(),
        ObjectZone::Hand(B)
    );
    assert_eq!(s.creature(object(&s, 6)).unwrap().unwrap().damage, 0.into());
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
        TargetRef::Object(old),
        TargetRef::Object(object(&s, 5)),
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
            CopyTargetChoice::Change(TargetRef::Object(target)),
        )
        .unwrap(),
    );
    let s = ready(resolve(s));
    assert_eq!(
        s.window().cards().get(object(&s, 6)).unwrap().zone(),
        ObjectZone::Graveyard(A)
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
    let s = ready(resolve(cast(s, A, 4, TargetRef::Object(old))));
    let s = ready(resolve(s));
    let s = ready(resolve(s));
    assert!(s.window().entries().is_empty());
    assert_eq!(
        s.window().cards().get(object(&s, 5)).unwrap().zone(),
        ObjectZone::Hand(B)
    );
    assert_eq!(s.creature(object(&s, 6)).unwrap().unwrap().damage, 0.into());
}
#[test]
fn copy_decision_is_not_accepted_for_an_ordinary_damage_spell() {
    let s = setup();
    let target = object(&s, 5);
    let s = pending(cast(s, A, 1, TargetRef::Object(target)));
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
    assert_eq!(s.creature(object(&s, 5)).unwrap().unwrap().damage, 0.into());
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("synthetic-only-tests")
}

#[test]
fn copy_of_copy_keeps_owned_traits_after_original_is_countered() {
    let expected_supertypes = BTreeSet::from([Supertype::Snow]);
    let expected_types = BTreeSet::from([CardType::Kindred, CardType::Instant]);
    let expected_subtypes = BTreeSet::from([test_vocabulary().resolve_label("Elf").unwrap()]);
    let s = setup_with_card_types(
        vec![instant(
            7,
            A,
            ManaType::Blue,
            0,
            Instruction::CopySpell,
            BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
        )],
        expected_supertypes.clone(),
        expected_types.clone(),
        expected_subtypes.clone(),
    );
    let target = object(&s, 5);
    let s = cast(s, A, 1, TargetRef::Object(target));
    let original = s.window().entries()[0].id();
    let expected = s.stack_copiable_names(original).unwrap().clone();
    assert_eq!(s.stack_copiable_card_types(original), Some(&expected_types));
    assert_eq!(
        s.stack_copiable_creature_subtypes(original),
        Some(&expected_subtypes)
    );
    assert_eq!(expected.iter().next().unwrap().as_str(), "Lightning Bolt");
    let s = pending(cast(s, B, 2, TargetRef::Stack(original)));
    let s = ready(
        s.resolve_top_with_copy_choice(s.revision(), B, CopyTargetChoice::Retain)
            .unwrap(),
    );
    let first = copy_id(&s);
    assert_eq!(s.stack_copiable_names(first), Some(&expected));
    assert_eq!(s.stack_copiable_card_types(first), Some(&expected_types));
    assert_eq!(
        s.stack_copiable_creature_subtypes(first),
        Some(&expected_subtypes)
    );
    assert_eq!(
        s.stack_copiable_supertypes(first),
        Some(&expected_supertypes)
    );
    let s = ready(resolve(cast(s, A, 3, TargetRef::Stack(original))));
    assert_eq!(s.stack_copiable_names(original), None);
    assert_eq!(s.stack_copiable_card_types(original), None);
    assert_eq!(s.stack_copiable_creature_subtypes(original), None);
    assert_eq!(s.stack_copiable_supertypes(original), None);
    let s = pending(cast(s, A, 7, TargetRef::Stack(first)));
    let s = ready(
        s.resolve_top_with_copy_choice(s.revision(), A, CopyTargetChoice::Retain)
            .unwrap(),
    );
    let second = s.window().entries().last().unwrap().id();
    assert_ne!(first, second);
    assert_eq!(s.stack_copiable_names(second), Some(&expected));
    assert_eq!(s.stack_copiable_card_types(second), Some(&expected_types));
    assert_eq!(
        s.stack_copiable_creature_subtypes(second),
        Some(&expected_subtypes)
    );
    assert_eq!(
        s.stack_copiable_supertypes(second),
        Some(&expected_supertypes)
    );
    assert_eq!(s.stack_copiable_names(original), None);
    assert_eq!(s.stack_copiable_card_types(original), None);
    assert_eq!(s.stack_copiable_creature_subtypes(original), None);
    assert_eq!(s.stack_copiable_names(first), Some(&expected));
    assert_eq!(s.stack_copiable_card_types(first), Some(&expected_types));
    assert_eq!(
        s.stack_copiable_creature_subtypes(first),
        Some(&expected_subtypes)
    );
    assert_eq!(s.stack_copiable_names(second), Some(&expected));
    assert_eq!(s.stack_copiable_card_types(second), Some(&expected_types));
    assert_eq!(
        s.stack_copiable_creature_subtypes(second),
        Some(&expected_subtypes)
    );
    assert_eq!(s.stack_body(first), s.stack_body(second));
    let s = ready(resolve(s));
    assert_eq!(s.stack_copiable_names(second), None);
    assert_eq!(s.stack_copiable_card_types(second), None);
    assert_eq!(s.stack_copiable_creature_subtypes(second), None);
    assert_eq!(s.stack_copiable_supertypes(second), None);
    assert_eq!(
        s.stack_copiable_supertypes(first),
        Some(&expected_supertypes)
    );
    assert_eq!(s.stack_copiable_names(first), Some(&expected));
    assert_eq!(s.stack_copiable_card_types(first), Some(&expected_types));
    assert_eq!(
        s.stack_copiable_creature_subtypes(first),
        Some(&expected_subtypes)
    );
}

#[test]
fn suspended_retarget_keeps_traits_atomic_and_emits_the_captured_copy_traits() {
    use spellwake_engine::{
        effects::RuleResult,
        game::DecisionReply,
        occurrence::{RulesOccurrence, RulesRecord},
    };
    let expected_supertypes = BTreeSet::from([Supertype::Snow]);
    let s = pending(stacked_with_traits(expected_supertypes.clone()));
    let original = s.window().entries()[0].id();
    let expected = s.stack_copiable_names(original).unwrap().clone();
    let progress = s.begin_resolution(s.revision()).unwrap();
    let RuleResult::Pending { frame, .. } = progress.result() else {
        panic!("expected copy target choice")
    };
    let state = progress.state();
    assert_eq!(state.stack_copiable_names(original), Some(&expected));
    assert_eq!(
        state.stack_copiable_supertypes(original),
        Some(&expected_supertypes)
    );
    assert!(state.window().spell_copies().is_empty());
    let saved = state.clone();
    assert!(
        state
            .resume_rule(
                state.revision(),
                frame,
                A,
                DecisionReply::CopyTargets(CopyTargetChoice::Retain),
                None
            )
            .is_err()
    );
    assert_eq!(state, &saved);
    let target = object(state, 6);
    let done = state
        .resume_rule(
            state.revision(),
            frame,
            B,
            DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Object(target))),
            None,
        )
        .unwrap();
    let copy = copy_id(done.state());
    assert_eq!(done.state().stack_copiable_names(copy), Some(&expected));
    assert_eq!(
        done.state().stack_copiable_supertypes(copy),
        Some(&expected_supertypes)
    );
    assert_eq!(
        done.state()
            .window()
            .entries()
            .last()
            .unwrap()
            .targets()
            .targets()[0]
            .choice()
            .target,
        TargetRef::Object(target)
    );
    let facts: Vec<_> = done
        .append()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(&event.occurrences),
            _ => None,
        })
        .flatten()
        .filter_map(|event| match &event.value {
            RulesOccurrence::CopyCreated {
                entry,
                copiable_names,
                copiable_supertypes,
                copiable_card_types,
                copiable_creature_subtypes,
                ..
            } => Some((
                *entry,
                copiable_names,
                copiable_supertypes,
                copiable_card_types,
                copiable_creature_subtypes,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        facts,
        vec![(
            copy,
            &expected,
            &expected_supertypes,
            &BTreeSet::from([CardType::Instant]),
            &BTreeSet::new()
        )]
    );
}
