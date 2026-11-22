//! Battlefield permissions to cast a printed class of spells with instant timing.
use super::*;
use spellwake_engine::characteristics::StaticAbilityId;

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
    let (prefix, clause) = text.trim().rsplit_once('\n').unwrap_or(("", text.trim()));
    let scope = clause.trim().strip_suffix(" as though they had flash.")?;
    let (any_player, spells) = if let Some(spells) = scope.strip_prefix("Any player may cast ") {
        (true, spells)
    } else {
        (false, scope.strip_prefix("You may cast ")?)
    };
    let matching = match spells {
        "spells" => spellwake_engine::program::SpellFilter::any(),
        "artifact spells" => spellwake_engine::program::SpellFilter::of_type(CardType::Artifact),
        "creature spells" => spellwake_engine::program::SpellFilter::of_type(CardType::Creature),
        "enchantment spells" => {
            spellwake_engine::program::SpellFilter::of_type(CardType::Enchantment)
        }
        "sorcery spells" => spellwake_engine::program::SpellFilter::of_type(CardType::Sorcery),
        "colorless spells" => spellwake_engine::program::SpellFilter::colorless(),
        _ => spellwake_engine::program::SpellFilter::of_subtype(
            vocabulary.resolve_label(spells.strip_suffix(" spells")?)?,
        ),
    };
    let mut program = match face.type_line.as_deref()? {
        "Artifact" if prefix.is_empty() => {
            Program::artifact(Some(fixed_cost(face.mana_cost.as_deref()?)?), vec![])
        }
        "Enchantment" if prefix.is_empty() => {
            Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?))
        }
        _ => ordinary_creature(card, vocabulary, Profile::Current, Some(prefix)).ok()?,
    };
    if program.continuous.is_some() {
        return None;
    }
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(1),
            permission: spellwake_engine::characteristics::CastPermissionRule::Flash {
                any_player,
                matching,
            },
        },
    );
    Some(program)
}
