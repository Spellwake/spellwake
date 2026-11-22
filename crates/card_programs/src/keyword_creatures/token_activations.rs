//! Fixed-cost activated abilities that create a printed group of creature tokens.
use super::*;
use spellwake_engine::activated::SourceCost;

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let (prefix, ability) = text.trim().rsplit_once('\n').unwrap_or(("", text.trim()));
    let (cost, template) = ability.strip_suffix('.')?.split_once(": Create ")?;
    let (mana, source_cost) = cost
        .strip_suffix(", {T}")
        .map_or((cost, SourceCost::None), |mana| (mana, SourceCost::Tap));
    let mana_cost = fixed_cost(mana)?;
    let instruction = ordinary_text::create_tokens(template, vocabulary)?;
    let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some(prefix)).ok()?;
    let next = program
        .activated
        .iter()
        .map(|ability| ability.id.0)
        .max()
        .map_or(Some(0), |id| id.checked_add(1))?;
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(next),
        mana_cost,
        source_cost,
        body: ExecutableBody::new(BodyTarget::None, vec![instruction]),
    });
    Some(program)
}
