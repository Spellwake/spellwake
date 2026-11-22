//! Source-bound attack triggers for decayed Zombies and mobilize Warriors.
use super::*;
use spellwake_engine::{
    program::{CombatTriggerKeyword, TriggerGrouping},
    token::TokenCreation,
};

pub(super) fn parse(label: &str) -> Option<CombatTriggerKeyword> {
    match label {
        "decayed" => Some(CombatTriggerKeyword::Decayed),
        _ => Some(CombatTriggerKeyword::Mobilize(
            number(Some(label.strip_prefix("mobilize ")?))?.into(),
        )),
    }
}

pub(super) fn zombie_creator(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut conditions = vec![];
    let mut remaining = vec![];
    for line in text.lines().map(str::trim) {
        match line {
            "When this creature enters, create a 2/2 black Zombie creature token with decayed." => {
                conditions.push(spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ))
            }
            "When this creature dies, create a 2/2 black Zombie creature token with decayed." => {
                conditions.push(spellwake_engine::program::TriggerSpec::new(
                    spellwake_engine::program::TriggerEvent::Dies {
                        subject: spellwake_engine::program::ObjectFilter::self_object(),
                        mode: spellwake_engine::program::DiesTriggerMode::Permanent,
                    },
                ))
            }
            other => remaining.push(other),
        }
    }
    if conditions.is_empty() {
        return None;
    }
    let mut program = ordinary_creature(
        card,
        vocabulary,
        Profile::Current,
        Some(&remaining.join("\n")),
    )
    .ok()?;
    let definition = spellwake_engine::program::decayed_zombie(vocabulary).ok()?;
    for condition in conditions {
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(u64::try_from(program.triggered.len()).ok()?),
            condition,
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation::one(definition.clone())],
                }],
            ),
        });
    }
    Some(program)
}

#[cfg(test)]
mod tests;
