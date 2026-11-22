use super::*;
use spellwake_engine::{
    characteristics::{
        AbilityChange, AbilityRemoval, KeywordAbility, ResolvedDuration, ResolvedPartSpec,
        ResolvedPartsSpec,
    },
    face_down::{FaceDownKind, FaceUpAbility},
    occurrence::{RulesOccurrence, RulesRecord},
};

fn fixture() -> GameState {
    let mut cards = seeds();
    let mut hidden = creature(
        100,
        A,
        ObjectDestination::Hand,
        Some(ManaCost::one(ManaType::Red)),
        6,
    );
    hidden.program.face_up_abilities =
        vec![FaceUpAbility::Megamorph(ManaCost::one(ManaType::Black))];
    hidden
        .program
        .printed_keywords
        .insert(KeywordAbility::Flying);
    cards.push(hidden);
    cards.push(spell(
        101,
        A,
        Instruction::CreateContinuousEffect {
            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                AbilityRemoval::All,
            ))]),
            duration: ResolvedDuration::UntilEndOfTurn,
        },
        ManaCost::free(),
        BodyTarget::SingleCreature,
    ));
    let mut split = spell(
        102,
        A,
        Instruction::ReturnToOwnersHand,
        ManaCost::free(),
        BodyTarget::SingleCreature,
    );
    split
        .program
        .printed_keywords
        .insert(KeywordAbility::SplitSecond);
    cards.push(split);
    let mut copier = creature(
        103,
        A,
        ObjectDestination::Battlefield { controller: A },
        None,
        4,
    );
    copier.program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        source_cost: SourceCost::None,
        mana_cost: ManaCost::free(),
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::CopyTargetToSource {
                duration: ResolvedDuration::NoStatedDuration,
                power_toughness: None,
            }],
        ),
    });
    cards.push(copier);
    setup(Some(Step::PrecombatMain), cards)
}
fn hide(game: &GameState) -> GameCommand {
    GameCommand::CastFaceDown {
        spell: object(game, 100),
        kind: FaceDownKind::Morph,
        payment: Some(payment(&[(ManaType::Colourless, 3)])),
    }
}
fn turn_up(game: &GameState) -> GameCommand {
    GameCommand::TurnFaceUp {
        permanent: object(game, 100),
        ability: 0,
        payment: Some(payment(&[(ManaType::Black, 1)])),
    }
}
fn reveals(candidate: &NativeCandidate) -> Vec<&GameObject> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| {
            if let RulesRecord::Event(event) = record {
                Some(event)
            } else {
                None
            }
        })
        .flat_map(|event| &event.occurrences)
        .filter_map(|record| match &record.value {
            RulesOccurrence::FaceDownRevealed { object } => Some(object),
            _ => None,
        })
        .collect()
}
#[test]
fn cast_payment_is_atomic_countering_reveals_while_permanent_resolution_preserves_secrecy() {
    let game = fixture();
    let command = |kind, payment| GameCommand::CastFaceDown {
        spell: object(&game, 100),
        kind,
        payment,
    };
    reject(
        &game,
        A,
        command(FaceDownKind::Disguise, None),
        EffectError::NotCastableSpell,
    );
    reject(
        &game,
        A,
        command(FaceDownKind::Morph, None),
        EffectError::ManaPaymentRequired,
    );
    assert!(physical(&game, 100).face_down().is_none());
    let cast = accept(&game, A, hide(&game));
    assert!(reveals(&cast).is_empty());
    let permanent_spell = cast.state().execution().window().entries()[0].id();
    let on_stack = physical(cast.state(), 100);
    let conceded = accept(cast.state(), B, GameCommand::Concede);
    assert_eq!(
        conceded.state().conclusion().unwrap().face_down_reveals(),
        std::slice::from_ref(on_stack)
    );
    let pass = pass(cast.state());
    let response = targeted(pass.state(), B, 11, TargetRef::Stack(permanent_spell));
    let countered = resolve(response.state());
    assert_eq!(reveals(&countered), vec![on_stack]);
    assert_eq!(
        physical(countered.state(), 100).zone(),
        ObjectZone::Graveyard(A)
    );
    assert!(physical(countered.state(), 100).face_down().is_none());
    let resolved = resolve(cast.state());
    assert!(reveals(&resolved).is_empty());
    let permanent = physical(resolved.state(), 100);
    assert_eq!(permanent.face_down(), Some(FaceDownKind::Morph));
    let bounce = targeted(resolved.state(), A, 13, TargetRef::Object(permanent.id()));
    let back = resolve(bounce.state());
    assert_eq!(reveals(&back), vec![permanent]);
    assert!(physical(back.state(), 100).face_down().is_none());
}
#[test]
fn copying_the_shell_then_turning_up_under_split_second_preserves_the_copy_and_original_epoch() {
    let game = fixture();
    let hidden = resolve(accept(&game, A, hide(&game)).state()).into_state();
    let permanent = object(&hidden, 100);
    let copier = object(&hidden, 103);
    let copy = accept(
        &hidden,
        A,
        GameCommand::ActivateAbility {
            source: copier,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(permanent),
            payment: None,
        },
    );
    let copied = resolve(copy.state()).into_state();
    assert_eq!(
        copied.execution().copiable_program(copier),
        Some(FaceDownKind::Morph.definition())
    );
    assert!(physical(&copied, 103).face_down().is_none());
    let epoch = physical(&copied, 100).control_epoch();
    let response = targeted(&copied, A, 102, TargetRef::Object(permanent));
    let revealed = accept(response.state(), A, turn_up(response.state()));
    assert_eq!(
        revealed.state().execution().window().entries(),
        response.state().execution().window().entries()
    );
    assert_eq!(physical(revealed.state(), 100).id(), permanent);
    assert_eq!(physical(revealed.state(), 100).control_epoch(), epoch);
    assert_eq!(
        revealed
            .state()
            .execution()
            .power_toughness_counters(permanent)
            .unwrap()
            .plus_one,
        1.into()
    );
    assert_eq!(
        revealed
            .state()
            .execution()
            .power_toughness(permanent)
            .unwrap()
            .unwrap()
            .derived
            .toughness,
        7.into()
    );
    assert_eq!(
        revealed.state().execution().copiable_program(copier),
        Some(FaceDownKind::Morph.definition())
    );
    let bounce = resolve(revealed.state());
    assert!(reveals(&bounce).is_empty());
}
#[test]
fn turn_up_uses_the_abilities_remaining_in_the_prospective_face_up_world() {
    let game = fixture();
    let hidden = resolve(accept(&game, A, hide(&game)).state()).into_state();
    let source = object(&hidden, 100);
    let erase = targeted(&hidden, A, 101, TargetRef::Object(source));
    let masked = resolve(erase.state()).into_state();
    assert!(
        masked
            .execution()
            .face_up_preview(source)
            .unwrap()
            .unwrap()
            .abilities
            .face_up
            .is_empty()
    );
    reject(
        &masked,
        A,
        turn_up(&masked),
        EffectError::UnknownActivatedAbility,
    );
    assert_eq!(
        physical(&masked, 100).face_down(),
        Some(FaceDownKind::Morph)
    );
}
