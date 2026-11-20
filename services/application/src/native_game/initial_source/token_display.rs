//! Descriptions of exact vanilla creation templates in the frozen guarded source.
//! No external token catalogue, guessed Oracle text or token printing is used.
use super::*;
use crate::player_wire::PlayerCharacteristics;
use spellwake_engine::{
    characteristics::PrintedStat,
    object_identity::{CardId, ObjectDestination},
    program::{CardType, Instruction, Program, ProgramKind},
};

impl NativeProgramSource {
    pub(super) fn token_description(
        &self,
        definition: &Program,
    ) -> Result<Option<PlayerCharacteristics>, NativeInitialError> {
        // Match an actual materialized template before rendering its properties.
        // Arbitrary vanilla Programs do not become source-backed token records.
        for oracle in self.catalogue.keys() {
            let program = self
                .registry
                .seed(
                    oracle,
                    CardId(1),
                    SeatId(0),
                    ObjectDestination::LibraryBottom,
                )?
                .program;
            let mut bodies = Vec::new();
            match &program.kind {
                ProgramKind::Spell(spell) => bodies.push(&spell.body),
                ProgramKind::Permanent(_) => {}
            }
            bodies.extend(program.activated.iter().map(|ability| &ability.body));
            bodies.extend(program.triggered.iter().map(|ability| &ability.body));
            for instruction in bodies.iter().flat_map(|body| &body.instructions) {
                if let Instruction::CreateTokens { creations } = instruction {
                    for creation in creations {
                        let materialized = creation
                            .definition
                            .materialize(&self.vocabulary)
                            .map_err(RegistryError::TokenDefinition)?;
                        if &materialized == definition {
                            return vanilla_description(definition, &self.vocabulary).map(Some);
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}
fn vanilla_description(
    program: &Program,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PlayerCharacteristics, NativeInitialError> {
    let ProgramKind::Permanent(permanent) = &program.kind else {
        return Err(NativeInitialError::Invalid);
    };
    // The finite guarded pools currently create vanilla creature templates only.
    // New capability-bearing templates need their own immutable text source.
    if program.card_types != BTreeSet::from([CardType::Creature])
        || !program.supertypes.is_empty()
        || !program.land_subtypes.is_empty()
        || !program.zone_replacements.is_empty()
        || !program.mana.is_empty()
        || !program.activated.is_empty()
        || !program.triggered.is_empty()
        || program.continuous.is_some()
        || permanent.casting_cost.is_some()
        || program.names.len() != 1
    {
        return Err(NativeInitialError::Invalid);
    }
    let stats = permanent
        .printed_pt
        .as_ref()
        .ok_or(NativeInitialError::Invalid)?;
    let (PrintedStat::Number(power), PrintedStat::Number(toughness)) =
        (&stats.power, &stats.toughness)
    else {
        return Err(NativeInitialError::Invalid);
    };
    let subtypes = program
        .creature_subtypes
        .iter()
        .map(|id| vocabulary.label(*id).ok_or(NativeInitialError::Invalid))
        .collect::<Result<Vec<_>, _>>()?;
    let result = PlayerCharacteristics {
        name: program
            .names
            .iter()
            .next()
            .ok_or(NativeInitialError::Invalid)?
            .as_str()
            .into(),
        type_line: if subtypes.is_empty() {
            "Token Creature".into()
        } else {
            format!("Token Creature — {}", subtypes.join(" "))
        },
        rules_text: String::new(),
        mana_cost: None,
        printed_power: Some(String::from(power.clone())),
        printed_toughness: Some(String::from(toughness.clone())),
        printed_loyalty: None,
    };
    if !result.valid() {
        return Err(NativeInitialError::Invalid);
    }
    Ok(result)
}
