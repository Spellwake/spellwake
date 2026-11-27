//! Printed artifact tap-and-sacrifice abilities with a single target.
use super::*;
use spellwake_engine::activated::SourceCost;

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Artifact")
    {
        return None;
    }
    let text = face.oracle_text.as_deref()?;
    if let Some(cost) = text.strip_suffix(": This artifact becomes a copy of target artifact, creature, enchantment, or land until end of turn.") {
        let mut program = Program::artifact(Some(fixed_cost(face.mana_cost.as_deref()?)?),vec![]);
        program.activated.push(ActivatedAbility {id:ActivatedAbilityId(0),mana_cost:fixed_cost(cost)?,source_cost:SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::FilteredPermanent(spellwake_engine::program::ObjectFilter::any_types(
                    spellwake_engine::program::CardTypeMask::ARTIFACT
                        .union(spellwake_engine::program::CardTypeMask::CREATURE)
                        .union(spellwake_engine::program::CardTypeMask::ENCHANTMENT)
                        .union(spellwake_engine::program::CardTypeMask::LAND),
                )),
                vec![Instruction::CopyTargetToSource {duration:spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,power_toughness:None}])});
        return Some(program);
    }
    let (other, ability) = text.rsplit_once('\n').unwrap_or(("", text));
    let (cost, effect) = ability.split_once(", {T}, Sacrifice this artifact: ")?;
    let (target, op) = match effect {
        "Destroy target permanent." => (
            BodyTarget::SinglePermanent,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                        semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                            allow_regenerate: true,
                        },
                    },
                ),
            ),
        ),
        "It deals 2 damage to target creature." => (
            BodyTarget::SingleCreature,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                spellwake_engine::toolbox::DamageTargetQuery::Targets,
                spellwake_engine::program::ValueExpr::literal(2),
                false,
            ),
        ),
        _ => return None,
    };
    let mut program = if other.is_empty() {
        Program::artifact(Some(fixed_cost(face.mana_cost.as_deref()?)?), vec![])
    } else {
        let mut remaining = card.clone();
        remaining.faces[0].oracle_text = Some(other.to_owned());
        mana_permanents::compile(&remaining, vocabulary)?
    };
    if !program.activated.is_empty() {
        return None;
    }
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: fixed_cost(cost)?,
        source_cost: SourceCost::TapSacrifice,
        body: ExecutableBody::new(target, vec![op]),
    });
    Some(program)
}
