//! Custom-Card Compositional Stress & Normalization Suite.
//!
//! Validates that `spellwake-oracle-compiler` parses brand-new, never-printed synthetic Magic
//! cards deterministically via compositional grammar rules (not printed-card memorization) and
//! decodes their escape-hatch fields cleanly through `typed_ir`.

use spellwake_oracle_compiler::ast::{
    AbilityAst, CardAst, CardTypeMask, ColorMask, ControllerRelation, DurationAst,
    FilterPredicateAst, InstructionAst, InteractiveChoiceKind, PlayerRefAst, ReplacementEffectAst,
    Rounding, StatKind, StaticAbilityAst, SubjectRefAst, TokenSpecAst, TurnStepAst, ValueExprAst,
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
    assert!(c6.faces[0].spell_effects.len() >= 2);

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

    // Card 8: Novel `with base power and toughness P/T` + `with power or toughness N or less`
    let c8 = compile_custom(
        "Scalewright's Verdict",
        "Sorcery",
        "Exile all creatures with power or toughness 5 or less. Draw a card for each creature exiled this way.",
        &vocab,
    );
    assert_eq!(c8.faces[0].spell_effects.len(), 2);

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
    assert!(!c12.faces[0].spell_effects.is_empty());

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
    assert!(body.len() >= 2);
    assert!(matches!(
        &body[1],
        InstructionAst::PutCounters { count, .. }
            if matches!(count.as_ref(), ValueExprAst::Mul(_, rhs) if matches!(rhs.as_ref(), ValueExprAst::Sub(_, _)))
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
    assert!(!c16.faces[0].statics.is_empty());
    assert!(!c16.faces[0].replacements.is_empty());

    // Card 17: Novel `under the control of that creature's owner` delayed death trigger
    let c17 = compile_custom(
        "Soul Tether",
        "Instant",
        "Choose target creature. When that creature dies this turn, return a creature card from its owner's graveyard to the battlefield under the control of that creature's owner.",
        &vocab,
    );
    assert_eq!(c17.faces[0].spell_effects.len(), 2);

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
            InstructionAst::ManaSpendRider { .. } | InstructionAst::TemporaryTrigger { .. }
        )),
        "expected ManaSpendRider or TemporaryTrigger in {body:?}"
    );

    // Card 26: Novel keyword-choice grant (`gains your choice of ... until end of turn`)
    let c26 = compile_custom(
        "Adaptive Carapace",
        "Instant",
        "Target creature you control gains your choice of flying, deathtouch, or hexproof until end of turn.",
        &vocab,
    );
    assert!(
        matches!(
            c26.faces[0].spell_effects.first(),
            Some(InstructionAst::GrantKeywordChoice { choices, duration: DurationAst::UntilEndOfTurn, .. })
                if choices.len() == 3
        ) || matches!(
            c26.faces[0].spell_effects.first(),
            Some(InstructionAst::Choice(branches)) if branches.len() == 3
        )
    );

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
    assert!(c31.faces[0].spell_effects.len() >= 2);

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
}
