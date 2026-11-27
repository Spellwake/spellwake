use super::*;
use flate2::read::MultiGzDecoder;
use serde_json::Value;
use spellwake_engine::{
    characteristics::Color,
    combat::CombatRules,
    game::{GameCommand, GameContext, GameId, GameInput, GameState},
    mana::{FloatingMana, ManaPayment, ManaSpend},
    object_identity::ObjectZone,
    priority::PriorityPhase,
    setup::GameSeed,
    stack_window::WindowPhase,
    turn::{Step, TurnStart},
};
use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    sync::Arc,
};

#[test]
fn keyword_list_consumes_the_entire_rules_box_and_cost() {
    use KeywordAbility::{Flying, Vigilance};
    assert_eq!(
        keyword_text("Flying, vigilance (Attacking doesn't cause this creature to tap.)"),
        Some((BTreeSet::from([Flying, Vigilance]), vec![]))
    );
    let (_, repeated) = keyword_text("Exalted\nExalted").unwrap();
    assert_eq!(
        repeated,
        [
            TriggeredAbility::exalted(TriggeredAbilityId(0)),
            TriggeredAbility::exalted(TriggeredAbilityId(1))
        ]
    );
    for raw in [
        "",
        "Flying\nDraw a card.",
        "Flying, ward {1}",
        "Flying (text",
        "Flying), vigilance",
        "Flying until end of turn",
        "Flying, flying",
    ] {
        assert_eq!(keyword_text(raw), None, "{raw}");
    }
    for raw in ["", "{X}", "{1}{W/U}", "{2/W}", "{S}", "{U/P}", "{1}{W}junk"] {
        assert_eq!(fixed_cost(raw), None, "{raw}");
    }
}

fn input(name: &str) -> PathBuf {
    std::env::var_os(name)
        .expect("explicit saved source input")
        .into()
}

/// Each profile compiles independently from the same verified dated input.
fn dated_sources() -> (&'static Inventory, Arc<CreatureTypeVocabulary>) {
    static SOURCE: std::sync::OnceLock<(Inventory, Arc<CreatureTypeVocabulary>)> =
        std::sync::OnceLock::new();
    let (inventory, vocabulary) = SOURCE.get_or_init(|| {
        let vocabulary =
            vocabulary::load_vocabulary(&input("SPELLWAKE_CREATURE_VOCABULARY_INPUT")).unwrap();
        let inventory = spellwake_content::snapshot::load(
            &input("SPELLWAKE_CATALOGUE_METADATA"),
            &input("SPELLWAKE_CATALOGUE_INPUT"),
        )
        .unwrap();
        (inventory, vocabulary)
    });
    (inventory, vocabulary.clone())
}
pub(super) fn extended_catalogue() -> (
    &'static Inventory,
    Arc<CreatureTypeVocabulary>,
    &'static Registry,
) {
    static REGISTRY: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
    let (inventory, vocabulary) = dated_sources();
    let registry = REGISTRY
        .get_or_init(|| Registry::from_extended_keyword_inventory(inventory, &vocabulary).unwrap());
    (inventory, vocabulary, registry)
}
pub(super) fn fourth_catalogue() -> (
    &'static Inventory,
    Arc<CreatureTypeVocabulary>,
    &'static Registry,
) {
    static REGISTRY: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
    let (inventory, vocabulary) = dated_sources();
    let registry = REGISTRY
        .get_or_init(|| Registry::from_fourth_keyword_inventory(inventory, &vocabulary).unwrap());
    (inventory, vocabulary, registry)
}
pub(super) fn fifth_catalogue() -> (
    &'static Inventory,
    Arc<CreatureTypeVocabulary>,
    &'static Registry,
) {
    static REGISTRY: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
    let (inventory, vocabulary) = dated_sources();
    let registry = REGISTRY
        .get_or_init(|| Registry::from_fifth_keyword_inventory(inventory, &vocabulary).unwrap());
    (inventory, vocabulary, registry)
}
pub(super) fn sixth_catalogue() -> (
    &'static Inventory,
    Arc<CreatureTypeVocabulary>,
    &'static Registry,
) {
    static REGISTRY: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
    let (inventory, vocabulary) = dated_sources();
    let registry = REGISTRY
        .get_or_init(|| Registry::from_sixth_keyword_inventory(inventory, &vocabulary).unwrap());
    (inventory, vocabulary, registry)
}
pub(crate) fn current_catalogue() -> (
    &'static Inventory,
    Arc<CreatureTypeVocabulary>,
    &'static Registry,
) {
    static REGISTRY: std::sync::OnceLock<Result<Registry, RegistryError>> =
        std::sync::OnceLock::new();
    let (inventory, vocabulary) = dated_sources();
    let registry = REGISTRY
        .get_or_init(|| Registry::from_current_keyword_inventory(inventory, &vocabulary))
        .as_ref()
        .unwrap();
    (inventory, vocabulary, registry)
}

fn advance(state: &GameState, player: SeatId, command: GameCommand) -> GameState {
    state
        .advance(GameInput {
            basis: state.basis().clone(),
            actor: player,
            command,
        })
        .unwrap()
        .into_state()
}

fn cast_source(registry: &Registry, raw: &Value, vocabulary: &Arc<CreatureTypeVocabulary>) {
    let id = OracleId::try_from(raw["oracle_id"].as_str().unwrap().to_owned()).unwrap();
    let (a, b) = (SeatId(0), SeatId(1));
    let seeded = registry
        .seed(&id, CardId(1), a, ObjectDestination::Hand)
        .unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        KEYWORD_CREATURE_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "keyword-creatures-registry12".into(),
        "paid-cast".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    // Fund exactly the source price, independently of the compiled Program.
    let mut required = BTreeMap::<_, i64>::new();
    let source_cost = raw["mana_cost"].as_str().unwrap();
    for symbol in source_cost[1..source_cost.len() - 1].split("}{") {
        let (ty, n) = match symbol {
            "W" => (ManaType::White, 1),
            "U" => (ManaType::Blue, 1),
            "B" => (ManaType::Black, 1),
            "R" => (ManaType::Red, 1),
            "G" => (ManaType::Green, 1),
            "C" => (ManaType::Colourless, 1),
            generic => (ManaType::Colourless, generic.parse().unwrap()),
        };
        if n > 0 {
            *required.entry(ty).or_default() += n;
        }
    }
    let mana = required
        .iter()
        .map(|(&mana_type, &n)| FloatingMana {
            player: a,
            mana_type,
            amount: n.into(),
        })
        .collect();
    let payment = ManaPayment {
        from_lots: Default::default(),
        spend: required
            .into_iter()
            .map(|(mana_type, n)| ManaSpend {
                mana_type,
                amount: n.into(),
            })
            .collect(),
    };
    let seed = GameSeed::with_uniform_life(
        vec![a, b],
        a,
        vec![seeded],
        vec![],
        mana,
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary.clone(),
        names,
    );
    let mut state = GameState::initialize_scheduled(
        GameId(12),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let object = |s: &GameState| {
        s.execution()
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap()
    };
    state = advance(
        &state,
        a,
        GameCommand::CastUntargeted {
            spell: object(&state),
            payment: Some(payment),
        },
    );
    assert_eq!(state.execution().window().entries().len(), 1);
    for pass in 0..2 {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        else {
            panic!("priority after casting {}", raw["name"]);
        };
        let candidate = state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor: holder,
                command: GameCommand::Pass,
            })
            .unwrap();
        if pass == 1 {
            let entry = candidate
                .permanent_entry()
                .expect("resolved creature entry");
            assert_eq!(entry.before.zone(), ObjectZone::Stack);
            assert_eq!(entry.after.zone(), ObjectZone::Battlefield);
            assert_eq!(entry.after.card_id(), Some(CardId(1)));
        }
        state = candidate.into_state();
    }
    let sheet = state
        .execution()
        .current_characteristics(object(&state))
        .unwrap()
        .unwrap();
    let expected_zone = if raw["toughness"].as_str().unwrap().parse::<i64>().unwrap() <= 0 {
        ObjectZone::Graveyard(a)
    } else {
        ObjectZone::Battlefield
    };
    assert_eq!(sheet.zone, expected_zone, "{}", raw["name"]);
    let mut source_keywords: BTreeSet<String> = raw["keywords"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().replace(' ', "").to_ascii_lowercase())
        .collect();
    // Scryfall omits these two specialized abilities from its keywords array.
    match raw["name"].as_str().unwrap() {
        "Rime Dryad" => {
            source_keywords.insert("snowforestwalk".into());
        }
        "Legions of Lim-Dûl" => {
            source_keywords.insert("snowswampwalk".into());
        }
        _ => {}
    }
    if source_keywords.remove("exalted") {
        let copies = raw["oracle_text"]
            .as_str()
            .unwrap()
            .split(['\n', ','])
            .filter(|line| {
                line.trim_start()
                    .split([' ', '('])
                    .next()
                    .is_some_and(|word| word.eq_ignore_ascii_case("exalted"))
            })
            .count();
        assert_eq!(sheet.abilities.triggered.len(), copies);
        for (index, trigger) in sheet.printed_text.triggered.iter().enumerate() {
            assert_eq!(
                trigger,
                &TriggeredAbility::exalted(TriggeredAbilityId(index as u64))
            );
        }
    } else {
        assert!(sheet.abilities.triggered.is_empty());
    }
    source_keywords.remove("landwalk"); // Generic family may accompany its actual member.
    assert_eq!(
        sheet
            .keywords
            .iter()
            .map(|k| format!("{k:?}").to_ascii_lowercase())
            .collect::<BTreeSet<_>>(),
        source_keywords,
        "{}",
        raw["name"]
    );
    let colors: BTreeSet<_> = raw["colors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|color| match color.as_str().unwrap() {
            "W" => Color::White,
            "U" => Color::Blue,
            "B" => Color::Black,
            "R" => Color::Red,
            "G" => Color::Green,
            _ => panic!("unexpected raw color"),
        })
        .collect();
    assert!(
        raw.get("color_indicator")
            .is_none_or(|v| v.as_array().unwrap().is_empty())
    );
    assert_eq!(sheet.current_colors, colors, "{}", raw["name"]);
    let pt = sheet.power_toughness.unwrap().derived;
    assert_eq!(String::from(pt.power), raw["power"].as_str().unwrap());
    assert_eq!(
        String::from(pt.toughness),
        raw["toughness"].as_str().unwrap()
    );
    if sheet.keywords.contains(&KeywordAbility::Changeling) {
        assert_eq!(
            sheet.current_creature_subtypes.len(),
            vocabulary.members().len()
        );
    }
}

#[test]
#[ignore = "requires the dated saved metadata/gzip and source-qualified vocabulary"]
fn every_bound_keyword_creature_casts_resolves_and_matches_raw_oracle_characteristics() {
    let meta = input("SPELLWAKE_CATALOGUE_METADATA");
    let cards = input("SPELLWAKE_CATALOGUE_INPUT");
    let vocabulary =
        vocabulary::load_vocabulary(&input("SPELLWAKE_CREATURE_VOCABULARY_INPUT")).unwrap();
    let mut inventory = spellwake_content::snapshot::load(&meta, &cards).unwrap();
    let registry = Registry::from_keyword_creature_inventory(&inventory, &vocabulary).unwrap();
    let mut remaining: BTreeSet<_> = registry.bindings.keys().map(|id| id.as_str()).collect();
    assert_eq!(remaining.len(), 623);
    assert_eq!(registry.coverage.cards.len(), 32_155);
    let raw = MultiGzDecoder::new(std::fs::File::open(cards).unwrap());
    for line in BufReader::new(raw).lines() {
        let raw: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if remaining.remove(raw["oracle_id"].as_str().unwrap()) {
            cast_source(&registry, &raw, &vocabulary);
        }
    }
    assert!(remaining.is_empty());
    let selected = inventory
        .cards
        .iter_mut()
        .find(|c| c.name == "Ornithopter")
        .unwrap();
    selected.faces[0].oracle_text = Some("Menace".into());
    assert!(matches!(
        Registry::from_keyword_creature_inventory(&inventory, &vocabulary),
        Err(RegistryError::KeywordCreatureSourceMismatch)
    ));
}

#[test]
#[ignore = "requires the dated saved metadata/gzip and source-qualified vocabulary"]
fn serra_angel_stays_untapped_and_only_giant_spider_can_block_it() {
    use spellwake_engine::{
        combat::{AttackDeclaration, BlockDeclaration, BlockStatus, CombatDecision},
        game::{DecisionReply, RuleFrame},
    };
    let vocabulary =
        vocabulary::load_vocabulary(&input("SPELLWAKE_CREATURE_VOCABULARY_INPUT")).unwrap();
    let inventory = spellwake_content::snapshot::load(
        &input("SPELLWAKE_CATALOGUE_METADATA"),
        &input("SPELLWAKE_CATALOGUE_INPUT"),
    )
    .unwrap();
    let registry = Registry::from_keyword_creature_inventory(&inventory, &vocabulary).unwrap();
    let (a, b) = (SeatId(0), SeatId(1));
    let mut seeded = vec![];
    for (n, name, owner, destination) in [
        (
            1,
            "Serra Angel",
            a,
            ObjectDestination::Battlefield { controller: a },
        ),
        (
            2,
            "Giant Spider",
            b,
            ObjectDestination::Battlefield { controller: b },
        ),
        (
            3,
            "Raging Goblin",
            b,
            ObjectDestination::Battlefield { controller: b },
        ),
        (4, "Raging Goblin", a, ObjectDestination::LibraryBottom),
        (5, "Raging Goblin", b, ObjectDestination::LibraryBottom),
    ] {
        let row = inventory
            .cards
            .iter()
            .find(|card| card.name == name)
            .unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        seeded.push(registry.seed(&id, CardId(n), owner, destination).unwrap());
    }
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        KEYWORD_CREATURE_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "keyword-creatures-registry12".into(),
        "named-combat".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![a, b],
        a,
        seeded,
        vec![CardId(1), CardId(2), CardId(3)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let mut state = GameState::initialize_scheduled(
        GameId(13),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::Untap,
        },
    )
    .unwrap()
    .into_state();
    let pass = |s: &GameState| {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) = s.execution().window().phase()
        else {
            panic!("expected priority");
        };
        advance(s, holder, GameCommand::Pass)
    };
    for _ in 0..32 {
        if state.pending_decision().is_some() {
            break;
        }
        state = pass(&state);
    }
    let objects = state.execution().window().cards();
    let angel = objects.current_identity(CardId(1)).unwrap();
    let spider = objects.current_identity(CardId(2)).unwrap();
    let goblin = objects.current_identity(CardId(3)).unwrap();
    let decision = state.pending_decision().unwrap();
    assert!(matches!(
        decision.frame(),
        RuleFrame::Combat(CombatDecision::Attackers(_))
    ));
    state = advance(
        &state,
        a,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: angel,
                defender: b,
                defending_planeswalker: None,
            }]),
        },
    );
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(angel)
            .unwrap()
            .tapped(),
        Some(false)
    );
    state = pass(&pass(&state));
    let pending = state.pending_decision().unwrap();
    let RuleFrame::Combat(CombatDecision::Blockers(blocks)) = pending.frame() else {
        panic!("expected block declaration");
    };
    assert_eq!(blocks.legal_attackers(spider), &[angel]);
    assert!(blocks.legal_attackers(goblin).is_empty());
    let block = |blocker| GameCommand::Reply {
        decision: pending.id(),
        choice: DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            attacker: angel,
            blocker,
        }]),
    };
    assert!(
        state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor: b,
                command: block(goblin)
            })
            .is_err()
    );
    let blocked = advance(&state, b, block(spider));
    assert_eq!(
        blocked
            .execution()
            .combat()
            .unwrap()
            .attacker(angel)
            .unwrap()
            .block_status(),
        BlockStatus::Blocked
    );
}

mod attachments;
mod crew_saddle;
mod face_down;
mod speed;

mod cascade;
mod escape;
mod exile_hand;
mod gameplay_v2;
mod hand_abilities;
mod madness;
mod nonbasic_lands;
mod rebound;
mod token_triggers;

mod bestow;
mod end_step_cast;
mod evoke;
mod ninja;
mod station;

mod limited_activations;
mod ward_protection_plot;

mod bargain;
mod cleave;
mod emerge;
mod entry_counters;

mod combat_prices;
mod commander;

mod cast_copies;
mod power_up;

mod batch_definitions;
mod clause_probe;
mod daybound;
mod named_card_edges;
mod ordinary_card_audit;
mod transfigure;

mod sliver_grants;

mod basic_land_search;

mod enchantment_watchers;

mod spell_token_watchers;
