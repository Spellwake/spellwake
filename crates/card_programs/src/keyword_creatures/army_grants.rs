//! Battlefield-wide printed bonuses on permanents with other printed abilities.
use super::{InventoryCard, Profile, fixed_cost, static_keyword, without_reminder};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    characteristics::{
        AbilityChange, ColorFilter, ControllerSelector, NumericExpression, NumericPair,
        PermanentSelector, StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
    },
    program::{CardType, Program},
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || card.name != face.name {
        return None;
    }
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut grants = Vec::new();
    let mut remaining = Vec::new();
    for line in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some((inclusive, other)) = line
            .strip_suffix(" creatures get +1/+1.")
            .and_then(|s| s.split_once(" creatures and other "))
        {
            let inclusive = vocabulary.resolve_label(inclusive)?;
            let other = vocabulary.resolve_label(other)?;
            let selector = PermanentSelector {
                required_types: [CardType::Creature].into(),
                creature_subtype: Some(inclusive),
                ..Default::default()
            };
            let bonus = StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(1.into()),
                toughness: NumericExpression::Literal(1.into()),
            });
            // The two selectors are disjoint even when a creature has both subtypes.
            grants.push((selector, vec![bonus.clone()]));
            grants.push((
                PermanentSelector {
                    required_types: [CardType::Creature].into(),
                    creature_subtype: Some(other),
                    forbidden_creature_subtype: Some(inclusive),
                    exclude_source: true,
                    ..Default::default()
                },
                vec![bonus],
            ));
        } else if let Some(effect) = parse(line, &card.name, vocabulary) {
            grants.push(effect);
        } else {
            remaining.push(line);
        }
    }
    if grants.is_empty() {
        return None;
    }
    let types = face.type_line.as_deref()?;
    let mut base = if remaining.is_empty() && matches!(types, "Enchantment" | "Artifact") {
        let cost = Some(fixed_cost(face.mana_cost.as_deref()?)?);
        if types == "Artifact" {
            Program::artifact(cost, vec![])
        } else {
            Program::enchantment(cost)
        }
    } else {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(remaining.join("\n"));
        super::compile(&reduced, vocabulary, Profile::Current).ok()?
    };
    base.permanent()?;
    for (selector, parts) in grants {
        add_battlefield_ability(&mut base, selector, StaticParts(parts))?;
    }
    Some(base)
}

/// Compile an ability whose source is the creature currently receiving the grant.
pub(super) fn granted_creature_ability(
    quoted: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticPart> {
    use spellwake_engine::{
        activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
        program::{TriggerGrouping, TriggeredAbility, TriggeredAbilityId},
    };
    if quoted
        == "{1}{U}: Exile this creature, then return it to the battlefield under your control."
    {
        return Some(StaticPart::GrantActivated(Box::new(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: fixed_cost("{1}{U}")?,
            source_cost: SourceCost::None,
            body: spellwake_engine::program::ExecutableBody::new(
                spellwake_engine::program::BodyTarget::None,
                vec![
                    spellwake_engine::program::Instruction::ExileAbilitySource,
                    spellwake_engine::program::Instruction::ReturnExiledTargetCohort {
                        next_end_step: false,
                        owner_controls: false,
                    },
                ],
            ),
        })));
    }
    if quoted.contains(": ") {
        let (mana_cost, source_cost, body, restriction) =
            super::source_boost::parse(quoted, "CARDNAME", "Creature", vocabulary)?;
        if restriction.is_some()
            || !matches!(
                source_cost,
                SourceCost::PayLife {
                    tap_source: false,
                    sacrifice: None,
                    ..
                } | SourceCost::None
                    | SourceCost::Tap
                    | SourceCost::Sacrifice
            )
        {
            return None;
        }
        return Some(StaticPart::GrantActivated(Box::new(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost,
            source_cost,
            body,
        })));
    }
    let (condition, effect) = super::ordinary_triggers::trigger(quoted, "CARDNAME", "Creature")?;
    let normalized = effect
        .strip_prefix("it deals ")
        .map(|rest| format!("this creature deals {rest}"));
    let body = super::ordinary_triggers::body(
        normalized.as_deref().unwrap_or(effect),
        "CARDNAME",
        vocabulary,
    )?;
    Some(StaticPart::GrantTrigger(Box::new(TriggeredAbility {
        id: TriggeredAbilityId(0),
        grouping: if condition
            == spellwake_engine::program::TriggerSpec::new(
                spellwake_engine::program::TriggerEvent::DealtDamage {
                    subject: spellwake_engine::program::ObjectFilter::self_object(),
                    linked_player: false,
                },
            ) {
            TriggerGrouping::OneOrMoreTogether
        } else {
            TriggerGrouping::EachOccurrence
        },
        condition,
        body,
    })))
}

/// Give each printed continuous effect its own identity, recipient set and layer history.
pub(super) fn add_battlefield_ability(
    base: &mut Program,
    selector: PermanentSelector,
    parts: StaticParts,
) -> Option<()> {
    let highest = base
        .continuous
        .iter()
        .chain(&base.additional_battlefield_abilities)
        .map(|ability| ability.id().0)
        .max()
        .unwrap_or(0);
    let ability = StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(highest.checked_add(1)?),
        selector,
        parts,
    };
    if base.continuous.is_none() {
        base.continuous = Some(ability);
    } else {
        base.additional_battlefield_abilities.push(ability);
    }
    Some(())
}

fn parse(
    line: &str,
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(PermanentSelector, Vec<StaticPart>)> {
    let line = if line.ends_with('"') {
        line
    } else {
        line.strip_suffix('.')?
    };
    use spellwake_engine::characteristics::{BlockingRestriction, CombatSelector};
    let own = || PermanentSelector {
        controller: ControllerSelector::SourceItself,
        ..Default::default()
    };
    if line.strip_prefix(name)
        == Some(" has all activated abilities of each other creature with a +1/+1 counter on it")
    {
        return Some((own(), vec![StaticPart::InheritCounteredCreatureAbilities]));
    }
    if line
        == "This creature gets +X/+0, where X is the greatest power among creature cards in your graveyard"
    {
        return Some((own(),vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::PlayerStat(
                spellwake_engine::characteristics::StaticPlayerStat::OwnGraveyardGreatestCreaturePower,
            ),
            toughness: NumericExpression::Literal(0.into()),
        })]));
    }
    if line == "This creature can't be blocked except by three or more creatures" {
        return Some((
            own(),
            vec![StaticPart::BlockingRestriction(
                BlockingRestriction::AtLeastThreeBlockers,
            )],
        ));
    }
    if line.strip_prefix(name)
        == Some(" can't be blocked unless all creatures defending player controls block it")
    {
        return Some((
            own(),
            vec![StaticPart::BlockingRestriction(
                BlockingRestriction::AllDefendingCreaturesMustBlock,
            )],
        ));
    }
    if line.strip_prefix(name) == Some(" has hexproof unless it's attacking or blocking") {
        return Some((
            PermanentSelector {
                combat_role: Some(CombatSelector::OutsideCombat),
                ..own()
            },
            vec![StaticPart::Abilities(AbilityChange::grant_keywords([
                spellwake_engine::characteristics::KeywordAbility::Hexproof,
            ]))],
        ));
    }
    if let Some((subject, grant)) = line
        .strip_suffix(" as long as two or more Equipment are attached to it")
        .and_then(|l| l.split_once(" has "))
        && (subject == name || Some(subject) == name.split_once(',').map(|(short, _)| short))
    {
        return Some((
            PermanentSelector {
                equipped_by_at_least: 2,
                ..own()
            },
            vec![StaticPart::Abilities(AbilityChange::grant_keywords([
                static_keyword(grant)?,
            ]))],
        ));
    }
    if line
        == "Other creatures you control get +1/+1 for each Aura and Equipment attached to this creature"
    {
        let n = NumericExpression::AttachedAurasAndEquipment {
            multiplier: 1.into(),
        };
        return Some((
            PermanentSelector {
                controller: ControllerSelector::SourceController,
                exclude_source: true,
                required_types: [CardType::Creature].into(),
                ..Default::default()
            },
            vec![StaticPart::ModifyPT(NumericPair {
                power: n.clone(),
                toughness: n,
            })],
        ));
    }
    if let Some(grant) = line.strip_prefix("Attacking creatures you control have ") {
        return Some((
            PermanentSelector {
                combat_role: Some(CombatSelector::Attacking),
                controller: ControllerSelector::SourceController,
                required_types: [CardType::Creature].into(),
                ..Default::default()
            },
            vec![StaticPart::Abilities(AbilityChange::grant_keywords([
                static_keyword(grant)?,
            ]))],
        ));
    }
    if let Some(granted) =
        line.strip_prefix("Each creature you control with a +1/+1 counter on it has ")
    {
        return Some((
            PermanentSelector {
                required_types: [CardType::Creature].into(),
                controller: ControllerSelector::SourceController,
                plus_one_counter: true,
                ..Default::default()
            },
            vec![StaticPart::Abilities(AbilityChange::grant_keywords([
                static_keyword(granted)?,
            ]))],
        ));
    }
    if let Some(granted) = line.strip_prefix("As long as this creature is equipped, it has ") {
        let keys = granted
            .split(" and ")
            .map(static_keyword)
            .collect::<Option<Vec<_>>>()?;
        return Some((
            PermanentSelector {
                controller: ControllerSelector::SourceItself,
                equipped_by_at_least: 1,
                ..Default::default()
            },
            vec![StaticPart::Abilities(AbilityChange::grant_keywords(keys))],
        ));
    }
    let self_modifier = match line {
        "This creature gets +2/+2 for each Aura and Equipment attached to it" => {
            Some(NumericExpression::AttachedAurasAndEquipment {
                multiplier: 2.into(),
            })
        }
        "This creature gets +10/+10 as long as you control ten or more lands" => {
            Some(NumericExpression::ConditionalBonus {
                condition:
                    spellwake_engine::characteristics::StaticBonusCondition::ControlledLandsAtLeast(
                        10,
                    ),
                bonus: 10.into(),
            })
        }
        _ => None,
    };
    if let Some(modifier) = self_modifier {
        return Some((
            PermanentSelector {
                controller: ControllerSelector::SourceItself,
                ..Default::default()
            },
            vec![StaticPart::ModifyPT(NumericPair {
                power: modifier.clone(),
                toughness: modifier,
            })],
        ));
    }
    let (subject, stats, keywords) = if let Some((subject, grant)) = line
        .split_once(" have ")
        .filter(|(_, grant)| grant.starts_with('"'))
    {
        (subject, None, Some(grant))
    } else if let Some((subject, grant)) = line.split_once(" get ") {
        let (stats, keywords) = grant
            .split_once(" and have ")
            .map_or((grant, None), |(s, k)| (s, Some(k)));
        (subject, Some(stats), keywords)
    } else {
        let (subject, keywords) = line.split_once(" have ")?;
        (subject, None, Some(keywords))
    };
    let mut selector = PermanentSelector::default();
    let subject = subject.strip_suffix(" with flying").map_or(subject, |s| {
        selector.requires_flying = true;
        s
    });
    let subject = subject.strip_prefix("Other ").map_or(subject, |s| {
        selector.exclude_source = true;
        s
    });
    let subject = subject.strip_prefix("All ").unwrap_or(subject);
    let subject = subject.strip_suffix(" you control").map_or(subject, |s| {
        selector.controller = ControllerSelector::SourceController;
        s
    });
    match subject {
        "permanents" | "Permanents" => {}
        subject if subject.starts_with("non-") && subject.ends_with(" creatures") => {
            let subtype = subject.strip_prefix("non-")?.strip_suffix(" creatures")?;
            selector.required_types.insert(CardType::Creature);
            selector.forbidden_creature_subtype = Some(vocabulary.resolve_label(subtype)?);
        }
        "Creatures" | "creatures" => {
            selector.required_types.insert(CardType::Creature);
        }
        "Creature tokens" | "creature tokens" => {
            selector.token = Some(true);
            selector.required_types.insert(CardType::Creature);
        }
        "enchantment creatures" | "Enchantment creatures" => {
            selector
                .required_types
                .extend([CardType::Enchantment, CardType::Creature]);
        }
        "Colorless creatures" | "colorless creatures" => {
            selector.required_types.insert(CardType::Creature);
            selector.color = ColorFilter::Colorless;
        }
        subtype => {
            let subtype = subtype
                .strip_suffix(" creatures")
                .inspect(|_| {
                    selector.required_types.insert(CardType::Creature);
                })
                .or_else(|| subtype.strip_suffix('s'))?;
            selector.creature_subtype = Some(vocabulary.resolve_label(subtype)?);
        }
    }
    let mut parts = Vec::new();
    if let Some(stats) = stats {
        let (power, toughness) = stats.split_once('/')?;
        let parse = |s: &str| {
            (s.starts_with('+') || s.starts_with('-'))
                .then(|| s.parse::<i32>().ok())
                .flatten()
        };
        parts.push(StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(i64::from(parse(power)?).into()),
            toughness: NumericExpression::Literal(i64::from(parse(toughness)?).into()),
        }));
    }
    if let Some(words) = keywords {
        if let Some(quoted) = words.strip_prefix('"').and_then(|w| w.strip_suffix('"')) {
            parts.push(granted_creature_ability(quoted, vocabulary)?);
        } else if matches!(words, "undying" | "persist") {
            use spellwake_engine::program::CounterKeyword;
            let keyword = if words == "undying" {
                CounterKeyword::Undying
            } else {
                CounterKeyword::Persist
            };
            parts.push(StaticPart::GrantTrigger(Box::new(
                keyword.ability(spellwake_engine::program::TriggeredAbilityId(0)),
            )));
        } else if let Some(ability) =
            super::advanced_combat::trigger(words, spellwake_engine::program::TriggeredAbilityId(0))
        {
            let mut change = AbilityChange::default();
            change
                .granted_combat_keywords
                .push(ability.as_combat_keyword()?.clone());
            parts.push(StaticPart::Abilities(change));
        } else {
            let keywords = words
                .split(" and ")
                .map(static_keyword)
                .collect::<Option<Vec<_>>>()?;
            parts.push(StaticPart::Abilities(AbilityChange::grant_keywords(
                keywords,
            )));
        }
    }
    Some((selector, parts))
}
