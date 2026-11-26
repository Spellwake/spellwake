//! Equipment and Aura scenarios through the public cast, activation and choice commands.
use super::*;
mod remaining;
use spellwake_engine::characteristics::ObjectCharacteristics;
use spellwake_engine::effects::ResolutionPaymentReply;

pub(super) fn sheet(state: &GameState, id: ObjectId) -> ObjectCharacteristics {
    state
        .execution()
        .current_characteristics(id)
        .unwrap()
        .unwrap()
}
pub(super) fn stats(state: &GameState, id: ObjectId, p: i64, t: i64) {
    assert_eq!(
        sheet(state, id).power_toughness.unwrap().derived,
        PowerToughness {
            power: p.into(),
            toughness: t.into()
        }
    );
}
pub(super) fn marker(state: &GameState, id: u64) -> TargetRef {
    TargetRef::Object(physical(state, id).id())
}
fn equip(state: &GameState, equipment: ObjectId, host: u64, cost: i64) -> GameCommand {
    GameCommand::ActivateAbility {
        source: equipment,
        ability: ActivatedAbilityId(1),
        target: marker(state, host),
        payment: (cost != 0).then(|| spend(&[(ManaType::Colourless, cost)])),
    }
}
pub(super) fn wield(state: &GameState, equipment: ObjectId, host: u64, cost: i64) -> GameState {
    let result =
        round(command(state, A, equip(state, equipment, host, cost)).into_state()).into_state();
    assert_eq!(
        result.execution().attached_to(equipment),
        Some(marker(&result, host))
    );
    result
}
pub(super) fn cast_card(state: &GameState, n: u64, price: &[(ManaType, i64)]) -> GameState {
    round(cast_untargeted(state, n, price).into_state()).into_state()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_all_that_glitters_and_champion_of_the_flame_recalculate_their_distinct_counts_on_attachment_and_departure()
 {
    use ManaType::*;
    let mut dual = Program::enchantment(None);
    dual.card_types.insert(CardType::Artifact);
    let cards = vec![
        named("All That Glitters", 1, A, ObjectDestination::Hand),
        named("Champion of the Flame", 2, A, ObjectDestination::Hand),
        named("Bonesplitter", 3, A, battlefield(A)),
        seed(4, A, battlefield(A), dual),
        named("Bonesplitter", 5, B, battlefield(B)),
        creature(6, B, 1, 1, &[]),
        seed(
            7,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SinglePermanent,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: true,
                            },
                        },
                    ),
                ),
            ),
        ),
    ];
    let initial = game(
        vec![A, B],
        cards,
        &[(Colourless, 3), (Red, 1), (White, 1)],
        Step::PrecombatMain,
    );
    let champion = cast_card(&initial, 2, &[(Colourless, 1), (Red, 1)]);
    let champion_id = physical(&champion, 2).id();
    stats(&champion, champion_id, 1, 1);
    assert!(sheet(&champion, champion_id).has_keyword(KeywordAbility::Trample));
    let cast = |n| GameCommand::CastPaid {
        spell: physical(&champion, 1).id(),
        target: marker(&champion, n),
        payment: spend(&[(Colourless, 1), (White, 1)]),
    };
    illegal(&champion, A, cast(3));
    let opposing = round(command(&champion, A, cast(6)).into_state()).into_state();
    stats(&opposing, physical(&opposing, 6).id(), 4, 4);
    stats(&opposing, champion_id, 1, 1);
    let aura = round(command(&champion, A, cast(2)).into_state()).into_state();
    stats(&aura, champion_id, 6, 6);
    let wielded = wield(&aura, physical(&aura, 3).id(), 2, 1);
    stats(&wielded, champion_id, 10, 8);
    let gone = round(targeted(&wielded, A, 7, marker(&wielded, 1)).into_state()).into_state();
    stats(&gone, champion_id, 5, 3);
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_champions_helm_grants_hexproof_only_on_a_legendary_host_and_transfers_the_bonus() {
    use ManaType::*;
    let mut legend = creature(2, A, 2, 3, &[]);
    legend
        .program
        .supertypes
        .insert(spellwake_engine::program::Supertype::Legendary);
    let cards = vec![
        named("Champion's Helm", 1, A, ObjectDestination::Hand),
        legend,
        creature(3, A, 2, 2, &[]),
        creature(4, B, 2, 2, &[]),
        super::super::cmm_gameplay::remove_spell(5, B),
    ];
    let initial = game(vec![A, B], cards, &[(Colourless, 5)], Step::PrecombatMain);
    let ready = cast_card(&initial, 1, &[(Colourless, 3)]);
    let helmet = physical(&ready, 1).id();
    illegal(&ready, A, equip(&ready, helmet, 4, 1));
    let first = wield(&ready, helmet, 2, 1);
    stats(&first, physical(&first, 2).id(), 4, 5);
    assert!(sheet(&first, physical(&first, 2).id()).has_keyword(KeywordAbility::Hexproof));
    let reaction = pass(&first).into_state();
    illegal(
        &reaction,
        B,
        GameCommand::Cast {
            spell: physical(&reaction, 5).id(),
            target: marker(&reaction, 2),
        },
    );
    let moved = wield(&first, helmet, 3, 1);
    stats(&moved, physical(&moved, 3).id(), 4, 4);
    stats(&moved, physical(&moved, 2).id(), 2, 3);
    assert!(!sheet(&moved, physical(&moved, 3).id()).has_keyword(KeywordAbility::Hexproof));
    let reaction = pass(&moved).into_state();
    let removed = round(targeted(&reaction, B, 5, marker(&reaction, 3)).into_state()).into_state();
    assert_eq!(physical(&removed, 3).zone(), ObjectZone::Graveyard(A));
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ancestral_blade_and_dwarven_hammer_create_and_attach_the_printed_tokens_then_equip_another_creature()
 {
    use ManaType::*;
    for (name, price, equip_cost, tribes, color, base, bonus, tramples) in [
        (
            "Ancestral Blade",
            vec![(Colourless, 1), (White, 1)],
            1,
            vec!["Soldier"],
            Color::White,
            (1, 1),
            (1, 1),
            false,
        ),
        (
            "Dwarven Hammer",
            vec![(Colourless, 2), (Red, 1)],
            3,
            vec!["Dwarf", "Berserker"],
            Color::Red,
            (2, 1),
            (3, 0),
            true,
        ),
    ] {
        let mana = vec![(Colourless, price[0].1 + equip_cost + 2), price[1]];
        let initial = game(
            vec![A, B],
            vec![
                named(name, 1, A, ObjectDestination::Hand),
                creature(2, A, 2, 3, &[]),
                creature(3, B, 1, 1, &[]),
            ],
            &mana,
            Step::PrecombatMain,
        );
        let entered = cast_card(&initial, 1, &price);
        assert_eq!(entered.execution().window().entries().len(), 1);
        let effect = round(entered);
        let created = if tramples {
            let no = reply(
                effect.state(),
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
            )
            .into_state();
            assert_eq!(no.execution().attached_to(physical(&no, 1).id()), None);
            assert!(!no.execution().window().cards().iter().any(|c| c.is_token()));
            reply(
                effect.state(),
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(&[(
                    Colourless, 2,
                )]))),
            )
        } else {
            effect
        };
        let token = tokens(&created, A, 1)[0];
        let result = created.into_state();
        let equipment = physical(&result, 1).id();
        assert_eq!(
            result.execution().attached_to(equipment),
            Some(TargetRef::Object(token))
        );
        let s = sheet(&result, token);
        assert_eq!(s.current_colors, [color].into());
        assert_eq!(
            s.current_creature_subtypes,
            tribes
                .into_iter()
                .map(|x| current_catalogue().1.resolve_label(x).unwrap())
                .collect()
        );
        stats(&result, token, base.0 + bonus.0, base.1 + bonus.1);
        assert_eq!(s.has_keyword(KeywordAbility::Trample), tramples);
        illegal(&result, A, equip(&result, equipment, 3, equip_cost));
        let moved = wield(&result, equipment, 2, equip_cost);
        stats(&moved, token, base.0, base.1);
        stats(&moved, physical(&moved, 2).id(), 2 + bonus.0, 3 + bonus.1);
        assert_eq!(
            sheet(&moved, physical(&moved, 2).id()).has_keyword(KeywordAbility::Trample),
            tramples
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_brass_knuckles_copies_its_permanent_spell_and_balan_moves_only_owned_equipment_to_reach_double_strike()
 {
    use ManaType::*;
    let balan = named("Balan, Wandering Knight", 1, A, ObjectDestination::Hand);
    assert_eq!(balan.program.activated.len(), 1);
    let ability = balan.program.activated[0].id;
    let initial = game(
        vec![A, B],
        vec![
            balan,
            named("Brass Knuckles", 2, A, ObjectDestination::Hand),
            seed(
                5,
                A,
                battlefield(A),
                Program::equipment(None, ManaCost::free()),
            ),
            seed(
                6,
                A,
                battlefield(A),
                Program::equipment(None, ManaCost::free()),
            ),
            creature(3, A, 2, 2, &[]),
            named("Bonesplitter", 4, B, battlefield(B)),
        ],
        &[(Colourless, 9), (White, 3)],
        Step::PrecombatMain,
    );
    let ready = cast_card(&initial, 1, &[(Colourless, 2), (White, 2)]);
    let knight = physical(&ready, 1).id();
    assert!(sheet(&ready, knight).has_keyword(KeywordAbility::FirstStrike));
    assert!(!sheet(&ready, knight).has_keyword(KeywordAbility::DoubleStrike));
    stats(&ready, knight, 3, 3);
    let single = wield(&ready, physical(&ready, 5).id(), 1, 0);
    assert!(!sheet(&single, knight).has_keyword(KeywordAbility::DoubleStrike));
    let pair = wield(&single, physical(&single, 6).id(), 1, 0);
    assert!(sheet(&pair, knight).has_keyword(KeywordAbility::DoubleStrike));
    let cast = cast_untargeted(&ready, 2, &[(Colourless, 4)]).into_state();
    assert_eq!(cast.execution().window().entries().len(), 2);
    let copied = round(cast).into_state();
    assert_eq!(copied.execution().window().entries().len(), 2);
    let copy = round(copied).into_state();
    let token = copy
        .execution()
        .window()
        .cards()
        .iter()
        .find(|c| c.is_token() && c.zone() == ObjectZone::Battlefield)
        .unwrap()
        .id();
    assert_eq!(copy.execution().window().entries().len(), 1);
    let done = round(copy).into_state();
    let original = physical(&done, 2).id();
    let one = wield(&done, original, 3, 1);
    let host = physical(&one, 3).id();
    assert!(!sheet(&one, host).has_keyword(KeywordAbility::DoubleStrike));
    let two = wield(&one, token, 3, 1);
    assert!(sheet(&two, host).has_keyword(KeywordAbility::DoubleStrike));
    let moved = round(
        command(
            &two,
            A,
            GameCommand::ActivateUntargetedAbility {
                source: knight,
                ability,
                payment: Some(spend(&[(Colourless, 1), (White, 1)])),
            },
        )
        .into_state(),
    )
    .into_state();
    let all = [
        original,
        token,
        physical(&moved, 5).id(),
        physical(&moved, 6).id(),
    ];
    let RuleFrame::ResolutionSelect(order) = moved.pending_decision().unwrap().frame() else {
        panic!("mandatory equipment selection");
    };
    assert_eq!(
        order.domain().iter().copied().collect::<BTreeSet<_>>(),
        all.into_iter().collect()
    );
    super::declarations::invalid(&moved, A, DecisionReply::SelectResolution(vec![original]));
    let moved = reply(&moved, DecisionReply::SelectResolution(all.to_vec())).into_state();
    for id in all {
        assert_eq!(
            moved.execution().attached_to(id),
            Some(TargetRef::Object(knight))
        );
    }
    assert_eq!(
        moved.execution().attached_to(physical(&moved, 4).id()),
        None
    );
    assert!(sheet(&moved, knight).has_keyword(KeywordAbility::DoubleStrike));
    assert!(!sheet(&moved, host).has_keyword(KeywordAbility::DoubleStrike));
}

pub(super) fn order_tokens(result: NativeCandidate) -> NativeCandidate {
    if let Some(p) = result.state().pending_decision()
        && let RuleFrame::TimestampOrder(order) = p.frame()
    {
        return reply(
            result.state(),
            DecisionReply::TimestampOrder(order.members().iter().map(|each| each.member).collect()),
        );
    }
    result
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_kemba_and_valduk_count_the_correct_attachments_at_their_own_steps_and_only_valduks_tokens_expire()
 {
    use ManaType::*;
    for (name, price, kind, color, base, limited) in [
        (
            "Kemba, Kha Regent",
            vec![(Colourless, 1), (White, 2)],
            "Cat",
            Color::White,
            (2, 2),
            false,
        ),
        (
            "Valduk, Keeper of the Flame",
            vec![(Colourless, 2), (Red, 1)],
            "Elemental",
            Color::Red,
            (3, 1),
            true,
        ),
    ] {
        let mut mana = price.clone();
        mana[0].1 += 2;
        let initial = game(
            vec![A, B],
            vec![
                named(name, 1, A, ObjectDestination::Hand),
                named("Bonesplitter", 2, A, battlefield(A)),
                seed(
                    3,
                    A,
                    ObjectDestination::Hand,
                    Program::aura(
                        Some(ManaCost::free()),
                        spellwake_engine::attachment::EnchantRestriction::Creature,
                    ),
                ),
                named("Bonesplitter", 4, A, battlefield(A)),
                named("Island", 8, B, ObjectDestination::LibraryBottom),
            ],
            &mana,
            Step::PrecombatMain,
        );
        let entered = cast_card(&initial, 1, &price);
        let one = wield(&entered, physical(&entered, 2).id(), 1, 1);
        let aura = round(targeted(&one, A, 3, marker(&one, 1)).into_state()).into_state();
        let trigger = exile_hand::until(aura, |s| {
            s.turn().unwrap().position().number == if limited { 1 } else { 3 }
                && s.turn().unwrap().position().step
                    == if limited {
                        Step::BeginningCombat
                    } else {
                        Step::Upkeep
                    }
                && !s.execution().window().entries().is_empty()
        });
        let made = order_tokens(round(trigger)).into_state();
        let batch = made
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|c| c.is_token() && c.zone() == ObjectZone::Battlefield)
            .map(|c| c.id())
            .collect::<Vec<_>>();
        assert_eq!(batch.len(), if limited { 2 } else { 1 });
        for &id in &batch {
            let view = sheet(&made, id);
            assert_eq!(view.current_colors, [color].into());
            assert_eq!(
                view.current_creature_subtypes,
                [current_catalogue().1.resolve_label(kind).unwrap()].into()
            );
            stats(&made, id, base.0, base.1);
            for key in [KeywordAbility::Trample, KeywordAbility::Haste] {
                assert_eq!(view.has_keyword(key), limited);
            }
        }
        let turn = made.turn().unwrap().position().number + 1;
        let next = exile_hand::until(made, |s| {
            s.turn().unwrap().position().number == turn
                && s.turn().unwrap().position().step == Step::PostcombatMain
        });
        let left = next
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|c| c.is_token() && c.zone() == ObjectZone::Battlefield)
            .count();
        assert_eq!(left, if limited { 0 } else { 1 });
    }
}

pub(super) fn optional(state: &GameState, yes: bool) -> GameState {
    use spellwake_engine::effects::OptionalEffectChoice;
    reply(
        state,
        DecisionReply::OptionalEffect(if yes {
            OptionalEffectChoice::Take
        } else {
            OptionalEffectChoice::Decline
        }),
    )
    .into_state()
}
fn castable_creature(id: u64, seat: SeatId, legendary: bool) -> SeedCard {
    let mut card = Program::creature(
        Some(ManaCost::free()),
        2.into(),
        2.into(),
        Default::default(),
    );
    if legendary {
        card.supertypes
            .insert(spellwake_engine::program::Supertype::Legendary);
    }
    seed(id, seat, ObjectDestination::Hand, card)
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_forebears_blade_moves_on_the_equipped_creatures_death_and_transfers_vigilance_trample_and_power()
 {
    use ManaType::*;
    let initial = game(
        vec![A, B],
        vec![
            named("Forebear's Blade", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 2, &[]),
            creature(3, A, 1, 3, &[]),
            creature(4, B, 2, 2, &[]),
            super::super::cmm_gameplay::remove_spell(5, A),
        ],
        &[(Colourless, 6)],
        Step::PrecombatMain,
    );
    let entered = cast_card(&initial, 1, &[(Colourless, 3)]);
    let blade = physical(&entered, 1).id();
    illegal(&entered, A, equip(&entered, blade, 4, 3));
    let wielded = wield(&entered, blade, 2, 3);
    stats(&wielded, physical(&wielded, 2).id(), 5, 2);
    for key in [KeywordAbility::Trample, KeywordAbility::Vigilance] {
        assert!(sheet(&wielded, physical(&wielded, 2).id()).has_keyword(key));
    }
    let unrelated = round(targeted(&wielded, A, 5, marker(&wielded, 3)).into_state()).into_state();
    assert!(
        unrelated.pending_decision().is_none()
            && unrelated.execution().window().entries().is_empty()
    );
    let dies = round(targeted(&wielded, A, 5, marker(&wielded, 2)).into_state()).into_state();
    assert_eq!(dies.execution().attached_to(blade), None);
    super::declarations::invalid(&dies, A, DecisionReply::TriggerTarget(marker(&dies, 4)));
    let chosen = reply(&dies, DecisionReply::TriggerTarget(marker(&dies, 3))).into_state();
    let moved = round(chosen).into_state();
    assert_eq!(
        moved.execution().attached_to(blade),
        Some(marker(&moved, 3))
    );
    stats(&moved, physical(&moved, 3).id(), 4, 3);
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_heros_blade_triggers_only_on_a_controlled_legendary_entrant_and_optional_attachment_transfers_its_bonus()
 {
    use ManaType::*;
    let initial = game(
        vec![A, B],
        vec![
            named("Hero's Blade", 1, A, ObjectDestination::Hand),
            creature(2, A, 1, 1, &[]),
            castable_creature(3, A, true),
            castable_creature(4, A, false),
            castable_creature(5, B, true),
        ],
        &[(Colourless, 6)],
        Step::PrecombatMain,
    );
    let entered = cast_card(&initial, 1, &[(Colourless, 2)]);
    let blade = physical(&entered, 1).id();
    let equip = wield(&entered, blade, 2, 4);
    stats(&equip, physical(&equip, 2).id(), 4, 3);
    let common = cast_card(&equip, 4, &[]);
    assert!(common.execution().window().entries().is_empty());
    let foreign = exile_hand::until(equip.clone(), |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let visitor = round(
        command(
            &foreign,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&foreign, 5).id(),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state();
    assert!(visitor.execution().window().entries().is_empty());
    let legend = cast_card(&equip, 3, &[]);
    let offer = round(legend).into_state();
    let declined = optional(&offer, false);
    assert_eq!(
        declined.execution().attached_to(blade),
        Some(marker(&declined, 2))
    );
    let moved = optional(&offer, true);
    assert_eq!(
        moved.execution().attached_to(blade),
        Some(marker(&moved, 3))
    );
    stats(&moved, physical(&moved, 2).id(), 1, 1);
    stats(&moved, physical(&moved, 3).id(), 5, 4);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_mace_of_the_valiant_keeps_its_own_charge_counters_and_transfers_the_current_bonus_and_vigilance()
 {
    use ManaType::*;
    let initial = game(
        vec![A, B],
        vec![
            named("Mace of the Valiant", 1, A, ObjectDestination::Hand),
            creature(2, A, 1, 3, &[]),
            castable_creature(3, A, false),
            castable_creature(4, A, false),
            castable_creature(5, B, false),
        ],
        &[(Colourless, 8), (White, 1)],
        Step::PrecombatMain,
    );
    let entered = cast_card(&initial, 1, &[(Colourless, 2), (White, 1)]);
    let mace = physical(&entered, 1).id();
    let mut ready = wield(&entered, mace, 2, 3);
    stats(&ready, physical(&ready, 2).id(), 1, 3);
    assert!(sheet(&ready, physical(&ready, 2).id()).has_keyword(KeywordAbility::Vigilance));
    for (n, count) in [(3, 1), (4, 2)] {
        let entry = cast_card(&ready, n, &[]);
        ready = round(entry).into_state();
        assert_eq!(
            ready.execution().charge_counters().get(&mace),
            Some(&count.into())
        );
        stats(&ready, physical(&ready, 2).id(), 1 + count, 3 + count);
    }
    let transferred = wield(&ready, mace, 3, 3);
    stats(&transferred, physical(&transferred, 2).id(), 1, 3);
    stats(&transferred, physical(&transferred, 3).id(), 4, 4);
    assert!(
        !sheet(&transferred, physical(&transferred, 2).id()).has_keyword(KeywordAbility::Vigilance)
    );
    let foreign = exile_hand::until(transferred, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let visitor = round(
        command(
            &foreign,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&foreign, 5).id(),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state();
    assert!(visitor.execution().window().entries().is_empty());
    assert_eq!(
        visitor.execution().charge_counters().get(&mace),
        Some(&2.into())
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_hammer_of_nazahn_attaches_itself_and_other_entering_equipment_only_to_own_creatures_and_grants_indestructible()
 {
    use ManaType::*;
    let initial = game(
        vec![A, B],
        vec![
            named("Hammer of Nazahn", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 2, &[]),
            creature(3, A, 1, 3, &[]),
            creature(4, B, 2, 2, &[]),
            named("Bonesplitter", 5, A, ObjectDestination::Hand),
            super::super::cmm_gameplay::remove_spell(6, A),
        ],
        &[(Colourless, 9)],
        Step::PrecombatMain,
    );
    let enters = cast_card(&initial, 1, &[(Colourless, 4)]);
    super::declarations::invalid(&enters, A, DecisionReply::TriggerTarget(marker(&enters, 4)));
    let aim = reply(&enters, DecisionReply::TriggerTarget(marker(&enters, 2))).into_state();
    let offer = round(aim).into_state();
    let no = optional(&offer, false);
    let hammer = physical(&no, 1).id();
    assert_eq!(no.execution().attached_to(hammer), None);
    let manual = wield(&no, hammer, 3, 4);
    stats(&manual, physical(&manual, 3).id(), 3, 3);
    let yes = optional(&offer, true);
    assert_eq!(yes.execution().attached_to(hammer), Some(marker(&yes, 2)));
    stats(&yes, physical(&yes, 2).id(), 4, 2);
    assert!(sheet(&yes, physical(&yes, 2).id()).has_keyword(KeywordAbility::Indestructible));
    let safe = round(targeted(&yes, A, 6, marker(&yes, 2)).into_state()).into_state();
    assert_eq!(physical(&safe, 2).zone(), ObjectZone::Battlefield);
    let second = cast_card(&safe, 5, &[(Colourless, 1)]);
    let aim = reply(&second, DecisionReply::TriggerTarget(marker(&second, 3))).into_state();
    let attached = optional(&round(aim).into_state(), true);
    assert_eq!(
        attached
            .execution()
            .attached_to(physical(&attached, 5).id()),
        Some(marker(&attached, 3))
    );
    assert_eq!(
        attached.execution().attached_to(hammer),
        Some(marker(&attached, 2))
    );
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_puresteel_paladin_draws_optionally_on_equipment_entry_and_grants_free_equip_only_above_the_artifact_threshold()
 {
    use ManaType::*;
    let initial = game(
        vec![A, B],
        vec![
            named("Puresteel Paladin", 1, A, ObjectDestination::Hand),
            named("Bonesplitter", 2, A, battlefield(A)),
            named("Bonesplitter", 3, B, battlefield(B)),
            named("Sol Ring", 4, A, battlefield(A)),
            named("Bonesplitter", 5, A, ObjectDestination::Hand),
            creature(6, B, 2, 2, &[]),
            seed(
                7,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SinglePermanent,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                    allow_regenerate: true,
                                },
                            },
                        ),
                    ),
                ),
            ),
        ],
        &[(Colourless, 1), (White, 2)],
        Step::PrecombatMain,
    );
    let ready = cast_card(&initial, 1, &[(White, 2)]);
    let held = physical(&ready, 2).id();
    assert!(sheet(&ready, held).granted_activated.is_empty());
    let threshold = cast_card(&ready, 5, &[(Colourless, 1)]);
    let offer = round(threshold).into_state();
    let declined = optional(&offer, false);
    let drawn = optional(&offer, true);
    assert_eq!(physical(&declined, 100).zone(), ObjectZone::Library(A));
    assert_eq!(physical(&drawn, 100).zone(), ObjectZone::Hand(A));
    let view = sheet(&drawn, held);
    assert_eq!(view.granted_activated.len(), 1);
    let (&grant, _) = view.granted_activated.first_key_value().unwrap();
    let free = |s: &GameState, n| GameCommand::ActivateGranted {
        source: held,
        effect: grant,
        target: Some(marker(s, n)),
        payment: None,
    };
    assert!(
        sheet(&drawn, physical(&drawn, 3).id())
            .granted_activated
            .is_empty()
    );
    assert!(
        sheet(&drawn, physical(&drawn, 4).id())
            .granted_activated
            .is_empty()
    );
    illegal(&drawn, A, free(&drawn, 6));
    let attached = round(command(&drawn, A, free(&drawn, 1)).into_state()).into_state();
    assert_eq!(
        attached.execution().attached_to(held),
        Some(marker(&attached, 1))
    );
    let gone = round(targeted(&attached, A, 7, marker(&attached, 4)).into_state()).into_state();
    assert!(sheet(&gone, held).granted_activated.is_empty());
    illegal(&gone, A, free(&gone, 1));
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_heavenly_blademaster_selects_any_number_of_own_legal_attachments_and_pumps_only_other_controlled_creatures()
 {
    use ManaType::*;
    let initial = game(
        vec![A, B],
        vec![
            named("Heavenly Blademaster", 1, A, ObjectDestination::Hand),
            named("Bonesplitter", 2, A, battlefield(A)),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                Program::aura(
                    Some(ManaCost::free()),
                    spellwake_engine::attachment::EnchantRestriction::Creature,
                ),
            ),
            named("Bonesplitter", 4, B, battlefield(B)),
            creature(5, A, 1, 2, &[]),
            creature(6, B, 2, 2, &[]),
        ],
        &[(Colourless, 5), (White, 1)],
        Step::PrecombatMain,
    );
    let aura = round(targeted(&initial, A, 3, marker(&initial, 5)).into_state()).into_state();
    let entered = cast_card(&aura, 1, &[(Colourless, 5), (White, 1)]);
    let offer = round(entered).into_state();
    super::declarations::invalid(
        &offer,
        A,
        DecisionReply::SelectResolution(vec![physical(&offer, 4).id()]),
    );
    for ids in [vec![], vec![3], vec![2, 3]] {
        let chosen = reply(
            &offer,
            DecisionReply::SelectResolution(
                ids.iter().map(|&id| physical(&offer, id).id()).collect(),
            ),
        )
        .into_state();
        let angel = physical(&chosen, 1).id();
        let view = sheet(&chosen, angel);
        assert!(
            view.has_keyword(KeywordAbility::Flying)
                && view.has_keyword(KeywordAbility::DoubleStrike)
        );
        stats(&chosen, angel, if ids.contains(&2) { 5 } else { 3 }, 6);
        let count = i64::try_from(ids.len()).unwrap();
        stats(&chosen, physical(&chosen, 5).id(), 1 + count, 2 + count);
        stats(&chosen, physical(&chosen, 6).id(), 2, 2);
        for id in &ids {
            assert_eq!(
                chosen.execution().attached_to(physical(&chosen, *id).id()),
                Some(TargetRef::Object(angel))
            );
        }
    }
}
