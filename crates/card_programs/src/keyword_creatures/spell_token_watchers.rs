//! Whole permanents that create literal tokens from completed casts.
use super::*;
use spellwake_engine::program::{BodyTarget, ExecutableBody, PlayerRelation, TriggerGrouping};

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
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut trigger = None;
    let mut remaining = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let matched = [
            (
                "Whenever you cast an instant or sorcery spell, create ",
                PlayerRelation::Controller,
                spellwake_engine::program::SpellFilter::instant_or_sorcery(),
            ),
            (
                "Whenever an opponent casts an instant or sorcery spell, create ",
                PlayerRelation::Opponent,
                spellwake_engine::program::SpellFilter::instant_or_sorcery(),
            ),
            (
                "Whenever you cast an enchantment spell, create ",
                PlayerRelation::Controller,
                spellwake_engine::program::SpellFilter::of_type(CardType::Enchantment),
            ),
            (
                "Whenever you cast an artifact spell, create ",
                PlayerRelation::Controller,
                spellwake_engine::program::SpellFilter::of_type(CardType::Artifact),
            ),
            (
                "Whenever you cast a noncreature spell, create ",
                PlayerRelation::Controller,
                spellwake_engine::program::SpellFilter::noncreature(),
            ),
        ]
        .into_iter()
        .find_map(|(prefix, player, filter)| {
            line.strip_prefix(prefix).map(|rest| (player, filter, rest))
        });
        if let Some((player, filter, token)) = matched {
            if trigger.is_some() {
                return None;
            }
            let creation =
                super::ordinary_text::create_tokens(token.strip_suffix('.')?, vocabulary)?;
            trigger = Some((player, filter, creation));
        } else {
            remaining.push(line);
        }
    }
    let (player, filter, creation) = trigger?;
    let mut program = if face.type_line.as_deref()? == "Enchantment" && remaining.is_empty() {
        Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?))
    } else {
        ordinary_creature(
            card,
            vocabulary,
            Profile::Current,
            Some(&remaining.join("\n")),
        )
        .ok()?
    };
    let id = program
        .triggered
        .iter()
        .map(|trigger| trigger.id.0)
        .max()
        .map_or(Some(0), |id| id.checked_add(1))?;
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(id),
        condition: spellwake_engine::program::TriggerSpec::spell_cast(player, filter),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![creation]),
    });
    Some(program)
}
