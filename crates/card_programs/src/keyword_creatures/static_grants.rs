//! Complete non-Aura enchantments that continuously grant keywords.
use super::{InventoryCard, fixed_cost, without_reminder};
use spellwake_engine::{
    characteristics::{
        AbilityChange, ControllerSelector, KeywordAbility, PermanentSelector, StaticAbilityId,
        StaticContinuousAbility, StaticPart, StaticParts,
    },
    program::{CardType, Program},
};
use std::collections::BTreeSet;

pub(super) fn compile(card: &InventoryCard) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal"
        || card.name != face.name
        || face.type_line.as_deref()? != "Enchantment"
    {
        return None;
    }
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let keyword = match text.trim() {
        "Creatures you control have first strike." => KeywordAbility::FirstStrike,
        _ => return None,
    };
    let mut program = Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?));
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            controller: ControllerSelector::SourceController,
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
            [keyword],
        ))]),
    });
    Some(program)
}

/// Compile a complete creature face whose sole rules text gives Sliver creatures
/// a fixed power and toughness modification.
pub(super) fn sliver_stats(
    card: &InventoryCard,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::characteristics::{NumericExpression, NumericPair};
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let (controller, pair) = if let Some(pair) = text.strip_prefix("All Sliver creatures get ") {
        (ControllerSelector::Any, pair)
    } else {
        (
            ControllerSelector::SourceController,
            text.strip_prefix("Sliver creatures you control get ")?,
        )
    };
    let (power, toughness) = pair.strip_suffix('.')?.split_once('/')?;
    let positive = |value: &str| super::number(Some(value.strip_prefix('+')?));
    let mut program =
        super::ordinary_creature(card, vocabulary, super::Profile::Current, Some("")).ok()?;
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            creature_subtype: Some(vocabulary.resolve_label("Sliver")?),
            controller,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(positive(power)?.into()),
            toughness: NumericExpression::Literal(positive(toughness)?.into()),
        })]),
    });
    Some(program)
}

/// Compile a complete creature face granting supported keywords to Sliver creatures.
pub(super) fn sliver_keywords(
    card: &InventoryCard,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Option<Program> {
    use KeywordAbility::{
        Deathtouch, DoubleStrike, FirstStrike, Flying, Haste, Indestructible, Lifelink, Menace,
        Shroud, Vigilance,
    };
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let (controller, creature_only, words) =
        if let Some(words) = text.strip_prefix("All Sliver creatures have ") {
            (ControllerSelector::Any, true, words)
        } else if text.trim() == "All Slivers have shroud." {
            (ControllerSelector::Any, false, "shroud.")
        } else {
            (
                ControllerSelector::SourceController,
                true,
                text.strip_prefix("Sliver creatures you control have ")?,
            )
        };
    let keywords: &[KeywordAbility] = match words.trim() {
        "flying." => &[Flying],
        "haste." => &[Haste],
        "vigilance." => &[Vigilance],
        "lifelink." => &[Lifelink],
        "deathtouch." => &[Deathtouch],
        "first strike." => &[FirstStrike],
        "double strike." => &[DoubleStrike],
        "indestructible." => &[Indestructible],
        "menace." => &[Menace],
        "shroud." => &[Shroud],
        "flying and haste." => &[Flying, Haste],
        _ => return None,
    };
    let mut program =
        super::ordinary_creature(card, vocabulary, super::Profile::Current, Some("")).ok()?;
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: if creature_only {
                BTreeSet::from([CardType::Creature])
            } else {
                BTreeSet::new()
            },
            creature_subtype: Some(vocabulary.resolve_label("Sliver")?),
            controller,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
            keywords.iter().copied(),
        ))]),
    });
    Some(program)
}

/// Compile complete creature faces with a supported static blocking restriction.
pub(super) fn blocking_restrictions(
    card: &InventoryCard,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::characteristics::BlockingRestriction;
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    if let Some((line, rest)) = text.split_once('\n')
        && let Some(minimum) = line
            .strip_prefix(&face.name)
            .and_then(|tail| tail.strip_prefix(" can't be blocked by creatures with toughness "))
            .and_then(|tail| tail.strip_suffix(" or greater."))
    {
        let minimum = minimum.parse::<u16>().ok()?;
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(rest.to_owned());
        let mut program = super::compile(&reduced, vocabulary, super::Profile::Current).ok()?;
        if program.continuous.is_some() {
            return None;
        }
        program.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(1),
            selector: PermanentSelector {
                controller: ControllerSelector::SourceItself,
                ..PermanentSelector::default()
            },
            parts: StaticParts(vec![StaticPart::BlockingRestriction(
                BlockingRestriction::CannotBeBlockedByToughnessAtLeast(minimum),
            )]),
        });
        program.validate(vocabulary).ok()?;
        return Some(program);
    }
    let (selector, restriction) = match text.trim() {
        "Slivers can't be blocked except by Slivers." => {
            let sliver = vocabulary.resolve_label("Sliver")?;
            (
                PermanentSelector {
                    creature_subtype: Some(sliver),
                    ..PermanentSelector::default()
                },
                BlockingRestriction::OnlyBlockedBySubtype(sliver),
            )
        }
        "Creatures you control with power or toughness 1 or less can't be blocked." => (
            PermanentSelector {
                required_types: BTreeSet::from([CardType::Creature]),
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            },
            BlockingRestriction::CannotBeBlockedIfPowerOrToughnessAtMostOne,
        ),
        _ => return None,
    };
    let mut program =
        super::ordinary_creature(card, vocabulary, super::Profile::Current, Some("")).ok()?;
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector,
        parts: StaticParts(vec![StaticPart::BlockingRestriction(restriction)]),
    });
    Some(program)
}

/// Resolution captures a target incarnation and expires at the cleanup step.
pub(super) fn unblockable_effect() -> spellwake_engine::program::Instruction {
    use spellwake_engine::characteristics::{
        BlockingRestriction, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    };
    spellwake_engine::program::Instruction::CreateContinuousEffect {
        duration: ResolvedDuration::UntilEndOfTurn,
        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
            BlockingRestriction::CannotBeBlocked,
        )]),
    }
}

/// Compile a creature face whose only activation makes a target unblockable.
pub(super) fn unblockable_creature(
    card: &InventoryCard,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::activated::{ActivatedAbility, ActivatedAbilityId, SourceCost};
    use spellwake_engine::program::{BodyTarget, ExecutableBody};
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let (other, activation) = text.trim().rsplit_once('\n')?;
    let cost = activation.strip_suffix(", {T}: Target creature can't be blocked this turn.")?;
    let mut program =
        super::ordinary_creature(card, vocabulary, super::Profile::Current, Some(other)).ok()?;
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(program.activated.len().try_into().ok()?),
        mana_cost: fixed_cost(cost)?,
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(BodyTarget::SingleCreature, vec![unblockable_effect()]),
    });
    Some(program)
}

/// Compile Hollowhead Sliver's exact controlled-Sliver activated ability grant.
pub(super) fn sliver_selected_activation(
    card: &InventoryCard,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::{
        activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
        mana::ManaCost,
        program::{BodyTarget, ExecutableBody, Instruction},
    };
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if without_reminder(face.oracle_text.as_deref()?)?.trim()
        != "Sliver creatures you control have \"{T}, Discard a card: Draw a card.\""
    {
        return None;
    }
    let mut program =
        super::ordinary_creature(card, vocabulary, super::Profile::Current, Some("")).ok()?;
    let ability = ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Discard {
            mode: spellwake_engine::activated::DiscardCostMode::SelectedOne,
            tap_source: true,
            sacrifice_source: false,
        },
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
            }],
        ),
    };
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            creature_subtype: Some(vocabulary.resolve_label("Sliver")?),
            controller: ControllerSelector::SourceController,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::GrantActivated(Box::new(ability))]),
    });
    Some(program)
}

/// Compile the complete reduction and once-per-turn graveyard creature permission.
pub(super) fn graveyard_creature_cast(
    card: &InventoryCard,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let (reduction, permission) = text.trim().split_once('\n')?;
    if reduction != "This spell costs {1} less to cast for each creature card in your graveyard."
        || permission
            != "Once during each of your turns, you may cast a creature spell from your graveyard."
    {
        return None;
    }
    let mut program =
        super::ordinary_creature(card, vocabulary, super::Profile::Current, Some(reduction))
            .ok()?;
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(1),
            permission:
                spellwake_engine::characteristics::CastPermissionRule::GraveyardCreatureCast,
        },
    );
    Some(program)
}
