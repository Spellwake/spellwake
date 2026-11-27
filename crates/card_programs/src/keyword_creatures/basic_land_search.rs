//! Qualified complete library-search bodies and permanent abilities.
use super::{InventoryCard, Profile, Program, fixed_cost, ordinary_creature};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::KeywordAbility,
    land::LandType,
    mana::{ManaCost, ManaType},
    program::{
        BodyTarget, CardType, ExecutableBody, Instruction, LibrarySearch, ResolvingPlayer,
        SearchCardMatcher, SearchCount, SearchDestination, SearchManaValue, SearchPairConstraint,
        SearchSource, TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
    },
};
use std::collections::BTreeSet;

/// Optional search performed by the controller captured before the target was exiled.
pub(super) fn exiled_target_controller_search() -> [Instruction; 2] {
    [
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::ExiledTargetController,
            instructions: 1,
        },
        Instruction::SearchLibrary {
            filter: Box::new(
                LibrarySearch::to_battlefield(SearchCardMatcher::BasicLand, true)
                    .with_searcher(ResolvingPlayer::ExiledTargetController),
            ),
        },
    ]
}

fn parse_land_subtype(word: &str) -> Option<LandType> {
    LandType::ALL
        .into_iter()
        .find(|kind| kind.wire_label().eq_ignore_ascii_case(word.trim()))
}

fn parse_card_type(word: &str) -> Option<CardType> {
    match word.trim() {
        "artifact" => Some(CardType::Artifact),
        "battle" => Some(CardType::Battle),
        "creature" => Some(CardType::Creature),
        "enchantment" => Some(CardType::Enchantment),
        "instant" => Some(CardType::Instant),
        "land" => Some(CardType::Land),
        "planeswalker" => Some(CardType::Planeswalker),
        "sorcery" => Some(CardType::Sorcery),
        _ => None,
    }
}

fn parse_search_count(text: &str) -> Option<(SearchCount, &str, bool)> {
    if let Some(rest) = text.strip_prefix("up to x ") {
        return Some((SearchCount::UpToPlayersWithMoreLands, rest, true));
    }
    if let Some(rest) = text.strip_prefix("up to ") {
        let (word, tail) = rest.split_once(' ')?;
        let count = u8::try_from(super::ordinary_text::amount(word)?).ok()?;
        return (count > 0).then_some((SearchCount::UpTo(count), tail, count > 1));
    }
    if let Some(rest) = text.strip_prefix("an ").or_else(|| text.strip_prefix("a ")) {
        return Some((SearchCount::UpTo(1), rest, false));
    }
    let (word, tail) = text.split_once(' ')?;
    let count = u8::try_from(super::ordinary_text::amount(word)?).ok()?;
    (count > 1).then_some((SearchCount::UpTo(count), tail, true))
}

fn parse_list_items(text: &str) -> Vec<&str> {
    text.split(',')
        .flat_map(|part| part.split(" or "))
        .map(|part| part.trim().trim_start_matches("or ").trim())
        .filter(|part| !part.is_empty())
        .collect()
}

fn parse_matcher_and_mv(
    mut subject: &str,
) -> Option<(
    SearchCardMatcher,
    Option<SearchManaValue>,
    Option<SearchPairConstraint>,
)> {
    let mut pair_constraint = None;
    if let Some(prefix) = subject.strip_suffix(" that share a land type") {
        pair_constraint = Some(SearchPairConstraint::ShareLandType);
        subject = prefix;
    }
    let mut mana_value = None;
    if let Some((prefix, mv_clause)) = subject.split_once(" with mana value ") {
        subject = prefix;
        mana_value = if let Some(n) = mv_clause.strip_suffix(" or greater") {
            Some(SearchManaValue::AtLeast(n.trim().parse().ok()?))
        } else if let Some(n) = mv_clause.strip_suffix(" or less") {
            Some(SearchManaValue::AtMost(n.trim().parse().ok()?))
        } else {
            Some(SearchManaValue::Exact(
                mv_clause.trim().parse::<i64>().ok()?.into(),
            ))
        };
    }
    // Pair of distinct land subtypes: "forest card and a plains card"
    if let Some((left, right)) = subject.split_once(" card and a ")
        && let Some(right) = right.strip_suffix(" card")
        && let (Some(first), Some(second)) = (parse_land_subtype(left), parse_land_subtype(right))
        && first != second
    {
        return Some((
            SearchCardMatcher::LandSubtypes {
                subtypes: [first, second].into(),
                require_land_type: true,
            },
            mana_value,
            Some(SearchPairConstraint::DistinctLandTypes(first, second)),
        ));
    }
    if matches!(subject, "card" | "cards") {
        return Some((SearchCardMatcher::Any, mana_value, pair_constraint));
    }
    let core = subject
        .strip_suffix(" cards")
        .or_else(|| subject.strip_suffix(" card"))?;
    let matcher = match core {
        "basic land" => SearchCardMatcher::BasicLand,
        "nonland" => SearchCardMatcher::Nonland,
        "equipment" => SearchCardMatcher::Equipment,
        other => {
            let items = parse_list_items(other);
            if items.is_empty() {
                return None;
            }
            if let Some(subtypes) = items
                .iter()
                .map(|item| parse_land_subtype(item))
                .collect::<Option<BTreeSet<_>>>()
            {
                SearchCardMatcher::LandSubtypes {
                    subtypes,
                    require_land_type: true,
                }
            } else {
                let types = items
                    .iter()
                    .map(|item| parse_card_type(item))
                    .collect::<Option<BTreeSet<_>>>()?;
                SearchCardMatcher::CardTypes(types)
            }
        }
    };
    Some((matcher, mana_value, pair_constraint))
}

/// Compile composable library-search clauses, including their required shuffle.
pub(super) fn body(text: &str) -> Option<ExecutableBody> {
    let text = text.trim().to_ascii_lowercase();
    let (source, rest) = if let Some(rest) = text.strip_prefix("search your library for ") {
        (SearchSource::OwnLibrary, rest)
    } else {
        let rest = text.strip_prefix("search target opponent's library for ")?;
        (SearchSource::TargetOpponentLibrary, rest)
    };

    // Split off morbid battlefield-override suffix when present.
    let (rest, morbid_battlefield) = match rest.strip_suffix(
        " morbid — you may put that card onto the battlefield instead of putting it into your hand if a creature died this turn.",
    ) {
        Some(prefix) => (prefix.trim(), true),
        None => (rest, false),
    };

    // Target-opponent library search ("...and put that card onto the battlefield under your control. then that player shuffles.")
    if source == SearchSource::TargetOpponentLibrary {
        let subject = rest.strip_suffix(
            " and put that card onto the battlefield under your control. then that player shuffles.",
        )?;
        let (count, subject, _) = parse_search_count(subject)?;
        if count != SearchCount::UpTo(1) {
            return None;
        }
        let (matcher, mana_value, pair_constraint) = parse_matcher_and_mv(subject)?;
        let mut search = LibrarySearch::to_battlefield(matcher, false)
            .with_source(SearchSource::TargetOpponentLibrary);
        if let Some(mv) = mana_value {
            search = search.with_mana_value(mv);
        }
        if let Some(pair) = pair_constraint {
            search = search.with_pair_constraint(pair);
        }
        return Some(ExecutableBody::new(
            BodyTarget::SINGLE_OPPONENT,
            vec![Instruction::SearchLibrary {
                filter: Box::new(search),
            }],
        ));
    }

    // Special two-card hand-and-graveyard split ("two cards. put one into your hand and the other into your graveyard. then shuffle.")
    if let Some(subject) = rest
        .strip_suffix(". put one into your hand and the other into your graveyard. then shuffle.")
    {
        let (count, subject, _) = parse_search_count(subject)?;
        let (matcher, mana_value, pair_constraint) = parse_matcher_and_mv(subject)?;
        if count != SearchCount::UpTo(2)
            || matcher != SearchCardMatcher::Any
            || mana_value.is_some()
            || pair_constraint.is_some()
        {
            return None;
        }
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::SearchLibrary {
                filter: Box::new(
                    LibrarySearch::to_hand(SearchCardMatcher::Any)
                        .with_count(SearchCount::UpTo(2))
                        .with_destination(SearchDestination::SplitHandAndGraveyard)
                        .with_reveal(false),
                ),
            }],
        ));
    }

    // Variable count: "up to x <matcher>, where x is the number of players who control more lands than you. <tail>"
    let (subject_part, tail, players_more_lands) = if let Some((subj, after)) =
        rest.split_once(", where x is the number of players who control more lands than you. ")
    {
        (subj, after, true)
    } else {
        let idx = rest.find(", reveal ").or_else(|| rest.find(", put "))?;
        (&rest[..idx], &rest[idx + 2..], false)
    };

    let (mut count, raw_subject, _) = parse_search_count(subject_part)?;
    if players_more_lands {
        if count != SearchCount::UpToPlayersWithMoreLands {
            return None;
        }
    } else if count == SearchCount::UpToPlayersWithMoreLands {
        return None;
    }

    let (mut matcher, mana_value, pair_constraint) = parse_matcher_and_mv(raw_subject)?;
    // Pair of distinct land subtypes ("a forest card and a plains card") searches up to 2 cards.
    if matches!(
        pair_constraint,
        Some(SearchPairConstraint::DistinctLandTypes(_, _))
    ) {
        count = SearchCount::UpTo(2);
    }
    // Land-subtype searches to hand match the land subtype directly (including nonland kindred if any).
    let (destination, reveal) = match tail {
        "reveal it, put it into your hand, then shuffle."
        | "reveal that card, put it into your hand, then shuffle."
        | "reveal them, put them into your hand, then shuffle."
        | "reveal those cards, put them into your hand, then shuffle." => {
            let dest = if morbid_battlefield {
                SearchDestination::HandOrBattlefieldIfCreatureDied
            } else {
                SearchDestination::Hand
            };
            (dest, true)
        }
        "put it into your hand, then shuffle."
        | "put that card into your hand, then shuffle."
        | "put them into your hand, then shuffle."
        | "put those cards into your hand, then shuffle."
            if matcher == SearchCardMatcher::Any && mana_value.is_none() && !morbid_battlefield =>
        {
            if let SearchCount::UpTo(n) = count {
                count = SearchCount::Exact(std::num::NonZeroU8::new(n)?);
            }
            (SearchDestination::Hand, false)
        }
        "put it onto the battlefield, then shuffle."
        | "put that card onto the battlefield, then shuffle."
        | "put them onto the battlefield, then shuffle."
        | "put those cards onto the battlefield, then shuffle."
            if !morbid_battlefield =>
        {
            (
                SearchDestination::Battlefield {
                    tapped: false,
                    post_entry: spellwake_engine::program::SearchBattlefieldPostEntry::None,
                },
                false,
            )
        }
        "put it onto the battlefield tapped, then shuffle."
        | "put that card onto the battlefield tapped, then shuffle."
        | "put them onto the battlefield tapped, then shuffle."
        | "put those cards onto the battlefield tapped, then shuffle."
            if !morbid_battlefield =>
        {
            (
                SearchDestination::Battlefield {
                    tapped: true,
                    post_entry: spellwake_engine::program::SearchBattlefieldPostEntry::None,
                },
                false,
            )
        }
        "reveal it, then shuffle and put it on top."
        | "reveal it, then shuffle and put that card on top."
        | "reveal that card, then shuffle and put that card on top."
            if count == SearchCount::UpTo(1) && !morbid_battlefield =>
        {
            (SearchDestination::LibraryTop, true)
        }
        "reveal those cards, put one onto the battlefield tapped and the other into your hand, then shuffle."
        | "reveal them, put one onto the battlefield tapped and the other into your hand, then shuffle."
            if count == SearchCount::UpTo(2) && !morbid_battlefield =>
        {
            (SearchDestination::SplitBattlefieldTappedAndHand, true)
        }
        _ => return None,
    };

    if destination == SearchDestination::Hand
        && let SearchCardMatcher::LandSubtypes {
            require_land_type, ..
        } = &mut matcher
    {
        *require_land_type = false;
    }

    let mut filter = LibrarySearch {
        searcher: ResolvingPlayer::Controller,
        source,
        matcher,
        mana_value,
        count,
        pair_constraint,
        destination,
        reveal,
    };
    if players_more_lands && destination != SearchDestination::Hand {
        return None;
    }
    if filter.destination == SearchDestination::Hand
        && filter.matcher != SearchCardMatcher::Any
        && !filter.reveal
    {
        return None;
    }
    // Preserve exact required flag for unrestricted single/multi hand searches.
    if filter.matcher == SearchCardMatcher::Any
        && filter.mana_value.is_none()
        && filter.destination == SearchDestination::Hand
    {
        filter.reveal = false;
    }
    Some(ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::SearchLibrary {
            filter: Box::new(filter),
        }],
    ))
}

/// Compile the complete supported sacrifice or tap-and-sacrifice activation.
pub(super) fn activation(text: &str, id: ActivatedAbilityId) -> Option<ActivatedAbility> {
    let (cost, effect) = text.trim().split_once(':')?;
    let (mana_cost, source_cost) = match cost.trim().to_ascii_lowercase().as_str() {
        "{1}{g}, sacrifice this enchantment" => (
            ManaCost {
                generic: 1.into(),
                required: [(ManaType::Green, 1.into())].into(),
            },
            SourceCost::Sacrifice,
        ),
        "{t}, sacrifice this land" => (ManaCost::free(), SourceCost::TapSacrifice),
        "{3}, sacrifice this creature" => (ManaCost::generic(3.into()), SourceCost::Sacrifice),
        "{2}, {t}, sacrifice this artifact" => {
            (ManaCost::generic(2.into()), SourceCost::TapSacrifice)
        }
        _ => return None,
    };
    Some(ActivatedAbility {
        id,
        mana_cost,
        source_cost,
        body: body(effect)?,
    })
}

/// Compile complete normal permanents with only a supported search activation.
pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || face.name != card.name {
        return None;
    }
    let text = face.oracle_text.as_deref()?.trim();
    if face.type_line.as_deref() == Some("Land") {
        let lines = text.lines().map(str::trim).collect::<Vec<_>>();
        if let ["This land enters tapped.", "{T}: Add {C}.", activation] = lines.as_slice()
            && let Some(effect) = activation.strip_prefix("{2}, {T}, Sacrifice this land: ")
            && let Some(search) = body(effect)
            && matches!(search.instructions.as_slice(), [Instruction::SearchLibrary { filter }] if filter.pair_constraint.is_some() && filter.enters_tapped())
        {
            let mut program = Program::land(
                Default::default(),
                vec![spellwake_engine::mana::ManaAbility {
                    id: spellwake_engine::mana::ManaAbilityId(0),
                    output: spellwake_engine::mana::ManaOutput::Fixed(
                        [(ManaType::Colourless, 1.into())].into(),
                    ),
                }],
            );
            if let spellwake_engine::program::ProgramKind::Permanent(definition) = &mut program.kind
            {
                definition.entry_tap = Some(spellwake_engine::land::EntryTap::Always);
            }
            program.activated.push(ActivatedAbility {
                id: ActivatedAbilityId(0),
                mana_cost: ManaCost::generic(2.into()),
                source_cost: SourceCost::TapSacrifice,
                body: search,
            });
            program.validate(vocabulary).ok()?;
            return Some(program);
        }
    }
    if let Some((keywords, ability)) = text.split_once('\n')
        && keywords.trim() == "Trample"
        && let Some(effect) = ability
            .trim()
            .strip_prefix("Channel — {2}{G}, Discard this card: ")
        && let Some(search) = body(effect)
        && matches!(search.instructions.as_slice(), [Instruction::SearchLibrary { filter }] if filter.matcher == SearchCardMatcher::BasicLand && filter.enters_tapped())
    {
        let mut program =
            ordinary_creature(card, vocabulary, Profile::Current, Some(keywords)).ok()?;
        program.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: fixed_cost("{2}{G}")?,
            source_cost: SourceCost::HandDiscardSelf(
                spellwake_engine::activated::HandDiscardKeyword::Channel,
            ),
            body: search,
        });
        program.validate(vocabulary).ok()?;
        return Some(program);
    }
    if let Some(program) = triggered(card, vocabulary, text) {
        return Some(program);
    }
    let (entry_tapped, activation_text) = match text.strip_prefix("This land enters tapped.\n") {
        Some(text) => (true, text.trim()),
        None => (false, text),
    };
    if entry_tapped && face.type_line.as_deref()? != "Land" {
        return None;
    }
    let ability = activation(activation_text, ActivatedAbilityId(0))?;
    let cost = activation_text
        .split_once(':')?
        .0
        .trim()
        .to_ascii_lowercase();
    let mut program = match (face.type_line.as_deref()?, cost.as_str()) {
        ("Enchantment", "{1}{g}, sacrifice this enchantment") => {
            Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?))
        }
        ("Artifact Creature — Elk", "{3}, sacrifice this creature") => {
            ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?
        }
        ("Artifact", "{2}, {t}, sacrifice this artifact") => {
            Program::artifact(Some(fixed_cost(face.mana_cost.as_deref()?)?), vec![])
        }
        ("Land", "{t}, sacrifice this land")
            if face.mana_cost.as_deref().is_none_or(str::is_empty) =>
        {
            Program::land(Default::default(), vec![])
        }
        _ => return None,
    };
    if entry_tapped {
        let spellwake_engine::program::ProgramKind::Permanent(definition) = &mut program.kind
        else {
            return None;
        };
        definition.entry_tap = Some(spellwake_engine::land::EntryTap::Always);
    }
    program.activated.push(ability);
    Some(program)
}

fn triggered(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    text: &str,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let lines: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let search = |text: &str| {
        let mut body = body(text)?;
        body.instructions.insert(
            0,
            Instruction::OptionalSequence {
                chooser: ResolvingPlayer::Controller,
                instructions: 1,
            },
        );
        Some(body)
    };
    let ability = |id, condition, body| TriggeredAbility {
        id: TriggeredAbilityId(id),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body,
    };
    match (face.type_line.as_deref()?, lines.as_slice()) {
        ("Enchantment", [text])
            if *text
                == "At the beginning of your upkeep, if an opponent controls more lands than you, you may search your library for up to three basic land cards, reveal them, put them into your hand, then shuffle." =>
        {
            let mut program = Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?));
            let mut body = search(
                "Search your library for up to three basic land cards, reveal them, put them into your hand, then shuffle.",
            )?;
            body.instructions.insert(
                0,
                Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                        spellwake_engine::program::InterveningCondition::OpponentMoreLands,
                    ),
                )),
            );
            program.triggered.push(ability(
                0,
                spellwake_engine::program::TriggerSpec::step_began(
                    spellwake_engine::turn::Step::Upkeep,
                    spellwake_engine::program::PlayerRelation::Controller,
                )
                .only_if(spellwake_engine::program::TriggerCondition::OpponentHasMoreLands),
                body,
            ));
            program.validate(vocabulary).ok()?;
            Some(program)
        }
        ("Artifact Creature — Golem", [entry, death])
            if *entry
                == "When this creature enters, you may search your library for a basic land card, put that card onto the battlefield tapped, then shuffle."
                && *death == "When this creature dies, you may draw a card." =>
        {
            let mut program =
                ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
            program.triggered.push(ability(
                0,
                spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                search(entry.strip_prefix("When this creature enters, you may ")?)?,
            ));
            program.triggered.push(ability(
                1,
                spellwake_engine::program::TriggerSpec::dies(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::Controller,
                            instructions: 1,
                        },
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                            spellwake_engine::program::ValueExpr::Literal(1.into()),
                        ),
                    ],
                ),
            ));
            Some(program)
        }
        ("Enchantment", [flash, entry, scry])
            if *flash == "Flash"
                && *entry
                    == "When this enchantment enters, you may search your library for a basic land card, put it onto the battlefield tapped, then shuffle."
                && *scry == "{2}{G}, Sacrifice this enchantment: Scry 2." =>
        {
            let mut program = Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?));
            program.printed_keywords.insert(KeywordAbility::Flash);
            program.triggered.push(ability(
                0,
                spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                search(entry.strip_prefix("When this enchantment enters, you may ")?)?,
            ));
            program.activated.push(ActivatedAbility {
                id: ActivatedAbilityId(0),
                mana_cost: fixed_cost("{2}{G}")?,
                source_cost: SourceCost::Sacrifice,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::scry(
                            ResolvingPlayer::Controller,
                            spellwake_engine::program::ValueExpr::Literal(2.into()),
                        ),
                    ))],
                ),
            });
            Some(program)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_requires_exact_destination_and_complete_search() {
        assert!(body("Search your library for a basic land card, put that card onto the battlefield tapped, then shuffle.").is_some());
        assert!(body("Search your library for a basic land card, put it onto the battlefield tapped, then shuffle.").is_some());
        assert!(body("Search your library for a basic land card, put that card onto the battlefield, then shuffle.").is_some());
        // Composable clauses that were never hard-coded as exact strings now compile directly.
        assert!(body("Search your library for a Plains, Island, Swamp, or Mountain card, put it onto the battlefield tapped, then shuffle.").is_some());
        assert!(body("Search your library for an enchantment card, reveal it, put it into your hand, then shuffle.").is_some());
        assert!(body("Search your library for an Island or Swamp card, put it onto the battlefield tapped, then shuffle.").is_some());
        assert!(body("Search your library for an instant card with mana value 3 or less, reveal it, put it into your hand, then shuffle.").is_some());
        for text in [
            "Search your library for a basic land card, put that card into your hand, then shuffle.",
            "Search your library for a basic land card, put that card onto the battlefield tapped.",
            "Search your library for a basic land card, put that card into your graveyard, then shuffle.",
        ] {
            assert!(body(text).is_none(), "expected rejection for: {text}");
        }
        assert!(activation("{1}{G}, Sacrifice this enchantment: Search your library for a basic land card, put it onto the battlefield tapped, then shuffle.", ActivatedAbilityId(0)).is_some());
        assert!(activation("{2}, {T}, Sacrifice this artifact: Search your library for a basic land card, put it onto the battlefield tapped, then shuffle.", ActivatedAbilityId(0)).is_some());
    }
}
