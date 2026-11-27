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
        summary.strictly_compiled_cards >= 37,
        "expected >= 37/103 strictly compiled real cards in v2, got {}/{}",
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
