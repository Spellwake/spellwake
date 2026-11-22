//! Card-specific mechanical evidence for single-faced, textless creatures and basic lands.
//! Input is the same pinned Oracle gzip, metadata, and vocabulary used by the registry.
use super::*;
use spellwake_engine::{
    checks::CheckDecision,
    combat::{AttackDeclaration, BlockDeclaration, CombatDecision},
    effects::DecisionReply,
    game::{NativeCandidate, RuleFrame},
    land::{BasicLandType, LandType},
    mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaCost, ManaOutput},
};
use std::{fmt::Write as _, fs, path::Path};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const LIFE: i64 = 100;

struct Creature {
    definition: Program,
    cost: ManaCost,
    budget: BTreeMap<ManaType, i64>,
    power: i64,
    toughness: i64,
    colors: BTreeSet<Color>,
}
struct Land {
    definition: Program,
    ability: ManaAbilityRef,
    mana: ManaType,
    snow: bool,
}
enum Shape {
    Creature(Creature),
    Land(Land),
}
struct Sources<'a> {
    registry: &'a Registry,
    vocabulary: Arc<CreatureTypeVocabulary>,
    ids: BTreeMap<String, OracleId>,
}
impl Sources<'_> {
    fn seed(&self, name: &str, n: u64, owner: SeatId, zone: ObjectDestination) -> SeedCard {
        let oracle = self
            .ids
            .get(name)
            .unwrap_or_else(|| panic!("missing required Oracle row for {name}"));
        self.registry
            .seed(oracle, CardId(n), owner, zone)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"))
    }
    fn game(&self, cards: Vec<SeedCard>, order: Vec<CardId>) -> GameState {
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let ctx = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            self.vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "ordinary-card-audit-v1".into(),
            "public-commands".into(),
            self.vocabulary.id(),
            names.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![A, B],
            A,
            cards,
            order,
            vec![],
            LIFE.into(),
            CombatRules::UnrestrictedFreeForAll,
            self.vocabulary.clone(),
            names,
        );
        GameState::initialize_scheduled(
            GameId(709),
            ctx,
            seed,
            TurnStart {
                number: 1,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state()
    }
}

fn eligible_shape(raw: &Value) -> bool {
    let line = raw["type_line"].as_str().unwrap_or("");
    let top = line.split(" — ").next().unwrap_or("");
    (top.split_whitespace().any(|w| w == "Creature")
        && raw["oracle_text"]
            .as_str()
            .is_some_and(|s| s.trim().is_empty()))
        || (top.split_whitespace().any(|w| w == "Basic")
            && top.split_whitespace().any(|w| w == "Land"))
}
// Scryfall also uses all_parts for cards whose own printed text names this card.
// Accept only an exact self row and independently source-matched incoming references.
fn inbound_name_references(
    raw: &Value,
    printings: &BTreeMap<&str, &InventoryCard>,
) -> Result<Vec<String>, &'static str> {
    let Some(parts) = raw.get("all_parts").filter(|p| !p.is_null()) else {
        return Ok(vec![]);
    };
    let parts = parts
        .as_array()
        .ok_or("malformed associated card records")?;
    if parts.is_empty() {
        return Ok(vec![]);
    }
    let source_id = raw["id"]
        .as_str()
        .ok_or("missing representative printing identity")?;
    let name = raw["name"].as_str().ok_or("missing source name")?;
    let phrase = format!("named {name}");
    let mut seen_self = false;
    let mut sources = BTreeSet::new();
    for part in parts {
        if part["component"] != "combo_piece" || part["object"] != "related_card" {
            return Err("associated token, meld component, or unverified card part");
        }
        let id = part["id"]
            .as_str()
            .ok_or("missing associated printing identity")?;
        if id == source_id {
            if seen_self || part["name"] != name {
                return Err("ambiguous associated self reference");
            }
            seen_self = true;
            continue;
        }
        let printed = printings
            .get(id)
            .ok_or("associated printing missing from dated source")?;
        let names_this_card = printed
            .faces
            .iter()
            .filter_map(|face| face.oracle_text.as_deref())
            .any(|text| {
                text.match_indices(&phrase).any(|(start, _)| {
                    text[start + phrase.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_alphanumeric() && c != '-')
                })
            });
        if part["name"].as_str() != Some(printed.name.as_str())
            || printed.name == name
            || !names_this_card
            || !sources.insert(printed.name.clone())
        {
            return Err(
                "associated card does not independently name this card in its printed ability",
            );
        }
    }
    if !seen_self || sources.is_empty() {
        return Err("associated cards do not form verified incoming name references");
    }
    Ok(sources.into_iter().collect())
}
fn ordinary_source(raw: &Value) -> Result<(), &'static str> {
    if raw["layout"] != "normal"
        || raw["lang"] != "en"
        || raw["name"].as_str().is_none_or(|s| s.contains(" // "))
    {
        return Err("requires another face, language, or layout");
    }
    for field in ["card_faces", "color_indicator", "keywords"] {
        if !raw[field].is_null() && raw[field].as_array().is_none_or(|v| !v.is_empty()) {
            return Err("associated faces, color indicator, or additional source keywords");
        }
    }
    if [
        "defense",
        "loyalty",
        "life_modifier",
        "hand_modifier",
        "attraction_lights",
    ]
    .iter()
    .any(|key| !raw[key].is_null())
        || raw["security_stamp"] == "acorn"
        || raw["oversized"] == true
        || raw["rebalanced"] == true
    {
        return Err("additional printed rules characteristics or nonordinary printing");
    }
    Ok(())
}
fn cost_from_oracle(raw: &str) -> Result<(ManaCost, BTreeMap<ManaType, i64>), &'static str> {
    let mut rest = raw;
    let mut cost = ManaCost::free();
    let mut budget = BTreeMap::new();
    if rest.is_empty() {
        return Err("missing castable cost");
    }
    while !rest.is_empty() {
        let (sym, tail) = rest
            .strip_prefix('{')
            .and_then(|r| r.split_once('}'))
            .ok_or("malformed mana symbol")?;
        rest = tail;
        let kind = match sym {
            "W" => Some(ManaType::White),
            "U" => Some(ManaType::Blue),
            "B" => Some(ManaType::Black),
            "R" => Some(ManaType::Red),
            "G" => Some(ManaType::Green),
            "C" => Some(ManaType::Colourless),
            _ => None,
        };
        let (pay, count) = if let Some(kind) = kind {
            let v = cost.required.entry(kind).or_insert_with(|| 0.into());
            *v = v.plus(&1.into());
            (kind, 1)
        } else {
            if !sym.bytes().all(|v| v.is_ascii_digit()) {
                return Err("requires another mana-symbol payment");
            }
            let generic = sym.parse::<i64>().map_err(|_| "invalid generic cost")?;
            cost.generic = cost.generic.plus(&generic.into());
            (ManaType::Colourless, generic)
        };
        if count > 0 {
            *budget.entry(pay).or_insert(0) += count;
        }
    }
    Ok((cost, budget))
}
fn colors(raw: &Value) -> Result<BTreeSet<Color>, &'static str> {
    raw.as_array()
        .ok_or("missing color data")?
        .iter()
        .map(|c| match c.as_str() {
            Some("W") => Ok(Color::White),
            Some("U") => Ok(Color::Blue),
            Some("B") => Ok(Color::Black),
            Some("R") => Ok(Color::Red),
            Some("G") => Ok(Color::Green),
            _ => Err("unrecognized source color"),
        })
        .collect()
}
fn shape(raw: &Value, vocab: &CreatureTypeVocabulary) -> Result<Shape, &'static str> {
    ordinary_source(raw)?;
    let name = raw["name"].as_str().ok_or("missing card name")?;
    let name = || ObjectNames::single(EnglishCardName::new(name.to_owned()).unwrap());
    let line = raw["type_line"].as_str().ok_or("missing type line")?;
    let (left, right) = line.split_once(" — ").unwrap_or((line, ""));
    let text = raw["oracle_text"]
        .as_str()
        .ok_or("missing explicit rules box")?
        .trim();
    if left.contains("Land") {
        let snow = match left {
            "Basic Land" => false,
            "Basic Snow Land" => true,
            _ => return Err("additional basic land types"),
        };
        if raw["mana_cost"] != ""
            || !raw["power"].is_null()
            || !raw["toughness"].is_null()
            || !colors(&raw["colors"])?.is_empty()
        {
            return Err("additional land characteristics");
        }
        let (mut definition, ability, mana) = if right.is_empty() && text == "{T}: Add {C}." {
            (
                Program::land(
                    BTreeSet::new(),
                    vec![ManaAbility {
                        id: ManaAbilityId(0),
                        output: ManaOutput::Fixed([(ManaType::Colourless, 1.into())].into()),
                    }],
                ),
                ManaAbilityRef::Printed(ManaAbilityId(0)),
                ManaType::Colourless,
            )
        } else {
            let (kind, subtype, symbol) = match right {
                "Plains" => (BasicLandType::Plains, LandType::Plains, "W"),
                "Island" => (BasicLandType::Island, LandType::Island, "U"),
                "Swamp" => (BasicLandType::Swamp, LandType::Swamp, "B"),
                "Mountain" => (BasicLandType::Mountain, LandType::Mountain, "R"),
                "Forest" => (BasicLandType::Forest, LandType::Forest, "G"),
                _ => return Err("additional basic land subtypes"),
            };
            if !text.is_empty() && text != format!("({{T}}: Add {{{symbol}}}.)") {
                return Err("additional basic land rules text");
            }
            (
                Program::land([subtype].into(), vec![]),
                ManaAbilityRef::BasicLand(kind),
                kind.mana_type(),
            )
        };
        definition.names = name();
        definition.supertypes.insert(Supertype::Basic);
        if snow {
            definition.supertypes.insert(Supertype::Snow);
        }
        return Ok(Shape::Land(Land {
            definition,
            ability,
            mana,
            snow,
        }));
    }
    if !text.is_empty() {
        return Err("additional creature rules text");
    }
    let pt = |k: &str| {
        raw[k]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or("nonliteral power or toughness")
    };
    let (power, toughness) = (pt("power")?, pt("toughness")?);
    if power < 0 || toughness <= 0 {
        return Err("requires a distinct zero-toughness or negative-power scenario");
    }
    let (cost, budget) = cost_from_oracle(raw["mana_cost"].as_str().ok_or("missing cost")?)?;
    let subtypes = right
        .split_whitespace()
        .map(|s| {
            vocab
                .resolve_label(s)
                .ok_or("unrecognized creature subtype")
        })
        .collect::<Result<_, _>>()?;
    let mut definition =
        Program::creature(Some(cost.clone()), power.into(), toughness.into(), subtypes);
    for word in left.split_whitespace() {
        match word {
            "Creature" => {}
            "Artifact" => {
                definition.card_types.insert(CardType::Artifact);
            }
            "Enchantment" => {
                definition.card_types.insert(CardType::Enchantment);
            }
            "Legendary" => {
                definition.supertypes.insert(Supertype::Legendary);
            }
            _ => return Err("additional creature card type or supertype"),
        }
    }
    let colors = colors(&raw["colors"])?;
    if colors != self::colors(&raw["color_identity"])? {
        return Err("color identity differs from simple colored cost");
    }
    definition.names = name();
    Ok(Shape::Creature(Creature {
        definition,
        cost,
        budget,
        power,
        toughness,
        colors,
    }))
}
fn object(game: &GameState, n: u64) -> spellwake_engine::object_identity::ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(n))
        .unwrap()
}
fn action(s: &GameState, seat: SeatId, command: GameCommand) -> NativeCandidate {
    s.advance(GameInput {
        basis: s.basis().clone(),
        actor: seat,
        command,
    })
    .unwrap()
}
fn pass(s: &GameState) -> GameState {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = s.execution().window().phase()
    else {
        panic!("unexpected nonpriority game boundary")
    };
    action(s, holder, GameCommand::Pass).into_state()
}
fn seek(mut s: GameState, predicate: impl Fn(&GameState) -> bool) -> GameState {
    for _ in 0..128 {
        if predicate(&s) {
            return s;
        }
        assert!(
            s.pending_decision().is_none(),
            "unexpected decision {:?}",
            s.pending_decision().map(|d| d.frame())
        );
        s = pass(&s);
    }
    panic!("expected game boundary was not reached")
}
fn reply(s: &GameState, choice: DecisionReply) -> GameState {
    let decision = s.pending_decision().unwrap();
    action(
        s,
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice,
        },
    )
    .into_state()
}
fn declaration(s: &GameState, attacking: bool) -> bool {
    let f = s.pending_decision().map(|d| d.frame());
    if attacking {
        matches!(f, Some(RuleFrame::Combat(CombatDecision::Attackers(_))))
    } else {
        matches!(f, Some(RuleFrame::Combat(CombatDecision::Blockers(_))))
    }
}
fn end_combat(s: &GameState) -> bool {
    s.turn()
        .is_some_and(|t| t.position().step == Step::EndCombat)
}
fn payment_source(kind: ManaType) -> (&'static str, ManaAbilityRef) {
    match kind {
        ManaType::White => ("Plains", ManaAbilityRef::BasicLand(BasicLandType::Plains)),
        ManaType::Blue => ("Island", ManaAbilityRef::BasicLand(BasicLandType::Island)),
        ManaType::Black => ("Swamp", ManaAbilityRef::BasicLand(BasicLandType::Swamp)),
        ManaType::Red => (
            "Mountain",
            ManaAbilityRef::BasicLand(BasicLandType::Mountain),
        ),
        ManaType::Green => ("Forest", ManaAbilityRef::BasicLand(BasicLandType::Forest)),
        ManaType::Colourless => ("Wastes", ManaAbilityRef::Printed(ManaAbilityId(0))),
    }
}
fn creature_start(
    source: &Sources<'_>,
    name: &str,
    case: &Creature,
    legend: bool,
) -> (GameState, NativeCandidate) {
    let home = |controller| ObjectDestination::Battlefield { controller };
    let mut cards = vec![
        source.seed(name, 1, A, ObjectDestination::Hand),
        source.seed(name, 2, B, home(B)),
        source.seed("Raging Goblin", 3, A, home(A)),
        source.seed("Forest", 4, A, ObjectDestination::LibraryBottom),
        source.seed("Forest", 5, B, ObjectDestination::LibraryBottom),
    ];
    let mut order = vec![CardId(2), CardId(3)];
    if legend {
        cards.push(source.seed(name, 6, A, home(A)));
        order.push(CardId(6));
    }
    let mut lands = vec![];
    for (&kind, &amount) in &case.budget {
        let (land, ability) = payment_source(kind);
        for _ in 0..amount {
            let n = 10 + u64::try_from(lands.len()).unwrap();
            cards.push(source.seed(land, n, A, home(A)));
            order.push(CardId(n));
            lands.push((n, ability, kind));
        }
    }
    let mut s = source.game(cards, order);
    for (n, ability, kind) in lands {
        let prior = s.execution().mana(A, kind);
        let next = action(
            &s,
            A,
            GameCommand::ActivateMana {
                source: object(&s, n),
                ability,
            },
        );
        assert!(next.mana_production().is_some());
        s = next.into_state();
        assert_eq!(s.execution().mana(A, kind), prior.plus(&1.into()));
    }
    let hand = object(&s, 1);
    let spend = case
        .budget
        .iter()
        .map(|(&mana_type, &n)| ManaSpend {
            mana_type,
            amount: n.into(),
        })
        .collect::<Vec<_>>();
    let cast = action(
        &s,
        A,
        GameCommand::CastUntargeted {
            spell: hand,
            payment: (!spend.is_empty()).then(|| ManaPayment {
                from_lots: Default::default(),
                spend: spend.clone(),
            }),
        },
    );
    let receipt = cast.mana_payment().unwrap();
    assert_eq!(receipt.cost, case.cost);
    assert_eq!(receipt.spend, spend);
    assert_eq!(receipt.payer, A);
    let stack = cast.into_state();
    assert_ne!(object(&stack, 1), hand);
    assert_eq!(stack.execution().window().entries().len(), 1);
    assert_eq!(
        stack
            .execution()
            .window()
            .cards()
            .get(object(&stack, 1))
            .unwrap()
            .zone(),
        ObjectZone::Stack
    );
    let first = pass(&stack);
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = first.execution().window().phase()
    else {
        panic!("second pass expected")
    };
    let resolved = action(&first, holder, GameCommand::Pass);
    let moved = resolved.permanent_entry().unwrap();
    assert_eq!(moved.before.zone(), ObjectZone::Stack);
    assert_eq!(moved.after.zone(), ObjectZone::Battlefield);
    (stack, resolved)
}

fn verify_creature(src: &Sources<'_>, name: &str, case: &Creature) -> String {
    let (stack, resolved) = creature_start(src, name, case, false);
    let s = resolved.into_state();
    let me = object(&s, 1);
    let other = object(&s, 2);
    assert_ne!(object(&stack, 1), me);
    let sheet = s.execution().current_characteristics(me).unwrap().unwrap();
    assert_eq!(sheet.zone, ObjectZone::Battlefield);
    assert_eq!(sheet.controller, Some(A));
    assert_eq!(sheet.current_names, case.definition.names);
    assert_eq!(sheet.current_colors, case.colors);
    assert_eq!(sheet.current_types, case.definition.card_types);
    assert_eq!(sheet.current_supertypes, case.definition.supertypes);
    assert_eq!(
        sheet.current_creature_subtypes,
        case.definition.creature_subtypes
    );
    let pt = sheet.power_toughness.unwrap().derived;
    assert_eq!(pt.power, case.power.into());
    assert_eq!(pt.toughness, case.toughness.into());
    assert!(
        sheet.keywords.is_empty()
            && sheet.abilities.activated.is_empty()
            && sheet.abilities.triggered.is_empty()
    );
    let first = seek(s, |s| declaration(s, true));
    let RuleFrame::Combat(CombatDecision::Attackers(f)) = first.pending_decision().unwrap().frame()
    else {
        unreachable!()
    };
    assert!(!f.eligible_attackers().contains(&me));
    assert!(f.eligible_attackers().contains(&object(&first, 3)));
    let b_attack = seek(
        reply(&first, DecisionReply::DeclareAttackers(vec![])),
        |s| declaration(s, true),
    );
    assert_eq!(b_attack.pending_decision().unwrap().chooser(), B);
    let b_declared = reply(
        &b_attack,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: other,
            defender: A,
            defending_planeswalker: None,
        }]),
    );
    let block = seek(b_declared, |s| declaration(s, false));
    let RuleFrame::Combat(CombatDecision::Blockers(f)) = block.pending_decision().unwrap().frame()
    else {
        unreachable!()
    };
    assert!(f.legal_attackers(me).contains(&other));
    let blocked = reply(
        &block,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker: me,
            attacker: other,
        }]),
    );
    let damage = seek(blocked, end_combat);
    assert_eq!(damage.execution().life_total(A), Some(&LIFE.into()));
    for (n, owner) in [(1, A), (2, B)] {
        let permanent = object(&damage, n);
        let zone = damage
            .execution()
            .window()
            .cards()
            .get(permanent)
            .unwrap()
            .zone();
        if case.power >= case.toughness {
            assert_eq!(zone, ObjectZone::Graveyard(owner));
        } else {
            assert_eq!(zone, ObjectZone::Battlefield);
            assert_eq!(
                damage.execution().battlefield_marked_damage(permanent),
                Some(case.power.into())
            );
        }
    }
    let ready = seek(
        reply(&b_attack, DecisionReply::DeclareAttackers(vec![])),
        |s| declaration(s, true),
    );
    assert_eq!(ready.pending_decision().unwrap().chooser(), A);
    let attacking = reply(
        &ready,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: me,
            defender: B,
            defending_planeswalker: None,
        }]),
    );
    assert_eq!(
        attacking
            .execution()
            .window()
            .cards()
            .get(me)
            .unwrap()
            .tapped(),
        Some(true)
    );
    let b_blocks = seek(attacking, |s| declaration(s, false));
    let damage = seek(
        reply(&b_blocks, DecisionReply::DeclareBlockers(vec![])),
        end_combat,
    );
    assert_eq!(
        damage.execution().life_total(B),
        Some(&(LIFE - case.power).into())
    );
    let legendary = case.definition.supertypes.contains(&Supertype::Legendary);
    if legendary {
        let (_, resolved) = creature_start(src, name, case, true);
        let s = resolved.into_state();
        let RuleFrame::Checks(CheckDecision::Legend(choose)) =
            s.pending_decision().unwrap().frame()
        else {
            panic!("legend rule did not offer a choice")
        };
        let [group] = choose.conflicts() else {
            panic!("expected one legend conflict")
        };
        assert_eq!(
            group.domain().iter().copied().collect::<BTreeSet<_>>(),
            [object(&s, 1), object(&s, 6)].into()
        );
        let d = reply(
            &s,
            DecisionReply::KeepLegend {
                conflict: group.id(),
                keep: object(&s, 1),
            },
        );
        assert_eq!(
            d.execution()
                .window()
                .cards()
                .get(object(&d, 6))
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(A)
        );
        assert_eq!(
            d.execution()
                .window()
                .cards()
                .get(object(&d, 1))
                .unwrap()
                .zone(),
            ObjectZone::Battlefield
        );
    }
    format!(
        "paid {} mana from battlefield sources; stack resolution and exact face; {}/{}; summoning sickness; attacked and tapped; blocked same-stat attacker; unblocked damage {}; legend rule {}",
        String::from(case.cost.total()),
        case.power,
        case.toughness,
        case.power,
        if legendary {
            "exercised"
        } else {
            "inapplicable"
        }
    )
}
fn verify_land(src: &Sources<'_>, name: &str, case: &Land) -> String {
    let s = src.game(
        vec![
            src.seed(name, 1, A, ObjectDestination::Hand),
            src.seed(name, 2, A, ObjectDestination::Hand),
        ],
        vec![],
    );
    let played = action(
        &s,
        A,
        GameCommand::PlayLand {
            land: object(&s, 1),
        },
    );
    assert!(played.land_play().is_some());
    let after = played.into_state();
    let land = object(&after, 1);
    assert_ne!(object(&s, 1), land);
    assert!(after.execution().window().entries().is_empty());
    assert!(
        after
            .advance(GameInput {
                basis: after.basis().clone(),
                actor: A,
                command: GameCommand::PlayLand {
                    land: object(&after, 2)
                }
            })
            .is_err()
    );
    let sheet = after
        .execution()
        .current_characteristics(land)
        .unwrap()
        .unwrap();
    assert_eq!(sheet.current_names, case.definition.names);
    assert_eq!(sheet.zone, ObjectZone::Battlefield);
    assert_eq!(sheet.current_types, case.definition.card_types);
    assert_eq!(sheet.current_supertypes, case.definition.supertypes);
    assert_eq!(sheet.current_land_subtypes, case.definition.land_subtypes);
    assert!(sheet.current_colors.is_empty());
    assert_eq!(sheet.controller, Some(A));
    let ability = GameCommand::ActivateMana {
        source: land,
        ability: case.ability,
    };
    let tapped = action(&after, A, ability.clone());
    assert!(tapped.mana_production().is_some());
    let tapped = tapped.into_state();
    assert_eq!(
        tapped
            .execution()
            .window()
            .cards()
            .get(land)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert!(tapped.execution().window().entries().is_empty());
    for kind in [
        ManaType::White,
        ManaType::Blue,
        ManaType::Black,
        ManaType::Red,
        ManaType::Green,
        ManaType::Colourless,
    ] {
        assert_eq!(
            tapped.execution().mana(A, kind),
            i64::from(kind == case.mana).into()
        );
    }
    assert!(
        tapped
            .advance(GameInput {
                basis: tapped.basis().clone(),
                actor: A,
                command: ability
            })
            .is_err()
    );
    format!(
        "normal land play; second land rejected; exact subtypes and supertypes; mana ability produced one {:?}; tapped source rejected",
        case.mana
    )
}
fn source_row_matches(row: &InventoryCard, raw: &Value) {
    assert_eq!(raw["id"].as_str(), Some(row.scryfall_id.as_str()));
    assert_eq!(raw["layout"].as_str(), Some(row.layout.as_str()));
    assert_eq!(raw["name"].as_str(), Some(row.name.as_str()));
    let [face] = row.faces.as_slice() else {
        panic!("expected one materialized source face")
    };
    for (name, exact) in [
        ("oracle_text", &face.oracle_text),
        ("mana_cost", &face.mana_cost),
        ("type_line", &face.type_line),
        ("power", &face.power),
        ("toughness", &face.toughness),
    ] {
        assert_eq!(
            raw[name].as_str(),
            exact.as_deref(),
            "materialized {name} mismatch"
        );
    }
}
fn tsv(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}
fn publish(path: &Path, text: &str) {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).unwrap();
    }
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&tmp, text).unwrap();
    fs::rename(tmp, path).unwrap();
}
#[test]
#[ignore = "requires pinned dated Oracle and vocabulary inputs and SPELLWAKE_CARD_AUDIT_OUTPUT"]
fn ordinary_card_audit_verifies_each_eligible_oracle_identity_against_play() {
    let path = input("SPELLWAKE_CARD_AUDIT_OUTPUT");
    let (inventory, vocabulary, registry) = current_catalogue();
    let printings = inventory
        .cards
        .iter()
        .map(|card| (card.scryfall_id.as_str(), card))
        .collect::<BTreeMap<_, _>>();
    let required = inventory
        .cards
        .iter()
        .filter(|r| r.required_by_base_formats)
        .map(|r| (r.oracle_id.as_deref().unwrap(), r))
        .collect::<BTreeMap<_, _>>();
    let ids = required
        .values()
        .map(|r| {
            (
                r.name.clone(),
                OracleId::try_from(r.oracle_id.clone().unwrap()).unwrap(),
            )
        })
        .collect();
    let src = Sources {
        registry,
        vocabulary: vocabulary.clone(),
        ids,
    };
    let signatures = registry
        .coverage()
        .cards
        .iter()
        .filter_map(|r| match &r.binding {
            BindingStatus::Partial {
                semantic_sha256, ..
            } => Some((r.oracle_id.as_deref().unwrap(), semantic_sha256.as_str())),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut rows = BTreeMap::new();
    let mut failed = vec![];
    let mut verified = 0;
    let file = MultiGzDecoder::new(fs::File::open(input("SPELLWAKE_CATALOGUE_INPUT")).unwrap());
    for line in BufReader::new(file).lines() {
        let raw: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if !eligible_shape(&raw) {
            continue;
        }
        let Some(id) = raw["oracle_id"].as_str() else {
            continue;
        };
        let Some(row) = required.get(id) else {
            continue;
        };
        let named = &row.name;
        let family = if raw["type_line"].as_str().unwrap().starts_with("Basic ") {
            "basic-land"
        } else {
            "textless-creature"
        };
        let signature = signatures.get(id).copied().unwrap_or("");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Err(reason) = inbound_name_references(&raw, &printings) {
                return ("outside-scope", reason.to_owned());
            }
            let case = match shape(&raw, &vocabulary) {
                Ok(v) => v,
                Err(reason) => return ("outside-scope", reason.to_owned()),
            };
            source_row_matches(row, &raw);
            let oracle = OracleId::try_from(id.to_owned()).unwrap();
            let Some(binding) = registry.resolve(&oracle) else {
                return ("unbound", "no current executable definition".to_owned());
            };
            assert!(!signature.is_empty());
            match case {
                Shape::Creature(c) => {
                    assert_eq!(
                        binding.program(),
                        &c.definition,
                        "compiled face has an extra or missing characteristic or ability"
                    );
                    ("verified", verify_creature(&src, named, &c))
                }
                Shape::Land(l) => {
                    assert_eq!(
                        binding.program(),
                        &l.definition,
                        "compiled basic land is not exactly its source definition"
                    );
                    let evidence = verify_land(&src, named, &l);
                    if l.snow {
                        (
                            "partial",
                            format!(
                                "{evidence}; snow-source mana provenance and snow-cost payment unverified"
                            ),
                        )
                    } else {
                        ("verified", evidence)
                    }
                }
            }
        }));
        let (status, detail) = match result {
            Ok(r) => r,
            Err(reason) => {
                if std::env::var_os("SPELLWAKE_AUDIT_FAIL_FAST").is_some() {
                    eprintln!("First failing card: {named}");
                    std::panic::resume_unwind(reason);
                }
                failed.push(named.clone());
                (
                    "failed",
                    reason
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| reason.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "fixture failed".into()),
                )
            }
        };
        let mut detail = detail;
        if status == "verified"
            && let Ok(sources) = inbound_name_references(&raw, &printings)
            && !sources.is_empty()
        {
            write!(
                detail,
                "; printed incoming name references from {}",
                sources.join(" | ")
            )
            .unwrap();
        }
        if status == "verified" {
            verified += 1;
        }
        let row = format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            id,
            tsv(named),
            family,
            status,
            tsv(&detail),
            raw["id"].as_str().unwrap(),
            signature,
            inventory.compressed_sha256,
            registry.coverage().registry_version,
            vocabulary.id(),
            vocabulary.rules_revision(),
            "ordinary-identity-v2"
        );
        assert!(
            rows.insert(id.to_owned(), row).is_none(),
            "duplicate Oracle row {id}"
        );
    }
    assert!(
        rows.len() > 100 && verified > 0,
        "no meaningful audited inventory"
    );
    let mut text="oracle_id\tname\tshape\tstatus\tevidence_or_reason\tsource_printing_id\tsemantic_sha256\tcatalogue_sha256\tregistry_version\tcreature_vocabulary_id\trules_revision\taudit_version\n".to_owned();
    for row in rows.values() {
        writeln!(text, "{row}").unwrap();
    }
    publish(&path, &text);
    println!(
        "ordinary-card audit: {} source candidates, {} verified, {} failed. {}",
        rows.len(),
        verified,
        failed.len(),
        path.display()
    );
    assert!(failed.is_empty(), "card games failed: {failed:?}");
}
