//! Mana sources for the current keyword card pool.
use super::*;
use spellwake_engine::{
    land::LandType,
    mana::{ManaAbility, ManaAbilityId, ManaOutput},
};

pub(super) fn compile(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal"
        || face.name != card.name
        || face
            .mana_cost
            .as_deref()
            .is_some_and(|cost| !cost.is_empty())
    {
        return None;
    }
    let line = face.type_line.as_deref()?;
    let (types, subtype) = line.split_once(" — ").unwrap_or((line, ""));
    let snow = match types {
        "Basic Land" => false,
        "Basic Snow Land" => true,
        _ if profile == Profile::Current => return nonbasic(face, types, subtype),
        _ => return None,
    };
    let text = without_reminder(face.oracle_text.as_deref().unwrap_or(""))?;
    let mut land = match (subtype, text.trim()) {
        ("Plains", "") => Program::land([LandType::Plains].into(), vec![]),
        ("Island", "") => Program::land([LandType::Island].into(), vec![]),
        ("Swamp", "") => Program::land([LandType::Swamp].into(), vec![]),
        ("Mountain", "") => Program::land([LandType::Mountain].into(), vec![]),
        ("Forest", "") => Program::land([LandType::Forest].into(), vec![]),
        ("", "{T}: Add {C}.") => Program::land(
            BTreeSet::new(),
            vec![ManaAbility {
                id: ManaAbilityId(0),
                output: ManaOutput::Fixed([(ManaType::Colourless, 1.into())].into()),
            }],
        ),
        _ => return None,
    };
    land.supertypes.insert(Supertype::Basic);
    if snow {
        land.supertypes.insert(Supertype::Snow);
    }
    Some(land)
}

fn color(symbol: &str) -> Option<ManaType> {
    Some(match symbol {
        "{W}" => ManaType::White,
        "{U}" => ManaType::Blue,
        "{B}" => ManaType::Black,
        "{R}" => ManaType::Red,
        "{G}" => ManaType::Green,
        "{C}" => ManaType::Colourless,
        _ => return None,
    })
}

pub(super) fn mana_output(clause: &str) -> Option<ManaOutput> {
    if clause.starts_with('{') && !clause.contains(' ') {
        let mut amounts = BTreeMap::<_, i64>::new();
        for symbol in clause.split_inclusive('}') {
            *amounts.entry(color(symbol)?).or_default() += 1;
        }
        return Some(ManaOutput::Fixed(
            amounts
                .into_iter()
                .map(|(color, count)| (color, count.into()))
                .collect(),
        ));
    }
    if let Some(amount) = match clause {
        "one mana of any color" => Some(1),
        "two mana of any one color" => Some(2),
        "three mana of any one color" => Some(3),
        _ => None,
    } {
        return Some(ManaOutput::ChooseColor {
            amount: amount.into(),
            colors: ManaType::ALL
                .into_iter()
                .filter(|c| *c != ManaType::Colourless)
                .collect(),
        });
    }
    let simplified = clause.replace(", or ", ", ").replace(" or ", ", ");
    let symbols: Vec<_> = simplified.split(", ").collect();
    let colors: BTreeSet<_> = symbols.iter().map(|s| color(s)).collect::<Option<_>>()?;
    if !clause.contains(" or ")
        || colors.contains(&ManaType::Colourless)
        || colors.len() != symbols.len()
        || colors.len() < 2
    {
        return None;
    }
    Some(ManaOutput::ChooseColor {
        amount: 1.into(),
        colors,
    })
}
fn land_type(word: &str) -> Option<LandType> {
    LandType::ALL
        .into_iter()
        .find(|value| value.rules_label() == word)
}
fn entry_rule(line: &str, source_name: &str) -> Option<spellwake_engine::land::EntryTap> {
    use spellwake_engine::land::EntryTap;
    let clause = line.strip_prefix("This land enters tapped").or_else(|| {
        line.strip_prefix(source_name)?
            .strip_prefix(" enters tapped")
    })?;
    Some(match clause.strip_suffix('.')? {
        "" => EntryTap::Always,
        " unless you control two or more other lands" => EntryTap::UnlessTwoOtherLands,
        " unless you control two or fewer other lands" => EntryTap::UnlessTwoOrFewerOtherLands,
        " unless you control two or more basic lands" => EntryTap::UnlessTwoBasicLands,
        " unless you have two or more opponents" => EntryTap::UnlessTwoOpponents,
        " unless your opponents control eight or more lands" => EntryTap::UnlessOpponentLandsEight,
        " unless a player has 13 or less life" => EntryTap::UnlessPlayerLifeThirteenOrLess,
        clause => {
            let (a, b) = clause
                .strip_prefix(" unless you control ")?
                .split_once(" or ")?;
            let basic = |s: &str| {
                land_type(s.strip_prefix("a ").or_else(|| s.strip_prefix("an "))?)?.basic()
            };
            EntryTap::UnlessLandType(basic(a)?, basic(b)?)
        }
    })
}
fn land_instruction(text: &str) -> Option<Instruction> {
    use spellwake_engine::program::ResolvingPlayer;
    let player = ResolvingPlayer::Controller;
    let amount = spellwake_engine::program::ValueExpr::Literal(1.into());
    Some(match text {
        "return a land you control to its owner's hand." => {
            crate::keyword_creatures::oracle_lowering::toolbox_emit::return_controlled_permanents(
                spellwake_engine::program::AffectedPlayers::One(ResolvingPlayer::Controller),
                amount,
                spellwake_engine::program::PermanentTypeFilter {
                    any_types: Default::default(),
                    required_types: [CardType::Land].into(),
                    forbidden_types: BTreeSet::new(),
                    object_filter: None,
                },
            )
        }
        "you gain 1 life." => crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
            spellwake_engine::toolbox::PlayerSelector::Resolving(
                spellwake_engine::program::ResolvingPlayer::Controller,
            ),
            spellwake_engine::program::ValueExpr::literal(1),
        ),
        "surveil 1." | "Surveil 1." => Instruction::Surveil { player, amount },
        "scry 1." | "Scry 1." => Instruction::Scry { player, amount },
        "Draw a card." => {
            crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(amount)
        }
        _ => return None,
    })
}
fn nonbasic(face: &CatalogFace, types: &str, subtype: &str) -> Option<Program> {
    use spellwake_engine::program::{ProgramKind, TriggerGrouping};
    let legendary = types.starts_with("Legendary ");
    let (artifact, snow) = match types.strip_prefix("Legendary ").unwrap_or(types) {
        "Artifact Land" => (true, false),
        "Snow Land" => (false, true),
        "Land" => (false, false),
        _ => return None,
    };
    let subtypes = subtype
        .split_whitespace()
        .map(land_type)
        .collect::<Option<BTreeSet<_>>>()?;
    let mut land = Program::land(subtypes, vec![]);
    if legendary {
        land.supertypes.insert(Supertype::Legendary);
    }
    if artifact {
        land.card_types.insert(CardType::Artifact);
    }
    if snow {
        land.supertypes.insert(Supertype::Snow);
    }
    let text = without_reminder(face.oracle_text.as_deref().unwrap_or(""))?;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(rule) = entry_rule(line, &face.name) {
            let ProgramKind::Permanent(permanent) = &mut land.kind else {
                return None;
            };
            if permanent.entry_tap.replace(rule).is_some() {
                return None;
            }
        } else if line == "You have no maximum hand size." {
            reductions::no_maximum_hand_size(&mut land)?;
        } else if line == "Indestructible" {
            land.printed_keywords.insert(KeywordAbility::Indestructible);
        } else if line == "Phasing" {
            if !land.printed_keywords.insert(KeywordAbility::Phasing) {
                return None;
            }
        } else if let Some(clause) = line
            .strip_prefix("{T}: Add ")
            .and_then(|s| s.strip_suffix('.'))
        {
            land.mana.push(ManaAbility {
                id: ManaAbilityId(land.mana.len().try_into().ok()?),
                output: mana_output(clause)?,
            });
        } else if let Some(cost) = line.strip_suffix(", {T}: Target creature can't be blocked this turn.") {
            land.activated.push(ActivatedAbility {
                id: ActivatedAbilityId(land.activated.len().try_into().ok()?),
                mana_cost: fixed_cost(cost)?,
                source_cost: spellwake_engine::activated::SourceCost::Tap,
                body: ExecutableBody::new(BodyTarget::SingleCreature, vec![super::static_grants::unblockable_effect()]),
            });
        } else if let Some(effect) = line.strip_prefix("When this land enters, ") {
            land.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(land.triggered.len().try_into().ok()?),
                condition: spellwake_engine::program::TriggerSpec::enters(spellwake_engine::program::ObjectFilter::self_object()),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(BodyTarget::None, vec![land_instruction(effect)?]),
            });
        } else if line == "{T}: Put a +1/+1 counter on target colorless creature that entered this turn." || line == "{T}: Choose target commander that entered this turn. Put a +1/+1 counter on it if it's a creature and a loyalty counter on it if it's a planeswalker." {
            use spellwake_engine::program::{ObjectColorFilter, ObjectFilter, PermanentTargetAction};
            let (filter, op) = if line.starts_with("{T}: Choose") {
                (ObjectFilter::any().commander().entered_this_turn(), Instruction::AffectPermanentTargets { minimum: 1, maximum: 1, action: PermanentTargetAction::PutCountersByCurrentType })
            } else {
                (ObjectFilter::creature().with_color(ObjectColorFilter::Colorless).entered_this_turn(), crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(spellwake_engine::effects::CounterKind::PlusOne, spellwake_engine::program::ValueExpr::literal(1)))
            };
            land.activated.push(ActivatedAbility {id:ActivatedAbilityId(land.activated.len().try_into().ok()?),mana_cost:ManaCost::free(),
                source_cost:spellwake_engine::activated::SourceCost::Tap,body:ExecutableBody::new(BodyTarget::FilteredPermanent(filter),vec![op])});
        } else if let Some(price) = line.strip_suffix(", {T}, Pay life equal to the number of colors in your commanders' color identity: Draw a card.") {
            land.activated.push(ActivatedAbility {
                id: ActivatedAbilityId(land.activated.len().try_into().ok()?),
                mana_cost: fixed_cost(price)?,
                source_cost: spellwake_engine::activated::SourceCost::PayLife {
                    amount: spellwake_engine::activated::LifeCostAmount::CommanderColors,
                    tap_source: true,
                    sacrifice: None,
                },
                body: ExecutableBody::new(BodyTarget::None, vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(spellwake_engine::program::ValueExpr::Literal(1.into()))]),
            });
        } else if let Some(cost) = line.strip_prefix("Basic landcycling ") {
            land.activated.push(ActivatedAbility::typecycling(ActivatedAbilityId(land.activated.len().try_into().ok()?),
                fixed_cost(cost)?,spellwake_engine::program::LibrarySearch::to_hand(spellwake_engine::program::SearchCardMatcher::BasicLand)));
        } else if let Some(cost) = line.strip_prefix("Cycling ") {
            land.activated.push(ActivatedAbility::cycling(
                ActivatedAbilityId(land.activated.len().try_into().ok()?),
                fixed_cost(cost)?,
            ));
        } else if let Some((cost, rules)) = line.split_once(", {T}: ")
            && let Some((lost, prohibited))=rules.strip_prefix("Until end of turn, creatures your opponents control lose ")
                .and_then(|wording|wording.strip_suffix('.')).and_then(|wording|wording.split_once(" and can't have "))
        {
            let keywords=lost.split(" and ").map(super::static_keyword).collect::<Option<BTreeSet<_>>>()?;
            let prohibited=prohibited.split(" or ").map(super::static_keyword).collect::<Option<BTreeSet<_>>>()?;
            if prohibited!=keywords {return None;}
            land.activated.push(ActivatedAbility {id:ActivatedAbilityId(land.activated.len().try_into().ok()?),mana_cost:fixed_cost(cost)?,
                source_cost:spellwake_engine::activated::SourceCost::Tap,
                body: ExecutableBody::new(BodyTarget::None, vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(spellwake_engine::program::EffectSubject::Matching(spellwake_engine::program::ObjectFilter::creature().opposing()), spellwake_engine::program::ValueExpr::literal(0), spellwake_engine::program::ValueExpr::literal(0), std::collections::BTreeSet::new(), keywords)])});
        } else if let Some((cost, effect)) = line.split_once(", {T}: ") {
            land.activated.push(ActivatedAbility {
                id: ActivatedAbilityId(land.activated.len().try_into().ok()?),
                mana_cost: fixed_cost(cost)?,
                source_cost: spellwake_engine::activated::SourceCost::Tap,
                body: ExecutableBody::new(BodyTarget::None, vec![land_instruction(effect)?]),
            });
        } else {
            return None;
        }
    }
    Some(land)
}
