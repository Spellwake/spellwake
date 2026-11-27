//! Custom-Card Compositional Stress & Normalization Suite.
//!
//! Validates that `spellwake-oracle-compiler` parses brand-new, never-printed synthetic Magic
//! cards deterministically via compositional grammar rules (not printed-card memorization) and
//! decodes their escape-hatch fields cleanly through `typed_ir`.

use spellwake_oracle_compiler::ast::{
    AbilityAst, CardAst, CardTypeMask, ColorMask, CompareOp, ConditionAst, ControllerRelation,
    DurationAst, FilterPredicateAst, InstructionAst, InteractiveChoiceKind, ObjectFilterAst,
    ObjectFilterFlags, PlayerRefAst, PriorActionRefKind, ReplacementEffectAst, Rounding,
    StatBoundAst, StatKind, StaticAbilityAst, SubjectRefAst, TokenSpecAst, TriggerEventAst,
    TurnStepAst, ValueExprAst, ZoneFilter,
};
use spellwake_oracle_compiler::audit::audit_card;
use spellwake_oracle_compiler::typed_ir::{
    ChoiceOptionAst, CombatRuleAst, GrantPayloadAst, LibraryOrderAst, LibraryPositionAst,
    ManaSpendTargetAst, ParsedDurationAst, RestrictionAst, RuleRestrictionAst, SpendPolarity,
    TimingRestrictionAst, ZoneKindAst, ZoneOwnerAst,
};
use spellwake_oracle_compiler::{CardInput, CompileOptions, FaceInput, Vocabulary, compile_card};

fn custom_vocab() -> Vocabulary {
    let members: Vec<String> = [
        "Angel",
        "Assassin",
        "Aura",
        "Beast",
        "Bird",
        "Cleric",
        "Construct",
        "Demon",
        "Dinosaur",
        "Dragon",
        "Druid",
        "Elemental",
        "Elf",
        "Equipment",
        "Faerie",
        "Forest",
        "Fractal",
        "Fungus",
        "Giant",
        "Goblin",
        "Golem",
        "Human",
        "Hydra",
        "Illusion",
        "Insect",
        "Island",
        "Knight",
        "Merfolk",
        "Monk",
        "Mountain",
        "Ninja",
        "Ooze",
        "Phoenix",
        "Pirate",
        "Plains",
        "Rogue",
        "Samurai",
        "Scout",
        "Shaman",
        "Shrine",
        "Skeleton",
        "Sliver",
        "Soldier",
        "Sphinx",
        "Spirit",
        "Swamp",
        "Thopter",
        "Treasure",
        "Vampire",
        "Vehicle",
        "Wall",
        "Warrior",
        "Wizard",
        "Wolf",
        "Wurm",
        "Zombie",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect();
    Vocabulary::from_creature_members(&members)
}

fn compile_custom(name: &str, type_line: &str, oracle_text: &str, vocab: &Vocabulary) -> CardAst {
    let input = CardInput {
        oracle_id: "custom-composability-test",
        name,
        layout: "normal",
        faces: vec![FaceInput {
            name,
            type_line,
            oracle_text,
        }],
    };
    let card = compile_card(&input, vocab);
    let report = audit_card(&input, &card, CompileOptions::default());
    assert!(
        card.strictly_compiled && report.has_zero_critical_defects(),
        "Custom card '{name}' failed strict compilation!\nOracle: {oracle_text}\nFailures: {:?}\nCritical defects: {:?}",
        card.faces[0].failures,
        report.critical_defects
    );
    card
}

/// 1. Novel multi-zone + multi-color + stat-bound + controller filter combinations.
#[test]
fn custom_cards_compositional_filters_and_multi_zones() {
    let vocab = custom_vocab();

    // Card 1: Novel multi-zone put with owner + type filter
    let c1 = compile_custom(
        "Astral Recall Engine",
        "Sorcery",
        "Return up to two target blue or red Wizard creature cards with mana value 3 or less from your graveyard to your hand.",
        &vocab,
    );
    assert!(matches!(
        c1.faces[0].spell_effects.first(),
        Some(InstructionAst::ReturnZone { to, .. }) if to == "your hand"
    ));

    // Card 2: Novel multi-zone graveyard + exile retrieval
    let c2 = compile_custom(
        "Vault of Forgotten Relics",
        "Sorcery",
        "Put all Shrine cards you own from exile and from your graveyard into your hand.",
        &vocab,
    );
    let dest2 = c2.faces[0]
        .spell_effects
        .first()
        .and_then(InstructionAst::parsed_zone_dest)
        .expect("parsed_zone_dest on Card 2");
    assert_eq!(dest2.zone, ZoneKindAst::Hand);
    assert_eq!(dest2.owner, ZoneOwnerAst::You);

    // Card 3: Novel multi-zone command zone + graveyard move
    let c3 = compile_custom(
        "Beacon of the Sovereign",
        "Instant",
        "You may put a Dragon creature card you own with mana value 6 or less from your hand or from the command zone onto the battlefield tapped.",
        &vocab,
    );
    assert!(matches!(
        c3.faces[0].spell_effects.first(),
        Some(InstructionAst::Optional(_))
    ));

    // Card 4: Novel non-color + non-type + attached-to filter
    let c4 = compile_custom(
        "Aether Unbinder",
        "Instant",
        "Exile target nonwhite, nonartifact permanent attached to a Dragon you control.",
        &vocab,
    );
    match c4.faces[0].spell_effects.first() {
        Some(InstructionAst::Exile { subject, .. }) => match subject.as_ref() {
            SubjectRefAst::TargetObject { filter, .. } => {
                assert_ne!(filter.forbidden_colors.0 & ColorMask::WHITE.0, 0);
                assert_ne!(filter.forbidden_types.0 & CardTypeMask::ARTIFACT.0, 0);
                assert!(
                    filter
                        .predicates
                        .iter()
                        .any(|p| matches!(p, FilterPredicateAst::AttachedTo(_)))
                );
            }
            other => panic!("expected TargetObject, got {other:?}"),
        },
        other => panic!("expected Exile, got {other:?}"),
    }

    // Card 5: Novel counter-on-it filter (`that has a <custom> counter on it`)
    let c5 = compile_custom(
        "Chronosiphon",
        "Sorcery",
        "Destroy each creature an opponent controls that has a slumber counter on it.",
        &vocab,
    );
    match c5.faces[0].spell_effects.first() {
        Some(InstructionAst::Destroy { subject, .. }) => match subject.as_ref() {
            SubjectRefAst::Each(flt) => {
                assert_eq!(flt.controller, Some(ControllerRelation::Opponent));
                assert_eq!(flt.counter_kind.as_deref(), Some("slumber"));
            }
            other => panic!("expected Each, got {other:?}"),
        },
        other => panic!("expected Destroy, got {other:?}"),
    }

    // Card 6: Novel `that were <verb> this way` + aggregate power scaling
    let c6 = compile_custom(
        "Eclipse Harvest",
        "Sorcery",
        "Exile all tapped Vampires. Each player creates a 0/0 black and white Spirit creature token and puts a number of +1/+1 counters on it equal to the total power of creatures they controlled that were exiled this way.",
        &vocab,
    );
    assert_eq!(c6.faces[0].spell_effects.len(), 3);
    assert!(matches!(
        &c6.faces[0].spell_effects[0],
        InstructionAst::Exile { subject, .. }
            if matches!(subject.as_ref(), SubjectRefAst::All(f)
                if f.tapped == Some(true) && f.subtypes.iter().any(|s| s == "Vampire"))
    ));
    assert!(matches!(
        &c6.faces[0].spell_effects[1],
        InstructionAst::CreateToken {
            controller: PlayerRefAst::EachPlayer,
            token: TokenSpecAst::Creature { power, toughness, .. },
            ..
        } if power == "0" && toughness == "0"
    ));
    assert!(matches!(
        &c6.faces[0].spell_effects[2],
        InstructionAst::PutCounters { counter_kind, count, .. }
            if counter_kind == "+1/+1"
                && matches!(count.as_ref(), ValueExprAst::AggregateStat { stat: StatKind::Power, filter, .. }
                    if filter.controller == Some(ControllerRelation::ThatPlayer)
                        && filter.predicates.iter().any(|p| matches!(p, FilterPredicateAst::AffectedByPriorAction(PriorActionRefKind::ExiledThisWay))))
    ));

    // Card 7: Novel `except` exclusion on ReturnZone
    let c7 = compile_custom(
        "Prism Containment",
        "Artifact",
        "When Prism Containment leaves the battlefield, return all cards exiled with Prism Containment except Prism Containment to the battlefield tapped under their owners' control.",
        &vocab,
    );
    let AbilityAst::Triggered { body, .. } = &c7.faces[0].abilities[0] else {
        panic!("expected triggered ability");
    };
    assert!(matches!(
        body.first(),
        Some(InstructionAst::ReturnZone {
            except: Some(_),
            tapped: true,
            ..
        })
    ));
    let dest7 = body[0]
        .parsed_zone_dest()
        .expect("parsed_zone_dest on Card 7");
    assert_eq!(dest7.zone, ZoneKindAst::Battlefield);
    assert!(dest7.modifiers.tapped);
    assert_eq!(dest7.owner, ZoneOwnerAst::TheirOwners);

    // Card 8: Novel `with power or toughness N or less` (must preserve BOTH power and toughness via AnyOf)
    let c8 = compile_custom(
        "Scalewright's Verdict",
        "Sorcery",
        "Exile all creatures with power or toughness 5 or less. Draw a card for each creature exiled this way.",
        &vocab,
    );
    assert_eq!(c8.faces[0].spell_effects.len(), 2);
    match &c8.faces[0].spell_effects[0] {
        InstructionAst::Exile { subject, .. } => match subject.as_ref() {
            SubjectRefAst::All(flt) => {
                assert_eq!(flt.any_types, CardTypeMask::CREATURE);
                let any_of = flt
                    .predicates
                    .iter()
                    .find_map(|p| match p {
                        FilterPredicateAst::AnyOf(branches) => Some(branches),
                        _ => None,
                    })
                    .expect("power or toughness must emit AnyOf([Power, Toughness])");
                assert_eq!(any_of.len(), 2);
                assert!(any_of.iter().any(|b| {
                    matches!(
                        b.stat_bounds.as_slice(),
                        [StatBoundAst { stat: StatKind::Power, op: CompareOp::Le, value }]
                            if matches!(value.as_ref(), ValueExprAst::Const(5))
                    )
                }));
                assert!(any_of.iter().any(|b| {
                    matches!(
                        b.stat_bounds.as_slice(),
                        [StatBoundAst { stat: StatKind::Toughness, op: CompareOp::Le, value }]
                            if matches!(value.as_ref(), ValueExprAst::Const(5))
                    )
                }));
            }
            other => panic!("expected All(creatures), got {other:?}"),
        },
        other => panic!("expected Exile, got {other:?}"),
    }
    assert!(matches!(
        &c8.faces[0].spell_effects[1],
        InstructionAst::Draw {
            player: PlayerRefAst::You,
            ..
        }
    ));

    // Card 9: Novel `that share a creature type with` + `from your library`
    let c9 = compile_custom(
        "Kinship Calling",
        "Sorcery",
        "Search your library for a creature card that shares a creature type with target Elf you control, reveal it, put it into your hand, then shuffle.",
        &vocab,
    );
    assert!(matches!(
        c9.faces[0].spell_effects.first(),
        Some(InstructionAst::SearchLibrary {
            reveal: true,
            shuffle: true,
            ..
        })
    ));

    // Card 10: Novel `for each graveyard with <custom filter> in it`
    let c10 = compile_custom(
        "Graveyard Census",
        "Sorcery",
        "For each graveyard with an artifact or enchantment card in it, exile target artifact or enchantment card from that graveyard.",
        &vocab,
    );
    assert!(matches!(
        c10.faces[0].spell_effects.first(),
        Some(InstructionAst::ForEachSubjectPlayer { .. })
    ));
}

/// 2. Novel triggers, delayed triggers, replacements, and duration combinations.
#[test]
fn custom_cards_triggers_replacements_and_durations() {
    let vocab = custom_vocab();

    // Card 11: Novel delayed trigger with custom token colors (`that are blue, black, and red`)
    let c11 = compile_custom(
        "Tidecaller Sovereign",
        "Legendary Creature — Merfolk Wizard",
        "When Tidecaller Sovereign enters, create X 2/2 Merfolk Shaman creature tokens that are blue, black, and red at the beginning of your next upkeep, where X is the number of Islands you control at that time.",
        &vocab,
    );
    let AbilityAst::Triggered { body, .. } = &c11.faces[0].abilities[0] else {
        panic!("expected ETB trigger");
    };
    assert!(matches!(body.first(), Some(InstructionAst::BindX { .. })));

    // Card 12: Novel multi-turn duration (`This turn and next turn, ...`)
    let c12 = compile_custom(
        "Armistice Protocol",
        "Sorcery",
        "This turn and next turn, creatures can't attack, and players and permanents can't be the targets of spells or activated abilities.",
        &vocab,
    );
    assert_eq!(c12.faces[0].spell_effects.len(), 2);
    assert!(c12.faces[0].spell_effects.iter().all(|ins| matches!(
        ins,
        InstructionAst::ScopedDuration {
            duration: DurationAst::UntilEndOfNextTurn,
            ..
        } | InstructionAst::CombatRuleMod {
            duration: DurationAst::UntilEndOfNextTurn,
            ..
        } | InstructionAst::TemporaryStatic {
            duration: DurationAst::UntilEndOfNextTurn,
            ..
        }
    )));

    // Card 13: Novel `for each spell you've cast this turn other than the first`
    let c13 = compile_custom(
        "Stormforged Artificer",
        "Creature — Human Artificer",
        "When Stormforged Artificer enters, create a 1/1 colorless Thopter artifact creature token with flying, then put a +1/+1 counter on that token for each spell you've cast this turn other than the first.",
        &vocab,
    );
    let AbilityAst::Triggered { body, .. } = &c13.faces[0].abilities[0] else {
        panic!("expected ETB trigger");
    };
    assert_eq!(body.len(), 2);
    assert!(matches!(
        &body[0],
        InstructionAst::CreateToken {
            controller: PlayerRefAst::You,
            token: TokenSpecAst::Creature { power, toughness, keywords, .. },
            ..
        } if power == "1" && toughness == "1" && keywords.iter().any(|k| k == "flying")
    ));
    assert!(matches!(
        &body[1],
        InstructionAst::PutCounters { counter_kind, count, .. }
            if counter_kind == "+1/+1"
                && matches!(count.as_ref(), ValueExprAst::Mul(_, rhs) if matches!(rhs.as_ref(), ValueExprAst::Sub(_, _)))
    ));

    // Card 14: Novel `for each mana from a Shrine that was spent to cast ~`
    let c14 = compile_custom(
        "Sanctum Surge",
        "Instant",
        "Target creature you control gets +2/+1 until end of turn for each mana from a Shrine that was spent to cast Sanctum Surge.",
        &vocab,
    );
    assert!(matches!(
        c14.faces[0].spell_effects.first(),
        Some(InstructionAst::ModifyPT {
            dx,
            dy,
            duration: DurationAst::UntilEndOfTurn,
            scale: Some(_),
            ..
        }) if dx == "+2" && dy == "+1"
    ));

    // Card 15: Novel `enters with a number of additional <kind> counters equal to <val>`
    let c15 = compile_custom(
        "Aether Foundry",
        "Sorcery",
        "The next Construct spell you cast this turn enters with a number of additional charge counters on it equal to the number of artifacts you control.",
        &vocab,
    );
    assert!(matches!(
        c15.faces[0].spell_effects.first(),
        Some(InstructionAst::TemporaryReplacement {
            replacement,
            ..
        }) if matches!(replacement.as_ref(), ReplacementEffectAst::OthersEnterWithCounters { .. })
    ));

    // Card 16: Novel replacement effect + conditional static
    let c16 = compile_custom(
        "Vanguard of the Reliquary",
        "Creature — Angel Knight",
        "Flying, vigilance\nAs long as you control three or more enchantments, Angels you control get +2/+2 and have lifelink.\nIf a creature an opponent controls would die, exile it instead.",
        &vocab,
    );
    assert_eq!(c16.faces[0].keywords.len(), 2);
    assert_eq!(c16.faces[0].statics.len(), 1);
    assert!(matches!(
        &c16.faces[0].statics[0],
        StaticAbilityAst::ContinuousModifyPT {
            dx,
            dy,
            grant_keywords,
            condition: Some(_),
            ..
        } if dx == "+2" && dy == "+2" && grant_keywords == &vec!["lifelink".to_string()]
    ));
    assert_eq!(c16.faces[0].replacements.len(), 1);
    assert!(matches!(
        &c16.faces[0].replacements[0],
        ReplacementEffectAst::EventInstead {
            instead_body: Some(body),
            ..
        } if matches!(body.as_slice(), [InstructionAst::Exile { .. }])
    ));

    // Card 17: Novel `under the control of that creature's owner` delayed death trigger
    let c17 = compile_custom(
        "Soul Tether",
        "Instant",
        "Choose target creature. When that creature dies this turn, return a creature card from its owner's graveyard to the battlefield under the control of that creature's owner.",
        &vocab,
    );
    assert_eq!(c17.faces[0].spell_effects.len(), 2);
    assert!(matches!(
        &c17.faces[0].spell_effects[1],
        InstructionAst::DelayedEventTrigger { body, .. }
            if matches!(body.as_slice(), [InstructionAst::ReturnZone { to, .. }] if to.contains("battlefield"))
    ));

    // Card 18: Novel `mana cost contains {R}` condition
    let c18 = compile_custom(
        "Pyromancer's Lens",
        "Enchantment",
        "Whenever you cast a spell, if its mana cost contains {R}, Pyromancer's Lens deals 2 damage to each opponent.",
        &vocab,
    );
    let AbilityAst::Triggered { trigger, body, .. } = &c18.faces[0].abilities[0] else {
        panic!("expected triggered ability");
    };
    assert!(trigger.intervening_if.is_some());
    assert!(matches!(
        body.first(),
        Some(InstructionAst::DealDamage { .. })
    ));

    // Card 19: Novel `<N> times <expr>` scaling in life gain / damage
    let c19 = compile_custom(
        "Solar Amplifier",
        "Sorcery",
        "You gain life equal to three times the number of Angels you control. Draw two cards.",
        &vocab,
    );
    match c19.faces[0].spell_effects.first() {
        Some(InstructionAst::ChangeLife {
            delta, gain: true, ..
        }) => {
            assert!(matches!(
                delta.as_ref(),
                ValueExprAst::Mul(l, r)
                    if matches!(l.as_ref(), ValueExprAst::Const(3))
                        && matches!(r.as_ref(), ValueExprAst::Count(_))
            ));
        }
        other => panic!("expected ChangeLife with Mul(3, Count), got {other:?}"),
    }

    // Card 20: Novel `If you do, <eff> if <cond>` nested trailing condition
    let c20 = compile_custom(
        "Blood Oath Seer",
        "Creature — Vampire Cleric",
        "At the beginning of your upkeep, you may pay 2 life. If you do, draw two cards if you control a Demon.",
        &vocab,
    );
    let AbilityAst::Triggered { body, .. } = &c20.faces[0].abilities[0] else {
        panic!("expected upkeep trigger");
    };
    assert!(
        body.iter()
            .any(|i| matches!(i, InstructionAst::IfPreviousSucceeded(_)))
    );
}

/// 3. Novel choices, modals, spree, villainous choices, piles, and mana riders.
#[test]
fn custom_cards_choices_modals_piles_and_mana_riders() {
    let vocab = custom_vocab();

    // Card 21: Novel villainous choice with arbitrary compound branches
    let c21 = compile_custom(
        "Inquisitor's Ultimatum",
        "Sorcery",
        "Each opponent faces a villainous choice — That player sacrifices two creatures and loses 3 life, or you draw three cards and create a 3/3 black Demon creature token with flying.",
        &vocab,
    );
    assert!(matches!(
        c21.faces[0].spell_effects.first(),
        Some(InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::VillainousChoice {
                chooser: PlayerRefAst::EachOpponent,
                options,
            }
        )) if options.len() == 2
    ));

    // Card 22: Novel modal spell with 3 mixed custom modes
    let c22 = compile_custom(
        "Guildmaster's Decree",
        "Instant",
        "Choose two —\n• Destroy target artifact or enchantment.\n• Target creature you control gets +3/+3 and gains trample until end of turn.\n• Scry 2, then draw a card.",
        &vocab,
    );
    assert!(matches!(
        c22.faces[0].spell_effects.first(),
        Some(InstructionAst::Modal { modes, .. }) if modes.len() == 3
    ));

    // Card 23: Novel Spree spell with custom additional costs
    let c23 = compile_custom(
        "Runeforge Escalation",
        "Sorcery",
        "Spree\n+ {1}{R} — Runeforge Escalation deals 3 damage to any target.\n+ {2}{U} — Draw two cards, then discard a card.\n+ {W} — Create two 1/1 white Soldier creature tokens.",
        &vocab,
    );
    let spree_modes = c23.faces[0]
        .spell_effects
        .iter()
        .filter(|i| matches!(i, InstructionAst::SpreeMode { .. }))
        .count();
    assert_eq!(spree_modes, 3);

    // Card 24: Novel pile separation + face-down/face-up visibility
    let c24 = compile_custom(
        "Sphinx's Partition",
        "Instant",
        "An opponent looks at the top five cards of your library and separates them into a face-down pile and a face-up pile. Put one pile into your hand and the other into your graveyard.",
        &vocab,
    );
    assert!(c24.faces[0].spell_effects.iter().any(|i| matches!(
        i,
        InstructionAst::SeparateIntoPiles {
            pile_count: 2,
            face_down_piles: 1,
            ..
        }
    )));

    // Card 25: Novel mana rider (`If that mana is spent to cast a <custom> spell, it enters with ...`)
    let c25 = compile_custom(
        "Dragonspire Sanctum",
        "Land",
        "{T}: Add {C}.\n{1}, {T}: Add one mana of any color. If that mana is spent to cast a Dragon spell, it enters with two additional +1/+1 counters on it.",
        &vocab,
    );
    let AbilityAst::Activated { body, .. } = &c25.faces[0].abilities[1] else {
        panic!("expected second activated ability");
    };
    assert!(
        body.iter().any(|i| matches!(
            i,
            InstructionAst::TemporaryTrigger {
                trigger,
                body: rider_body,
                duration: DurationAst::UntilEndOfTurn,
            } if matches!(
                trigger.events.as_slice(),
                [TriggerEventAst::SpellCast { caster: PlayerRefAst::You, spell: Some(flt), .. }]
                    if flt.subtypes.iter().any(|s| s == "Dragon")
            ) && matches!(
                rider_body.as_slice(),
                [InstructionAst::PutCounters { count, .. }]
                    if matches!(count.as_ref(), ValueExprAst::Const(2))
            )
        )),
        "expected Dragon SpellCast TemporaryTrigger with 2 counters in {body:?}"
    );

    // Card 26: Novel keyword-choice grant (`gains your choice of ... until end of turn`)
    let c26 = compile_custom(
        "Adaptive Carapace",
        "Instant",
        "Target creature you control gains your choice of flying, deathtouch, or hexproof until end of turn.",
        &vocab,
    );
    match c26.faces[0].spell_effects.first() {
        Some(InstructionAst::Choice(branches)) => {
            assert_eq!(branches.len(), 3);
            let granted: Vec<&str> = branches
                .iter()
                .map(|br| match br.as_slice() {
                    [
                        InstructionAst::GrantKeywords {
                            keywords,
                            duration: DurationAst::UntilEndOfTurn,
                            ..
                        },
                    ] if keywords.len() == 1 => keywords[0].as_str(),
                    other => panic!("unexpected choice branch: {other:?}"),
                })
                .collect();
            assert_eq!(granted, vec!["flying", "deathtouch", "hexproof"]);
        }
        other => panic!("expected 3-branch Choice(GrantKeywords), got {other:?}"),
    }

    // Card 27: Novel divided damage with explicit divider (`divided as target opponent chooses`)
    let c27 = compile_custom(
        "Cinder Arbitration",
        "Sorcery",
        "Cinder Arbitration deals 5 damage divided as target opponent chooses among any number of creatures they control.",
        &vocab,
    );
    assert!(matches!(
        c27.faces[0].spell_effects.first(),
        Some(InstructionAst::DealDamageDivided {
            divider: Some(PlayerRefAst::TargetOpponent),
            ..
        })
    ));

    // Card 28: Novel distribute counters equal to dynamic expression
    let c28 = compile_custom(
        "Biomancer's Tithe",
        "Sorcery",
        "You may distribute a number of +1/+1 counters equal to the number of Forests you control among any number of target creatures.",
        &vocab,
    );
    assert!(matches!(
        c28.faces[0].spell_effects.first(),
        Some(InstructionAst::Optional(inner))
            if matches!(inner.first(), Some(InstructionAst::DistributeCounters { .. }))
    ));

    // Card 29: Novel multi-target damage list (`deals 3 damage to target creature, 2 damage to its controller, and 1 damage to you`)
    let c29 = compile_custom(
        "Backlash Arc",
        "Instant",
        "Backlash Arc deals 3 damage to target creature, 2 damage to target opponent, and 1 damage to you.",
        &vocab,
    );
    let dmg_count = c29.faces[0]
        .spell_effects
        .iter()
        .filter(|i| matches!(i, InstructionAst::DealDamage { .. }))
        .count();
    assert_eq!(dmg_count, 3);

    // Card 30: Novel conditional `instead` sacrifice with `N of those <noun> of their choice`
    let c30 = compile_custom(
        "Valkyrie's Reckoning",
        "Instant",
        "Target opponent sacrifices a creature that attacked this turn. If you control an Angel, that player sacrifices two of those creatures of their choice instead.",
        &vocab,
    );
    assert_eq!(c30.faces[0].spell_effects.len(), 2);
    assert!(matches!(
        &c30.faces[0].spell_effects[1],
        InstructionAst::IfCondition { then_body, .. }
            if matches!(then_body.first(), Some(InstructionAst::ReplaceEffect(_)))
    ));
}

/// 4. Novel compound player actions, animations, library ops, and typed IR decoding end-to-end.
#[test]
fn custom_cards_compound_actions_and_typed_ir_roundtrip() {
    let vocab = custom_vocab();

    // Card 31: Novel wheel with energy-scaled draw (`Each player may exile their hand and draw a number of cards equal to ...`)
    let c31 = compile_custom(
        "Aetherial Reset",
        "Sorcery",
        "You get {E}{E}{E}. Each player may exile their hand and draw a number of cards equal to the amount of {E} paid this way.",
        &vocab,
    );
    assert!(matches!(
        c31.faces[0].spell_effects.first(),
        Some(InstructionAst::GainPlayerCounters {
            player: PlayerRefAst::You,
            count,
            ..
        }) if matches!(count.as_ref(), ValueExprAst::Const(3))
    ));
    assert!(
        c31.faces[0].spell_effects.len() >= 2
            && matches!(
                &c31.faces[0].spell_effects[1],
                InstructionAst::OptionalPlayer {
                    player: PlayerRefAst::EachPlayer,
                    ..
                } | InstructionAst::ForEachPlayer {
                    player: PlayerRefAst::EachPlayer,
                    ..
                }
            ),
        "expected EachPlayer optional/loop in {:?}",
        c31.faces[0].spell_effects
    );

    // Card 32: Novel `Each player discards their hand and returns to their hand each card they exiled this way`
    let c32 = compile_custom(
        "Temporal Vault",
        "Artifact",
        "{T}, Sacrifice Temporal Vault: Each player exiles all cards from their hand face down and draws five cards. At the beginning of the next end step, each player discards their hand and returns to their hand each card they exiled this way.",
        &vocab,
    );
    let AbilityAst::Activated { body, .. } = &c32.faces[0].abilities[0] else {
        panic!("expected activated ability");
    };
    assert!(body.iter().any(|i| matches!(
        i,
        InstructionAst::DelayedTrigger {
            step: TurnStepAst::EndStep,
            ..
        }
    )));

    // Card 33: Novel `Gain control of <filter ending in 'this turn'> and put a <counter> on it`
    let c33 = compile_custom(
        "Frostbind Claim",
        "Instant",
        "Gain control of target nonartifact, nonred creature that attacked you this turn and put a stun counter on it.",
        &vocab,
    );
    assert_eq!(c33.faces[0].spell_effects.len(), 2);
    assert!(matches!(
        &c33.faces[0].spell_effects[0],
        InstructionAst::GainControl {
            controller: PlayerRefAst::You,
            duration: DurationAst::Permanent,
            ..
        }
    ));
    assert!(matches!(
        &c33.faces[0].spell_effects[1],
        InstructionAst::PutCounters { counter_kind, .. } if counter_kind == "stun"
    ));

    // Card 34: Novel `Each player turns face up all cards they own exiled with ~, then puts all <type> cards among them onto the battlefield`
    let c34 = compile_custom(
        "Pandora's Reliquary",
        "Artifact",
        "{4}, {T}, Sacrifice Pandora's Reliquary: Each player turns face up all cards they own exiled with Pandora's Reliquary, then puts all artifact cards among them onto the battlefield.",
        &vocab,
    );
    let AbilityAst::Activated { body, .. } = &c34.faces[0].abilities[0] else {
        panic!("expected activated ability");
    };
    assert!(matches!(
        body.first(),
        Some(InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            ..
        })
    ));

    // Card 35: Novel `put a <kind> counter on <subj> for every N <filter>`
    let c35 = compile_custom(
        "Mycelial Conduit",
        "Creature — Fungus Druid",
        "At the beginning of your end step, put a spore counter on Mycelial Conduit for every three creatures you control.",
        &vocab,
    );
    let AbilityAst::Triggered { body, .. } = &c35.faces[0].abilities[0] else {
        panic!("expected end step trigger");
    };
    assert!(matches!(
        body.first(),
        Some(InstructionAst::PutCounters {
            counter_kind,
            count,
            ..
        }) if counter_kind == "spore"
            && matches!(count.as_ref(), ValueExprAst::DivConst { denom: 3, rounding: Rounding::Down, .. })
    ));

    // Card 36: Novel `becomes a creature in addition to its other types and has base power and base toughness each equal to <val>`
    let c36 = compile_custom(
        "Living Obelisk",
        "Artifact",
        "{3}: Until end of turn, Living Obelisk becomes a creature in addition to its other types and has base power and base toughness each equal to the number of artifacts you control.",
        &vocab,
    );
    let AbilityAst::Activated { body, .. } = &c36.faces[0].abilities[0] else {
        panic!("expected activated ability");
    };
    let has_dynamic_pt = body.iter().any(|i| match i {
        InstructionAst::SetDynamicPT { .. } | InstructionAst::AnimateOrSetBasePT { .. } => true,
        InstructionAst::ScopedDuration { body: inner, .. } => inner.iter().any(|sub| {
            matches!(
                sub,
                InstructionAst::SetDynamicPT { .. } | InstructionAst::AnimateOrSetBasePT { .. }
            )
        }),
        _ => false,
    });
    assert!(
        has_dynamic_pt,
        "expected SetDynamicPT/AnimateOrSetBasePT in {body:?}"
    );

    // Card 37: Novel `lose life and discard cards equal to <val>`
    let c37 = compile_custom(
        "Mindrot Surge",
        "Sorcery",
        "Each opponent loses life and discards cards equal to the number of Zombies you control.",
        &vocab,
    );
    assert!(matches!(
        c37.faces[0].spell_effects.first(),
        Some(InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachOpponent,
            body,
        }) if body.len() == 2
    ));

    // Card 38: Novel `create N tokens that are copies of <src> and that are tapped and attacking, where X is <val>`
    let c38 = compile_custom(
        "Mirror Cavalry",
        "Instant",
        "Create X tokens that are copies of target Knight you control and that are tapped and attacking, where X is the number of opponents you have.",
        &vocab,
    );
    assert!(matches!(
        c38.faces[0].spell_effects.first(),
        Some(InstructionAst::BindX { body, .. })
            if matches!(
                body.first(),
                Some(InstructionAst::CreateToken {
                    token: TokenSpecAst::Copy { tapped: true, attacking: true, .. },
                    ..
                })
            )
    ));

    // Card 39: Novel `Search your library and/or outside the game for exactly N <filter>`
    let c39 = compile_custom(
        "Triad Tutor",
        "Sorcery",
        "Search your library for exactly three Dragon cards with different names, then reveal those cards.",
        &vocab,
    );
    assert!(matches!(
        c39.faces[0].spell_effects.first(),
        Some(InstructionAst::SearchLibrary {
            exact: true,
            reveal: true,
            ..
        })
    ));

    // Card 40: Novel Planeswalker with loyalty +1, -2, -7 emblem + typed IR payload decoding
    let c40 = compile_custom(
        "Vespera, Weaver of Echoes",
        "Legendary Planeswalker — Vespera",
        "+1: Scry 1, then draw a card.\n-2: Return target nonland permanent to its owner's hand.\n-7: You get an emblem with \"Whenever you cast an instant or sorcery spell, draw a card.\"",
        &vocab,
    );
    assert_eq!(c40.faces[0].abilities.len(), 3);
    let AbilityAst::Loyalty { body: ult_body, .. } = &c40.faces[0].abilities[2] else {
        panic!("expected loyalty ultimate");
    };
    let emblem_payload = ult_body[0]
        .parsed_grant_payload(&vocab)
        .expect("decoded emblem payload");
    assert!(matches!(emblem_payload, GrantPayloadAst::Triggered { .. }));

    // Card 41: Novel ManaSpendRestriction + typed IR decoding (only for + cast from exile)
    let c41 = compile_custom(
        "Rift Conduit",
        "Artifact",
        "{T}: Add {U}{B}. Spend this mana only to cast spells from exile.",
        &vocab,
    );
    let AbilityAst::Activated { body: b41, .. } = &c41.faces[0].abilities[0] else {
        panic!("expected mana ability");
    };
    let msr41 = b41
        .iter()
        .find_map(|i| i.parsed_mana_spend_restriction(&vocab))
        .expect("decoded exile mana spend restriction");
    assert_eq!(msr41.polarity, SpendPolarity::OnlyFor);
    assert_eq!(
        msr41.targets,
        vec![ManaSpendTargetAst::CastFromZone(ZoneKindAst::Exile)]
    );

    // Card 42: Novel GrantCastPermission + ParsedDurationAst decoding (`for as long as it remains exiled`)
    let c42 = compile_custom(
        "Vault Scavenger",
        "Creature — Rogue",
        "When Vault Scavenger enters, exile the top card of target opponent's library. You may play that card for as long as it remains exiled.",
        &vocab,
    );
    let AbilityAst::Triggered { body: b42, .. } = &c42.faces[0].abilities[0] else {
        panic!("expected ETB trigger");
    };
    let dur42 = b42
        .iter()
        .find_map(|i| i.parsed_duration_text(&vocab))
        .expect("decoded duration text");
    assert_eq!(dur42, ParsedDurationAst::ForAsLongAsExiled);

    // Card 43: Novel ChoiceOptionAst + ZoneDestAst library-bottom-random decoding
    let c43 = compile_custom(
        "Oracle of the Shifting Sands",
        "Sorcery",
        "Choose a creature type. Look at the top four cards of your library. Put the rest on the bottom of your library in a random order.",
        &vocab,
    );
    let opt43 = c43.faces[0]
        .spell_effects
        .iter()
        .find_map(|i| i.parsed_choice_option(&vocab))
        .expect("decoded choice option");
    assert_eq!(opt43, ChoiceOptionAst::CreatureType { excluded: None });
    let dest43 = c43.faces[0]
        .spell_effects
        .iter()
        .find_map(InstructionAst::parsed_zone_dest)
        .expect("decoded bottom-of-library dest");
    assert_eq!(dest43.zone, ZoneKindAst::Library);
    assert_eq!(dest43.library_position, Some(LibraryPositionAst::Bottom));
    assert_eq!(dest43.library_order, Some(LibraryOrderAst::RandomOrder));

    // Card 44: Novel CombatRuleAst + StaticAbilityAst::parsed_restriction decoding
    let c44 = compile_custom(
        "Ironclad Bastion",
        "Enchantment",
        "Creatures you control can't be the targets of spells your opponents control.\n{2}{W}: Target creature can't attack or block until end of turn. Activate only during your turn and only once each turn.",
        &vocab,
    );
    let static_restr = c44.faces[0]
        .statics
        .iter()
        .find_map(StaticAbilityAst::parsed_restriction)
        .expect("decoded static restriction");
    assert!(matches!(
        static_restr,
        RestrictionAst::Rule(RuleRestrictionAst::CantBeTargetOf(_))
    ));
    let AbilityAst::Activated { body: b44, .. } = &c44.faces[0].abilities[0] else {
        panic!("expected activated ability");
    };
    let timing44 = b44
        .iter()
        .find_map(InstructionAst::parsed_restriction)
        .expect("decoded timing restriction");
    assert_eq!(
        timing44,
        RestrictionAst::Timing(vec![
            TimingRestrictionAst::DuringYourTurn,
            TimingRestrictionAst::OnceEachTurn,
        ])
    );
    let combat44 = b44
        .iter()
        .find_map(InstructionAst::parsed_combat_rule)
        .expect("decoded combat rule");
    assert_eq!(combat44, CombatRuleAst::CantAttackOrBlock);

    // Card 45: Novel `exile <subj> with a number of <kind> counters on it equal to <val>` + `for each`
    let c45 = compile_custom(
        "Chronomancer's Hourglass",
        "Sorcery",
        "Exile target creature with a number of time counters on it equal to its mana value. You gain 1 life for each card in your hand.",
        &vocab,
    );
    assert_eq!(c45.faces[0].spell_effects.len(), 2);
    assert!(matches!(
        &c45.faces[0].spell_effects[0],
        InstructionAst::ExileWithCounters {
            counter_kind,
            count,
            ..
        } if counter_kind == "time"
            && matches!(count.as_ref(), ValueExprAst::ObjectStat { stat: StatKind::ManaValue, .. })
    ));

    // Card 46: Novel generalized ExtremeLifePlayerAction (lowest life draws cards)
    let c46 = compile_custom(
        "Merciful Scales",
        "Sorcery",
        "The player with the lowest life total draws four cards.",
        &vocab,
    );
    assert!(matches!(
        &c46.faces[0].spell_effects[0],
        InstructionAst::ExtremeLifePlayerAction { highest: false, action }
            if matches!(action.as_ref(), InstructionAst::Draw { .. })
    ));

    // Card 47: Novel generalized next-<filter>-spell uncounterable modifier
    let c47 = compile_custom(
        "Sanctum Whisper",
        "Instant",
        "The next enchantment spell you cast this turn can't be countered.",
        &vocab,
    );
    assert!(matches!(
        &c47.faces[0].spell_effects[0],
        InstructionAst::TemporaryStatic { static_ability, duration: DurationAst::UntilEndOfTurn }
            if matches!(static_ability.as_ref(), StaticAbilityAst::NextSpellModifier { .. })
    ));

    // Card 48: Novel 3-way compound zone sweep across battlefield, all graveyards, and all hands
    let c48 = compile_custom(
        "Void Erasure",
        "Sorcery",
        "Exile all enchantments from the battlefield, all instant cards from all graveyards, and all artifact cards from all hands.",
        &vocab,
    );
    let exile_subjects: Vec<&ObjectFilterAst> = c48.faces[0]
        .spell_effects
        .iter()
        .flat_map(|ins| match ins {
            InstructionAst::Exile { subject, .. } => match subject.as_ref() {
                SubjectRefAst::Compound(items) => items
                    .iter()
                    .filter_map(|s| match s {
                        SubjectRefAst::All(f) | SubjectRefAst::Group(f) => Some(f.as_ref()),
                        _ => None,
                    })
                    .collect(),
                SubjectRefAst::All(f) | SubjectRefAst::Group(f) => vec![f.as_ref()],
                _ => vec![],
            },
            _ => vec![],
        })
        .collect();
    assert_eq!(
        exile_subjects.len(),
        3,
        "expected all 3 exiled zone groups in {:?}",
        c48.faces[0].spell_effects
    );
    assert!(exile_subjects.iter().any(|f| {
        f.any_types == CardTypeMask::ENCHANTMENT && f.zone == Some(ZoneFilter::Battlefield)
    }));
    assert!(exile_subjects.iter().any(|f| {
        f.any_types == CardTypeMask::INSTANT && f.zone == Some(ZoneFilter::AllGraveyards)
    }));
    assert!(exile_subjects.iter().any(|f| {
        f.any_types == CardTypeMask::ARTIFACT
            && matches!(
                f.zone,
                Some(ZoneFilter::EachPlayerHand | ZoneFilter::AnyHand)
            )
    }));

    // Card 49: Novel PlayerDealtDamageBy sweep with Dragon subtype and discard action
    let c49 = compile_custom(
        "Cinderwake Decree",
        "Sorcery",
        "Each opponent dealt combat damage this game by a Dragon you control discards two cards.",
        &vocab,
    );
    assert!(matches!(
        &c49.faces[0].spell_effects[0],
        InstructionAst::ForEachSubjectPlayer { body, .. }
            if matches!(body.as_slice(), [InstructionAst::Discard { .. }])
    ));

    // Card 50: Novel conditional goad duration + combat-scoped unblockable
    let c50 = compile_custom(
        "Taunt of the Phantom",
        "Instant",
        "Target creature is goaded for as long as you control Taunt of the Phantom. It can't be blocked that combat.",
        &vocab,
    );
    assert_eq!(c50.faces[0].spell_effects.len(), 2);
    assert!(
        matches!(
            &c50.faces[0].spell_effects[0],
            InstructionAst::TemporaryStatic {
                static_ability,
                duration: DurationAst::ForAsLongAs(_),
            } if matches!(
                static_ability.as_ref(),
                StaticAbilityAst::RuleOrCombatRestriction { rule, .. }
                    if rule == "permanents_are_goaded"
            )
        ),
        "unexpected c50[0]: {:?}",
        c50.faces[0].spell_effects[0]
    );
    assert!(matches!(
        &c50.faces[0].spell_effects[1],
        InstructionAst::CombatRuleMod {
            duration: DurationAst::UntilEndOfCombat,
            ..
        }
    ));
}

/// 5. All 20 adversarial novel custom cards verifying zero silent mis-parses across every
///    confirmed parser bug class, plus hardened auditor self-tests.
#[test]
fn adversarial_20_custom_cards_deep_semantic_assertions() {
    let vocab = custom_vocab();

    // 1. Aether Tithe Collector: static cost tax on opponent noncreature spells + opponent cast trigger
    let c1 = compile_custom(
        "Aether Tithe Collector",
        "Creature — Human Advisor",
        "Noncreature spells your opponents cast cost {1} more to cast.\nWhenever an opponent casts a spell from anywhere other than their hand, you draw a card and create a Treasure token.",
        &vocab,
    );
    assert_eq!(c1.faces[0].statics.len(), 1);
    match &c1.faces[0].statics[0] {
        StaticAbilityAst::CostModifier {
            subject,
            delta,
            less,
            ..
        } => {
            assert_eq!(delta, "{1}");
            assert!(!less);
            match subject.as_ref() {
                SubjectRefAst::Group(flt) => {
                    assert_eq!(flt.forbidden_types, CardTypeMask::CREATURE);
                    assert_eq!(flt.controller, Some(ControllerRelation::Opponents));
                }
                other => panic!("expected Group(noncreature opp spells), got {other:?}"),
            }
        }
        other => panic!("expected CostModifier, got {other:?}"),
    }
    let AbilityAst::Triggered { trigger, body, .. } = &c1.faces[0].abilities[0] else {
        panic!("expected triggered ability");
    };
    assert!(
        matches!(
            trigger.events.as_slice(),
            [TriggerEventAst::CastFromOutsideHand {
                player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent
            }] | [TriggerEventAst::KeywordActionPerformed {
                player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
                ..
            }]
        ),
        "opponent trigger must NOT attribute actor to You: {:?}",
        trigger.events
    );
    assert_eq!(body.len(), 2);

    // 2. Chronosiphon Field: exile with time counters + unqualified `gains suspend` (Permanent!)
    let c2 = compile_custom(
        "Chronosiphon Field",
        "Sorcery",
        "Exile target creature with three time counters on it. If it doesn't have suspend, it gains suspend.",
        &vocab,
    );
    assert_eq!(c2.faces[0].spell_effects.len(), 2);
    assert!(matches!(
        &c2.faces[0].spell_effects[0],
        InstructionAst::ExileWithCounters { counter_kind, count, .. }
            if counter_kind == "time" && matches!(count.as_ref(), ValueExprAst::Const(3))
    ));
    assert!(matches!(
        &c2.faces[0].spell_effects[1],
        InstructionAst::IfCondition { then_body, .. }
            if matches!(
                then_body.as_slice(),
                [InstructionAst::GrantKeywords {
                    keywords,
                    duration: DurationAst::Permanent,
                    ..
                }] if keywords == &vec!["suspend".to_string()]
            )
    ));

    // 3. Council of Shattered Crowns: VillainousChoice keeping `, then ...` inside Option 2
    let c3 = compile_custom(
        "Council of Shattered Crowns",
        "Sorcery",
        "Each opponent faces a villainous choice — That player loses 4 life, or you draw two cards, then discard a card.",
        &vocab,
    );
    assert_eq!(
        c3.faces[0].spell_effects.len(),
        1,
        "VillainousChoice must NOT leak `, then discard a card` outside Option 2: {:?}",
        c3.faces[0].spell_effects
    );
    match &c3.faces[0].spell_effects[0] {
        InstructionAst::InteractiveChoice(InteractiveChoiceKind::VillainousChoice {
            chooser: PlayerRefAst::EachOpponent,
            options,
        }) => {
            assert_eq!(options.len(), 2);
            assert!(matches!(
                options[0].as_slice(),
                [InstructionAst::ChangeLife {
                    player: PlayerRefAst::ThatPlayer,
                    gain: false,
                    ..
                }]
            ));
            assert_eq!(options[1].len(), 2);
            assert!(matches!(
                &options[1][0],
                InstructionAst::Draw {
                    player: PlayerRefAst::You,
                    ..
                }
            ));
            assert!(matches!(
                &options[1][1],
                InstructionAst::Discard {
                    player: PlayerRefAst::You,
                    ..
                }
            ));
        }
        other => panic!("expected VillainousChoice, got {other:?}"),
    }

    // 4. Cartographer's Balance: player-subject propagation across `and` (`EachPlayer` / `ThatPlayer`, never `You`)
    let c4 = compile_custom(
        "Cartographer's Balance",
        "Sorcery",
        "Each player creates a Treasure token and draws a card.",
        &vocab,
    );
    assert_eq!(c4.faces[0].spell_effects.len(), 2);
    assert!(matches!(
        &c4.faces[0].spell_effects[0],
        InstructionAst::CreateToken {
            controller: PlayerRefAst::EachPlayer,
            ..
        }
    ));
    assert!(matches!(
        &c4.faces[0].spell_effects[1],
        InstructionAst::Draw {
            player: PlayerRefAst::EachPlayer,
            ..
        }
    ));

    // 5. Soulforged Crucible: `artifact creature` (AND via TypesMatchAll) vs `artifact or creature` (OR)
    let c5 = compile_custom(
        "Soulforged Crucible",
        "Artifact",
        "{2}, {T}: Destroy target artifact creature.\n{3}, {T}: Exile target artifact or creature.",
        &vocab,
    );
    let AbilityAst::Activated { body: b5a, .. } = &c5.faces[0].abilities[0] else {
        panic!("expected first activated ability");
    };
    let AbilityAst::Activated { body: b5b, .. } = &c5.faces[0].abilities[1] else {
        panic!("expected second activated ability");
    };
    match &b5a[0] {
        InstructionAst::Destroy { subject, .. } => match subject.as_ref() {
            SubjectRefAst::TargetObject { filter, .. } => {
                assert_eq!(
                    filter.any_types,
                    CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE)
                );
                assert!(
                    filter
                        .predicates
                        .contains(&FilterPredicateAst::TypesMatchAll),
                    "`artifact creature` must carry FilterPredicateAst::TypesMatchAll: {filter:?}"
                );
            }
            other => panic!("expected TargetObject, got {other:?}"),
        },
        other => panic!("expected Destroy, got {other:?}"),
    }
    match &b5b[0] {
        InstructionAst::Exile { subject, .. } => match subject.as_ref() {
            SubjectRefAst::TargetObject { filter, .. } => {
                assert_eq!(
                    filter.any_types,
                    CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE)
                );
                assert!(
                    !filter
                        .predicates
                        .contains(&FilterPredicateAst::TypesMatchAll),
                    "`artifact or creature` must NOT carry TypesMatchAll: {filter:?}"
                );
            }
            other => panic!("expected TargetObject, got {other:?}"),
        },
        other => panic!("expected Exile, got {other:?}"),
    }

    // 6. Arcane Tax Collector: static CostModifier preserving `you cast` + scaling with EventCountThisTurn (zero PriorResult!)
    let c6 = compile_custom(
        "Arcane Tax Collector",
        "Creature — Wizard",
        "Instant and sorcery spells you cast cost {1} less to cast for each spell you've cast this turn.",
        &vocab,
    );
    assert_eq!(c6.faces[0].statics.len(), 1);
    match &c6.faces[0].statics[0] {
        StaticAbilityAst::CostModifier {
            subject,
            delta,
            less,
            per,
            scale_expr,
            ..
        } => {
            assert_eq!(delta, "{1}");
            assert!(*less);
            match subject.as_ref() {
                SubjectRefAst::Group(flt) => {
                    assert_eq!(
                        flt.any_types,
                        CardTypeMask::INSTANT.union(CardTypeMask::SORCERY)
                    );
                    assert_eq!(flt.controller, Some(ControllerRelation::You));
                }
                other => panic!("expected typed Group filter, got {other:?}"),
            }
            assert!(
                !matches!(scale_expr.as_deref(), Some(ValueExprAst::PriorResult)),
                "static CostModifier scale must NEVER be PriorResult: {scale_expr:?}"
            );
            assert!(
                matches!(
                    scale_expr.as_deref(),
                    Some(ValueExprAst::EventCountThisTurn {
                        player: Some(PlayerRefAst::You),
                        ..
                    })
                ) || matches!(per.as_deref(), Some(f) if f.any_spell),
                "expected EventCountThisTurn or typed spell per-filter, got per={per:?} scale={scale_expr:?}"
            );
        }
        other => panic!("expected CostModifier, got {other:?}"),
    }

    // 7. The Fall of Spirehold: Saga with multi-line reminder text, `power or toughness`, and `with a finality counter on it`
    let c7 = compile_custom(
        "The Fall of Spirehold",
        "Enchantment — Saga",
        "(As this Saga enters and after your draw step,\nadd a lore counter. Sacrifice after III.)\nI — Destroy all creatures with power or toughness 3 or less.\nII — Each player discards a card and loses 2 life.\nIII — Return target creature card from your graveyard to the battlefield with a finality counter on it.",
        &vocab,
    );
    assert_eq!(c7.faces[0].abilities.len(), 3);
    let AbilityAst::SagaChapter { body: ch1, .. } = &c7.faces[0].abilities[0] else {
        panic!("expected chapter I");
    };
    assert!(matches!(
        &ch1[0],
        InstructionAst::Destroy { subject, .. }
            if matches!(subject.as_ref(), SubjectRefAst::All(f)
                if f.predicates.iter().any(|p| matches!(p, FilterPredicateAst::AnyOf(b) if b.len() == 2)))
    ));
    let AbilityAst::SagaChapter { body: ch2, .. } = &c7.faces[0].abilities[1] else {
        panic!("expected chapter II");
    };
    assert_eq!(ch2.len(), 2);
    assert!(matches!(
        &ch2[1],
        InstructionAst::ChangeLife {
            player: PlayerRefAst::EachPlayer,
            gain: false,
            ..
        }
    ));
    let AbilityAst::SagaChapter { body: ch3, .. } = &c7.faces[0].abilities[2] else {
        panic!("expected chapter III");
    };
    assert!(matches!(
        &ch3[0],
        InstructionAst::ReturnZone {
            with_counter: Some(ck),
            ..
        } if ck == "finality"
    ));

    // 8. Aegis of Reflected Fury: explicit `until end of turn` vs unqualified permanent keyword grant
    let c8 = compile_custom(
        "Aegis of Reflected Fury",
        "Instant",
        "Target creature gets +2/+0 and gains first strike until end of turn. Another target creature gains vigilance.",
        &vocab,
    );
    assert_eq!(c8.faces[0].spell_effects.len(), 3);
    assert!(matches!(
        &c8.faces[0].spell_effects[1],
        InstructionAst::GrantKeywords {
            keywords,
            duration: DurationAst::UntilEndOfTurn,
            ..
        } if keywords == &vec!["first strike".to_string()]
    ));
    assert!(matches!(
        &c8.faces[0].spell_effects[2],
        InstructionAst::GrantKeywords {
            keywords,
            duration: DurationAst::Permanent,
            ..
        } if keywords == &vec!["vigilance".to_string()]
    ));

    // 9. High Court Referendum: VillainousChoice with `They discard ... and lose 4 life` (ThatPlayer!)
    let c9 = compile_custom(
        "High Court Referendum",
        "Sorcery",
        "Each opponent faces a villainous choice — They discard two cards and lose 4 life, or you scry 2, then draw two cards.",
        &vocab,
    );
    assert_eq!(c9.faces[0].spell_effects.len(), 1);
    match &c9.faces[0].spell_effects[0] {
        InstructionAst::InteractiveChoice(InteractiveChoiceKind::VillainousChoice {
            chooser: PlayerRefAst::EachOpponent,
            options,
        }) => {
            assert_eq!(options.len(), 2);
            assert_eq!(options[0].len(), 2);
            assert!(matches!(
                &options[0][0],
                InstructionAst::Discard {
                    player: PlayerRefAst::ThatPlayer,
                    ..
                }
            ));
            assert!(matches!(
                &options[0][1],
                InstructionAst::ChangeLife {
                    player: PlayerRefAst::ThatPlayer,
                    gain: false,
                    ..
                }
            ));
            assert_eq!(options[1].len(), 2);
            assert!(matches!(&options[1][0], InstructionAst::Scry { .. }));
            assert!(matches!(&options[1][1], InstructionAst::Draw { .. }));
        }
        other => panic!("expected VillainousChoice, got {other:?}"),
    }

    // 10. Architect of Ruin: multi-clause put/return MUST NOT greedily swallow intermediate destination into `from_zone`
    let c10 = compile_custom(
        "Architect of Ruin",
        "Sorcery",
        "Put a creature card from your graveyard onto the battlefield, then put a card from your hand into your graveyard.",
        &vocab,
    );
    assert_eq!(c10.faces[0].spell_effects.len(), 2);
    for ins in &c10.faces[0].spell_effects {
        if let InstructionAst::ReturnZone { from_zone, .. } = ins {
            assert!(
                from_zone.as_deref().is_none_or(|fz| !fz.contains("onto")),
                "greedy from_zone leaked destination: {from_zone:?}"
            );
        }
    }

    // 11. Blade of the Fractured Sun: conditional untap + additional combat MUST NOT be swallowed into single AdditionalCombat
    let c11 = compile_custom(
        "Blade of the Fractured Sun",
        "Artifact — Equipment",
        "Equipped creature gets +2/+2.\nWhenever equipped creature attacks, if it's the first combat phase of the turn, untap all creatures that attacked this turn and there is an additional combat phase after this phase.",
        &vocab,
    );
    let AbilityAst::Triggered { trigger, body, .. } = &c11.faces[0].abilities[0] else {
        panic!("expected attack trigger");
    };
    assert!(trigger.intervening_if.is_some());
    assert_eq!(
        body.len(),
        2,
        "must preserve BOTH Untap and AdditionalCombat: {body:?}"
    );
    assert!(matches!(&body[0], InstructionAst::UntapPermanent { .. }));
    assert!(matches!(&body[1], InstructionAst::AdditionalCombat(_)));

    // 12. Searing Convergence: `differently named` -> DIFFERENT_NAMES + `power or toughness` -> AnyOf
    let c12 = compile_custom(
        "Searing Convergence",
        "Sorcery",
        "Exile up to two target differently named creatures with power or toughness 4 or less.",
        &vocab,
    );
    assert_eq!(c12.faces[0].spell_effects.len(), 1);
    match &c12.faces[0].spell_effects[0] {
        InstructionAst::Exile { subject, .. } => match subject.as_ref() {
            SubjectRefAst::TargetObject { filter, .. } => {
                assert_ne!(
                    filter.flags.0 & ObjectFilterFlags::DIFFERENT_NAMES.0,
                    0,
                    "differently named must set DIFFERENT_NAMES"
                );
                assert!(
                    filter
                        .predicates
                        .iter()
                        .any(|p| matches!(p, FilterPredicateAst::AnyOf(b) if b.len() == 2)),
                    "power or toughness must emit AnyOf"
                );
            }
            other => panic!("expected TargetObject, got {other:?}"),
        },
        other => panic!("expected Exile, got {other:?}"),
    }

    // 13. Echoes of the Blind Eternities: exile + unqualified `gains suspend` (Permanent)
    let c13 = compile_custom(
        "Echoes of the Blind Eternities",
        "Instant",
        "Exile target spell with two time counters on it. It gains suspend.",
        &vocab,
    );
    assert_eq!(c13.faces[0].spell_effects.len(), 2);
    assert!(matches!(
        &c13.faces[0].spell_effects[1],
        InstructionAst::GrantKeywords {
            keywords,
            duration: DurationAst::Permanent,
            ..
        } if keywords == &vec!["suspend".to_string()]
    ));

    // 14. Obelisk of Narrow Paths: first opponent spell cost tax + opponent trigger actor check
    let c14 = compile_custom(
        "Obelisk of Narrow Paths",
        "Artifact",
        "The first spell an opponent casts each turn costs {2} more to cast.\nWhenever an opponent casts a spell from anywhere other than their hand, each opponent loses 2 life.",
        &vocab,
    );
    assert_eq!(c14.faces[0].statics.len(), 1);
    match &c14.faces[0].statics[0] {
        StaticAbilityAst::CostModifier {
            subject,
            delta,
            less,
            ..
        } => {
            assert_eq!(delta, "{2}");
            assert!(!less);
            match subject.as_ref() {
                SubjectRefAst::Group(flt) => {
                    assert_eq!(flt.controller, Some(ControllerRelation::Opponent));
                    assert!(
                        flt.predicates
                            .contains(&FilterPredicateAst::NthSpellCastThisTurn(1)),
                        "must preserve first spell qualifier: {flt:?}"
                    );
                }
                other => panic!("expected Group, got {other:?}"),
            }
        }
        other => panic!("expected CostModifier, got {other:?}"),
    }

    // 15. Crystalline Nexus: `Move a counter` MUST NOT store `counter_kind: "a"`
    let c15 = compile_custom(
        "Crystalline Nexus",
        "Artifact",
        "{1}, {T}: Move a counter from target permanent you control onto another target permanent.\n{2}, {T}: Move two +1/+1 counters from target creature you control onto another target creature.",
        &vocab,
    );
    let AbilityAst::Activated { body: b15a, .. } = &c15.faces[0].abilities[0] else {
        panic!("expected first activated ability");
    };
    assert!(matches!(
        &b15a[0],
        InstructionAst::MoveCounters { counter_kind, count, .. }
            if counter_kind == "any" && matches!(count.as_deref(), Some(ValueExprAst::Const(1)))
    ));
    let AbilityAst::Activated { body: b15b, .. } = &c15.faces[0].abilities[1] else {
        panic!("expected second activated ability");
    };
    assert!(matches!(
        &b15b[0],
        InstructionAst::MoveCounters { counter_kind, count, .. }
            if counter_kind == "+1/+1" && matches!(count.as_deref(), Some(ValueExprAst::Const(2)))
    ));

    // 16. Temporal Excavation: artifact creature (AND) + with_counter("finality") + permanent haste
    let c16 = compile_custom(
        "Temporal Excavation",
        "Sorcery",
        "Return target artifact creature card with mana value 4 or less from your graveyard to the battlefield with a finality counter on it. It gains haste.",
        &vocab,
    );
    assert_eq!(c16.faces[0].spell_effects.len(), 2);
    match &c16.faces[0].spell_effects[0] {
        InstructionAst::ReturnZone {
            subject,
            with_counter,
            ..
        } => {
            assert_eq!(with_counter.as_deref(), Some("finality"));
            match subject.as_ref() {
                SubjectRefAst::TargetObject { filter, .. } => {
                    assert_eq!(
                        filter.any_types,
                        CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE)
                    );
                    assert!(
                        filter
                            .predicates
                            .contains(&FilterPredicateAst::TypesMatchAll)
                    );
                }
                other => panic!("expected TargetObject, got {other:?}"),
            }
        }
        other => panic!("expected ReturnZone, got {other:?}"),
    }
    assert!(matches!(
        &c16.faces[0].spell_effects[1],
        InstructionAst::GrantKeywords {
            keywords,
            duration: DurationAst::Permanent,
            ..
        } if keywords == &vec!["haste".to_string()]
    ));

    // 17. Flux Harvester: opponent sacrifice trigger + differently named artifact cost
    let c17 = compile_custom(
        "Flux Harvester",
        "Artifact",
        "Whenever an opponent sacrifices a creature, create a Treasure token and you gain 1 life.\nSacrifice two differently named artifacts: Draw two cards.",
        &vocab,
    );
    let AbilityAst::Triggered { trigger, body, .. } = &c17.faces[0].abilities[0] else {
        panic!("expected trigger");
    };
    assert!(matches!(
        trigger.events.as_slice(),
        [TriggerEventAst::PermanentSacrificed {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            ..
        }]
    ));
    assert_eq!(body.len(), 2);
    let AbilityAst::Activated { cost, .. } = &c17.faces[0].abilities[1] else {
        panic!("expected activated ability");
    };
    assert!(
        matches!(
            cost.as_slice(),
            [spellwake_oracle_compiler::ast::CostAtomAst::Sacrifice { filter, .. }]
                if (filter.flags.0 & ObjectFilterFlags::DIFFERENT_NAMES.0) != 0
        ),
        "expected DIFFERENT_NAMES on sacrifice cost: {cost:?}"
    );

    // 18. Chains of Borrowed Loyalty: power or toughness 4 or less + permanent control + permanent haste
    let c18 = compile_custom(
        "Chains of Borrowed Loyalty",
        "Sorcery",
        "Gain control of target creature with power or toughness 4 or less. It gains haste.",
        &vocab,
    );
    assert_eq!(c18.faces[0].spell_effects.len(), 2);
    assert!(matches!(
        &c18.faces[0].spell_effects[0],
        InstructionAst::GainControl {
            controller: PlayerRefAst::You,
            duration: DurationAst::Permanent,
            subject,
        } if matches!(subject.as_ref(), SubjectRefAst::TargetObject { filter, .. }
            if filter.predicates.iter().any(|p| matches!(p, FilterPredicateAst::AnyOf(b) if b.len() == 2)))
    ));
    assert!(matches!(
        &c18.faces[0].spell_effects[1],
        InstructionAst::GrantKeywords {
            keywords,
            duration: DurationAst::Permanent,
            ..
        } if keywords == &vec!["haste".to_string()]
    ));

    // 19. Warlord's Reckoning: leading `If <cond>, untap ... and there is an additional combat phase`
    let c19 = compile_custom(
        "Warlord's Reckoning",
        "Sorcery",
        "If it's the first combat phase of the turn, untap all creatures that attacked this turn and there is an additional combat phase after this phase.",
        &vocab,
    );
    assert_eq!(c19.faces[0].spell_effects.len(), 1);
    match &c19.faces[0].spell_effects[0] {
        InstructionAst::IfCondition {
            condition,
            then_body,
        } => {
            assert!(!matches!(condition.as_ref(), ConditionAst::WasKicked));
            assert_eq!(then_body.len(), 2);
            assert!(matches!(
                &then_body[0],
                InstructionAst::UntapPermanent { .. }
            ));
            assert!(matches!(&then_body[1], InstructionAst::AdditionalCombat(_)));
        }
        other => panic!("expected IfCondition([Untap, AdditionalCombat]), got {other:?}"),
    }

    // 20. Kaelen, Riftbound Tactician: Planeswalker combining EachPlayer propagation + Move a counter + Emblem
    let c20 = compile_custom(
        "Kaelen, Riftbound Tactician",
        "Legendary Planeswalker — Kaelen",
        "+1: Each player creates a 1/1 white Soldier creature token and draws a card.\n-2: Move a counter from target permanent onto another target permanent.\n-6: You get an emblem with \"Noncreature spells you cast cost {1} less to cast.\"",
        &vocab,
    );
    assert_eq!(c20.faces[0].abilities.len(), 3);
    let AbilityAst::Loyalty { body: l1, .. } = &c20.faces[0].abilities[0] else {
        panic!("expected +1");
    };
    assert_eq!(l1.len(), 2);
    assert!(matches!(
        &l1[1],
        InstructionAst::Draw {
            player: PlayerRefAst::EachPlayer,
            ..
        }
    ));
    let AbilityAst::Loyalty { body: l2, .. } = &c20.faces[0].abilities[1] else {
        panic!("expected -2");
    };
    assert!(matches!(
        &l2[0],
        InstructionAst::MoveCounters { counter_kind, .. } if counter_kind == "any"
    ));
    let AbilityAst::Loyalty { body: l3, .. } = &c20.faces[0].abilities[2] else {
        panic!("expected -6");
    };
    assert!(matches!(
        &l3[0],
        InstructionAst::CreateEmblemFor { compiled: Some(face), .. }
            if matches!(
                face.statics.as_slice(),
                [StaticAbilityAst::CostModifier { subject, less: true, .. }]
                    if matches!(subject.as_ref(), SubjectRefAst::Group(f) if f.controller == Some(ControllerRelation::You))
            )
    ));
}

/// 6. Verify hardened `audit_card` rejects space-stripped prose laundering, PriorResult in
///    statics, malformed counter_kind/with_counter, greedy from_zone, and all raw escape hatches.
#[test]
fn hardened_audit_catches_structural_and_opaque_escapes() {
    use spellwake_oracle_compiler::ast::FaceAst;

    let input = CardInput {
        oracle_id: "audit-hardening-test",
        name: "Synthetic Audit Probe",
        layout: "normal",
        faces: vec![FaceInput {
            name: "Synthetic Audit Probe",
            type_line: "Artifact",
            oracle_text: "Spells you cast cost {1} less to cast.",
        }],
    };
    let ast = CardAst {
        oracle_id: "audit-hardening-test".to_string(),
        name: "Synthetic Audit Probe".to_string(),
        layout: "normal".to_string(),
        strictly_compiled: true,
        uses_tabletop_honor_system: false,
        faces: vec![FaceAst {
            name: "Synthetic Audit Probe".to_string(),
            type_line: "Artifact".to_string(),
            statics: vec![
                StaticAbilityAst::CostModifier {
                    subject: Box::new(SubjectRefAst::Anaphora(
                        "noncreature spells your opponents cast".to_string(),
                    )),
                    delta: "{1}".to_string(),
                    less: true,
                    next_only: false,
                    per: None,
                    scale_expr: Some(Box::new(ValueExprAst::PriorResult)),
                    condition: None,
                },
                StaticAbilityAst::LookOrRevealRule {
                    rule: "PlayWithHandRevealed".to_string(),
                    filter: None,
                    follow_up: Vec::new(),
                    condition: None,
                },
            ],
            spell_effects: vec![
                InstructionAst::MoveCounters {
                    from: Box::new(SubjectRefAst::SelfObject),
                    to: Box::new(SubjectRefAst::SelfObject),
                    counter_kind: "a".to_string(),
                    all: false,
                    any_number: false,
                    count: Some(Box::new(ValueExprAst::Const(1))),
                },
                InstructionAst::ReturnZone {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    from_zone: Some(
                        "your graveyard onto the battlefield, then put a card from your hand"
                            .to_string(),
                    ),
                    to: "your graveyard".to_string(),
                    tapped: false,
                    transformed: false,
                    with_counter: Some("a finality".to_string()),
                    attached_to: None,
                    random: false,
                    except: None,
                    count: None,
                    up_to: false,
                    any_number: false,
                    actor: None,
                },
                InstructionAst::AbilityRestriction {
                    restriction: "YouHaveNoMaxHandSize".to_string(),
                    condition: None,
                },
                InstructionAst::CombatRuleMod {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    rule: "cant_block".to_string(),
                    duration: DurationAst::UntilEndOfTurn,
                },
                InstructionAst::AdditionalCombat("additional combat phase".to_string()),
                InstructionAst::ChooseOption("a color".to_string()),
            ],
            ..FaceAst::default()
        }],
    };

    let report = audit_card(&input, &ast, CompileOptions::default());
    let crit_codes: Vec<&str> = report
        .critical_defects
        .iter()
        .map(|f| f.code.as_str())
        .collect();
    let opaque_codes: Vec<&str> = report
        .opaque_strings
        .iter()
        .map(|f| f.code.as_str())
        .collect();

    assert!(
        crit_codes.contains(&"prior_result_in_static"),
        "must flag PriorResult in static: {crit_codes:?}"
    );
    assert!(
        crit_codes.contains(&"malformed_counter_kind"),
        "must flag malformed counter_kind/with_counter: {crit_codes:?}"
    );
    assert!(
        crit_codes.contains(&"greedy_from_zone"),
        "must flag greedy from_zone: {crit_codes:?}"
    );
    assert!(
        opaque_codes.contains(&"opaque:Anaphora_prose"),
        "must flag non-allowlisted multi-word Anaphora: {opaque_codes:?}"
    );
    assert!(
        opaque_codes.contains(&"opaque:LookOrRevealRule"),
        "must flag LookOrRevealRule even without spaces: {opaque_codes:?}"
    );
    assert!(
        opaque_codes.contains(&"opaque:AbilityRestriction_prose"),
        "must flag space-stripped prose AbilityRestriction: {opaque_codes:?}"
    );
    assert!(
        opaque_codes.contains(&"opaque:CombatRuleMod"),
        "must flag CombatRuleMod: {opaque_codes:?}"
    );
    assert!(
        opaque_codes.contains(&"opaque:AdditionalCombat"),
        "must flag AdditionalCombat: {opaque_codes:?}"
    );
    assert!(
        opaque_codes.contains(&"opaque:ChooseOption"),
        "must flag ChooseOption: {opaque_codes:?}"
    );
}
