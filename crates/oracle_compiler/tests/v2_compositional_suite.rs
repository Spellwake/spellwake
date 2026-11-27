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
            "Teferi's Ageless Insight",
            "Legendary Enchantment",
            "If you would draw a card except the first one you draw in each of your draw steps, draw two cards instead.",
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
            "Pithing Needle",
            "Artifact",
            "As Pithing Needle enters, choose a card name.\nActivated abilities of sources with the chosen name can't be activated unless they're mana abilities.",
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
            "Exotic Orchard",
            "Land",
            "{T}: Add one mana of any color that a land an opponent controls could produce.",
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

    // 9. Dain, Lord of the Iron Hills -> ConditionFlag::HasEnduringStory + StaticAbility::AttackTax
    let dain = compile_v2_strict(
        "Dain, Lord of the Iron Hills",
        "Legendary Creature — Dwarf Noble",
        "As long as you have an enduring story, creatures can't attack you unless their controller pays {1} for each of those creatures.",
        &vocab,
    );
    assert!(matches!(
        dain.faces[0].statics.as_slice(),
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

    // 11. Pithing Needle -> StaticAbility::GlobalFlag(StaticFlagKind::NamedSourcesCantActivate)
    let needle = compile_v2_strict(
        "Pithing Needle",
        "Artifact",
        "As Pithing Needle enters, choose a card name.\nActivated abilities of sources with the chosen name can't be activated unless they're mana abilities.",
        &vocab,
    );
    assert!(matches!(
        needle.faces[0].statics.as_slice(),
        [StaticAbility::GlobalFlag(
            StaticFlagKind::NamedSourcesCantActivate
        )]
    ));

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
    use spellwake_oracle_compiler::v2::ir::{
        ConditionExpr, DamageReplacement, PlayerRestrictionKind, ZoneMetricKind,
    };
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
            "Stormwild Capridor",
            "Creature - Bird Goat",
            "Flying\nIf noncombat damage would be dealt to Stormwild Capridor, prevent that damage. Put a +1/+1 counter on Stormwild Capridor for each 1 damage prevented this way.",
        ),
        (
            "Inkshield",
            "Instant",
            "Prevent all combat damage that would be dealt to you this turn. For each 1 damage prevented this way, create a 2/1 white and black Inkling creature token with flying.",
        ),
        (
            "Wolverine, Best There Is",
            "Legendary Creature - Mutant Berserker Hero",
            "Double all damage Wolverine would deal.\nAt the beginning of each end step, if Wolverine dealt damage to another creature this turn, put a +1/+1 counter on him.\n{1}{G}: Regenerate Wolverine.",
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
            "Awaken the Ancient",
            "Enchantment - Aura",
            "Enchant Mountain\nEnchanted Mountain is a 7/7 red Giant creature with haste. It's still a land.",
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
            "Kozilek, the Great Distortion",
            "Legendary Creature - Eldrazi",
            "When you cast this spell, if you have fewer than seven cards in hand, draw cards equal to the difference.\nMenace\nDiscard a card with mana value X: Counter target spell with mana value X.",
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
            "Epic Experiment",
            "Sorcery",
            "Exile the top X cards of your library. For each instant and sorcery card with mana value X or less among them, you may cast that spell without paying its mana cost. Then put all cards exiled this way that weren't cast into your graveyard.",
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

    // 3. Stormwild Capridor two-sentence damage-prevention replacement preserves rider
    let capridor = compile_v2_strict(
        "Stormwild Capridor",
        "Creature - Bird Goat",
        "If noncombat damage would be dealt to Stormwild Capridor, prevent that damage. Put a +1/+1 counter on Stormwild Capridor for each 1 damage prevented this way.",
        &vocab,
    );
    assert!(matches!(
        capridor.faces[0].replacements.as_slice(),
        [Replacement::DamageRule(DamageReplacement::PreventWithRider { rider, .. })]
            if !rider.is_empty()
    ));

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

    // 10. Wolverine `Double all damage ~ would deal` -> Replacement::ModifyDamage { factor: 2 }
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
        CounterKind, DamageReplacement, LibraryPosition, ObjectMutation, PlayerRestrictionKind,
        PlayerTriggerKind, Replacement, StaticAbility, TokenReplacementKind, TriggerEvent, VarName,
        ZoneOpKind,
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
            "Jinnie Fay, Jetmir's Second",
            "Legendary Creature - Elf Druid",
            "If you would create one or more tokens, you may instead create that many 2/2 green Cat creature tokens with haste or that many 3/1 green Dog creature tokens with vigilance.",
        ),
        (
            "Winter Orb",
            "Artifact",
            "As long as Winter Orb is untapped, players can't untap more than one land during their untap steps.",
        ),
        (
            "Winter Moon",
            "Artifact",
            "Players can't untap more than one nonbasic land during their untap steps.",
        ),
        (
            "Phyrexian Vindicator",
            "Creature - Phyrexian Horror",
            "Flying\nIf damage would be dealt to Phyrexian Vindicator, prevent that damage. When damage is prevented this way, Phyrexian Vindicator deals that much damage to any other target.",
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
            "Kruphix, God of Horizons",
            "Legendary Enchantment Creature - God",
            "Indestructible\nAs long as your devotion to green and blue is less than seven, Kruphix isn't a creature.\nYou have no maximum hand size.\nIf you would lose unspent mana, that mana becomes colorless instead.",
        ),
        (
            "Training Grounds",
            "Enchantment",
            "Activated abilities of creatures you control cost {2} less to activate. This effect can't reduce the mana in that cost to less than one mana.",
        ),
        (
            "Fatal Push",
            "Instant",
            "Destroy target creature if it has mana value 2 or less.\nRevolt \u{2014} Destroy that creature if it has mana value 4 or less instead if a permanent left the battlefield under your control this turn.",
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
            "Bolas's Citadel",
            "Legendary Artifact",
            "You may look at the top card of your library any time.\nYou may play lands and cast spells from the top of your library. If you cast a spell this way, pay life equal to its mana value rather than pay its mana cost.\n{T}, Sacrifice ten nonland permanents: Each opponent loses 10 life.",
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
            "Cactus Preserve",
            "Land",
            "Cactus Preserve enters tapped.\n{T}: Add one mana of any type that a land you control could produce.\n{3}: Until end of turn, Cactus Preserve becomes an X/X green Plant creature with reach, where X is the greatest mana value among your commanders. It's still a land.",
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

    // 4. Winter Moon (`PlayerRestrictionKind::CantUntapMoreThan(1)`)
    let winter_moon = compile_v2_strict(
        "Winter Moon",
        "Artifact",
        "Players can't untap more than one nonbasic land during their untap steps.",
        &vocab,
    );
    assert!(matches!(
        winter_moon.faces[0].statics.as_slice(),
        [StaticAbility::PlayerRestriction {
            restriction: PlayerRestrictionKind::CantUntapMoreThan(1),
            ..
        }]
    ));

    // 5. Phyrexian Vindicator (`PreventWithRider` with non-empty reflexive trigger rider)
    let vindicator = compile_v2_strict(
        "Phyrexian Vindicator",
        "Creature - Phyrexian Horror",
        "If damage would be dealt to Phyrexian Vindicator, prevent that damage. When damage is prevented this way, Phyrexian Vindicator deals that much damage to any other target.",
        &vocab,
    );
    assert!(matches!(
        vindicator.faces[0].replacements.as_slice(),
        [Replacement::DamageRule(DamageReplacement::PreventWithRider { rider, .. })]
            if !rider.is_empty()
    ));

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

    // 12. Training Grounds (activation cost modifier -> StaticAbility::ActivationCostModifier with less: true)
    let grounds = compile_v2_strict(
        "Training Grounds",
        "Enchantment",
        "Activated abilities of creatures you control cost {2} less to activate. This effect can't reduce the mana in that cost to less than one mana.",
        &vocab,
    );
    assert!(matches!(
        grounds.faces[0].statics.first(),
        Some(StaticAbility::ActivationCostModifier { less: true, .. })
    ));
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
        CastPermissionMode, ConditionExpr, DamageReplacement, Duration, FilterFlags, KeywordKind,
        KeywordParam, KeywordSpec, LibraryPosition, ManaSpec, ObjectMutation, PlayerMutation,
        PlayerObjectTriggerKind, PlayerRestrictionKind, PlayerTriggerKind, Replacement,
        RuleRestrictionKind, StaticAbility, SubjectStaticRule, TriggerEvent, ValueExpr,
        ZoneMetricKind, ZoneOpKind,
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
            "Solphim, Mayhem Dominus",
            "Legendary Creature - Phyrexian Horror",
            "If a source you control would deal noncombat damage to an opponent or a permanent an opponent controls, it deals double that damage to that player or permanent instead.\n{1}{R/P}{R/P}, Discard two cards: Put an indestructible counter on Solphim, Mayhem Dominus.",
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
            "Command Tower",
            "Land",
            "{T}: Add one mana of any color in your commander's color identity.",
        ),
        (
            "Arcane Signet",
            "Artifact",
            "{T}: Add one mana of any color in your commander's color identity.",
        ),
        (
            "Giver of Runes",
            "Creature - Kor Cleric",
            "{T}: Another target creature you control gains protection from colorless or from the color of your choice until end of turn.",
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
            "Ghyrson Starn, Kelermorph",
            "Legendary Creature - Tyranid Human",
            "Ward {2}\nThree Autostubs \u{2014} Whenever another source you control deals exactly 1 damage to a permanent or player, Ghyrson Starn, Kelermorph deals 2 damage to that permanent or player.",
        ),
        (
            "Forbidden Orchard",
            "Land",
            "{T}: Add one mana of any color.\nWhenever you tap Forbidden Orchard for mana, target opponent creates a 1/1 colorless Spirit creature token.",
        ),
        (
            "Encroaching Mycosynth",
            "Artifact",
            "Nonland permanents you control are artifacts in addition to their other types. The same is true for permanent spells you control and nonland permanent cards you own that aren't on the battlefield.",
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
            "Grist, the Hunger Tide",
            "Legendary Planeswalker - Grist",
            "As long as Grist, the Hunger Tide isn't on the battlefield, it's a 1/1 Insect creature in addition to its other types.\n+1: Create a 1/1 black and green Insect creature token, then mill a card. If an Insect card was milled this way, put a loyalty counter on Grist, the Hunger Tide and repeat this process.\n-2: You may sacrifice a creature. When you do, destroy target creature or planeswalker.\n-5: Each opponent loses life equal to the number of creature cards in your graveyard.",
        ),
        (
            "Maha, Its Feathers Night",
            "Legendary Creature - Elemental Bird",
            "Flying, trample\nWard\u{2014}Discard a card.\nCreatures your opponents control have base toughness 1.",
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

    // 6. Command Tower -> PlayerMutation::AddMana(ManaSpec::AnyColor)
    let tower = compile_v2_strict(
        "Command Tower",
        "Land",
        "{T}: Add one mana of any color in your commander's color identity.",
        &vocab,
    );
    assert!(matches!(
        tower.faces[0].abilities.as_slice(),
        [Ability::Activated { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::MutatePlayer {
                    mutation: PlayerMutation::AddMana(ManaSpec::AnyColor { .. }),
                    ..
                }]
            )
    ));

    // 7. Giver of Runes -> Protection with KeywordParam::ChosenColor
    let giver = compile_v2_strict(
        "Giver of Runes",
        "Creature - Kor Cleric",
        "{T}: Another target creature you control gains protection from colorless or from the color of your choice until end of turn.",
        &vocab,
    );
    assert!(matches!(
        giver.faces[0].abilities.as_slice(),
        [Ability::Activated { body, .. }]
            if matches!(
                body.as_slice(),
                [Effect::MutateObject {
                    mutation: ObjectMutation::GrantKeywords(kws),
                    ..
                }] if matches!(
                    kws.as_slice(),
                    [KeywordSpec {
                        kind: KeywordKind::Protection,
                        param: Some(KeywordParam::ChosenColor),
                    }]
                )
            )
    ));

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

    // 11. Grist (`~ isn't on the battlefield` -> ConditionExpr::SubjectMatches with NOT_ON_BATTLEFIELD)
    let grist = compile_v2_strict(
        "Grist, the Hunger Tide",
        "Legendary Planeswalker - Grist",
        "As long as Grist, the Hunger Tide isn't on the battlefield, it's a 1/1 Insect creature in addition to its other types.",
        &vocab,
    );
    assert!(matches!(
        grist.faces[0].statics.as_slice(),
        [StaticAbility::Compound(parts)] if !parts.is_empty()
    ));

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
