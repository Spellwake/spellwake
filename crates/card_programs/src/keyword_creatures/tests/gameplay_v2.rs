use super::*;
mod ordinary_spell_audit;
use spellwake_engine::targets::TargetRef;

/// Exercise the ordinary printed price from hand independently for every new
/// identity. Named mechanism tests cover activation, discounts and optional costs.
#[test]
#[ignore = "requires the saved Oracle snapshot and matching creature vocabulary"]
fn every_new_gameplay_binding_casts_and_reaches_its_source_zone() {
    check_added(Profile::GameplayV2);
}

#[test]
#[ignore = "requires the saved Oracle snapshot and matching creature vocabulary"]
fn every_extended_binding_casts_and_reaches_its_source_zone() {
    check_added(Profile::GameplayV3);
}

#[test]
#[ignore = "requires the saved Oracle snapshot and matching creature vocabulary"]
fn every_fourth_binding_casts_and_reaches_its_source_zone() {
    check_added(Profile::GameplayV4);
}

#[test]
#[ignore = "requires the saved Oracle snapshot and matching creature vocabulary"]
fn every_fifth_binding_casts_and_reaches_its_source_zone() {
    check_added(Profile::GameplayV5);
}

#[test]
#[ignore = "requires the saved Oracle snapshot and matching creature vocabulary"]
fn every_sixth_binding_casts_and_reaches_its_source_zone() {
    check_added(Profile::GameplayV6);
}

#[test]
#[ignore = "requires the saved Oracle snapshot and matching creature vocabulary"]
fn every_current_binding_casts_and_reaches_its_source_zone() {
    check_added(Profile::Current);
}

#[test]
#[ignore = "requires the saved Oracle snapshot and matching creature vocabulary"]
fn plain_draw_spells_compile_from_oracle_text_and_draw_the_two_library_cards() {
    let (_, vocabulary, registry) = current_catalogue();
    let mut wanted = BTreeSet::from(["Divination", "Counsel of the Soratami"]);
    let source =
        MultiGzDecoder::new(std::fs::File::open(input("SPELLWAKE_CATALOGUE_INPUT")).unwrap());
    for line in BufReader::new(source).lines() {
        let raw: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if raw["name"]
            .as_str()
            .is_some_and(|name| wanted.contains(name))
            && raw["oracle_text"] == "Draw two cards."
        {
            let id = OracleId::try_from(raw["oracle_id"].as_str().unwrap().to_owned()).unwrap();
            if !registry.bindings.contains_key(&id) {
                continue;
            }
            cast_new_card(registry, &id, &raw, &vocabulary);
            wanted.remove(raw["name"].as_str().unwrap());
        }
        if wanted.is_empty() {
            return;
        }
    }
    panic!("Missing source-backed draw spells: {wanted:?}");
}

fn check_added(profile: Profile) {
    let cards = input("SPELLWAKE_CATALOGUE_INPUT");
    let (inventory, vocabulary) = dated_sources();
    let predecessor;
    let compiled;
    let (previous, registry) = match profile {
        Profile::GameplayV2 => {
            predecessor =
                Registry::from_keyword_creature_inventory(inventory, &vocabulary).unwrap();
            compiled = Registry::from_keyword_gameplay_inventory(inventory, &vocabulary).unwrap();
            (&predecessor, &compiled)
        }
        Profile::GameplayV3 => {
            predecessor =
                Registry::from_keyword_gameplay_inventory(inventory, &vocabulary).unwrap();
            (&predecessor, extended_catalogue().2)
        }
        Profile::GameplayV4 => (extended_catalogue().2, fourth_catalogue().2),
        Profile::GameplayV5 => (fourth_catalogue().2, fifth_catalogue().2),
        Profile::GameplayV6 => (fifth_catalogue().2, sixth_catalogue().2),
        Profile::Current => (sixth_catalogue().2, current_catalogue().2),
        Profile::CreatureV1 => unreachable!(),
    };
    assert_eq!(
        previous.bindings.len(),
        match profile {
            Profile::Current => 1747,
            Profile::GameplayV6 => 1370,
            Profile::GameplayV5 => 1234,
            Profile::GameplayV4 => 1140,
            Profile::GameplayV3 => 995,
            Profile::GameplayV2 => 623,
            Profile::CreatureV1 => unreachable!(),
        }
    );
    assert!(
        previous
            .bindings
            .keys()
            .all(|id| registry.bindings.contains_key(id))
    );
    let mut remaining: BTreeSet<_> = registry
        .bindings
        .keys()
        .filter(|id| !previous.bindings.contains_key(*id))
        .cloned()
        .collect();
    let total = remaining.len();
    let mut lands = 0;
    let source = MultiGzDecoder::new(std::fs::File::open(cards).unwrap());
    for line in BufReader::new(source).lines() {
        let raw: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let Some(oid) = raw["oracle_id"].as_str() else {
            continue;
        };
        let id = OracleId::try_from(oid.to_owned()).unwrap();
        if remaining.remove(&id) {
            if raw["type_line"].as_str().is_some_and(|t| {
                t.split(" — ")
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .any(|w| w == "Land")
                    && raw["name"] != "Zoetic Cavern"
            }) {
                lands += 1;
            }
            cast_new_card(registry, &id, &raw, &vocabulary);
        }
    }
    assert!(remaining.is_empty());
    if profile >= Profile::GameplayV5 {
        eprintln!(
            "Verified {} additional real spells through paid casting and resolution and {lands} lands through land play in registry {}",
            total - lands,
            registry.coverage.registry_version
        );
        return;
    }
    eprintln!(
        "Verified {total} additional real cards through paid casting and resolution in registry {}",
        registry.coverage.registry_version
    );
}

fn cast_new_card(
    registry: &Registry,
    id: &OracleId,
    raw: &Value,
    vocabulary: &Arc<CreatureTypeVocabulary>,
) {
    let (a, b) = (SeatId(0), SeatId(1));
    let name = raw["name"].as_str().unwrap();
    let raw = raw.pointer("/card_faces/0").unwrap_or(raw);
    let kind = raw["type_line"].as_str().unwrap();
    let text = without_reminder(raw["oracle_text"].as_str().unwrap_or("")).unwrap();
    let instant_or_sorcery = kind
        .split(" — ")
        .next()
        .unwrap()
        .split_whitespace()
        .any(|word| matches!(word, "Instant" | "Sorcery"));
    use spellwake_engine::program::{BodyTarget, ProgramKind};
    let source = registry
        .seed(id, CardId(1), a, ObjectDestination::Hand)
        .unwrap();
    let target = match &source.program.kind {
        ProgramKind::Spell(definition) => definition.body.target,
        ProgramKind::Permanent(_) => BodyTarget::None,
    };
    let audited = if matches!(kind, "Instant" | "Sorcery") {
        ordinary_spell_audit::Outcome::from_source(name, &text)
    } else {
        None
    };
    if let Some(outcome) = &audited {
        assert_eq!(target, outcome.target(), "{name}");
    }
    let has_target = kind.contains("— Aura") || target != BodyTarget::None;
    let target_player = matches!(
        target,
        BodyTarget::SinglePlayer | BodyTarget::SingleCreatureOrPlayer
    );
    let out_of_hand = target
        == BodyTarget::SingleSpell(
            spellwake_engine::program::SpellFilter::any().not_from_owners_hand(),
        );
    let target_spell = matches!(target, BodyTarget::SingleSpell(_));
    let mut cards = vec![source];
    let toughness = if audited.is_some() {
        ordinary_spell_audit::TARGET_TOUGHNESS
    } else {
        20
    };
    let vanilla = Program::creature(None, 2.into(), toughness.into(), BTreeSet::new());
    if target_spell {
        let mut primer =
            Program::creature(Some(ManaCost::free()), 1.into(), 1.into(), BTreeSet::new());
        if out_of_hand {
            primer = Program::instant(
                ManaCost::free(),
                spellwake_engine::program::ExecutableBody::new(
                    spellwake_engine::program::BodyTarget::None,
                    vec![spellwake_engine::program::Instruction::DrawController {
                        amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                    }],
                ),
            );
            primer.flashback = Some(ManaCost::free());
        }
        cards.push(SeedCard {
            card: CardId(7),
            owner: a,
            destination: if out_of_hand {
                ObjectDestination::Graveyard
            } else {
                ObjectDestination::Hand
            },
            program: primer,
        });
    }
    if has_target && !target_player && !target_spell {
        let controller = if text.starts_with("Enchant creature you control") {
            a
        } else {
            b
        };
        cards.push(SeedCard {
            card: CardId(2),
            owner: controller,
            destination: ObjectDestination::Battlefield { controller },
            program: vanilla.clone(),
        });
    }
    for index in 3..7 {
        cards.push(SeedCard {
            card: CardId(index),
            owner: a,
            destination: ObjectDestination::LibraryBottom,
            program: vanilla.clone(),
        });
    }
    let mut price = BTreeMap::<ManaType, i64>::new();
    // Zoetic Cavern has no printed mana cost and can be cast face down for {3}.
    let face_down_land = kind == "Land" && text.lines().any(|line| line.starts_with("Morph "));
    let basic_land = kind.starts_with("Basic ") && kind.contains("Land");
    let plays_land = !face_down_land
        && kind
            .split(" — ")
            .next()
            .unwrap()
            .split_whitespace()
            .any(|w| w == "Land");
    let source_cost = if plays_land {
        "{0}"
    } else if face_down_land {
        "{3}"
    } else {
        raw["mana_cost"].as_str().unwrap()
    };
    let undaunted = text.lines().any(|line| line.trim() == "Undaunted");
    for symbol in source_cost
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split("}{")
    {
        let (color, amount) = match symbol {
            "W" => (ManaType::White, 1),
            "U" => (ManaType::Blue, 1),
            "B" => (ManaType::Black, 1),
            "R" => (ManaType::Red, 1),
            "G" => (ManaType::Green, 1),
            "C" => (ManaType::Colourless, 1),
            n => (
                ManaType::Colourless,
                (n.parse::<i64>().unwrap() - i64::from(undaunted)).max(0),
            ),
        };
        if amount > 0 {
            *price.entry(color).or_default() += amount;
        }
    }
    let mana = price
        .iter()
        .map(|(&mana_type, &amount)| FloatingMana {
            player: a,
            mana_type,
            amount: amount.into(),
        })
        .collect();
    let payment = ManaPayment {
        from_lots: Default::default(),
        spend: price
            .into_iter()
            .map(|(mana_type, amount)| ManaSpend {
                mana_type,
                amount: amount.into(),
            })
            .collect(),
    };
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        match registry.coverage.registry_version {
            13 => KEYWORD_GAMEPLAY_PROFILE,
            14 => KEYWORD_GAMEPLAY_V3_PROFILE,
            15 => KEYWORD_GAMEPLAY_V4_PROFILE,
            16 => KEYWORD_GAMEPLAY_V5_PROFILE,
            17 => KEYWORD_GAMEPLAY_V6_PROFILE,
            _ => CURRENT_KEYWORD_PROFILE,
        }
        .into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        format!(
            "keyword-gameplay-registry{}",
            registry.coverage.registry_version
        ),
        "source-paid-cast".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![a, b],
        a,
        cards,
        if has_target && !target_player && !target_spell {
            vec![CardId(2)]
        } else {
            vec![]
        },
        mana,
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary.clone(),
        names,
    );
    let mut state = GameState::initialize_scheduled(
        GameId(13),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap_or_else(|e| panic!("{name}: {e:?}"))
    .into_state();
    let object = |s: &GameState, n| {
        s.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    };
    if target_spell {
        let primer = GameCommand::CastUntargeted {
            spell: object(&state, 7),
            payment: None,
        };
        state = advance(&state, a, primer);
    }
    let spell = object(&state, 1);
    if plays_land {
        state = advance(&state, a, GameCommand::PlayLand { land: spell });
        let permanent = object(&state, 1);
        let sheet = state
            .execution()
            .current_characteristics(permanent)
            .unwrap()
            .unwrap();
        assert_eq!(sheet.zone, ObjectZone::Battlefield, "{name}");
        let abilities = sheet.mana_abilities();
        if !basic_land {
            let expect_tapped = text.lines().any(|line| {
                line.trim().starts_with("This land enters tapped")
                    && !line.contains("unless you control two or fewer other lands")
            });
            let object = state.execution().window().cards().get(permanent).unwrap();
            assert_eq!(object.tapped(), Some(expect_tapped), "{name}");
            assert!(!abilities.is_empty(), "{name}");
            return;
        }
        assert_eq!(abilities.len(), 1, "{name}");
        let produced = advance(
            &state,
            a,
            GameCommand::ActivateMana {
                source: permanent,
                ability: *abilities.iter().next().unwrap(),
            },
        );
        let expected = match kind.split(" — ").nth(1) {
            Some("Plains") => ManaType::White,
            Some("Island") => ManaType::Blue,
            Some("Swamp") => ManaType::Black,
            Some("Mountain") => ManaType::Red,
            Some("Forest") => ManaType::Green,
            None => ManaType::Colourless,
            _ => panic!("{name}"),
        };
        assert_eq!(produced.execution().mana(a, expected), 1.into(), "{name}");
        return;
    }
    let command = if face_down_land {
        GameCommand::CastFaceDown {
            spell,
            kind: spellwake_engine::face_down::FaceDownKind::Morph,
            payment: Some(payment),
        }
    } else if has_target {
        GameCommand::CastPaid {
            spell,
            target: if target_spell {
                TargetRef::Stack(state.execution().window().entries()[0].id())
            } else if target_player {
                TargetRef::Player(b)
            } else {
                TargetRef::Object(object(&state, 2))
            },
            payment,
        }
    } else {
        GameCommand::CastUntargeted {
            spell,
            payment: Some(payment),
        }
    };
    use spellwake_engine::{
        effects::{LibraryChoiceKind, LibraryTopReply, OptionalEffectChoice},
        game::{DecisionReply, RuleFrame},
    };
    state = state
        .advance(GameInput {
            basis: state.basis().clone(),
            actor: a,
            command,
        })
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .into_state();
    // A cast trigger can resolve before the spell. Keep passing priority until
    // the physical card actually moves, answering the current authorized frame.
    for _ in 0..64 {
        let zone = state
            .execution()
            .window()
            .cards()
            .get(object(&state, 1))
            .unwrap()
            .zone();
        if zone != ObjectZone::Stack {
            break;
        }
        let (actor, command) = if let Some(decision) = state.pending_decision() {
            let choice = match decision.frame() {
                RuleFrame::LibraryTop(frame) => DecisionReply::LibraryTop(match frame.kind() {
                    LibraryChoiceKind::Scry => LibraryTopReply::Scry {
                        top_first: frame.domain_top_first().to_vec(),
                        bottom_top_first: vec![],
                    },
                    LibraryChoiceKind::Surveil => LibraryTopReply::Surveil {
                        top_first: frame.domain_top_first().to_vec(),
                        to_graveyard: vec![],
                    },
                    LibraryChoiceKind::Search => LibraryTopReply::Search { selected: None },
                }),
                RuleFrame::Triggers(frame) => {
                    DecisionReply::OrderTriggers(frame.instances().to_vec())
                }
                RuleFrame::TriggerTarget(frame) => DecisionReply::TriggerTarget(frame.domain()[0]),
                RuleFrame::ResolutionSelect(frame) => DecisionReply::SelectResolution(
                    frame
                        .domain()
                        .iter()
                        .take(frame.required())
                        .copied()
                        .collect(),
                ),
                RuleFrame::ResolutionGraveyardOrder(frame) => {
                    DecisionReply::OrderResolutionGraveyard(frame.domain().to_vec())
                }
                RuleFrame::OptionalEffect(_) => {
                    DecisionReply::OptionalEffect(OptionalEffectChoice::Decline)
                }
                RuleFrame::TimestampOrder(frame) => DecisionReply::TimestampOrder(
                    frame.members().iter().map(|member| member.member).collect(),
                ),
                other => panic!("{name}: unhandled decision during paid casting {other:?}"),
            };
            (
                decision.chooser(),
                GameCommand::Reply {
                    decision: decision.id(),
                    choice,
                },
            )
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                state.execution().window().phase()
            else {
                panic!("{name}: priority before spell resolution")
            };
            (holder, GameCommand::Pass)
        };
        state = state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor,
                command,
            })
            .unwrap_or_else(|error| panic!("{name}: {error:?}"))
            .into_state();
    }
    if let Some(outcome) = audited {
        outcome.verify(&state, name);
    }
    let zero_toughness = raw["toughness"]
        .as_str()
        .and_then(|s| s.parse::<i64>().ok())
        .is_some_and(|n| n <= 0);
    let sunburst_survived = text.lines().any(|line| line == "Sunburst")
        && source_cost.bytes().any(|b| b"WUBRG".contains(&b));
    let enters_with_counters = text.split([',', '\n']).map(str::trim).any(|line| {
        ["Modular ", "Graft "].iter().any(|prefix| {
            line.strip_prefix(prefix)
                .and_then(|n| n.parse::<u16>().ok())
                .is_some_and(|n| n > 0)
        })
    });
    let expected = if text.lines().any(|line| line.trim() == "Rebound") {
        ObjectZone::Exile
    } else if instant_or_sorcery || (zero_toughness && !sunburst_survived && !enters_with_counters)
    {
        ObjectZone::Graveyard(a)
    } else {
        ObjectZone::Battlefield
    };
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 1))
            .unwrap()
            .zone(),
        expected,
        "{name}"
    );
}
