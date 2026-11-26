//! Closed fixed-mana Power-up clauses on otherwise admitted creature faces.
use super::*;
use spellwake_engine::activated::{ActivationRestriction, SourceCost};
pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut abilities = vec![];
    let mut remaining = vec![];
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Some(printed) = line.strip_prefix("Power-up — ") else {
            remaining.push(line);
            continue;
        };
        let (cost, effect) = printed.split_once(": ")?;
        let (counters, draw) = match effect {
            "Put a +1/+1 counter on this creature." => (1, 0),
            "Put two +1/+1 counters on this creature." => (2, 0),
            "Put three +1/+1 counters on this creature." => (3, 0),
            "Put a +1/+1 counter on this creature and draw two cards." => (1, 2),
            _ => return None,
        };
        let mut instructions = vec![
            crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                spellwake_engine::effects::CounterKind::PlusOne,
                spellwake_engine::program::ValueExpr::Literal(counters.into()),
            ),
        ];
        if draw > 0 {
            instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(draw.into()),
                ),
            );
        }
        abilities.push((
            fixed_cost(cost)?,
            ExecutableBody::new(BodyTarget::None, instructions),
        ));
    }
    if abilities.is_empty() {
        return None;
    }
    let mut program = ordinary_creature(
        card,
        vocabulary,
        Profile::Current,
        Some(&remaining.join("\n")),
    )
    .ok()?;
    let mut next_id = program.activated.iter().map(|a| a.id.0).max().unwrap_or(0);
    for (mana_cost, body) in abilities {
        next_id = next_id.checked_add(1)?;
        let id = ActivatedAbilityId(next_id);
        program.activated.push(ActivatedAbility {
            id,
            mana_cost,
            body,
            source_cost: SourceCost::None,
        });
        program
            .activation_restrictions
            .insert(id, ActivationRestriction::PowerUp);
    }
    Some(program)
}
