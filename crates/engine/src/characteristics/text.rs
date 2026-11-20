//! Semantic creature-type substitutions in printed definitions, never raw text.
use super::types::*;
use crate::mana::ManaOutput;
use crate::program::{ExecutableBody, Instruction};
use spellwake_contracts::creature_types::CreatureTypeId;
use std::collections::BTreeSet;

fn word(change: CreatureTypeTextChange, value: &mut CreatureTypeId) {
    if *value == change.from {
        *value = change.to;
    }
}
fn words(change: CreatureTypeTextChange, values: &mut BTreeSet<CreatureTypeId>) {
    if values.remove(&change.from) {
        values.insert(change.to);
    }
}
fn operand(change: CreatureTypeTextChange, value: &mut CreatureTypeWord) {
    match value {
        CreatureTypeWord::Literal(value) => word(change, value),
        // A slot denotes a runtime value, not a printed type word.
        CreatureTypeWord::Chosen(_) => {}
    }
}
fn subtypes<T>(
    change: CreatureTypeTextChange,
    value: &mut SubtypeOperation<T>,
    rewrite: impl Fn(CreatureTypeTextChange, &mut T),
) {
    match value {
        SubtypeOperation::Keep => {}
        SubtypeOperation::Remove(values) => words(change, values),
        SubtypeOperation::Add(value) | SubtypeOperation::Set(value) => rewrite(change, value),
    }
}
fn instruction_subtype(change: CreatureTypeTextChange, value: &mut InstructionSubtypeOperand) {
    match value {
        InstructionSubtypeOperand::Literal(values) => words(change, values),
        InstructionSubtypeOperand::Chosen(_) => {}
    }
}
fn static_subtype(change: CreatureTypeTextChange, value: &mut StaticSubtypeOperand) {
    match value {
        StaticSubtypeOperand::Literal(values) => words(change, values),
        StaticSubtypeOperand::SourceCurrentSubtypes => {}
    }
}
fn selector(change: CreatureTypeTextChange, value: &mut PermanentSelector) {
    if let Some(value) = &mut value.creature_subtype {
        word(change, value);
    }
}
fn numeric(change: CreatureTypeTextChange, value: &mut NumericExpression) {
    match value {
        NumericExpression::Literal(_) => {}
        NumericExpression::Count {
            selector: value, ..
        } => selector(change, value),
    }
}
fn parts(change: CreatureTypeTextChange, value: &mut ResolvedPartsSpec) {
    for part in &mut value.0 {
        match part {
            ResolvedPartSpec::Text(value) => {
                operand(change, &mut value.from);
                operand(change, &mut value.to);
            }
            ResolvedPartSpec::Type(value) => {
                subtypes(change, &mut value.subtypes, instruction_subtype);
            }
            ResolvedPartSpec::Names(_)
            | ResolvedPartSpec::Control(_)
            | ResolvedPartSpec::Color(_)
            | ResolvedPartSpec::Abilities(_)
            | ResolvedPartSpec::SetPT(_)
            | ResolvedPartSpec::ModifyPT(_)
            | ResolvedPartSpec::SwitchPT => {}
        }
    }
}
fn body(change: CreatureTypeTextChange, value: &mut ExecutableBody) {
    for instruction in &mut value.instructions {
        match instruction {
            Instruction::CreateTokens { creations } => {
                for creation in creations {
                    creation.definition.rewrite_creature_types(change);
                }
            }
            Instruction::ChooseCreatureType { excluding, .. } => words(change, excluding),
            Instruction::CreateContinuousEffect { parts: value, .. } => parts(change, value),
            // These admitted instructions contain no literal creature-type words.
            // Draw's chosen-type reference remains a runtime local value.
            Instruction::OptionalEffect { .. }
            | Instruction::OfferManaPayment { .. }
            | Instruction::Sacrifice { .. }
            | Instruction::Discard { .. }
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::AdjustControllerLife { .. }
            | Instruction::AdjustTargetLife { .. }
            | Instruction::Damage { .. }
            | Instruction::CreatePreventionShield { .. }
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::ReturnToOwnersHand
            | Instruction::CounterSpell
            | Instruction::CopySpell
            | Instruction::CopyTargetToSource { .. }
            | Instruction::PutPowerToughnessCounters { .. }
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. }
            | Instruction::DrawController { .. } => {}
        }
    }
}

pub(super) fn apply(change: CreatureTypeTextChange, object: &mut ObjectCharacteristics) {
    words(change, &mut object.current_creature_subtypes);
    printed(change, &mut object.printed_text);
}
/// Apply the same semantic text transformation to a literal creation template.
pub(crate) fn rewrite_program(
    change: CreatureTypeTextChange,
    program: &mut crate::program::Program,
) {
    words(change, &mut program.creature_subtypes);
    let mut text = PrintedText::from_program(program);
    printed(change, &mut text);
    program.zone_replacements = text.zone_replacements;
    program.mana = text.mana;
    program.activated = text.activated;
    program.triggered = text.triggered;
    program.continuous = text.continuous;
    if let crate::program::ProgramKind::Spell(value) = &mut program.kind {
        body(change, &mut value.body);
    }
}
fn printed(change: CreatureTypeTextChange, text: &mut PrintedText) {
    for ability in &mut text.mana {
        match &mut ability.output {
            ManaOutput::Counted { permanents, .. } => selector(change, permanents),
            ManaOutput::Fixed(_) | ManaOutput::ChooseColor { .. } => {}
        }
    }
    for ability in &mut text.activated {
        body(change, &mut ability.body);
    }
    for ability in &mut text.triggered {
        body(change, &mut ability.body);
    }
    if let Some(StaticContinuousAbility::Battlefield {
        selector: target,
        parts,
        ..
    }) = &mut text.continuous
    {
        selector(change, target);
        for part in &mut parts.0 {
            match part {
                StaticPart::Text(value) => {
                    word(change, &mut value.from);
                    word(change, &mut value.to);
                }
                StaticPart::Type(value) => subtypes(change, &mut value.subtypes, static_subtype),
                StaticPart::SetPT(value) | StaticPart::ModifyPT(value) => {
                    numeric(change, &mut value.power);
                    numeric(change, &mut value.toughness);
                }
                StaticPart::Names(_)
                | StaticPart::ControlSourceController
                | StaticPart::Color(_)
                | StaticPart::Abilities(_)
                | StaticPart::SwitchPT => {}
            }
        }
    }
    if let Some(StaticContinuousAbility::CharacteristicDefining { parts, .. }) =
        &mut text.continuous
    {
        for part in &mut parts.0 {
            match part {
                DefiningPart::CreatureTypes(DefiningCreatureTypes::Literal(values)) => {
                    words(change, values)
                }
                DefiningPart::PowerToughness { power, toughness } => {
                    for expression in power.iter_mut().chain(toughness) {
                        numeric(change, expression);
                    }
                }
                DefiningPart::CreatureTypes(DefiningCreatureTypes::All)
                | DefiningPart::Colors(_) => {}
            }
        }
    }
}
