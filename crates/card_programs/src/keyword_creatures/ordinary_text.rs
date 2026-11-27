//! Shared parsing of literal amounts and printed creature token creation.
use super::*;
use spellwake_engine::{
    characteristics::Color,
    program::Instruction,
    token::{TokenCreation, TokenDefinition, TokenName, TokenSubtype},
};

pub(super) fn amount(s: &str) -> Option<i64> {
    match s {
        "a" | "an" | "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        "twenty" => Some(20),
        n if n.bytes().all(|b| b.is_ascii_digit()) => n.parse::<i64>().ok().filter(|&n| n > 0),
        _ => None,
    }
}

pub(super) fn amass(_text: &str, _vocabulary: &CreatureTypeVocabulary) -> Option<[Instruction; 2]> {
    None
}

struct TokenGroup<'a> {
    quantity: i64,
    power: i64,
    toughness: i64,
    description: &'a str,
}

fn group(text: &str) -> Option<TokenGroup<'_>> {
    let (quantity, rest) = text.split_once(' ')?;
    let (size, description) = rest.split_once(' ')?;
    let (power, toughness) = size.split_once('/')?;
    Some(TokenGroup {
        quantity: amount(quantity)?,
        power: super::number(Some(power))?,
        toughness: super::number(Some(toughness))?,
        description,
    })
}

pub(super) fn create_tokens(
    mut text: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Instruction> {
    if let Some(description) = text
        .strip_prefix("an X/X ")
        .and_then(|s| s.strip_suffix(", where X is the number of creatures you control"))
    {
        let mut spec = creation(group(&format!("a 0/0 {description}"))?, vocabulary)?;
        spec.base_size = Some(spellwake_engine::token::TokenSize::AtCreation(
            spellwake_engine::program::ValueExpr::controlled_type(CardType::Creature),
        ));
        return Some(Instruction::CreateTokens {
            creations: vec![spec],
        });
    }
    if let Some(printed) = text
        .strip_prefix("a tapped ")
        .and_then(|s| s.strip_suffix(" for each instant and sorcery card in your graveyard"))
    {
        let mut spec = creation(group(&format!("a {printed}"))?, vocabulary)?;
        spec.enters_tapped = true;
        spec.amount = spellwake_engine::program::ValueExpr::count_graveyard(
            spellwake_engine::program::ResolvingPlayer::Controller,
            spellwake_engine::program::ObjectFilter::instant_or_sorcery(),
        );
        return Some(Instruction::CreateTokens {
            creations: vec![spec],
        });
    }
    if let Some((name, description)) = text.split_once(", a legendary ") {
        let mut spec = creation(group(&format!("a {description}"))?, vocabulary)?;
        let mut program = spec.definition.program().clone();
        program.supertypes.insert(Supertype::Legendary);
        spec.definition = TokenDefinition::new(
            program,
            TokenName::Specified(EnglishCardName::new(name.to_owned()).ok()?),
            vocabulary,
        )
        .ok()?;
        return Some(Instruction::CreateTokens {
            creations: vec![spec],
        });
    }
    if let Some((quantity, kind)) = text
        .strip_suffix(" tokens")
        .or_else(|| text.strip_suffix(" token"))
        .and_then(|words| words.split_once(' '))
        && let Some(kind) = spellwake_engine::program::ArtifactSubtype::from_wire_label(kind)
    {
        let mut spec = predefined_artifact(kind, vocabulary)?;
        spec.amount = spellwake_engine::program::ValueExpr::Literal(amount(quantity)?.into());
        return Some(Instruction::CreateTokens {
            creations: vec![spec],
        });
    }
    if let Some(description) = text
        .strip_suffix(". They have \"Sacrifice this token: Add {C}.\"")
        .or_else(|| text.strip_suffix(". It has \"Sacrifice this token: Add {C}.\""))
    {
        let spec = creation(group(description)?, vocabulary)?;
        let mut program = spec.definition.program().clone();
        add_sacrifice_mana(
            &mut program,
            false,
            spellwake_engine::mana::ManaOutput::Fixed([(ManaType::Colourless, 1.into())].into()),
        );
        return Some(Instruction::CreateTokens {
            creations: vec![TokenCreation {
                definition: TokenDefinition::new(
                    program,
                    spec.definition.name().clone(),
                    vocabulary,
                )
                .ok()?,
                ..spec
            }],
        });
    }
    if let Some(description) = text
        .strip_prefix("X ")
        .and_then(|s| s.strip_suffix(" tokens, where X is the sacrificed creature's power"))
    {
        let mut spec = creation(group(&format!("a {description} token"))?, vocabulary)?;
        spec.amount = spellwake_engine::program::ValueExpr::object_stat(
            spellwake_engine::program::ValueObjectRef::SacrificedCreature,
            spellwake_engine::program::ObjectStatKind::Power,
        );
        return Some(Instruction::CreateTokens {
            creations: vec![spec],
        });
    }
    if let Some((description, count)) = text
        .strip_prefix("X ")
        .and_then(|s| s.split_once(", where X is the number of "))
    {
        let subtype = count.strip_suffix(" you control")?.strip_suffix('s')?;
        let singular = description.strip_suffix(" tokens")?;
        let template = format!("a {singular} token");
        let mut spec = creation(group(&template)?, vocabulary)?;
        spec.amount = spellwake_engine::program::ValueExpr::controlled_subtype(
            vocabulary.resolve_label(subtype)?,
        );
        return Some(Instruction::CreateTokens {
            creations: vec![spec],
        });
    }
    let mut creations = Vec::new();
    loop {
        // "And" can join colors, keywords, or complete token groups.
        let split = text.match_indices(" and ").find_map(|(index, join)| {
            let next = &text[index + join.len()..];
            group(next).map(|_| (&text[..index], next))
        });
        let (current, next) = split.map_or((text, None), |(left, right)| (left, Some(right)));
        creations.push(creation(group(current)?, vocabulary)?);
        let Some(next) = next else {
            return Some(Instruction::CreateTokens { creations });
        };
        text = next;
    }
}

fn add_sacrifice_mana(
    program: &mut Program,
    tap: bool,
    output: spellwake_engine::mana::ManaOutput,
) {
    use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaSacrificeCost};
    program.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: output.sacrifice(ManaSacrificeCost::Source { tap }),
    });
}

fn predefined_artifact(
    kind: spellwake_engine::program::ArtifactSubtype,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<TokenCreation> {
    use spellwake_engine::{activated::SourceCost, mana::ManaOutput, program::ArtifactSubtype};
    let mut program = Program::artifact(None, vec![]);
    match kind {
        ArtifactSubtype::Clue | ArtifactSubtype::Food => {
            let (source_cost, instruction) = match kind {
                ArtifactSubtype::Clue => (
                    SourceCost::Sacrifice,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ),
                ),
                _ => (
                    SourceCost::TapSacrifice,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::literal(3),
                    ),
                ),
            };
            program.activated.push(ActivatedAbility {
                id: ActivatedAbilityId(0),
                mana_cost: fixed_cost("{2}")?,
                source_cost,
                body: ExecutableBody::new(BodyTarget::None, vec![instruction]),
            });
        }
        ArtifactSubtype::Gold | ArtifactSubtype::Treasure => {
            let colors = [
                ManaType::White,
                ManaType::Blue,
                ManaType::Black,
                ManaType::Red,
                ManaType::Green,
            ]
            .into();
            add_sacrifice_mana(
                &mut program,
                kind == ArtifactSubtype::Treasure,
                ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors,
                },
            );
        }
        _ => return None,
    }
    program.artifact_subtypes.insert(kind);
    let name = TokenName::Default {
        ordered_subtypes: vec![TokenSubtype::Artifact(kind)],
    };
    Some(TokenCreation::one(
        TokenDefinition::new(program, name, vocabulary).ok()?,
    ))
}

fn creation(group: TokenGroup<'_>, vocabulary: &CreatureTypeVocabulary) -> Option<TokenCreation> {
    let TokenGroup {
        quantity,
        power,
        toughness,
        description: rest,
    } = group;
    let (rest, death_creation) = match rest.split_once(" and \"When this token dies, create ") {
        Some((rest, effect)) => (rest, Some(effect.strip_suffix('"')?)),
        None => (rest, None),
    };
    let (rest, activation) = match rest.split_once(" and '") {
        Some((rest, activation)) => (rest, Some(activation.strip_suffix('\'')?)),
        None => (rest, None),
    };
    let (printed, keyword) = rest
        .split_once(" with ")
        .map_or((rest, None), |(p, k)| (p, Some(k)));
    let printed = printed
        .strip_suffix(" creature token")
        .or_else(|| printed.strip_suffix(" creature tokens"))?;
    let (printed, enchantment) = printed
        .strip_suffix(" enchantment")
        .map_or((printed, false), |types| (types, true));
    let (printed, artifact) = printed
        .strip_suffix(" artifact")
        .map_or((printed, false), |types| (types, true));
    let mut colors = BTreeSet::new();
    let mut subtypes = Vec::new();
    let mut words = printed.split_whitespace().peekable();
    let explicitly_colorless = words.peek() == Some(&"colorless");
    if explicitly_colorless {
        words.next();
    }
    loop {
        let color = match words.peek().copied() {
            Some("white") => Color::White,
            Some("blue") => Color::Blue,
            Some("black") => Color::Black,
            Some("red") => Color::Red,
            Some("green") => Color::Green,
            _ => break,
        };
        colors.insert(color);
        words.next();
        if words.peek() == Some(&"and") {
            words.next();
        }
    }
    if colors.is_empty() != explicitly_colorless {
        return None;
    }
    for word in words {
        subtypes.push(vocabulary.resolve_label(word)?);
    }
    if subtypes.is_empty() {
        return None;
    }
    let mut definition = Program::creature(
        None,
        power.into(),
        toughness.into(),
        subtypes.iter().copied().collect(),
    );
    if enchantment {
        definition.card_types.insert(CardType::Enchantment);
    }
    if artifact {
        definition.card_types.insert(CardType::Artifact);
    }
    if keyword == Some("\"This token can't block.\"") {
        use spellwake_engine::characteristics::{
            BlockingRestriction, ControllerSelector, PermanentSelector, StaticPart, StaticParts,
        };
        army_grants::add_battlefield_ability(
            &mut definition,
            PermanentSelector {
                controller: ControllerSelector::SourceItself,
                ..Default::default()
            },
            StaticParts(vec![StaticPart::BlockingRestriction(
                BlockingRestriction::CannotBlock,
            )]),
        )?;
    } else if let Some(keyword) = keyword {
        for word in keyword
            .split(", ")
            .flat_map(|segment| segment.split(" and "))
            .map(|word| word.strip_prefix("and ").unwrap_or(word))
        {
            let key = match word {
                "defender" => KeywordAbility::Defender,
                "flying" => KeywordAbility::Flying,
                "haste" => KeywordAbility::Haste,
                "indestructible" => KeywordAbility::Indestructible,
                "vigilance" => KeywordAbility::Vigilance,
                "lifelink" => KeywordAbility::Lifelink,
                "deathtouch" => KeywordAbility::Deathtouch,
                "trample" => KeywordAbility::Trample,
                "menace" => KeywordAbility::Menace,
                "reach" => KeywordAbility::Reach,
                "first strike" => KeywordAbility::FirstStrike,
                _ => return None,
            };
            definition.printed_keywords.insert(key);
        }
    }
    if let Some(activation) = activation {
        let (cost, effect) = activation.split_once(": ")?;
        let effect = effect.strip_prefix("This token ")?;
        let normalized = format!("{cost}: This creature {effect}");
        let (mana_cost, source_cost, body, restriction) =
            super::source_boost::parse(&normalized, "CARDNAME", "Creature", vocabulary)?;
        if restriction.is_some() {
            return None;
        }
        definition.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost,
            source_cost,
            body,
        });
    }
    if let Some(effect) = death_creation {
        use spellwake_engine::program::TriggerGrouping;
        let nested = effect
            .strip_suffix('.')
            .or_else(|| effect.ends_with(".'").then_some(effect))?;
        definition.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::dies(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, vec![create_tokens(nested, vocabulary)?]),
        });
    }
    definition.effect_defined_colors = Some(colors);
    let name = TokenName::Default {
        ordered_subtypes: subtypes.into_iter().map(TokenSubtype::Creature).collect(),
    };
    let mut creation = TokenCreation::one(TokenDefinition::new(definition, name, vocabulary).ok()?);
    creation.amount = spellwake_engine::program::ValueExpr::Literal(quantity.into());
    Some(creation)
}
