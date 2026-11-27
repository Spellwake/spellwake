//! A released Siege and a Commander Masters activated ability in a three-player native game.
use super::declarations::{attacks, blocks, combat_on, invalid};
use super::equipment::{marker, optional, sheet};
use super::*;
use spellwake_engine::{damage::DamageRecipient, effects::CounterKind, land::EntryChoice};

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_siege_spikeshot_goblin_hits_invasion_of_dominaria_then_combat_defeats_and_casts_serra_faithkeeper()
 {
    use ManaType::*;
    let goblin = named("Spikeshot Goblin", 2, A, battlefield(A));
    let ability = goblin.program.activated[0].id;
    let price = [(Colourless, 2), (White, 1)];
    let start = game(
        vec![A, B, C],
        vec![
            named(
                "Invasion of Dominaria // Serra Faithkeeper",
                1,
                A,
                ObjectDestination::Hand,
            ),
            goblin,
            named("Lightning Greaves", 6, A, battlefield(A)),
            creature(3, A, 4, 3, &[KeywordAbility::Haste]),
            creature(4, C, 1, 2, &[]),
            creature(5, B, 1, 2, &[]),
        ],
        &[(Colourless, 2), (White, 1), (Red, 1)],
        Step::PrecombatMain,
    );
    let cast = cast_untargeted(&start, 1, &price);
    let entering = round(cast.into_state()).into_state();
    let protect = |seat| DecisionReply::EntryChoice(EntryChoice::Opponent(seat));
    invalid(&entering, A, protect(A));
    let field = reply(&entering, protect(B)).into_state();
    let battle = physical(&field, 1).id();
    assert_eq!(
        field.execution().permanent_defender(&sheet(&field, battle)),
        Some(B)
    );
    assert_eq!(
        field
            .execution()
            .counters_of_kind(battle, CounterKind::Defense),
        5.into()
    );
    let field = round(field).into_state();
    assert_eq!(field.execution().life_total(A), Some(&24.into()));
    assert_eq!(physical(&field, 100).zone(), ObjectZone::Hand(A));
    let ping = GameCommand::ActivateAbility {
        source: physical(&field, 2).id(),
        ability,
        target: marker(&field, 1),
        payment: Some(spend(&[(Red, 1)])),
    };
    illegal(&field, A, ping.clone());
    let wielding = super::equipment::wield(&field, physical(&field, 6).id(), 2, 0);
    assert!(sheet(&wielding, physical(&wielding, 2).id()).has_keyword(KeywordAbility::Haste));
    let hit = round(command(&wielding, A, ping).into_state());
    assert_eq!(
        hit.damage_journal().applied[0].target,
        DamageRecipient::Battle(battle)
    );
    assert_eq!(
        hit.state()
            .execution()
            .counters_of_kind(battle, CounterKind::Defense),
        4.into()
    );
    let attack = combat_on(hit.into_state(), 1);
    invalid(&attack, A, attacks(&attack, &[(3, C, Some(1))]));
    let attackers = reply(&attack, attacks(&attack, &[(3, B, Some(1))])).into_state();
    let blocking = exile_hand::until(attackers, |state| {
        matches!(
            state.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::Combat(CombatDecision::Blockers(_)))
        )
    });
    assert_eq!(blocking.pending_decision().unwrap().chooser(), B);
    invalid(&blocking, B, blocks(&blocking, &[(4, 3)]));
    let blocked = reply(&blocking, blocks(&blocking, &[(5, 3)])).into_state();
    let survived = exile_hand::until(blocked, |state| {
        state.turn().unwrap().position().step == Step::PostcombatMain
    });
    assert_eq!(
        survived
            .execution()
            .counters_of_kind(battle, CounterKind::Defense),
        4.into()
    );
    let unblocked = reply(&blocking, DecisionReply::DeclareBlockers(vec![])).into_state();
    let defeated = exile_hand::until(unblocked, |state| {
        state
            .execution()
            .counters_of_kind(battle, CounterKind::Defense)
            == 0.into()
            && !state.execution().window().entries().is_empty()
    });
    assert_eq!(physical(&defeated, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(defeated.execution().life_total(B), Some(&20.into()));
    let may = round(defeated).into_state();
    assert_eq!(physical(&may, 1).zone(), ObjectZone::Exile);
    let decline = optional(&may, false);
    assert_eq!(physical(&decline, 1).zone(), ObjectZone::Exile);
    let cast = optional(&may, true);
    assert_eq!(physical(&cast, 1).zone(), ObjectZone::Stack);
    assert!(physical(&cast, 1).transformed());
    let done = round(cast).into_state();
    let angel = physical(&done, 1);
    assert_eq!(angel.zone(), ObjectZone::Battlefield);
    assert!(angel.transformed());
    let view = sheet(&done, angel.id());
    assert!(view.is_creature());
    assert!(view.has_keyword(KeywordAbility::Flying));
    assert!(view.has_keyword(KeywordAbility::Vigilance));
    assert_eq!(
        done.execution()
            .counters_of_kind(angel.id(), CounterKind::Defense),
        0.into()
    );
    let pt = view.power_toughness.unwrap().derived;
    assert_eq!((pt.power, pt.toughness), (4.into(), 4.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_siege_protector_who_gains_control_must_choose_another_opponent() {
    let steal = free_instant(
        BodyTarget::SINGLE_PERMANENT,
        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
            spellwake_engine::toolbox::ObjectQuery::Targets,
            ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                ControlRecipient::ResolvingController,
            )]),
            ResolvedDuration::NoStatedDuration,
        ),
    );
    let price = [(ManaType::Colourless, 2), (ManaType::White, 1)];
    let start = game(
        vec![A, B, C],
        vec![
            named(
                "Invasion of Dominaria // Serra Faithkeeper",
                1,
                A,
                ObjectDestination::Hand,
            ),
            seed(2, B, ObjectDestination::Hand, steal),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entering = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    let entered = reply(
        &entering,
        DecisionReply::EntryChoice(EntryChoice::Opponent(B)),
    )
    .into_state();
    let field = round(entered).into_state();
    let battle = physical(&field, 1).id();
    let priority = pass(&field);
    let cast = targeted(priority.state(), B, 2, TargetRef::Object(battle));
    let choosing = round(cast.into_state()).into_state();
    let RuleFrame::Checks(spellwake_engine::checks::CheckDecision::BattleProtector(frame)) =
        choosing.pending_decision().unwrap().frame()
    else {
        panic!("new Battle controller must choose its protector")
    };
    assert_eq!(frame.battle(), battle);
    assert_eq!(
        frame.domain().iter().copied().collect::<BTreeSet<_>>(),
        [A, C].into()
    );
    invalid(&choosing, B, DecisionReply::OpponentChoice(B));
    invalid(&choosing, A, DecisionReply::OpponentChoice(C));
    for protector in [A, C] {
        let changed = reply(&choosing, DecisionReply::OpponentChoice(protector)).into_state();
        assert_eq!(physical(&changed, 1).controller(), Some(B));
        assert_eq!(
            changed.execution().battle_protector(battle),
            Some(protector)
        );
        assert_eq!(
            changed
                .execution()
                .counters_of_kind(battle, CounterKind::Defense),
            5.into()
        );
    }
}
