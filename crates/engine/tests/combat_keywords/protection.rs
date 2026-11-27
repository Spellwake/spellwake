#![allow(dead_code, unused_imports)]
//! Prevention uses the affected permanent's controller and the actual (possibly departed) source.
use super::keyword_damage::{blocks, cast_on, change, combat_creature};
use super::*;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        AbilityChange, Color, ColorOperation,
        KeywordAbility::{Infect, Lifelink, ProtectionFromBlue, ProtectionFromRed},
        ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    },
    damage::DamageEffectId,
    effects::CopyTargetChoice,
    mana::ManaPayment,
};
fn block_protection() -> SeedCard {
    instant(
        3,
        A,
        change(AbilityChange::grant_keywords([ProtectionFromRed])),
    )
}
fn select(state: &GameState, matches: impl Fn(DamageEffectId) -> bool) -> NativeCandidate {
    let RuleFrame::Damage(DamageDecision::Order(f)) = state.pending_decision().unwrap().frame()
    else {
        panic!("order of competing damage effects")
    };
    let selected = f
        .applications()
        .iter()
        .copied()
        .find(|a| matches(a.effect))
        .unwrap();
    reply(state, DecisionReply::DamageOrder(selected))
}

#[test]
fn protection_competes_with_shields_and_multipliers_and_prevents_every_damage_result() {
    let mut attacker = combat_creature(1, A, 3, 5, &[Lifelink, Infect]);
    attacker.program.color_indicator = Some([Color::Red].into());
    let mut state = blocks(
        vec![
            attacker,
            combat_creature(2, B, 0, 8, &[]),
            block_protection(),
            instant(
                4,
                A,
                Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec {
                        selector: None,
                        payload:
                            spellwake_engine::toolbox::ContinuousPayload::TargetPreventionShield {
                                amount: 2.into(),
                            },
                        duration:
                            spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                        condition: None,
                        one_shot: true,
                        as_emblem: None,
                        emblem_label: None,
                    },
                )),
            ),
            instant(
                5,
                A,
                Instruction::CreateDamageMultiplier { factor: 2.into() },
            ),
        ],
        &[(2, 1)],
    );
    for spell in [3, 4, 5] {
        state = cast_on(&state, spell, 2);
    }
    let order = round(&state).into_state();
    assert_eq!(order.pending_decision().unwrap().chooser(), B);
    let prevented = select(&order, |e| e == DamageEffectId::Protection(Color::Red));
    assert_eq!(
        prevented
            .state()
            .execution()
            .prevention_shields()
            .next()
            .unwrap()
            .remaining,
        2.into()
    );
    assert_eq!(
        prevented.damage_journal().protections[0].allocations[0].amount,
        3.into()
    );
    let multiplied = select(&order, |e| matches!(e, DamageEffectId::Multiplier(_)));
    let shielded = select(multiplied.state(), |e| {
        matches!(e, DamageEffectId::Prevention(_))
    });
    assert!(
        shielded
            .state()
            .execution()
            .prevention_shields()
            .next()
            .is_none()
    );
    assert_eq!(
        shielded.damage_journal().protections[0].allocations[0].amount,
        4.into()
    );
    for candidate in [&prevented, &shielded] {
        let source = object(candidate.state(), 2).id();
        assert!(candidate.damage_journal().applied.is_empty());
        assert_eq!(
            candidate.state().execution().life_total(A),
            Some(&20.into())
        );
        assert_eq!(
            candidate
                .state()
                .execution()
                .power_toughness_counters(source)
                .unwrap()
                .minus_one,
            0.into()
        );
    }
}

#[test]
fn protection_filters_sources_by_color_in_a_simultaneous_damage_batch() {
    let mut seeds = vec![
        combat_creature(1, A, 0, 12, &[]),
        instant(
            5,
            A,
            change(AbilityChange::grant_keywords([
                ProtectionFromBlue,
                ProtectionFromRed,
            ])),
        ),
    ];
    for (id, power, color) in [
        (2, 2, Color::Red),
        (3, 3, Color::Blue),
        (4, 4, Color::Green),
    ] {
        let mut blocker = combat_creature(id, B, power, 5, &[]);
        blocker.program.color_indicator = Some([color].into());
        seeds.push(blocker);
    }
    let blocked = blocks(seeds, &[(2, 1), (3, 1), (4, 1)]);
    let protected = cast_on(&blocked, 5, 1);
    let pending = round(&protected);
    assert_eq!(pending.state().pending_decision().unwrap().chooser(), A);
    let finished = select(pending.state(), |e| {
        e == DamageEffectId::Protection(Color::Red)
    });
    assert_eq!(finished.damage_journal().protections.len(), 2);
    assert_eq!(
        finished.damage_journal().protections[0].allocations[0].amount,
        2.into()
    );
    assert_eq!(
        finished.damage_journal().protections[1].allocations[0].amount,
        3.into()
    );
    assert_eq!(
        finished
            .state()
            .execution()
            .battlefield_marked_damage(object(finished.state(), 1).id()),
        Some(4.into())
    );
}
fn body() -> ExecutableBody {
    ExecutableBody::new(
        BodyTarget::SingleCreature,
        vec![
            change(AbilityChange::grant_keywords([ProtectionFromRed])),
            spellwake_engine::program::Instruction::DealDamage(Box::new(
                spellwake_engine::toolbox::DealDamageSpec {
                    source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                    recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    amount: spellwake_engine::program::DamageAmountSpec::Expr(
                        spellwake_engine::program::ValueExpr::Literal(3.into()),
                    ),
                    exile_if_dies: false,
                },
            )),
        ],
    )
}

#[test]
fn an_instruction_spell_copy_uses_the_copied_spells_color_for_damage() {
    let mut source = instant(
        3,
        A,
        Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(
            spellwake_engine::toolbox::ObjectQuery::Targets,
            spellwake_engine::toolbox::ZoneDest::Graveyard {
                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                    allow_regenerate: true,
                },
            },
        ))),
    );
    source.program = Program::instant(ManaCost::free(), body());
    source.program.color_indicator = Some([Color::Red].into());
    let copier = SeedCard {
        card: CardId(4),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                ))],
            ),
        ),
    };
    let game = setup(
        vec![A, B],
        vec![battlefield(1, B), battlefield(2, B), source, copier],
        Step::PrecombatMain,
    )
    .into_state();
    let original = act(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 3).id(),
            target: TargetRef::Object(object(&game, 1).id()),
        },
    );
    let copy = act(
        original.state(),
        A,
        GameCommand::Cast {
            spell: object(original.state(), 4).id(),
            target: TargetRef::Stack(original.state().execution().window().entries()[0].id()),
        },
    );
    let choosing = round(copy.state());
    let copied = reply(
        choosing.state(),
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Object(
            object(&game, 2).id(),
        ))),
    );
    let finished = round(copied.state());
    assert_eq!(
        finished.damage_journal().protections[0].quality,
        spellwake_engine::damage::ProtectionQuality::Color(
            spellwake_engine::characteristics::ColorFilter::Has(Color::Red)
        )
    );
    assert!(finished.damage_journal().applied.is_empty());
    assert_eq!(
        finished
            .state()
            .execution()
            .battlefield_marked_damage(object(&game, 2).id()),
        Some(0.into())
    );
    let original_done = round(finished.state());
    assert!(original_done.damage_journal().applied.is_empty());
    assert_eq!(
        original_done.damage_journal().protections[0].quality,
        spellwake_engine::damage::ProtectionQuality::Color(
            spellwake_engine::characteristics::ColorFilter::Has(Color::Red)
        )
    );
}
