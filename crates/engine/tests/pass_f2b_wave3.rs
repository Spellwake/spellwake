#![allow(dead_code, unused_imports)]
//! Runtime integration tests for Pass F2-B Wave 3 primitives:
//! - `StaticLandSubtypeOperand::SourceEntryBasicLandType` (`Realmwright`)
//! - `EntryTap::ChooseOptions(&[...])` (`Barrensteppe Siege` / Tarkir modal entry options)
//! - `ResolvingPlayer::{AttachedHostController, TriggerSubjectController, TriggerSubjectOwner, SourceChosenPlayer}`
//! - Up-to-one target `ValueObjectRef::TargetObject` CR 107.2 zero fallback (`Brightmare`)
//! - `BlockingRestriction::AssignNoCombatDamage` (CR 510.1a)
//! - `BlockingRestriction::CannotBeRegenerated` (CR 701.19c)
//! - `CombatSelector::BlockingOrBlockedBySource` (`Alms Beast`).
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    attachment::{AttachmentKind, EnchantRestriction},
    characteristics::{
        AbilityChange, BlockingRestriction, CombatSelector, ControllerSelector, KeywordAbility,
        NumericExpression, NumericPair, PermanentSelector, ResolvedDuration, ResolvedPartSpec,
        ResolvedPartsSpec, StaticAbilityId, StaticContinuousAbility, StaticLandSubtypeOperand,
        StaticPart, StaticParts, StaticSourceCondition, StaticTypeChange, SubtypeOperation,
        TypeOperation,
    },
    combat::{AttackDeclaration, BlockDeclaration},
    effects::SeedCard,
    game::{DecisionReply, GameCommand, GameState, NativeCandidate, RuleFrame},
    land::{EntryChoice, EntryTap, LandType},
    mana::{ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    program::{
        AffectedPlayers, BodyTarget, CardType, ExecutableBody, Instruction, ObjectFilter,
        ObjectStatKind, PermanentDefinition, PermanentTargetAction, Program, ProgramKind,
        ResolvingPlayer, TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
        ValueExpr, ValueObjectRef,
    },
    targets::TargetRef,
    turn::Step,
};
use std::collections::BTreeSet;

fn cast_untargeted(game: &GameState, id: u64) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(game, id),
            payment: Some(ManaPayment::default()),
        },
    );
    pass_pair(cast.state())
}

fn cast_targeted(
    game: &GameState,
    actor: spellwake_engine::priority::SeatId,
    id: u64,
    target: TargetRef,
) -> NativeCandidate {
    let cast = submit(
        game,
        actor,
        GameCommand::CastPaid {
            spell: object(game, id),
            target,
            payment: ManaPayment::default(),
        },
    );
    let order = if actor == P0 { [P0, P1] } else { [P1, P0] };
    pass_in_order(cast.state(), order)
}

fn opp_battlefield(id: u64, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: P1,
        destination: ObjectDestination::Battlefield { controller: P1 },
        program,
    }
}

#[test]
fn source_entry_basic_land_type_grants_chosen_land_subtype_to_controlled_lands() {
    let vocab = vocabulary();
    let mut realmwright =
        Program::creature(Some(ManaCost::free()), 1.into(), 1.into(), BTreeSet::new());
    if let ProgramKind::Permanent(def) = &mut realmwright.kind {
        def.entry_tap = Some(EntryTap::ChooseBasicLandType);
    }
    realmwright.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            controller: ControllerSelector::SourceController,
            required_types: BTreeSet::from([CardType::Land]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
            land_subtypes: SubtypeOperation::Add(
                StaticLandSubtypeOperand::SourceEntryBasicLandType,
            ),
        })]),
    });

    let forest = Program::new(
        BTreeSet::from([CardType::Land]),
        BTreeSet::new(),
        BTreeSet::from([LandType::Forest]),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: None,
            casting_cost: None,
        }),
    );

    let game = world(
        vocab,
        vec![card(1, forest, true), card(2, realmwright, false)],
        vec![CardId(1)],
    );

    let forest_id = object(&game, 1);
    assert!(
        current(&game, forest_id)
            .current_land_subtypes
            .contains(&LandType::Forest)
    );
    assert!(
        !current(&game, forest_id)
            .current_land_subtypes
            .contains(&LandType::Island)
    );

    let pending = cast_untargeted(&game, 2);
    assert!(matches!(
        pending.state().pending_decision().map(|d| d.frame()),
        Some(RuleFrame::EntryChoice(frame))
            if frame.rule() == EntryTap::ChooseBasicLandType
    ));
    // Index 1 in ["Plains", "Island", "Swamp", "Mountain", "Forest"] is Island.
    let after = reply(
        pending.state(),
        P0,
        DecisionReply::EntryChoice(EntryChoice::OptionIndex(1)),
    );
    let view = current(after.state(), forest_id);
    assert!(view.current_land_subtypes.contains(&LandType::Forest));
    assert!(view.current_land_subtypes.contains(&LandType::Island));
}

#[test]
fn entry_tap_choose_options_gates_named_siege_modes() {
    let vocab = vocabulary();
    let mut siege = Program::enchantment(Some(ManaCost::free()));
    if let ProgramKind::Permanent(def) = &mut siege.kind {
        def.entry_tap = Some(EntryTap::ChooseOptions(&["Abzan", "Mardu"]));
    }
    // Option 0 ("Abzan"): creatures you control get +2/+2.
    siege.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            controller: ControllerSelector::SourceController,
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(2.into()),
            toughness: NumericExpression::Literal(2.into()),
        })]),
    });
    siege.static_conditions.insert(
        StaticAbilityId(1),
        StaticSourceCondition::EntryOptionChosen(0),
    );
    // Option 1 ("Mardu"): creatures you control get +4/+0.
    siege
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(2),
            selector: PermanentSelector {
                controller: ControllerSelector::SourceController,
                required_types: BTreeSet::from([CardType::Creature]),
                ..PermanentSelector::default()
            },
            parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(4.into()),
                toughness: NumericExpression::Literal(0.into()),
            })]),
        });
    siege.static_conditions.insert(
        StaticAbilityId(2),
        StaticSourceCondition::EntryOptionChosen(1),
    );

    let bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let game = world(
        vocab,
        vec![card(1, bear, true), card(2, siege, false)],
        vec![CardId(1)],
    );
    let bear_id = object(&game, 1);

    let pending = cast_untargeted(&game, 2);
    assert!(matches!(
        pending.state().pending_decision().map(|d| d.frame()),
        Some(RuleFrame::EntryChoice(frame))
            if frame.rule().option_labels() == Some(&["Abzan", "Mardu"][..])
    ));

    // Choosing option 0 (Abzan) activates +2/+2 -> 4/4.
    let abzan = reply(
        pending.state(),
        P0,
        DecisionReply::EntryChoice(EntryChoice::OptionIndex(0)),
    );
    let abzan_pt = current(abzan.state(), bear_id)
        .power_toughness
        .expect("creature pt");
    assert_eq!(abzan_pt.derived.power, 4.into());
    assert_eq!(abzan_pt.derived.toughness, 4.into());

    // Choosing option 1 (Mardu) activates +4/+0 -> 6/2.
    let mardu = reply(
        pending.state(),
        P0,
        DecisionReply::EntryChoice(EntryChoice::OptionIndex(1)),
    );
    let mardu_pt = current(mardu.state(), bear_id)
        .power_toughness
        .expect("creature pt");
    assert_eq!(mardu_pt.derived.power, 6.into());
    assert_eq!(mardu_pt.derived.toughness, 2.into());
}

#[test]
fn trigger_subject_controller_resolves_dying_creature_controller_lki() {
    let vocab = vocabulary();
    // Watcher: Whenever a creature dies, its controller loses 3 life.
    let mut watcher = Program::enchantment(None);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::dies(ObjectFilter::creature()),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustPlayersLife {
                players: AffectedPlayers::One(ResolvingPlayer::TriggerSubjectController),
                gain: false,
                amount: ValueExpr::literal(3),
            }],
        ),
    });

    let opp_bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let destroy = spell(
        3,
        BodyTarget::SingleCreature,
        vec![Instruction::DestroyTarget],
    );

    let game = world(
        vocab,
        vec![
            card(1, watcher, true),
            opp_battlefield(2, opp_bear),
            destroy,
        ],
        vec![CardId(1), CardId(2)],
    );

    let opp_bear_id = object(&game, 2);
    let p0_before = game.execution().life_total(P0).cloned().expect("p0 life");
    let p1_before = game.execution().life_total(P1).cloned().expect("p1 life");

    // Cast destroy targeting P1's bear, resolve spell, then resolve the dies trigger.
    let after_spell = cast_targeted(&game, P0, 3, TargetRef::Object(opp_bear_id));
    let after_trigger = pass_pair(after_spell.state());
    assert_eq!(
        after_trigger.state().execution().life_total(P0),
        Some(&p0_before)
    );
    assert_eq!(
        after_trigger.state().execution().life_total(P1),
        Some(&p1_before.minus(&3.into()))
    );
}

#[test]
fn source_chosen_player_resolves_entry_opponent_choice() {
    let vocab = vocabulary();
    // Enchantment: As ~ enters, choose an opponent. When ~ enters, chosen player loses 4 life.
    let mut curse = Program::enchantment(Some(ManaCost::free()));
    if let ProgramKind::Permanent(def) = &mut curse.kind {
        def.entry_tap = Some(EntryTap::ChooseOpponent);
    }
    curse.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::enters(ObjectFilter::self_object()),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustPlayersLife {
                players: AffectedPlayers::One(ResolvingPlayer::SourceChosenPlayer),
                gain: false,
                amount: ValueExpr::literal(4),
            }],
        ),
    });

    let game = world(vocab, vec![card(1, curse, false)], vec![]);
    let p1_before = game.execution().life_total(P1).cloned().expect("p1 life");
    let pending = cast_untargeted(&game, 1);
    assert!(matches!(
        pending.state().pending_decision().map(|d| d.frame()),
        Some(RuleFrame::EntryChoice(frame)) if frame.rule() == EntryTap::ChooseOpponent
    ));
    let entered = reply(
        pending.state(),
        P0,
        DecisionReply::EntryChoice(EntryChoice::Opponent(P1)),
    );
    let resolved = pass_pair(entered.state());
    assert_eq!(
        resolved.state().execution().life_total(P1),
        Some(&p1_before.minus(&4.into()))
    );
}

#[test]
fn attached_host_controller_resolves_enchanted_permanents_controller() {
    let vocab = vocabulary();
    // Aura: Enchant creature. When ~ enters, enchanted creature's controller loses 2 life.
    let mut aura = Program::new(
        BTreeSet::from([CardType::Enchantment]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: Some(AttachmentKind::Aura(EnchantRestriction::Creature)),
            printed_pt: None,
            casting_cost: Some(ManaCost::free()),
        }),
    );
    aura.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::enters(ObjectFilter::self_object()),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustPlayersLife {
                players: AffectedPlayers::One(ResolvingPlayer::AttachedHostController),
                gain: false,
                amount: ValueExpr::literal(2),
            }],
        ),
    });

    let opp_bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let game = world(
        vocab,
        vec![opp_battlefield(1, opp_bear), card(2, aura, false)],
        vec![CardId(1)],
    );

    let opp_bear_id = object(&game, 1);
    let p1_before = game.execution().life_total(P1).cloned().expect("p1 life");
    let entered = cast_targeted(&game, P0, 2, TargetRef::Object(opp_bear_id));
    let resolved = pass_pair(entered.state());
    assert_eq!(
        resolved.state().execution().life_total(P1),
        Some(&p1_before.minus(&2.into()))
    );
}

#[test]
fn up_to_one_target_object_stat_evaluates_power_or_zero_when_untargeted() {
    let vocab = vocabulary();
    // Brightmare-style ability: Tap up to one target creature. You gain life equal to its power.
    let mut brightmare = Program::artifact(None, vec![]);
    brightmare.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![
                Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: 1,
                    action: PermanentTargetAction::Tap,
                },
                Instruction::GainControllerLife {
                    amount: ValueExpr::ObjectStat {
                        subject: ValueObjectRef::TargetObject,
                        stat: ObjectStatKind::Power,
                    },
                },
            ],
        ),
    });
    let fatty = Program::creature(None, 4.into(), 4.into(), BTreeSet::new());
    let game = world(
        vocab,
        vec![card(1, fatty, true), card(2, brightmare, true)],
        vec![CardId(1), CardId(2)],
    );
    let p0_before = game.execution().life_total(P0).cloned().expect("p0 life");
    let fatty_id = object(&game, 1);
    let source_id = object(&game, 2);

    // Case 1: activate with 1 target (4/4) -> taps it and gains 4 life.
    let act_one = submit(
        &game,
        P0,
        GameCommand::ActivatePermanentTargets {
            source: source_id,
            ability: ActivatedAbilityId(1),
            targets: vec![TargetRef::Object(fatty_id)],
            payment: Some(ManaPayment::default()),
        },
    );
    let with_target = pass_pair(act_one.state());
    assert_eq!(
        with_target.state().execution().life_total(P0),
        Some(&p0_before.plus(&4.into()))
    );
    assert_eq!(
        with_target
            .state()
            .execution()
            .window()
            .cards()
            .get(fatty_id)
            .and_then(|c| c.tapped()),
        Some(true)
    );

    // Case 2: activate with 0 targets -> resolves cleanly and gains 0 life (CR 107.2).
    let act_zero = submit(
        &game,
        P0,
        GameCommand::ActivatePermanentTargets {
            source: source_id,
            ability: ActivatedAbilityId(1),
            targets: vec![],
            payment: Some(ManaPayment::default()),
        },
    );
    let without_target = pass_pair(act_zero.state());
    assert_eq!(
        without_target.state().execution().life_total(P0),
        Some(&p0_before)
    );
    assert_eq!(
        without_target
            .state()
            .execution()
            .window()
            .cards()
            .get(fatty_id)
            .and_then(|c| c.tapped()),
        Some(false)
    );
}

#[test]
fn combat_selector_blocking_or_blocked_by_source_grants_lifelink_only_to_engaged_creatures() {
    let vocab = vocabulary();
    // Alms Beast: 6/6 with Haste; creatures blocking or blocked by ~ have lifelink.
    let mut alms_beast = Program::creature(None, 6.into(), 6.into(), BTreeSet::new());
    alms_beast.printed_keywords.insert(KeywordAbility::Haste);
    alms_beast.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            combat_role: Some(CombatSelector::BlockingOrBlockedBySource),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
            [KeywordAbility::Lifelink],
        ))]),
    });

    let blocker = Program::creature(None, 3.into(), 3.into(), BTreeSet::new());
    let bystander = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());

    let game = world(
        vocab,
        vec![
            card(1, alms_beast, true),
            opp_battlefield(2, blocker),
            opp_battlefield(3, bystander),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );

    let beast_id = object(&game, 1);
    let blocker_id = object(&game, 2);
    let bystander_id = object(&game, 3);

    // Before combat: neither blocker nor bystander has Lifelink.
    assert!(
        !current(&game, blocker_id)
            .keywords
            .contains(&KeywordAbility::Lifelink)
    );
    assert!(
        !current(&game, bystander_id)
            .keywords
            .contains(&KeywordAbility::Lifelink)
    );

    // Advance to DeclareAttackers and attack with Alms Beast.
    let beg_combat = pass_pair(&game);
    let dec_attackers = pass_pair(beg_combat.state());
    let declared = reply(
        dec_attackers.state(),
        P0,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: beast_id,
            defender: P1,
            defending_planeswalker: None,
        }]),
    );
    let dec_blockers = pass_pair(declared.state());
    let blocked = reply(
        dec_blockers.state(),
        P1,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker: blocker_id,
            attacker: beast_id,
        }]),
    );

    // During combat after blockers declared: blocker has Lifelink, bystander does not.
    assert!(
        current(blocked.state(), blocker_id)
            .keywords
            .contains(&KeywordAbility::Lifelink),
        "creature blocking Alms Beast must gain Lifelink"
    );
    assert!(
        !current(blocked.state(), bystander_id)
            .keywords
            .contains(&KeywordAbility::Lifelink),
        "uninvolved creature must not gain Lifelink"
    );
}
