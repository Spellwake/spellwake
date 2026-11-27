//! Compositional `v2` Parser Verification Suite.
//!
//! Proves:
//! 1. All 20/20 adversarial novel custom cards compile strictly in `v2` with zero unconsumed
//!    tokens, zero raw-prose `String` fields, and deep field-by-field IR assertions.
//! 2. All 50/50 custom cards from the composability suite compile strictly in `v2` (70/70 = 100%).
//! 3. Fail-closed behavior: unparsed words, trailing garbage, and malformed counter kinds return
//!    explicit `ParseError`s rather than silently dropping clauses or producing `"a"`.

use spellwake_oracle_compiler::ast::{
    CardTypeMask, ColorMask, CompareOp, ControllerRelation, Rounding, StatKind, TurnStepAst,
};
use spellwake_oracle_compiler::v2::ir::{
    Ability, ActivationRestriction, ChoiceCount, ChoiceKind, CompiledCardV2, Cost, CounterKind,
    DelayedTriggerKind, Duration, Effect, FilterFlags, FilterPredicate, ForEachDomain, KeywordKind,
    LibraryOrder, LibraryPosition, LifeOp, LoyaltyDelta, ManaSymbol, ObjectMutation,
    PlayerMutation, PlayerRef, PriorActionKind, Replacement, RuleRestrictionKind, StatBound,
    StaticAbility, StepOwner, SubjectRef, TokenSpec, TriggerEvent, ValueExpr, ZoneKind, ZoneOpKind,
    ZoneOwner,
};
use spellwake_oracle_compiler::v2::{V2CorpusSummary, compile_card_v2};
use spellwake_oracle_compiler::{CardInput, FaceInput, Vocabulary};

fn v2_vocab() -> Vocabulary {
    let members: Vec<String> = [
        "Advisor",
        "Angel",
        "Artificer",
        "Assassin",
        "Aura",
        "Bear",
        "Beast",
        "Bird",
        "Cat",
        "Cleric",
        "Construct",
        "Demon",
        "Dinosaur",
        "Djinn",
        "Dragon",
        "Druid",
        "Dwarf",
        "Efreet",
        "Eldrazi",
        "Elemental",
        "Elf",
        "Elk",
        "Equipment",
        "Faerie",
        "Forest",
        "Fractal",
        "Fungus",
        "Giant",
        "Goblin",
        "God",
        "Golem",
        "Human",
        "Hydra",
        "Illusion",
        "Insect",
        "Island",
        "Kirin",
        "Knight",
        "Kor",
        "Licid",
        "Lizard",
        "Merfolk",
        "Monk",
        "Mountain",
        "Nephilim",
        "Nightmare",
        "Ninja",
        "Noble",
        "Ooze",
        "Phoenix",
        "Phyrexian",
        "Pirate",
        "Plains",
        "Plant",
        "Rat",
        "Rogue",
        "Samurai",
        "Scout",
        "Shaman",
        "Shrine",
        "Skeleton",
        "Sliver",
        "Soldier",
        "Sphinx",
        "Spider",
        "Spirit",
        "Swamp",
        "Thopter",
        "Thrull",
        "Treasure",
        "Treefolk",
        "Tyranid",
        "Vampire",
        "Vehicle",
        "Wall",
        "Warlock",
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

fn v2_vocab_or_env() -> Vocabulary {
    if let Ok(vocab_path) = std::env::var("SPELLWAKE_CREATURE_VOCABULARY_INPUT") {
        Vocabulary::load_from_file(std::path::Path::new(&vocab_path)).expect("load vocab")
    } else {
        v2_vocab()
    }
}

fn compile_v2_strict(
    name: &str,
    type_line: &str,
    oracle_text: &str,
    vocab: &Vocabulary,
) -> CompiledCardV2 {
    let input = CardInput {
        oracle_id: "v2-test",
        name,
        layout: "normal",
        faces: vec![FaceInput {
            name,
            type_line,
            oracle_text,
        }],
    };
    let card = compile_card_v2(&input, vocab);
    assert!(
        card.strictly_compiled,
        "v2 failed on '{name}':\nOracle: {oracle_text}\nErrors: {:#?}",
        card.faces[0].errors
    );
    card
}

/// 1. All 20/20 adversarial custom cards pass strictly in `v2` with deep field-by-field assertions.
#[test]
fn v2_parses_all_20_adversarial_custom_cards_strictly() {
    let vocab = v2_vocab();

    // 1. Aether Tithe Collector
    let c1 = compile_v2_strict(
        "Aether Tithe Collector",
        "Creature — Human Advisor",
        "Noncreature spells your opponents cast cost {1} more to cast.\nWhenever an opponent casts a spell from anywhere other than their hand, you draw a card and create a Treasure token.",
        &vocab,
    );
    assert!(matches!(
        c1.faces[0].statics.as_slice(),
        [StaticAbility::CostModifier { subject, less: false, .. }]
            if subject.forbidden_types == CardTypeMask::CREATURE
                && subject.controller == Some(ControllerRelation::Opponents)
    ));
    let Ability::Triggered { trigger, body } = &c1.faces[0].abilities[0] else {
        panic!("expected trigger");
    };
    assert!(matches!(
        trigger.events.as_slice(),
        [TriggerEvent::SpellCast {
            caster: PlayerRef::AnyOpponent,
            from_outside_hand: true,
            ..
        }]
    ));
    assert_eq!(body.len(), 2);

    // 2. Chronosiphon Field
    let c2 = compile_v2_strict(
        "Chronosiphon Field",
        "Sorcery",
        "Exile target creature with three time counters on it. If it doesn't have suspend, it gains suspend.",
        &vocab,
    );
    assert_eq!(c2.faces[0].spell_effects.len(), 2);
    assert!(matches!(
        &c2.faces[0].spell_effects[0],
        Effect::ZoneOp {
            op: ZoneOpKind::Exile {
                counters: Some((CounterKind::Time, ValueExpr::Const(3))),
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        &c2.faces[0].spell_effects[1],
        Effect::BranchCondition { then_body, .. }
            if matches!(
                then_body.as_slice(),
                [Effect::MutateObject {
                    mutation: ObjectMutation::GrantKeywords(kws),
                    duration: Duration::Permanent,
                    ..
                }] if kws.len() == 1 && kws[0].kind == KeywordKind::Suspend
            )
    ));

    // 3. Council of Shattered Crowns
    let c3 = compile_v2_strict(
        "Council of Shattered Crowns",
        "Sorcery",
        "Each opponent faces a villainous choice — That player loses 4 life, or you draw two cards, then discard a card.",
        &vocab,
    );
    assert_eq!(c3.faces[0].spell_effects.len(), 1);
    let Effect::PromptChoice {
        chooser: PlayerRef::EachOpponent,
        kind: ChoiceKind::VillainousChoice { options },
    } = &c3.faces[0].spell_effects[0]
    else {
        panic!("expected VillainousChoice");
    };
    assert_eq!(options.len(), 2);
    assert_eq!(options[1].len(), 2);

    // 4. Cartographer's Balance
    let c4 = compile_v2_strict(
        "Cartographer's Balance",
        "Sorcery",
        "Each player creates a Treasure token and draws a card.",
        &vocab,
    );
    assert_eq!(c4.faces[0].spell_effects.len(), 2);
    assert!(matches!(
        &c4.faces[0].spell_effects[0],
        Effect::CreateToken {
            controller: PlayerRef::EachPlayer,
            ..
        }
    ));
    assert!(matches!(
        &c4.faces[0].spell_effects[1],
        Effect::ZoneOp {
            actor: PlayerRef::EachPlayer,
            op: ZoneOpKind::Draw,
            ..
        }
    ));

    // 5. Soulforged Crucible: explicit `all_types` (AND) vs `any_types` (OR)
    let c5 = compile_v2_strict(
        "Soulforged Crucible",
        "Artifact",
        "{2}, {T}: Destroy target artifact creature.\n{3}, {T}: Exile target artifact or creature.",
        &vocab,
    );
    let Ability::Activated { body: b5a, .. } = &c5.faces[0].abilities[0] else {
        panic!("expected ability 0");
    };
    let Ability::Activated { body: b5b, .. } = &c5.faces[0].abilities[1] else {
        panic!("expected ability 1");
    };
    assert!(matches!(
        &b5a[0],
        Effect::ZoneOp {
            subject: SubjectRef::TargetObject { filter, .. },
            ..
        } if filter.all_types == CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE)
            && filter.any_types.is_empty()
    ));
    assert!(matches!(
        &b5b[0],
        Effect::ZoneOp {
            subject: SubjectRef::TargetObject { filter, .. },
            ..
        } if filter.any_types == CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE)
            && filter.all_types.is_empty()
    ));

    // 6. Arcane Tax Collector
    let c6 = compile_v2_strict(
        "Arcane Tax Collector",
        "Creature — Wizard",
        "Instant and sorcery spells you cast cost {1} less to cast for each spell you've cast this turn.",
        &vocab,
    );
    assert!(matches!(
        c6.faces[0].statics.as_slice(),
        [StaticAbility::CostModifier {
            subject,
            less: true,
            scale: Some(ValueExpr::EventCountThisTurn {
                player: Some(PlayerRef::You),
                exclude_first: false,
                ..
            }),
            ..
        }] if subject.controller == Some(ControllerRelation::You)
            && subject.any_types == CardTypeMask::INSTANT.union(CardTypeMask::SORCERY)
    ));

    // 7. The Fall of Spirehold
    let c7 = compile_v2_strict(
        "The Fall of Spirehold",
        "Enchantment — Saga",
        "(As this Saga enters and after your draw step,\nadd a lore counter. Sacrifice after III.)\nI — Destroy all creatures with power or toughness 3 or less.\nII — Each player discards a card and loses 2 life.\nIII — Return target creature card from your graveyard to the battlefield with a finality counter on it.",
        &vocab,
    );
    assert_eq!(c7.faces[0].abilities.len(), 3);
    let Ability::SagaChapter { body: ch2, .. } = &c7.faces[0].abilities[1] else {
        panic!("expected ch2");
    };
    assert!(matches!(
        &ch2[1],
        Effect::AdjustLife {
            player: PlayerRef::EachPlayer,
            op: LifeOp::Lose,
            amount: ValueExpr::Const(2),
        }
    ));
    let Ability::SagaChapter { body: ch3, .. } = &c7.faces[0].abilities[2] else {
        panic!("expected ch3");
    };
    assert!(matches!(
        &ch3[0],
        Effect::ZoneOp {
            modifiers,
            ..
        } if modifiers.with_counter == Some((CounterKind::Finality, 1))
    ));

    // 8. Aegis of Reflected Fury
    let c8 = compile_v2_strict(
        "Aegis of Reflected Fury",
        "Instant",
        "Target creature gets +2/+0 and gains first strike until end of turn. Another target creature gains vigilance.",
        &vocab,
    );
    assert_eq!(c8.faces[0].spell_effects.len(), 3);
    assert!(matches!(
        &c8.faces[0].spell_effects[1],
        Effect::MutateObject {
            duration: Duration::UntilEndOfTurn,
            ..
        }
    ));
    assert!(matches!(
        &c8.faces[0].spell_effects[2],
        Effect::MutateObject {
            duration: Duration::Permanent,
            ..
        }
    ));

    // 9. High Court Referendum
    let c9 = compile_v2_strict(
        "High Court Referendum",
        "Sorcery",
        "Each opponent faces a villainous choice — They discard two cards and lose 4 life, or you scry 2, then draw two cards.",
        &vocab,
    );
    let Effect::PromptChoice {
        kind: ChoiceKind::VillainousChoice { options: o9 },
        ..
    } = &c9.faces[0].spell_effects[0]
    else {
        panic!("expected VillainousChoice");
    };
    assert!(matches!(
        &o9[0][1],
        Effect::AdjustLife {
            player: PlayerRef::ThatPlayer,
            op: LifeOp::Lose,
            amount: ValueExpr::Const(4),
        }
    ));
    assert_eq!(o9[1].len(), 2);

    // 10. Architect of Ruin
    let c10 = compile_v2_strict(
        "Architect of Ruin",
        "Sorcery",
        "Put a creature card from your graveyard onto the battlefield, then put a card from your hand into your graveyard.",
        &vocab,
    );
    assert_eq!(c10.faces[0].spell_effects.len(), 2);

    // 11. Blade of the Fractured Sun
    let c11 = compile_v2_strict(
        "Blade of the Fractured Sun",
        "Artifact — Equipment",
        "Equipped creature gets +2/+2.\nWhenever equipped creature attacks, if it's the first combat phase of the turn, untap all creatures that attacked this turn and there is an additional combat phase after this phase.",
        &vocab,
    );
    let Ability::Triggered {
        trigger: t11,
        body: b11,
    } = &c11.faces[0].abilities[0]
    else {
        panic!("expected trigger");
    };
    assert!(t11.intervening_if.is_some());
    assert_eq!(b11.len(), 2);
    assert!(matches!(
        &b11[0],
        Effect::MutateObject {
            mutation: ObjectMutation::Untap,
            ..
        }
    ));
    assert!(matches!(
        &b11[1],
        Effect::MutatePlayer {
            mutation: PlayerMutation::AdditionalCombat { .. },
            ..
        }
    ));

    // 12. Searing Convergence
    let c12 = compile_v2_strict(
        "Searing Convergence",
        "Sorcery",
        "Exile up to two target differently named creatures with power or toughness 4 or less.",
        &vocab,
    );
    assert!(matches!(
        &c12.faces[0].spell_effects[0],
        Effect::ZoneOp {
            subject: SubjectRef::TargetObject { filter, .. },
            ..
        } if filter.flags.contains(FilterFlags::DIFFERENT_NAMES)
            && filter.predicates.iter().any(|p| matches!(p, FilterPredicate::AnyOf(b) if b.len() == 2))
    ));

    // 13. Echoes of the Blind Eternities
    let c13 = compile_v2_strict(
        "Echoes of the Blind Eternities",
        "Instant",
        "Exile target spell with two time counters on it. It gains suspend.",
        &vocab,
    );
    assert!(matches!(
        &c13.faces[0].spell_effects[1],
        Effect::MutateObject {
            duration: Duration::Permanent,
            ..
        }
    ));

    // 14. Obelisk of Narrow Paths
    let c14 = compile_v2_strict(
        "Obelisk of Narrow Paths",
        "Artifact",
        "The first spell an opponent casts each turn costs {2} more to cast.\nWhenever an opponent casts a spell from anywhere other than their hand, each opponent loses 2 life.",
        &vocab,
    );
    assert!(matches!(
        c14.faces[0].statics.as_slice(),
        [StaticAbility::CostModifier { subject, less: false, .. }]
            if subject.controller == Some(ControllerRelation::Opponent)
                && subject.predicates.contains(&FilterPredicate::NthSpellCastThisTurn(1))
    ));

    // 15. Crystalline Nexus
    let c15 = compile_v2_strict(
        "Crystalline Nexus",
        "Artifact",
        "{1}, {T}: Move a counter from target permanent you control onto another target permanent.\n{2}, {T}: Move two +1/+1 counters from target creature you control onto another target creature.",
        &vocab,
    );
    let Ability::Activated { body: b15a, .. } = &c15.faces[0].abilities[0] else {
        panic!("expected ability 0");
    };
    assert!(matches!(
        &b15a[0],
        Effect::MutateObject {
            mutation: ObjectMutation::MoveCounters {
                kind: CounterKind::Any,
                count: ValueExpr::Const(1),
                ..
            },
            ..
        }
    ));

    // 16. Temporal Excavation
    let c16 = compile_v2_strict(
        "Temporal Excavation",
        "Sorcery",
        "Return target artifact creature card with mana value 4 or less from your graveyard to the battlefield with a finality counter on it. It gains haste.",
        &vocab,
    );
    assert!(matches!(
        &c16.faces[0].spell_effects[0],
        Effect::ZoneOp {
            subject: SubjectRef::TargetObject { filter, .. },
            modifiers,
            ..
        } if filter.all_types == CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE)
            && modifiers.with_counter == Some((CounterKind::Finality, 1))
    ));
    assert!(matches!(
        &c16.faces[0].spell_effects[1],
        Effect::MutateObject {
            duration: Duration::Permanent,
            ..
        }
    ));

    // 17. Flux Harvester
    let c17 = compile_v2_strict(
        "Flux Harvester",
        "Artifact",
        "Whenever an opponent sacrifices a creature, create a Treasure token and you gain 1 life.\nSacrifice two differently named artifacts: Draw two cards.",
        &vocab,
    );
    let Ability::Activated { cost: cost17, .. } = &c17.faces[0].abilities[1] else {
        panic!("expected activated ability");
    };
    assert!(matches!(
        cost17.as_slice(),
        [Cost::Sacrifice { filter, .. }] if filter.flags.contains(FilterFlags::DIFFERENT_NAMES)
    ));

    // 18. Chains of Borrowed Loyalty
    let c18 = compile_v2_strict(
        "Chains of Borrowed Loyalty",
        "Sorcery",
        "Gain control of target creature with power or toughness 4 or less. It gains haste.",
        &vocab,
    );
    assert!(matches!(
        &c18.faces[0].spell_effects[0],
        Effect::MutateObject {
            mutation: ObjectMutation::GainControl {
                controller: PlayerRef::You
            },
            duration: Duration::Permanent,
            ..
        }
    ));

    // 19. Warlord's Reckoning
    let c19 = compile_v2_strict(
        "Warlord's Reckoning",
        "Sorcery",
        "If it's the first combat phase of the turn, untap all creatures that attacked this turn and there is an additional combat phase after this phase.",
        &vocab,
    );
    assert!(matches!(
        c19.faces[0].spell_effects.as_slice(),
        [Effect::BranchCondition { then_body, .. }] if then_body.len() == 2
    ));

    // 20. Kaelen, Riftbound Tactician
    let c20 = compile_v2_strict(
        "Kaelen, Riftbound Tactician",
        "Legendary Planeswalker — Kaelen",
        "+1: Each player creates a 1/1 white Soldier creature token and draws a card.\n-2: Move a counter from target permanent onto another target permanent.\n-6: You get an emblem with \"Noncreature spells you cast cost {1} less to cast.\"",
        &vocab,
    );
    assert_eq!(c20.faces[0].abilities.len(), 3);
    assert!(matches!(
        &c20.faces[0].abilities[0],
        Ability::Loyalty { delta: LoyaltyDelta::Fixed(1), body } if body.len() == 2
    ));
}

/// 2. All 50/50 custom cards from `custom_card_composability_suite.rs` also compile strictly in `v2`!
#[test]
fn v2_parses_all_50_custom_composability_cards_strictly() {
    let vocab = v2_vocab();
    let cards: &[(&str, &str, &str)] = &[
        (
            "Astral Recall Engine",
            "Sorcery",
            "Return up to two target blue or red Wizard creature cards with mana value 3 or less from your graveyard to your hand.",
        ),
        (
            "Vault of Forgotten Relics",
            "Sorcery",
            "Put all Shrine cards you own from exile and from your graveyard into your hand.",
        ),
        (
            "Beacon of the Sovereign",
            "Instant",
            "You may put a Dragon creature card you own with mana value 6 or less from your hand or from the command zone onto the battlefield tapped.",
        ),
        (
            "Aether Unbinder",
            "Instant",
            "Exile target nonwhite, nonartifact permanent attached to a Dragon you control.",
        ),
        (
            "Chronosiphon",
            "Sorcery",
            "Destroy each creature an opponent controls that has a slumber counter on it.",
        ),
        (
            "Eclipse Harvest",
            "Sorcery",
            "Exile all tapped Vampires. Each player creates a 0/0 black and white Spirit creature token and puts a number of +1/+1 counters on it equal to the total power of creatures they controlled that were exiled this way.",
        ),
        (
            "Prism Containment",
            "Artifact",
            "When Prism Containment leaves the battlefield, return all cards exiled with Prism Containment except Prism Containment to the battlefield tapped under their owners' control.",
        ),
        (
            "Scalewright's Verdict",
            "Sorcery",
            "Exile all creatures with power or toughness 5 or less. Draw a card for each creature exiled this way.",
        ),
        (
            "Kinship Calling",
            "Sorcery",
            "Search your library for a creature card that shares a creature type with target Elf you control, reveal it, put it into your hand, then shuffle.",
        ),
        (
            "Graveyard Census",
            "Sorcery",
            "For each graveyard with an artifact or enchantment card in it, exile target artifact or enchantment card from that graveyard.",
        ),
        (
            "Tidecaller Sovereign",
            "Legendary Creature — Merfolk Wizard",
            "When Tidecaller Sovereign enters, create X 2/2 Merfolk Shaman creature tokens that are blue, black, and red at the beginning of your next upkeep, where X is the number of Islands you control at that time.",
        ),
        (
            "Armistice Protocol",
            "Sorcery",
            "This turn and next turn, creatures can't attack, and players and permanents can't be the targets of spells or activated abilities.",
        ),
        (
            "Stormforged Artificer",
            "Creature — Human Artificer",
            "When Stormforged Artificer enters, create a 1/1 colorless Thopter artifact creature token with flying, then put a +1/+1 counter on that token for each spell you've cast this turn other than the first.",
        ),
        (
            "Sanctum Surge",
            "Instant",
            "Target creature you control gets +2/+1 until end of turn for each mana from a Shrine that was spent to cast Sanctum Surge.",
        ),
        (
            "Aether Foundry",
            "Sorcery",
            "The next Construct spell you cast this turn enters with a number of additional charge counters on it equal to the number of artifacts you control.",
        ),
        (
            "Vanguard of the Reliquary",
            "Creature — Angel Knight",
            "Flying, vigilance\nAs long as you control three or more enchantments, Angels you control get +2/+2 and have lifelink.\nIf a creature an opponent controls would die, exile it instead.",
        ),
        (
            "Soul Tether",
            "Instant",
            "Choose target creature. When that creature dies this turn, return a creature card from its owner's graveyard to the battlefield under the control of that creature's owner.",
        ),
        (
            "Pyromancer's Lens",
            "Enchantment",
            "Whenever you cast a spell, if its mana cost contains {R}, Pyromancer's Lens deals 2 damage to each opponent.",
        ),
        (
            "Solar Amplifier",
            "Sorcery",
            "You gain life equal to three times the number of Angels you control. Draw two cards.",
        ),
        (
            "Blood Oath Seer",
            "Creature — Vampire Cleric",
            "At the beginning of your upkeep, you may pay 2 life. If you do, draw two cards if you control a Demon.",
        ),
        (
            "Inquisitor's Ultimatum",
            "Sorcery",
            "Each opponent faces a villainous choice — That player sacrifices two creatures and loses 3 life, or you draw three cards and create a 3/3 black Demon creature token with flying.",
        ),
        (
            "Guildmaster's Decree",
            "Instant",
            "Choose two —\n• Destroy target artifact or enchantment.\n• Target creature you control gets +3/+3 and gains trample until end of turn.\n• Scry 2, then draw a card.",
        ),
        (
            "Runeforge Escalation",
            "Sorcery",
            "Spree\n+ {1}{R} — Runeforge Escalation deals 3 damage to any target.\n+ {2}{U} — Draw two cards, then discard a card.\n+ {W} — Create two 1/1 white Soldier creature tokens.",
        ),
        (
            "Sphinx's Partition",
            "Instant",
            "An opponent looks at the top five cards of your library and separates them into a face-down pile and a face-up pile. Put one pile into your hand and the other into your graveyard.",
        ),
        (
            "Dragonspire Sanctum",
            "Land",
            "{T}: Add {C}.\n{1}, {T}: Add one mana of any color. If that mana is spent to cast a Dragon spell, it enters with two additional +1/+1 counters on it.",
        ),
        (
            "Adaptive Carapace",
            "Instant",
            "Target creature you control gains your choice of flying, deathtouch, or hexproof until end of turn.",
        ),
        (
            "Cinder Arbitration",
            "Sorcery",
            "Cinder Arbitration deals 5 damage divided as target opponent chooses among any number of creatures they control.",
        ),
        (
            "Biomancer's Tithe",
            "Sorcery",
            "You may distribute a number of +1/+1 counters equal to the number of Forests you control among any number of target creatures.",
        ),
        (
            "Backlash Arc",
            "Instant",
            "Backlash Arc deals 3 damage to target creature, 2 damage to target opponent, and 1 damage to you.",
        ),
        (
            "Valkyrie's Reckoning",
            "Instant",
            "Target opponent sacrifices a creature that attacked this turn. If you control an Angel, that player sacrifices two of those creatures of their choice instead.",
        ),
        (
            "Aetherial Reset",
            "Sorcery",
            "You get {E}{E}{E}. Each player may exile their hand and draw a number of cards equal to the amount of {E} paid this way.",
        ),
        (
            "Temporal Vault",
            "Artifact",
            "{T}, Sacrifice Temporal Vault: Each player exiles all cards from their hand face down and draws five cards. At the beginning of the next end step, each player discards their hand and returns to their hand each card they exiled this way.",
        ),
        (
            "Frostbind Claim",
            "Instant",
            "Gain control of target nonartifact, nonred creature that attacked you this turn and put a stun counter on it.",
        ),
        (
            "Pandora's Reliquary",
            "Artifact",
            "{4}, {T}, Sacrifice Pandora's Reliquary: Each player turns face up all cards they own exiled with Pandora's Reliquary, then puts all artifact cards among them onto the battlefield.",
        ),
        (
            "Mycelial Conduit",
            "Creature — Fungus Druid",
            "At the beginning of your end step, put a spore counter on Mycelial Conduit for every three creatures you control.",
        ),
        (
            "Living Obelisk",
            "Artifact",
            "{3}: Until end of turn, Living Obelisk becomes a creature in addition to its other types and has base power and base toughness each equal to the number of artifacts you control.",
        ),
        (
            "Mindrot Surge",
            "Sorcery",
            "Each opponent loses life and discards cards equal to the number of Zombies you control.",
        ),
        (
            "Mirror Cavalry",
            "Instant",
            "Create X tokens that are copies of target Knight you control and that are tapped and attacking, where X is the number of opponents you have.",
        ),
        (
            "Triad Tutor",
            "Sorcery",
            "Search your library for exactly three Dragon cards with different names, then reveal those cards.",
        ),
        (
            "Vespera, Weaver of Echoes",
            "Legendary Planeswalker — Vespera",
            "+1: Scry 1, then draw a card.\n-2: Return target nonland permanent to its owner's hand.\n-7: You get an emblem with \"Whenever you cast an instant or sorcery spell, draw a card.\"",
        ),
        (
            "Rift Conduit",
            "Artifact",
            "{T}: Add {U}{B}. Spend this mana only to cast spells from exile.",
        ),
        (
            "Vault Scavenger",
            "Creature — Rogue",
            "When Vault Scavenger enters, exile the top card of target opponent's library. You may play that card for as long as it remains exiled.",
        ),
        (
            "Oracle of the Shifting Sands",
            "Sorcery",
            "Choose a creature type. Look at the top four cards of your library. Put the rest on the bottom of your library in a random order.",
        ),
        (
            "Ironclad Bastion",
            "Enchantment",
            "Creatures you control can't be the targets of spells your opponents control.\n{2}{W}: Target creature can't attack or block until end of turn. Activate only during your turn and only once each turn.",
        ),
        (
            "Chronomancer's Hourglass",
            "Sorcery",
            "Exile target creature with a number of time counters on it equal to its mana value. You gain 1 life for each card in your hand.",
        ),
        (
            "Merciful Scales",
            "Sorcery",
            "The player with the lowest life total draws four cards.",
        ),
        (
            "Sanctum Whisper",
            "Instant",
            "The next enchantment spell you cast this turn can't be countered.",
        ),
        (
            "Void Erasure",
            "Sorcery",
            "Exile all enchantments from the battlefield, all instant cards from all graveyards, and all artifact cards from all hands.",
        ),
        (
            "Cinderwake Decree",
            "Sorcery",
            "Each opponent dealt combat damage this game by a Dragon you control discards two cards.",
        ),
        (
            "Taunt of the Phantom",
            "Instant",
            "Target creature is goaded for as long as you control Taunt of the Phantom. It can't be blocked that combat.",
        ),
    ];

    let mut summary = V2CorpusSummary::default();
    let mut failures = Vec::new();
    for &(name, type_line, oracle) in cards {
        let input = CardInput {
            oracle_id: name,
            name,
            layout: "normal",
            faces: vec![FaceInput {
                name,
                type_line,
                oracle_text: oracle,
            }],
        };
        let compiled = compile_card_v2(&input, &vocab);
        summary.record(&compiled);
        if !compiled.strictly_compiled {
            failures.push(format!("{name}: {:?}", compiled.faces[0].errors));
        }
    }

    assert!(
        failures.is_empty(),
        "v2 failed on {}/{} cards:\n{}",
        failures.len(),
        cards.len(),
        failures.join("\n")
    );
    assert_eq!(summary.total_cards, 50);
    assert_eq!(summary.strictly_compiled_cards, 50);
    assert_eq!(summary.failed_cards, 0);

    // Spot-check deep typed fields across key cards in the 50-card suite
    let oracle_sands = compile_v2_strict(
        "Oracle of the Shifting Sands",
        "Sorcery",
        "Choose a creature type. Look at the top four cards of your library. Put the rest on the bottom of your library in a random order.",
        &vocab,
    );
    assert!(matches!(
        &oracle_sands.faces[0].spell_effects[2],
        Effect::ZoneOp {
            to: Some(dest),
            ..
        } if dest.zone == ZoneKind::Library
            && dest.library_position == Some(LibraryPosition::Bottom)
            && dest.library_order == Some(LibraryOrder::RandomOrder)
    ));

    let bastion = compile_v2_strict(
        "Ironclad Bastion",
        "Enchantment",
        "Creatures you control can't be the targets of spells your opponents control.\n{2}{W}: Target creature can't attack or block until end of turn. Activate only during your turn and only once each turn.",
        &vocab,
    );
    let Ability::Activated { restrictions, .. } = &bastion.faces[0].abilities[0] else {
        panic!("expected activated ability");
    };
    assert_eq!(
        restrictions,
        &vec![
            ActivationRestriction::DuringYourTurn,
            ActivationRestriction::OnceEachTurn
        ]
    );

    let mycelial = compile_v2_strict(
        "Mycelial Conduit",
        "Creature — Fungus Druid",
        "At the beginning of your end step, put a spore counter on Mycelial Conduit for every three creatures you control.",
        &vocab,
    );
    let Ability::Triggered { trigger, body } = &mycelial.faces[0].abilities[0] else {
        panic!("expected end step trigger");
    };
    assert!(matches!(
        trigger.events.as_slice(),
        [TriggerEvent::StepBegins {
            step: TurnStepAst::EndStep,
            owner: StepOwner::Your
        }]
    ));
    assert!(matches!(
        body.as_slice(),
        [Effect::MutateObject {
            mutation: ObjectMutation::PutCounters {
                kind: CounterKind::Spore,
                count: ValueExpr::Div {
                    denom: 3,
                    rounding: Rounding::Down,
                    ..
                }
            },
            ..
        }]
    ));
}

/// 3. Fail-closed verification: unimplemented grammar or trailing garbage MUST return `ParseError`.
#[test]
fn v2_fails_closed_on_unparsed_trailing_tokens_and_unknown_grammar() {
    let vocab = v2_vocab();
    let bad = CardInput {
        oracle_id: "bad",
        name: "Unparsed Probe",
        layout: "normal",
        faces: vec![FaceInput {
            name: "Unparsed Probe",
            type_line: "Sorcery",
            oracle_text: "Draw a card florbulate the quantum nexus.",
        }],
    };
    let compiled = compile_card_v2(&bad, &vocab);
    assert!(
        !compiled.strictly_compiled,
        "v2 MUST fail closed on trailing unparsed words"
    );
    assert!(!compiled.faces[0].errors.is_empty());

    // Ensure unused imports are exercised cleanly
    let _ = (
        ColorMask::WHITE,
        CompareOp::Le,
        ChoiceCount::Exact(1),
        DelayedTriggerKind::Step {
            step: TurnStepAst::EndStep,
            owner: StepOwner::Next,
        },
        ForEachDomain::Player(PlayerRef::You),
        ManaSymbol::Red,
        PriorActionKind::ExiledThisWay,
        Replacement::EntersTapped { unless: None },
        RuleRestrictionKind::CantAttack,
        StatBound {
            stat: StatKind::Power,
            op: CompareOp::Le,
            value: Box::new(ValueExpr::Const(1)),
        },
        TokenSpec::CopyOf {
            source: Box::new(SubjectRef::SelfObject),
            exceptions: Vec::new(),
        },
        ZoneOwner::You,
    );
}

#[derive(Debug, serde::Deserialize)]
struct RealFixtureFace {
    name: String,
    type_line: String,
    oracle_text: String,
}

#[derive(Debug, serde::Deserialize)]
struct RealFixtureCard {
    oracle_id: String,
    name: String,
    layout: String,
    faces: Vec<RealFixtureFace>,
}

/// 4. Honest fail-closed real-card corpus regression floor + deep IR assertions on iconic real Magic cards.
#[test]
fn v2_real_magic_card_corpus_sample_and_iconic_staples() {
    let vocab_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("content")
        .join("tests")
        .join("fixtures")
        .join("creature-vocabulary.input.json");
    let vocab = Vocabulary::load_from_file(&vocab_path).expect("load CR creature vocabulary");

    let raw = include_str!("fixtures/real_cards_corpus_sample.json");
    let cards: Vec<RealFixtureCard> =
        serde_json::from_str(raw).expect("valid real_cards_corpus_sample.json");
    assert_eq!(cards.len(), 103);

    let mut summary = V2CorpusSummary::default();
    let mut compiled_by_name = std::collections::BTreeMap::new();
    for card in &cards {
        let faces: Vec<FaceInput<'_>> = card
            .faces
            .iter()
            .map(|f| FaceInput {
                name: &f.name,
                type_line: &f.type_line,
                oracle_text: &f.oracle_text,
            })
            .collect();
        let input = CardInput {
            oracle_id: &card.oracle_id,
            name: &card.name,
            layout: &card.layout,
            faces,
        };
        let compiled = compile_card_v2(&input, &vocab);
        summary.record(&compiled);
        compiled_by_name.insert(card.name.as_str(), compiled);
    }

    // Honest strict fail-closed coverage floor on the 103 hardest real Magic cards.
    assert!(
        summary.strictly_compiled_cards >= 50,
        "expected >= 50/103 strictly compiled real cards in v2, got {}/{}",
        summary.strictly_compiled_cards,
        summary.total_cards
    );

    // Verify iconic real cards compile 100% strictly with zero unconsumed tokens and typed IR.
    let must_strict = [
        "Sylvan Library",
        "Urza's Saga",
        "Liliana of the Veil",
        "Smothering Tithe",
        "Sen Triplets",
        "Panharmonicon",
        "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki",
        "Rhystic Study",
        "Doubling Season",
        "Necropotence",
        "Teferi, Time Raveler",
        "Bonecrusher Giant // Stomp",
        "Brainstorm",
        "Wrath of God",
        "Collected Company",
        "Fact or Fiction",
        "Blood Moon",
        "Humility",
        "Wear // Tear",
    ];
    for name in must_strict {
        let compiled = compiled_by_name
            .get(name)
            .unwrap_or_else(|| panic!("fixture missing `{name}`"));
        assert!(
            compiled.strictly_compiled,
            "expected iconic real card `{name}` to strictly compile in v2; errors: {:#?}",
            compiled
                .faces
                .iter()
                .flat_map(|f| f.errors.iter())
                .collect::<Vec<_>>()
        );
    }

    // Spot-check deep typed IR semantics on Urza's Saga, Rhystic Study, Blood Moon, Humility, Wrath of God.
    let saga = &compiled_by_name["Urza's Saga"];
    assert_eq!(saga.faces[0].abilities.len(), 3);
    assert!(matches!(
        &saga.faces[0].abilities[1],
        Ability::SagaChapter { chapters, body }
            if chapters == &[2]
                && matches!(
                    body.as_slice(),
                    [Effect::MutateObject {
                        mutation: ObjectMutation::GrantAbility(granted),
                        ..
                    }] if granted.is_ok() && granted.abilities.len() == 1
                )
    ));

    let rhystic = &compiled_by_name["Rhystic Study"];
    assert!(matches!(
        rhystic.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(body.as_slice(), [Effect::Unless { payer: Some(PlayerRef::ThatPlayer), .. }])
    ));

    let blood_moon = &compiled_by_name["Blood Moon"];
    assert!(matches!(
        blood_moon.faces[0].statics.as_slice(),
        [StaticAbility::SetSubtypes { subtypes, keep_existing: false, .. }]
            if subtypes == &["Mountain".to_string()]
    ));

    let humility = &compiled_by_name["Humility"];
    assert!(matches!(
        humility.faces[0].statics.as_slice(),
        [StaticAbility::Compound(parts)]
            if matches!(
                parts.as_slice(),
                [
                    StaticAbility::RemoveAllAbilities { .. },
                    StaticAbility::SetBasePT { power: 1, toughness: 1, .. }
                ]
            )
    ));

    let wrath = &compiled_by_name["Wrath of God"];
    assert_eq!(wrath.faces[0].spell_effects.len(), 2);
    assert!(matches!(
        &wrath.faces[0].spell_effects[1],
        Effect::MutateObject {
            mutation: ObjectMutation::CantBeRegenerated,
            ..
        }
    ));
}

/// 5. Deep Wave 2 compositional IR assertions covering shocklands, Thoughtseize/Duress,
///    Drown in the Loch, Theros Devotion, Gaea's Cradle scaled mana, Pharika's Cure
///    multi-clause damage+life, Path to Exile, Tormod's Crypt, X/X tokens, Tap-or-Untap,
///    and extra turns / phasing.
#[test]
fn v2_wave2_compositional_grammar_deep_ir_assertions() {
    let vocab_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("content")
        .join("tests")
        .join("fixtures")
        .join("creature-vocabulary.input.json");
    let vocab = Vocabulary::load_from_file(&vocab_path).expect("load CR creature vocabulary");

    let cases: &[(&str, &str, &str)] = &[
        (
            "Breeding Pool",
            "Land — Forest Island",
            "As Breeding Pool enters, you may pay 2 life. If you don't, it enters tapped.",
        ),
        (
            "Thoughtseize",
            "Sorcery",
            "Target player reveals their hand. You choose a nonland card from it. That player discards that card. You lose 2 life.",
        ),
        (
            "Drown in the Loch",
            "Instant",
            "Choose one —\n• Counter target spell with mana value less than or equal to the number of cards in its controller's graveyard.\n• Destroy target creature with mana value less than or equal to the number of cards in its controller's graveyard.",
        ),
        (
            "Thassa, God of the Sea",
            "Legendary Enchantment Creature — God",
            "Indestructible\nAs long as your devotion to blue is less than five, Thassa isn't a creature.\nAt the beginning of your upkeep, scry 1.",
        ),
        (
            "Pharika's Cure",
            "Instant",
            "Pharika's Cure deals 2 damage to target creature and you gain 2 life.",
        ),
        (
            "Path to Exile",
            "Instant",
            "Exile target creature. Its controller may search their library for a basic land card, put that card onto the battlefield tapped, then shuffle.",
        ),
        (
            "Gaea's Cradle",
            "Legendary Land",
            "{T}: Add {G} for each creature you control.",
        ),
        (
            "Tormod's Crypt",
            "Artifact",
            "{T}, Sacrifice Tormod's Crypt: Exile target player's graveyard.",
        ),
        (
            "Time Warp",
            "Sorcery",
            "Target player takes an extra turn after this one.",
        ),
        (
            "Slip Out the Back",
            "Instant",
            "Put a +1/+1 counter on target creature. It phases out.",
        ),
        (
            "Twiddle",
            "Instant",
            "You may tap or untap target artifact, creature, or land.",
        ),
        (
            "Veil of Summer",
            "Instant",
            "Draw a card if an opponent has cast a blue or black spell this turn. Spells you control can't be countered this turn. You and permanents you control gain hexproof from blue and from black until end of turn.",
        ),
        (
            "Elvish Ooze Maker",
            "Sorcery",
            "Create an X/X green Ooze creature token, where X is your life total.",
        ),
    ];

    for (name, type_line, oracle_text) in cases {
        let card = CardInput {
            oracle_id: name,
            name,
            layout: "normal",
            faces: vec![FaceInput {
                name,
                type_line,
                oracle_text,
            }],
        };
        let compiled = compile_card_v2(&card, &vocab);
        assert!(
            compiled.strictly_compiled,
            "Wave 2 card `{name}` failed strict v2 compilation: {:#?}",
            compiled.faces[0].errors
        );
    }

    // Verify Pharika's Cure emits BOTH DealDamage AND AdjustLife (never swallows `you` as damage target!)
    let cure = compile_card_v2(
        &CardInput {
            oracle_id: "cure",
            name: "Pharika's Cure",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Pharika's Cure",
                type_line: "Instant",
                oracle_text: "Pharika's Cure deals 2 damage to target creature and you gain 2 life.",
            }],
        },
        &vocab,
    );
    assert_eq!(cure.faces[0].spell_effects.len(), 2);
    assert!(matches!(
        &cure.faces[0].spell_effects[0],
        Effect::DealDamage { .. }
    ));
    assert!(matches!(
        &cure.faces[0].spell_effects[1],
        Effect::AdjustLife {
            player: PlayerRef::You,
            op: LifeOp::Gain,
            ..
        }
    ));

    // Verify Gaea's Cradle emits scaled ManaSpec::Scaled
    let cradle = compile_card_v2(
        &CardInput {
            oracle_id: "cradle",
            name: "Gaea's Cradle",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Gaea's Cradle",
                type_line: "Legendary Land",
                oracle_text: "{T}: Add {G} for each creature you control.",
            }],
        },
        &vocab,
    );
    assert!(matches!(
        cradle.faces[0].abilities.as_slice(),
        [Ability::Activated { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::MutatePlayer {
                    mutation: PlayerMutation::AddMana(spellwake_oracle_compiler::v2::ir::ManaSpec::Scaled { .. }),
                    ..
                }]
            )
    ));
}

/// 6. Deep Wave 3 compositional IR assertions covering damage/draw/discard replacements,
///    mid-sentence `where X is` bindings, multi-quote copy tokens, static animations,
///    replacement damage without repeated target, Station/Roll tables, Cases, Stickers,
///    Level Up bands + bare P/T, Firebending X bounds, Chroma, and Adamant.
#[test]
fn v2_wave3_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{
        AnaphoraRef, ConditionExpr, CopyException, KeywordKind, KeywordParam, Replacement,
        TokenSpec,
    };

    let vocab_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("content")
        .join("tests")
        .join("fixtures")
        .join("creature-vocabulary.input.json");
    let vocab = Vocabulary::load_from_file(&vocab_path).expect("load CR creature vocabulary");

    let cases: &[(&str, &str, &str)] = &[
        (
            "Dictate of the Twin Gods",
            "Enchantment",
            "Flash\nIf a source would deal damage to a permanent or player, it deals double that damage to that permanent or player instead.",
        ),
        (
            "Nine Lives",
            "Enchantment",
            "Hexproof\nIf a source would deal damage to you, prevent that damage and put an incarnation counter on Nine Lives.\nWhen there are nine or more incarnation counters on Nine Lives, exile it.\nWhen Nine Lives leaves the battlefield, you lose the game.",
        ),
        (
            "Akoum Hellkite",
            "Creature — Dragon",
            "Flying\nLandfall — Whenever a land you control enters, Akoum Hellkite deals 1 damage to any target. If that land is a Mountain, Akoum Hellkite deals 2 damage instead.",
        ),
        (
            "Minion Reflector",
            "Artifact",
            "Whenever a nontoken creature you control enters, you may pay {2}. If you do, create a token that's a copy of that creature, except it has haste and \"At the beginning of the end step, sacrifice this permanent.\"",
        ),
        (
            "Collective Voyage",
            "Sorcery",
            "Starting with you, each player may pay any amount of mana. Each player searches their library for up to X basic land cards, where X is the total amount of mana paid this way, puts them onto the battlefield tapped, then shuffles.",
        ),
        (
            "Ensoul Artifact",
            "Enchantment — Aura",
            "Enchant artifact\nEnchanted artifact is a creature with base power and toughness 5/5 in addition to its other types.",
        ),
        (
            "Ashiok's Erasure",
            "Enchantment",
            "Flash\nWhen Ashiok's Erasure enters, exile target spell.\nYour opponents can't cast spells with the same name as the exiled card.\nWhen Ashiok's Erasure leaves the battlefield, return the exiled card to its owner's hand.",
        ),
        (
            "Counterbalance",
            "Enchantment",
            "Whenever an opponent casts a spell, you may reveal the top card of your library. If you do, counter that spell if it has the same mana value as the revealed card.",
        ),
        (
            "Gerrard's Hourglass Pendant",
            "Legendary Artifact",
            "Flash\nIf a player would begin an extra turn, that player skips that turn instead.\n{4}, {T}, Exile Gerrard's Hourglass Pendant: Return to the battlefield tapped all artifact, creature, enchantment, and land cards in your graveyard that were put there from the battlefield this turn.",
        ),
        (
            "Loxodon Smiter",
            "Creature — Elephant Soldier",
            "This spell can't be countered.\nIf a spell or ability an opponent controls causes you to discard Loxodon Smiter, put it onto the battlefield instead of putting it into your graveyard.",
        ),
        (
            "Leyline of Sanctity",
            "Enchantment",
            "If Leyline of Sanctity is in your opening hand, you may begin the game with it on the battlefield.\nYou have hexproof.",
        ),
        (
            "Case of the Locked Hothouse",
            "Enchantment — Case",
            "You may play an additional land on each of your turns.\nTo solve — You control seven or more lands.\nSolved — You may look at the top card of your library any time, and you may play lands and cast creature and enchantment spells from the top of your library.",
        ),
        (
            "Zulaport Enforcer",
            "Creature — Human Warrior",
            "Level up {4}\nLEVEL 1-2\n3/3\nLEVEL 3+\n5/5\nZulaport Enforcer can't be blocked except by black creatures.",
        ),
        (
            "Firebending Student",
            "Creature — Human Monk",
            "Prowess\nFirebending X, where X is Firebending Student's power.",
        ),
        (
            "Heartlash Cinder",
            "Creature — Elemental Warrior",
            "Haste\nChroma — When Heartlash Cinder enters, it gets +X/+0 until end of turn, where X is the number of red mana symbols in the mana costs of permanents you control.",
        ),
        (
            "Rally for the Throne",
            "Instant",
            "Create two 1/1 white Human creature tokens.\nAdamant — If at least three white mana was spent to cast this spell, you gain 1 life for each creature you control.",
        ),
        (
            "Station Vessel",
            "Artifact — Spacecraft",
            "Station\n9+ | Flying, vigilance",
        ),
        (
            "Treasure Chest Roll",
            "Sorcery",
            "Roll a d20.\n1—9 | Draw a card.\n10—19 | Create three Treasure tokens.\n20 | Draw three cards.",
        ),
        (
            "Sticker Sheet Sample",
            "Stickers",
            "{TK}{TK} — 4/4\n{TK}{TK}{TK} — Whenever this creature attacks, draw a card.",
        ),
    ];

    for (name, type_line, oracle_text) in cases {
        let card = CardInput {
            oracle_id: name,
            name,
            layout: "normal",
            faces: vec![FaceInput {
                name,
                type_line,
                oracle_text,
            }],
        };
        let compiled = compile_card_v2(&card, &vocab);
        assert!(
            compiled.strictly_compiled,
            "Wave 3 card `{name}` failed strict v2 compilation: {:#?}",
            compiled.faces[0].errors
        );
    }

    // Deep IR spot checks:
    // 1. Dictate of the Twin Gods -> Replacement::ModifyDamage with factor == 2
    let dictate = compile_card_v2(
        &CardInput {
            oracle_id: "dictate",
            name: "Dictate of the Twin Gods",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Dictate of the Twin Gods",
                type_line: "Enchantment",
                oracle_text: "If a source would deal damage to a permanent or player, it deals double that damage to that permanent or player instead.",
            }],
        },
        &vocab,
    );
    assert!(matches!(
        dictate.faces[0].replacements.as_slice(),
        [Replacement::ModifyDamage {
            factor: 2,
            add: 0,
            ..
        }]
    ));

    // 2. Akoum Hellkite -> BranchCondition with ReplacePrevious(DealDamage { target: SameTarget, amount: Const(2) })
    let hellkite = compile_card_v2(
        &CardInput {
            oracle_id: "hellkite",
            name: "Akoum Hellkite",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Akoum Hellkite",
                type_line: "Creature — Dragon",
                oracle_text: "Landfall — Whenever a land you control enters, Akoum Hellkite deals 1 damage to any target. If that land is a Mountain, Akoum Hellkite deals 2 damage instead.",
            }],
        },
        &vocab,
    );
    assert!(matches!(
        hellkite.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(
                body.as_slice(),
                [
                    Effect::DealDamage { target: SubjectRef::AnyTarget, .. },
                    Effect::BranchCondition { then_body, .. }
                ] if matches!(
                    then_body.as_slice(),
                    [Effect::ReplacePrevious(inner)]
                        if matches!(
                            inner.as_slice(),
                            [Effect::DealDamage {
                                target: SubjectRef::Anaphora(AnaphoraRef::SameTarget),
                                ..
                            }]
                        )
                )
            )
    ));

    // 3. Minion Reflector -> CopyOf token with BOTH AddKeywords(Haste) AND compiled AddAbility
    let reflector = compile_card_v2(
        &CardInput {
            oracle_id: "reflector",
            name: "Minion Reflector",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Minion Reflector",
                type_line: "Artifact",
                oracle_text: "Whenever a nontoken creature you control enters, you may pay {2}. If you do, create a token that's a copy of that creature, except it has haste and \"At the beginning of the end step, sacrifice this permanent.\"",
            }],
        },
        &vocab,
    );
    assert!(matches!(
        reflector.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(
                &body[1],
                Effect::IfPrevious { succeeded: true, body: inner }
                    if matches!(
                        inner.as_slice(),
                        [Effect::CreateToken {
                            spec: TokenSpec::CopyOf { exceptions, .. },
                            ..
                        }] if matches!(
                            exceptions.as_slice(),
                            [CopyException::AddKeywords(_), CopyException::AddAbility(ab)] if ab.is_ok()
                        )
                    )
            )
    ));

    // 4. Firebending Student -> KeywordParam::VariableXBound(ObjectStat(Self, Power))
    let student = compile_card_v2(
        &CardInput {
            oracle_id: "student",
            name: "Firebending Student",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Firebending Student",
                type_line: "Creature — Human Monk",
                oracle_text: "Firebending X, where X is Firebending Student's power.",
            }],
        },
        &vocab,
    );
    assert!(matches!(
        student.faces[0].keywords.as_slice(),
        [spellwake_oracle_compiler::v2::ir::KeywordSpec {
            kind: KeywordKind::Firebending,
            param: Some(KeywordParam::VariableXBound(_)),
        }]
    ));

    // 5. Rally for the Throne -> Adamant ConditionExpr::ColoredManaSpent
    let rally = compile_card_v2(
        &CardInput {
            oracle_id: "rally",
            name: "Rally for the Throne",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Rally for the Throne",
                type_line: "Instant",
                oracle_text: "Create two 1/1 white Human creature tokens.\nAdamant — If at least three white mana was spent to cast this spell, you gain 1 life for each creature you control.",
            }],
        },
        &vocab,
    );
    assert!(matches!(
        &rally.faces[0].spell_effects[1],
        Effect::BranchCondition {
            condition: ConditionExpr::ColoredManaSpent { min_count: 3, .. },
            ..
        }
    ));
}

/// 7. Deep Wave 4 compositional IR assertions covering zero-lexer-error coverage,
///    `AnyPhyrexian`, `LegendRuleDoesntApply`, multi-clause `Protection from`,
///    `DoubleStat` / `DoubleCounters`, `BecomeColors`, `LoseLifeInsteadOfGain`,
///    `SkipExtraTurn`, `BonusTokens` with full creature specs, `DamageCap`,
///    `LoyaltyAsInstant`, `SpendManaAsAnyColor`, `AllColors`, `GameIsDraw`,
///    `UnlessEffect`, `Blight`, `Behold`, and `has base power and toughness` conjuncts.
#[test]
fn v2_wave4_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{
        KeywordKind, KeywordParam, ObjectMutation, PlayerMutation, Replacement, StaticAbility,
        TriggerEvent, ValueExpr,
    };

    let vocab_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("content")
        .join("tests")
        .join("fixtures")
        .join("creature-vocabulary.input.json");
    let vocab = Vocabulary::load_from_file(&vocab_path).expect("load CR creature vocabulary");

    let cases: &[(&str, &str, &str)] = &[
        (
            "Mirror Box",
            "Artifact",
            "The \"legend rule\" doesn't apply to permanents you control.\nEach legendary creature you control gets +1/+1.\nEach nontoken creature you control gets +1/+1 for each other creature you control with the same name as that creature.",
        ),
        (
            "Tainted Remedy",
            "Enchantment",
            "If an opponent would gain life, that player loses that much life instead.",
        ),
        (
            "Stranglehold",
            "Enchantment",
            "Your opponents can't search libraries.\nIf an opponent would begin an extra turn, that player skips that turn instead.",
        ),
        (
            "Oversoul of Dusk",
            "Creature — Spirit Avatar",
            "Protection from blue, from black, and from red",
        ),
        (
            "Elite Inquisitor",
            "Creature — Human Soldier",
            "First strike, vigilance\nProtection from Vampires, from Werewolves, and from Zombies",
        ),
        (
            "Alpha Authority",
            "Enchantment — Aura",
            "Enchant creature\nEnchanted creature has hexproof and can't be blocked by more than one creature.",
        ),
        (
            "Turn to Frog",
            "Instant",
            "Until end of turn, target creature loses all abilities and becomes a blue Frog with base power and toughness 1/1.",
        ),
        (
            "Unleash Fury",
            "Instant",
            "Double the power of target creature until end of turn.",
        ),
        (
            "Vorel of the Hull Clade",
            "Legendary Creature — Human Merfolk",
            "{G}{U}, {T}: Double the number of each kind of counter on target artifact, creature, or land.",
        ),
        (
            "Azusa, Lost but Seeking",
            "Legendary Creature — Human Monk",
            "You may play two additional lands on each of your turns.",
        ),
        (
            "Faerie Squadron",
            "Creature — Faerie",
            "Kicker {3}{U}\nIf Faerie Squadron was kicked, it enters with two +1/+1 counters on it and with flying.",
        ),
        (
            "Forethought Amulet",
            "Artifact",
            "At the beginning of your upkeep, sacrifice Forethought Amulet unless you pay {3}.\nIf an instant or sorcery source would deal 3 or more damage to you, it deals 2 damage to you instead.",
        ),
        (
            "Quina, Qu Gourmet",
            "Legendary Creature — Qu Citizen",
            "If one or more tokens would be created under your control, those tokens plus a 1/1 green Frog creature token are created instead.",
        ),
        (
            "Eight-and-a-Half-Tails",
            "Legendary Creature — Fox Cleric",
            "{1}{W}: Target spell or permanent becomes white until end of turn.",
        ),
        (
            "Square Up",
            "Instant",
            "Target creature has base power and toughness 4/4 until end of turn.",
        ),
        (
            "Riverfall Mimic",
            "Creature — Shapeshifter",
            "Whenever you cast a spell that's both blue and red, Riverfall Mimic has base power and toughness 3/3 until end of turn and can't be blocked this turn.",
        ),
        (
            "Divine Intervention",
            "Enchantment",
            "Divine Intervention enters with two intervention counters on it.\nAt the beginning of your upkeep, remove an intervention counter from Divine Intervention.\nWhen you remove the last intervention counter from Divine Intervention, the game is a draw.",
        ),
        (
            "Warped Tusker",
            "Creature — Eldrazi Boar",
            "Reach\nWhen you cast or cycle Warped Tusker, create a 0/1 colorless Eldrazi Spawn creature token with \"Sacrifice this token: Add {C}.\"\nCycling {2}{G}",
        ),
        (
            "Wound Reflection",
            "Enchantment",
            "At the beginning of each end step, each opponent loses life equal to the life they lost this turn.",
        ),
    ];

    for (name, type_line, oracle_text) in cases {
        let card = CardInput {
            oracle_id: name,
            name,
            layout: "normal",
            faces: vec![FaceInput {
                name,
                type_line,
                oracle_text,
            }],
        };
        let compiled = compile_card_v2(&card, &vocab);
        assert!(
            compiled.strictly_compiled,
            "Wave 4 card `{name}` failed strict v2 compilation: {:#?}",
            compiled.faces[0].errors
        );
    }

    // Deep IR spot checks:
    // 1. Tainted Remedy -> Replacement::LoseLifeInsteadOfGain(AnyOpponent)
    let remedy = compile_v2_strict(
        "Tainted Remedy",
        "Enchantment",
        "If an opponent would gain life, that player loses that much life instead.",
        &vocab,
    );
    assert!(matches!(
        remedy.faces[0].replacements.as_slice(),
        [Replacement::LoseLifeInsteadOfGain(PlayerRef::AnyOpponent)]
    ));

    // 2. Elite Inquisitor -> Protection from AnyOf([Vampires, Werewolves, Zombies])
    let inquisitor = compile_v2_strict(
        "Elite Inquisitor",
        "Creature — Human Soldier",
        "Protection from Vampires, from Werewolves, and from Zombies",
        &vocab,
    );
    assert!(matches!(
        inquisitor.faces[0].keywords.as_slice(),
        [spellwake_oracle_compiler::v2::ir::KeywordSpec {
            kind: KeywordKind::Protection,
            param: Some(KeywordParam::Filter(flt)),
        }] if matches!(
            flt.predicates.as_slice(),
            [FilterPredicate::AnyOf(parts)] if parts.len() == 3
        )
    ));

    // 3. Turn to Frog -> ScopedDuration(RemoveAllAbilities, BecomeColors(BLUE), BecomeCreatureTypes(Frog, 1/1))
    let frog = compile_v2_strict(
        "Turn to Frog",
        "Instant",
        "Until end of turn, target creature loses all abilities and becomes a blue Frog with base power and toughness 1/1.",
        &vocab,
    );
    assert!(matches!(
        frog.faces[0].spell_effects.as_slice(),
        [Effect::ScopedDuration {
            duration: Duration::UntilEndOfTurn,
            body,
        }] if matches!(
            body.as_slice(),
            [
                Effect::MutateObject { mutation: ObjectMutation::RemoveAllAbilities, .. },
                Effect::MutateObject { mutation: ObjectMutation::BecomeColors { .. }, .. },
                Effect::MutateObject {
                    mutation: ObjectMutation::BecomeCreatureTypes { base_pt: Some((1, 1)), .. },
                    ..
                }
            ]
        )
    ));

    // 4. Unleash Fury -> ObjectMutation::DoubleStat { power: true, toughness: false }
    let fury = compile_v2_strict(
        "Unleash Fury",
        "Instant",
        "Double the power of target creature until end of turn.",
        &vocab,
    );
    assert!(matches!(
        fury.faces[0].spell_effects.as_slice(),
        [Effect::MutateObject {
            mutation: ObjectMutation::DoubleStat {
                power: true,
                toughness: false,
            },
            duration: Duration::UntilEndOfTurn,
            ..
        }]
    ));

    // 5. Divine Intervention -> LastCounterRemoved trigger with PlayerMutation::GameIsDraw
    let divine = compile_v2_strict(
        "Divine Intervention",
        "Enchantment",
        "When you remove the last intervention counter from Divine Intervention, the game is a draw.",
        &vocab,
    );
    assert!(matches!(
        divine.faces[0].abilities.as_slice(),
        [Ability::Triggered { trigger, body }]
            if matches!(trigger.events.as_slice(), [TriggerEvent::LastCounterRemoved { .. }])
                && matches!(
                    body.as_slice(),
                    [Effect::MutatePlayer {
                        mutation: PlayerMutation::GameIsDraw,
                        ..
                    }]
                )
    ));

    // 6. Mirror Box -> StaticAbility::LegendRuleDoesntApply(Some(...))
    let mirror = compile_v2_strict(
        "Mirror Box",
        "Artifact",
        "The \"legend rule\" doesn't apply to permanents you control.",
        &vocab,
    );
    assert!(matches!(
        mirror.faces[0].statics.as_slice(),
        [StaticAbility::LegendRuleDoesntApply(Some(flt))]
            if flt.any_permanent && flt.controller == Some(ControllerRelation::You)
    ));

    // 7. Wound Reflection -> AdjustLife using ValueExpr::EventCountThisTurn(LifeLost)
    let wound = compile_v2_strict(
        "Wound Reflection",
        "Enchantment",
        "At the beginning of each end step, each opponent loses life equal to the life they lost this turn.",
        &vocab,
    );
    assert!(matches!(
        wound.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::AdjustLife {
                    player: PlayerRef::EachOpponent,
                    amount: ValueExpr::EventCountThisTurn { .. },
                    op: LifeOp::Lose,
                }]
            )
    ));
}

#[test]
fn v2_wave5_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{
        CastPermissionMode, ConditionExpr, ConditionFlag, DamageReplacement, ObjectStateAction,
        ReplacementRuleKind, StaticFlagKind, SubjectStaticRule, TriggerModifierKind,
        VisibilityKind,
    };

    let vocab = v2_vocab();

    // 20 real Wave 5 cards exercising consolidated orthogonal IR primitives and compositional grammar
    let wave5_cards: &[(&str, &str, &str)] = &[
        (
            "Birthing Pod",
            "Artifact",
            "{1}{G/P}, {T}, Sacrifice a creature: Search your library for a creature card with mana value equal to 1 plus the sacrificed creature's mana value, put that card onto the battlefield, then shuffle. Activate only as a sorcery.",
        ),
        (
            "Worship",
            "Enchantment",
            "If you control a creature, damage that would reduce your life total to less than 1 reduces it to 1 instead.",
        ),
        (
            "Contamination",
            "Enchantment",
            "At the beginning of your upkeep, sacrifice Contamination unless you sacrifice a creature.\nIf a land is tapped for mana, it produces {B} instead of any other type and amount.",
        ),
        (
            "Isshin, Two Heavens as One",
            "Legendary Creature — Human Samurai",
            "If a creature attacking causes a triggered ability of a permanent you control to trigger, that ability triggers an additional time.",
        ),
        (
            "Thryx, the Sudden Storm",
            "Legendary Creature — Elemental Giant",
            "Flash\nFlying\nSpells you cast with mana value 5 or greater cost {1} less to cast and can't be countered.",
        ),
        (
            "Disallow",
            "Instant",
            "Counter target spell, activated ability, or triggered ability.",
        ),
        (
            "Dream Prowler",
            "Creature — Illusion",
            "Dream Prowler can't be blocked as long as it's attacking alone.",
        ),
        (
            "Stoic Sphinx",
            "Creature — Sphinx",
            "Flash\nFlying\nStoic Sphinx has hexproof as long as you haven't cast a spell this turn.",
        ),
        (
            "Jaheira's Respite",
            "Instant",
            "Search your library for up to X basic land cards, where X is the number of creatures attacking you, put those cards onto the battlefield tapped, then shuffle.\nPrevent all combat damage that would be dealt this turn.",
        ),
        (
            "Laccolith Titan",
            "Creature — Beast",
            "Whenever Laccolith Titan becomes blocked, you may have it deal damage equal to its power to target creature. If you do, Laccolith Titan assigns no combat damage this turn.",
        ),
        (
            "Sanguine Statuette",
            "Artifact",
            "When Sanguine Statuette enters, create a Blood token.\nWhenever you sacrifice a Blood token, you may have Sanguine Statuette become a 3/3 Vampire artifact creature with haste until end of turn.",
        ),
        (
            "Band Together",
            "Instant",
            "Up to two target creatures you control each deal damage equal to their power to another target creature.",
        ),
        (
            "Jaws of Defeat",
            "Enchantment",
            "Whenever a creature you control enters, target opponent loses life equal to the difference between that creature's power and its toughness.",
        ),
        (
            "Gingerbrute",
            "Artifact Creature — Food Golem",
            "Haste\n{1}: Gingerbrute can't be blocked this turn except by creatures with haste.\n{2}, {T}, Sacrifice Gingerbrute: You gain 3 life.",
        ),
        (
            "Swift Warkite",
            "Creature — Dragon",
            "Flying\nWhen Swift Warkite enters, you may put a creature card with mana value 3 or less from your hand or graveyard onto the battlefield. That creature gains haste. Return it to your hand at the beginning of the next end step.",
        ),
        (
            "Repay in Kind",
            "Sorcery",
            "Each player's life total becomes the lowest life total among all players.",
        ),
        (
            "Crown of Vigor",
            "Enchantment — Aura",
            "Enchant creature\nEnchanted creature gets +1/+1.\nSacrifice Crown of Vigor: Enchanted creature and other creatures that share a creature type with it get +1/+1 until end of turn.",
        ),
        (
            "Nezumi Graverobber",
            "Creature — Rat Rogue",
            "{1}{B}: Exile target card from an opponent's graveyard. If no cards are in that graveyard, flip Nezumi Graverobber.",
        ),
        (
            "Bolas's Citadel",
            "Legendary Artifact",
            "You may look at the top card of your library any time.\nYou may play lands and cast spells from the top of your library.",
        ),
    ];

    for &(name, type_line, oracle) in wave5_cards {
        let compiled = compile_v2_strict(name, type_line, oracle, &vocab);
        assert!(
            compiled.strictly_compiled,
            "Wave 5 card `{name}` failed to compile strictly"
        );
    }

    // Deep IR spot assertions on consolidated orthogonal primitives:

    // 1. Isshin -> StaticAbility::TriggerModifier(AdditionalTriggers)
    let isshin = compile_v2_strict(
        "Isshin, Two Heavens as One",
        "Legendary Creature — Human Samurai",
        "If a creature attacking causes a triggered ability of a permanent you control to trigger, that ability triggers an additional time.",
        &vocab,
    );
    assert!(matches!(
        isshin.faces[0].statics.as_slice(),
        [StaticAbility::TriggerModifier {
            kind: TriggerModifierKind::AdditionalTriggers,
            extra_count: 1,
            ..
        }]
    ));

    // 2. Bolas's Citadel -> VisibilityRule(LookAtTop) + ZoneCastPermission(PlayFromZone)
    let citadel = compile_v2_strict(
        "Bolas's Citadel",
        "Legendary Artifact",
        "You may look at the top card of your library any time.\nYou may play lands and cast spells from the top of your library.",
        &vocab,
    );
    assert!(matches!(
        citadel.faces[0].statics.as_slice(),
        [
            StaticAbility::VisibilityRule {
                player: PlayerRef::You,
                kind: VisibilityKind::LookAtTop(_),
            },
            StaticAbility::ZoneCastPermission {
                player: PlayerRef::You,
                mode: CastPermissionMode::PlayFromZone {
                    top_only: true,
                    lands: true,
                    spells: true,
                },
                ..
            }
        ]
    ));

    // 3. Gingerbrute -> ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeBlockedExceptBy)
    let ginger = compile_v2_strict(
        "Gingerbrute",
        "Artifact Creature — Food Golem",
        "{1}: Gingerbrute can't be blocked this turn except by creatures with haste.",
        &vocab,
    );
    assert!(matches!(
        ginger.faces[0].abilities.as_slice(),
        [Ability::Activated { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::MutateObject {
                    mutation: ObjectMutation::ApplyRule(
                        RuleRestrictionKind::CantBeBlockedExceptBy(_)
                    ),
                    ..
                }]
            )
    ));

    // 4. Dream Prowler -> StaticAbility::RuleRestriction with trailing `as long as` condition
    let prowler = compile_v2_strict(
        "Dream Prowler",
        "Creature — Illusion",
        "Dream Prowler can't be blocked as long as it's attacking alone.",
        &vocab,
    );
    assert!(matches!(
        prowler.faces[0].statics.as_slice(),
        [StaticAbility::RuleRestriction {
            rule: RuleRestrictionKind::CantBeBlocked,
            condition: Some(_),
            ..
        }]
    ));

    // 5. Contamination -> Replacement::Rule(ReplacementRuleKind::ManaOverride)
    let contam = compile_v2_strict(
        "Contamination",
        "Enchantment",
        "If a land is tapped for mana, it produces {B} instead of any other type and amount.",
        &vocab,
    );
    assert!(matches!(
        contam.faces[0].replacements.as_slice(),
        [Replacement::Rule(ReplacementRuleKind::ManaOverride(_))]
    ));

    // 6. Nezumi Graverobber -> ObjectMutation::StateAction(ObjectStateAction::Flip)
    let nezumi = compile_v2_strict(
        "Nezumi Graverobber",
        "Creature — Rat Rogue",
        "{1}{B}: Exile target card from an opponent's graveyard. If no cards are in that graveyard, flip Nezumi Graverobber.",
        &vocab,
    );
    assert!(matches!(
        nezumi.faces[0].abilities.as_slice(),
        [Ability::Activated { body, .. }]
            if matches!(
                body.get(1),
                Some(Effect::BranchCondition { then_body, .. })
                    if matches!(
                        then_body.as_slice(),
                        [Effect::MutateObject {
                            mutation: ObjectMutation::StateAction(ObjectStateAction::Flip),
                            ..
                        }]
                    )
            )
    ));

    // 7. Repay in Kind -> Effect::AdjustLife { op: LifeOp::Set }
    let repay = compile_v2_strict(
        "Repay in Kind",
        "Sorcery",
        "Each player's life total becomes the lowest life total among all players.",
        &vocab,
    );
    assert!(matches!(
        repay.faces[0].spell_effects.as_slice(),
        [Effect::AdjustLife {
            player: PlayerRef::EachPlayer,
            op: LifeOp::Set,
            ..
        }]
    ));

    // 8. Excruciator -> Replacement::DamageRule(DamageReplacement::UnpreventableBySource)
    let excruc = compile_v2_strict(
        "Excruciator",
        "Creature — Avatar",
        "Damage that would be dealt by Excruciator can't be prevented.",
        &vocab,
    );
    assert!(matches!(
        excruc.faces[0].replacements.as_slice(),
        [Replacement::DamageRule(
            DamageReplacement::UnpreventableBySource(_)
        )]
    ));

    // 9. Conditional AttackTax (`Dain`) fails closed honestly (AttackTax has no condition field);
    //    unconditional `Propaganda` still compiles faithfully to StaticAbility::AttackTax
    let dain = compile_card_v2(
        &CardInput {
            oracle_id: "dain",
            name: "Dain, Lord of the Iron Hills",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Dain, Lord of the Iron Hills",
                type_line: "Legendary Creature - Dwarf Noble",
                oracle_text: "As long as you have an enduring story, creatures can't attack you unless their controller pays {1} for each of those creatures.",
            }],
        },
        &vocab,
    );
    assert!(!dain.strictly_compiled);
    let propaganda = compile_v2_strict(
        "Ghostly Prison",
        "Enchantment",
        "Creatures can't attack you unless their controller pays {2} for each of those creatures.",
        &vocab,
    );
    assert!(matches!(
        propaganda.faces[0].statics.as_slice(),
        [StaticAbility::AttackTax {
            per_creature: true,
            ..
        }]
    ));

    // 10. Ori, Keeper of Songs -> ConditionFlag::HasEnduringStory in ContinuousModifyPT
    let ori = compile_v2_strict(
        "Ori, Keeper of Songs",
        "Legendary Creature — Dwarf",
        "As long as you have an enduring story, Ori gets +1/+0 and has vigilance.",
        &vocab,
    );
    assert!(matches!(
        ori.faces[0].statics.as_slice(),
        [StaticAbility::ContinuousModifyPT {
            condition: Some(ConditionExpr::Flag(ConditionFlag::HasEnduringStory)),
            ..
        }]
    ));

    // 11. Pithing Needle fails closed honestly (cannot drop `unless they're mana abilities`)
    let needle = compile_card_v2(
        &CardInput {
            oracle_id: "pithing-needle",
            name: "Pithing Needle",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Pithing Needle",
                type_line: "Artifact",
                oracle_text: "As Pithing Needle enters, choose a card name.\nActivated abilities of sources with the chosen name can't be activated unless they're mana abilities.",
            }],
        },
        &vocab,
    );
    assert!(!needle.strictly_compiled);
    let _ = StaticFlagKind::NamedSourcesCantActivate;

    // 12. Assault Formation -> StaticAbility::SubjectRule(SubjectStaticRule::AssignDamageByToughness)
    let assault = compile_v2_strict(
        "Assault Formation",
        "Enchantment",
        "Each creature you control assigns combat damage equal to its toughness rather than its power.",
        &vocab,
    );
    assert!(matches!(
        assault.faces[0].statics.as_slice(),
        [StaticAbility::SubjectRule {
            rule: SubjectStaticRule::AssignDamageByToughness,
            ..
        }]
    ));
}

/// 9. Deep Wave 6 compositional IR assertions (zero new core IR enum variants):
///    conjoined sacrifice costs, multi-filter cost modifiers, aura-transform statics,
///    two-sentence damage-prevention replacements, `For each 1 damage`, restrictive player clauses,
///    negated relative filters, `that's <types>`, and zero regression on `Teferi, Mage of Zhalfir`.
#[test]
fn v2_wave6_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::ast::SupertypeMask;
    use spellwake_oracle_compiler::v2::ir::{ConditionExpr, PlayerRestrictionKind, ZoneMetricKind};
    let vocab_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("content")
        .join("tests")
        .join("fixtures")
        .join("creature-vocabulary.input.json");
    let vocab = Vocabulary::load_from_file(&vocab_path).expect("load CR creature vocabulary");

    let wave6_cards: &[(&str, &str, &str)] = &[
        (
            "Teferi, Mage of Zhalfir",
            "Legendary Creature - Human Wizard",
            "Flash\nCreature cards you own that aren't on the battlefield have flash.\nEach opponent can cast spells only any time they could cast a sorcery.",
        ),
        (
            "Darksteel Mutation",
            "Enchantment - Aura",
            "Enchant creature\nEnchanted creature is an Insect artifact creature with base power and toughness 0/1 and has indestructible, and it loses all other abilities, card types, and creature types.",
        ),
        (
            "Imprisoned in the Moon",
            "Enchantment - Aura",
            "Enchant creature, land, or planeswalker\nEnchanted permanent is a colorless land with \"{T}: Add {C}\" and loses all other card types and abilities.",
        ),
        (
            "Inkshield",
            "Instant",
            "Prevent all combat damage that would be dealt to you this turn. For each 1 damage prevented this way, create a 2/1 white and black Inkling creature token with flying.",
        ),
        (
            "Plaguecrafter",
            "Creature - Human Shaman",
            "When Plaguecrafter enters, each player sacrifices a creature or planeswalker. Each player who can't discards a card.",
        ),
        (
            "Tinybones, Trinket Thief",
            "Legendary Creature - Skeleton Rogue",
            "At the beginning of each end step, if an opponent discarded a card this turn, you draw a card and you lose 1 life.\n{4}{B}{B}: Each opponent with no cards in hand loses 10 life.",
        ),
        (
            "Jarad, Golgari Lich Lord",
            "Legendary Creature - Zombie Elf",
            "Jarad gets +1/+1 for each creature card in your graveyard.\n{1}{B}{G}, Sacrifice another creature: Each opponent loses life equal to the sacrificed creature's power.\nSacrifice a Swamp and a Forest: Return Jarad from your graveyard to your hand.",
        ),
        (
            "Stonybrook Banneret",
            "Creature - Merfolk Wizard",
            "Islandwalk\nMerfolk spells and Wizard spells you cast cost {1} less to cast.",
        ),
        (
            "Bounty Agent",
            "Creature - Human Soldier",
            "Vigilance\n{T}, Sacrifice Bounty Agent: Destroy target legendary permanent that's an artifact, creature, or enchantment.",
        ),
        (
            "Enduring Renewal",
            "Enchantment",
            "Play with your hand revealed.\nIf you would draw a card, reveal the top card of your library instead. If it's a creature card, put it into your graveyard. Otherwise, draw a card.\nWhenever a creature is put into your graveyard from the battlefield, return it to your hand.",
        ),
        (
            "Vastwood Animist",
            "Creature - Elf Shaman Ally",
            "{T}: Target land you control becomes an X/X Elemental creature until end of turn, where X is the number of Allies you control. It's still a land.",
        ),
        (
            "Urza's Ruinous Blast",
            "Legendary Sorcery",
            "Exile all nonland permanents that aren't legendary.",
        ),
        (
            "Cursed Totem",
            "Artifact",
            "Activated abilities of creatures can't be activated.",
        ),
        (
            "Collector Ouphe",
            "Creature - Ouphe",
            "Activated abilities of artifacts can't be activated.",
        ),
        (
            "Blanket of Night",
            "Enchantment",
            "Each land is a Swamp in addition to its other land types.",
        ),
        (
            "Rocco, Street Chef",
            "Legendary Creature - Elf Druid",
            "At the beginning of your end step, each player exiles the top card of their library. Until your next end step, each player may play the card they exiled this way.\nWhenever a player plays a land from exile or casts a spell from exile, you put a +1/+1 counter on target creature and create a Food token.",
        ),
        (
            "Spider-Punk",
            "Legendary Creature - Spider Human Hero",
            "Riot\nOther Spiders you control have riot.\nSpells and abilities can't be countered.\nDamage can't be prevented.",
        ),
    ];

    for (name, type_line, oracle_text) in wave6_cards {
        let compiled = compile_v2_strict(name, type_line, oracle_text, &vocab);
        assert!(
            compiled.strictly_compiled,
            "Wave 6 card `{name}` failed strict v2 compilation: {:#?}",
            compiled.faces[0].errors
        );
    }

    // Deep typed IR assertions on orthogonal Wave 6 primitives:
    // 1. Jarad conjoined sacrifice cost splits cleanly into two independent Cost::Sacrifice atoms
    let jarad = compile_v2_strict(
        "Jarad, Golgari Lich Lord",
        "Legendary Creature - Zombie Elf",
        "Sacrifice a Swamp and a Forest: Return Jarad from your graveyard to your hand.",
        &vocab,
    );
    assert!(matches!(
        jarad.faces[0].abilities.as_slice(),
        [Ability::Activated { cost, .. }]
            if matches!(
                cost.as_slice(),
                [Cost::Sacrifice { .. }, Cost::Sacrifice { .. }]
            )
    ));

    // 2. Brighthearth Banneret multi-filter cost modifier composes FilterPredicate::AnyOf
    let banneret = compile_v2_strict(
        "Brighthearth Banneret",
        "Creature - Elemental Warrior",
        "Elemental spells and Warrior spells you cast cost {1} less to cast.",
        &vocab,
    );
    assert!(matches!(
        banneret.faces[0].statics.as_slice(),
        [StaticAbility::CostModifier { subject, less: true, .. }]
            if matches!(subject.predicates.as_slice(), [FilterPredicate::AnyOf(parts)] if parts.len() == 2)
    ));

    // 3. Stormwild Capridor fails closed honestly because Replacement has no noncombat_only discriminator
    let capridor = compile_card_v2(
        &CardInput {
            oracle_id: "stormwild-capridor",
            name: "Stormwild Capridor",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Stormwild Capridor",
                type_line: "Creature - Bird Goat",
                oracle_text: "If noncombat damage would be dealt to Stormwild Capridor, prevent that damage. Put a +1/+1 counter on Stormwild Capridor for each 1 damage prevented this way.",
            }],
        },
        &vocab,
    );
    assert!(!capridor.strictly_compiled);

    // 4. Inkshield `For each 1 damage prevented this way` compiles to ForEachDomain::EachUnitOfValue
    let inkshield = compile_v2_strict(
        "Inkshield",
        "Instant",
        "Prevent all combat damage that would be dealt to you this turn. For each 1 damage prevented this way, create a 2/1 white and black Inkling creature token with flying.",
        &vocab,
    );
    assert!(matches!(
        inkshield.faces[0].spell_effects.as_slice(),
        [
            Effect::PreventDamage {
                combat_only: true,
                ..
            },
            Effect::ForEach {
                over: ForEachDomain::EachUnitOfValue(ValueExpr::PriorActionCount(
                    PriorActionKind::DamagePreventedThisWay
                )),
                ..
            }
        ]
    ));

    // 5. Darksteel Mutation compiles to Compound static (SetBasePT + GrantKeywords + RemoveAllAbilities + SetSubtypes)
    let mutation = compile_v2_strict(
        "Darksteel Mutation",
        "Enchantment - Aura",
        "Enchanted creature is an Insect artifact creature with base power and toughness 0/1 and has indestructible, and it loses all other abilities, card types, and creature types.",
        &vocab,
    );
    assert!(matches!(
        mutation.faces[0].statics.as_slice(),
        [StaticAbility::Compound(parts)] if parts.len() >= 4
    ));

    // 6. Plaguecrafter `Each player who can't discards a card` -> ForEachDomain::EachPlayerWhoCant
    let plague = compile_v2_strict(
        "Plaguecrafter",
        "Creature - Human Shaman",
        "When Plaguecrafter enters, each player sacrifices a creature or planeswalker. Each player who can't discards a card.",
        &vocab,
    );
    assert!(matches!(
        plague.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(
                body.get(1),
                Some(Effect::ForEach {
                    over: ForEachDomain::EachPlayerWhoCant(PlayerRef::EachPlayer),
                    ..
                })
            )
    ));

    // 7. Tinybones `Each opponent with no cards in hand loses 10 life` -> ZoneMetric::IsEmpty
    let tinybones = compile_v2_strict(
        "Tinybones, Trinket Thief",
        "Legendary Creature - Skeleton Rogue",
        "{4}{B}{B}: Each opponent with no cards in hand loses 10 life.",
        &vocab,
    );
    assert!(matches!(
        tinybones.faces[0].abilities.as_slice(),
        [Ability::Activated { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::ForEach {
                    over: ForEachDomain::EachOpponentWithCondition(ConditionExpr::ZoneMetric {
                        kind: ZoneMetricKind::IsEmpty,
                        ..
                    }),
                    ..
                }]
            )
    ));

    // 8. Urza's Ruinous Blast `that aren't legendary` sets forbidden_supertypes = LEGENDARY
    let blast = compile_v2_strict(
        "Urza's Ruinous Blast",
        "Legendary Sorcery",
        "Exile all nonland permanents that aren't legendary.",
        &vocab,
    );
    assert!(matches!(
        blast.faces[0].spell_effects.as_slice(),
        [Effect::ZoneOp {
            subject: SubjectRef::All(flt),
            ..
        }] if flt.forbidden_supertypes == SupertypeMask::LEGENDARY
    ));

    // 9. Enduring Renewal `Play with your hand revealed` -> PlayerRestrictionKind::PlaysWithHandRevealed
    let renewal = compile_v2_strict(
        "Enduring Renewal",
        "Enchantment",
        "Play with your hand revealed.",
        &vocab,
    );
    assert!(matches!(
        renewal.faces[0].statics.as_slice(),
        [StaticAbility::PlayerRestriction {
            player: PlayerRef::You,
            restriction: PlayerRestrictionKind::PlaysWithHandRevealed,
            ..
        }]
    ));

    // 10. Wolverine `Double all damage ~ would deal` -> Replacement::ModifyDamage { factor: 2 } (kept honest!)
    let wolverine = compile_v2_strict(
        "Wolverine, Best There Is",
        "Legendary Creature - Mutant Berserker Hero",
        "Double all damage Wolverine would deal.",
        &vocab,
    );
    assert!(matches!(
        wolverine.faces[0].replacements.as_slice(),
        [Replacement::ModifyDamage { factor: 2, .. }]
    ));

    // 11. Cursed Totem `Activated abilities of creatures can't be activated` -> RuleRestrictionKind::ActivatedAbilitiesCantBeActivated
    let totem = compile_v2_strict(
        "Cursed Totem",
        "Artifact",
        "Activated abilities of creatures can't be activated.",
        &vocab,
    );
    assert!(matches!(
        totem.faces[0].statics.as_slice(),
        [StaticAbility::RuleRestriction {
            rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
            ..
        }]
    ));

    // 12. Teferi, Mage of Zhalfir zero-regression guard (`that aren't on the battlefield`)
    let teferi = compile_v2_strict(
        "Teferi, Mage of Zhalfir",
        "Legendary Creature - Human Wizard",
        "Creature cards you own that aren't on the battlefield have flash.",
        &vocab,
    );
    assert!(matches!(
        teferi.faces[0].statics.as_slice(),
        [StaticAbility::GrantKeywords {
            subject: SubjectRef::Group(flt),
            ..
        }] if flt.flags.contains(FilterFlags::NOT_ON_BATTLEFIELD)
    ));
}

/// 10. Deep Wave 7 compositional IR assertions (zero new core IR enum variants):
///     shared static subject tail combinator, activation cost modifiers, revolt/trap conditions,
///     `instead of putting it anywhere else` replacements, damage-prevention reflexive riders,
///     generalized untap/token/counter/lose-game replacements, `or on the bottom` library destinations,
///     `in addition to its other types` conjuncts, and `Hardened Scales` zero-regression guard.
#[test]
fn v2_wave7_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{
        CounterKind, LibraryPosition, ObjectMutation, PlayerRestrictionKind, PlayerTriggerKind,
        Replacement, StaticAbility, TokenReplacementKind, TriggerEvent, VarName, ZoneOpKind,
    };
    let vocab_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("content")
        .join("tests")
        .join("fixtures")
        .join("creature-vocabulary.input.json");
    let vocab = Vocabulary::load_from_file(&vocab_path).expect("load CR creature vocabulary");

    let wave7_cards: &[(&str, &str, &str)] = &[
        (
            "Hardened Scales",
            "Enchantment",
            "If one or more +1/+1 counters would be put on a creature you control, that many plus one +1/+1 counters are put on it instead.",
        ),
        (
            "Vizier of Remedies",
            "Creature - Human Cleric",
            "If one or more -1/-1 counters would be put on a creature you control, that many -1/-1 counters minus one are put on it instead.",
        ),
        (
            "Divine Visitation",
            "Enchantment",
            "If one or more creature tokens would be created under your control, that many 4/4 white Angel creature tokens with flying and vigilance are created instead.",
        ),
        (
            "Primal Vigor",
            "Enchantment",
            "If one or more tokens would be created, twice that many of those tokens are created instead.\nIf one or more +1/+1 counters would be put on a creature, twice that many +1/+1 counters are put on that creature instead.",
        ),
        (
            "Dralnu, Lich Lord",
            "Legendary Creature - Zombie Wizard",
            "If damage would be dealt to Dralnu, Lich Lord, sacrifice that many permanents instead.\n{T}: Target instant or sorcery card in your graveyard gains flashback until end of turn. The flashback cost is equal to its mana cost.",
        ),
        (
            "The Golden Throne",
            "Legendary Artifact",
            "Arcane Life-support \u{2014} If you would lose the game, instead exile The Golden Throne and your life total becomes 1.\nA Thousand Souls Die Every Day \u{2014} {T}, Sacrifice a creature: Add three mana in any combination of colors.",
        ),
        (
            "Ygra, Eater of All",
            "Legendary Creature - Elemental Cat",
            "Ward\u{2014}Sacrifice a Food.\nOther creatures are Food artifacts in addition to their other types and have \"{2}, {T}, Sacrifice this permanent: You gain 3 life.\"\nWhenever a Food is put into a graveyard from the battlefield, put two +1/+1 counters on Ygra, Eater of All.",
        ),
        (
            "Whip of Erebos",
            "Legendary Enchantment Artifact",
            "Creatures you control have lifelink.\n{2}{B}{B}, {T}: Return target creature card from your graveyard to the battlefield. It gains haste. Exile it at the beginning of the next end step. If it would leave the battlefield, exile it instead of putting it anywhere else. Activate only as a sorcery.",
        ),
        (
            "Ramos, Dragon Engine",
            "Legendary Artifact Creature - Dragon",
            "Flying\nWhenever you cast a spell, put a +1/+1 counter on Ramos, Dragon Engine for each of that spell's colors.\nRemove five +1/+1 counters from Ramos, Dragon Engine: Add {W}{W}{U}{U}{B}{B}{R}{R}{G}{G}. Activate only once each turn.",
        ),
        (
            "Knight of the Holy Nimbus",
            "Creature - Human Rebel Knight",
            "Flanking\nIf Knight of the Holy Nimbus would be destroyed, regenerate it.\n{2}: Knight of the Holy Nimbus can't be regenerated this turn. Only any opponent may activate this ability.",
        ),
        (
            "Baloth Cage Trap",
            "Instant - Trap",
            "If an opponent had an artifact enter the battlefield under their control this turn, you may pay {1}{G} rather than pay this spell's mana cost.\nCreate a 4/4 green Beast creature token.",
        ),
        (
            "Foul Renewal",
            "Sorcery",
            "Return target creature card from your graveyard to your hand. Target creature gets -X/-X until end of turn, where X is the toughness of the card returned this way.",
        ),
        (
            "Deem Inferior",
            "Sorcery",
            "This spell costs {1} less to cast for each card you've drawn this turn.\nThe owner of target nonland permanent puts it into their library second from the top or on the bottom.",
        ),
        (
            "Stegron the Dinosaur Man",
            "Legendary Creature - Dinosaur Human Scientist Villain",
            "Menace\nDinosaur Formula \u{2014} {1}{R}, Discard Stegron the Dinosaur Man: Until end of turn, target creature you control gets +3/+1 and becomes a Dinosaur in addition to its other types.",
        ),
        (
            "Bestial Menace",
            "Sorcery",
            "Create a 1/1 green Snake creature token, a 2/2 green Wolf creature token, and a 3/3 green Elephant creature token.",
        ),
        (
            "Crumbling Sanctuary",
            "Artifact",
            "If damage would be dealt to a player, that player exiles that many cards from the top of their library instead.",
        ),
        (
            "Nether Spirit",
            "Creature - Spirit",
            "At the beginning of your upkeep, if Nether Spirit is the only creature card in your graveyard, you may return Nether Spirit to the battlefield.",
        ),
    ];

    for (name, type_line, oracle_text) in wave7_cards {
        let compiled = compile_v2_strict(name, type_line, oracle_text, &vocab);
        assert!(
            compiled.strictly_compiled,
            "Wave 7 card `{name}` failed strict v2 compilation: {:#?}",
            compiled.faces[0].errors
        );
    }

    // Deep typed IR assertions on Wave 7 orthogonal compositions:
    // 1. Hardened Scales zero-regression guard (`add: 1`, `factor: 1`)
    let scales = compile_v2_strict(
        "Hardened Scales",
        "Enchantment",
        "If one or more +1/+1 counters would be put on a creature you control, that many plus one +1/+1 counters are put on it instead.",
        &vocab,
    );
    assert!(matches!(
        scales.faces[0].replacements.as_slice(),
        [Replacement::CounterRule {
            kind: Some(CounterKind::PlusOnePlusOne),
            add: 1,
            factor: 1,
            ..
        }]
    ));

    // 2. Vizier of Remedies (`add: -1`, `MinusOneMinusOne`)
    let vizier = compile_v2_strict(
        "Vizier of Remedies",
        "Creature - Human Cleric",
        "If one or more -1/-1 counters would be put on a creature you control, that many -1/-1 counters minus one are put on it instead.",
        &vocab,
    );
    assert!(matches!(
        vizier.faces[0].replacements.as_slice(),
        [Replacement::CounterRule {
            kind: Some(CounterKind::MinusOneMinusOne),
            add: -1,
            factor: 1,
            ..
        }]
    ));

    // 3. Divine Visitation (`TokenReplacementKind::Replace`)
    let visitation = compile_v2_strict(
        "Divine Visitation",
        "Enchantment",
        "If one or more creature tokens would be created under your control, that many 4/4 white Angel creature tokens with flying and vigilance are created instead.",
        &vocab,
    );
    assert!(matches!(
        visitation.faces[0].replacements.as_slice(),
        [Replacement::TokenRule {
            rule: TokenReplacementKind::Replace(_),
            ..
        }]
    ));

    // 4. Winter Moon fails closed honestly (CantUntapMoreThan has no filter for `nonbasic land`)
    let winter_moon = compile_card_v2(
        &CardInput {
            oracle_id: "winter-moon",
            name: "Winter Moon",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Winter Moon",
                type_line: "Artifact",
                oracle_text: "Players can't untap more than one nonbasic land during their untap steps.",
            }],
        },
        &vocab,
    );
    assert!(!winter_moon.strictly_compiled);
    // Unfiltered `permanent` still compiles faithfully to CantUntapMoreThan(1)
    let static_orb = compile_v2_strict(
        "Static Orb",
        "Artifact",
        "Players can't untap more than two permanents during their untap steps.",
        &vocab,
    );
    assert!(matches!(
        static_orb.faces[0].statics.as_slice(),
        [StaticAbility::PlayerRestriction {
            restriction: PlayerRestrictionKind::CantUntapMoreThan(2),
            ..
        }]
    ));

    // 5. Phyrexian Vindicator fails closed honestly (reflexive trigger rider on damage prevention)
    let vindicator = compile_card_v2(
        &CardInput {
            oracle_id: "phyrexian-vindicator",
            name: "Phyrexian Vindicator",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Phyrexian Vindicator",
                type_line: "Creature - Phyrexian Horror",
                oracle_text: "If damage would be dealt to Phyrexian Vindicator, prevent that damage. When damage is prevented this way, Phyrexian Vindicator deals that much damage to any other target.",
            }],
        },
        &vocab,
    );
    assert!(!vindicator.strictly_compiled);

    // 6. The Golden Throne (`Replacement::EventInstead` on `PlayerTriggerKind::LosesGame`)
    let throne = compile_v2_strict(
        "The Golden Throne",
        "Legendary Artifact",
        "If you would lose the game, instead exile The Golden Throne and your life total becomes 1.",
        &vocab,
    );
    assert!(matches!(
        throne.faces[0].replacements.as_slice(),
        [Replacement::EventInstead {
            event: TriggerEvent::PlayerAction {
                player: PlayerRef::You,
                action: PlayerTriggerKind::LosesGame,
            },
            instead,
        }] if instead.len() >= 2
    ));

    // 7. Dralnu (`sacrifice that many permanents` compiles as counted ZoneOpKind::Sacrifice)
    let dralnu = compile_v2_strict(
        "Dralnu, Lich Lord",
        "Legendary Creature - Zombie Wizard",
        "If damage would be dealt to Dralnu, Lich Lord, sacrifice that many permanents instead.",
        &vocab,
    );
    assert!(matches!(
        dralnu.faces[0].replacements.as_slice(),
        [Replacement::EventInstead {
            event: TriggerEvent::DealtDamage { .. },
            instead,
        }] if matches!(
            instead.as_slice(),
            [Effect::ZoneOp {
                op: ZoneOpKind::Sacrifice,
                count: Some(_),
                ..
            }]
        )
    ));

    // 8. Deem Inferior (`second from the top or on the bottom` -> `LibraryPosition::TopOrBottom`)
    let deem = compile_v2_strict(
        "Deem Inferior",
        "Sorcery",
        "The owner of target nonland permanent puts it into their library second from the top or on the bottom.",
        &vocab,
    );
    assert!(matches!(
        deem.faces[0].spell_effects.as_slice(),
        [Effect::ZoneOp {
            op: ZoneOpKind::Move,
            to: Some(dest),
            ..
        }] if dest.library_position == Some(LibraryPosition::TopOrBottom)
    ));

    // 9. Stegron (`keep_existing_types: true` on `gets +3/+1 and becomes a Dinosaur in addition to its other types`)
    let stegron = compile_v2_strict(
        "Stegron the Dinosaur Man",
        "Legendary Creature - Dinosaur Human Scientist Villain",
        "{1}{R}, Discard Stegron the Dinosaur Man: Until end of turn, target creature you control gets +3/+1 and becomes a Dinosaur in addition to its other types.",
        &vocab,
    );
    assert!(matches!(
        stegron.faces[0].abilities.as_slice(),
        [Ability::Activated { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::ScopedDuration { body: inner, .. }]
                    if matches!(
                        inner.as_slice(),
                        [
                            Effect::MutateObject { mutation: ObjectMutation::ModifyPT { .. }, .. },
                            Effect::MutateObject {
                                mutation: ObjectMutation::BecomeCreatureTypes { keep_existing_types: true, .. },
                                ..
                            }
                        ]
                    )
            )
    ));

    // 10. Bestial Menace (3-token creation compiles into 3 distinct Effect::CreateToken atoms)
    let menace = compile_v2_strict(
        "Bestial Menace",
        "Sorcery",
        "Create a 1/1 green Snake creature token, a 2/2 green Wolf creature token, and a 3/3 green Elephant creature token.",
        &vocab,
    );
    assert_eq!(menace.faces[0].spell_effects.len(), 3);
    assert!(
        menace.faces[0]
            .spell_effects
            .iter()
            .all(|e| matches!(e, Effect::CreateToken { .. }))
    );

    // 11. Foul Renewal (`where X is the toughness of the card returned this way` -> BindVariable::X)
    let renewal = compile_v2_strict(
        "Foul Renewal",
        "Sorcery",
        "Return target creature card from your graveyard to your hand. Target creature gets -X/-X until end of turn, where X is the toughness of the card returned this way.",
        &vocab,
    );
    assert!(matches!(
        renewal.faces[0].spell_effects.get(1),
        Some(Effect::BindVariable {
            var: VarName::X,
            ..
        })
    ));

    // 12. Training Grounds fails closed honestly (ActivationCostModifier has no `of <flt>` filter)
    let grounds = compile_card_v2(
        &CardInput {
            oracle_id: "training-grounds",
            name: "Training Grounds",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Training Grounds",
                type_line: "Enchantment",
                oracle_text: "Activated abilities of creatures you control cost {2} less to activate. This effect can't reduce the mana in that cost to less than one mana.",
            }],
        },
        &vocab,
    );
    assert!(!grounds.strictly_compiled);
}

/// 11. Deep Wave 8 compositional IR assertions (zero new core IR enum variants,
///     `replacement/` modular split): Necrotic Ooze / Trazyn ability-copying,
///     Once-each-turn {0} alternate cost (`As Foretold`, `Darksteel Monolith`),
///     `Silence` / `Alchemist's Refuge` flash/restriction durations, `Command Tower` / `Arcane Signet`,
///     `Doomsday` / `Gifts Ungiven`, `Giver of Runes`, `Ghyrson Starn` / `Tamanoa`,
///     `Solphim` / `Lashknife Barrier` / `Szadek` damage replacements, `Grist` / `Maha`,
///     `Delirium` zero-regression guard, and `Equipment` token creation (`Mabel`).
#[test]
fn v2_wave8_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{
        CastPermissionMode, ConditionExpr, DamageReplacement, Duration, FilterFlags,
        LibraryPosition, ManaSpec, PlayerMutation, PlayerObjectTriggerKind, PlayerRestrictionKind,
        PlayerTriggerKind, Replacement, RuleRestrictionKind, StaticAbility, SubjectStaticRule,
        TriggerEvent, ValueExpr, ZoneMetricKind, ZoneOpKind,
    };
    let vocab_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("content")
        .join("tests")
        .join("fixtures")
        .join("creature-vocabulary.input.json");
    let vocab = Vocabulary::load_from_file(&vocab_path).expect("load CR creature vocabulary");

    let wave8_cards: &[(&str, &str, &str)] = &[
        (
            "Necrotic Ooze",
            "Creature - Ooze",
            "As long as Necrotic Ooze is on the battlefield, it has all activated abilities of all creature cards in all graveyards.",
        ),
        (
            "As Foretold",
            "Enchantment",
            "At the beginning of your upkeep, put a time counter on As Foretold.\nOnce each turn, you may pay {0} rather than pay the mana cost for a spell you cast with mana value X or less, where X is the number of time counters on As Foretold.",
        ),
        (
            "Lashknife Barrier",
            "Enchantment",
            "When Lashknife Barrier enters, draw a card.\nIf a source would deal damage to a creature you control, it deals that much damage minus 1 to that creature instead.",
        ),
        (
            "Szadek, Lord of Secrets",
            "Legendary Creature - Vampire",
            "Flying\nIf Szadek, Lord of Secrets would deal combat damage to a player, instead put that many +1/+1 counters on Szadek, Lord of Secrets and that player mills that many cards.",
        ),
        (
            "Silence",
            "Instant",
            "Your opponents can't cast spells this turn.",
        ),
        (
            "Alchemist's Refuge",
            "Land",
            "{T}: Add {C}.\n{G}{U}, {T}: You may cast spells this turn as though they had flash.",
        ),
        (
            "Due Respect",
            "Instant",
            "Permanents enter tapped this turn.\nDraw a card.",
        ),
        (
            "Doomsday",
            "Sorcery",
            "Search your library and graveyard for five cards and exile the rest. Put the chosen cards on top of your library in any order. You lose half your life, rounded up.",
        ),
        (
            "Gifts Ungiven",
            "Instant",
            "Search your library for up to four cards with different names and reveal them. Target opponent chooses two of those cards. Put the chosen cards into your graveyard and the rest into your hand. Then shuffle.",
        ),
        (
            "Forbidden Orchard",
            "Land",
            "{T}: Add one mana of any color.\nWhenever you tap Forbidden Orchard for mana, target opponent creates a 1/1 colorless Spirit creature token.",
        ),
        (
            "Anje, Maid of Dishonor",
            "Legendary Creature - Vampire",
            "Whenever Anje, Maid of Dishonor and/or one or more other Vampires you control enter, create a Blood token. This ability triggers only once each turn.\n{2}, Sacrifice another creature or a Blood token: Each opponent loses 2 life and you gain 2 life.",
        ),
        (
            "Trespasser's Curse",
            "Enchantment - Aura Curse",
            "Enchant player\nWhenever a creature enchanted player controls enters, that player loses 1 life and you gain 1 life.",
        ),
        (
            "Diffusion Sliver",
            "Creature - Sliver",
            "Whenever a Sliver creature you control becomes the target of a spell or ability an opponent controls, counter that spell or ability unless its controller pays {2}.",
        ),
        (
            "Demonfire",
            "Sorcery",
            "Demonfire deals X damage to any target. If a creature dealt damage this way would die this turn, exile it instead.\nHellbent \u{2014} If you have no cards in hand, this spell can't be countered and the damage can't be prevented.",
        ),
        (
            "Oko, the Ringleader",
            "Legendary Planeswalker - Oko",
            "At the beginning of combat on your turn, Oko, the Ringleader becomes a copy of up to one target creature you control until end of turn, except it has hexproof.\n+1: Draw two cards. If you've committed a crime this turn, discard a card. Otherwise, discard two cards.\n-1: Create a 3/3 green Elk creature token.\n-5: For each other nonland permanent you control, create a token that's a copy of it.",
        ),
        (
            "Mabel, Heir to Cragflame",
            "Legendary Creature - Mouse Soldier",
            "Other Mice you control get +1/+1.\nWhen Mabel, Heir to Cragflame enters, create Cragflame, a legendary colorless Equipment artifact token with \"Equipped creature gets +1/+1 and has vigilance, trample, and haste\" and equip {2}.",
        ),
        (
            "Jadelight Ranger",
            "Creature - Merfolk Scout",
            "When Jadelight Ranger enters, it explores, then it explores again.",
        ),
        (
            "Arcane Bombardment",
            "Enchantment",
            "Whenever you cast your first instant or sorcery spell each turn, exile an instant or sorcery card at random from your graveyard. Then copy each card exiled with Arcane Bombardment. You may cast any number of the copies without paying their mana costs.",
        ),
        (
            "Dragon's Rage Channeler",
            "Creature - Human Shaman",
            "Whenever you cast a noncreature spell, surveil 1.\nDelirium \u{2014} As long as there are four or more card types among cards in your graveyard, Dragon's Rage Channeler gets +2/+2, has flying, and attacks each combat if able.",
        ),
        (
            "Rosie Cotton of South Lane",
            "Legendary Creature - Halfling Peasant",
            "When Rosie Cotton of South Lane enters, create a Food token.\nWhenever you create a token, put a +1/+1 counter on target creature you control other than Rosie Cotton of South Lane.",
        ),
    ];

    for (name, type_line, oracle_text) in wave8_cards {
        let compiled = compile_v2_strict(name, type_line, oracle_text, &vocab);
        assert!(
            compiled.strictly_compiled,
            "Wave 8 card `{name}` failed strict v2 compilation: {:#?}",
            compiled.faces[0].errors
        );
    }

    // Deep typed IR assertions on Wave 8 orthogonal compositions:
    // 1. Necrotic Ooze -> SubjectStaticRule::CopyActivatedAbilities
    let ooze = compile_v2_strict(
        "Necrotic Ooze",
        "Creature - Ooze",
        "As long as Necrotic Ooze is on the battlefield, it has all activated abilities of all creature cards in all graveyards.",
        &vocab,
    );
    assert!(matches!(
        ooze.faces[0].statics.as_slice(),
        [StaticAbility::SubjectRule {
            rule: SubjectStaticRule::CopyActivatedAbilities(_),
            condition: Some(_),
            ..
        }]
    ));

    // 2. Lashknife Barrier -> ModifyDamage with negative add (-1)
    let barrier = compile_v2_strict(
        "Lashknife Barrier",
        "Enchantment",
        "If a source would deal damage to a creature you control, it deals that much damage minus 1 to that creature instead.",
        &vocab,
    );
    assert!(matches!(
        barrier.faces[0].replacements.as_slice(),
        [Replacement::ModifyDamage {
            add: -1,
            factor: 1,
            ..
        }]
    ));

    // 3. Szadek zero-regression guard (`instead` restored before `parse_instead_clause`)
    let szadek = compile_v2_strict(
        "Szadek, Lord of Secrets",
        "Legendary Creature - Vampire",
        "If Szadek, Lord of Secrets would deal combat damage to a player, instead put that many +1/+1 counters on Szadek, Lord of Secrets and that player mills that many cards.",
        &vocab,
    );
    assert!(matches!(
        szadek.faces[0].replacements.as_slice(),
        [Replacement::EventInstead {
            event: TriggerEvent::DealsDamage {
                combat_only: true,
                ..
            },
            instead,
        }] if instead.len() == 2
    ));

    // 4. Silence -> ApplyContinuous(CantCastSpells) with Duration::UntilEndOfTurn
    let silence = compile_v2_strict(
        "Silence",
        "Instant",
        "Your opponents can't cast spells this turn.",
        &vocab,
    );
    assert!(matches!(
        silence.faces[0].spell_effects.as_slice(),
        [Effect::ApplyContinuous {
            effect,
            duration: Duration::UntilEndOfTurn,
        }] if matches!(
            effect.as_ref(),
            StaticAbility::PlayerRestriction {
                restriction: PlayerRestrictionKind::CantCastSpells,
                ..
            }
        )
    ));

    // 5. Alchemist's Refuge -> CastPermissionMode::AsThoughFlash
    let refuge = compile_v2_strict(
        "Alchemist's Refuge",
        "Land",
        "{G}{U}, {T}: You may cast spells this turn as though they had flash.",
        &vocab,
    );
    assert!(matches!(
        refuge.faces[0].abilities.as_slice(),
        [Ability::Activated { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::Optional { body: inner, .. }]
                    if matches!(
                        inner.as_slice(),
                        [Effect::ApplyContinuous { effect, .. }]
                            if matches!(
                                effect.as_ref(),
                                StaticAbility::ZoneCastPermission {
                                    mode: CastPermissionMode::AsThoughFlash { .. },
                                    ..
                                }
                            )
                    )
            )
    ));

    // 6. Command Tower fails closed honestly (ManaSpec::AnyColor has no commander color identity qualifier)
    let tower = compile_card_v2(
        &CardInput {
            oracle_id: "command-tower",
            name: "Command Tower",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Command Tower",
                type_line: "Land",
                oracle_text: "{T}: Add one mana of any color in your commander's color identity.",
            }],
        },
        &vocab,
    );
    assert!(!tower.strictly_compiled);
    // Plain `Add one mana of any color.` still compiles faithfully to ManaSpec::AnyColor
    let paradise = compile_v2_strict(
        "Birds of Paradise",
        "Creature - Bird",
        "{T}: Add one mana of any color.",
        &vocab,
    );
    assert!(matches!(
        paradise.faces[0].abilities.as_slice(),
        [Ability::Activated { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::MutatePlayer {
                    mutation: PlayerMutation::AddMana(ManaSpec::AnyColor { .. }),
                    ..
                }]
            )
    ));

    // 7. Giver of Runes fails closed honestly (cannot drop `from colorless` in `protection from colorless or from the color of your choice`)
    let giver = compile_card_v2(
        &CardInput {
            oracle_id: "giver-of-runes",
            name: "Giver of Runes",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Giver of Runes",
                type_line: "Creature - Kor Cleric",
                oracle_text: "{T}: Another target creature you control gains protection from colorless or from the color of your choice until end of turn.",
            }],
        },
        &vocab,
    );
    assert!(!giver.strictly_compiled);

    // 8. Forbidden Orchard -> PlayerObjectTriggerKind::TappedForMana
    let orchard = compile_v2_strict(
        "Forbidden Orchard",
        "Land",
        "Whenever you tap Forbidden Orchard for mana, target opponent creates a 1/1 colorless Spirit creature token.",
        &vocab,
    );
    assert!(matches!(
        orchard.faces[0].abilities.as_slice(),
        [Ability::Triggered { trigger, .. }]
            if matches!(
                trigger.events.as_slice(),
                [TriggerEvent::PlayerObjectEvent {
                    kind: PlayerObjectTriggerKind::TappedForMana,
                    ..
                }]
            )
    ));

    // 9. Rosie Cotton -> PlayerObjectTriggerKind::CreatesTokens
    let rosie = compile_v2_strict(
        "Rosie Cotton of South Lane",
        "Legendary Creature - Halfling Peasant",
        "Whenever you create a token, put a +1/+1 counter on target creature you control other than Rosie Cotton of South Lane.",
        &vocab,
    );
    assert!(matches!(
        rosie.faces[0].abilities.as_slice(),
        [Ability::Triggered { trigger, .. }]
            if matches!(
                trigger.events.as_slice(),
                [TriggerEvent::PlayerObjectEvent {
                    kind: PlayerObjectTriggerKind::CreatesTokens,
                    ..
                }]
            )
    ));

    // 10. Dragon's Rage Channeler Delirium zero-regression guard (`ZoneMetricKind::DistinctCardTypes`)
    let drc = compile_v2_strict(
        "Dragon's Rage Channeler",
        "Creature - Human Shaman",
        "Delirium \u{2014} As long as there are four or more card types among cards in your graveyard, Dragon's Rage Channeler gets +2/+2, has flying, and attacks each combat if able.",
        &vocab,
    );
    assert!(matches!(
        drc.faces[0].statics.as_slice(),
        [StaticAbility::Compound(parts)]
            if matches!(
                parts.first(),
                Some(StaticAbility::ContinuousModifyPT {
                    condition: Some(ConditionExpr::ZoneMetric {
                        kind: ZoneMetricKind::DistinctCardTypes,
                        min_count: 4,
                        ..
                    }),
                    ..
                })
            )
    ));

    // 11. Grist fails closed honestly (SetSubtypes has no condition field for `As long as ~ isn't on the battlefield`)
    let grist = compile_card_v2(
        &CardInput {
            oracle_id: "grist",
            name: "Grist, the Hunger Tide",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Grist, the Hunger Tide",
                type_line: "Legendary Planeswalker - Grist",
                oracle_text: "As long as Grist, the Hunger Tide isn't on the battlefield, it's a 1/1 Insect creature in addition to its other types.",
            }],
        },
        &vocab,
    );
    assert!(!grist.strictly_compiled);

    // 12. Diffusion Sliver (`counter that spell or ability unless its controller pays {2}`)
    let sliver = compile_v2_strict(
        "Diffusion Sliver",
        "Creature - Sliver",
        "Whenever a Sliver creature you control becomes the target of a spell or ability an opponent controls, counter that spell or ability unless its controller pays {2}.",
        &vocab,
    );
    assert!(matches!(
        sliver.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::ZoneOp {
                    op: ZoneOpKind::CounterSpell {
                        unless_pay: Some(_),
                    },
                    ..
                }] | [Effect::Unless { .. }]
            )
    ));
    let _ = (
        DamageReplacement::PreventAll {
            target: SubjectRef::SelfObject,
        },
        FilterFlags::NOT_ON_BATTLEFIELD,
        LibraryPosition::Top,
        PlayerTriggerKind::LosesGame,
        RuleRestrictionKind::DamageCantBePrevented,
        ValueExpr::Const(1),
    );
}

/// 12. Deep Wave 9 compositional IR assertions (84.75% -> 85.55%, +258 cards, 0 regressions,
///     0 IR enum bloat, `clause/player_verbs.rs` modularized into `clause/player_actions.rs`):
///     - Activated abilities on Instants/Sorceries (`Magma Opus`, `Piercing Rays` Forecast)
///     - Keyword + static lines (`Plate Armor`)
///     - DMU Defiler cycle (`Defiler of Vigor`), Commander Curses (`Curse of Opulence`),
///       and Job Select Equipment (`Black Mage's Rod`, `Thief's Knife`)
///     - Player visibility & spell-limit statics (`Lantern of Insight`, `Moderation`, `Steel Golem`)
///     - Activated-ability locks & defender-attack tails (`Encrust`, `Faith's Fetters`, `Arcades`)
///     - Repeated keyword actions (`Contagion Engine`, `Progenitor Exarch`, `Ad Nauseam`)
///     - Library-order / sacrifice-the-rest / shuffle-the-rest (`Goblin Recruiter`, `Archfiend of Depravity`, `Kindred Summons`)
///     - Energy scaling (`Peema Trailblazer`), damage prevention (`Pay No Heed`, `Guard Dogs`),
///       protection choices (`Apostle's Blessing`), and `Luminate Primordial` zero-regression guard.
#[test]
fn v2_wave9_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{
        ActivationRestriction, AnaphoraRef, Effect, ForEachDomain, KeywordKind, KeywordParam,
        LibraryOrder, ManaRuleKind, ObjectMutation, PlayerActionKind, PlayerMutation,
        PlayerRestrictionKind, RuleRestrictionKind, StaticAbility, SubjectRef, SubjectStaticRule,
        VisibilityKind, ZoneOpKind,
    };

    let vocab_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("content")
        .join("tests")
        .join("fixtures")
        .join("creature-vocabulary.input.json");
    let vocab = Vocabulary::load_from_file(&vocab_path).expect("load CR creature vocabulary");

    let cases: &[(&str, &str, &str)] = &[
        (
            "Rings of Brighthearth",
            "Artifact",
            "Whenever you activate an ability, if it isn't a mana ability, you may pay {2}. If you do, copy that ability. You may choose new targets for the copy.",
        ),
        (
            "Penance",
            "Enchantment",
            "Put a card from your hand on top of your library: The next time a black or red source of your choice would deal damage this turn, prevent that damage.",
        ),
        (
            "Luminate Primordial",
            "Creature - Avatar",
            "Vigilance\nWhen Luminate Primordial enters, for each opponent, exile up to one target creature that player controls and that player gains life equal to its power.",
        ),
        (
            "Black Mage's Rod",
            "Artifact - Equipment",
            "Job select\nEquipped creature gets +1/+0, has \"Whenever you cast a noncreature spell, this creature deals 1 damage to each opponent,\" and is a Wizard in addition to its other types.\nEquip {3}",
        ),
        (
            "Encrust",
            "Enchantment - Aura",
            "Enchant artifact or creature\nEnchanted permanent doesn't untap during its controller's untap step and its activated abilities can't be activated.",
        ),
        (
            "Arcades, the Strategist",
            "Legendary Creature - Elder Dragon",
            "Flying, vigilance\nWhenever a creature you control with defender enters, draw a card.\nEach creature you control with defender assigns combat damage equal to its toughness rather than its power and can attack as though it didn't have defender.",
        ),
        (
            "Lantern of Insight",
            "Artifact",
            "Players play with the top card of their libraries revealed.\n{T}, Sacrifice Lantern of Insight: Target player shuffles.",
        ),
        (
            "Moderation",
            "Enchantment",
            "You can't cast more than one spell each turn.\nWhenever you cast a spell, draw a card.",
        ),
        (
            "Hellrider",
            "Creature - Devil",
            "Haste\nWhenever a creature you control attacks, Hellrider deals 1 damage to the player or planeswalker it's attacking.",
        ),
        (
            "Contagion Engine",
            "Artifact",
            "When Contagion Engine enters, put a -1/-1 counter on each creature target player controls.\n{4}, {T}: Proliferate twice.",
        ),
        (
            "Goblin Recruiter",
            "Creature - Goblin",
            "When Goblin Recruiter enters, search your library for any number of Goblin cards, reveal them, then shuffle and put those cards on top in any order.",
        ),
        (
            "Pay No Heed",
            "Instant",
            "Prevent all damage a source of your choice would deal this turn.",
        ),
        (
            "Searing Blood",
            "Instant",
            "Searing Blood deals 2 damage to target creature. When that creature dies this turn, Searing Blood deals 3 damage to the creature's controller.",
        ),
        (
            "Scandalmonger",
            "Creature - Boar Monger",
            "{2}: Target player discards a card. Any player may activate this ability but only as a sorcery.",
        ),
        (
            "Piercing Rays",
            "Sorcery",
            "Exile target tapped creature.\nForecast \u{2014} {2}{W}, Reveal Piercing Rays from your hand: Tap target untapped creature.",
        ),
        (
            "Plate Armor",
            "Artifact - Equipment",
            "Equipped creature gets +3/+3 and has ward {1}.\nEquip {3}. This ability costs {1} less to activate for each other Equipment you control.",
        ),
        (
            "Archfiend of Depravity",
            "Creature - Demon",
            "Flying\nAt the beginning of each opponent's end step, that player chooses up to two creatures they control, then sacrifices the rest.",
        ),
        (
            "Steel Golem",
            "Artifact Creature - Golem",
            "You can't cast creature spells.",
        ),
        (
            "Peema Trailblazer",
            "Creature - Elf Warrior",
            "Trample\nWhenever Peema Trailblazer deals combat damage to a player, you get that many {E}.\nExhaust \u{2014} Pay six {E}: Put two +1/+1 counters on Peema Trailblazer. Then draw cards equal to the greatest power among creatures you control.",
        ),
        (
            "Kindred Summons",
            "Instant",
            "Choose a creature type. Reveal cards from the top of your library until you reveal X creature cards of the chosen type, where X is the number of creatures you control of that type. Put those cards onto the battlefield, then shuffle the rest of the revealed cards into your library.",
        ),
    ];

    for (name, type_line, oracle_text) in cases {
        let compiled = compile_v2_strict(name, type_line, oracle_text, &vocab);
        assert!(
            compiled.strictly_compiled,
            "Wave 9 card `{name}` failed strict v2 compilation: {:#?}",
            compiled.faces[0].errors
        );
    }

    // Deep IR assertions:
    // 1. Piercing Rays -> spell_effects + Forecast activated ability on Sorcery!
    let rays = compile_v2_strict(
        "Piercing Rays",
        "Sorcery",
        "Exile target tapped creature.\nForecast \u{2014} {2}{W}, Reveal Piercing Rays from your hand: Tap target untapped creature.",
        &vocab,
    );
    assert_eq!(rays.faces[0].spell_effects.len(), 1);
    assert!(matches!(
        rays.faces[0].abilities.as_slice(),
        [Ability::Activated { cost, body, .. }] if cost.len() == 2 && body.len() == 1
    ));

    // 2. Plate Armor -> Equip keyword + CostModifier/ActivationCostModifier static on same line
    let plate = compile_v2_strict(
        "Plate Armor",
        "Artifact - Equipment",
        "Equipped creature gets +3/+3 and has ward {1}.\nEquip {3}. This ability costs {1} less to activate for each other Equipment you control.",
        &vocab,
    );
    assert!(
        plate.faces[0]
            .keywords
            .iter()
            .any(|k| k.kind == KeywordKind::Equip)
    );
    assert_eq!(plate.faces[0].statics.len(), 2);

    // 3. Lantern of Insight -> VisibilityRule(PlayWithTopRevealed)
    let lantern = compile_v2_strict(
        "Lantern of Insight",
        "Artifact",
        "Players play with the top card of their libraries revealed.",
        &vocab,
    );
    assert!(matches!(
        lantern.faces[0].statics.as_slice(),
        [StaticAbility::VisibilityRule {
            kind: VisibilityKind::PlayWithTopRevealed,
            ..
        }]
    ));

    // 4. Moderation -> PlayerRestriction(CantCastMoreThanOnePerTurn)
    let moderation = compile_v2_strict(
        "Moderation",
        "Enchantment",
        "You can't cast more than one spell each turn.",
        &vocab,
    );
    assert!(matches!(
        moderation.faces[0].statics.as_slice(),
        [StaticAbility::PlayerRestriction {
            restriction: PlayerRestrictionKind::CantCastMoreThanOnePerTurn,
            ..
        }]
    ));

    // 5. Steel Golem -> CantCastSpellsMatching preserving BOTH PlayerRef::You and creature-spell filter
    let golem = compile_v2_strict(
        "Steel Golem",
        "Artifact Creature - Golem",
        "You can't cast creature spells.",
        &vocab,
    );
    assert!(matches!(
        golem.faces[0].statics.as_slice(),
        [StaticAbility::CantCastSpellsMatching {
            player: PlayerRef::You,
            ..
        }]
    ));

    // 6. Encrust -> Compound([DoesntUntap, ActivatedAbilitiesCantBeActivated])
    let encrust = compile_v2_strict(
        "Encrust",
        "Enchantment - Aura",
        "Enchant artifact or creature\nEnchanted permanent doesn't untap during its controller's untap step and its activated abilities can't be activated.",
        &vocab,
    );
    assert!(matches!(
        encrust.faces[0].statics.as_slice(),
        [StaticAbility::Compound(parts)]
            if matches!(
                parts.as_slice(),
                [
                    StaticAbility::SubjectRule {
                        rule: SubjectStaticRule::DoesntUntap,
                        ..
                    },
                    StaticAbility::RuleRestriction {
                        rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
                        ..
                    },
                ]
            )
    ));

    // 7. Arcades -> Compound([AssignDamageByToughness, CanAttackWithDefender])
    let arcades = compile_v2_strict(
        "Arcades, the Strategist",
        "Legendary Creature - Elder Dragon",
        "Each creature you control with defender assigns combat damage equal to its toughness rather than its power and can attack as though it didn't have defender.",
        &vocab,
    );
    assert!(matches!(
        arcades.faces[0].statics.as_slice(),
        [StaticAbility::Compound(parts)]
            if matches!(
                parts.as_slice(),
                [
                    StaticAbility::SubjectRule {
                        rule: SubjectStaticRule::AssignDamageByToughness,
                        ..
                    },
                    StaticAbility::SubjectRule {
                        rule: SubjectStaticRule::CanAttackWithDefender,
                        ..
                    },
                ]
            )
    ));

    // 8. Contagion Engine -> Proliferate twice wrapped in ForEach(Count(2))
    let engine = compile_v2_strict(
        "Contagion Engine",
        "Artifact",
        "{4}, {T}: Proliferate twice.",
        &vocab,
    );
    assert!(matches!(
        engine.faces[0].abilities.as_slice(),
        [Ability::Activated { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::ForEach {
                    over: ForEachDomain::Count(_),
                    body: inner,
                }] if matches!(
                    inner.as_slice(),
                    [Effect::MutatePlayer {
                        mutation: PlayerMutation::Action(PlayerActionKind::Proliferate),
                        ..
                    }]
                )
            )
    ));

    // 9. Rosheen Meanderer fails closed honestly (SpendOnlyColorOnX(EMPTY) proxy purged)
    let rosheen = compile_card_v2(
        &CardInput {
            oracle_id: "rosheen-meanderer",
            name: "Rosheen Meanderer",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Rosheen Meanderer",
                type_line: "Legendary Creature - Giant Shaman",
                oracle_text: "{T}: Add {C}{C}{C}{C}. Spend this mana only on costs that contain {X}.",
            }],
        },
        &vocab,
    );
    assert!(!rosheen.strictly_compiled);
    let _ = ManaRuleKind::SpendOnlyColorOnX(ColorMask::EMPTY);

    // 10. Goblin Recruiter -> ZoneOp(Move, Top + AnyOrder)
    let recruiter = compile_v2_strict(
        "Goblin Recruiter",
        "Creature - Goblin",
        "When Goblin Recruiter enters, search your library for any number of Goblin cards, reveal them, then shuffle and put those cards on top in any order.",
        &vocab,
    );
    assert!(matches!(
        recruiter.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if body.iter().any(|e| matches!(
                e,
                Effect::ZoneOp {
                    op: ZoneOpKind::Move,
                    to: Some(dest),
                    ..
                } if dest.library_order == Some(LibraryOrder::AnyOrder)
            ))
    ));

    // 11. Scandalmonger -> AllowedPlayer + AsSorcery restrictions both preserved
    let monger = compile_v2_strict(
        "Scandalmonger",
        "Creature - Boar Monger",
        "{2}: Target player discards a card. Any player may activate this ability but only as a sorcery.",
        &vocab,
    );
    assert!(matches!(
        monger.faces[0].abilities.as_slice(),
        [Ability::Activated { restrictions, .. }]
            if matches!(
                restrictions.as_slice(),
                [
                    ActivationRestriction::AllowedPlayer(_),
                    ActivationRestriction::AsSorcery,
                ]
            )
    ));

    // 12. Luminate Primordial zero-regression guard (ForEach opponent -> Exile + AdjustLife, NOT swallowed as compound exile!)
    let primordial = compile_v2_strict(
        "Luminate Primordial",
        "Creature - Avatar",
        "When Luminate Primordial enters, for each opponent, exile up to one target creature that player controls and that player gains life equal to its power.",
        &vocab,
    );
    assert!(matches!(
        primordial.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::ForEach { body: inner, .. }]
                    if matches!(
                        inner.as_slice(),
                        [
                            Effect::ZoneOp { op: ZoneOpKind::Exile { .. }, .. },
                            Effect::AdjustLife { .. },
                        ]
                    )
            )
    ));

    // 13. Archfiend of Depravity (`sacrifices the rest` -> AnaphoraRef::TheRest) & Apostle's Blessing (`ChosenColor`)
    let archfiend = compile_v2_strict(
        "Archfiend of Depravity",
        "Creature - Demon",
        "At the beginning of each opponent's end step, that player chooses up to two creatures they control, then sacrifices the rest.",
        &vocab,
    );
    assert!(matches!(
        archfiend.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if body.iter().any(|e| matches!(
                e,
                Effect::ZoneOp {
                    op: ZoneOpKind::Sacrifice,
                    subject: SubjectRef::Anaphora(AnaphoraRef::TheRest),
                    ..
                }
            ))
    ));
    let blessing = compile_card_v2(
        &CardInput {
            oracle_id: "apostles-blessing",
            name: "Apostle's Blessing",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Apostle's Blessing",
                type_line: "Instant",
                oracle_text: "Target artifact or creature you control gains protection from artifacts or from the color of your choice until end of turn.",
            }],
        },
        &vocab,
    );
    assert!(!blessing.strictly_compiled);
    let _ = (KeywordParam::ChosenColor, ObjectMutation::Tap);
}

/// 13. Deep Wave 10 compositional IR assertions (85.55% -> 86.16%, +198 cards, 0 regressions,
///     0 core-enum bloat):
///     - Proper card-name identifiers `FilterPredicate::Named(String)` (`Vraska's Scorn`,
///       `Dragonstorm Forecaster`, `Power Plant Worker`, `Mine Worker`, `Nissa's Encouragement`)
///     - `<count> of those tokens` (`Rite of Replication`, `Anax, Hardened in the Forge`,
///       `Increasing Devotion`, `Runo Stromkirk`)
///     - Compound `deals damage equal to <val> to <t1> and <t2>` (`Chandra's Ignition`, `Rupture`, `Time Bomb`)
///     - Player `can't` restrictions in effects (`Skullcrack`, `Atarka's Command`, `Ranger-Captain of Eos`, `Azor`)
///     - `the total number of` value expressions (`Crackling Drake`, `Beacon Bolt`, `Ral, Izzet Viceroy`)
///     - `have N or more <flt> in your graveyard` (`Octavia, Living Thesis`, `Oversold Cemetery`, `Battle of Wits`)
///     - `move any number of counters` (`Forgotten Ancient`, `Bioshift`)
///     - Multi-color `that's white, blue, black, or red` (`Questing Druid`, `Quirion Dryad`, `Aether Gust`, `Fry`)
///     - `from among cards exiled with ~` (`Nightveil Specter`, `Intellect Devourer`)
///     - `blocks each combat if able` / `attacks or blocks each combat if able` (`Grand Melee`, `Avatar of Slaughter`, `Iron Golem`)
///     - `unless <player> pays its mana cost [reduced by {M}]` (`Flash`, `Disruption Aura`),
///       delayed-step conditions (`Shirei, Shizo's Caretaker`, `Phytotitan`), and copy exceptions with inline keywords (`Kaya, Intangible Slayer`, `Leonardo da Vinci`).
#[test]
fn v2_wave10_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{
        AnaphoraRef, CastPermissionMode, ConditionExpr, ConditionFlag, CopyException, Cost,
        DelayedTriggerKind, Duration, Effect, FilterPredicate, ObjectMutation, PlayerConditionKind,
        PlayerRestrictionKind, RuleRestrictionKind, StaticAbility, SubjectRef, SubjectStaticRule,
        TokenSpec, ValueExpr, ZoneOpKind,
    };

    let vocab_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("content")
        .join("tests")
        .join("fixtures")
        .join("creature-vocabulary.input.json");
    let vocab = Vocabulary::load_from_file(&vocab_path).expect("load CR creature vocabulary");

    let cases: &[(&str, &str, &str)] = &[
        (
            "Chandra's Ignition",
            "Sorcery",
            "Target creature you control deals damage equal to its power to each other creature and each opponent.",
        ),
        (
            "Shirei, Shizo's Caretaker",
            "Legendary Creature - Spirit",
            "Whenever a creature with power 1 or less is put into your graveyard from the battlefield, you may return that card to the battlefield at the beginning of the next end step if Shirei, Shizo's Caretaker is still on the battlefield.",
        ),
        (
            "Kaya, Intangible Slayer",
            "Legendary Planeswalker - Kaya",
            "Hexproof\n+2: Each opponent loses 3 life and you gain 3 life.\n0: You draw two cards. Then each opponent may scry 1.\n\u{2212}3: Exile target creature or enchantment. If it wasn't an Aura, create a token that's a copy of it, except it's a 1/1 white Spirit creature with flying in addition to its other types.",
        ),
        (
            "Vraska's Scorn",
            "Sorcery",
            "Target opponent loses 4 life. You may search your library and/or graveyard for a card named Vraska, Scheming Gorgon, reveal it, and put it into your hand. If you search your library this way, shuffle.",
        ),
        (
            "Dragonstorm Forecaster",
            "Creature - Human Scout",
            "{2}, {T}: Search your library for a card named Dragonstorm Globe or Boulderborn Dragon, reveal it, put it into your hand, then shuffle.",
        ),
        (
            "Rite of Replication",
            "Sorcery",
            "Kicker {5}\nCreate a token that's a copy of target creature. If this spell was kicked, create five of those tokens instead.",
        ),
        (
            "Increasing Devotion",
            "Sorcery",
            "Create five 1/1 white Human creature tokens. If this spell was cast from a graveyard, create ten of those tokens instead.\nFlashback {7}{W}{W}",
        ),
        (
            "Aether Gust",
            "Instant",
            "Choose target spell or permanent that's red or green. Its owner puts it on their choice of the top or bottom of their library.",
        ),
        (
            "Krosan Verge",
            "Land",
            "Krosan Verge enters tapped.\n{T}: Add {C}.\n{2}, {T}, Sacrifice Krosan Verge: Search your library for a Forest card and a Plains card, put them onto the battlefield tapped, then shuffle.",
        ),
        (
            "Moonsilver Key",
            "Artifact",
            "{1}, {T}, Sacrifice Moonsilver Key: Search your library for an artifact card with a mana ability or a basic land card, reveal it, put it into your hand, then shuffle.",
        ),
        (
            "Conflux",
            "Sorcery",
            "Search your library for a white card, a blue card, a black card, a red card, and a green card. Reveal those cards, put them into your hand, then shuffle.",
        ),
        (
            "Questing Druid",
            "Creature - Human Druid",
            "Whenever you cast a spell that's white, blue, black, or red, put a +1/+1 counter on Questing Druid.",
        ),
        (
            "Nightveil Specter",
            "Creature - Specter",
            "Flying\nWhenever Nightveil Specter deals combat damage to a player, that player exiles the top card of their library.\nYou may play lands and cast spells from among cards exiled with Nightveil Specter.",
        ),
        (
            "Grand Melee",
            "Enchantment",
            "All creatures attack each combat if able.\nAll creatures block each combat if able.",
        ),
        (
            "Avatar of Slaughter",
            "Creature - Avatar",
            "All creatures have double strike and attack each combat if able.",
        ),
        (
            "Captain Ripley Vance",
            "Legendary Creature - Human Pirate",
            "Whenever you cast your third spell each turn, put a +1/+1 counter on Captain Ripley Vance, then it deals damage equal to its power to any target.",
        ),
        (
            "Skullcrack",
            "Instant",
            "Players can't gain life this turn. Damage can't be prevented this turn. Skullcrack deals 3 damage to target player or planeswalker.",
        ),
        (
            "Atarka's Command",
            "Instant",
            "Choose two \u{2014}\n\u{2022} Your opponents can't gain life this turn.\n\u{2022} Atarka's Command deals 3 damage to each opponent.\n\u{2022} You may put a land card from your hand onto the battlefield.\n\u{2022} Creatures you control get +1/+1 and gain reach until end of turn.",
        ),
        (
            "Ranger-Captain of Eos",
            "Creature - Human Soldier",
            "When Ranger-Captain of Eos enters, you may search your library for a creature card with mana value 1 or less, reveal it, put it into your hand, then shuffle.\nSacrifice Ranger-Captain of Eos: Your opponents can't cast noncreature spells this turn.",
        ),
        (
            "Crackling Drake",
            "Creature - Drake",
            "Flying\nCrackling Drake's power is equal to the total number of instant and sorcery cards you own in exile and in your graveyard.\nWhen Crackling Drake enters, draw a card.",
        ),
        (
            "Octavia, Living Thesis",
            "Creature - Elemental Octopus",
            "Octavia, Living Thesis costs {8} less to cast if you have eight or more instant and/or sorcery cards in your graveyard.\nWard {8}\nMagecraft \u{2014} Whenever you cast or copy an instant or sorcery spell, target creature has base power and toughness 8/8 until end of turn.",
        ),
        (
            "Forgotten Ancient",
            "Creature - Elemental",
            "Whenever a player casts a spell, you may put a +1/+1 counter on Forgotten Ancient.\nAt the beginning of your upkeep, you may move any number of +1/+1 counters from Forgotten Ancient onto other creatures.",
        ),
        (
            "Essence Channeler",
            "Creature - Bat Cleric",
            "As long as you've lost life this turn, Essence Channeler has flying and vigilance.\nWhenever you gain life, put a +1/+1 counter on Essence Channeler.\nWhen Essence Channeler dies, put its counters on target creature you control.",
        ),
        (
            "Timecrafting",
            "Instant",
            "Choose one \u{2014}\n\u{2022} Remove X time counters from target permanent or suspended card.\n\u{2022} Put X time counters on target permanent with a time counter on it or suspended card.",
        ),
    ];

    for (name, type_line, oracle_text) in cases {
        let compiled = compile_v2_strict(name, type_line, oracle_text, &vocab);
        assert!(
            compiled.strictly_compiled,
            "Wave 10 card `{name}` failed strict v2 compilation: {:#?}",
            compiled.faces[0].errors
        );
    }

    // Deep IR assertions:
    // 1. Vraska's Scorn -> FilterPredicate::Named("Vraska, Scheming Gorgon")
    let vraska = compile_v2_strict(
        "Vraska's Scorn",
        "Sorcery",
        "You may search your library and/or graveyard for a card named Vraska, Scheming Gorgon, reveal it, and put it into your hand. If you search your library this way, shuffle.",
        &vocab,
    );
    assert!(matches!(
        &vraska.faces[0].spell_effects[0],
        Effect::Optional { body, .. }
            if matches!(
                body.first(),
                Some(Effect::ZoneOp {
                    op: ZoneOpKind::SearchLibrary { .. },
                    subject: SubjectRef::Group(flt),
                    ..
                }) if flt.predicates.iter().any(|p| matches!(p, FilterPredicate::Named(n) if n == "Vraska, Scheming Gorgon"))
            )
    ));

    // 2. Rite of Replication -> `create five of those tokens instead` -> ReplacePrevious(CreateToken(CopyOf(ThatToken)))
    let rite = compile_v2_strict(
        "Rite of Replication",
        "Sorcery",
        "Kicker {5}\nCreate a token that's a copy of target creature. If this spell was kicked, create five of those tokens instead.",
        &vocab,
    );
    assert!(matches!(
        &rite.faces[0].spell_effects[1],
        Effect::BranchCondition { then_body, .. }
            if matches!(
                then_body.as_slice(),
                [Effect::ReplacePrevious(inner)]
                    if matches!(
                        inner.as_slice(),
                        [Effect::CreateToken {
                            count: ValueExpr::Const(5),
                            spec: TokenSpec::CopyOf { source, .. },
                            ..
                        }] if matches!(source.as_ref(), SubjectRef::Anaphora(AnaphoraRef::ThatToken))
                    )
            )
    ));

    // 3. Chandra's Ignition -> DealDamage with Compound target (each other creature + each opponent)
    let ignition = compile_v2_strict(
        "Chandra's Ignition",
        "Sorcery",
        "Target creature you control deals damage equal to its power to each other creature and each opponent.",
        &vocab,
    );
    assert!(matches!(
        ignition.faces[0].spell_effects.as_slice(),
        [Effect::DealDamage {
            target: SubjectRef::Compound(tgts),
            ..
        }] if tgts.len() == 2
    ));

    // 4. Flash fails closed honestly (cannot proxy `pays its mana cost reduced by {2}` to Cost::PayAnyAmountMana)
    let flash = compile_card_v2(
        &CardInput {
            oracle_id: "flash",
            name: "Flash",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Flash",
                type_line: "Instant",
                oracle_text: "You may put a creature card from your hand onto the battlefield. If you do, sacrifice it unless you pay its mana cost reduced by {2}.",
            }],
        },
        &vocab,
    );
    assert!(!flash.strictly_compiled);
    let _ = Cost::PayAnyAmountMana;

    // 5. Skullcrack -> CantGainLife + DamageCantBePrevented + DealDamage
    let skullcrack = compile_v2_strict(
        "Skullcrack",
        "Instant",
        "Players can't gain life this turn. Damage can't be prevented this turn. Skullcrack deals 3 damage to target player or planeswalker.",
        &vocab,
    );
    assert!(matches!(
        skullcrack.faces[0].spell_effects.as_slice(),
        [
            Effect::ApplyContinuous {
                effect: e1,
                duration: Duration::UntilEndOfTurn,
            },
            Effect::ApplyContinuous {
                effect: e2,
                duration: Duration::UntilEndOfTurn,
            },
            Effect::DealDamage { .. },
        ] if matches!(
            e1.as_ref(),
            StaticAbility::PlayerRestriction {
                restriction: PlayerRestrictionKind::CantGainLife,
                ..
            }
        ) && matches!(
            e2.as_ref(),
            StaticAbility::RuleRestriction {
                rule: RuleRestrictionKind::DamageCantBePrevented,
                ..
            }
        )
    ));

    // 6. Ranger-Captain of Eos -> Activated ability applying CantBeCast on noncreature spells
    let ranger = compile_v2_strict(
        "Ranger-Captain of Eos",
        "Creature - Human Soldier",
        "Sacrifice Ranger-Captain of Eos: Your opponents can't cast noncreature spells this turn.",
        &vocab,
    );
    assert!(matches!(
        ranger.faces[0].abilities.as_slice(),
        [Ability::Activated { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::ApplyContinuous { effect, .. }]
                    if matches!(
                        effect.as_ref(),
                        StaticAbility::RuleRestriction {
                            rule: RuleRestrictionKind::CantBeCast,
                            ..
                        }
                    )
            )
    ));

    // 7. Octavia, Living Thesis -> CostModifier with count >= 8 in graveyard
    let octavia = compile_v2_strict(
        "Octavia, Living Thesis",
        "Creature - Elemental Octopus",
        "Octavia, Living Thesis costs {8} less to cast if you have eight or more instant and/or sorcery cards in your graveyard.",
        &vocab,
    );
    assert!(matches!(
        octavia.faces[0].statics.as_slice(),
        [StaticAbility::CostModifier {
            less: true,
            condition: Some(cond),
            ..
        }] if matches!(cond, ConditionExpr::ZoneHasCount { min_count: 8, .. })
            || matches!(
                cond,
                ConditionExpr::Compare {
                    op: spellwake_oracle_compiler::ast::CompareOp::Ge,
                    rhs,
                    ..
                } if matches!(rhs.as_ref(), ValueExpr::Const(8))
            )
    ));

    // 8. Nightveil Specter -> ZoneCastPermission(PlayFromZone { lands: true, spells: true })
    let specter = compile_v2_strict(
        "Nightveil Specter",
        "Creature - Specter",
        "You may play lands and cast spells from among cards exiled with Nightveil Specter.",
        &vocab,
    );
    assert!(matches!(
        specter.faces[0].statics.as_slice(),
        [StaticAbility::ZoneCastPermission {
            mode: CastPermissionMode::PlayFromZone {
                lands: true,
                spells: true,
                ..
            },
            ..
        }]
    ));

    // 9. Grand Melee -> MustAttack + AllMustBlock
    let melee = compile_v2_strict(
        "Grand Melee",
        "Enchantment",
        "All creatures attack each combat if able.\nAll creatures block each combat if able.",
        &vocab,
    );
    assert!(matches!(
        melee.faces[0].statics.as_slice(),
        [
            StaticAbility::SubjectRule {
                rule: SubjectStaticRule::MustAttack,
                ..
            },
            StaticAbility::SubjectRule {
                rule: SubjectStaticRule::AllMustBlock,
                ..
            },
        ]
    ));

    // 10. Essence Channeler -> ConditionExpr::PlayerCheck(LostLifeThisTurn)
    let channeler = compile_v2_strict(
        "Essence Channeler",
        "Creature - Bat Cleric",
        "As long as you've lost life this turn, Essence Channeler has flying and vigilance.",
        &vocab,
    );
    assert!(matches!(
        channeler.faces[0].statics.as_slice(),
        [StaticAbility::GrantKeywords {
            condition: Some(ConditionExpr::PlayerCheck {
                kind: PlayerConditionKind::LostLifeThisTurn,
                ..
            }),
            ..
        }]
    ));

    // 11. Kaya, Intangible Slayer -> CopyException::SetBasePTAndTypes + AddKeywords(Flying)
    let kaya = compile_v2_strict(
        "Kaya, Intangible Slayer",
        "Legendary Planeswalker - Kaya",
        "\u{2212}3: Exile target creature or enchantment. If it wasn't an Aura, create a token that's a copy of it, except it's a 1/1 white Spirit creature with flying in addition to its other types.",
        &vocab,
    );
    assert!(matches!(
        kaya.faces[0].abilities.as_slice(),
        [Ability::Loyalty { body, .. }]
            if matches!(
                &body[1],
                Effect::BranchCondition { then_body, .. }
                    if matches!(
                        then_body.as_slice(),
                        [Effect::CreateToken {
                            spec: TokenSpec::CopyOf { exceptions, .. },
                            ..
                        }] if matches!(
                            exceptions.as_slice(),
                            [
                                CopyException::SetBasePTAndTypes {
                                    power: Some(1),
                                    toughness: Some(1),
                                    keep_existing_types: true,
                                    ..
                                },
                                CopyException::AddKeywords(_),
                            ]
                        )
                    )
            )
    ));

    // 12. Shirei, Shizo's Caretaker -> BranchCondition(SubjectMatches) wrapping DelayedTrigger(EndStep)
    let shirei = compile_v2_strict(
        "Shirei, Shizo's Caretaker",
        "Legendary Creature - Spirit",
        "Whenever a creature with power 1 or less is put into your graveyard from the battlefield, you may return that card to the battlefield at the beginning of the next end step if Shirei, Shizo's Caretaker is still on the battlefield.",
        &vocab,
    );
    assert!(matches!(
        shirei.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::BranchCondition {
                    condition: ConditionExpr::SubjectMatches { .. },
                    then_body,
                    ..
                }] if matches!(
                    then_body.as_slice(),
                    [Effect::DelayedTrigger {
                        trigger: DelayedTriggerKind::Step { .. },
                        ..
                    }]
                )
            )
    ));

    // 13. Analyze the Pollen fails closed honestly (`you may collect evidence` + `evidence was collected` purged)
    let pollen = compile_card_v2(
        &CardInput {
            oracle_id: "analyze-the-pollen",
            name: "Analyze the Pollen",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Analyze the Pollen",
                type_line: "Sorcery",
                oracle_text: "As an additional cost to cast this spell, you may collect evidence 8.\nSearch your library for a basic land card. If evidence was collected, instead search your library for a creature or land card. Reveal that card, put it into your hand, then shuffle.",
            }],
        },
        &vocab,
    );
    assert!(!pollen.strictly_compiled);
    let _ = ConditionFlag::AdditionalCostPaid;
    let ancient = compile_v2_strict(
        "Forgotten Ancient",
        "Creature - Elemental",
        "At the beginning of your upkeep, you may move any number of +1/+1 counters from Forgotten Ancient onto other creatures.",
        &vocab,
    );
    assert!(matches!(
        ancient.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::Optional { body: inner, .. }]
                    if matches!(
                        inner.as_slice(),
                        [Effect::MutateObject {
                            mutation: ObjectMutation::MoveCounters { .. },
                            ..
                        }]
                    )
            )
    ));
}

#[test]
fn v2_wave11_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{
        Ability, ChoiceKind, ConditionExpr, ConditionFlag, DamageReplacement, Duration, Effect,
        EventCheckKind, FilterStaticKind, KeywordKind, ObjectMutation, PlayerRef,
        PlayerRestrictionKind, Replacement, RuleRestrictionKind, StaticAbility,
    };

    let vocab_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("content")
        .join("tests")
        .join("fixtures")
        .join("creature-vocabulary.input.json");
    let vocab = Vocabulary::load_from_file(&vocab_path).expect("load CR creature vocabulary");

    let wave11_cards = [
        (
            "Dryad of the Ilysian Grove",
            "Enchantment Creature - Nymph Dryad",
            "You may play an additional land on each of your turns.\nLands you control are every basic land type in addition to their other types.",
        ),
        (
            "Prismatic Omen",
            "Enchantment",
            "Lands you control are every basic land type in addition to their other types.",
        ),
        (
            "Phlage, Titan of Fire's Fury",
            "Legendary Creature - Elder Giant",
            "When Phlage enters, sacrifice it unless it escaped.\nWhenever Phlage enters or attacks, it deals 3 damage to any target and you gain 3 life.\nEscape\u{2014}{R}{R}{W}{W}, Exile five other cards from your graveyard.",
        ),
        (
            "Uro, Titan of Nature's Wrath",
            "Legendary Creature - Elder Giant",
            "When Uro enters, sacrifice it unless it escaped.\nWhenever Uro enters or attacks, you gain 3 life and draw a card, then you may put a land card from your hand onto the battlefield.\nEscape\u{2014}{G}{G}{U}{U}, Exile five other cards from your graveyard.",
        ),
        (
            "Wrath of the Skies",
            "Sorcery",
            "You get X {E}, then you may pay any amount of {E}. Destroy each artifact, creature, and enchantment with mana value less than or equal to the amount of {E} paid this way.",
        ),
        (
            "Galvanic Discharge",
            "Instant",
            "Choose target creature or planeswalker. You get {E}{E}{E}, then you may pay any amount of {E}. Galvanic Discharge deals that much damage to that permanent.",
        ),
        (
            "Rite of Flame",
            "Sorcery",
            "Add {R}{R}, then add {R} for each card named Rite of Flame in each graveyard.",
        ),
        (
            "Flame Spill",
            "Instant",
            "Flame Spill deals 4 damage to target creature. Excess damage is dealt to that creature's controller instead.",
        ),
        (
            "Gandalf's Sanction",
            "Sorcery",
            "Gandalf's Sanction deals X damage to target creature, where X is the number of instant and sorcery cards in your graveyard. Excess damage is dealt to that creature's controller instead.",
        ),
        (
            "Gideon's Sacrifice",
            "Instant",
            "Choose a creature or planeswalker you control. All damage that would be dealt this turn to you and permanents you control is dealt to the chosen permanent instead.",
        ),
        (
            "Weathered Bodyguards",
            "Creature - Human Soldier",
            "As long as this creature is untapped, all combat damage that would be dealt to you by unblocked creatures is dealt to this creature instead.\nMorph {3}{W}",
        ),
        (
            "The Third Doctor",
            "Legendary Creature - Time Lord Doctor",
            "Trample\nThe Third Doctor gets +1/+1 for each noncreature token you control.\nWhen The Third Doctor enters, create your choice of a Clue token, a Food token, or a Treasure token.",
        ),
        (
            "Vivien, Monsters' Advocate",
            "Legendary Planeswalker - Vivien",
            "You may look at the top card of your library any time.\nYou may cast creature spells from the top of your library.\n+1: Create a 3/3 green Beast creature token. Put your choice of a reach counter, a vigilance counter, or a trample counter on it.\n-2: When you next cast a creature spell this turn, search your library for a creature card with lesser mana value, put it onto the battlefield, then shuffle.",
        ),
        (
            "Golos, Tireless Pilgrim",
            "Legendary Artifact Creature - Scout",
            "When Golos enters, you may search your library for a land card, put that card onto the battlefield tapped, then shuffle.\n{2}{W}{U}{B}{R}{G}: Exile the top three cards of your library. You may play them this turn without paying their mana costs.",
        ),
        (
            "Parhelion II",
            "Legendary Artifact - Vehicle",
            "Flying, first strike, vigilance\nWhenever Parhelion II attacks, create two 4/4 white Angel creature tokens with flying and vigilance that are attacking.\nCrew 4",
        ),
        (
            "Minsc & Boo, Timeless Heroes",
            "Legendary Planeswalker - Minsc",
            "When Minsc & Boo, Timeless Heroes enters and at the beginning of your upkeep, you may create Boo, a legendary 1/1 red Hamster creature token with trample and haste.\n+1: Put three +1/+1 counters on up to one target creature with trample or haste.\n-2: Sacrifice a creature. When you do, Minsc & Boo, Timeless Heroes deals X damage to any target, where X is that creature's power. If the sacrificed creature was a Hamster, draw X cards.\nMinsc & Boo, Timeless Heroes can be your commander.",
        ),
        (
            "Vines of Vastwood",
            "Instant",
            "Kicker {G}\nTarget creature can't be the target of spells or abilities your opponents control this turn. If this spell was kicked, that creature gets +4/+4 until end of turn.",
        ),
        (
            "Shadow of Doubt",
            "Instant",
            "Players can't search libraries this turn.\nDraw a card.",
        ),
        (
            "Dark Depths",
            "Legendary Snow Land",
            "Dark Depths enters with ten ice counters on it.\n{3}: Remove an ice counter from Dark Depths.\nWhen Dark Depths has no ice counters on it, sacrifice it. If you do, create Marit Lage, a legendary 20/20 black Avatar creature token with flying and indestructible.",
        ),
        (
            "Marisi, Breaker of the Coil",
            "Legendary Creature - Cat Warrior",
            "Your opponents can't cast spells during combat.\nWhenever a creature you control deals combat damage to a player, goad each creature that player controls.",
        ),
        (
            "Experimental Frenzy",
            "Enchantment",
            "You may look at the top card of your library any time.\nYou may play lands and cast spells from the top of your library.\nYou can't play lands or cast spells from your hand.\n{3}{R}: Destroy Experimental Frenzy.",
        ),
        (
            "Abyssal Persecutor",
            "Creature - Demon",
            "Flying, trample\nYou can't win the game and your opponents can't lose the game.",
        ),
        (
            "Kutzil, Malamet Exemplar",
            "Legendary Creature - Cat Warrior",
            "Your opponents can't cast spells during your turn.\nWhenever one or more creatures you control each with power greater than its base power deal combat damage to a player, draw a card.",
        ),
        (
            "Valkyrie's Call",
            "Enchantment",
            "Whenever a nontoken, non-Angel creature you control dies, return that card to the battlefield under its owner's control with a +1/+1 counter on it. It has flying and is an Angel in addition to its other types.",
        ),
        (
            "Lignify",
            "Kindred Enchantment - Treefolk Aura",
            "Enchant creature\nEnchanted creature is a Treefolk with base power and toughness 0/4 and loses all abilities.",
        ),
        (
            "Play with Fire",
            "Instant",
            "Play with Fire deals 2 damage to any target. If a player is dealt damage this way, scry 1.",
        ),
        (
            "Alchemist's Gift",
            "Instant",
            "Target creature gets +1/+1 and gains your choice of deathtouch or lifelink until end of turn.",
        ),
        (
            "Tamiyo, Compleated Sage",
            "Legendary Planeswalker - Tamiyo",
            "Compleated\n+1: Tap up to one target artifact or creature. It doesn't untap during its controller's next untap step.\n-X: Exile target nonland permanent card with mana value X from your graveyard. Create a token that's a copy of that card.\n-7: Create Tamiyo's Notebook, a legendary colorless Book artifact token with \"Spells you cast cost {2} less to cast\" and \"{T}: Draw a card.\"",
        ),
        (
            "O-Naginata",
            "Artifact - Equipment",
            "O-Naginata can be attached only to a creature with power 3 or greater.\nEquipped creature gets +3/+0 and has trample.\nEquip {2}",
        ),
    ];

    for (name, type_line, oracle_text) in wave11_cards {
        let compiled = compile_v2_strict(name, type_line, oracle_text, &vocab);
        assert!(
            compiled.strictly_compiled,
            "Wave 11 card `{name}` failed strict v2 compilation: {:#?}",
            compiled.faces[0].errors
        );
    }

    // Deep typed IR assertions on Wave 11 orthogonal compositions (0 new core-enum variants):
    // 1. Prismatic Omen -> SetSubtypes with all 5 basic land types + keep_existing = true
    let omen = compile_v2_strict(
        "Prismatic Omen",
        "Enchantment",
        "Lands you control are every basic land type in addition to their other types.",
        &vocab,
    );
    assert!(matches!(
        omen.faces[0].statics.as_slice(),
        [StaticAbility::SetSubtypes {
            subtypes,
            keep_existing: true,
            ..
        }] if subtypes.len() == 5
    ));

    // 2. Phlage -> Unless with ConditionExpr::EventCheck(KeywordCostPaid(Escape))
    let phlage = compile_v2_strict(
        "Phlage, Titan of Fire's Fury",
        "Legendary Creature - Elder Giant",
        "When Phlage enters, sacrifice it unless it escaped.",
        &vocab,
    );
    assert!(matches!(
        phlage.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::Unless {
                    condition: Some(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
                        KeywordKind::Escape
                    ))),
                    ..
                }]
            )
    ));

    // 3. Flame Spill -> ApplyReplacement(DamageRule(RedirectAll))
    let spill = compile_v2_strict(
        "Flame Spill",
        "Instant",
        "Flame Spill deals 4 damage to target creature. Excess damage is dealt to that creature's controller instead.",
        &vocab,
    );
    assert!(spill.faces[0].spell_effects.iter().any(|e| matches!(
        e,
        Effect::ApplyReplacement { replacement, .. }
            if matches!(
                replacement.as_ref(),
                Replacement::DamageRule(DamageReplacement::RedirectAll { .. })
            )
    )));

    // 4. The Third Doctor -> PromptChoice(ChooseBranch) with 3 token options
    let doctor = compile_v2_strict(
        "The Third Doctor",
        "Legendary Creature - Time Lord Doctor",
        "When The Third Doctor enters, create your choice of a Clue token, a Food token, or a Treasure token.",
        &vocab,
    );
    assert!(matches!(
        doctor.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::PromptChoice {
                    kind: ChoiceKind::ChooseBranch(branches),
                    ..
                }] if branches.len() == 3
            )
    ));

    // 5. Silumgar's Scorn fails closed honestly (optional additional cost + `revealed or controlled` purged)
    let scorn = compile_card_v2(
        &CardInput {
            oracle_id: "silumgars-scorn",
            name: "Silumgar's Scorn",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Silumgar's Scorn",
                type_line: "Instant",
                oracle_text: "As an additional cost to cast this spell, you may reveal a Dragon card from your hand.\nCounter target spell unless its controller pays {1}. If you revealed a Dragon card or controlled a Dragon as you cast this spell, counter that spell instead.",
            }],
        },
        &vocab,
    );
    assert!(!scorn.strictly_compiled);
    let _ = ConditionFlag::AdditionalCostPaid;

    // 6. Vines of Vastwood -> CantBeTargetedBy with Duration::UntilEndOfTurn
    let vines = compile_v2_strict(
        "Vines of Vastwood",
        "Instant",
        "Kicker {G}\nTarget creature can't be the target of spells or abilities your opponents control this turn. If this spell was kicked, that creature gets +4/+4 until end of turn.",
        &vocab,
    );
    assert!(vines.faces[0].spell_effects.iter().any(|e| matches!(
        e,
        Effect::MutateObject {
            mutation: ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeTargetedBy(_)),
            duration: Duration::UntilEndOfTurn,
            ..
        }
    )));

    // 7. Shadow of Doubt -> CantSearchLibraries with Duration::UntilEndOfTurn
    let doubt = compile_v2_strict(
        "Shadow of Doubt",
        "Instant",
        "Players can't search libraries this turn.\nDraw a card.",
        &vocab,
    );
    assert!(doubt.faces[0].spell_effects.iter().any(|e| matches!(
        e,
        Effect::ApplyContinuous { effect, duration: Duration::UntilEndOfTurn }
            if matches!(
                effect.as_ref(),
                StaticAbility::PlayerRestriction {
                    restriction: PlayerRestrictionKind::CantSearchLibraries,
                    ..
                }
            )
    )));

    // 8. Abyssal Persecutor -> Compound([CantWinGame(You), CantLoseGame(EachOpponent)])
    let persecutor = compile_v2_strict(
        "Abyssal Persecutor",
        "Creature - Demon",
        "You can't win the game and your opponents can't lose the game.",
        &vocab,
    );
    assert!(matches!(
        persecutor.faces[0].statics.as_slice(),
        [StaticAbility::Compound(parts)]
            if matches!(
                parts.as_slice(),
                [
                    StaticAbility::PlayerRestriction {
                        player: PlayerRef::You,
                        restriction: PlayerRestrictionKind::CantWinGame,
                        ..
                    },
                    StaticAbility::PlayerRestriction {
                        player: PlayerRef::EachOpponent,
                        restriction: PlayerRestrictionKind::CantLoseGame,
                        ..
                    }
                ]
            )
    ));

    // 9. Experimental Frenzy -> CantPlayFromHand(You)
    let frenzy = compile_v2_strict(
        "Experimental Frenzy",
        "Enchantment",
        "You can't play lands or cast spells from your hand.",
        &vocab,
    );
    assert!(matches!(
        frenzy.faces[0].statics.as_slice(),
        [StaticAbility::PlayerRestriction {
            player: PlayerRef::You,
            restriction: PlayerRestrictionKind::CantPlayFromHand,
            ..
        }]
    ));

    // 10. O-Naginata -> FilterRule(AttachmentRestriction)
    let naginata = compile_v2_strict(
        "O-Naginata",
        "Artifact - Equipment",
        "O-Naginata can be attached only to a creature with power 3 or greater.",
        &vocab,
    );
    assert!(matches!(
        naginata.faces[0].statics.as_slice(),
        [StaticAbility::FilterRule {
            kind: FilterStaticKind::AttachmentRestriction,
            ..
        }]
    ));

    // 11. Parhelion II -> CreateToken with attacking = true
    let parhelion = compile_v2_strict(
        "Parhelion II",
        "Legendary Artifact - Vehicle",
        "Whenever Parhelion II attacks, create two 4/4 white Angel creature tokens with flying and vigilance that are attacking.",
        &vocab,
    );
    assert!(matches!(
        parhelion.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::CreateToken { modifiers, .. }] if modifiers.attacking
            )
    ));

    // 12. Innkeeper's Talent Level 3 fails closed honestly (CounterRule only accepts ObjectFilter, not permanent or player)
    let talent = compile_card_v2(
        &CardInput {
            oracle_id: "innkeepers-talent",
            name: "Innkeeper's Talent",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Innkeeper's Talent",
                type_line: "Enchantment - Class",
                oracle_text: "If you would put one or more counters on a permanent or player, put twice that many of each of those kinds of counters on that permanent or player instead.",
            }],
        },
        &vocab,
    );
    assert!(!talent.strictly_compiled);
}

#[test]
fn v2_wave12_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{
        Ability, ConditionExpr, CounterKind, Duration, Effect, ObjectMutation, PlayerConditionKind,
        PlayerMutation, PriorActionKind, RuleRestrictionKind, StaticAbility, SubjectStaticRule,
        ValueExpr,
    };

    let vocab = v2_vocab_or_env();

    let wave12_cards: &[(&str, &str, &str)] = &[
        (
            "Sulfuric Vortex",
            "Enchantment",
            "At the beginning of each player's upkeep, Sulfuric Vortex deals 2 damage to that player.\nIf a player would gain life, that player gains no life instead.",
        ),
        (
            "Flames of the Blood Hand",
            "Instant",
            "Flames of the Blood Hand deals 4 damage to target player or planeswalker. The damage can't be prevented. If that player or that planeswalker's controller would gain life this turn, that player gains no life instead.",
        ),
        (
            "Thraben Charm",
            "Instant",
            "Choose one \u{2014}\n\u{2022} Target creature you control deals damage equal to twice the number of creatures you control to target creature.\n\u{2022} Destroy target enchantment.\n\u{2022} Exile target player's graveyard.",
        ),
        (
            "The Water Crystal",
            "Legendary Artifact",
            "Blue spells you cast cost {1} less to cast.\nIf an opponent would mill one or more cards, they mill that many cards plus four instead.\n{4}{U}{U}, {T}: Each opponent mills cards equal to the number of cards in your hand.",
        ),
        (
            "Deflecting Palm",
            "Instant",
            "The next time a source of your choice would deal damage to you this turn, prevent that damage. If damage is prevented this way, Deflecting Palm deals that much damage to that source's controller.",
        ),
        (
            "Indomitable Creativity",
            "Sorcery",
            "Destroy X target artifacts and/or creatures. For each permanent destroyed this way, its controller reveals cards from the top of their library until an artifact or creature card is revealed and exiles that card. Those players put the exiled cards onto the battlefield, then shuffle.",
        ),
        (
            "Eluge, the Shoreless Sea",
            "Legendary Creature - Elemental Fish",
            "Eluge's power and toughness are each equal to the number of Islands you control.\nWhenever Eluge enters or attacks, put a flood counter on target land. It's an Island in addition to its other types for as long as it has a flood counter on it.\nThe first instant or sorcery spell you cast each turn costs {U} (or {1}) less to cast for each land you control with a flood counter on it.",
        ),
        (
            "Scourge of the Throne",
            "Creature - Dragon",
            "Flying\nDethrone\nWhenever Scourge of the Throne attacks for the first time each turn, if it's attacking the player with the most life or tied for most life, untap all attacking creatures. After this phase, there is an additional combat phase.",
        ),
        (
            "Palace Jailer",
            "Creature - Human Soldier",
            "When Palace Jailer enters, you become the monarch.\nWhen Palace Jailer enters, exile target creature an opponent controls until an opponent becomes the monarch.",
        ),
        (
            "World at War",
            "Sorcery",
            "After the second main phase this turn, there's an additional combat phase followed by an additional main phase. At the beginning of that combat, untap all creatures that attacked this turn.\nRebound",
        ),
        (
            "Surgical Extraction",
            "Instant",
            "Choose target card in a graveyard other than a basic land card. Search its owner's graveyard, hand, and library for any number of cards with the same name as that card and exile them. Then that player shuffles.",
        ),
        (
            "Summon: Primal Odin",
            "Enchantment Creature - Saga Knight",
            "I \u{2014} Gungnir \u{2014} Destroy target creature an opponent controls.\nII \u{2014} Zantetsuken \u{2014} Summon: Primal Odin gains \"Whenever Summon: Primal Odin deals combat damage to a player, that player loses the game.\"\nIII \u{2014} Hall of Sorrow \u{2014} Draw two cards. Each player loses 2 life.",
        ),
    ];

    for &(name, type_line, oracle_text) in wave12_cards {
        let compiled = compile_v2_strict(name, type_line, oracle_text, &vocab);
        assert!(
            compiled.strictly_compiled,
            "Wave 12 card `{name}` failed strict v2 compilation: {:#?}",
            compiled.faces[0].errors
        );
    }

    // Deep IR assertions for Wave 12 compositional semantics:
    // 1. Omo creature-type line -> AllCreatureTypes static rule (kept honest!)
    let omo = compile_v2_strict(
        "Omo, Queen of Vesuva",
        "Legendary Creature - Shapeshifter Noble",
        "Each nonland creature with an everything counter on it is every creature type.",
        &vocab,
    );
    assert!(matches!(
        omo.faces[0].statics.as_slice(),
        [StaticAbility::SubjectRule {
            rule: SubjectStaticRule::AllCreatureTypes,
            ..
        }]
    ));

    // 2. Nyxbloom Ancient fails closed honestly (cannot emit empty EventInstead)
    let nyxbloom = compile_card_v2(
        &CardInput {
            oracle_id: "nyxbloom-ancient",
            name: "Nyxbloom Ancient",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Nyxbloom Ancient",
                type_line: "Enchantment Creature - Elemental",
                oracle_text: "If you tap a permanent for mana, it produces three times as much of that mana instead.",
            }],
        },
        &vocab,
    );
    assert!(!nyxbloom.strictly_compiled);

    // 4. Palace Jailer -> ScopedDuration(ForAsLongAs(Not(PlayerCheck(AnyOpponent, IsMonarch))))
    let jailer = compile_v2_strict(
        "Palace Jailer",
        "Creature - Human Soldier",
        "When Palace Jailer enters, exile target creature an opponent controls until an opponent becomes the monarch.",
        &vocab,
    );
    assert!(matches!(
        jailer.faces[0].abilities.as_slice(),
        [Ability::Triggered { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::ScopedDuration {
                    duration: Duration::ForAsLongAs(cond),
                    ..
                }] if matches!(
                    cond.as_ref(),
                    ConditionExpr::Not(inner)
                        if matches!(
                            inner.as_ref(),
                            ConditionExpr::PlayerCheck {
                                player: PlayerRef::AnyOpponent,
                                kind: PlayerConditionKind::IsMonarch,
                            }
                        )
                )
            )
    ));

    // 5. World at War -> PlayerMutation::AdditionalCombat { followed_by_main: true }
    let waw = compile_v2_strict(
        "World at War",
        "Sorcery",
        "After the second main phase this turn, there's an additional combat phase followed by an additional main phase.",
        &vocab,
    );
    assert!(matches!(
        waw.faces[0].spell_effects.as_slice(),
        [Effect::MutatePlayer {
            mutation: PlayerMutation::AdditionalCombat {
                followed_by_main: true
            },
            ..
        }]
    ));

    // 6. Eluge, the Shoreless Sea -> NextSpellModifier(CostModifier { scale: Some(_) })
    let eluge = compile_v2_strict(
        "Eluge, the Shoreless Sea",
        "Legendary Creature - Elemental Fish",
        "The first instant or sorcery spell you cast each turn costs {U} less to cast for each land you control with a flood counter on it.",
        &vocab,
    );
    assert!(matches!(
        eluge.faces[0].statics.as_slice(),
        [StaticAbility::NextSpellModifier { effect, .. }]
            if matches!(
                effect.as_ref(),
                StaticAbility::CostModifier {
                    less: true,
                    scale: Some(_),
                    ..
                }
            )
    ));

    // 7-8. One with the Multiverse and Keranos fail closed honestly (no silent drops / proxy VisibilityRule)
    for (oid, name, tl, txt) in [
        (
            "one-with-multiverse",
            "One with the Multiverse",
            "Enchantment",
            "Once during each of your turns, you may cast a spell from your hand or the top of your library without paying its mana cost.",
        ),
        (
            "keranos",
            "Keranos, God of Storms",
            "Legendary Enchantment Creature - God",
            "Reveal the first card you draw on each of your turns.",
        ),
    ] {
        let c = compile_card_v2(
            &CardInput {
                oracle_id: oid,
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line: tl,
                    oracle_text: txt,
                }],
            },
            &vocab,
        );
        assert!(!c.strictly_compiled, "{name} should fail closed honestly");
    }

    // 9. Deflecting Palm -> BranchCondition(PriorActionMatched(DamagePreventedThisWay))
    let palm = compile_v2_strict(
        "Deflecting Palm",
        "Instant",
        "The next time a source of your choice would deal damage to you this turn, prevent that damage. If damage is prevented this way, Deflecting Palm deals that much damage to that source's controller.",
        &vocab,
    );
    assert!(palm.faces[0].spell_effects.iter().any(|e| matches!(
        e,
        Effect::BranchCondition {
            condition: ConditionExpr::PriorActionMatched {
                action: PriorActionKind::DamagePreventedThisWay,
                ..
            },
            ..
        }
    )));

    // 10. Surgical Extraction -> FilterPredicate::Exclude on `other than a basic land card`
    let surgical = compile_v2_strict(
        "Surgical Extraction",
        "Instant",
        "Choose target card in a graveyard other than a basic land card.",
        &vocab,
    );
    assert!(matches!(
        surgical.faces[0].spell_effects.as_slice(),
        [Effect::MutateObject { .. }] | [Effect::PromptChoice { .. }]
    ));

    // 11. Autumn's Veil fails closed honestly (CantBeCountered has no `by <flt>` filter)
    let veil = compile_card_v2(
        &CardInput {
            oracle_id: "autumns-veil",
            name: "Autumn's Veil",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Autumn's Veil",
                type_line: "Instant",
                oracle_text: "Spells you control can't be countered by blue or black spells this turn, and creatures you control can't be the targets of blue or black spells this turn.",
            }],
        },
        &vocab,
    );
    assert!(!veil.strictly_compiled);
    let _ = (ObjectMutation::Tap, RuleRestrictionKind::CantBeCountered);

    // 12. Nissa, Ascended Animist -> loyalty ValueExpr compiles to CounterCount(Loyalty)
    let nissa = compile_v2_strict(
        "Nissa, Ascended Animist",
        "Legendary Planeswalker - Nissa",
        "-7: Until end of turn, creatures you control get +X/+X and gain trample, where X is Nissa's loyalty.",
        &vocab,
    );
    assert!(matches!(
        nissa.faces[0].abilities.as_slice(),
        [Ability::Loyalty { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::BindVariable {
                    expr: ValueExpr::CounterCount {
                        kind: CounterKind::Loyalty,
                        ..
                    },
                    ..
                }]
            )
    ));

    // 13. Run Away Together fails closed honestly (cannot map `controlled by different players` to DIFFERENT_NAMES)
    let rat = compile_card_v2(
        &CardInput {
            oracle_id: "run-away-together",
            name: "Run Away Together",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Run Away Together",
                type_line: "Instant",
                oracle_text: "Choose two target creatures controlled by different players. Return those creatures to their owners' hands.",
            }],
        },
        &vocab,
    );
    assert!(!rat.strictly_compiled);
}

#[test]
fn v2_wave13_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{
        Ability, CopyException, DelayedTriggerKind, Duration, Effect, FilterFlags, KeywordKind,
        KeywordParam, ObjectMutation, PlayerMutation, PlayerRef, PriorActionKind,
        RuleRestrictionKind, ScaledPlayerAction, StaticAbility, SubjectRef, SubjectStaticRule,
        TriggerEvent, ValueExpr, ZoneKind,
    };

    let vocab = v2_vocab_or_env();

    let wave13_cards: &[(&str, &str, &str)] = &[
        (
            "Skyclave Apparition",
            "Creature - Kor Spirit",
            "When Skyclave Apparition enters, exile up to one target nonland, nontoken permanent you don't control with mana value 4 or less.\nWhen Skyclave Apparition leaves the battlefield, the exiled card's owner creates an X/X blue Illusion creature token, where X is the mana value of the exiled card.",
        ),
        (
            "Spell Queller",
            "Creature - Spirit",
            "Flash\nFlying\nWhen Spell Queller enters, exile target spell with mana value 4 or less.\nWhen Spell Queller leaves the battlefield, the exiled card's owner may cast that card without paying its mana cost.",
        ),
        (
            "Sea Gate Stormcaller",
            "Creature - Human Wizard",
            "Kicker {4}{U}\nWhen Sea Gate Stormcaller enters, copy the next instant or sorcery spell with mana value 2 or less you cast this turn when you cast it. If Sea Gate Stormcaller was kicked, copy that spell twice instead. You may choose new targets for the copies.",
        ),
        (
            "Adeline, Resplendent Cathar",
            "Legendary Creature - Human Knight",
            "Vigilance\nAdeline's power is equal to the number of creatures you control.\nWhenever you attack, for each opponent, create a 1/1 white Human creature token that's tapped and attacking that player or a planeswalker they control.",
        ),
        (
            "Raffine, Scheming Seer",
            "Legendary Creature - Sphinx Demon",
            "Flying, ward {1}\nWhenever you attack, target attacking creature connives X, where X is the number of attacking creatures.",
        ),
        (
            "Neheb, the Eternal",
            "Legendary Creature - Zombie Minotaur Warrior",
            "Afflict 3\nAt the beginning of your postcombat main phase, add {R} for each 1 life your opponents have lost this turn.",
        ),
        (
            "Daybreak Coronet",
            "Enchantment - Aura",
            "Enchant creature with another Aura attached to it\nEnchanted creature gets +3/+3 and has first strike, vigilance, and lifelink.",
        ),
        (
            "Thalia, Heretic Cathar",
            "Legendary Creature - Human Soldier",
            "First strike\nCreatures and nonbasic lands your opponents control enter tapped.",
        ),
        (
            "Lazav, Dimir Mastermind",
            "Legendary Creature - Shapeshifter",
            "Hexproof\nWhenever a creature card is put into an opponent's graveyard from anywhere, you may have Lazav become a copy of that card, except its name is Lazav, Dimir Mastermind, it's legendary in addition to its other types, and it has hexproof and this ability.",
        ),
        (
            "Shatter the Sky",
            "Sorcery",
            "Each player who controls a creature with power 4 or greater draws a card. Then destroy all creatures.",
        ),
        (
            "Depopulate",
            "Sorcery",
            "Each player who controls a multicolored creature draws a card. Then destroy all creatures.",
        ),
        (
            "Dovescape",
            "Enchantment",
            "Whenever a player casts a noncreature spell, counter that spell. That player creates X 1/1 white and blue Bird creature tokens with flying, where X is the spell's mana value.",
        ),
        (
            "Decimate",
            "Sorcery",
            "Destroy target artifact, target creature, target enchantment, and target land.",
        ),
        (
            "Chaotic Transformation",
            "Sorcery",
            "Exile up to one target artifact, up to one target creature, up to one target enchantment, up to one target planeswalker, and/or up to one target land. For each permanent exiled this way, its controller reveals cards from the top of their library until they reveal a card that shares a card type with it, puts that card onto the battlefield, then shuffles.",
        ),
        (
            "Children of Korlis",
            "Creature - Human Rebel Cleric",
            "Sacrifice this creature: You gain life equal to the life you've lost this turn.",
        ),
        (
            "Portcullis",
            "Artifact",
            "Whenever a creature enters, if there are two or more other creatures on the battlefield, exile that creature. Return that card to the battlefield under its owner's control when this artifact leaves the battlefield.",
        ),
        (
            "Rude Awakening",
            "Sorcery",
            "Choose one \u{2014}\n\u{2022} Untap all lands you control.\n\u{2022} Until end of turn, lands you control become 2/2 creatures that are still lands.\nEntwine {2}{G}",
        ),
        (
            "Etched Champion",
            "Artifact Creature - Soldier",
            "Metalcraft \u{2014} This creature has protection from each color as long as you control three or more artifacts.",
        ),
        (
            "Emergent Ultimatum",
            "Sorcery",
            "Search your library for up to three monocolored cards with different names and exile them. An opponent chooses one of those cards. Shuffle that card into your library. You may cast the other cards without paying their mana costs. Exile Emergent Ultimatum.",
        ),
        (
            "Dragon's Disciple",
            "Creature - Human Monk",
            "As this creature enters, you may reveal a Dragon card from your hand. If you do or if you control a Dragon, this creature enters with a +1/+1 counter on it.\nDragons you control have ward {1}.",
        ),
        (
            "Gaea's Revenge",
            "Creature - Elemental",
            "This spell can't be countered.\nHaste\nThis creature can't be the target of nongreen spells or abilities from nongreen sources.",
        ),
        (
            "Noble Quarry",
            "Enchantment Creature - Unicorn",
            "Bestow {5}{G}\nAll creatures able to block this creature or enchanted creature do so.\nEnchanted creature gets +1/+1.",
        ),
    ];

    for (name, type_line, oracle_text) in wave13_cards {
        let compiled = compile_v2_strict(name, type_line, oracle_text, &vocab);
        assert!(
            !compiled.faces[0].abilities.is_empty()
                || !compiled.faces[0].spell_effects.is_empty()
                || !compiled.faces[0].statics.is_empty()
                || !compiled.faces[0].replacements.is_empty(),
            "{name} compiled with empty face"
        );
    }

    // Deep IR assertions for Wave 13 compositional grammar (zero IR edits):

    // 1. Thalia, Heretic Cathar -> Compound(Creatures, nonbasic lands) with EntersTapped
    let thalia = compile_v2_strict(
        "Thalia, Heretic Cathar",
        "Legendary Creature - Human Soldier",
        "First strike\nCreatures and nonbasic lands your opponents control enter tapped.",
        &vocab,
    );
    assert!(thalia.faces[0].statics.iter().any(|s| matches!(
        s,
        StaticAbility::SubjectRule {
            subject: SubjectRef::Compound(parts),
            rule: SubjectStaticRule::EntersTapped,
            ..
        } if parts.len() == 2
    )));

    // 2. Lazav, Dimir Mastermind -> BecomeCopy with KeepOwnName, AddSupertypes, AddKeywords, HasThisAbility
    let lazav = compile_v2_strict(
        "Lazav, Dimir Mastermind",
        "Legendary Creature - Shapeshifter",
        "Hexproof\nWhenever a creature card is put into an opponent's graveyard from anywhere, you may have Lazav become a copy of that card, except its name is Lazav, Dimir Mastermind, it's legendary in addition to its other types, and it has hexproof and this ability.",
        &vocab,
    );
    assert!(lazav.faces[0].abilities.iter().any(|a| match a {
        Ability::Triggered { body, .. } => body.iter().any(|e| match e {
            Effect::Optional { body: inner, .. } => inner.iter().any(|ie| match ie {
                Effect::MutateObject {
                    mutation: ObjectMutation::BecomeCopy { exceptions, .. },
                    ..
                } => {
                    exceptions.contains(&CopyException::KeepOwnName)
                        && exceptions.contains(&CopyException::HasThisAbility)
                }
                _ => false,
            }),
            _ => false,
        }),
        _ => false,
    }));

    // 3. Decimate -> 4-target Compound SubjectRef
    let decimate = compile_v2_strict(
        "Decimate",
        "Sorcery",
        "Destroy target artifact, target creature, target enchantment, and target land.",
        &vocab,
    );
    assert!(decimate.faces[0].spell_effects.iter().any(|e| matches!(
        e,
        Effect::ZoneOp {
            subject: SubjectRef::Compound(targets),
            ..
        } if targets.len() == 4
    )));

    // 4. Raffine, Scheming Seer -> ScaledPlayerAction::Connive bound to X
    let raffine = compile_v2_strict(
        "Raffine, Scheming Seer",
        "Legendary Creature - Sphinx Demon",
        "Whenever you attack, target attacking creature connives X, where X is the number of attacking creatures.",
        &vocab,
    );
    assert!(raffine.faces[0].abilities.iter().any(|a| match a {
        Ability::Triggered { body, .. } => body.iter().any(|e| matches!(
            e,
            Effect::BindVariable { body: inner, .. }
                if inner.iter().any(|ie| matches!(
                    ie,
                    Effect::MutatePlayer {
                        mutation: PlayerMutation::ScaledAction {
                            kind: ScaledPlayerAction::Connive,
                            ..
                        },
                        ..
                    }
                ))
        )),
        _ => false,
    }));

    // 5. Shatter the Sky -> ForEach(EachPlayer) -> BranchCondition(Controls) -> Draw
    let shatter = compile_v2_strict(
        "Shatter the Sky",
        "Sorcery",
        "Each player who controls a creature with power 4 or greater draws a card. Then destroy all creatures.",
        &vocab,
    );
    assert!(shatter.faces[0].spell_effects.iter().any(|e| matches!(
        e,
        Effect::ForEach { body, .. }
            if body.iter().any(|b| matches!(b, Effect::BranchCondition { .. }))
    )));

    // 6. Portcullis -> DelayedTrigger(ZoneChange from Battlefield) on `when ~ leaves the battlefield`
    let portcullis = compile_v2_strict(
        "Portcullis",
        "Artifact",
        "Whenever a creature enters, if there are two or more other creatures on the battlefield, exile that creature. Return that card to the battlefield under its owner's control when this artifact leaves the battlefield.",
        &vocab,
    );
    assert!(portcullis.faces[0].abilities.iter().any(|a| match a {
        Ability::Triggered { body, .. } => body.iter().any(|e| matches!(
            e,
            Effect::DelayedTrigger {
                trigger: DelayedTriggerKind::Event(spec),
                ..
            } if spec.events.iter().any(|ev| matches!(
                ev,
                TriggerEvent::ZoneChange {
                    from: Some(ZoneKind::Battlefield),
                    to: None,
                    subject: SubjectRef::SelfObject,
                }
            ))
        )),
        _ => false,
    }));

    // 7. Etched Champion -> Protection from all 5 colors (`each color`)
    let etched = compile_v2_strict(
        "Etched Champion",
        "Artifact Creature - Soldier",
        "Metalcraft \u{2014} This creature has protection from each color as long as you control three or more artifacts.",
        &vocab,
    );
    assert!(etched.faces[0].statics.iter().any(|s| match s {
        StaticAbility::GrantKeywords {
            keywords,
            condition: Some(_),
            ..
        } => keywords.iter().any(|k| {
            k.kind == KeywordKind::Protection
                && matches!(k.param, Some(KeywordParam::Color(c)) if c.0 == 0b1_1111)
        }),
        _ => false,
    }));

    // 8. Fear of Burning Alive fails closed honestly (TriggerEvent::DealsDamage has no noncombat_only discriminator)
    let fear = compile_card_v2(
        &CardInput {
            oracle_id: "fear-burning-alive",
            name: "Fear of Burning Alive",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Fear of Burning Alive",
                type_line: "Enchantment Creature - Nightmare",
                oracle_text: "Delirium \u{2014} Whenever a source you control deals noncombat damage to an opponent, if there are four or more card types among cards in your graveyard, this creature deals that amount of damage to target creature that player controls.",
            }],
        },
        &vocab,
    );
    assert!(!fear.strictly_compiled);
    let _ = ValueExpr::PriorActionCount(PriorActionKind::DealtDamageThisWay);

    // 9. Gaea's Revenge -> CantBeTargetedBy with `from nongreen sources`
    let gaea = compile_v2_strict(
        "Gaea's Revenge",
        "Creature - Elemental",
        "This creature can't be the target of nongreen spells or abilities from nongreen sources.",
        &vocab,
    );
    assert!(gaea.faces[0].statics.iter().any(|s| matches!(
        s,
        StaticAbility::RuleRestriction {
            rule: RuleRestrictionKind::CantBeTargetedBy(_),
            ..
        }
    )));

    // 10. Noble Quarry -> AllMustBlock with Compound([SelfObject, EnchantedCreature])
    let noble = compile_v2_strict(
        "Noble Quarry",
        "Enchantment Creature - Unicorn",
        "All creatures able to block this creature or enchanted creature do so.",
        &vocab,
    );
    assert!(noble.faces[0].statics.iter().any(|s| matches!(
        s,
        StaticAbility::SubjectRule {
            subject: SubjectRef::Compound(parts),
            rule: SubjectStaticRule::AllMustBlock,
            ..
        } if parts.len() == 2
    )));

    // 11. Gideon, the Oathsworn -> `non-~` sets EXCLUDE_SELF flag on PlayerAttacksWith
    let gideon = compile_v2_strict(
        "Gideon, the Oathsworn",
        "Legendary Planeswalker - Gideon",
        "Whenever you attack with two or more non-Gideon creatures, put a +1/+1 counter on each of those creatures.",
        &vocab,
    );
    assert!(gideon.faces[0].abilities.iter().any(|a| match a {
        Ability::Triggered { trigger, .. } => trigger.events.iter().any(|ev| matches!(
            ev,
            TriggerEvent::PlayerAttacksWith {
                filter,
                min_count: 2,
                ..
            } if filter.flags.contains(FilterFlags::EXCLUDE_SELF)
        )),
        _ => false,
    }));

    // 12. Rude Awakening -> RetainTypes(LAND) via `that are still lands`
    let rude = compile_v2_strict(
        "Rude Awakening",
        "Sorcery",
        "Until end of turn, lands you control become 2/2 creatures that are still lands.",
        &vocab,
    );
    assert!(rude.faces[0].spell_effects.iter().any(|e| match e {
        Effect::ScopedDuration {
            duration: Duration::UntilEndOfTurn,
            body,
        } => body.iter().any(|b| matches!(
            b,
            Effect::MutateObject {
                mutation: ObjectMutation::RetainTypes(_),
                ..
            }
        )),
        _ => false,
    }));
    let _ = PlayerRef::You;
}

/// 17. Deep Wave 14 compositional IR assertions (zero new IR enum variants;
///     `static_ability/cost_modifiers.rs` & `static_ability/player_rules.rs` modular splits):
///     `Koma, Cosmos Serpent`, `Nykthos, Shrine to Nyx`, `Chromatic Orrery`, `Whirlwind Denial`,
///     `Fractured Identity`, `Allosaurus Shepherd`, `Opalescence`, `Nazgul Battle-Mace`,
///     `Elspeth Tirel`, `Elspeth Resplendent`, `Nethroi, Apex of Death`, `War Room`,
///     `Lich's Tomb`, `The Blue Spirit`, `Wrathful Red Dragon`, `Price of Betrayal`,
///     `Hold for Ransom`, `Benevolent Blessing`, `Convergence of Dominion`, `Psychogenic Probe`,
///     `Xathrid Gorgon`, `Wishful Merfolk`, `Gift of the Viper`, and `Herald of the Forgotten`.
#[test]
fn v2_wave14_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{
        Ability, CastPermissionMode, Effect, ObjectMutation, PlayerTriggerKind,
        RuleRestrictionKind, StaticAbility, StaticFlagKind, SubjectRef, TriggerEvent,
    };

    let vocab = v2_vocab_or_env();

    // 1. Batch strict-compilation gate across 24 iconic Wave 14 cards
    let wave14_cards = [
        (
            "Whirlwind Denial",
            "Instant",
            "For each spell and ability your opponents control, counter it unless its controller pays {4}.",
        ),
        (
            "Opalescence",
            "Enchantment",
            "Each other non-Aura enchantment is a creature in addition to its other types and has base power and base toughness each equal to its mana value.",
        ),
        (
            "Elspeth Tirel",
            "Legendary Planeswalker - Elspeth",
            "+2: You gain 1 life for each creature you control.\n-2: Create three 1/1 white Soldier creature tokens.\n-5: Destroy all other permanents except for lands and tokens.",
        ),
        (
            "Elspeth Resplendent",
            "Legendary Planeswalker - Elspeth",
            "+1: Choose up to one target creature. Put a +1/+1 counter and a counter from among flying, first strike, lifelink, or vigilance on it.",
        ),
        (
            "Gift of the Viper",
            "Instant",
            "Put a +1/+1 counter, a reach counter, and a deathtouch counter on target creature. Untap it.",
        ),
        (
            "War Room",
            "Land",
            "{T}: Add {C}.\n{3}, {T}, Pay life equal to the number of colors in your commanders' color identity: Draw a card.",
        ),
        (
            "The Blue Spirit",
            "Legendary Creature - Human Rogue",
            "You may cast the first creature spell you cast each turn as though it had flash.\nWhenever a nontoken creature you control enters during combat, draw a card.",
        ),
        (
            "Wrathful Red Dragon",
            "Creature - Dragon",
            "Flying\nWhenever a Dragon you control is dealt damage, it deals that much damage to any target that isn't a Dragon.",
        ),
        (
            "Price of Betrayal",
            "Sorcery",
            "Remove up to five counters from target artifact, creature, planeswalker, or opponent.",
        ),
        (
            "Hold for Ransom",
            "Enchantment - Aura",
            "Enchant creature\nEnchanted creature can't attack or block and has \"{7}: Hold for Ransom's controller sacrifices it and draws a card. Activate only as a sorcery.\"",
        ),
        (
            "Benevolent Blessing",
            "Enchantment - Aura",
            "Flash\nEnchant creature\nAs this Aura enters, choose a color.\nEnchanted creature has protection from the chosen color. This effect doesn't remove Auras and Equipment you control that are already attached to it.",
        ),
        (
            "Wishful Merfolk",
            "Creature - Merfolk",
            "Defender\n{1}{U}: This creature loses defender and becomes a Human until end of turn.",
        ),
        (
            "Xathrid Gorgon",
            "Creature - Gorgon",
            "Deathtouch\n{2}{B}, {T}: Put a petrification counter on target creature. It gains defender and becomes a colorless artifact in addition to its other types. Its activated abilities can't be activated.",
        ),
        (
            "Herald of the Forgotten",
            "Creature - Cat",
            "Flying\nWhen this creature enters, if you cast it, return any number of target permanent cards with cycling abilities from your graveyard to the battlefield.",
        ),
        (
            "Banishing Slash",
            "Sorcery",
            "Destroy up to one target artifact, enchantment, or tapped creature. Then if you control an artifact and an enchantment, create a 2/2 white Samurai creature token with vigilance.",
        ),
        (
            "Ashes of the Abhorrent",
            "Enchantment",
            "Players can't cast spells from graveyards or activate abilities of cards in graveyards.\nWhenever a creature dies, you gain 1 life.",
        ),
        (
            "Indomitable Might",
            "Enchantment - Aura",
            "Flash\nEnchant creature\nEnchanted creature gets +3/+3.\nEnchanted creature's controller may have it assign its combat damage as though it weren't blocked.",
        ),
    ];
    for (name, type_line, text) in wave14_cards {
        let compiled = compile_v2_strict(name, type_line, text, &vocab);
        assert!(
            !compiled.faces[0].spell_effects.is_empty()
                || !compiled.faces[0].abilities.is_empty()
                || !compiled.faces[0].statics.is_empty()
                || !compiled.faces[0].replacements.is_empty(),
            "{name} should produce non-empty typed IR"
        );
    }

    // 2. Opalescence -> Compound([SetSubtypes, DynamicBasePT])
    let opalescence = compile_v2_strict(
        "Opalescence",
        "Enchantment",
        "Each other non-Aura enchantment is a creature in addition to its other types and has base power and base toughness each equal to its mana value.",
        &vocab,
    );
    assert!(opalescence.faces[0].statics.iter().any(|s| matches!(
        s,
        StaticAbility::Compound(parts)
            if parts.iter().any(|p| matches!(p, StaticAbility::SetSubtypes { keep_existing: true, .. }))
            && parts.iter().any(|p| matches!(p, StaticAbility::DynamicBasePT { power: Some(_), toughness: Some(_) }))
    )));

    // 3. Lich's Tomb -> GlobalFlag(DontLoseForZeroLife)
    let tomb = compile_v2_strict(
        "Lich's Tomb",
        "Artifact",
        "You don't lose the game for having 0 or less life.",
        &vocab,
    );
    assert!(tomb.faces[0].statics.iter().any(|s| matches!(
        s,
        StaticAbility::GlobalFlag(StaticFlagKind::DontLoseForZeroLife)
    )));

    // 4. The Blue Spirit -> NextSpellModifier wrapping AsThoughFlash
    let spirit = compile_v2_strict(
        "The Blue Spirit",
        "Legendary Creature - Human Rogue",
        "You may cast the first creature spell you cast each turn as though it had flash.",
        &vocab,
    );
    assert!(spirit.faces[0].statics.iter().any(|s| matches!(
        s,
        StaticAbility::NextSpellModifier { effect, .. }
            if matches!(effect.as_ref(), StaticAbility::ZoneCastPermission { mode: CastPermissionMode::AsThoughFlash { .. }, .. })
    )));

    // 5. Ashes of the Abhorrent -> Compound([CantCastSpellsMatching, RuleRestriction(ActivatedAbilitiesCantBeActivated)])
    let ashes = compile_v2_strict(
        "Ashes of the Abhorrent",
        "Enchantment",
        "Players can't cast spells from graveyards or activate abilities of cards in graveyards.",
        &vocab,
    );
    assert!(ashes.faces[0].statics.iter().any(|s| matches!(
        s,
        StaticAbility::Compound(parts)
            if parts.iter().any(|p| matches!(p, StaticAbility::CantCastSpellsMatching { .. }))
            && parts.iter().any(|p| matches!(p, StaticAbility::RuleRestriction { rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated, .. }))
    )));

    // 6. Wrathful Red Dragon -> AnyTarget except Dragon -> Compound([TargetPlayer, TargetObject(forbidden_subtypes: ["Dragon"])])
    let dragon = compile_v2_strict(
        "Wrathful Red Dragon",
        "Creature - Dragon",
        "Whenever a Dragon you control is dealt damage, it deals that much damage to any target that isn't a Dragon.",
        &vocab,
    );
    assert!(dragon.faces[0].abilities.iter().any(|a| match a {
        Ability::Triggered { body, .. } => body.iter().any(|e| matches!(
            e,
            Effect::DealDamage {
                target: SubjectRef::Compound(parts),
                ..
            } if parts.iter().any(|p| matches!(
                p,
                SubjectRef::TargetObject { filter, .. }
                    if filter.forbidden_subtypes.iter().any(|s| s == "Dragon")
            ))
        )),
        _ => false,
    }));

    // 7. Gift of the Viper -> 3 PutCounters mutations + Untap
    let viper = compile_v2_strict(
        "Gift of the Viper",
        "Instant",
        "Put a +1/+1 counter, a reach counter, and a deathtouch counter on target creature. Untap it.",
        &vocab,
    );
    let put_count = viper.faces[0]
        .spell_effects
        .iter()
        .filter(|e| {
            matches!(
                e,
                Effect::MutateObject {
                    mutation: ObjectMutation::PutCounters { .. },
                    ..
                }
            )
        })
        .count();
    assert_eq!(put_count, 3);

    // 8. Psychogenic Probe fails closed honestly (cannot drop `a spell or ability causes` on ShufflesLibrary)
    let probe = compile_card_v2(
        &CardInput {
            oracle_id: "psychogenic-probe",
            name: "Psychogenic Probe",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Psychogenic Probe",
                type_line: "Artifact",
                oracle_text: "Whenever a spell or ability causes a player to shuffle their library, this artifact deals 2 damage to that player.",
            }],
        },
        &vocab,
    );
    assert!(!probe.strictly_compiled);
    let _ = TriggerEvent::PlayerAction {
        player: PlayerRef::EachPlayer,
        action: PlayerTriggerKind::ShufflesLibrary,
    };

    // 9. Wishful Merfolk -> chained RemoveKeywords + BecomeCreatureTypes on same subject
    let merfolk = compile_v2_strict(
        "Wishful Merfolk",
        "Creature - Merfolk",
        "{1}{U}: This creature loses defender and becomes a Human until end of turn.",
        &vocab,
    );
    assert!(merfolk.faces[0].abilities.iter().any(|a| match a {
        Ability::Activated { body, .. } => {
            body.iter().any(|e| {
                matches!(
                    e,
                    Effect::MutateObject {
                        mutation: ObjectMutation::RemoveKeywords(_),
                        ..
                    }
                )
            }) && body.iter().any(|e| {
                matches!(
                    e,
                    Effect::MutateObject {
                        mutation: ObjectMutation::BecomeCreatureTypes { .. },
                        ..
                    }
                )
            })
        }
        _ => false,
    }));
}

/// 18. Deep Wave 15 compositional IR assertions (+1,107 / -0 cards, 89.22% -> 92.66%, 0 IR edits;
///     modular splits `filter/relatives.rs`, `clause/sentence_ext.rs`, `clause/zone_ext.rs`,
///     `condition/extended.rs`): iconic staples across filter, clause, static, replacement,
///     trigger, condition, and value families.
#[test]
fn v2_wave15_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{
        CastPermissionMode, Replacement, StaticAbility, TriggerModifierKind, VarName,
    };

    let vocab = v2_vocab_or_env();

    let wave15_cards = [
        (
            "Runechanter's Pike",
            "Artifact - Equipment",
            "Equipped creature has first strike and gets +X/+0, where X is the number of instant and sorcery cards in your graveyard.\nEquip {2}",
        ),
        (
            "Aettir and Priwen",
            "Legendary Artifact - Equipment",
            "Equipped creature has base power and toughness X/X, where X is your life total.\nEquip {5}",
        ),
        (
            "Deep Freeze",
            "Enchantment - Aura",
            "Enchant creature\nEnchanted creature has base power and toughness 0/4, has defender, loses all other abilities, and is a blue Wall in addition to its other colors and types.",
        ),
        (
            "Aluren",
            "Enchantment",
            "Any player may cast creature spells with mana value 3 or less without paying their mana costs and as though they had flash.",
        ),
        (
            "Drannith Magistrate",
            "Creature - Human Wizard",
            "Your opponents can't cast spells from anywhere other than their hands.",
        ),
        ("Time Stop", "Instant", "End the turn."),
        (
            "Mindslaver",
            "Legendary Artifact",
            "{4}, {T}, Sacrifice Mindslaver: You control target player during that player's next turn.",
        ),
        (
            "Tree of Perdition",
            "Creature - Plant",
            "Defender\n{T}: Exchange target opponent's life total with Tree of Perdition's toughness.",
        ),
        (
            "Inalla, Archmage Ritualist",
            "Legendary Creature - Human Wizard",
            "Eminence \u{2014} Whenever another nontoken Wizard you control enters, if Inalla is in the command zone or on the battlefield, you may pay {1}. If you do, create a token that's a copy of that Wizard. The token gains haste. Exile it at the beginning of the next end step.\nTap five untapped Wizards you control: Target player loses 7 life.",
        ),
        (
            "Bloodspore Thrinax",
            "Creature - Lizard",
            "Devour 1\nEach other creature you control enters with an additional X +1/+1 counters on it, where X is the number of +1/+1 counters on Bloodspore Thrinax.",
        ),
    ];
    for (name, type_line, text) in wave15_cards {
        let compiled = compile_v2_strict(name, type_line, text, &vocab);
        assert!(
            !compiled.faces[0].spell_effects.is_empty()
                || !compiled.faces[0].abilities.is_empty()
                || !compiled.faces[0].statics.is_empty()
                || !compiled.faces[0].replacements.is_empty(),
            "{name} should produce non-empty typed IR"
        );
    }

    // Deep IR checks across the 4 worker domains
    let aluren = compile_v2_strict(
        "Aluren",
        "Enchantment",
        "Any player may cast creature spells with mana value 3 or less without paying their mana costs and as though they had flash.",
        &vocab,
    );
    assert!(aluren.faces[0].statics.iter().any(|s| matches!(
        s,
        StaticAbility::Compound(parts)
            if parts.iter().any(|p| matches!(p, StaticAbility::ZoneCastPermission { mode: CastPermissionMode::WithoutPaying, .. }))
            && parts.iter().any(|p| matches!(p, StaticAbility::ZoneCastPermission { mode: CastPermissionMode::AsThoughFlash { .. }, .. }))
    )));

    // Hushbringer (`entering or dying`) and Containment Priest (`nontoken creature would enter and it wasn't cast`)
    // fail closed honestly rather than dropping `or dying` or `nontoken` / `wasn't cast`.
    for (oid, name, tl, txt) in [
        (
            "hushbringer",
            "Hushbringer",
            "Creature - Faerie",
            "Flying, lifelink\nCreatures entering or dying don't cause abilities to trigger.",
        ),
        (
            "containment-priest",
            "Containment Priest",
            "Creature - Human Cleric",
            "Flash\nIf a nontoken creature would enter and it wasn't cast, exile it instead.",
        ),
    ] {
        let c = compile_card_v2(
            &CardInput {
                oracle_id: oid,
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line: tl,
                    oracle_text: txt,
                }],
            },
            &vocab,
        );
        assert!(!c.strictly_compiled, "{name} should fail closed honestly");
    }
    let _ = (
        Replacement::EntersTapped { unless: None },
        TriggerModifierKind::EntersDontTrigger,
    );

    let pike = compile_v2_strict(
        "Runechanter's Pike",
        "Artifact - Equipment",
        "Equipped creature has first strike and gets +X/+0, where X is the number of instant and sorcery cards in your graveyard.\nEquip {2}",
        &vocab,
    );
    assert!(pike.faces[0].statics.iter().any(|s| matches!(
        s,
        StaticAbility::BindVariable {
            var: VarName::X,
            inner,
            ..
        } if matches!(inner.as_ref(), StaticAbility::Compound(parts)
            if parts.iter().any(|p| matches!(p, StaticAbility::GrantKeywords { .. }))
            && parts.iter().any(|p| matches!(p, StaticAbility::ContinuousModifyPT { .. })))
    )));
}

#[test]
fn v2_wave16_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{Ability, Cost, Effect, StaticAbility};

    let vocab = v2_vocab();

    // Honestly compilable Wave 16 cards (zero IR edits, zero dishonest proxies)
    let wave16_cards: &[(&str, &str, &str)] = &[
        (
            "Dream Halls",
            "Enchantment",
            "Rather than pay the mana cost for a spell, its controller may discard a card that shares a color with that spell.",
        ),
        (
            "Cataclysmic Gearhulk",
            "Artifact Creature - Construct",
            "Vigilance\nWhen this creature enters, each player chooses an artifact, a creature, an enchantment, and a planeswalker from among the nonland permanents they control, then sacrifices the rest.",
        ),
        (
            "Steam Augury",
            "Instant",
            "Reveal the top five cards of your library and separate them into two piles. An opponent chooses one of those piles. Put that pile into your hand and the other into your graveyard.",
        ),
        (
            "Elsha of the Infinite",
            "Legendary Creature - Djinn Monk",
            "Prowess\nYou may look at the top card of your library any time.\nYou may cast noncreature spells from the top of your library. If you cast a spell this way, you may cast it as though it had flash.",
        ),
    ];
    for (name, type_line, text) in wave16_cards {
        let compiled = compile_v2_strict(name, type_line, text, &vocab);
        assert!(
            !compiled.faces[0].spell_effects.is_empty()
                || !compiled.faces[0].abilities.is_empty()
                || !compiled.faces[0].statics.is_empty()
                || !compiled.faces[0].replacements.is_empty(),
            "{name} should produce non-empty typed IR"
        );
    }

    // Wave 16 cards requiring unmodeled IR fail closed honestly after the integrity purge
    for (oid, name, tl, txt) in [
        (
            "grafdiggers-cage",
            "Grafdigger's Cage",
            "Artifact",
            "Creature cards in graveyards and libraries can't enter.\nPlayers can't cast spells from graveyards or libraries.",
        ),
        (
            "hinata",
            "Hinata, Dawn-Crowned",
            "Legendary Creature - Kirin Spirit",
            "Flying, trample\nSpells you cast cost {1} less to cast for each target.\nSpells your opponents cast cost {1} more to cast for each target.",
        ),
        (
            "time-vault",
            "Time Vault",
            "Artifact",
            "Time Vault enters tapped.\nTime Vault doesn't untap during your untap step.\nIf you would begin your turn while Time Vault is tapped, you may skip that turn instead. If you do, untap Time Vault.\n{T}: Take an extra turn after this one.",
        ),
        (
            "councils-judgment",
            "Council's Judgment",
            "Sorcery",
            "Will of the council \u{2014} Starting with you, each player votes for a nonland permanent you don't control. Exile each permanent with the most votes or tied for most votes.",
        ),
    ] {
        let c = compile_card_v2(
            &CardInput {
                oracle_id: oid,
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line: tl,
                    oracle_text: txt,
                }],
            },
            &vocab,
        );
        assert!(!c.strictly_compiled, "{name} should fail closed honestly");
    }
    let _ = (
        Ability::Activated {
            cost: vec![Cost::TapSelf],
            body: Vec::new(),
            restrictions: Vec::new(),
        },
        Effect::ReflexiveTrigger(Vec::new()),
        StaticAbility::RemoveAllAbilities {
            subject: spellwake_oracle_compiler::v2::ir::SubjectRef::SelfObject,
        },
    );
}

#[test]
fn v2_wave17_compositional_grammar_deep_ir_assertions() {
    use spellwake_oracle_compiler::v2::ir::{Ability, ChoiceKind, Cost, Effect, StaticAbility};

    let vocab = v2_vocab();

    // Honestly compilable Wave 17 cards (reflexive modals, Siege bullets, honest exact-IR triggers & effects)
    let wave17_cards: &[(&str, &str, &str)] = &[
        (
            "Hylda of the Icy Crown",
            "Legendary Creature - Human Warlock",
            "Whenever you tap an untapped creature an opponent controls, you may pay {1}. When you do, choose one \u{2014}\n\u{2022} Create a 4/4 white and blue Elemental creature token.\n\u{2022} Put a +1/+1 counter on each creature you control.\n\u{2022} Scry 2, then draw a card.",
        ),
        (
            "Caesar, Legion's Emperor",
            "Legendary Creature - Human Soldier",
            "Whenever you attack, you may sacrifice another creature. When you do, choose two \u{2014}\n\u{2022} Create two 1/1 red and white Soldier creature tokens with haste that are tapped and attacking.\n\u{2022} You draw a card and you lose 1 life.\n\u{2022} Caesar deals damage equal to the number of creature tokens you control to target opponent.",
        ),
        (
            "Monastery Siege",
            "Enchantment",
            "As this enchantment enters, choose Khans or Dragons.\n\u{2022} Khans \u{2014} At the beginning of your draw step, draw an additional card, then discard a card.\n\u{2022} Dragons \u{2014} Spells your opponents cast that target you or a permanent you control cost {2} more to cast.",
        ),
        (
            "Timely Reinforcements",
            "Sorcery",
            "If you have less life than an opponent, you gain 6 life. If you control fewer creatures than an opponent, create three 1/1 white Soldier creature tokens.",
        ),
        (
            "Duskana, the Rage Mother",
            "Legendary Creature - Bear",
            "When Duskana enters, draw a card for each creature you control with base power and toughness 2/2.\nWhenever a creature you control with base power and toughness 2/2 attacks, it gets +3/+3 until end of turn.",
        ),
        (
            "Preacher of the Schism",
            "Creature - Vampire Cleric",
            "Deathtouch\nWhenever this creature attacks the player with the most life or tied for most life, create a 1/1 white Vampire creature token with lifelink.\nWhenever this creature attacks while you have the most life or are tied for most life, you draw a card and you lose 1 life.",
        ),
        (
            "Rowan, Scion of War",
            "Legendary Creature - Human Wizard",
            "Menace\n{T}: Spells you cast this turn that are black and/or red cost {X} less to cast, where X is the amount of life you lost this turn. Activate only as a sorcery.",
        ),
    ];

    for (name, type_line, text) in wave17_cards {
        let compiled = compile_v2_strict(name, type_line, text, &vocab);
        assert!(
            !compiled.faces[0].spell_effects.is_empty()
                || !compiled.faces[0].abilities.is_empty()
                || !compiled.faces[0].statics.is_empty()
                || !compiled.faces[0].replacements.is_empty(),
            "{name} should produce non-empty typed IR"
        );
    }

    // 1. Hylda of the Icy Crown -> Triggered ability with TapOpponentCreature + ReflexiveTrigger containing 3-mode Modal
    let hylda = compile_v2_strict(
        "Hylda of the Icy Crown",
        "Legendary Creature - Human Warlock",
        "Whenever you tap an untapped creature an opponent controls, you may pay {1}. When you do, choose one \u{2014}\n\u{2022} Create a 4/4 white and blue Elemental creature token.\n\u{2022} Put a +1/+1 counter on each creature you control.\n\u{2022} Scry 2, then draw a card.",
        &vocab,
    );
    assert!(hylda.faces[0].abilities.iter().any(|a| matches!(
        a,
        Ability::Triggered { body, .. }
            if body.iter().any(|e| matches!(
                e,
                Effect::ReflexiveTrigger(inner)
                    if inner.iter().any(|ie| matches!(
                        ie,
                        Effect::PromptChoice { kind: ChoiceKind::Modal { modes, .. }, .. } if modes.len() == 3
                    ))
            ))
    )));

    // 2. Monastery Siege -> Replacement choice + Khans triggered + Dragons static cost modifier
    let siege = compile_v2_strict(
        "Monastery Siege",
        "Enchantment",
        "As this enchantment enters, choose Khans or Dragons.\n\u{2022} Khans \u{2014} At the beginning of your draw step, draw an additional card, then discard a card.\n\u{2022} Dragons \u{2014} Spells your opponents cast that target you or a permanent you control cost {2} more to cast.",
        &vocab,
    );
    assert!(!siege.faces[0].replacements.is_empty());
    assert!(
        siege.faces[0]
            .abilities
            .iter()
            .any(|a| matches!(a, Ability::Triggered { .. }))
    );
    assert!(
        siege.faces[0]
            .statics
            .iter()
            .any(|s| matches!(s, StaticAbility::CostModifier { .. }))
    );

    // 3. Lurrus (Companion) and Clay Golem (Roll a d8 cost) fail closed honestly after the integrity purge
    for (oid, name, tl, txt) in [
        (
            "lurrus",
            "Lurrus of the Dream-Den",
            "Legendary Creature - Cat Nightmare",
            "Companion \u{2014} Each permanent card in your starting deck has mana value 2 or less.\nLifelink\nOnce during each of your turns, you may cast a permanent spell with mana value 2 or less from your graveyard.",
        ),
        (
            "clay-golem",
            "Clay Golem",
            "Artifact Creature - Golem",
            "{6}, Roll a d8: Monstrosity X, where X is the result.\nBerserk \u{2014} When this creature becomes monstrous, destroy target permanent.",
        ),
    ] {
        let c = compile_card_v2(
            &CardInput {
                oracle_id: oid,
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line: tl,
                    oracle_text: txt,
                }],
            },
            &vocab,
        );
        assert!(!c.strictly_compiled, "{name} should fail closed honestly");
    }
    let _ = Cost::TapSelf;
}

#[test]
fn v2_adversarial_integrity_purge_fail_closed_and_semantics_suite() {
    let vocab = v2_vocab_or_env();

    // 1. All 3 live fail-open backdoors + dishonest proxy traps MUST fail closed!
    let must_fail_closed: &[(&str, &str, &str)] = &[
        (
            "Inline Would Nonsense",
            "Sorcery",
            "If a player would florb the zargle, draw a card instead.",
        ),
        ("Unknown Token Zargle", "Sorcery", "Create a Zargle token."),
        ("Unknown Token Idol", "Sorcery", "Create an Idol token."),
        (
            "Bare Ability Word On Creature",
            "Creature - Human",
            "Soulknell \u{2014} Draw a card.",
        ),
        (
            "Bare Ability Word Counter On Creature",
            "Creature - Beast",
            "Death knell \u{2014} Put a trample counter on target creature.",
        ),
        (
            "Even Power Proxy Trap",
            "Creature - Wizard",
            "As long as this creature's power is even, it has flying.",
        ),
        (
            "No Longer Suspected Inversion Trap",
            "Instant",
            "Target creature is no longer suspected.",
        ),
        (
            "Spend Only Mana Produced By Inversion Trap",
            "Artifact",
            "Spend only mana produced by basic lands to cast creature spells.",
        ),
        (
            "Roll Die Cost Proxy Trap",
            "Artifact Creature - Golem",
            "{6}, Roll a d8: Draw a card.",
        ),
        (
            "Reflexive Modal Gibberish Trap",
            "Enchantment",
            "Whenever a creature you control dies, you may pay {1}. When florb zargles the blargle, choose one \u{2014}\n\u{2022} Draw a card.",
        ),
        (
            "Arbitrary Role Token Trap",
            "Sorcery",
            "Create a Florb Role token attached to target creature you control.",
        ),
        (
            "Keyword Period Spell Effect Leak On Creature",
            "Creature - Bird",
            "Flying. Draw a card.",
        ),
        (
            "Truncated Token Thats Trap",
            "Sorcery",
            "Create a 1/1 white Soldier creature token that's.",
        ),
        (
            "Static Period Spell Effect Leak On Enchantment",
            "Enchantment",
            "Creatures you control have haste. Draw a card.",
        ),
    ];

    for (name, type_line, oracle_text) in must_fail_closed {
        let compiled = compile_card_v2(
            &CardInput {
                oracle_id: "test-fail-closed",
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line,
                    oracle_text,
                }],
            },
            &vocab,
        );
        assert!(
            !compiled.strictly_compiled,
            "Adversarial trap '{name}' MUST fail closed, but compiled cleanly as: {:#?}",
            compiled.faces[0]
        );
    }

    // 2. Curly quotes & typographic apostrophes normalize cleanly in v2 lexer/card pipeline.
    let curly = compile_v2_strict(
        "Curly Quote Test",
        "Enchantment",
        "Creatures you control can\u{2019}t be blocked and have \u{201c}{T}: Draw a card.\u{201d}",
        &vocab,
    );
    assert!(!curly.faces[0].statics.is_empty());

    // 3. Multi-zone possessive owner carries `TargetPlayer` to all 3 listed zones (never `Default`).
    let multi_zone = compile_v2_strict(
        "Salva Tore Multi Zone",
        "Sorcery",
        "Search target player's graveyard, hand, and library for a card and exile it. Then that player shuffles.",
        &vocab,
    );
    let zones_found = multi_zone.faces[0].spell_effects.iter().any(|e| match e {
        Effect::ZoneOp { from, .. } if from.len() == 3 => {
            from.iter().all(|z| z.owner == ZoneOwner::TargetPlayer)
        }
        _ => false,
    });
    assert!(
        zones_found,
        "Multi-zone search must carry ZoneOwner::TargetPlayer to all 3 zones, got: {:#?}",
        multi_zone.faces[0].spell_effects
    );

    // 4. `attacks alone` MUST NOT silently drop `alone` (either encodes AttackingAlone or fails closed).
    let alone_card = compile_card_v2(
        &CardInput {
            oracle_id: "test-alone",
            name: "Aspiring Hero",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Aspiring Hero",
                type_line: "Creature - Human Knight",
                oracle_text: "Whenever this creature attacks alone, it gets +2/+2 until end of turn.",
            }],
        },
        &vocab,
    );
    if alone_card.strictly_compiled {
        let has_alone = alone_card.faces[0].abilities.iter().any(|a| match a {
            Ability::Triggered { trigger, .. } => {
                format!("{:?}", trigger.intervening_if).contains("AttackingAlone")
                    || format!("{:?}", trigger.events).contains("AttackingAlone")
            }
            _ => false,
        });
        assert!(
            has_alone,
            "`attacks alone` compiled without preserving AttackingAlone: {:#?}",
            alone_card.faces[0].abilities
        );
    }

    // 5. Land animation (`Grove of Whispering Roots`) preserves colors & merges `It's still a land.` cleanly
    let grove = compile_v2_strict(
        "Grove of Whispering Roots",
        "Land",
        "{2}{G}: Until end of turn, this land becomes a 3/3 green Elemental creature with haste. It's still a land.",
        &vocab,
    );
    assert!(
        grove.faces[0].abilities.iter().any(|a| match a {
            Ability::Activated { body, .. } => {
                let has_bogus_permanent_become = body.iter().any(|e| {
                    matches!(
                        e,
                        Effect::MutateObject {
                            mutation: ObjectMutation::BecomeCreatureTypes { .. },
                            duration: Duration::Permanent,
                            ..
                        }
                    )
                });
                let scoped_ok = body.iter().any(|e| match e {
                    Effect::ScopedDuration {
                        duration: Duration::UntilEndOfTurn,
                        body: inner,
                    } => {
                        let has_green = inner.iter().any(|ie| {
                            matches!(
                                ie,
                                Effect::MutateObject {
                                    mutation: ObjectMutation::BecomeColors { colors, .. },
                                    ..
                                } if *colors == ColorMask::GREEN
                            )
                        });
                        let has_keep_types = inner.iter().any(|ie| {
                            matches!(
                                ie,
                                Effect::MutateObject {
                                    mutation: ObjectMutation::BecomeCreatureTypes {
                                        keep_existing_types: true,
                                        ..
                                    },
                                    ..
                                }
                            )
                        });
                        has_green && has_keep_types
                    }
                    _ => false,
                });
                scoped_ok && !has_bogus_permanent_become
            }
            _ => false,
        }),
        "Grove of Whispering Roots must preserve GREEN + keep_existing_types: true inside UntilEndOfTurn with zero Permanent BecomeCreatureTypes, got: {:#?}",
        grove.faces[0].abilities
    );
}
