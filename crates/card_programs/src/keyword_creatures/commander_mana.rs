//! Complete simple permanents with commander-identity tap mana.
use super::*;
use spellwake_engine::{
    activated::SourceCost,
    mana::{ManaAbility, ManaAbilityId, ManaOutput},
};

pub(super) fn compile(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current || card.layout != "normal" || face.name != card.name {
        return None;
    }
    let text = face.oracle_text.as_deref()?;
    if face.type_line.as_deref() == Some("Land")
        && face.mana_cost.as_deref().is_none_or(str::is_empty)
        && text.contains("color identity. ")
    {
        return land_with_spend_effect(&without_reminder(text)?);
    }
    let mut lines = text.lines();
    if lines.next()? != "{T}: Add one mana of any color in your commander's color identity." {
        return None;
    }
    let ability = ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::SpecialColor(
            spellwake_engine::mana::SpecialColorYield::CommanderColorIdentity,
        ),
    };
    match face.type_line.as_deref()? {
        "Land" if face.mana_cost.as_deref().is_none_or(str::is_empty) && lines.next().is_none() => {
            Some(Program::land(BTreeSet::new(), vec![ability]))
        }
        "Artifact" => {
            let mut program =
                Program::artifact(Some(fixed_cost(face.mana_cost.as_deref()?)?), vec![]);
            program.mana.push(ability);
            if let Some(line) = lines.next() {
                if line != "Sacrifice this artifact: Draw a card." || lines.next().is_some() {
                    return None;
                }
                program.activated.push(ActivatedAbility {
                    id: ActivatedAbilityId(0),
                    mana_cost: ManaCost::free(),
                    source_cost: SourceCost::Sacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![crate::keyword_creatures::toolbox_emit::draw_controller(
                            spellwake_engine::program::ValueExpr::Literal(1.into()),
                        )],
                    ),
                });
            }
            Some(program)
        }
        _ => None,
    }
}

fn land_with_spend_effect(text: &str) -> Option<Program> {
    use spellwake_engine::{
        land::EntryTap,
        mana::{ManaSpendEffect, ManaType},
        program::ProgramKind,
    };

    let mut lines = text.trim().lines().map(str::trim).peekable();
    let tapped = lines.next_if_eq(&"This land enters tapped.").is_some();
    let mut mana = Vec::new();
    if lines.next_if_eq(&"{T}: Add {C}.").is_some() {
        mana.push(ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::Fixed([(ManaType::Colourless, 1.into())].into()),
        });
    }
    let (price, clause) = lines
        .next()?
        .split_once(": Add one mana of any color in your commander's color identity. ")?;
    let cost = if price == "{T}" {
        ManaCost::free()
    } else {
        fixed_cost(price.strip_suffix(", {T}")?)?
    };
    let effect = match clause {
        "If you spend this mana to cast your commander, it enters with a number of additional +1/+1 counters on it equal to the number of times it's been cast from the command zone this game." => {
            ManaSpendEffect::CommanderEntryCounters
        }
        "When that mana is spent to cast a creature spell that shares a creature type with your commander, scry 1." => {
            ManaSpendEffect::SharedCommanderTypeScry
        }
        _ => return None,
    };
    if lines.next().is_some() {
        return None;
    }
    mana.push(ManaAbility {
        id: ManaAbilityId(u64::try_from(mana.len()).ok()?),
        output: ManaOutput::SpecialColor(
            spellwake_engine::mana::SpecialColorYield::CommanderColorIdentity,
        )
        .spend_effect(effect)
        .paid(cost),
    });
    let mut program = Program::land(BTreeSet::new(), mana);
    if let ProgramKind::Permanent(def) = &mut program.kind
        && tapped
    {
        def.entry_tap = Some(EntryTap::Always);
    }
    Some(program)
}
