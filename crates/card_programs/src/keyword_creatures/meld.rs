//! Source-backed meld sets. Both components and the joined face are independent Oracle rows.
use super::*;
use spellwake_engine::program::{
    BodyTarget, ExecutableBody, Instruction, ProgramKind, TriggerGrouping,
};
use spellwake_engine::turn::Step;

pub(super) fn compile(
    card: &InventoryCard,
    inventory: &Inventory,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let family = inventory.meld_set(card.oracle_id.as_deref()?)?;
    let [first, second] = &family.parts;
    let partner = if &card.name == first {
        second
    } else if &card.name == second {
        first
    } else {
        return None;
    };
    let other = inventory
        .cards
        .iter()
        .find(|row| row.layout == "meld" && &row.name == partner)?;
    let result = inventory
        .cards
        .iter()
        .find(|row| row.layout == "meld" && row.name == family.result)?;
    if [other, result]
        .iter()
        .any(|r| r.oracle_id.as_deref().and_then(|id| inventory.meld_set(id)) != Some(family))
    {
        return None;
    }
    let normalize = |row: &InventoryCard| {
        let [face] = row.faces.as_slice() else {
            return None;
        };
        let text = without_reminder(face.oracle_text.as_deref()?)?;
        let mut single = row.clone();
        single.layout = "normal".into();
        single.faces[0].oracle_text = Some(text.trim().to_owned());
        Some(single)
    };
    let mut joined = normalize(result)?;
    if !joined.faces[0]
        .mana_cost
        .as_deref()
        .unwrap_or("")
        .is_empty()
    {
        return None;
    }
    joined.faces[0].mana_cost = Some("{0}".into());
    let mut back = super::compile(&joined, vocabulary, Profile::Current).ok()?;
    let ProgramKind::Permanent(p) = &mut back.kind else {
        return None;
    };
    p.casting_cost = None;
    back.names = ObjectNames::single(EnglishCardName::new(result.name.clone()).ok()?);
    let primary_sentence = |mate: &str| {
        format!(
            "At the beginning of combat on your turn, if you both own and control this creature and a creature named {mate}, exile them, then meld them into {}.",
            result.name
        )
    };
    let mut front = normalize(card)?;
    let other = normalize(other)?;
    let mut remaining = Vec::new();
    let mut trigger = false;
    for line in front.faces[0].oracle_text.as_deref()?.lines() {
        if line.trim() == primary_sentence(partner) {
            if trigger {
                return None;
            }
            trigger = true;
        } else {
            remaining.push(line);
        }
    }
    if !trigger
        && !other.faces[0]
            .oracle_text
            .as_deref()?
            .lines()
            .any(|line| line.trim() == primary_sentence(&card.name))
    {
        return None;
    }
    front.faces[0].oracle_text = Some(remaining.join("\n"));
    let mut program = super::compile(&front, vocabulary, Profile::Current).ok()?;
    let mate = EnglishCardName::new(partner.clone()).ok()?;
    program.meld_partner = Some(mate.clone());
    if trigger {
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(u64::try_from(program.triggered.len()).ok()?),
            condition: spellwake_engine::program::TriggerSpec::step_began(
                Step::BeginningCombat,
                spellwake_engine::program::PlayerRelation::Controller,
            )
            .only_if(spellwake_engine::program::TriggerCondition::ControlsMeldPair),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::ChooseMeldPartner(mate),
                    Instruction::ExileMeldPair,
                    Instruction::MeldExiledPair(Box::new(back)),
                ],
            ),
        });
    }
    Some(program)
}
